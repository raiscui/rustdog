use std::{
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use iroh::{
    endpoint::{presets, Connection, ConnectionError, PortmapperConfig, RecvStream, SendStream},
    Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey, TransportAddr,
};
use serde_json::{json, Value};
use tokio::{
    task::{JoinHandle, JoinSet},
    time::{sleep_until, timeout},
};

mod iroh_p2_state;
use iroh_p2_state::{admit, Decision, Directory, Lease, Permit};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const ALPN: &[u8] = b"rdog-test/rai-178/1";
const TARGET: &str = "lab/target";
const TTL: u64 = 720;
const IO: Duration = Duration::from_secs(2);
const CLOUD_CHECK: Duration = Duration::from_millis(160);
const DENIED: (u32, &str) = (43, "iroh_target_denied");
const CLOUD_DOWN: (u32, &str) = (44, "cloud_unavailable");
const EXPIRED: (u32, &str) = (45, "lease_expired");
const RPC_DONE: (u32, &str) = (0, "fixture_rpc_done");

#[derive(Clone, Copy)]
struct Clock(Instant);

impl Clock {
    fn now(self) -> u64 {
        self.0.elapsed().as_millis().try_into().unwrap()
    }

    async fn until(self, at: u64) {
        sleep_until((self.0 + Duration::from_millis(at)).into()).await;
    }
}

async fn endpoint(key: SecretKey) -> Result<Endpoint> {
    Ok(Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec()])
        .secret_key(key)
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")?
        .bind()
        .await?)
}

fn address(ep: &Endpoint) -> EndpointAddr {
    let sockets = ep.bound_sockets();
    assert_eq!(sockets.len(), 1);
    assert!(sockets[0].is_ipv4() && sockets[0].ip().is_loopback());
    EndpointAddr::from_parts(ep.id(), [TransportAddr::Ip(sockets[0])])
}

async fn authenticated_route(conn: &Connection) -> Result<()> {
    timeout(IO, async {
        loop {
            let paths = conn.paths();
            if paths.iter().next().is_some() {
                assert!(paths.iter().all(|p| matches!(
                    p.remote_addr(), TransportAddr::Ip(ip) if ip.is_ipv4() && ip.ip().is_loopback()
                )));
                return;
            }
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    })
    .await?;
    Ok(())
}

async fn put(stream: &mut SendStream, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    assert!(bytes.len() <= 1024);
    timeout(IO, stream.write_all(&bytes)).await??;
    stream.finish()?;
    Ok(())
}

async fn get(stream: &mut RecvStream) -> Result<Value> {
    Ok(serde_json::from_slice(
        &timeout(IO, stream.read_to_end(1024)).await??,
    )?)
}

async fn exchange(conn: &Connection, value: Value) -> Result<Value> {
    let (mut send, mut read) = timeout(IO, conn.open_bi()).await??;
    put(&mut send, &value).await?;
    get(&mut read).await
}

async fn pair(client: &Endpoint, server: &Endpoint) -> Result<(Connection, Connection)> {
    let incoming = async {
        Ok::<_, Box<dyn Error + Send + Sync>>(server.accept().await.ok_or("target stopped")?.await?)
    };
    let addr = address(server);
    let dial = async { Ok::<_, Box<dyn Error + Send + Sync>>(client.connect(addr, ALPN).await?) };
    let (client_conn, server_conn) =
        timeout(IO, async { tokio::try_join!(dial, incoming) }).await??;
    assert_eq!(client_conn.remote_id(), server.id());
    assert_eq!(server_conn.remote_id(), client.id());
    tokio::try_join!(
        authenticated_route(&client_conn),
        authenticated_route(&server_conn)
    )?;
    Ok((client_conn, server_conn))
}

type Registry = Arc<Mutex<Directory<EndpointId, EndpointAddr>>>;

fn directory_decision(
    registry: &Registry,
    clock: Clock,
    peer: EndpointId,
    request: Value,
) -> Result<Value> {
    let now = clock.now();
    let op = request["op"].as_str().ok_or("missing fixture operation")?;
    if op == "reachability" {
        return Ok(json!({"result":"Reachable"}));
    }
    let target = request["name"].as_str().ok_or("missing fixture name")?;
    let mut guard = registry.lock().unwrap();
    if op == "lookup" {
        return Ok(match guard.lookup(now, &peer, target) {
            Ok(addr) => json!({"result":"Found","addr":addr,"at":now}),
            Err(decision) => json!({"result":format!("{decision:?}"),"at":now}),
        });
    }
    let instance = request["instance"]
        .as_u64()
        .ok_or("missing fixture instance")?;
    let decision = match op {
        "register" => {
            let addr: EndpointAddr = serde_json::from_value(request["addr"].clone())?;
            if addr.id != peer {
                Decision::RegistrationDenied
            } else {
                guard.register(now, TTL, &peer, target, instance, addr)
            }
        }
        "renew" => guard.update(now, Some(TTL), &peer, target, instance),
        "release" => guard.update(now, None, &peer, target, instance),
        _ => return Err("unknown fixture directory operation".into()),
    };
    Ok(json!({"result":format!("{decision:?}"),"at":now}))
}

fn cloud_server(endpoint: Endpoint, registry: Registry, clock: Clock) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        let mut jobs = JoinSet::new();
        loop {
            tokio::select! {
                job = jobs.join_next(), if !jobs.is_empty() => { job.unwrap()??; }
                incoming = endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    let data = registry.clone();
                    jobs.spawn(async move {
                        let conn = timeout(IO, incoming).await??;
                        authenticated_route(&conn).await?;
                        let (mut reply, mut read) = timeout(IO, conn.accept_bi()).await??;
                        let response = directory_decision(&data, clock, conn.remote_id(), get(&mut read).await?)?;
                        put(&mut reply, &response).await?;
                        assert!(timeout(IO, reply.stopped()).await??.is_none());
                        close(&conn, RPC_DONE);
                        Ok::<_, Box<dyn Error + Send + Sync>>(())
                    });
                }
            }
        }
        while let Some(job) = jobs.join_next().await {
            job??;
        }
        Ok(())
    })
}

struct World {
    cloud: Endpoint,
    daemon: Endpoint,
    alternate: Endpoint,
    client: Endpoint,
    outsider: Endpoint,
    address: EndpointAddr,
    clock: Clock,
    state: Registry,
    server: JoinHandle<Result<()>>,
}

impl World {
    async fn new() -> Result<Self> {
        let (cloud, daemon, alternate, client, outsider) = tokio::try_join!(
            endpoint(SecretKey::generate()),
            endpoint(SecretKey::generate()),
            endpoint(SecretKey::generate()),
            endpoint(SecretKey::generate()),
            endpoint(SecretKey::generate()),
        )?;
        let clock = Clock(Instant::now());
        let state = Arc::new(Mutex::new(Directory::new(TARGET, daemon.id(), client.id())));
        let server = cloud_server(cloud.clone(), state.clone(), clock);
        let address = address(&cloud);
        Ok(Self {
            cloud,
            daemon,
            alternate,
            client,
            outsider,
            address,
            clock,
            state,
            server,
        })
    }

    async fn response(&self, ep: &Endpoint, op: Value) -> Result<Value> {
        let conn = timeout(IO, ep.connect(self.address.clone(), ALPN)).await??;
        assert_eq!(conn.remote_id(), self.cloud.id());
        authenticated_route(&conn).await?;
        let operation = op["op"].as_str().ok_or("missing operation")?.to_owned();
        let reply = exchange(&conn, op).await?;
        expect_close(&conn, RPC_DONE).await?;
        println!(
            "directory operation={operation} result={} authenticated_iroh_handshake=true",
            reply["result"].as_str().unwrap()
        );
        Ok(reply)
    }

    async fn register(&self, ep: &Endpoint, instance: u64) -> Result<Value> {
        self.response(
            ep,
            json!({"op":"register","name":TARGET,"instance":instance,"addr":address(ep)}),
        )
        .await
    }

    async fn lookup(&self, ep: &Endpoint) -> Result<Value> {
        self.response(ep, json!({"op":"lookup","name":TARGET}))
            .await
    }

    async fn clear(self) -> Result<()> {
        tokio::join!(
            self.cloud.close(),
            self.daemon.close(),
            self.alternate.close(),
            self.client.close(),
            self.outsider.close(),
        );
        timeout(IO, self.server).await???;
        let registry =
            Arc::try_unwrap(self.state).map_err(|_| "test registry still shared after stop")?;
        drop(registry.into_inner().unwrap());
        println!("cleanup: all endpoints closed, test directory empty");
        Ok(())
    }
}

fn expect(reply: &Value, wanted: &str) {
    assert_eq!(reply["result"], wanted);
}

fn assert_addr(reply: &Value, endpoint: &Endpoint) -> Result<()> {
    let actual: EndpointAddr = serde_json::from_value(reply["addr"].clone())?;
    assert_eq!(actual, address(endpoint));
    Ok(())
}

async fn directory_case(case: &str, w: &World) -> Result<u8> {
    let denied = match case {
        "register-allow" => {
            expect(&w.register(&w.daemon, 1).await?, "Registered");
            assert_addr(&w.lookup(&w.client).await?, &w.daemon)?;
            false
        }
        "register-deny" => {
            let denial = w.register(&w.outsider, 1).await?;
            expect(&denial, "RegistrationDenied");
            assert!(denial["addr"].is_null());
            assert!(!w.state.lock().unwrap().occupied(w.clock.now()));
            true
        }
        "register-renew" => {
            let registered = w.register(&w.daemon, 1).await?;
            expect(&registered, "Registered");
            let expires = registered["at"].as_u64().unwrap() + TTL;
            w.clock.until(expires - TTL / 2).await;
            let reply = w
                .response(&w.daemon, json!({"op":"renew","name":TARGET,"instance":1}))
                .await?;
            expect(&reply, "Renewed");
            w.clock.until(expires + 30).await;
            let after = w.lookup(&w.client).await?;
            expect(&after, "Found");
            assert_addr(&after, &w.daemon)?;
            println!("renewed registration resolved after original deadline");
            false
        }
        "register-race" => {
            let rival = endpoint(w.daemon.secret_key().clone()).await?;
            let (a, b) = tokio::try_join!(w.register(&w.daemon, 1), w.register(&rival, 2))?;
            let winner = if a["result"] == "Registered" {
                expect(&b, "NameOccupied");
                &w.daemon
            } else {
                expect(&a, "NameOccupied");
                expect(&b, "Registered");
                &rival
            };
            assert_addr(&w.lookup(&w.client).await?, winner)?;
            rival.close().await;
            println!("concurrent same-name registration: one winner, one NameOccupied");
            false
        }
        "delayed-release-deny" => {
            let registered = w.register(&w.daemon, 1).await?;
            expect(&registered, "Registered");
            w.clock
                .until(registered["at"].as_u64().unwrap() + TTL + 30)
                .await;
            expect(&w.lookup(&w.client).await?, "NotRegistered");
            assert!(!w.state.lock().unwrap().occupied(w.clock.now()));
            let replacement = endpoint(w.daemon.secret_key().clone()).await?;
            expect(&w.register(&replacement, 2).await?, "Registered");
            let stale = w
                .response(
                    &w.daemon,
                    json!({"op":"release","name":TARGET,"instance":1}),
                )
                .await?;
            expect(&stale, "StaleInstance");
            assert_addr(&w.lookup(&w.client).await?, &replacement)?;
            expect(
                &w.response(
                    &replacement,
                    json!({"op":"release","name":TARGET,"instance":2}),
                )
                .await?,
                "Released",
            );
            assert!(!w.state.lock().unwrap().occupied(w.clock.now()));
            replacement.close().await;
            println!(
                "expired entry reclaimed; delayed release rejected; replacement stayed resolvable"
            );
            true
        }
        "lookup-allow" | "lookup-deny" => {
            expect(&w.register(&w.daemon, 1).await?, "Registered");
            if case == "lookup-allow" {
                let hit = w.lookup(&w.client).await?;
                expect(&hit, "Found");
                assert_addr(&hit, &w.daemon)?;
                false
            } else {
                let denial = w.lookup(&w.outsider).await?;
                expect(&denial, "LookupDenied");
                assert!(denial.get("addr").is_none());
                true
            }
        }
        _ => return Err("unknown fixture directory case".into()),
    };
    println!(
        "case={case} authenticated_iroh_handshake=true verdict={}",
        if denied { "deny" } else { "allow" }
    );
    Ok(if denied { 10 } else { 0 })
}

async fn cloud_reachable(ep: &Endpoint, addr: &EndpointAddr) -> bool {
    timeout(CLOUD_CHECK, async {
        let conn = ep.connect(addr.clone(), ALPN).await?;
        assert_eq!(conn.remote_id(), addr.id);
        authenticated_route(&conn).await?;
        let reply = exchange(&conn, json!({"op":"reachability"})).await?;
        expect_close(&conn, RPC_DONE).await?;
        Ok::<_, Box<dyn Error + Send + Sync>>(reply["result"] == "Reachable")
    })
    .await
    .is_ok_and(|result| matches!(result, Ok(true)))
}

fn close(conn: &Connection, reason: (u32, &str)) {
    conn.close(reason.0.into(), reason.1.as_bytes());
}

async fn expect_close(conn: &Connection, reason: (u32, &str)) -> Result<()> {
    match timeout(IO, conn.closed()).await? {
        ConnectionError::ApplicationClosed(close) => {
            assert_eq!(close.error_code.into_inner(), u64::from(reason.0));
            assert_eq!(close.reason.as_ref(), reason.1.as_bytes());
        }
        other => return Err(format!("expected explicit application close, got {other}").into()),
    }
    assert!(
        conn.open_bi().await.is_err(),
        "closed session accepted a new stream"
    );
    Ok(())
}

struct Active(Arc<AtomicUsize>);

impl Active {
    fn new(count: Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);
        Self(count)
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct SessionResult {
    entries: usize,
    admission: Option<Decision>,
}

async fn serve_session(
    conn: Connection,
    ep: Endpoint,
    directory: EndpointAddr,
    clock: Clock,
    permits: Permit<EndpointId>,
    active: Arc<AtomicUsize>,
) -> Result<SessionResult> {
    let (mut send, mut read) = timeout(IO, conn.accept_bi()).await??;
    let fixture_request = get(&mut read).await?;
    let name = fixture_request["name"]
        .as_str()
        .ok_or("missing fixture target")?;
    let fixture_entry = (fixture_request["handle"].as_u64() == Some(1)).then_some(&permits);
    let mut lease = match admit(
        fixture_entry,
        clock.now(),
        &conn.remote_id(),
        &ep.id(),
        name,
    ) {
        Ok(lease) => lease,
        Err(why) => {
            close(&conn, DENIED);
            return Ok(SessionResult {
                entries: 0,
                admission: Some(why),
            });
        }
    };
    if !cloud_reachable(&ep, &directory).await {
        close(&conn, CLOUD_DOWN);
        return Ok(SessionResult {
            entries: 0,
            admission: Some(Decision::CloudUnavailable),
        });
    }
    if !lease.live(clock.now()) {
        close(&conn, EXPIRED);
        return Ok(SessionResult {
            entries: 0,
            admission: Some(Decision::LeaseExpired),
        });
    }
    let _live = Active::new(active);
    put(&mut send, &json!({"result":"Accepted","until":lease.0})).await?;
    let mut entries = 0;
    loop {
        let (mut reply, mut request) = tokio::select! {
            biased;
            _ = clock.until(lease.0) => { close(&conn, EXPIRED); break; }
            _ = conn.closed() => break,
            s = conn.accept_bi() => s?,
        };
        let incoming = tokio::select! {
            biased;
            _ = clock.until(lease.0) => { close(&conn, EXPIRED); break; }
            message = get(&mut request) => message?,
        };
        let response = match incoming["op"].as_str() {
            Some("ping") if lease.live(clock.now()) => {
                entries += 1;
                json!({"result":"Pong","id":42,"pong":true})
            }
            Some("renew") => {
                let online = tokio::select! {
                    biased;
                    _ = clock.until(lease.0) => { close(&conn, EXPIRED); break; }
                    result = cloud_reachable(&ep, &directory) => result,
                };
                let result = lease.renew(clock.now(), TTL, online);
                json!({"result":format!("{result:?}"),"until":lease.0})
            }
            _ => return Err("fixture only accepts ping and renewal".into()),
        };
        tokio::select! {
            biased;
            _ = clock.until(lease.0) => { close(&conn, EXPIRED); break; }
            result = put(&mut reply, &response) => result?,
        }
    }
    Ok(SessionResult {
        entries,
        admission: None,
    })
}

async fn ping(conn: &Connection) -> Result<()> {
    let reply = exchange(conn, json!({"op":"ping"})).await?;
    assert_eq!(reply, json!({"result":"Pong","id":42,"pong":true}));
    Ok(())
}

async fn session_case(case: &str, w: &World) -> Result<u8> {
    let client = if case == "admit-wrong-client" {
        &w.outsider
    } else {
        &w.client
    };
    let daemon = if case == "admit-wrong-endpoint" {
        &w.alternate
    } else {
        &w.daemon
    };
    let requested = if case == "admit-wrong-target" {
        "lab/other"
    } else {
        TARGET
    };
    let handle = if case == "admit-unissued" { 2 } else { 1 };
    if case == "cloud-new-session" {
        w.cloud.close().await;
    }
    let (connection, server_conn) = pair(client, daemon).await?;
    let stable = connection.stable_id();
    let permit = Permit {
        subject: w.client.id(),
        endpoint: w.daemon.id(),
        target: TARGET,
        lease: Lease(w.clock.now() + if case == "admit-expired" { 0 } else { TTL }),
    };
    let active = Arc::new(AtomicUsize::new(0));
    let running = tokio::spawn(serve_session(
        server_conn,
        daemon.clone(),
        w.address.clone(),
        w.clock,
        permit,
        active.clone(),
    ));
    let reply = exchange(&connection, json!({"name":requested,"handle":handle})).await;
    if let Some((wanted, reason)) = match case {
        "admit-wrong-client" | "admit-wrong-endpoint" | "admit-wrong-target" => {
            Some((Decision::PermitMismatch, DENIED))
        }
        "admit-unissued" => Some((Decision::PermitUnknown, DENIED)),
        "admit-expired" => Some((Decision::PermitExpired, DENIED)),
        "cloud-new-session" => Some((Decision::CloudUnavailable, CLOUD_DOWN)),
        _ => None,
    } {
        assert!(reply.is_err());
        expect_close(&connection, reason).await?;
        let result = timeout(IO, running).await???;
        assert_eq!(result.admission, Some(wanted));
        assert_eq!(result.entries, 0);
        assert_eq!(active.load(Ordering::Relaxed), 0);
        println!("case={case} authenticated_iroh_handshake=true verdict=deny detail={wanted:?} close={}/{} handler_entries=0 active_sessions=0", reason.0, reason.1);
        return Ok(10);
    }
    let reply = reply?;
    expect(&reply, "Accepted");
    assert_eq!(active.load(Ordering::Relaxed), 1);
    let original = reply["until"].as_u64().unwrap();
    ping(&connection).await?;
    let entries = match case {
        "admit-allow" => {
            connection.close(0u8.into(), b"fixture client finished");
            1
        }
        "session-renew" => {
            w.clock.until(original - TTL / 2).await;
            let renewal = exchange(&connection, json!({"op":"renew"})).await?;
            expect(&renewal, "Renewed");
            let new = renewal["until"].as_u64().unwrap();
            assert!(new > original);
            w.clock.until(original + 30).await;
            assert!(w.clock.now() < new);
            ping(&connection).await?;
            assert_eq!(connection.stable_id(), stable);
            connection.close(0u8.into(), b"fixture client finished");
            println!("renewed same session operated after original_deadline_ms={original} new_deadline_ms={new}");
            2
        }
        "session-expire" | "cloud-renew" | "cloud-existing" => {
            if case != "session-expire" {
                w.cloud.close().await;
                if case == "cloud-renew" {
                    let renewal = exchange(&connection, json!({"op":"renew"})).await?;
                    expect(&renewal, "CloudUnavailable");
                    assert_eq!(renewal["until"], original);
                    println!("offline renewal refused; deadline unchanged");
                }
                assert!(w.clock.now() < original);
                ping(&connection).await?;
                assert_eq!(connection.stable_id(), stable);
                println!("same session still operated after cloud stopped, before its deadline");
            }
            expect_close(&connection, EXPIRED).await?;
            assert!(w.clock.now() >= original && w.clock.now() - original <= 150);
            println!("lease original_deadline_ms={original} observed_close_ms={} explicit 45/lease_expired; new stream rejected", w.clock.now());
            if case == "session-expire" {
                1
            } else {
                2
            }
        }
        _ => return Err("unknown fixture session case".into()),
    };
    let result = timeout(IO, running).await???;
    assert!(result.admission.is_none());
    assert_eq!(result.entries, entries);
    assert_eq!(active.load(Ordering::Relaxed), 0);
    let denied = case == "session-expire" || case == "cloud-renew";
    println!("case={case} authenticated_iroh_handshake=true verdict={} handler_entries={entries} active_sessions=0", if denied { "deny" } else { "allow" });
    Ok(if denied { 10 } else { 0 })
}

fn main() -> Result<std::process::ExitCode> {
    let case = std::env::args().nth(1).ok_or("missing fixture case")?;
    let code = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            timeout(Duration::from_secs(6), async {
                let world = World::new().await?;
                let result = if case.starts_with("register-")
                    || case.starts_with("lookup-")
                    || case == "delayed-release-deny"
                {
                    directory_case(&case, &world).await
                } else {
                    session_case(&case, &world).await
                };
                world.clear().await?;
                result
            })
            .await?
        })?;
    Ok(code.into())
}
