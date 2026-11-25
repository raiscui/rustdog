use super::{setup, Admission, FixedFixture, Rejection, Revocation, SignedGrant, EXPIRED};
use crate::{
    control_frames::ControlFrame,
    control_protocol::PtyAttachRequest,
    pty_control::{attach_active_pty_session, AttachedPtySession, LocalRawTerminalGuard},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use iroh::{Endpoint, EndpointId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tokio::time::timeout;

const TEST_ALPN: &[u8] = b"rdog/test-fixture-admission/1";
const CASE_BUDGET: Duration = Duration::from_secs(20);
const FRAME_BUDGET: Duration = Duration::from_secs(2);

async fn exchange<T: Serialize + DeserializeOwned, U: Serialize + DeserializeOwned>(
    client: &Endpoint,
    target: &Endpoint,
    request: &T,
    accept: impl FnOnce(EndpointId, T) -> U,
) -> U {
    let addr = target.addr();
    assert!(addr.ip_addrs().next().is_some());
    assert!(addr.ip_addrs().all(|addr| addr.ip().is_loopback()));
    assert!(addr.relay_urls().next().is_none());
    let outbound = async {
        let conn = client.connect(addr, TEST_ALPN).await.unwrap();
        assert_eq!(conn.remote_id(), target.id());
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        super::super::frame_put(&mut send, request).await.unwrap();
        super::super::frame_get(&mut recv).await.unwrap()
    };
    let inbound = async {
        let conn = target.accept().await.unwrap().await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let request = super::super::frame_get(&mut recv).await.unwrap();
        let response = accept(conn.remote_id(), request);
        super::super::frame_put(&mut send, &response).await.unwrap();
        send.stopped().await.unwrap();
    };
    timeout(super::super::CONNECT, async {
        tokio::join!(outbound, inbound).0
    })
    .await
    .expect("fixed fixture Iroh exchange within budget")
}

#[derive(Serialize, Deserialize)]
enum Op {
    Open(SignedGrant),
    Attach,
    Detach(String),
    Sample(String),
    Close(String),
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Owner(String),
    Frame(String),
    Denied(u32, String),
    Noop,
}

impl Reply {
    fn denied(error: Rejection) -> Self {
        let (code, reason) = error.code_reason();
        Self::Denied(code, reason.into())
    }

    fn owner(self) -> String {
        match self {
            Self::Owner(owner) => owner,
            _ => panic!("expected attachment owner"),
        }
    }
}

fn wait_frame(
    session: &mut AttachedPtySession,
    mut predicate: impl FnMut(&ControlFrame) -> bool,
) -> ControlFrame {
    let bound = Instant::now() + FRAME_BUDGET;
    loop {
        match session.try_recv_frame() {
            Ok(frame) if predicate(&frame) => return frame,
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(err) => panic!("fixture channel closed before expected frame: {err}"),
        }
        assert!(Instant::now() < bound, "fixture frame missing");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

struct Lane {
    epoch: Instant,
    admission: Admission,
    fixture: FixedFixture,
    lease_id: String,
    owner: Option<String>,
    pty: Option<AttachedPtySession>,
}

impl Lane {
    fn new(admission: Admission) -> Self {
        Self {
            epoch: Instant::now(),
            admission,
            fixture: FixedFixture::new(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            owner: None,
            pty: None,
        }
    }

    fn clock(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn run(&mut self, peer: EndpointId, operation: Op) -> Reply {
        let now = self.clock();
        if let Op::Open(grant) = operation {
            if grant.claims.session != self.lease_id {
                return Reply::denied(Rejection::Denied);
            }
            return match self.fixture.open(&mut self.admission, peer, &grant, now) {
                Ok(pty) => {
                    let owner = pty.session_id().to_owned();
                    self.owner = Some(owner.clone());
                    self.pty = Some(pty);
                    Reply::Owner(owner)
                }
                Err(err) => Reply::denied(err),
            };
        }
        if let Err(err) = self.admission.active(peer, &self.lease_id, now) {
            return Reply::denied(err);
        }
        let pty = self.pty.as_mut().expect("admitted fixed PTY");
        match operation {
            Op::Attach if self.owner.is_none() => {
                let mut next = attach_active_pty_session(PtyAttachRequest {
                    session_id: pty.session_id().into(),
                    cols: 80,
                    rows: 24,
                })
                .unwrap()
                .expect("reattach fixed fixture");
                let ControlFrame::PtyAttached(frame) = wait_frame(&mut next, |frame| {
                    matches!(frame, ControlFrame::PtyAttached(_))
                }) else {
                    unreachable!()
                };
                let owner = frame.control_session_id;
                self.owner = Some(owner.clone());
                pty.disconnect();
                *pty = next;
                Reply::Owner(owner)
            }
            Op::Detach(owner) if self.owner.as_ref() == Some(&owner) => {
                pty.detach("owner_detach").unwrap();
                let frame = wait_frame(pty, |frame| matches!(frame, ControlFrame::PtyDetached(_)));
                self.owner = None;
                Reply::Frame(frame.to_wire_message())
            }
            Op::Sample(owner) if self.owner.as_ref() == Some(&owner) => {
                pty.send_stdin_bytes(b"sample\n".to_vec()).unwrap();
                let mut output = Vec::new();
                wait_frame(pty, |frame| {
                    if let ControlFrame::PtyOutput(frame) = frame {
                        output.extend(STANDARD.decode(&frame.data).unwrap());
                    }
                    output
                        .windows(b"fixture-ack".len())
                        .any(|bytes| bytes == b"fixture-ack")
                });
                Reply::Frame("fixture-ack".into())
            }
            Op::Close(owner) if self.owner.as_ref() == Some(&owner) => {
                pty.disconnect();
                Reply::Frame(
                    wait_frame(pty, |frame| matches!(frame, ControlFrame::PtyClosed(_)))
                        .to_wire_message(),
                )
            }
            _ => Reply::Noop,
        }
    }

    fn finish(&mut self, reason: &'static str, pid: u32) -> String {
        assert!(
            self.owner.is_some(),
            "this case expects an attached fixture"
        );
        let pty = self.pty.as_mut().unwrap();
        pty.close(reason).unwrap();
        let frame = wait_frame(pty, |frame| matches!(frame, ControlFrame::PtyClosed(_)));
        let ControlFrame::PtyClosed(ref details) = frame else {
            unreachable!()
        };
        assert_eq!(details.reason, reason);
        assert!(!exists(pid));
        frame.to_wire_message()
    }
}

#[test]
#[ignore = "requires private loopback and PID namespaces"]
fn fixed_fixture_lifecycle_and_revocation_over_isolated_iroh() {
    assert_eq!(std::process::id(), 1, "run in the isolated test namespace");
    super::super::runtime().unwrap().block_on(async {
        timeout(CASE_BUDGET, async {
            for mode in [Revocation::Graceful, Revocation::Emergency] {
                let (_root, server_key, client_key, admission) = setup();
                let server_id = server_key.public();
                let client_id = client_key.public();
                let bind = Some("127.0.0.1:0".parse().unwrap());
                let target = super::super::endpoint(server_key, bind, TEST_ALPN, None).await.unwrap();
                let client = super::super::endpoint(client_key.clone(), bind, TEST_ALPN, None).await.unwrap();
                let stranger = super::super::endpoint(iroh::SecretKey::generate(), bind, TEST_ALPN, None).await.unwrap();
                let mut lane = Lane::new(admission);
                let ttl = match mode {
                    Revocation::Graceful => Duration::from_secs(3),
                    Revocation::Emergency => Duration::from_secs(12),
                };
                let proof = SignedGrant::issue(&client_key, server_id, &lane.lease_id, lane.clock(), ttl).unwrap();
                let expires = proof.claims.expires_ms;
                let open = Op::Open(proof);
                let denied: Reply = exchange(&stranger, &target, &open, |peer, op| lane.run(peer, op)).await;
                assert!(matches!(denied, Reply::Denied(..)));
                assert!(lane.fixture.pty_id.is_none() && !lane.fixture.dir.0.join("pid").exists());
                let first: Reply = exchange(&client, &target, &open, |peer, op| lane.run(peer, op)).await;
                let first = first.owner();
                let pid = lane.fixture.wait_pid();
                let sample = Op::Sample(first.clone());
                let reply: Reply = exchange(&client, &target, &sample, |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref ack) if ack == "fixture-ack"));

                let reply: Reply = exchange(&client, &target, &Op::Detach(first.clone()), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref frame) if frame.starts_with("@pty-detached ")));
                assert!(exists(pid));
                let next: Reply = exchange(&client, &target, &Op::Attach, |peer, op| lane.run(peer, op)).await;
                let next = next.owner();
                assert_ne!(first, next);
                let reply: Reply = exchange(&client, &target, &Op::Close(first), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Noop));
                let reply: Reply = exchange(&stranger, &target, &Op::Close(next.clone()), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Denied(..)));
                let reply: Reply = exchange(&client, &target, &Op::Sample(next), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref ack) if ack == "fixture-ack"));
                assert!(exists(pid));
                println!("direct Iroh; pre-admission spawn=0; attached and reattached fixture pid={pid}; stale and foreign owner did not close it");

                let affected = lane.admission.revoke(client_id, mode).unwrap();
                let new_id = uuid::Uuid::new_v4().to_string();
                let new_grant = SignedGrant::issue(&client_key, server_id, &new_id, lane.clock(), ttl).unwrap();
                let fresh: Reply = exchange(&client, &target, &new_grant, |peer, grant| {
                    match lane.admission.admit(peer, &grant, lane.clock()) {
                        Ok(()) => Reply::Noop,
                        Err(err) => Reply::denied(err),
                    }
                }).await;
                assert!(matches!(fresh, Reply::Denied(code, _) if code == super::super::DENIED_CODE));
                let terminal = match mode {
                    Revocation::Graceful => {
                        assert!(affected.is_empty());
                        assert!(lane.admission.active(client_id, &lane.lease_id, lane.clock()).is_ok());
                        assert!(exists(pid));
                        tokio::time::sleep_until((lane.epoch + Duration::from_millis(expires)).into()).await;
                        assert_eq!(lane.admission.active(client_id, &lane.lease_id, lane.clock()).err().unwrap().code_reason(), EXPIRED);
                        lane.finish(EXPIRED.1, pid)
                    }
                    Revocation::Emergency => {
                        assert_eq!(affected, [lane.lease_id.clone()]);
                        let terminal = lane.finish("lease_revoked", pid);
                        assert!(lane.clock() < expires);
                        terminal
                    }
                };
                println!("revocation rejected new admission; fixture pid={pid} reaped; elapsed_ms={} expiry_ms={expires}; {terminal}", lane.clock());
                stranger.close().await;
                client.close().await;
                target.close().await;
            }
        })
        .await
        .expect("fixed lifecycle matrix within total budget");
    });
}

#[test]
#[ignore = "requires a real local TTY inside the private test namespace"]
fn local_tty_guard_restores_settings() {
    let before = termios::Termios::from_fd(0).unwrap();
    {
        let _guard = LocalRawTerminalGuard::enter_if_tty()
            .unwrap()
            .expect("real local TTY");
        let raw = termios::Termios::from_fd(0).unwrap();
        assert_ne!(before.c_lflag, raw.c_lflag, "raw mode was not entered");
    }
    let after = termios::Termios::from_fd(0).unwrap();
    assert_eq!(
        (
            before.c_iflag,
            before.c_oflag,
            before.c_cflag,
            before.c_lflag,
            before.c_cc
        ),
        (
            after.c_iflag,
            after.c_oflag,
            after.c_cflag,
            after.c_lflag,
            after.c_cc
        )
    );
    println!("local real TTY restored all recorded flags and control bytes");
}
