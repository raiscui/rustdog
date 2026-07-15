use super::{
    apply_control_line_exchange_to_state, apply_ui_script_target, evaluate_ui_script_expect,
    execute_ui_script_plan, parse_response_payload_value, record_ui_script_control_step,
    split_ui_script_run_positionals, PendingUiScriptControlLine, UiScriptRunState,
};
use crate::{
    control_frames::ControlFrame,
    control_invocation::{ControlArtifactRecord, ControlInvocation, ControlLineExchange},
    input::{Command, Opts as InputOpts, UiScriptCommand},
    ui_script::{UiScriptDryRun, UiScriptDryRunEffect, UiScriptDryRunStep, UiScriptRunSummary},
};
use clap::Parser as _;
use std::{
    fs::{self, OpenOptions},
    io::{BufRead as _, BufReader, Write as _},
    net::TcpListener,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_ui_script_dir(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis();
    std::env::temp_dir().join(format!(
        "rdog-ui-script-{name}-{millis}-{}",
        std::process::id()
    ))
}

fn fake_ui_script_state(name: &str) -> UiScriptRunState {
    let run_dir = temp_ui_script_dir(name);
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("test artifacts dir should create");
    let trace_path = run_dir.join("trace.jsonl");
    let trace_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&trace_path)
        .expect("test trace file should create");

    UiScriptRunState {
        run_id: format!("test-{name}"),
        run_dir,
        artifacts_dir,
        trace_path,
        trace_file,
        completed_step_count: 0,
        failed_step_index: None,
        last_response_line: None,
        last_response_value: None,
        last_artifacts: Vec::new(),
    }
}

#[test]
fn ui_script_run_positionals_should_use_single_arg_as_script_path() {
    let (target, script_path) = split_ui_script_run_positionals(vec![
        "tests/fixtures/ui_script/ping_control_line.json".to_string(),
    ])
    .unwrap();

    assert!(target.is_empty());
    assert_eq!(
        script_path,
        PathBuf::from("tests/fixtures/ui_script/ping_control_line.json")
    );
}

#[test]
fn ui_script_run_positionals_should_keep_target_before_script_path() {
    let (target, script_path) = split_ui_script_run_positionals(vec![
        "mac.lab".to_string(),
        "tests/fixtures/ui_script/ping_control_line.json".to_string(),
    ])
    .unwrap();

    assert_eq!(target, vec!["mac.lab".to_string()]);
    assert_eq!(
        script_path,
        PathBuf::from("tests/fixtures/ui_script/ping_control_line.json")
    );
}

#[test]
fn ui_script_run_positionals_should_reject_control_line_as_target() {
    let err = split_ui_script_run_positionals(vec![
        "@ping".to_string(),
        "tests/fixtures/ui_script/ping_control_line.json".to_string(),
    ])
    .unwrap_err();

    assert!(err.contains("target 位置参数不能是 `@<line>`"));
}

#[test]
fn ui_script_run_cli_should_parse_target_and_script_path() {
    let opts = InputOpts::try_parse_from([
        "rdog",
        "ui-script",
        "run",
        "self",
        "tests/fixtures/ui_script/ping_control_line.json",
    ])
    .unwrap();

    let Command::UiScript {
        command:
            UiScriptCommand::Run {
                dry_run,
                positional,
                trace_dir,
                ..
            },
    } = opts.command
    else {
        panic!("expected ui-script run command");
    };
    assert!(!dry_run);
    assert!(trace_dir.is_none());
    assert_eq!(
        positional,
        vec![
            "self".to_string(),
            "tests/fixtures/ui_script/ping_control_line.json".to_string()
        ]
    );
}

#[test]
fn ui_script_run_cli_should_parse_trace_dir() {
    let opts = InputOpts::try_parse_from([
        "rdog",
        "ui-script",
        "run",
        "--trace-dir",
        "rdog_script_runs/demo",
        "tests/fixtures/ui_script/ping_control_line.json",
    ])
    .unwrap();

    let Command::UiScript {
        command:
            UiScriptCommand::Run {
                trace_dir,
                positional,
                ..
            },
    } = opts.command
    else {
        panic!("expected ui-script run command");
    };
    assert_eq!(trace_dir, Some(PathBuf::from("rdog_script_runs/demo")));
    assert_eq!(
        positional,
        vec!["tests/fixtures/ui_script/ping_control_line.json".to_string()]
    );
}

#[test]
fn ui_script_expect_should_validate_response_and_window_rect() {
    let mut state = fake_ui_script_state("expect-response");
    state.last_response_line =
            Some(r#"@response {"value":{"status":"ok","after_rect":{"x":10,"y":20,"width":300,"height":200}}}"#.to_string());
    state.last_response_value = state
        .last_response_line
        .as_deref()
        .and_then(parse_response_payload_value);

    let response_contains = serde_json::json!({
        "kind": "response_contains",
        "contains": "after_rect"
    });
    evaluate_ui_script_expect(response_contains.as_object().unwrap(), &state).unwrap();

    let response_status = serde_json::json!({
        "kind": "response_status",
        "status": "ok"
    });
    evaluate_ui_script_expect(response_status.as_object().unwrap(), &state).unwrap();

    let window_rect = serde_json::json!({
        "kind": "window_rect",
        "x": 10,
        "y": 21,
        "width": 300,
        "height": 200,
        "tolerance_px": 1
    });
    evaluate_ui_script_expect(window_rect.as_object().unwrap(), &state).unwrap();
}

#[test]
fn ui_script_expect_should_validate_control_status_and_screenshot_artifact() {
    let mut state = fake_ui_script_state("expect-artifact");
    state.last_response_line = Some(r#"@response {"code":64,"error":"bad request"}"#.to_string());
    state.last_response_value = state
        .last_response_line
        .as_deref()
        .and_then(parse_response_payload_value);
    let control_status = serde_json::json!({
        "kind": "control_status",
        "code": 64
    });
    evaluate_ui_script_expect(control_status.as_object().unwrap(), &state).unwrap();

    let artifact_path = state.artifacts_dir.join("before.jpg");
    fs::write(&artifact_path, b"fake image").expect("fake artifact should write");
    state.last_artifacts.push(ControlArtifactRecord {
        filename: "before.jpg".to_string(),
        mime: "image/jpeg".to_string(),
        path: artifact_path,
        width: Some(1),
        height: Some(1),
    });
    let screenshot_exists = serde_json::json!({
        "kind": "screenshot_exists",
        "label": "before"
    });
    evaluate_ui_script_expect(screenshot_exists.as_object().unwrap(), &state).unwrap();
}

#[test]
fn ui_script_expect_should_reject_status_checks_without_prior_response() {
    let state = fake_ui_script_state("expect-missing-response");

    let response_status = serde_json::json!({
        "kind": "response_status",
        "status": "ok"
    });
    let response_err = evaluate_ui_script_expect(response_status.as_object().unwrap(), &state)
        .expect_err("response_status must not pass before any @response");
    assert!(response_err.contains("没有上一条 @response"));

    let control_status = serde_json::json!({
        "kind": "control_status",
        "code": 0
    });
    let control_err = evaluate_ui_script_expect(control_status.as_object().unwrap(), &state)
        .expect_err("control_status must not pass before any @response");
    assert!(control_err.contains("没有上一条 @response"));

    let control_ok = serde_json::json!({
        "kind": "control_status",
        "ok": true
    });
    let control_ok_err = evaluate_ui_script_expect(control_ok.as_object().unwrap(), &state)
        .expect_err("control_status ok=true must not pass before any @response");
    assert!(control_ok_err.contains("没有上一条 @response"));
}

#[test]
fn ui_script_trace_should_write_control_step_record() {
    let mut state = fake_ui_script_state("trace-control");
    let pending = PendingUiScriptControlLine {
        step_index: 0,
        step_kind: "ControlLine",
        line: "@ping".to_string(),
    };
    let exchange = ControlLineExchange {
        line: "@ping".to_string(),
        frames: vec![ControlFrame::ResponseLine(
            r#"@response "pong""#.to_string(),
        )],
        response_line: Some(r#"@response "pong""#.to_string()),
        artifacts: Vec::new(),
    };

    apply_control_line_exchange_to_state(&mut state, &exchange);
    record_ui_script_control_step(&mut state, &pending, &exchange, 123).unwrap();
    let trace = fs::read_to_string(&state.trace_path).unwrap();
    assert!(trace.contains(r#""step_kind":"ControlLine""#));
    assert!(trace.contains(r#"@response \"pong\""#));
}

#[test]
fn ui_script_trace_should_include_timing_and_target_resolution() {
    let mut state = fake_ui_script_state("trace-target-resolution");
    let pending = PendingUiScriptControlLine {
        step_index: 4,
        step_kind: "MouseMove",
        line: r#"@mouse-move#4:{"target":{"x":10,"y":20}}"#.to_string(),
    };
    let response = r#"@response {"value":{"status":"ok","target_resolution":{"source":"coordinate_fallback","coordinate_space":"os-logical"}}}"#;
    let exchange = ControlLineExchange {
        line: pending.line.clone(),
        frames: vec![ControlFrame::ResponseLine(response.to_string())],
        response_line: Some(response.to_string()),
        artifacts: Vec::new(),
    };

    apply_control_line_exchange_to_state(&mut state, &exchange);
    record_ui_script_control_step(&mut state, &pending, &exchange, 123).unwrap();

    let trace = fs::read_to_string(&state.trace_path).unwrap();
    let record: serde_json::Value = serde_json::from_str(trace.trim()).unwrap();
    assert_eq!(record["started_at_unix_ms"].as_u64(), Some(123));
    assert!(record["finished_at_unix_ms"].as_u64().is_some());
    assert_eq!(
        record["response"]["target_resolution"]["source"].as_str(),
        Some("coordinate_fallback")
    );
}

#[test]
fn ui_script_runner_should_stop_before_second_control_line_after_failed_response() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener should bind");
    let port = listener
        .local_addr()
        .expect("listener addr should resolve")
        .port();
    let (tx, rx) = mpsc::channel();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("runner should connect");
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(300)))
            .expect("test stream timeout should set");
        let mut reader =
            BufReader::new(stream.try_clone().expect("test stream reader should clone"));
        let mut received = Vec::new();
        let mut first = String::new();
        reader
            .read_line(&mut first)
            .expect("first control line should read");
        received.push(first.trim_end_matches(['\r', '\n']).to_owned());
        stream
            .write_all(
                br#"@response {"code":64,"error":"first failed"}
"#,
            )
            .expect("first error response should write");
        stream.flush().expect("first error response should flush");

        let mut second = String::new();
        match reader.read_line(&mut second) {
            Ok(0) => {}
            Ok(_) => received.push(second.trim_end_matches(['\r', '\n']).to_owned()),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(err) => panic!("unexpected second read error: {err}"),
        }
        tx.send(received).expect("received lines should send");
    });

    let dry_run = UiScriptDryRun {
        steps: vec![
            UiScriptDryRunStep {
                index: 0,
                kind: "ControlLine",
                effect: UiScriptDryRunEffect::ControlLine("@first".to_string()),
            },
            UiScriptDryRunStep {
                index: 1,
                kind: "ControlLine",
                effect: UiScriptDryRunEffect::ControlLine("@second".to_string()),
            },
        ],
        control_lines: vec!["@first".to_string(), "@second".to_string()],
        summary: UiScriptRunSummary {
            step_count: 2,
            backend_request_count: 2,
            semantic_action_count: 0,
            mouse_fallback_count: 0,
        },
    };
    let invocation = ControlInvocation::Tcp {
        host: "127.0.0.1".to_string(),
        port: port.to_string(),
    };
    let mut state = fake_ui_script_state("fail-fast-control-line");

    let err = execute_ui_script_plan(&invocation, &dry_run, &mut state)
        .expect_err("failed control response should stop the script");
    server.join().expect("server thread should finish");
    let received = rx.recv().expect("received lines should be available");

    assert!(err.contains("first failed"));
    assert_eq!(received, vec!["@first".to_string()]);
    assert_eq!(state.failed_step_index, Some(0));
    assert_eq!(state.completed_step_count, 0);
}

#[test]
fn ui_script_control_step_should_fail_on_error_response() {
    let mut state = fake_ui_script_state("trace-control-error");
    let pending = PendingUiScriptControlLine {
        step_index: 7,
        step_kind: "ControlLine",
        line: "@window-resize#1:{}".to_string(),
    };
    let exchange = ControlLineExchange {
        line: pending.line.clone(),
        frames: vec![ControlFrame::ResponseLine(
            r#"@response {"code":64,"error":"permission denied"}"#.to_string(),
        )],
        response_line: Some(r#"@response {"code":64,"error":"permission denied"}"#.to_string()),
        artifacts: Vec::new(),
    };

    apply_control_line_exchange_to_state(&mut state, &exchange);
    let err = record_ui_script_control_step(&mut state, &pending, &exchange, 123)
        .expect_err("error response should fail the script step");
    let trace = fs::read_to_string(&state.trace_path).unwrap();

    assert_eq!(state.completed_step_count, 0);
    assert_eq!(state.failed_step_index, Some(7));
    assert!(err.contains("permission denied"));
    assert!(trace.contains(r#""status":"failed""#));
}

#[test]
fn ui_script_target_should_fill_empty_cli_target_and_namespace() {
    let program = crate::ui_script::parse_script_json(
        r#"[{"Target":{"name":"self","namespace":"lab"}},{"ControlLine":"@ping"}]"#,
    )
    .unwrap();
    let mut control_positionals = Vec::new();
    let mut namespace = None;
    let mut target_name = None;

    apply_ui_script_target(
        &program,
        &mut control_positionals,
        &mut namespace,
        &mut target_name,
    )
    .unwrap();

    assert_eq!(control_positionals, vec!["self".to_string()]);
    assert_eq!(namespace.as_deref(), Some("lab"));
    assert!(target_name.is_none());
}

#[test]
fn ui_script_target_should_reject_cli_target_mismatch() {
    let program = crate::ui_script::parse_script_json(
        r#"[{"Target":{"name":"self"}},{"ControlLine":"@ping"}]"#,
    )
    .unwrap();
    let mut control_positionals = vec!["mac.lab".to_string()];
    let mut namespace = None;
    let mut target_name = None;
    let err = apply_ui_script_target(
        &program,
        &mut control_positionals,
        &mut namespace,
        &mut target_name,
    )
    .unwrap_err();

    assert!(err.contains("不一致"));
}
