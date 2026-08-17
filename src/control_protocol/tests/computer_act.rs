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
            assert_eq!(
                req.args.get("duration_ms").and_then(|v| v.as_u64()),
                Some(100)
            );
            assert!(req.verify.is_none());
            assert!(req.postcondition.is_none());
            assert!(req.observation_id.is_none());
            assert!(req.timeout_ms.is_none());
            assert!(req.trace.is_none());
        }
        _ => panic!("expected ComputerAct command"),
    }
}

#[test]
fn parse_should_accept_exists_postcondition() {
    let result = parse_control_line(
        r#"@computer-act#3:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[1,2]},postcondition:{kind:"exists",query:{role:"AXStaticText",value:"42"}}}"#,
    )
    .expect("exists postcondition should parse");
    let request = match result {
        ControlParseResult::Control(request) => request,
        _ => panic!("expected Control result"),
    };
    let ControlCommand::ComputerAct(request) = request.command else {
        panic!("expected ComputerAct command");
    };
    let condition = request.postcondition.expect("postcondition should exist");
    assert_eq!(condition.kind, ComputerActPostconditionKind::Exists);
    assert_eq!(condition.query.role.as_deref(), Some("AXStaticText"));
    assert_eq!(condition.query.value.as_deref(), Some("42"));
}

#[test]
fn parse_should_accept_not_exists_postcondition() {
    let result = parse_control_line(
        r#"@computer-act#4:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[1,2]},postcondition:{kind:"not_exists",query:{name_contains:"Loading"}}}"#,
    )
    .expect("not_exists postcondition should parse");
    let request = match result {
        ControlParseResult::Control(request) => request,
        _ => panic!("expected Control result"),
    };
    let ControlCommand::ComputerAct(request) = request.command else {
        panic!("expected ComputerAct command");
    };
    let condition = request.postcondition.expect("postcondition should exist");
    assert_eq!(condition.kind, ComputerActPostconditionKind::NotExists);
    assert_eq!(condition.query.name_contains.as_deref(), Some("Loading"));
}

#[test]
fn parse_should_reject_invalid_postconditions() {
    for payload in [
        r#"{kind:"changed",query:{role:"AXButton"}}"#,
        r#"{kind:"exists",query:{}}"#,
        r#"{kind:"exists",query:{unknown:"x"}}"#,
    ] {
        let line = format!(
            r#"@computer-act#5:{{schema:"rdog.computer-act.v1",action:"click",args:{{start_box:[1,2]}},postcondition:{payload}}}"#
        );
        assert!(
            parse_control_line(&line).is_err(),
            "should reject {payload}"
        );
    }
}

#[test]
fn parse_should_reject_duplicate_postcondition() {
    let err = parse_control_line(
        r#"@computer-act#6:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[1,2]},postcondition:{kind:"exists",query:{role:"AXButton"}},postcondition:{kind:"not_exists",query:{role:"AXButton"}}}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("postcondition"));
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
    let err = parse_control_line(r#"@computer-act#1:{action:"wait",args:{duration_ms:100}}"#)
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
    let err =
        parse_control_line(r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait"}"#)
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

// --- epoch (feature/observe-epoch-stale-reject) ---

#[test]
fn parse_should_accept_computer_act_epoch() {
    let result = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100},epoch:1700000000000}"#,
    )
    .expect("epoch should be accepted as optional field");
    let request = match result {
        ControlParseResult::Control(req) => req,
        _ => panic!("expected Control result"),
    };
    match request.command {
        ControlCommand::ComputerAct(req) => {
            assert_eq!(req.epoch, Some(1700000000000));
            assert_eq!(req.action, "wait");
        }
        other => panic!("expected ComputerAct, got {other:?}"),
    }
}

#[test]
fn parse_should_accept_computer_act_without_epoch() {
    let result = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}"#,
    )
    .expect("missing epoch should not break parsing");
    let request = match result {
        ControlParseResult::Control(req) => req,
        _ => panic!("expected Control result"),
    };
    match request.command {
        ControlCommand::ComputerAct(req) => {
            assert_eq!(req.epoch, None);
        }
        other => panic!("expected ComputerAct, got {other:?}"),
    }
}

#[test]
fn parse_should_reject_computer_act_negative_epoch() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100},epoch:-1}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("epoch"));
}

#[test]
fn parse_should_reject_computer_act_non_integer_epoch() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100},epoch:"abc"}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("epoch"));
}

#[test]
fn parse_should_reject_computer_act_duplicate_epoch_field() {
    let err = parse_control_line(
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100},epoch:100,epoch:200}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
