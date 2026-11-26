#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Registered,
    Renewed,
    Released,
    RegistrationDenied,
    NameOccupied,
    StaleInstance,
    LookupDenied,
    NotRegistered,
    PermitUnknown,
    PermitMismatch,
    PermitExpired,
    LeaseExpired,
    CloudUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Lease(pub(super) u64);

impl Lease {
    pub(super) fn live(self, now: u64) -> bool {
        now < self.0
    }

    pub(super) fn renew(&mut self, now: u64, ttl: u64, connected: bool) -> Decision {
        if !self.live(now) {
            return Decision::LeaseExpired;
        }
        if !connected {
            return Decision::CloudUnavailable;
        }
        self.0 = self.0.max(now.saturating_add(ttl));
        Decision::Renewed
    }
}

pub(super) struct Directory<I, A> {
    pub(super) target: &'static str,
    pub(super) owner: I,
    pub(super) reader: I,
    entry: Option<Entry<A>>,
}

struct Entry<A> {
    instance: u64,
    addr: A,
    lease: Lease,
}

impl<I: Eq, A: Clone> Directory<I, A> {
    pub(super) fn new(target: &'static str, owner: I, reader: I) -> Self {
        Self {
            target,
            owner,
            reader,
            entry: None,
        }
    }

    fn expire(&mut self, now: u64) {
        if self.entry.as_ref().is_some_and(|e| !e.lease.live(now)) {
            self.entry = None;
        }
    }

    pub(super) fn register(
        &mut self,
        now: u64,
        ttl: u64,
        peer: &I,
        name: &str,
        instance: u64,
        addr: A,
    ) -> Decision {
        self.expire(now);
        if peer != &self.owner || name != self.target {
            return Decision::RegistrationDenied;
        }
        if self.entry.is_some() {
            return Decision::NameOccupied;
        }
        self.entry = Some(Entry {
            instance,
            addr,
            lease: Lease(now.saturating_add(ttl)),
        });
        Decision::Registered
    }

    pub(super) fn update(
        &mut self,
        now: u64,
        ttl: Option<u64>,
        peer: &I,
        name: &str,
        instance: u64,
    ) -> Decision {
        self.expire(now);
        if peer != &self.owner || name != self.target {
            return Decision::RegistrationDenied;
        }
        let Some(current) = self.entry.as_mut().filter(|e| e.instance == instance) else {
            return Decision::StaleInstance;
        };
        if let Some(ttl) = ttl {
            current.lease.renew(now, ttl, true)
        } else {
            self.entry = None;
            Decision::Released
        }
    }

    pub(super) fn lookup(&mut self, now: u64, peer: &I, name: &str) -> Result<A, Decision> {
        self.expire(now);
        if peer != &self.reader || name != self.target {
            return Err(Decision::LookupDenied);
        }
        self.entry
            .as_ref()
            .map(|e| e.addr.clone())
            .ok_or(Decision::NotRegistered)
    }

    pub(super) fn occupied(&mut self, now: u64) -> bool {
        self.expire(now);
        self.entry.is_some()
    }
}

pub(super) struct Permit<I> {
    pub(super) subject: I,
    pub(super) endpoint: I,
    pub(super) target: &'static str,
    pub(super) lease: Lease,
}

pub(super) fn admit<I: Eq>(
    permit: Option<&Permit<I>>,
    now: u64,
    subject: &I,
    endpoint: &I,
    name: &str,
) -> Result<Lease, Decision> {
    let permit = permit.ok_or(Decision::PermitUnknown)?;
    if &permit.subject != subject || &permit.endpoint != endpoint || permit.target != name {
        return Err(Decision::PermitMismatch);
    }
    if !permit.lease.live(now) {
        return Err(Decision::PermitExpired);
    }
    Ok(permit.lease)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_end_is_exclusive_and_cannot_be_revived() {
        let mut lease = Lease(100);
        assert!(lease.live(99));
        assert!(!lease.live(100));
        assert_eq!(lease.renew(100, 100, true), Decision::LeaseExpired);
        assert_eq!(lease, Lease(100));
    }

    #[test]
    fn renewal_requires_reachability_and_keeps_the_old_deadline_on_failure() {
        let mut lease = Lease(100);
        assert_eq!(lease.renew(90, 100, false), Decision::CloudUnavailable);
        assert_eq!(lease, Lease(100));
        assert_eq!(lease.renew(95, 100, true), Decision::Renewed);
        assert!(lease.live(100));
        assert!(!lease.live(195));
    }

    #[test]
    fn a_stale_registration_fence_cannot_delete_or_renew_its_replacement() {
        let mut d = Directory::new("lab/target", 1, 2);
        assert_eq!(
            d.register(0, 10, &1, "lab/target", 11, 111),
            Decision::Registered
        );
        assert_eq!(
            d.register(9, 10, &1, "lab/target", 22, 222),
            Decision::NameOccupied
        );
        assert_eq!(
            d.register(10, 10, &1, "lab/target", 22, 222),
            Decision::Registered
        );
        assert_eq!(
            d.update(11, Some(30), &1, "lab/target", 11),
            Decision::StaleInstance
        );
        assert_eq!(
            d.update(11, None, &1, "lab/target", 11),
            Decision::StaleInstance
        );
        assert_eq!(d.lookup(11, &2, "lab/target"), Ok(222));
        assert!(!d.occupied(20));
    }

    #[test]
    fn acl_applies_before_returning_an_address() {
        let mut d = Directory::new("lab/target", 1, 2);
        assert_eq!(
            d.register(0, 10, &3, "lab/target", 7, 111),
            Decision::RegistrationDenied
        );
        assert_eq!(
            d.register(0, 10, &1, "lab/target", 7, 111),
            Decision::Registered
        );
        assert_eq!(d.lookup(5, &3, "lab/target"), Err(Decision::LookupDenied));
        assert_eq!(d.lookup(5, &2, "lab/other"), Err(Decision::LookupDenied));
        assert_eq!(d.lookup(5, &2, "lab/target"), Ok(111));
        assert_eq!(d.lookup(10, &2, "lab/target"), Err(Decision::NotRegistered));
    }

    #[test]
    fn permits_require_both_authenticated_identities_the_exact_target_and_a_live_lease() {
        let p = Permit {
            subject: 1,
            endpoint: 2,
            target: "lab/target",
            lease: Lease(10),
        };
        assert_eq!(admit(Some(&p), 9, &1, &2, "lab/target"), Ok(Lease(10)));
        assert_eq!(
            admit(Some(&p), 9, &9, &2, "lab/target"),
            Err(Decision::PermitMismatch)
        );
        assert_eq!(
            admit(Some(&p), 9, &1, &9, "lab/target"),
            Err(Decision::PermitMismatch)
        );
        assert_eq!(
            admit(Some(&p), 9, &1, &2, "lab/other"),
            Err(Decision::PermitMismatch)
        );
        assert_eq!(
            admit(Some(&p), 10, &1, &2, "lab/target"),
            Err(Decision::PermitExpired)
        );
        assert_eq!(
            admit(None, 9, &1, &2, "lab/target"),
            Err(Decision::PermitUnknown)
        );
    }
}
