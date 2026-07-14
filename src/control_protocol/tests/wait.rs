use super::*;

#[test]
fn parse_should_accept_wait_with_duration_ms() {
    let result = parse_control_line("@wait#1:{duration_ms:100}").unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(1),
            command: ControlCommand::Wait(WaitRequest { duration_ms: 100 }),
        })
    );
}

#[test]
fn parse_should_accept_wait_with_zero_duration() {
    // 0 is allowed — returns immediately.
    let result = parse_control_line("@wait#2:{duration_ms:0}").unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(2),
            command: ControlCommand::Wait(WaitRequest { duration_ms: 0 }),
        })
    );
}

#[test]
fn parse_should_reject_wait_without_duration_ms_field() {
    let err = parse_control_line("@wait#1:{}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_wait_with_negative_duration() {
    let err = parse_control_line("@wait#1:{duration_ms:-1}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_wait_with_non_numeric_duration() {
    let err = parse_control_line("@wait#1:{duration_ms:\"abc\"}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_wait_with_non_object_payload() {
    let err = parse_control_line("@wait#1:\"100\"").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_wait_with_duplicate_duration_ms_field() {
    let err = parse_control_line("@wait#1:{duration_ms:100,duration_ms:200}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
