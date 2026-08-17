use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn shell_lane_should_capture_stdout_stderr_and_exit_code() {
    let request = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_shell":true},"steps":[{"Cmd":{"run":"printf 'out-line\n'; printf 'err-line\n' >&2; exit 7","capture":"cmd1"}},{"Expect":{"kind":"cmd_exit_code","capture":"cmd1","code":7}},{"Expect":{"kind":"cmd_stdout_contains","capture":"cmd1","contains":"out-line"}},{"Expect":{"kind":"cmd_stderr_contains","capture":"cmd1","contains":"err-line"}},{"Exit":null}]}"#,
    )
    .unwrap();

    let report = execute_flow_shell_lane(&request, "sh");
    assert!(report.is_success(), "report should pass: {report:?}");
    assert_eq!(report.completed_steps, 5);
    assert!(report.exit_requested);

    let result = report.captures.get("cmd1").expect("cmd1 should capture");
    assert_eq!(result.exit_code, Some(7));
    assert!(result.stdout.contains("out-line"));
    assert!(result.stderr.contains("err-line"));
    assert!(!result.timed_out);
    assert!(!result.truncated);
}

#[test]
fn shell_lane_should_apply_cwd_env_and_script_text() {
    let dir = temp_flow_dir("cwd-env");
    fs::create_dir_all(&dir).expect("temp dir should create");
    let dir = fs::canonicalize(&dir).expect("temp dir should canonicalize");
    let dir = dir.to_str().expect("temp dir should be utf8");
    let request = parse_flow_payload(&format!(
        r#"{{"schema":"rdog.flow.v1","policy":{{"allow_shell":true}},"steps":[{{"Script":{{"text":"printf '%s:%s' \"$FLOW_TEST\" \"$PWD\"","cwd":"{}","env":{{"FLOW_TEST":"ok"}},"capture":"script1"}}}},{{"Expect":{{"kind":"cmd_stdout_contains","capture":"script1","contains":"ok:{}"}}}}]}}"#,
        escape_json(dir),
        escape_json(dir),
    ))
    .unwrap();

    let report = execute_flow_shell_lane(&request, "sh");
    assert!(report.is_success(), "report should pass: {report:?}");
    let result = report
        .captures
        .get("script1")
        .expect("script1 should capture");
    assert!(result.stdout.contains("ok:"));
}

#[test]
fn shell_lane_should_mark_timeout_and_continue_to_expect() {
    let request = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_shell":true,"timeout_ms":1000},"steps":[{"Cmd":{"run":"sleep 2","timeout_ms":50,"capture":"slow"}},{"Expect":{"kind":"cmd_exit_code","capture":"slow","code":0}}]}"#,
    )
    .unwrap();

    let report = execute_flow_shell_lane(&request, "sh");
    let result = report.captures.get("slow").expect("slow should capture");
    assert!(result.timed_out);
    assert!(
        result.duration_ms < 1000,
        "timeout should be bounded: {result:?}"
    );
    let failure = report
        .failed_step
        .expect("expect should fail after timeout");
    assert_eq!(failure.index, 1);
    assert!(failure.message.contains("exit_code"));
}

#[test]
fn shell_lane_should_truncate_stdout_and_stderr_by_policy() {
    let request = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_shell":true,"max_output_bytes":4},"steps":[{"Cmd":{"run":"printf 123456789; printf abcdefghi >&2","capture":"big"}}]}"#,
    )
    .unwrap();

    let report = execute_flow_shell_lane(&request, "sh");
    assert!(report.is_success(), "report should pass: {report:?}");
    let result = report.captures.get("big").expect("big should capture");
    assert_eq!(result.stdout, "1234");
    assert_eq!(result.stderr, "abcd");
    assert!(result.truncated);
}

#[test]
fn shell_lane_should_stop_on_expect_failure() {
    let request = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_shell":true},"steps":[{"Cmd":{"run":"exit 3","capture":"cmd1"}},{"Expect":{"kind":"cmd_exit_code","capture":"cmd1","code":0}},{"Cmd":{"run":"printf should-not-run","capture":"after"}}]}"#,
    )
    .unwrap();

    let report = execute_flow_shell_lane(&request, "sh");
    let failure = report.failed_step.expect("expect should fail");
    assert_eq!(failure.index, 1);
    assert_eq!(failure.kind, "Expect");
    assert!(failure.message.contains("期望 0"));
    assert!(!report.captures.contains_key("after"));
}

fn temp_flow_dir(name: &str) -> std::path::PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();
    std::env::temp_dir().join(format!("rdog-flow-{name}-{millis}-{}", std::process::id()))
}

fn escape_json(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// ticket 20: json_pointer_lookup + json_value_to_string 单测
// ---------------------------------------------------------------------------

#[test]
fn json_pointer_lookup_root_returns_full_value() {
    let v = serde_json::json!({"foo": 1});
    let out = json_pointer_lookup(&v, "$").unwrap();
    assert_eq!(out, v);
}

#[test]
fn json_pointer_lookup_simple_path() {
    let v = serde_json::json!({"a": {"b": {"c": 42}}});
    assert_eq!(
        json_pointer_lookup(&v, "$.a.b.c").unwrap(),
        serde_json::json!(42)
    );
    assert_eq!(
        json_pointer_lookup(&v, "a.b.c").unwrap(),
        serde_json::json!(42)
    );
}

#[test]
fn json_pointer_lookup_array_index() {
    let v = serde_json::json!({"items": [10, 20, 30]});
    assert_eq!(
        json_pointer_lookup(&v, "$.items[1]").unwrap(),
        serde_json::json!(20)
    );
    assert_eq!(
        json_pointer_lookup(&v, "$.items[0]").unwrap(),
        serde_json::json!(10)
    );
}

#[test]
fn json_pointer_lookup_mixed_path_and_index() {
    let v = serde_json::json!({"a": [{"b": 1}, {"b": 2}]});
    assert_eq!(
        json_pointer_lookup(&v, "$.a[1].b").unwrap(),
        serde_json::json!(2)
    );
}

#[test]
fn json_pointer_lookup_missing_path_returns_none() {
    let v = serde_json::json!({"a": 1});
    assert!(json_pointer_lookup(&v, "$.b.c").is_none());
    assert!(json_pointer_lookup(&v, "$.a[5]").is_none());
}

#[test]
fn json_value_to_string_for_various_types() {
    assert_eq!(json_value_to_string(&serde_json::json!("hello")), "hello");
    assert_eq!(json_value_to_string(&serde_json::json!(42)), "42");
    assert_eq!(json_value_to_string(&serde_json::json!(true)), "true");
    assert_eq!(json_value_to_string(&serde_json::json!(null)), "null");
    let obj_str = json_value_to_string(&serde_json::json!({"k": "v"}));
    // 序列化成 compact JSON, 含 k 和 v 字段
    assert!(obj_str.starts_with('{'));
    assert!(obj_str.ends_with('}'));
    assert!(obj_str.contains("\"k\""));
    assert!(obj_str.contains("\"v\""));
}

#[test]
fn flow_expect_step_deserializes_new_field() {
    let step: FlowExpectStep =
        serde_json::from_str(r#"{"kind": "response_field_equals", "path": "$.ok", "value": true}"#)
            .unwrap();
    assert_eq!(step.kind, FlowExpectKind::ResponseFieldEquals);
    assert_eq!(step.path.as_deref(), Some("$.ok"));
    assert_eq!(step.value, Some(serde_json::json!(true)));
}

#[test]
fn flow_expect_step_value_omitted_defaults_to_none() {
    let step: FlowExpectStep =
        serde_json::from_str(r#"{"kind": "cmd_exit_code", "capture": "c1", "code": 0}"#).unwrap();
    assert_eq!(step.kind, FlowExpectKind::CmdExitCode);
    assert!(step.value.is_none());
}

#[test]
fn flow_expect_step_response_path_contains_kind_deserializes() {
    let step: FlowExpectStep = serde_json::from_str(
        r#"{"kind": "response_path_contains", "path": "$.error.error_code", "contains": "invalid"}"#,
    ).unwrap();
    assert_eq!(step.kind, FlowExpectKind::ResponsePathContains);
    assert_eq!(step.path.as_deref(), Some("$.error.error_code"));
    assert_eq!(step.contains.as_deref(), Some("invalid"));
}

fn gui_transaction_request(actions: &[&str]) -> FlowRequest {
    let payload = serde_json::json!({
        "schema": "rdog.flow.v1",
        "policy": {"allow_computer_act": true},
        "steps": [{"GuiTransaction": {"actions": actions}}],
    });
    parse_flow_payload(&payload.to_string()).expect("GUI transaction payload should parse")
}

fn transaction_response(id: u64, ok: bool, successor: Option<(&str, &str, u64)>) -> String {
    let successor = successor.map(|(ref_id, observation_id, epoch)| {
        serde_json::json!({
            "ref": ref_id,
            "observation_id": observation_id,
            "epoch": epoch,
        })
    });
    format!(
        "@response {}",
        serde_json::json!({
            "id": id,
            "value": {
                "ok": ok,
                "successor_target": successor,
            }
        })
    )
}

#[test]
fn checked_gui_transaction_consumes_successor_chain() {
    let actions = [
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e1",observation_id:"obs-1"}},observation_id:"obs-1",epoch:7}"#,
        r#"@computer-act#2:{schema:"rdog.computer-act.v1",action:"click",args:{target:"$successor"},observation_id:"$successor",epoch:$successor}"#,
    ];
    let request = gui_transaction_request(&actions);
    let mut seen = Vec::new();
    let responses = [
        transaction_response(1, true, Some(("@e2", "obs-2", 9))),
        transaction_response(2, true, Some(("@e3", "obs-3", 11))),
    ];
    let mut response_index = 0;
    let output = execute_flow_runtime(
        None,
        &request,
        "sh",
        Some(&mut |line| {
            seen.push(line.to_owned());
            let response = responses[response_index].clone();
            response_index += 1;
            ControlExecutionOutcome::from_response_line(response)
        }),
    );

    assert!(
        output.report.is_success(),
        "transaction should pass: {:?}",
        output.report
    );
    assert_eq!(output.report.checked_transactions[0].completed_actions, 2);
    assert_eq!(seen.len(), 2);
    assert!(seen[1].contains("target:{ref:\"@e2\",observation_id:\"obs-2\"}"));
    assert!(seen[1].contains("observation_id:\"obs-2\""));
    assert!(seen[1].contains("epoch:9"));
    assert!(!seen[1].contains("$successor"));
}

#[test]
fn checked_gui_transaction_stops_on_nonzero_response() {
    let actions = [
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e1",observation_id:"obs-1"}},observation_id:"obs-1",epoch:7}"#,
        r#"@computer-act#2:{schema:"rdog.computer-act.v1",action:"click",args:{target:"$successor"},observation_id:"$successor",epoch:$successor}"#,
    ];
    let request = gui_transaction_request(&actions);
    let mut calls = 0;
    let output = execute_flow_runtime(
        None,
        &request,
        "sh",
        Some(&mut |_line| {
            calls += 1;
            if calls == 1 {
                ControlExecutionOutcome::from_response_line(transaction_response(
                    1,
                    true,
                    Some(("@e2", "obs-2", 9)),
                ))
            } else {
                ControlExecutionOutcome::from_response_line(transaction_response(2, false, None))
            }
        }),
    );

    let failure = output.report.failed_step.expect("transaction should fail");
    assert_eq!(failure.index, 0);
    assert_eq!(output.report.checked_transactions[0].completed_actions, 1);
    assert_eq!(output.report.checked_transactions[0].stopped_at, Some(1));
    assert_eq!(calls, 2);
}

#[test]
fn checked_gui_transaction_fails_closed_when_successor_is_missing() {
    let actions = [
        r#"@computer-act#1:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e1",observation_id:"obs-1"}},observation_id:"obs-1",epoch:7}"#,
        r#"@computer-act#2:{schema:"rdog.computer-act.v1",action:"click",args:{target:"$successor"},observation_id:"$successor",epoch:$successor}"#,
    ];
    let request = gui_transaction_request(&actions);
    let output = execute_flow_runtime(
        None,
        &request,
        "sh",
        Some(&mut |_line| {
            ControlExecutionOutcome::from_response_line(transaction_response(1, true, None))
        }),
    );

    let failure = output
        .report
        .failed_step
        .expect("missing successor should fail");
    assert!(failure.message.contains("successor_target"));
    assert_eq!(output.report.checked_transactions[0].completed_actions, 0);
    assert_eq!(output.report.checked_transactions[0].stopped_at, Some(0));
}

#[test]
fn checked_gui_transaction_rejects_missing_initial_target_or_epoch() {
    let missing_target = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true},"steps":[{"GuiTransaction":{"actions":["@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{start_box:[1,2]},epoch:7}"]}}]}"#,
    )
    .expect_err("coordinate-only first action must be rejected");
    assert!(missing_target.to_string().contains("args.target"));

    let missing_epoch = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true},"steps":[{"GuiTransaction":{"actions":["@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{target:{ref:\"@e1\",observation_id:\"obs-1\"}},observation_id:\"obs-1\"}"]}}]}"#,
    )
    .expect_err("first action without epoch must be rejected");
    assert!(missing_epoch.to_string().contains("epoch"));

    let mismatched_observation = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true},"steps":[{"GuiTransaction":{"actions":["@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{target:{ref:\"@e1\",observation_id:\"obs-1\"}},observation_id:\"obs-other\",epoch:7}"]}}]}"#,
    )
    .expect_err("mismatched request-level observation_id must be rejected");
    assert!(mismatched_observation.to_string().contains("必须与"));
}

#[test]
fn checked_gui_transaction_rejects_non_computer_act_successor_action() {
    for successor in [
        r#"@cmd:echo target:"$successor" observation_id:"$successor" epoch:$successor"#,
        r#"@script:echo target:"$successor" observation_id:"$successor" epoch:$successor"#,
        r#"echo target:"$successor" observation_id:"$successor" epoch:$successor"#,
        r#"@wait:{duration_ms:1,target:"$successor",observation_id:"$successor",epoch:$successor}"#,
    ] {
        let first = r#"@computer-act:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e1",observation_id:"obs-1"}},observation_id:"obs-1",epoch:7}"#;
        let payload = serde_json::json!({
            "schema": "rdog.flow.v1",
            "policy": {"allow_computer_act": true},
            "steps": [{"GuiTransaction": {"actions": [first, successor]}}],
        });
        let error = parse_flow_payload(&payload.to_string())
            .expect_err("transaction must reject every non-computer-act successor action");
        assert!(error.to_string().contains("GuiTransaction.actions[1]"));
    }
}

#[test]
fn checked_gui_transaction_rejects_whitespace_successor_template() {
    let error = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true},"steps":[{"GuiTransaction":{"actions":["@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{target:{ref:\"@e1\",observation_id:\"obs-1\"}},observation_id:\"obs-1\",epoch:7}","@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{target : \"$successor\"},observation_id : \"$successor\",epoch : $successor}"]}}]}"#,
    )
    .expect_err("unsupported whitespace template must fail during parsing");
    assert!(error.to_string().contains("successor"));
}

#[test]
fn checked_gui_transaction_enforces_action_limit() {
    let first = r#"@computer-act:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e1",observation_id:"obs-1"}},observation_id:"obs-1",epoch:7}"#;
    let successor = r#"@computer-act:{schema:"rdog.computer-act.v1",action:"click",args:{target:"$successor"},observation_id:"$successor",epoch:$successor}"#;
    let actions = std::iter::once(first)
        .chain(std::iter::repeat_n(successor, MAX_GUI_TRANSACTION_ACTIONS))
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "schema": "rdog.flow.v1",
        "policy": {"allow_computer_act": true},
        "steps": [{"GuiTransaction": {"actions": actions}}],
    });
    let error = parse_flow_payload(&payload.to_string())
        .expect_err("transaction over limit must be rejected");
    assert!(error.to_string().contains("超过上限"));
}

#[test]
fn strict_background_rejects_raw_keyboard_before_execution() {
    let error = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true,"execution":{"strict_background":true}},"steps":[{"ControlLine":"@key:\"A\""}]}"#,
    )
    .expect_err("strict background must reject raw keyboard");
    assert!(error.to_string().contains("physical_input_prohibited"));
}

#[test]
fn strict_background_rejects_foreground_activation_before_execution() {
    let error = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"execution":{"strict_background":true}},"steps":[{"ControlLine":"@window-activate:{window_id:\"pid:1/window:0\"}"}]}"#,
    )
    .expect_err("strict background must reject window activation");
    assert!(error.to_string().contains("foreground_prohibited"));
}

#[test]
fn strict_background_allows_semantic_ax_value_type() {
    let request = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true,"execution":{"strict_background":true}},"steps":[{"ControlLine":"@computer-act:{schema:\"rdog.computer-act.v1\",action:\"type\",args:{target:{ref:\"@e1\",observation_id:\"obs-1\"},content:\"hello\",mode:\"ax-value\"},epoch:7}"}]}"#,
    )
    .expect("strict background should allow semantic AX value type");
    assert!(request.policy.execution.strict_background);
}

#[test]
fn strict_background_rejects_physical_successor_before_execution() {
    let error = parse_flow_payload(
        r#"{"schema":"rdog.flow.v1","policy":{"allow_computer_act":true,"execution":{"strict_background":true}},"steps":[{"GuiTransaction":{"actions":["@computer-act:{schema:\"rdog.computer-act.v1\",action:\"type\",args:{target:{ref:\"@e1\",observation_id:\"obs-1\"},content:\"hello\",mode:\"ax-value\"},observation_id:\"obs-1\",epoch:7}","@computer-act:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{target:\"$successor\"},observation_id:\"$successor\",epoch:$successor}"]}}]}"#,
    )
    .expect_err("strict background must reject physical successor actions during parsing");
    assert!(error.to_string().contains("physical_input_prohibited"));
}
