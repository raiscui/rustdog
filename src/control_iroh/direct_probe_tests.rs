use super::*;
use std::future::pending;
use tokio::sync::oneshot;

struct Fixture {
    daemon: Endpoint,
    client: Endpoint,
}

impl Fixture {
    async fn new(reverse: bool) -> Self {
        let bind = Some("127.0.0.1:0".parse().unwrap());
        let client_alpn = if reverse {
            DIRECT_PROBE_RETURN_ALPN
        } else {
            CONTROL_ALPN
        };
        Self {
            daemon: endpoint_on(
                SecretKey::generate(),
                bind,
                &[DIRECT_PROBE_ALPN, CONTROL_ALPN],
                None,
                false,
            )
            .await
            .unwrap(),
            client: endpoint(SecretKey::generate(), bind, client_alpn, None)
                .await
                .unwrap(),
        }
    }

    async fn lane(&self, alpn: &[u8]) -> (Connection, Connection) {
        timeout(IO, async {
            tokio::join!(
                async { self.client.connect(self.daemon.addr(), alpn).await.unwrap() },
                async { self.daemon.accept().await.unwrap().await.unwrap() },
            )
        })
        .await
        .unwrap()
    }

    fn gate(&self, admission: Option<admission::Admission>) -> Arc<DirectProbeGate> {
        Arc::new(DirectProbeGate {
            allow: Arc::new(vec![self.client.id()]),
            admission: admission.map(|state| Arc::new(Mutex::new(state))),
            expires: Arc::new(Mutex::new(Instant::now() + LEASE)),
            slots: Semaphore::new(MAX_DIRECT_PROBES),
        })
    }

    async fn close(self) {
        tokio::join!(self.client.close(), self.daemon.close());
    }
}

async fn close_is(conn: &Connection, code: u32, reason: &str) {
    match timeout(IO, conn.closed()).await.unwrap() {
        ConnectionError::ApplicationClosed(close) => {
            assert_eq!(close.error_code.into_inner(), u64::from(code));
            assert_eq!(close.reason.as_ref(), reason.as_bytes());
        }
        err => panic!("expected application close, got {err}"),
    }
}

async fn request(conn: &Connection) -> RecvStream {
    let (mut send, read) = timeout(IO, conn.open_bi()).await.unwrap().unwrap();
    send.write_all(b"@ping#718\n").await.unwrap();
    send.finish().unwrap();
    read
}

async fn pong(conn: &Connection) {
    let mut read = request(conn).await;
    let response = timeout(IO, read.read_to_end(8192)).await.unwrap().unwrap();
    let response = std::str::from_utf8(&response).unwrap();
    let body: serde_json::Value =
        serde_json::from_str(response.strip_prefix("@response ").unwrap()).unwrap();
    assert_eq!(body["id"], 718);
    assert_eq!(body["value"]["pong"], true);
}

struct PolicyFixture(PathBuf);

impl PolicyFixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("rdog-iroh-probe-{}.json", uuid::Uuid::new_v4()));
        Self(path)
    }

    fn write(&self, policy: &admission::Policy) {
        std::fs::write(&self.0, serde_json::to_vec(policy).unwrap()).unwrap();
    }
}

impl Drop for PolicyFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[tokio::test]
async fn probe_policy_denials_close_before_any_reverse_dial() {
    let fixture = Fixture::new(true).await;
    let file = PolicyFixture::new();
    let peer = fixture.client.id();
    for case in [
        "unlisted",
        "revoked",
        "emergency",
        "unreadable",
        "uncertain",
        "expired",
    ] {
        let mut policy = admission::Policy::default();
        if case != "unlisted" {
            policy.allow.insert(peer);
        }
        match case {
            "revoked" => {
                policy.revoked.insert(peer);
            }
            "emergency" => {
                policy.terminate.insert(peer);
            }
            _ => {}
        }
        file.write(&policy);
        let mut admission = admission::Admission::new(file.0.clone(), fixture.daemon.id());
        if case == "unreadable" {
            std::fs::remove_file(&file.0).unwrap();
        }
        if case == "uncertain" {
            assert!(admission
                .revoke_with(peer, admission::Revocation::Graceful, utc_ms, |_, _| {
                    Err(admission::PersistenceError::DurabilityUnconfirmed(
                        io::Error::other("fixture"),
                    ))
                })
                .is_err());
        }
        let gate = fixture.gate(Some(admission));
        if case == "expired" {
            *gate.expires.lock().unwrap() = Instant::now();
        }
        let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
        serve_direct_probe(&server, &gate, false, IO, async {
            panic!("{case}: denied identity started a reverse dial");
        })
        .await
        .unwrap();
        close_is(&client, DENIED_CODE, DENIED).await;
        assert_eq!(gate.slots.available_permits(), MAX_DIRECT_PROBES);
    }
    fixture.close().await;
}

#[tokio::test]
async fn revocation_between_reverse_dial_and_ping_line_denies_the_reply() {
    let fixture = Fixture::new(true).await;
    let file = PolicyFixture::new();
    let mut policy = admission::Policy::default();
    policy.allow.insert(fixture.client.id());
    file.write(&policy);
    let gate = fixture.gate(Some(admission::Admission::new(
        file.0.clone(),
        fixture.daemon.id(),
    )));
    let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
    let (started, ready) = oneshot::channel();
    let run = serve_direct_probe(&server, &gate, false, IO, async {
        started.send(()).unwrap();
        pending().await
    });
    let check = async {
        timeout(IO, ready).await.unwrap().unwrap();
        policy.revoked.insert(fixture.client.id());
        file.write(&policy);
        let mut read = request(&client).await;
        close_is(&client, DENIED_CODE, DENIED).await;
        assert!(read.read_to_end(8192).await.is_err());
    };
    let (denied, _) = tokio::join!(run, check);
    assert!(denied.is_err());
    fixture.close().await;
}

#[tokio::test]
async fn probe_pongs_with_one_reverse_dial_and_releases_the_connection() {
    let fixture = Fixture::new(true).await;
    let gate = fixture.gate(None);
    let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let reverse = dial_direct_probe_reverse(&server, IO, async {
        calls.fetch_add(1, Ordering::SeqCst);
        fixture
            .daemon
            .connect(fixture.client.addr(), DIRECT_PROBE_RETURN_ALPN)
            .await
    });
    let run = serve_direct_probe(&server, &gate, false, IO, reverse);
    let check = async {
        let back = timeout(
            IO,
            accept_direct_probe_reverse(&fixture.client, fixture.daemon.id()),
        )
        .await
        .unwrap()
        .unwrap();
        pong(&client).await;
        assert!(back.close_reason().is_none());
        client.close(0u32.into(), b"done");
        timeout(IO, back.closed()).await.unwrap();
    };
    let (served, _) = tokio::join!(run, check);
    served.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(gate.slots.available_permits(), MAX_DIRECT_PROBES);
    fixture.close().await;
}

#[tokio::test]
async fn missing_reverse_alpn_and_reverse_timeout_preserve_the_direct_ping() {
    let fixture = Fixture::new(false).await;
    let gate = fixture.gate(None);
    for time_out in [false, true] {
        let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
        let (finished, observed) = oneshot::channel();
        let reverse = async {
            let returned = dial_direct_probe_reverse(&server, Duration::from_millis(100), async {
                if time_out {
                    pending().await
                } else {
                    fixture
                        .daemon
                        .connect(fixture.client.addr(), DIRECT_PROBE_RETURN_ALPN)
                        .await
                }
            })
            .await;
            assert!(returned.is_none(), "unexpected reverse connection");
            finished.send(()).unwrap();
            returned
        };
        let run = serve_direct_probe(&server, &gate, false, IO, reverse);
        let check = async {
            timeout(IO, observed).await.unwrap().unwrap();
            pong(&client).await;
            client.close(0u32.into(), b"done");
        };
        let (served, _) = tokio::join!(run, check);
        served.unwrap();
        assert_eq!(gate.slots.available_permits(), MAX_DIRECT_PROBES);
    }
    fixture.close().await;
}

#[tokio::test]
async fn probe_without_a_usable_direct_path_does_not_dispatch() {
    let fixture = Fixture::new(true).await;
    let gate = fixture.gate(None);
    let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
    let run = serve_direct_probe(&server, &gate, true, IO, pending());
    let check = async {
        let mut read = request(&client).await;
        assert!(timeout(Duration::from_millis(100), read.read_chunk(8192))
            .await
            .is_err());
        client.close(0u32.into(), b"done");
    };
    let (not_served, _) = tokio::join!(run, check);
    assert!(not_served.is_err());
    assert_eq!(gate.slots.available_permits(), MAX_DIRECT_PROBES);
    fixture.close().await;
}

#[tokio::test]
async fn probe_saturation_leaves_slots_for_normal_control() {
    let fixture = Fixture::new(true).await;
    let gate = fixture.gate(None);
    assert!(MAX_DIRECT_PROBES < MAX_SESSIONS);
    let mut probes = JoinSet::new();
    let mut clients = Vec::new();
    for _ in 0..MAX_DIRECT_PROBES {
        let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
        clients.push(client);
        let (started, ready) = oneshot::channel();
        let gate = gate.clone();
        probes.spawn(async move {
            serve_direct_probe(&server, &gate, false, IO, async {
                started.send(()).unwrap();
                pending().await
            })
            .await
        });
        timeout(IO, ready).await.unwrap().unwrap();
    }
    let (client, server) = fixture.lane(DIRECT_PROBE_ALPN).await;
    serve_direct_probe(&server, &gate, false, IO, async {
        panic!("busy probe started a reverse dial");
    })
    .await
    .unwrap();
    close_is(&client, DIRECT_PROBE_BUSY_CODE, "iroh_direct_probe_busy").await;
    let (client, server) = fixture.lane(CONTROL_ALPN).await;
    let normal = async {
        let (send, read) = timeout(IO, server.accept_bi()).await.unwrap().unwrap();
        let transport = ControlTransport::Iroh(IrohLineTransport::new(send, read, IO));
        tokio::task::spawn_blocking(move || serve_ping(transport, "direct", || Ok(())))
            .await
            .unwrap()
            .unwrap();
    };
    tokio::join!(normal, pong(&client));
    for client in clients {
        client.close(0u32.into(), b"done");
    }
    while let Some(result) = timeout(IO, probes.join_next()).await.unwrap() {
        assert!(result.unwrap().is_err());
    }
    assert_eq!(gate.slots.available_permits(), MAX_DIRECT_PROBES);
    fixture.close().await;
}

#[tokio::test(start_paused = true)]
async fn a_return_arriving_after_the_old_window_is_held_until_ping_finishes() {
    let arrived = Arc::new(AtomicBool::new(false));
    let held = Arc::new(());
    let witness = Arc::downgrade(&held);
    let returning = async {
        tokio::time::sleep(RELAY_CONNECT + Duration::from_secs(1)).await;
        arrived.store(true, Ordering::SeqCst);
        held
    };
    let forward = async {
        tokio::time::sleep(RELAY_CONNECT + Duration::from_secs(2)).await;
        assert!(arrived.load(Ordering::SeqCst));
        assert!(witness.upgrade().is_some());
    };
    with_probe_reverse(forward, returning).await;
    assert!(witness.upgrade().is_none());
}
