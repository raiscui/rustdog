//! Shared fixtures for the isolated admission and lifecycle tests.

use super::admission::{save_policy, Admission, AdmissionResult, Policy, Rejection, SignedGrant};
use crate::{
    control_protocol::PtyOpenRequest,
    pty_control::{close_active_pty_session, open_attached_pty_session, AttachedPtySession},
};
use iroh::{EndpointId, SecretKey};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) fn pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

pub(super) fn pid_running(pid: u32) -> bool {
    fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .and_then(|(_, tail)| tail.chars().next())
        })
        .is_some_and(|state| !matches!(state, 'Z' | 'X' | 'x'))
}

pub(super) struct TempRoot(pub(super) PathBuf);

impl TempRoot {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!("rdog-admission-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).expect("create isolated test dir");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct FixedFixture {
    pub(super) dir: TempRoot,
    pub(super) pty_id: Option<String>,
}

impl FixedFixture {
    pub(super) fn new() -> Self {
        Self {
            dir: TempRoot::new(),
            pty_id: None,
        }
    }

    pub(super) fn open(
        &mut self,
        admission: &mut Admission,
        peer: EndpointId,
        grant: &SignedGrant,
        now_ms: u64,
    ) -> AdmissionResult<AttachedPtySession> {
        if self.pty_id.is_some() {
            return Err(Rejection::Denied);
        }
        admission.admit(peer, grant, now_ms)?;
        let program = "import os, sys\nfrom pathlib import Path\nassert os.isatty(0)\nPath(sys.argv[1]).write_text(str(os.getpid()))\nfor line in sys.stdin:\n    if line == 'sample\\n': print('fixture-ack', flush=True)\n";
        let session = open_attached_pty_session(PtyOpenRequest {
            cmd: "python3".into(),
            args: vec![
                "-c".into(),
                program.into(),
                self.dir.0.join("pid").display().to_string(),
            ],
            cols: 80,
            rows: 24,
        })
        .expect("start fixed Python PTY fixture");
        admission.record_session(&grant.claims.session, session.session_id().into());
        self.pty_id = Some(session.session_id().into());
        Ok(session)
    }

    pub(super) fn wait_pid(&self) -> u32 {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(pid) = fs::read_to_string(self.dir.0.join("pid"))
                .ok()
                .and_then(|raw| raw.parse().ok())
            {
                return pid;
            }
            assert!(std::time::Instant::now() < deadline, "fixture PID missing");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for FixedFixture {
    fn drop(&mut self) {
        if let Some(id) = &self.pty_id {
            let _ = close_active_pty_session(id);
        }
    }
}

pub(super) fn setup() -> (TempRoot, SecretKey, SecretKey, Admission) {
    let root = TempRoot::new();
    let target = SecretKey::generate();
    let client = SecretKey::generate();
    let path = root.0.join("policy.json");
    save_policy(
        &path,
        &Policy {
            allow: BTreeSet::from([client.public()]),
            ..Policy::default()
        },
    )
    .unwrap();
    let admission = Admission::new(path, target.public());
    (root, target, client, admission)
}

pub(super) fn grant(
    issuer: &SecretKey,
    target: EndpointId,
    id: &str,
    now_ms: u64,
) -> SignedGrant {
    SignedGrant::issue(issuer, target, id, now_ms, Duration::from_secs(1)).unwrap()
}
