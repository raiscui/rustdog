//! line-control @record-* parser round-trip tests (TDD).

use super::protocol::parse_record_start_payload;
use super::control_handler::RecordRequest;
use super::session::Profile;

#[test]
fn start_without_payload_uses_semantic_default_and_no_duration() {
    let request = parse_record_start_payload("").expect("empty payload ok");
    match request {
        RecordRequest::Start { profile, duration_ms } => {
            assert_eq!(profile, Profile::Semantic);
            assert_eq!(duration_ms, None);
        }
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn start_with_duration_ms_parses_some() {
    let request = parse_record_start_payload(
        r#"{"profile":"physical","duration_ms":90000}"#,
    )
    .expect("parse ok");
    match request {
        RecordRequest::Start { profile, duration_ms } => {
            assert_eq!(profile, Profile::Physical);
            assert_eq!(duration_ms, Some(90_000));
        }
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn start_with_duration_ms_zero_accepted() {
    let request = parse_record_start_payload(r#"{"duration_ms":0}"#).expect("parse ok");
    match request {
        RecordRequest::Start { duration_ms, .. } => assert_eq!(duration_ms, Some(0)),
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn start_without_duration_ms_field_parses_none() {
    let request =
        parse_record_start_payload(r#"{"profile":"semantic"}"#).expect("parse ok");
    match request {
        RecordRequest::Start { duration_ms, .. } => assert_eq!(duration_ms, None),
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn start_with_negative_duration_ms_rejected() {
    let result = parse_record_start_payload(r#"{"duration_ms":-1}"#);
    assert!(result.is_err(), "negative u64 should be rejected");
}
