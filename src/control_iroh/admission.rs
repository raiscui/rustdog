//! Local Iroh session admission.
//!
//! One registered public key per daemon (H1=B), a local allowlist as the only
//! policy authority (H2=A), and a revocation order that persists the marker
//! before it refuses new sessions and only then, for an explicit emergency,
//! terminates the sessions that already exist (H3=C).
//!
//! The semantics live here once: the daemon serves sessions with them and the
//! isolation matrix exercises the same types.

use super::{DENIED, DENIED_CODE};
use iroh::{EndpointId, Signature};
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

#[cfg(all(test, target_os = "linux"))]
mod tests;

pub(crate) const MAX_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
pub(crate) const MIN_SESSION_TTL: Duration = Duration::from_secs(1);
/// Grants may be issued by a host whose clock runs ahead of this one, so a small
/// future-dated issue is tolerated instead of failing closed on every request.
pub(crate) const CLOCK_SKEW: Duration = Duration::from_secs(30);
pub(crate) const EXPIRED: (u32, &str) = (45, "lease_expired");
pub(crate) const REVOKED_REASON: &str = "lease_revoked";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Claims {
    pub(crate) issuer: EndpointId,
    pub(crate) target: EndpointId,
    pub(crate) session: String,
    pub(crate) issued_ms: u64,
    pub(crate) expires_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignedGrant {
    pub(crate) claims: Claims,
    pub(crate) signature: Signature,
}

impl Claims {
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = b"rdog/isolated-admission/1\0".to_vec();
        bytes.extend(serde_json::to_vec(self).expect("serialize isolated claims"));
        bytes
    }
}

impl SignedGrant {
    pub(crate) fn issue(
        issuer: &iroh::SecretKey,
        target: EndpointId,
        session: &str,
        now_ms: u64,
        ttl: Duration,
    ) -> AdmissionResult<Self> {
        if ttl < MIN_SESSION_TTL || ttl > MAX_SESSION_TTL {
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
pub(crate) enum Rejection {
    Denied,
    Expired,
}

impl Rejection {
    pub(crate) fn code_reason(&self) -> (u32, &'static str) {
        match self {
            Self::Denied => (DENIED_CODE, DENIED),
            Self::Expired => EXPIRED,
        }
    }
}

pub(crate) type AdmissionResult<T> = Result<T, Rejection>;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) allow: BTreeSet<EndpointId>,
    pub(crate) revoked: BTreeSet<EndpointId>,
    #[serde(default)]
    pub(crate) terminate: BTreeSet<EndpointId>,
    /// Who each admitted party is: a note and when it was registered. The
    /// admission decision itself only reads the three sets above.
    #[serde(default)]
    pub(crate) parties: Vec<Party>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Party {
    pub(crate) id: EndpointId,
    #[serde(default)]
    pub(crate) note: String,
    #[serde(default)]
    pub(crate) registered_ms: u64,
}

impl Policy {
    pub(crate) fn party(&mut self, id: EndpointId) -> &mut Party {
        if let Some(index) = self.parties.iter().position(|party| party.id == id) {
            return &mut self.parties[index];
        }
        self.parties.push(Party { id, note: String::new(), registered_ms: 0 });
        self.parties.last_mut().expect("party just pushed")
    }

    pub(crate) fn note(&mut self, id: EndpointId, note: &str, now_ms: u64) {
        let party = self.party(id);
        if party.registered_ms == 0 {
            party.registered_ms = now_ms;
        }
        if !note.is_empty() {
            party.note = note.to_owned();
        }
    }
}

#[derive(Debug)]
pub(crate) enum PersistenceError {
    NotWritten(io::Error),
    DurabilityUnconfirmed(io::Error),
}

pub(crate) fn save_policy(path: &Path, policy: &Policy) -> Result<(), PersistenceError> {
    save_policy_with(path, policy, |parent| File::open(parent)?.sync_all())
}

pub(crate) fn save_policy_with(
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeaseState {
    Granted,
    Live,
    TerminationPending,
    Finished,
}

struct Lease {
    peer: EndpointId,
    expires_ms: u64,
    state: LeaseState,
    pty: Option<String>,
}

/// A live session the daemon has to close, with the reason the terminal frame
/// must carry.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Termination {
    pub(crate) pty: String,
    pub(crate) reason: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Revocation {
    Graceful,
    Emergency,
}

impl Revocation {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Graceful => "lease_expired",
            Self::Emergency => REVOKED_REASON,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RevocationProgress {
    Persisted,
    TerminationPending(Vec<Uuid>),
}

pub(crate) struct Admission {
    path: PathBuf,
    target: EndpointId,
    leases: HashMap<Uuid, Lease>,
    durability_unconfirmed: bool,
}

impl Admission {
    pub(crate) fn new(path: PathBuf, target: EndpointId) -> Self {
        Self {
            path,
            target,
            leases: HashMap::new(),
            durability_unconfirmed: false,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn policy(&self) -> io::Result<Policy> {
        Ok(serde_json::from_reader(File::open(&self.path)?)?)
    }

    pub(crate) fn validate(
        &self,
        peer: EndpointId,
        grant: &SignedGrant,
        now_ms: u64,
    ) -> AdmissionResult<()> {
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
            || claims.issued_ms > now_ms.saturating_add(CLOCK_SKEW.as_millis() as u64)
            || claims.expires_ms <= now_ms
            || length == 0
            || u128::from(length) > MAX_SESSION_TTL.as_millis()
            || u128::from(length) < MIN_SESSION_TTL.as_millis()
        {
            return Err(Rejection::Denied);
        }
        peer.verify(&claims.bytes(), &grant.signature)
            .map_err(|_| Rejection::Denied)
    }

    /// The one-shot ping on the admitted lane follows the same local policy:
    /// the policy file, not the transport allowlist, is its authority.
    pub(crate) fn admits_ping(&self, peer: EndpointId) -> bool {
        self.policy()
            .map(|policy| {
                !self.durability_unconfirmed
                    && policy.allow.contains(&peer)
                    && !policy.revoked.contains(&peer)
                    && !policy.terminate.contains(&peer)
            })
            .unwrap_or(false)
    }

    pub(crate) fn admit(
        &mut self,
        peer: EndpointId,
        grant: &SignedGrant,
        now_ms: u64,
    ) -> AdmissionResult<()> {
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
                pty: None,
            },
        );
        Ok(())
    }

    pub(crate) fn lease_peer(&self, id: &str) -> Option<EndpointId> {
        self.leases
            .get(&Uuid::parse_str(id).ok()?)
            .map(|lease| lease.peer)
    }

    pub(crate) fn active(&self, peer: EndpointId, id: &str, now_ms: u64) -> AdmissionResult<()> {
        let lease = Uuid::parse_str(id)
            .ok()
            .and_then(|id| self.leases.get(&id))
            .ok_or(Rejection::Denied)?;
        if lease.peer != peer || matches!(lease.state, LeaseState::TerminationPending) {
            return Err(Rejection::Denied);
        }
        if lease.expires_ms <= now_ms {
            return Err(Rejection::Expired);
        }
        if matches!(lease.state, LeaseState::Finished) {
            return Err(Rejection::Denied);
        }
        Ok(())
    }

    pub(crate) fn renew(
        &mut self,
        peer: EndpointId,
        grant: &SignedGrant,
        now_ms: u64,
    ) -> AdmissionResult<()> {
        let id = &grant.claims.session;
        let lease = Uuid::parse_str(id)
            .ok()
            .and_then(|id| self.leases.get(&id))
            .ok_or(Rejection::Denied)?;
        if grant.claims.expires_ms <= lease.expires_ms {
            return Err(Rejection::Denied);
        }
        self.active(peer, id, now_ms)?;
        self.validate(peer, grant, now_ms)?;
        self.leases
            .get_mut(&Uuid::parse_str(id).unwrap())
            .expect("validated lease")
            .expires_ms = grant.claims.expires_ms;
        Ok(())
    }

    /// Binds the PTY session a lease opened. The daemon reclaims that session
    /// when the lease runs out or its peer is revoked.
    pub(crate) fn record_session(&mut self, id: &str, pty: String) {
        if let Some(lease) = self.leases.get_mut(&Uuid::parse_str(id).unwrap()) {
            lease.state = LeaseState::Live;
            lease.pty = Some(pty);
        }
    }

    /// The PTY session this lease already holds, if it holds one.
    pub(crate) fn session_of(&self, id: &str) -> Option<String> {
        let id = Uuid::parse_str(id).ok()?;
        self.leases.get(&id)?.pty.clone()
    }

    pub(crate) fn confirm_terminal(&mut self, id: &str) {
        if let Some(lease) = self.leases.get_mut(&Uuid::parse_str(id).unwrap()) {
            lease.state = LeaseState::Finished;
            lease.pty = None;
        }
    }

    pub(crate) fn revoke(
        &mut self,
        peer: EndpointId,
        mode: Revocation,
        now_ms: impl FnOnce() -> u64,
    ) -> Result<RevocationProgress, PersistenceError> {
        self.revoke_with(peer, mode, now_ms, save_policy)
    }

    pub(crate) fn revoke_with(
        &mut self,
        peer: EndpointId,
        mode: Revocation,
        now_ms: impl FnOnce() -> u64,
        save: impl FnOnce(&Path, &Policy) -> Result<(), PersistenceError>,
    ) -> Result<RevocationProgress, PersistenceError> {
        let mut policy = self.policy().map_err(PersistenceError::NotWritten)?;
        policy.revoked.insert(peer);
        if matches!(mode, Revocation::Emergency) {
            policy.terminate.insert(peer);
        }
        match save(&self.path, &policy) {
            Ok(()) => self.durability_unconfirmed = false,
            Err(err) => {
                self.durability_unconfirmed |=
                    matches!(err, PersistenceError::DurabilityUnconfirmed(_));
                return Err(err);
            }
        }
        let pending = if matches!(mode, Revocation::Emergency) {
            self.select(now_ms(), &BTreeSet::from([peer]))
        } else {
            Vec::new()
        };
        Ok(if pending.is_empty() {
            RevocationProgress::Persisted
        } else {
            RevocationProgress::TerminationPending(pending)
        })
    }

    /// Marks each live, unexpired lease of `peers` as pending termination and
    /// returns its id. A lease that already reached its deadline stays on the
    /// expiry path.
    fn select(&mut self, now_ms: u64, peers: &BTreeSet<EndpointId>) -> Vec<Uuid> {
        let mut pending = Vec::new();
        for (id, lease) in &mut self.leases {
            if !peers.contains(&lease.peer) {
                continue;
            }
            match lease.state {
                LeaseState::Live if lease.expires_ms > now_ms => {
                    lease.state = LeaseState::TerminationPending;
                    pending.push(*id);
                }
                LeaseState::TerminationPending => pending.push(*id),
                _ => {}
            }
        }
        pending
    }

    /// One sweep under a single lock: expire due leases, apply a persisted
    /// emergency marker, and resend closes that have not been confirmed yet.
    /// `is_active` reports whether the PTY session is still registered.
    pub(crate) fn sweep(
        &mut self,
        now_ms: u64,
        is_active: impl Fn(&str) -> bool,
    ) -> Vec<Termination> {
        let terminate = self.policy().map(|policy| policy.terminate).unwrap_or_default();
        let mut work: Vec<(Uuid, Termination)> = Vec::new();
        let mut confirmed = Vec::new();
        for (id, lease) in &mut self.leases {
            let Some(pty) = lease.pty.clone() else {
                if matches!(lease.state, LeaseState::Live) && lease.expires_ms <= now_ms {
                    lease.state = LeaseState::Finished;
                }
                continue;
            };
            let reason = match lease.state {
                LeaseState::Live if lease.expires_ms <= now_ms => Some(EXPIRED.1),
                LeaseState::Live if terminate.contains(&lease.peer) => Some(REVOKED_REASON),
                LeaseState::Live => None,
                LeaseState::TerminationPending => Some(REVOKED_REASON),
                LeaseState::Granted | LeaseState::Finished => None,
            };
            if let Some(reason) = reason {
                if !matches!(lease.state, LeaseState::TerminationPending) {
                    lease.state = LeaseState::TerminationPending;
                }
                if is_active(&pty) {
                    work.push((*id, Termination { pty, reason }));
                } else {
                    confirmed.push(*id);
                }
            }
        }
        for id in confirmed {
            self.confirm_terminal(&id.to_string());
        }
        work.into_iter().map(|(_, work)| work).collect()
    }
}
