use super::*;

#[test]
fn parse_should_accept_minimal_computer_act_request() {
    let result = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}"#,
    )
    .unwrap();
    let request_id = Some(1);
    let request = match result {
        ControlParseResult::Control(req) => req,
        _ => panic!("expected Control result"),
    };
    assert_eq!(request.request_id, request_id);
    match request.command {
        ControlCommand::ComputerAct(req) => {
            assert_eq!(req.schema, "rdog.computer-act.v1");
            assert_eq!(req.action, "wait");
            assert_eq!(req.args.get("duration_ms").and_then(|v| v.as_u64()), Some(100));
            assert!(req.verify.is_none());
            assert!(req.observation_id.is_none());
            assert!(req.timeout_ms.is_none());
            assert!(req.trace.is_none());
        }
        _ => panic!("expected ComputerAct command"),
    }
}

#[test]
fn parse_should_accept_computer_act_with_all_optional_fields() {
    let result = parse_control_line(
        r#"@computer-act#2:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[100,200]},verify:"best_effort",observation_id:"obs-123",timeout_ms:5000,trace:"savefile"}"#,
    )
    .unwrap();
    let request = match result {
        ControlParseResult::Control(req) => req,
        _ => panic!("expected Control result"),
    };
    match request.command {
        ControlCommand::ComputerAct(req) => {
            assert_eq!(req.action, "click");
            assert_eq!(req.verify.as_deref(), Some("best_effort"));
            assert_eq!(req.observation_id.as_deref(), Some("obs-123"));
            assert_eq!(req.timeout_ms, Some(5000));
            assert_eq!(req.trace.as_deref(), Some("savefile"));
        }
        _ => panic!("expected ComputerAct command"),
    }
}

#[test]
fn parse_should_reject_computer_act_missing_schema() {
    let err = parse_control_line(
        r#"@computer-act#1:{action:"wait",args:{duration_ms:100}}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_wrong_schema() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v2",action:"wait",args:{duration_ms:100}}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_missing_action() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",args:{duration_ms:100}}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_missing_args() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait"}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_non_object_payload() {
    let err = parse_control_line(r#"@computer-act#1:"wait""#).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_negative_timeout_ms() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100},timeout_ms:-1}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_duplicate_schema_field() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_computer_act_unknown_top_level_field() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{},unknown_field:"x"}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
