use super::*;

#[test]
fn parse_should_accept_open_app_with_app_name() {
    let result = parse_control_line(r#"@open-app#1:{app_name:"Calculator"}"#).unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(1),
            command: ControlCommand::OpenApp(OpenAppRequest {
                app_name: "Calculator".to_string(),
                wait_ms: 1500, // default when not specified
            }),
        })
    );
}

#[test]
fn parse_should_accept_open_app_with_explicit_wait_ms() {
    let result = parse_control_line(r#"@open-app#2:{app_name:"Xcode",wait_ms:5000}"#).unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(2),
            command: ControlCommand::OpenApp(OpenAppRequest {
                app_name: "Xcode".to_string(),
                wait_ms: 5000,
            }),
        })
    );
}

#[test]
fn parse_should_accept_open_app_with_zero_wait_ms() {
    // wait_ms == 0 是合法值 (立即返回, 不等启动完成)
    let result = parse_control_line(r#"@open-app#3:{app_name:"Notes",wait_ms:0}"#).unwrap();
    assert_eq!(
        result,
        ControlParseResult::Control(ControlRequest {
            request_id: Some(3),
            command: ControlCommand::OpenApp(OpenAppRequest {
                app_name: "Notes".to_string(),
                wait_ms: 0,
            }),
        })
    );
}

#[test]
fn parse_should_reject_open_app_without_app_name_field() {
    let err = parse_control_line("@open-app#1:{}").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_open_app_with_non_object_payload() {
    let err = parse_control_line(r#"@open-app#1:"Calculator""#).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_open_app_with_negative_wait_ms() {
    let err =
        parse_control_line(r#"@open-app#1:{app_name:"X",wait_ms:-1}"#).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_open_app_with_duplicate_app_name_field() {
    let err = parse_control_line(
        r#"@open-app#1:{app_name:"X",app_name:"Y"}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn parse_should_reject_open_app_with_unknown_field() {
    let err = parse_control_line(
        r#"@open-app#1:{app_name:"X",foo:"bar"}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
