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
    assert_eq!(json_pointer_lookup(&v, "$.a.b.c").unwrap(), serde_json::json!(42));
    assert_eq!(json_pointer_lookup(&v, "a.b.c").unwrap(), serde_json::json!(42));
}

#[test]
fn json_pointer_lookup_array_index() {
    let v = serde_json::json!({"items": [10, 20, 30]});
    assert_eq!(json_pointer_lookup(&v, "$.items[1]").unwrap(), serde_json::json!(20));
    assert_eq!(json_pointer_lookup(&v, "$.items[0]").unwrap(), serde_json::json!(10));
}

#[test]
fn json_pointer_lookup_mixed_path_and_index() {
    let v = serde_json::json!({"a": [{"b": 1}, {"b": 2}]});
    assert_eq!(json_pointer_lookup(&v, "$.a[1].b").unwrap(), serde_json::json!(2));
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
    let step: FlowExpectStep = serde_json::from_str(
        r#"{"kind": "response_field_equals", "path": "$.ok", "value": true}"#,
    ).unwrap();
    assert_eq!(step.kind, FlowExpectKind::ResponseFieldEquals);
    assert_eq!(step.path.as_deref(), Some("$.ok"));
    assert_eq!(step.value, Some(serde_json::json!(true)));
}

#[test]
fn flow_expect_step_value_omitted_defaults_to_none() {
    let step: FlowExpectStep = serde_json::from_str(
        r#"{"kind": "cmd_exit_code", "capture": "c1", "code": 0}"#,
    ).unwrap();
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
