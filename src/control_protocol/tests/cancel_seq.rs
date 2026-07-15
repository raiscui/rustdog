use super::*;

#[test]
fn parse_should_accept_cancel_seq_with_target_seq() {
    let result = parse_control_line("@cancel#seq#5:{target_seq:1}").unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(5),
            command: ControlCommand::Cancel(CancelRequest { target_seq: 1 }),
        })
    );
}

#[test]
fn parse_should_accept_cancel_seq_with_self_target() {
    // 自杀场景: target_seq == cancel command 自己的 seq,通常无意义但合法
    let result = parse_control_line("@cancel#seq#7:{target_seq:7}").unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(7),
            command: ControlCommand::Cancel(CancelRequest { target_seq: 7 }),
        })
    );
}

#[test]
fn parse_should_reject_cancel_seq_without_target_seq_field() {
    let err = parse_control_line("@cancel#seq#5:{}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_cancel_seq_with_non_object_payload() {
    let err = parse_control_line("@cancel#seq#5:\"1\"").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_cancel_seq_with_negative_target_seq() {
    let err = parse_control_line("@cancel#seq#5:{target_seq:-1}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_cancel_seq_with_non_numeric_target_seq() {
    let err =
        parse_control_line("@cancel#seq#5:{target_seq:\"abc\"}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_cancel_seq_with_duplicate_target_seq_field() {
    let err = parse_control_line(
        "@cancel#seq#5:{target_seq:1,target_seq:2}",
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_cancel_seq_with_unknown_field() {
    let err =
        parse_control_line("@cancel#seq#5:{target_seq:1,foo:\"bar\"}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
