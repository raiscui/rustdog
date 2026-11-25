use super::{EndpointId, SecretKey, DENIED, DENIED_CODE};
use crate::{
    control_protocol::PtyOpenRequest,
    pty_control::{close_active_pty_session, open_attached_pty_session, AttachedPtySession},
};
use iroh::Signature;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

const MAX_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
const EXPIRED: (u32, &str) = (45, "lease_expired");

#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    issuer: EndpointId,
    target: EndpointId,
    session: String,
    issued_ms: u64,
    expires_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedGrant {
    claims: Claims,
    signature: Signature,
}

impl Claims {
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = b"rdog/isolated-admission/1\0".to_vec();
        bytes.extend(serde_json::to_vec(self).expect("serialize isolated claims"));
        bytes
    }
}

impl SignedGrant {
    fn issue(
        issuer: &SecretKey,
        target: EndpointId,
        session: &str,
        now_ms: u64,
        ttl: Duration,
    ) -> AdmissionResult<Self> {
        if ttl.is_zero() || ttl > MAX_SESSION_TTL {
            return Err(Rejection::Denied);
        }
        let lifetime = ttl.as_millis() as u64;
        if lifetime == 0 {
            return Err(Rejection::Denied);
        }
        let expires_ms = now_ms.checked_add(lifetime).ok_or(Rejection::Denied)?;
        let claims = Claims {
            issuer: issuer.public(),
            target,
            session: session.into(),
            issued_ms: now_ms,
            expires_ms,
        };
        let signature = issuer.sign(&claims.bytes());
        Ok(Self { claims, signature })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Rejection {
    Denied,
    Expired,
}

impl Rejection {
    fn code_reason(&self) -> (u32, &'static str) {
        match self {
            Self::Denied => (DENIED_CODE, DENIED),
            Self::Expired => EXPIRED,
        }
    }
}

type AdmissionResult<T> = Result<T, Rejection>;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    allow: BTreeSet<EndpointId>,
    revoked: BTreeSet<EndpointId>,
}

#[derive(Debug)]
enum PersistenceError {
    NotWritten(io::Error),
    DurabilityUnconfirmed(io::Error),
}

fn save_policy(path: &Path, policy: &Policy) -> Result<(), PersistenceError> {
    save_policy_with(path, policy, |parent| File::open(parent)?.sync_all())
}

fn save_policy_with(
    path: &Path,
    policy: &Policy,
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), PersistenceError> {
    let parent = path.parent().expect("private policy has a parent");
    let staging = parent.join(format!("policy-{}.tmp", uuid::Uuid::new_v4()));
    let replaced = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)?;
        file.write_all(&serde_json::to_vec(policy)?)?;
        file.sync_all()?;
        fs::rename(&staging, path)
    })();
    if let Err(err) = replaced {
        let _ = fs::remove_file(&staging);
        return Err(PersistenceError::NotWritten(err));
    }
    sync_parent(parent).map_err(PersistenceError::DurabilityUnconfirmed)
}

#[derive(Clone, Copy)]
enum LeaseState {
    Granted,
    Live(u32),
    TerminationPending,
    Finished,
}

struct Lease {
    peer: EndpointId,
    expires_ms: u64,
    state: LeaseState,
}

#[derive(Clone, Copy)]
enum Revocation {
    Graceful,
    Emergency,
}

#[derive(Debug, PartialEq, Eq)]
enum RevocationProgress {
    Persisted,
    TerminationPending(Vec<Uuid>),
}

struct Admission {
    path: PathBuf,
    target: EndpointId,
    leases: HashMap<Uuid, Lease>,
    durability_unconfirmed: bool,
}

impl Admission {
    fn new(path: PathBuf, target: EndpointId) -> Self {
        Self {
            path,
            target,
            leases: HashMap::new(),
            durability_unconfirmed: false,
        }
    }

    fn policy(&self) -> io::Result<Policy> {
        Ok(serde_json::from_reader(File::open(&self.path)?)?)
    }

    fn validate(&self, peer: EndpointId, grant: &SignedGrant, now_ms: u64) -> AdmissionResult<()> {
        let claims = &grant.claims;
        let policy = self.policy().map_err(|_| Rejection::Denied)?;
        let length = claims
            .expires_ms
            .checked_sub(claims.issued_ms)
            .ok_or(Rejection::Denied)?;
        if self.durability_unconfirmed
            || peer != claims.issuer
            || self.target != claims.target
            || !policy.allow.contains(&peer)
            || policy.revoked.contains(&peer)
            || uuid::Uuid::parse_str(&claims.session).is_err()
            || claims.issued_ms > now_ms
            || claims.expires_ms <= now_ms
            || length == 0
            || u128::from(length) > MAX_SESSION_TTL.as_millis()
        {
            return Err(Rejection::Denied);
        }
        peer.verify(&claims.bytes(), &grant.signature)
            .map_err(|_| Rejection::Denied)
    }

    fn admit(&mut self, peer: EndpointId, grant: &SignedGrant, now_ms: u64) -> AdmissionResult<()> {
        self.validate(peer, grant, now_ms)?;
        let id = Uuid::parse_str(&grant.claims.session).map_err(|_| Rejection::Denied)?;
        if self.leases.contains_key(&id) {
            return Err(Rejection::Denied);
        }
        self.leases.insert(
            id,
            Lease {
                peer,
                expires_ms: grant.claims.expires_ms,
                state: LeaseState::Granted,
            },
        );
        Ok(())
    }

    fn active(&self, peer: EndpointId, id: &str, now_ms: u64) -> AdmissionResult<&Lease> {
        let id = Uuid::parse_str(id).map_err(|_| Rejection::Denied)?;
        let lease = self.leases.get(&id).ok_or(Rejection::Denied)?;
        if lease.peer != peer || matches!(lease.state, LeaseState::TerminationPending) {
            return Err(Rejection::Denied);
        }
        if lease.expires_ms <= now_ms {
            return Err(Rejection::Expired);
        }
        if matches!(lease.state, LeaseState::Finished) {
            return Err(Rejection::Denied);
        }
        Ok(lease)
    }

    fn renew(&mut self, peer: EndpointId, grant: &SignedGrant, now_ms: u64) -> AdmissionResult<()> {
        let id = &grant.claims.session;
        let lease = self.active(peer, id, now_ms)?;
        if grant.claims.expires_ms <= lease.expires_ms {
            return Err(Rejection::Denied);
        }
        self.validate(peer, grant, now_ms)?;
        self.leases
            .get_mut(&Uuid::parse_str(id).unwrap())
            .expect("validated lease")
            .expires_ms = grant.claims.expires_ms;
        Ok(())
    }

    fn record_fixture(&mut self, id: &str, pid: u32) {
        self.leases
            .get_mut(&Uuid::parse_str(id).unwrap())
            .expect("fixture has a lease")
            .state = LeaseState::Live(pid);
    }

    fn confirm_terminal(&mut self, id: &str) {
        self.leases
            .get_mut(&Uuid::parse_str(id).unwrap())
            .expect("fixture has a lease")
            .state = LeaseState::Finished;
    }

    fn revoke(
        &mut self,
        peer: EndpointId,
        mode: Revocation,
        now_ms: u64,
    ) -> Result<RevocationProgress, PersistenceError> {
        self.revoke_with(peer, mode, now_ms, save_policy)
    }

    fn revoke_with(
        &mut self,
        peer: EndpointId,
        mode: Revocation,
        now_ms: u64,
        save: impl FnOnce(&Path, &Policy) -> Result<(), PersistenceError>,
    ) -> Result<RevocationProgress, PersistenceError> {
        let mut policy = self.policy().map_err(PersistenceError::NotWritten)?;
        policy.revoked.insert(peer);
        match save(&self.path, &policy) {
            Ok(()) => self.durability_unconfirmed = false,
            Err(err) => {
                self.durability_unconfirmed |=
                    matches!(err, PersistenceError::DurabilityUnconfirmed(_));
                return Err(err);
            }
        }
        let mut pending = Vec::new();
        if matches!(mode, Revocation::Emergency) {
            for (id, lease) in &mut self.leases {
                if lease.peer != peer {
                    continue;
                }
                match lease.state {
                    LeaseState::Live(pid) if lease.expires_ms > now_ms && pid_running(pid) => {
                        lease.state = LeaseState::TerminationPending;
                        pending.push(*id);
                    }
                    LeaseState::TerminationPending => pending.push(*id),
                    _ => {}
                }
            }
        }
        Ok(if pending.is_empty() {
            RevocationProgress::Persisted
        } else {
            RevocationProgress::TerminationPending(pending)
        })
    }
}

fn pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn pid_running(pid: u32) -> bool {
    fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .and_then(|(_, tail)| tail.chars().next())
        })
        .is_some_and(|state| !matches!(state, 'Z' | 'X' | 'x'))
}

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
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

struct FixedFixture {
    dir: TempRoot,
    pty_id: Option<String>,
}

impl FixedFixture {
    fn new() -> Self {
        Self {
            dir: TempRoot::new(),
            pty_id: None,
        }
    }

    fn open(
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
        self.pty_id = Some(session.session_id().into());
        Ok(session)
    }

    fn wait_pid(&self) -> u32 {
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

fn setup() -> (TempRoot, SecretKey, SecretKey, Admission) {
    let root = TempRoot::new();
    let target = SecretKey::generate();
    let client = SecretKey::generate();
    let path = root.0.join("policy.json");
    save_policy(
        &path,
        &Policy {
            allow: BTreeSet::from([client.public()]),
            revoked: BTreeSet::new(),
        },
    )
    .unwrap();
    let admission = Admission::new(path, target.public());
    (root, target, client, admission)
}

fn grant(issuer: &SecretKey, target: EndpointId, id: &str, now_ms: u64) -> SignedGrant {
    SignedGrant::issue(issuer, target, id, now_ms, Duration::from_secs(1)).unwrap()
}

#[test]
fn signed_claims_bind_peer_target_and_expiration() {
    let (_root, target, client, mut admission) = setup();
    let other = SecretKey::generate();
    let id = uuid::Uuid::new_v4().to_string();
    let proof = grant(&client, target.public(), &id, 0);
    assert_eq!(
        admission.admit(other.public(), &proof, 0),
        Err(Rejection::Denied)
    );
    assert_eq!(
        admission.admit(client.public(), &grant(&client, other.public(), &id, 0), 0),
        Err(Rejection::Denied)
    );
    let mut changed = proof.clone();
    changed.claims.expires_ms += 1;
    assert_eq!(
        admission.admit(client.public(), &changed, 0),
        Err(Rejection::Denied)
    );
    assert_eq!(
        admission.admit(client.public(), &proof, 1000),
        Err(Rejection::Denied)
    );
    assert!(SignedGrant::issue(&client, target.public(), &id, 0, MAX_SESSION_TTL).is_ok());
    assert!(SignedGrant::issue(
        &client,
        target.public(),
        &id,
        0,
        MAX_SESSION_TTL + Duration::from_millis(1)
    )
    .is_err());

    admission.admit(client.public(), &proof, 0).unwrap();
    assert_eq!(
        admission.admit(client.public(), &proof, 1),
        Err(Rejection::Denied)
    );
    let renewal = grant(&client, target.public(), &id, 250);
    admission.renew(client.public(), &renewal, 250).unwrap();
    assert!(admission.active(client.public(), &id, 1000).is_ok());
    assert_eq!(
        admission
            .active(client.public(), &id, 1250)
            .err()
            .unwrap()
            .code_reason(),
        EXPIRED
    );
    assert_eq!(
        admission.renew(
            client.public(),
            &grant(&client, target.public(), &id, 1250),
            1250
        ),
        Err(Rejection::Expired)
    );
    println!("peer and target bound; expired grant and replay rejected; renewal bounded");
}

#[test]
fn revocation_is_durable_and_modes_differ() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = uuid::Uuid::new_v4().to_string();
    let other_id = uuid::Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &id, 0), 0)
        .unwrap();
    admission.record_fixture(&id, std::process::id());
    assert_eq!(
        admission.revoke(peer, Revocation::Graceful, 0).unwrap(),
        RevocationProgress::Persisted
    );
    assert!(admission.active(peer, &id, 999).is_ok());
    let other = grant(&client, target.public(), &other_id, 1);
    assert_eq!(admission.admit(peer, &other, 1), Err(Rejection::Denied));
    assert_eq!(
        admission.renew(peer, &grant(&client, target.public(), &id, 1), 1),
        Err(Rejection::Denied)
    );
    let mut restarted = Admission::new(admission.path.clone(), target.public());
    assert_eq!(restarted.admit(peer, &other, 1), Err(Rejection::Denied));
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 1).unwrap(),
        RevocationProgress::TerminationPending(vec![Uuid::parse_str(&id).unwrap()])
    );
    assert_eq!(
        admission.active(peer, &id, 1).err(),
        Some(Rejection::Denied)
    );
    println!(
        "graceful does not notify existing lease; restart rejects; emergency selects active lease"
    );
}

#[test]
fn accepted_admission_can_open_only_the_fixed_fixture() {
    let (_root, target, client, mut admission) = setup();
    let mut fixture = FixedFixture::new();
    let other = SecretKey::generate();
    let id = uuid::Uuid::new_v4().to_string();
    let proof = grant(&client, target.public(), &id, 0);
    assert_eq!(
        fixture
            .open(&mut admission, other.public(), &proof, 0)
            .err(),
        Some(Rejection::Denied)
    );
    assert!(fixture.pty_id.is_none());
    assert!(!fixture.dir.0.join("pid").exists());
    let session = fixture
        .open(&mut admission, client.public(), &proof, 0)
        .unwrap();
    let pid = fixture.wait_pid();
    assert!(Path::new("/proc").join(pid.to_string()).exists());
    session.disconnect();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while Path::new("/proc").join(pid.to_string()).exists() {
        assert!(std::time::Instant::now() < deadline, "fixture not reaped");
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("pre-admission no fixture; accepted fixture pid={pid} reaped");
}

#[test]
fn retry_keeps_emergency_termination_pending_until_confirmed() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4();
    admission
        .admit(
            peer,
            &grant(&client, target.public(), &id.to_string(), 0),
            0,
        )
        .unwrap();
    admission.record_fixture(&id.to_string(), std::process::id());
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 0).unwrap(),
        RevocationProgress::TerminationPending(vec![id])
    );
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 1).unwrap(),
        RevocationProgress::TerminationPending(vec![id]),
        "retry must keep an unconfirmed termination pending"
    );
    admission.confirm_terminal(&id.to_string());
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 2).unwrap(),
        RevocationProgress::Persisted
    );
}

#[test]
fn only_live_unexpired_fixtures_enter_emergency_pending() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let mut add = |start, live| {
        let id = Uuid::new_v4();
        let key = id.to_string();
        admission
            .admit(peer, &grant(&client, target.public(), &key, start), start)
            .unwrap();
        if live {
            admission.record_fixture(&key, std::process::id());
        }
        id
    };
    add(0, true);
    add(500, false);
    let closed = add(500, true);
    let live = add(500, true);
    admission.confirm_terminal(&closed.to_string());
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 1000).unwrap(),
        RevocationProgress::TerminationPending(vec![live])
    );
}

#[test]
fn policy_write_reports_pre_replace_and_unconfirmed_durability() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4();
    let proof = grant(&client, target.public(), &id.to_string(), 0);
    admission.admit(peer, &proof, 0).unwrap();
    admission.record_fixture(&id.to_string(), std::process::id());
    let before = admission.revoke_with(peer, Revocation::Emergency, 1, |path, policy| {
        let missing_dir = path.with_file_name("does-not-exist").join("policy.json");
        save_policy(&missing_dir, policy)
    });
    assert!(
        matches!(before, Err(PersistenceError::NotWritten(err)) if err.kind() == io::ErrorKind::NotFound)
    );
    assert!(!admission.policy().unwrap().revoked.contains(&peer));

    let uncertain = admission.revoke_with(peer, Revocation::Emergency, 2, |path, policy| {
        save_policy_with(path, policy, |_| {
            Err(io::Error::other("test directory sync failure"))
        })
    });
    assert!(
        matches!(uncertain, Err(PersistenceError::DurabilityUnconfirmed(err)) if err.kind() == io::ErrorKind::Other)
    );
    assert!(admission.policy().unwrap().revoked.contains(&peer));
    assert!(admission.active(peer, &id.to_string(), 2).is_ok());
    let fresh = grant(&client, target.public(), &Uuid::new_v4().to_string(), 2);
    assert_eq!(admission.admit(peer, &fresh, 2), Err(Rejection::Denied));
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, 3).unwrap(),
        RevocationProgress::TerminationPending(vec![id])
    );
}

#[test]
fn admitted_id_has_one_identity_within_an_instance() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = uuid::Uuid::new_v4();
    admission
        .admit(
            peer,
            &grant(&client, target.public(), &id.to_string(), 0),
            0,
        )
        .unwrap();
    assert_eq!(
        admission.admit(
            peer,
            &grant(&client, target.public(), &id.simple().to_string(), 1),
            1
        ),
        Err(Rejection::Denied),
        "one session ID must not open a second lease in this instance"
    );
}
