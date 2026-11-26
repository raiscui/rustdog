use super::{bridge_attached_pty_session_inner, PtyBridgeIo};
use crate::{
    control_frames::ControlFrame,
    control_protocol::{PtyAttachRequest, PtyOpenRequest},
    pty_control::{
        attach_active_pty_session, close_active_pty_session, open_attached_pty_session,
        AttachedPtySession,
    },
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    dir: PathBuf,
    session_id: Option<String>,
    pid: Option<u32>,
}

impl Fixture {
    fn open() -> (Self, AttachedPtySession) {
        let dir = std::env::temp_dir().join(format!("rdog-pty-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let mut fixture = Self {
            dir,
            session_id: None,
            pid: None,
        };
        let program = r#"
import os, sys
from pathlib import Path
root = Path(sys.argv[1])
assert os.isatty(0)
(root / "pid").write_text(str(os.getpid()))
for line in sys.stdin:
    command = line.rstrip("\r\n")
    if command == "fixture-exit":
        sys.exit(23)
    if command == "fixture-check":
        (root / "checked").touch()
"#;
        let session = open_attached_pty_session(PtyOpenRequest {
            cmd: "/usr/bin/python3".into(),
            args: vec![
                "-c".into(),
                program.into(),
                fixture.dir.to_string_lossy().into_owned(),
            ],
            cols: 80,
            rows: 24,
        })
        .unwrap();
        fixture.session_id = Some(session.session_id().into());
        let pid = wait_for(|| {
            fs::read_to_string(fixture.dir.join("pid"))
                .ok()?
                .parse::<u32>()
                .ok()
        })
        .expect("fixture must start in a PTY");
        fixture.pid = Some(pid);
        assert!(proc_path(pid).exists());
        (fixture, session)
    }

    fn pid(&self) -> u32 {
        self.pid.expect("fixture pid registered")
    }

    fn alive(&self) -> bool {
        proc_path(self.pid()).exists()
    }

    fn expect_reaped(&self) {
        assert!(
            wait_for(|| (!self.alive()).then_some(())).is_some(),
            "fixture pid={} still exists after bridge returned",
            self.pid()
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(id) = self.session_id.as_deref() {
            if self.pid.is_none() || self.alive() {
                if let Err(err) = close_active_pty_session(id) {
                    eprintln!("fixture PTY {id} could not be closed: {err}");
                }
                if self.pid.is_some() && wait_for(|| (!self.alive()).then_some(())).is_none() {
                    eprintln!("fixture pid={} did not exit during cleanup", self.pid());
                }
            }
        }
        if let Err(err) = fs::remove_dir_all(&self.dir) {
            eprintln!("fixture directory cleanup failed: {err}");
        }
    }
}

fn proc_path(pid: u32) -> PathBuf {
    Path::new("/proc").join(pid.to_string())
}

fn wait_for<T>(mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(value) = check() {
            return Some(value);
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

struct MemoryBridge<R, W> {
    read: R,
    write: W,
}

impl<R, W> PtyBridgeIo for MemoryBridge<R, W>
where
    R: FnMut() -> io::Result<Option<String>>,
    W: FnMut(&str) -> io::Result<()>,
{
    fn read_message(&mut self) -> io::Result<Option<String>> {
        (self.read)()
    }
    fn write_message(&mut self, message: &str) -> io::Result<()> {
        (self.write)(message)
    }
}

fn injected_read_failure(session: &mut AttachedPtySession) -> io::Error {
    bridge_attached_pty_session_inner(
        &mut MemoryBridge {
            read: || Err(io::ErrorKind::ConnectionAborted.into()),
            write: |_: &str| Ok(()),
        },
        session,
    )
    .unwrap_err()
}

fn terminal_exit(session: &mut AttachedPtySession, fixture: &Fixture) {
    session
        .send_stdin_bytes(b"fixture-exit\n".to_vec())
        .unwrap();
    let mut terminal = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    bridge_attached_pty_session_inner(
        &mut MemoryBridge {
            read: || {
                assert!(
                    Instant::now() < deadline,
                    "normal exit didn't produce a terminal frame"
                );
                thread::sleep(Duration::from_millis(2));
                Err(io::ErrorKind::WouldBlock.into())
            },
            write: |line: &str| {
                if let ControlFrame::PtyExit(frame) =
                    ControlFrame::parse_inbound_result_message(line)?
                {
                    assert!(terminal.replace((frame.exit_code, frame.reason)).is_none());
                }
                Ok(())
            },
        },
        session,
    )
    .unwrap();
    assert_eq!(terminal, Some((23, "process_exit".into())));
    fixture.expect_reaped();
    eprintln!(
        "fixture pid={} terminal=@pty-exit exit_code=23 process_exit",
        fixture.pid()
    );
}

fn disconnected_exit(session: &mut AttachedPtySession, fixture: &Fixture) {
    fixture.expect_reaped();
    let frame = wait_for(|| {
        while let Ok(frame) = session.try_recv_frame() {
            if let ControlFrame::PtyClosed(frame) = frame {
                return Some(frame);
            }
        }
        None
    })
    .expect("runtime must report a closed terminal state");
    assert_eq!(frame.reason, "control_disconnect");
    assert_eq!(frame.session_id, session.session_id());
    eprintln!(
        "fixture pid={} terminal=@pty-closed reason=control_disconnect",
        fixture.pid()
    );
}

#[test]
fn read_error_reaps_attached_child() {
    let (fixture, mut session) = Fixture::open();
    assert_eq!(
        injected_read_failure(&mut session).kind(),
        io::ErrorKind::ConnectionAborted
    );
    disconnected_exit(&mut session, &fixture);
}

#[test]
fn write_error_reaps_attached_child() {
    let (fixture, mut session) = Fixture::open();
    let error = bridge_attached_pty_session_inner(
        &mut MemoryBridge {
            read: || panic!("write failure must precede any read"),
            write: |_: &str| Err(io::ErrorKind::BrokenPipe.into()),
        },
        &mut session,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    disconnected_exit(&mut session, &fixture);
}

#[test]
fn non_tcp_write_error_reaps_attached_child() {
    let (fixture, mut session) = Fixture::open();
    let error = bridge_attached_pty_session_inner(
        &mut MemoryBridge {
            read: || panic!("ready frame must precede first read"),
            write: |_: &str| Err(io::ErrorKind::BrokenPipe.into()),
        },
        &mut session,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    disconnected_exit(&mut session, &fixture);
}

#[test]
fn disconnect_does_not_reap_parallel_child() {
    let (fixture, mut broken) = Fixture::open();
    let (other, mut healthy) = Fixture::open();
    assert_eq!(
        injected_read_failure(&mut broken).kind(),
        io::ErrorKind::ConnectionAborted
    );
    disconnected_exit(&mut broken, &fixture);
    assert!(other.alive());
    eprintln!(
        "parallel fixture pid={} still alive after pid={} was reaped",
        other.pid(),
        fixture.pid()
    );
    terminal_exit(&mut healthy, &other);
}

#[test]
fn detach_ack_write_failure_does_not_reap_detached_child() {
    let (fixture, mut original) = Fixture::open();
    original.detach("owner_detach").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let error = bridge_attached_pty_session_inner(
        &mut MemoryBridge {
            read: || {
                assert!(Instant::now() < deadline, "no detached frame");
                thread::sleep(Duration::from_millis(2));
                Err(io::ErrorKind::WouldBlock.into())
            },
            write: |line: &str| {
                if matches!(
                    ControlFrame::parse_inbound_result_message(line)?,
                    ControlFrame::PtyDetached(_)
                ) {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                Ok(())
            },
        },
        &mut original,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let session_id = original.session_id().to_owned();
    drop(original);
    let mut reattached = attach_active_pty_session(PtyAttachRequest {
        session_id,
        cols: 80,
        rows: 24,
    })
    .unwrap()
    .expect("detached child still attachable");
    assert!(fixture.alive());
    eprintln!(
        "fixture pid={} detached child survived, reattached same process",
        fixture.pid()
    );
    terminal_exit(&mut reattached, &fixture);
}

#[test]
fn disconnect_from_old_owner_does_not_reap_reattached_child() {
    let (fixture, mut original) = Fixture::open();
    original.detach("owner_detach").unwrap();
    wait_for(|| {
        while let Ok(frame) = original.try_recv_frame() {
            if matches!(frame, ControlFrame::PtyDetached(_)) {
                return Some(());
            }
        }
        None
    })
    .expect("original owner must detach");

    let mut reattached = attach_active_pty_session(PtyAttachRequest {
        session_id: original.session_id().to_owned(),
        cols: 80,
        rows: 24,
    })
    .unwrap()
    .expect("same PTY must attach to a new owner");
    original.disconnect();
    reattached
        .send_stdin_bytes(b"fixture-check\n".to_vec())
        .unwrap();
    wait_for(|| fixture.dir.join("checked").is_file().then_some(()))
        .expect("the same child must process input after its old owner's disconnect");
    assert!(fixture.alive());

    reattached.disconnect();
    disconnected_exit(&mut reattached, &fixture);
}

#[test]
fn ordinary_nonzero_exit_keeps_explicit_terminal_frame() {
    let (fixture, mut session) = Fixture::open();
    terminal_exit(&mut session, &fixture);
}
