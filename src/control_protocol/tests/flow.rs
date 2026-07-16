use std::collections::BTreeMap;

use super::*;
use crate::control_flow::{
    FlowCmdStep, FlowExpectKind, FlowExpectStep, FlowOptions, FlowPolicy, FlowRequest,
    FlowSaveArtifactStep, FlowScriptStep, FlowStep, FlowTraceMode, DEFAULT_FLOW_MAX_OUTPUT_BYTES,
    DEFAULT_FLOW_MAX_STEPS, DEFAULT_FLOW_TIMEOUT_MS,
};

#[test]
fn parse_should_support_flow_request_with_request_id() {
    let parsed = parse_control_line(
        r#"@flow#9:{"schema":"rdog.flow.v1","policy":{"allow_shell":true,"allow_file_read":true,"timeout_ms":60000,"max_steps":32,"max_output_bytes":4096},"options":{"trace":"savefile"},"steps":[{"Cmd":{"run":"echo hi","capture":"cmd1"}},{"Script":{"text":"echo script","shell":"bash","cwd":"/tmp","env":{"A":"B"},"timeout_ms":1000,"capture":"script1"}},{"ControlLine":"@ping"},{"SleepMs":1},{"Expect":{"kind":"cmd_exit_code","capture":"cmd1","code":0}},{"Expect":{"kind":"cmd_stdout_contains","capture":"cmd1","contains":"hi"}},{"SaveArtifact":{"path":"/tmp/report.json","mime":"application/json","filename":"report.json"}},{"Exit":null}]}"#,
    )
    .unwrap();

    let ControlParseResult::Control(ControlRequest {
        request_id: Some(9),
        command: ControlCommand::Flow(request),
    }) = parsed
    else {
        panic!("expected @flow control request with id");
    };

    assert_eq!(request.schema, "rdog.flow.v1");
    assert_eq!(
        request.policy,
        FlowPolicy {
            allow_shell: true,
            allow_file_read: true,
            allow_computer_act: false, // ticket 19
            timeout_ms: 60_000,
            max_steps: 32,
            max_output_bytes: 4096,
        }
    );
    assert_eq!(
        request.options,
        FlowOptions {
            trace: FlowTraceMode::SaveFile,
        }
    );
    assert_eq!(request.steps.len(), 8);

    assert_eq!(
        request.steps[0],
        FlowStep::Cmd(FlowCmdStep {
            run: "echo hi".to_owned(),
            shell: None,
            cwd: None,
            env: BTreeMap::new(),
            timeout_ms: None,
            capture: Some("cmd1".to_owned()),
        })
    );
    assert_eq!(
        request.steps[1],
        FlowStep::Script(FlowScriptStep {
            text: "echo script".to_owned(),
            shell: Some("bash".to_owned()),
            cwd: Some("/tmp".to_owned()),
            env: BTreeMap::from([("A".to_owned(), "B".to_owned())]),
            timeout_ms: Some(1000),
            capture: Some("script1".to_owned()),
        })
    );
    assert_eq!(request.steps[2], FlowStep::ControlLine("@ping".to_owned()));
    assert_eq!(request.steps[3], FlowStep::SleepMs(1));
    assert_eq!(
        request.steps[4],
        FlowStep::Expect(FlowExpectStep {
            kind: FlowExpectKind::CmdExitCode,
            capture: Some("cmd1".to_owned()),
            code: Some(0),
            contains: None,
            path: None,
            artifact: None,
            value: None, // ticket 20
        })
    );
    assert_eq!(
        request.steps[6],
        FlowStep::SaveArtifact(FlowSaveArtifactStep {
            path: "/tmp/report.json".to_owned(),
            mime: Some("application/json".to_owned()),
            filename: Some("report.json".to_owned()),
            max_bytes: None,
        })
    );
    assert_eq!(request.steps[7], FlowStep::Exit);
}

#[test]
fn parse_should_default_flow_policy_and_options_for_control_only_flow() {
    assert_eq!(
        parse_control_line(r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@ping"}]}"#)
            .unwrap(),
        ControlParseResult::Control(ControlRequest {
            request_id: None,
            command: ControlCommand::Flow(FlowRequest {
                schema: "rdog.flow.v1".to_owned(),
                policy: FlowPolicy {
                    allow_shell: false,
                    allow_file_read: false,
                    allow_computer_act: false, // ticket 19
                    timeout_ms: DEFAULT_FLOW_TIMEOUT_MS,
                    max_steps: DEFAULT_FLOW_MAX_STEPS,
                    max_output_bytes: DEFAULT_FLOW_MAX_OUTPUT_BYTES,
                },
                steps: vec![FlowStep::ControlLine("@ping".to_owned())],
                options: FlowOptions {
                    trace: FlowTraceMode::Summary,
                },
            }),
        })
    );
}

#[test]
fn parse_should_reject_flow_invalid_json_and_missing_required_fields() {
    assert_error_contains(
        r#"@flow:{schema:"rdog.flow.v1","steps":[]}"#,
        "严格 JSON object",
    );
    assert_error_contains(
        r#"@flow:{"steps":[{"ControlLine":"@ping"}]}"#,
        "@flow.schema 必填",
    );
    assert_error_contains(r#"@flow:{"schema":"rdog.flow.v1"}"#, "@flow.steps 必填");
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[]}"#,
        "@flow.steps 不能为空",
    );
}

#[test]
fn parse_should_reject_flow_shell_steps_without_policy_allow_shell() {
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"Cmd":{"run":"echo hi"}}]}"#,
        "policy.allow_shell:true",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"Script":{"text":"echo hi"}}]}"#,
        "policy.allow_shell:true",
    );
}

#[test]
fn parse_should_reject_flow_save_artifact_without_policy_allow_file_read() {
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"SaveArtifact":{"path":"/tmp/report.json","filename":"report.json"}}]}"#,
        "policy.allow_file_read:true",
    );
}

#[test]
fn parse_should_reject_flow_unknown_fields_and_unknown_steps() {
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"Bogus":{}}]}"#,
        "Bogus",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","unknown":true,"steps":[{"ControlLine":"@ping"}]}"#,
        "unknown field",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","policy":{"allow_shell":true},"steps":[{"Cmd":{"run":"echo hi","unknown":true}}]}"#,
        "unknown field",
    );
}

#[test]
fn parse_should_reject_status_expect_without_code() {
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@ping"},{"Expect":{"kind":"response_status"}}]}"#,
        "Expect.code",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@ping"},{"Expect":{"kind":"control_status"}}]}"#,
        "Expect.code",
    );
}

#[test]
fn parse_should_reject_flow_nested_flow_pty_and_shell_control_line_bypass() {
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@flow:{\"schema\":\"rdog.flow.v1\",\"steps\":[]}"}]}"#,
        "nested @flow",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@pty:\"bash\""}]}"#,
        "不支持 @pty",
    );
    assert_error_contains(
        r#"@flow:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@cmd:\"echo hi\""}]}"#,
        "请使用 Cmd/Script step",
    );
}

fn assert_error_contains(line: &str, expected: &str) {
    let err = parse_control_line(line).expect_err("line should be rejected");
    assert!(
        err.to_string().contains(expected),
        "error should contain `{expected}`, got `{err}`"
    );
}
