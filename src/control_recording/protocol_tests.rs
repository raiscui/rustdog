use super::{
    control_handler::{MarkKind, RecordRequest},
    protocol::{
        parse_record_mark_payload, parse_record_start_payload, parse_record_status_payload,
        parse_record_stop_payload,
    },
    session::Profile,
};

#[test]
fn start_requires_typed_targets() {
    assert!(parse_record_start_payload("{}").is_err());
    let request = parse_record_start_payload(
        r#"{"profile":"physical","duration_ms":1000,"targets":[{"pid":7,"hwnd":9}]}"#,
    )
    .unwrap();
    match request {
        RecordRequest::Start {
            profile,
            duration_ms,
            targets,
        } => {
            assert_eq!(profile, Profile::Physical);
            assert_eq!(duration_ms, Some(1000));
            assert_eq!(targets[0].hwnd, 9);
        }
        _ => panic!("expected start"),
    }
}
#[test]
fn mutation_requires_id_and_limited_mark() {
    assert!(parse_record_stop_payload("{}").is_err());
    assert!(parse_record_mark_payload(r#"{"recording_id":"r","mark":"private text"}"#).is_err());
    assert!(matches!(
        parse_record_mark_payload(r#"{"recording_id":"r","mark":"checkpoint"}"#).unwrap(),
        RecordRequest::Mark {
            mark: MarkKind::Checkpoint,
            ..
        }
    ));
}
#[test]
fn status_renews_by_recording_id_and_rejects_payload_authentication() {
    assert!(matches!(
        parse_record_status_payload(r#"{"recording_id":"r"}"#).unwrap(),
        RecordRequest::Status {
            recording_id: Some(_),
        }
    ));
    assert!(parse_record_status_payload(r#"{"recording_id":"r","owner_nonce":"secret"}"#).is_err());
}
#[test]
fn capture_deserialization_rejects_unknown_nested_fields() {
    use super::model::{CaptureEvent, CaptureSpec, CaptureStop};
    assert!(
        parse_record_start_payload(r#"{"targets":[{"pid":7,"hwnd":9,"text":"secret"}]}"#).is_err()
    );
    assert!(serde_json::from_str::<CaptureSpec>(
        r#"{"targets":[],"semantic":false,"raw":"secret"}"#
    )
    .is_err());
    assert!(
        serde_json::from_str::<CaptureStop>(r#"{"last_sequence":null,"raw":"secret"}"#).is_err()
    );
    let event = r#"{"sequence":0,"monotonic_ns":0,"input":{"type":"mouse_move","window":{"pid":1,"hwnd":2,"process_start":3,"generation":4},"x":1,"y":2,"injected":false}}"#;
    assert!(serde_json::from_str::<CaptureEvent>(event).is_ok());
    for (from, to) in [
        (r#""sequence":0"#, r#""sequence":0,"raw":"secret""#),
        (r#""x":1"#, r#""x":1,"text":"secret""#),
        (r#""generation":4"#, r#""generation":4,"raw":"secret""#),
    ] {
        assert!(serde_json::from_str::<CaptureEvent>(&event.replace(from, to)).is_err());
    }
    let rect = r#"{"sequence":0,"monotonic_ns":0,"input":{"type":"window_context","window":{"pid":1,"hwnd":2,"process_start":3,"generation":4},"rect":{"x":0,"y":0,"width":1,"height":1,"raw":"secret"},"dpi":96,"focused":true}}"#;
    assert!(serde_json::from_str::<CaptureEvent>(rect).is_err());
    let display = r#"{"sequence":0,"monotonic_ns":0,"input":{"type":"display_context","generation":1,"displays":[{"id":1,"rect":{"x":0,"y":0,"width":1920,"height":1080},"primary":true,"name":"secret"}]}}"#;
    assert!(serde_json::from_str::<CaptureEvent>(display).is_err());
}
