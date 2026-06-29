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
