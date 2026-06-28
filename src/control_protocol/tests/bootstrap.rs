use super::*;
use crate::{
    control_bootstrap::{
        BootstrapCapabilityPolicy, BootstrapMode, BootstrapRequest,
        BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED,
    },
    control_observation::observe::ObserveMode,
};

#[test]
fn parse_should_support_bare_bootstrap_default() {
    assert_eq!(
        parse_control_line("@bootstrap").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Bootstrap(BootstrapRequest::default()),
        })
    );

    assert_eq!(
        parse_control_line("@bootstrap#17").unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(17),
            command: ControlCommand::Bootstrap(BootstrapRequest::default()),
        })
    );
}

#[test]
fn parse_should_support_gui_bootstrap_request() {
    assert_eq!(
        parse_control_line(r#"@bootstrap#1:{mode:"gui"}"#).unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: Some(1),
            command: ControlCommand::Bootstrap(BootstrapRequest {
                mode: BootstrapMode::Gui,
                capability_policy: BootstrapCapabilityPolicy::Fresh,
                observe: None,
                include_trace: true,
            }),
        })
    );
}

#[test]
fn parse_should_support_gui_bootstrap_observe_override() {
    let parsed =
        parse_control_line(r#"@bootstrap#2:{mode:"gui",observe:{mode:"window"}}"#).unwrap();
    let ControlParseResult::Control(ControlRequest {
        request_id: Some(2),
        command: ControlCommand::Bootstrap(request),
    }) = parsed
    else {
        panic!("expected @bootstrap control request");
    };

    let observe = request.observe.expect("gui observe override should parse");
    assert_eq!(request.mode, BootstrapMode::Gui);
    assert_eq!(request.capability_policy, BootstrapCapabilityPolicy::Fresh);
    assert_eq!(observe.mode, ObserveMode::Window);
    assert!(!observe.include_screenshot);
    assert!(!observe.include_ax);
    assert!(observe.include_windows);
}

#[test]
fn parse_should_support_gui_bootstrap_nested_observe_display_scope() {
    let parsed = parse_control_line(
        r#"@bootstrap#3:{mode:"gui",observe:{mode:"hybrid",scope:{display:{id:"d2"}}}}"#,
    )
    .unwrap();
    let ControlParseResult::Control(ControlRequest {
        request_id: Some(3),
        command: ControlCommand::Bootstrap(request),
    }) = parsed
    else {
        panic!("expected @bootstrap control request");
    };

    let observe = request.observe.expect("nested observe should parse");
    assert_eq!(request.mode, BootstrapMode::Gui);
    assert!(observe.display_scope.is_some());
}

#[test]
fn parse_should_support_bootstrap_policy_and_trace_fields() {
    assert_eq!(
        parse_control_line(
            r#"@bootstrap:{mode:"gui",capability_policy:"fresh",include_trace:false}"#
        )
        .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Bootstrap(BootstrapRequest {
                mode: BootstrapMode::Gui,
                capability_policy: BootstrapCapabilityPolicy::Fresh,
                observe: None,
                include_trace: false,
            }),
        })
    );
}

#[test]
fn parse_should_reject_bootstrap_cached_policy_v1() {
    assert_error_contains(
        r#"@bootstrap:{capability_policy:"cached"}"#,
        BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED,
    );
}

#[test]
fn parse_should_reject_basic_bootstrap_with_observe() {
    assert_error_contains(
        r#"@bootstrap:{mode:"basic",observe:{mode:"window"}}"#,
        "mode:\"basic\" 不接受 observe",
    );

    assert_error_contains(
        r#"@bootstrap:{observe:{mode:"window"}}"#,
        "mode:\"basic\" 不接受 observe",
    );
}

#[test]
fn parse_should_reject_bootstrap_unknown_duplicate_and_side_effect_fields() {
    assert_error_contains(r#"@bootstrap:{unknown:true}"#, "未知字段");
    assert_error_contains(r#"@bootstrap:{mode:"gui",mode:"basic"}"#, "字段重复");
    assert_error_contains(
        r#"@bootstrap:{scope:{display:{id:"d2"}}}"#,
        "顶层不接受 scope",
    );
    assert_error_contains(r#"@bootstrap:{display_id:"d2"}"#, "display_id 不是请求字段");
    assert_error_contains(r#"@bootstrap:{click:true}"#, "read-only preflight");
    assert_error_contains(
        r#"@bootstrap:{allow_side_effects:true}"#,
        "read-only preflight",
    );
}

fn assert_error_contains(line: &str, expected: &str) {
    let err = parse_control_line(line).expect_err("line should be rejected");
    assert!(
        err.to_string().contains(expected),
        "error should contain `{expected}`, got `{err}`"
    );
}
