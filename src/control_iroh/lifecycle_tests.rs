use super::{
    pid_exists, setup, Admission, FixedFixture, PersistenceError, Rejection, Revocation,
    RevocationProgress, SignedGrant, EXPIRED,
};
use crate::{
    control_frames::ControlFrame,
    control_protocol::PtyAttachRequest,
    pty_control::{
        attach_active_pty_session, close_test_pty_fixture, AttachedPtySession,
        LocalRawTerminalGuard,
    },
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use iroh::{Endpoint, EndpointId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io,
    sync::{Arc, Mutex, MutexGuard},
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
    TerminationPending,
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
    predicate: impl FnMut(&ControlFrame) -> bool,
) -> ControlFrame {
    try_wait_frame(session, predicate).expect("fixed fixture frame")
}

fn try_wait_frame(
    session: &mut AttachedPtySession,
    mut predicate: impl FnMut(&ControlFrame) -> bool,
) -> io::Result<ControlFrame> {
    let bound = Instant::now() + FRAME_BUDGET;
    loop {
        match session.try_recv_frame() {
            Ok(frame) if predicate(&frame) => return Ok(frame),
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(err) => return Err(io::Error::other(err)),
        }
        if Instant::now() >= bound {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture frame missing",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Lane {
    epoch: Instant,
    admission: Admission,
    fixture: FixedFixture,
    lease_id: String,
    owner: Option<String>,
    pty: Option<AttachedPtySession>,
    first_attachment: Option<AttachedPtySession>,
    pid: Option<u32>,
    termination_sent: bool,
    observed_terminal: Option<String>,
    terminal: Option<String>,
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
            first_attachment: None,
            pid: None,
            termination_sent: false,
            observed_terminal: None,
            terminal: None,
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
                    let pid = self.fixture.wait_pid();
                    self.admission.record_fixture(&self.lease_id, pid);
                    self.pid = Some(pid);
                    self.owner = Some(owner.clone());
                    self.pty = Some(pty);
                    Reply::Owner(owner)
                }
                Err(err) => Reply::denied(err),
            };
        }
        if self.terminal.is_some() && matches!(operation, Op::Close(_)) {
            return Reply::Noop;
        }
        if let Err(err) = self.admission.active(peer, &self.lease_id, now) {
            return Reply::denied(err);
        }
        if self.termination_sent
            && !matches!(&operation, Op::Close(owner) if self.owner.as_ref() == Some(owner))
        {
            return Reply::TerminationPending;
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
                let previous = std::mem::replace(pty, next);
                if self.first_attachment.is_none() {
                    self.first_attachment = Some(previous);
                }
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
                match self.finish("control_disconnect") {
                    Ok(frame) => Reply::Frame(frame),
                    Err(_) => Reply::TerminationPending,
                }
            }
            _ => Reply::Noop,
        }
    }

    fn finish(&mut self, reason: &'static str) -> io::Result<String> {
        if let Some(frame) = &self.terminal {
            return Ok(frame.clone());
        }
        let pty = self.pty.as_mut().unwrap();
        if !self.termination_sent {
            close_test_pty_fixture(pty.session_id(), reason)?;
            self.termination_sent = true;
        }
        if self.observed_terminal.is_none() {
            let terminal_receiver = if self.owner.is_none() {
                self.first_attachment.as_mut().unwrap_or(pty)
            } else {
                pty
            };
            let frame = try_wait_frame(terminal_receiver, |frame| {
                matches!(frame, ControlFrame::PtyClosed(_) | ControlFrame::PtyExit(_))
            })?;
            self.observed_terminal = Some(frame.to_wire_message());
        }
        if self.pid.is_some_and(pid_exists) {
            return Err(io::Error::other(
                "terminal observed, but fixture PID not reaped",
            ));
        }
        let result = self.observed_terminal.take().expect("observed terminal");
        self.admission.confirm_terminal(&self.lease_id);
        self.owner = None;
        self.pty = None;
        self.first_attachment = None;
        self.pid = None;
        self.fixture.pty_id = None;
        self.terminal = Some(result.clone());
        Ok(result)
    }

    fn revoke(
        &mut self,
        peer: EndpointId,
        mode: Revocation,
    ) -> Result<RevocationProgress, PersistenceError> {
        let progress = self.admission.revoke(peer, mode, self.clock())?;
        if let RevocationProgress::TerminationPending(ids) = &progress {
            if ids.contains(&uuid::Uuid::parse_str(&self.lease_id).unwrap())
                && self.finish("lease_revoked").is_ok()
            {
                return Ok(RevocationProgress::Persisted);
            }
        }
        Ok(progress)
    }
}

struct LaneDriver {
    state: Arc<Mutex<Lane>>,
    deadline_task: Option<tokio::task::JoinHandle<io::Result<()>>>,
}

impl LaneDriver {
    fn new(admission: Admission) -> Self {
        Self {
            state: Arc::new(Mutex::new(Lane::new(admission))),
            deadline_task: None,
        }
    }

    fn state(&self) -> MutexGuard<'_, Lane> {
        self.state.lock().expect("isolated lane state")
    }

    fn run(&mut self, peer: EndpointId, operation: Op) -> Reply {
        let (reply, cutoff) = {
            let mut lane = self.state();
            let expires = match &operation {
                Op::Open(grant) => Some(grant.claims.expires_ms),
                _ => None,
            };
            let reply = lane.run(peer, operation);
            let cutoff = expires
                .filter(|_| matches!(reply, Reply::Owner(_)))
                .map(|expiry| lane.epoch + Duration::from_millis(expiry));
            (reply, cutoff)
        };
        if let Some(cutoff) = cutoff {
            let weak = Arc::downgrade(&self.state);
            self.deadline_task = Some(tokio::spawn(async move {
                tokio::time::sleep_until(cutoff.into()).await;
                if let Some(state) = weak.upgrade() {
                    let mut lane = state.lock().expect("isolated lane state");
                    if lane.pty.is_some() {
                        lane.finish(EXPIRED.1)?;
                    }
                }
                Ok(())
            }));
        }
        reply
    }

    async fn wait_for_deadline(&mut self) {
        timeout(
            FRAME_BUDGET + CASE_BUDGET,
            self.deadline_task.take().expect("admitted lane timer"),
        )
        .await
        .expect("isolated expiry controller terminated")
        .expect("isolated expiry controller joined")
        .expect("expiry-confirmed terminal");
    }
}

impl Drop for LaneDriver {
    fn drop(&mut self) {
        if let Some(timer) = &self.deadline_task {
            timer.abort();
        }
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
                let mut lane = LaneDriver::new(admission);
                let ttl = match mode {
                    Revocation::Graceful => Duration::from_secs(3),
                    Revocation::Emergency => Duration::from_secs(12),
                };
                let proof = {
                    let state = lane.state();
                    SignedGrant::issue(&client_key, server_id, &state.lease_id, state.clock(), ttl).unwrap()
                };
                let expires = proof.claims.expires_ms;
                let open = Op::Open(proof);
                let denied: Reply = exchange(&stranger, &target, &open, |peer, op| lane.run(peer, op)).await;
                assert!(matches!(denied, Reply::Denied(..)));
                {
                    let state = lane.state();
                    assert!(state.fixture.pty_id.is_none() && !state.fixture.dir.0.join("pid").exists());
                }
                let first: Reply = exchange(&client, &target, &open, |peer, op| lane.run(peer, op)).await;
                let first = first.owner();
                let pid = lane.state().pid.unwrap();
                let sample = Op::Sample(first.clone());
                let reply: Reply = exchange(&client, &target, &sample, |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref ack) if ack == "fixture-ack"));

                let reply: Reply = exchange(&client, &target, &Op::Detach(first.clone()), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref frame) if frame.starts_with("@pty-detached ")));
                assert!(pid_exists(pid));
                let next: Reply = exchange(&client, &target, &Op::Attach, |peer, op| lane.run(peer, op)).await;
                let next = next.owner();
                assert_ne!(first, next);
                let reply: Reply = exchange(&client, &target, &Op::Close(first), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Noop));
                let reply: Reply = exchange(&stranger, &target, &Op::Close(next.clone()), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Denied(..)));
                let reply: Reply = exchange(&client, &target, &Op::Sample(next.clone()), |peer, op| lane.run(peer, op)).await;
                assert!(matches!(reply, Reply::Frame(ref ack) if ack == "fixture-ack"));
                assert!(pid_exists(pid));
                println!("direct Iroh; pre-admission spawn=0; attached and reattached fixture pid={pid}; stale and foreign owner did not close it");

                assert_eq!(lane.state().revoke(client_id, mode).unwrap(), RevocationProgress::Persisted);
                let new_id = uuid::Uuid::new_v4().to_string();
                let new_grant = SignedGrant::issue(&client_key, server_id, &new_id, lane.state().clock(), ttl).unwrap();
                let fresh: Reply = exchange(&client, &target, &new_grant, |peer, grant| {
                    let mut state = lane.state();
                    let now = state.clock();
                    match state.admission.admit(peer, &grant, now) {
                        Ok(()) => Reply::Noop,
                        Err(err) => Reply::denied(err),
                    }
                }).await;
                assert!(matches!(fresh, Reply::Denied(code, ref reason) if (code, reason.as_str()) == (super::super::DENIED_CODE, super::super::DENIED)));
                let expected_reason = match mode {
                    Revocation::Graceful => {
                        assert!(pid_exists(pid));
                        let reply: Reply = exchange(&client, &target, &Op::Sample(next.clone()), |peer, op| lane.run(peer, op)).await;
                        assert!(matches!(reply, Reply::Frame(ref ack) if ack == "fixture-ack"));
                        let reply: Reply = exchange(&client, &target, &Op::Detach(next), |peer, op| lane.run(peer, op)).await;
                        assert!(matches!(reply, Reply::Frame(ref frame) if frame.starts_with("@pty-detached ")));
                        lane.wait_for_deadline().await;
                        let state = lane.state();
                        assert_eq!(state.admission.active(client_id, &state.lease_id, state.clock()).err().unwrap().code_reason(), EXPIRED);
                        EXPIRED.1
                    }
                    Revocation::Emergency => {
                        assert!(lane.state().clock() < expires);
                        "lease_revoked"
                    }
                };
                {
                    let state = lane.state();
                    let terminal = state.terminal.as_ref().unwrap();
                    assert!(matches!(
                        ControlFrame::parse_inbound_result_message(terminal).unwrap(),
                        ControlFrame::PtyClosed(frame) if frame.reason == expected_reason
                    ));
                    assert!(!pid_exists(pid) && state.pty.is_none() && state.owner.is_none());
                    println!("revocation rejected new admission; fixture pid={pid} reaped; elapsed_ms={} expiry_ms={expires}; {terminal}", state.clock());
                }
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

#[test]
fn detached_fixture_expires_without_further_requests() {
    let (_root, target, client, admission) = setup();
    let peer = client.public();
    super::super::runtime().unwrap().block_on(async {
        let mut lane = LaneDriver::new(admission);
        let proof = {
            let state = lane.state();
            SignedGrant::issue(&client, target.public(), &state.lease_id, state.clock(), Duration::from_millis(300)).unwrap()
        };
        let owner = lane.run(peer, Op::Open(proof)).owner();
        let pid = lane.state().pid.unwrap();
        assert!(matches!(lane.run(peer, Op::Detach(owner)), Reply::Frame(_)));
        assert!(pid_exists(pid));
        timeout(FRAME_BUDGET, lane.wait_for_deadline()).await.expect("detached fixture still live after lease deadline");
        assert!(!pid_exists(pid));
        assert!(matches!(
            ControlFrame::parse_inbound_result_message(lane.state().terminal.as_ref().unwrap()).unwrap(),
            ControlFrame::PtyClosed(frame) if frame.reason == EXPIRED.1
        ));
        println!("detached fixture pid={pid} reaped by service timer without a later request");
    });
}

#[test]
fn confirmed_close_clears_live_state_and_emergency_selection() {
    let (_root, target, client, admission) = setup();
    let peer = client.public();
    let mut lane = Lane::new(admission);
    let proof = SignedGrant::issue(
        &client,
        target.public(),
        &lane.lease_id,
        lane.clock(),
        Duration::from_secs(5),
    )
    .unwrap();
    let owner = lane.run(peer, Op::Open(proof)).owner();
    let pid = lane.fixture.wait_pid();
    assert!(matches!(
        lane.run(peer, Op::Close(owner.clone())),
        Reply::Frame(_)
    ));
    assert!(!pid_exists(pid));
    assert!(
        lane.pty.is_none() && lane.owner.is_none(),
        "completed Close must clear live fixture state"
    );
    assert!(matches!(lane.run(peer, Op::Close(owner)), Reply::Noop));
    assert_eq!(
        lane.revoke(peer, Revocation::Emergency).unwrap(),
        RevocationProgress::Persisted
    );
}
