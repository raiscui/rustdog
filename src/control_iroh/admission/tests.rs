use super::*;
use crate::control_iroh::fixtures::{grant, pid_exists, setup, FixedFixture};
use std::time::Duration;

#[test]
fn signed_claims_bind_peer_target_and_expiration() {
    let (_root, target, client, mut admission) = setup();
    let other = iroh::SecretKey::generate();
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
    admission.record_session(&id, "fixture-session".into());
    assert_eq!(
        admission.revoke(peer, Revocation::Graceful, || 0).unwrap(),
        RevocationProgress::Persisted
    );
    assert!(admission.active(peer, &id, 999).is_ok());
    let other = grant(&client, target.public(), &other_id, 1);
    assert_eq!(admission.admit(peer, &other, 1), Err(Rejection::Denied));
    assert_eq!(
        admission.renew(peer, &grant(&client, target.public(), &id, 1), 1),
        Err(Rejection::Denied)
    );
    let mut restarted = Admission::new(admission.path().to_path_buf(), target.public());
    assert_eq!(restarted.admit(peer, &other, 1), Err(Rejection::Denied));
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, || 1).unwrap(),
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
    let other = iroh::SecretKey::generate();
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
    assert!(pid_exists(pid));
    session.disconnect();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while pid_exists(pid) {
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
    admission.record_session(&id.to_string(), "fixture-session".into());
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, || 0).unwrap(),
        RevocationProgress::TerminationPending(vec![id])
    );
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, || 1).unwrap(),
        RevocationProgress::TerminationPending(vec![id]),
        "retry must keep an unconfirmed termination pending"
    );
    admission.confirm_terminal(&id.to_string());
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, || 2).unwrap(),
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
            admission.record_session(&key, format!("fixture-{key}"));
        }
        id
    };
    add(0, true);
    add(500, false);
    let closed = add(500, true);
    let live = add(500, true);
    admission.confirm_terminal(&closed.to_string());
    assert_eq!(
        admission
            .revoke(peer, Revocation::Emergency, || 1000)
            .unwrap(),
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
    admission.record_session(&id.to_string(), "fixture-session".into());
    let before = admission.revoke_with(
        peer,
        Revocation::Emergency,
        || 1,
        |path, policy| {
            let missing_dir = path.with_file_name("does-not-exist").join("policy.json");
            save_policy(&missing_dir, policy)
        },
    );
    assert!(
        matches!(before, Err(PersistenceError::NotWritten(err)) if err.kind() == io::ErrorKind::NotFound)
    );
    assert!(!admission.policy().unwrap().revoked.contains(&peer));

    let uncertain = admission.revoke_with(
        peer,
        Revocation::Emergency,
        || 2,
        |path, policy| {
            save_policy_with(path, policy, |_| {
                Err(io::Error::other("test directory sync failure"))
            })
        },
    );
    assert!(
        matches!(uncertain, Err(PersistenceError::DurabilityUnconfirmed(err)) if err.kind() == io::ErrorKind::Other)
    );
    assert!(admission.policy().unwrap().revoked.contains(&peer));
    assert!(admission.active(peer, &id.to_string(), 2).is_ok());
    let fresh = grant(&client, target.public(), &Uuid::new_v4().to_string(), 2);
    assert_eq!(admission.admit(peer, &fresh, 2), Err(Rejection::Denied));
    assert_eq!(
        admission.revoke(peer, Revocation::Emergency, || 3).unwrap(),
        RevocationProgress::TerminationPending(vec![id])
    );
}

#[test]
fn emergency_does_not_select_a_fixture_that_expires_during_persistence() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &id, 0), 0)
        .unwrap();
    admission.record_session(&id, "fixture-session".into());
    let clock = std::cell::Cell::new(999);
    let result = admission.revoke_with(
        peer,
        Revocation::Emergency,
        || clock.get(),
        |path, policy| {
            save_policy(path, policy)?;
            clock.set(1000);
            Ok(())
        },
    );
    assert_eq!(clock.get(), 1000);
    assert_eq!(
        result.unwrap(),
        RevocationProgress::Persisted,
        "a lease whose deadline passed during the write must remain on the expiry path"
    );
    assert_eq!(
        admission.active(peer, &id, clock.get()).err(),
        Some(Rejection::Expired)
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

#[test]
fn sweep_reclaims_a_detached_session_without_a_later_request() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &id, 0), 0)
        .unwrap();
    admission.record_session(&id, "detached-session".into());
    assert!(admission.sweep(999, |_| true).is_empty());
    assert_eq!(
        admission.sweep(1000, |_| true),
        vec![Termination {
            pty: "detached-session".into(),
            reason: EXPIRED.1,
        }]
    );
    assert_eq!(
        admission.sweep(1300, |_| false),
        Vec::new(),
        "a session that left the registry is confirmed instead of re-closed"
    );
    assert!(
        admission.active(peer, &id, 1300).is_err(),
        "a confirmed termination no longer admits the lease"
    );
}

#[test]
fn sweep_applies_a_persisted_emergency_marker() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &id, 0), 0)
        .unwrap();
    admission.record_session(&id, "live-session".into());
    let mut policy = admission.policy().unwrap();
    policy.revoked.insert(peer);
    save_policy(admission.path(), &policy).unwrap();
    assert!(
        admission.sweep(1, |_| true).is_empty(),
        "a graceful marker keeps the live session until its deadline"
    );
    policy.terminate.insert(peer);
    save_policy(admission.path(), &policy).unwrap();
    assert_eq!(
        admission.sweep(2, |_| true),
        vec![Termination {
            pty: "live-session".into(),
            reason: REVOKED_REASON,
        }]
    );
}

#[test]
fn an_attached_session_survives_the_lease_that_detached_it() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let first = Uuid::new_v4().to_string();
    let second = Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &first, 0), 0)
        .unwrap();
    admission.record_session(&first, "reattached-session".into());
    admission
        .admit(peer, &grant(&client, target.public(), &second, 900), 900)
        .unwrap();
    admission
        .attach_session(&second, "reattached-session", |_| true)
        .unwrap();
    assert_eq!(
        admission.session_of(&first),
        None,
        "the detached lease lets go"
    );
    assert_eq!(
        admission.session_of(&second),
        Some("reattached-session".into()),
        "the attaching lease owns the session now"
    );
    assert_eq!(
        admission.sweep(1_500, |_| true),
        Vec::new(),
        "the original deadline no longer reclaims a session that was reattached"
    );
}

#[test]
fn a_lease_cannot_take_a_second_session_and_orphan_the_first() {
    let (_root, target, client, mut admission) = setup();
    let peer = client.public();
    let id = Uuid::new_v4().to_string();
    admission
        .admit(peer, &grant(&client, target.public(), &id, 0), 0)
        .unwrap();
    admission.record_session(&id, "held-session".into());
    assert_eq!(
        admission.attach_session(&id, "other-session", |_| true),
        Err(Rejection::Denied),
        "a second session on one lease would leave the first with no reclaimer"
    );
    assert_eq!(
        admission.session_of(&id),
        Some("held-session".into()),
        "the refusal changes nothing"
    );
    assert_eq!(
        admission.attach_session(&id, "other-session", |_| false),
        Ok(()),
        "a held session that no longer exists must not block the attach"
    );
}
