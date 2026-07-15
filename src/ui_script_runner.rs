use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    control_frames,
    control_invocation::{self, ControlArtifactRecord, ControlInvocation, ControlLineExchange},
    input::Transport,
    ui_script,
};

pub(crate) struct UiScriptRunOptions {
    pub(crate) dry_run: bool,
    pub(crate) url: Option<String>,
    pub(crate) transport: Option<Transport>,
    pub(crate) namespace: Option<String>,
    pub(crate) target_name: Option<String>,
    pub(crate) entry_point: Vec<String>,
    pub(crate) trace_dir: Option<PathBuf>,
    pub(crate) positional: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingUiScriptControlLine {
    step_index: usize,
    step_kind: &'static str,
    line: String,
}

struct UiScriptRunState {
    run_id: String,
    run_dir: PathBuf,
    artifacts_dir: PathBuf,
    trace_path: PathBuf,
    trace_file: fs::File,
    completed_step_count: usize,
    failed_step_index: Option<usize>,
    last_response_line: Option<String>,
    last_response_value: Option<serde_json::Value>,
    last_artifacts: Vec<ControlArtifactRecord>,
}

impl UiScriptRunState {
    fn create(
        script_path: &Path,
        dry_run: &ui_script::UiScriptDryRun,
        trace_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let run_id = build_ui_script_run_id();
        let run_dir = trace_dir.unwrap_or_else(|| PathBuf::from("rdog_script_runs").join(&run_id));
        let artifacts_dir = run_dir.join("artifacts");
        fs::create_dir_all(&artifacts_dir).map_err(|err| {
            format!(
                "创建 UI script artifacts 目录失败: {}: {err}",
                artifacts_dir.display()
            )
        })?;
        let trace_path = run_dir.join("trace.jsonl");
        let trace_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&trace_path)
            .map_err(|err| format!("创建 UI script trace 失败: {}: {err}", trace_path.display()))?;

        write_ui_script_normalized_plan(&run_dir, script_path, dry_run, &run_id)?;
        write_ui_script_summary(&run_dir, dry_run, &run_id, "running", 0, None, None)?;

        Ok(Self {
            run_id,
            run_dir,
            artifacts_dir,
            trace_path,
            trace_file,
            completed_step_count: 0,
            failed_step_index: None,
            last_response_line: None,
            last_response_value: None,
            last_artifacts: Vec::new(),
        })
    }

    fn record_step(&mut self, record: serde_json::Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.trace_file, &record)
            .map_err(|err| format!("写入 UI script trace JSON 失败: {err}"))?;
        writeln!(self.trace_file).map_err(|err| format!("写入 UI script trace 换行失败: {err}"))?;
        self.trace_file
            .flush()
            .map_err(|err| format!("刷新 UI script trace 失败: {err}"))
    }
}

pub(crate) fn run(options: UiScriptRunOptions) -> Result<(), String> {
    let mut options = options;
    let (mut control_positionals, script_path) =
        split_ui_script_run_positionals(options.positional)?;
    let program = ui_script::parse_script_file(&script_path).map_err(|err| err.to_string())?;
    apply_ui_script_target(
        &program,
        &mut control_positionals,
        &mut options.namespace,
        &mut options.target_name,
    )?;
    let dry_run = ui_script::compile_dry_run(&program).map_err(|err| err.to_string())?;

    if options.dry_run {
        emit_ui_script_dry_run(&script_path, &dry_run);
        return Ok(());
    }

    let invocation = control_invocation::resolve_control_invocation(
        options.transport,
        options.url,
        options.namespace,
        options.target_name,
        options.entry_point,
        control_positionals,
    )?;
    let mut state = UiScriptRunState::create(&script_path, &dry_run, options.trace_dir)?;
    let result = execute_ui_script_plan(&invocation, &dry_run, &mut state);
    let (status, error) = match &result {
        Ok(()) => ("complete", None),
        Err(err) => ("failed", Some(err.as_str())),
    };
    write_ui_script_summary(
        &state.run_dir,
        &dry_run,
        &state.run_id,
        status,
        state.completed_step_count,
        state.failed_step_index,
        error,
    )?;
    println!("ui-script trace: {}", state.trace_path.display());
    result
}

fn split_ui_script_run_positionals(
    mut positional: Vec<String>,
) -> Result<(Vec<String>, PathBuf), String> {
    let Some(script_path) = positional.pop() else {
        return Err("`rdog ui-script run` 需要脚本文件路径".to_string());
    };
    if positional.iter().any(|item| item.starts_with('@')) {
        return Err(
            "`rdog ui-script run` 的 target 位置参数不能是 `@<line>`;脚本内容应写在 JSON 文件里"
                .to_string(),
        );
    }
    Ok((positional, PathBuf::from(script_path)))
}

fn apply_ui_script_target(
    program: &ui_script::UiScriptProgram,
    control_positionals: &mut Vec<String>,
    namespace: &mut Option<String>,
    target_name: &mut Option<String>,
) -> Result<(), String> {
    let mut targets = program.steps.iter().filter_map(|step| match step {
        ui_script::UiScriptStep::Target(payload) => Some(payload),
        _ => None,
    });
    let Some(target) = targets.next() else {
        return Ok(());
    };
    if targets.next().is_some() {
        return Err("UI script 只能声明一个 Target step".to_string());
    }

    if let Some(script_namespace) = target.get("namespace").and_then(serde_json::Value::as_str) {
        match namespace {
            Some(cli_namespace) if cli_namespace != script_namespace => {
                return Err(format!(
                    "CLI --namespace={cli_namespace} 与脚本 Target.namespace={script_namespace} 不一致"
                ));
            }
            Some(_) => {}
            None => *namespace = Some(script_namespace.to_owned()),
        }
    }

    let Some(script_target_name) = target.get("name").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    if let Some(cli_target_name) = target_name.as_deref() {
        if cli_target_name != script_target_name {
            return Err(format!(
                "CLI --target-name={cli_target_name} 与脚本 Target.name={script_target_name} 不一致"
            ));
        }
        return Ok(());
    }
    if control_positionals.is_empty() {
        control_positionals.push(script_target_name.to_owned());
        return Ok(());
    }
    if control_positionals.len() == 1 && control_positionals[0] == script_target_name {
        return Ok(());
    }
    Err(format!(
        "CLI target {:?} 与脚本 Target.name={script_target_name} 不一致",
        control_positionals
    ))
}

fn emit_ui_script_dry_run(script_path: &PathBuf, dry_run: &ui_script::UiScriptDryRun) {
    println!("ui-script dry-run: {}", script_path.display());
    println!(
        "summary: steps={}, backend_requests={}, semantic_actions={}, mouse_fallbacks={}",
        dry_run.summary.step_count,
        dry_run.summary.backend_request_count,
        dry_run.summary.semantic_action_count,
        dry_run.summary.mouse_fallback_count
    );
    for step in &dry_run.steps {
        match &step.effect {
            ui_script::UiScriptDryRunEffect::Context(label) => {
                println!("step {} {} local context:{label}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Local(label) => {
                println!("step {} {} local {label}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Expect(payload) => {
                let kind = payload
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                println!("step {} {} expect {kind}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::ControlLine(line) => {
                println!("step {} {} control {line}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Exit => {
                println!("step {} {} exit", step.index, step.kind);
            }
        }
    }
}

fn execute_ui_script_plan(
    invocation: &ControlInvocation,
    dry_run: &ui_script::UiScriptDryRun,
    state: &mut UiScriptRunState,
) -> Result<(), String> {
    let mut pending_lines = Vec::new();
    for step in &dry_run.steps {
        match &step.effect {
            ui_script::UiScriptDryRunEffect::ControlLine(line) => {
                pending_lines.push(PendingUiScriptControlLine {
                    step_index: step.index,
                    step_kind: step.kind,
                    line: line.clone(),
                });
            }
            ui_script::UiScriptDryRunEffect::Context(label) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                record_ui_script_local_step(state, step.index, step.kind, "context", label, None)?;
            }
            ui_script::UiScriptDryRunEffect::Local(label) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                match execute_ui_script_local_effect(label) {
                    Ok(()) => {
                        record_ui_script_local_step(
                            state, step.index, step.kind, "local", label, None,
                        )?;
                    }
                    Err(err) => {
                        record_ui_script_local_step(
                            state,
                            step.index,
                            step.kind,
                            "local",
                            label,
                            Some(err.as_str()),
                        )?;
                        state.failed_step_index = Some(step.index);
                        return Err(err);
                    }
                }
            }
            ui_script::UiScriptDryRunEffect::Expect(payload) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                match evaluate_ui_script_expect(payload, state) {
                    Ok(()) => {
                        record_ui_script_expect_step(state, step.index, step.kind, payload, None)?;
                    }
                    Err(err) => {
                        record_ui_script_expect_step(
                            state,
                            step.index,
                            step.kind,
                            payload,
                            Some(err.as_str()),
                        )?;
                        state.failed_step_index = Some(step.index);
                        return Err(err);
                    }
                }
            }
            ui_script::UiScriptDryRunEffect::Exit => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                record_ui_script_exit_step(state, step.index, step.kind)?;
                return Ok(());
            }
        }
    }
    flush_ui_script_pending_lines(invocation, &mut pending_lines, state)
}

fn flush_ui_script_pending_lines(
    invocation: &ControlInvocation,
    pending_lines: &mut Vec<PendingUiScriptControlLine>,
    state: &mut UiScriptRunState,
) -> Result<(), String> {
    if pending_lines.is_empty() {
        return Ok(());
    }
    for pending in pending_lines.iter() {
        let line = pending.line.clone();
        let started_at_unix_ms = unix_time_ms();
        let exchanges = control_invocation::send_control_lines_for_invocation(
            invocation,
            std::slice::from_ref(&line),
            &state.artifacts_dir,
        )?;
        let Some(exchange) = exchanges.first() else {
            return Err(format!(
                "UI script control line 没有返回 exchange: {}",
                pending.line
            ));
        };
        apply_control_line_exchange_to_state(state, exchange);
        record_ui_script_control_step(state, pending, exchange, started_at_unix_ms)?;
    }
    pending_lines.clear();
    Ok(())
}

fn execute_ui_script_local_effect(label: &str) -> Result<(), String> {
    if let Some(ms) = label.strip_prefix("sleep_ms:") {
        let ms = ms
            .parse::<u64>()
            .map_err(|err| format!("UI script SleepMs 编译结果非法: {ms}, error={err}"))?;
        std::thread::sleep(std::time::Duration::from_millis(ms));
        return Ok(());
    }
    if label.starts_with("expect:") {
        return Err("UI script real runner 暂不支持 Expect 验证;请先用显式 control step 验证,或使用 --dry-run 检查编译结果".to_string());
    }
    if label == "barrier:observe" {
        return Err(
            "UI script real runner 暂不支持 Barrier observe;请先显式插入 Observe step".to_string(),
        );
    }
    Ok(())
}

fn record_ui_script_local_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
    effect: &str,
    label: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let status = if error.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error.is_none() {
        state.completed_step_count += 1;
    }
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": effect,
        "label": label,
        "status": status,
        "error": error,
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_expect_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
    error: Option<&str>,
) -> Result<(), String> {
    let status = if error.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error.is_none() {
        state.completed_step_count += 1;
    }
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": "expect",
        "expect": payload,
        "status": status,
        "error": error,
        "last_response": state.last_response_line,
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_exit_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
) -> Result<(), String> {
    state.completed_step_count += 1;
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": "exit",
        "status": "complete",
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_control_step(
    state: &mut UiScriptRunState,
    pending: &PendingUiScriptControlLine,
    exchange: &ControlLineExchange,
    started_at_unix_ms: u128,
) -> Result<(), String> {
    let error_message = if last_response_is_error(state) {
        Some(ui_script_control_response_error_message(state, exchange))
    } else {
        None
    };
    let status = if error_message.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error_message.is_none() {
        state.completed_step_count += 1;
    } else {
        state.failed_step_index = Some(pending.step_index);
    }
    let finished_at_unix_ms = unix_time_ms();

    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": pending.step_index,
        "step_kind": pending.step_kind,
        "effect": "control",
        "control_lines": [exchange.line],
        "status": status,
        "error": error_message.as_deref(),
        "started_at_unix_ms": started_at_unix_ms,
        "response_line": exchange.response_line,
        "response": summarize_response_value(state.last_response_value.as_ref(), exchange.response_line.as_ref()),
        "frames": summarize_control_frames(&exchange.frames),
        "artifacts": summarize_artifacts(&exchange.artifacts),
        "finished_at_unix_ms": finished_at_unix_ms,
    }))?;

    match error_message {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

fn apply_control_line_exchange_to_state(
    state: &mut UiScriptRunState,
    exchange: &ControlLineExchange,
) {
    state.last_response_line = exchange.response_line.clone();
    state.last_response_value = exchange
        .response_line
        .as_deref()
        .and_then(parse_response_payload_value);
    state.last_artifacts = exchange.artifacts.clone();
}

fn evaluate_ui_script_expect(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let kind = payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Expect 缺少 kind 字段".to_string())?;
    match kind {
        "response_status" => expect_response_status(payload, state),
        "response_contains" => expect_response_contains(payload, state),
        "control_status" => expect_control_status(payload, state),
        "window_rect" => expect_window_rect(payload, state),
        "screenshot_exists" => expect_screenshot_exists(payload, state),
        other => Err(format!(
            "UI script real runner 暂不支持 Expect kind: {other}"
        )),
    }
}

fn expect_response_status(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    require_last_response_for_expect(state, "response_status")?;
    let expected = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ok");
    let is_error = last_response_is_error(state);
    match expected {
        "ok" if !is_error => Ok(()),
        "error" if is_error => Ok(()),
        "ok" | "error" => Err(format!(
            "Expect response_status={expected} 不满足, last_response={:?}",
            state.last_response_line
        )),
        other => {
            let actual = find_json_string_field(state.last_response_value.as_ref(), "status");
            if actual.as_deref() == Some(other) {
                Ok(())
            } else {
                Err(format!(
                    "Expect response_status={other} 不满足, actual={actual:?}"
                ))
            }
        }
    }
}

fn expect_response_contains(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let needle = payload
        .get("contains")
        .or_else(|| payload.get("text"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Expect response_contains 需要 contains 或 text 字段".to_string())?;
    let response = state
        .last_response_line
        .as_deref()
        .ok_or_else(|| "Expect response_contains 没有上一条 @response".to_string())?;
    if response.contains(needle) {
        Ok(())
    } else {
        Err(format!("Expect response_contains 未命中: needle={needle}"))
    }
}

fn expect_control_status(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    require_last_response_for_expect(state, "control_status")?;
    if let Some(expected_ok) = payload.get("ok").and_then(serde_json::Value::as_bool) {
        let actual_ok = !last_response_is_error(state);
        return if actual_ok == expected_ok {
            Ok(())
        } else {
            Err(format!(
                "Expect control_status ok={expected_ok} 不满足, actual={actual_ok}"
            ))
        };
    }

    let expected_code = payload
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let actual_code = response_error_code(state.last_response_value.as_ref()).unwrap_or(0);
    if actual_code == expected_code {
        Ok(())
    } else {
        Err(format!(
            "Expect control_status code={expected_code} 不满足, actual={actual_code}"
        ))
    }
}

fn require_last_response_for_expect(
    state: &UiScriptRunState,
    expect_kind: &str,
) -> Result<(), String> {
    if state.last_response_line.is_some() {
        return Ok(());
    }

    Err(format!(
        "Expect {expect_kind} 没有上一条 @response;请先执行 ControlLine/Observe/Action step"
    ))
}

fn expect_window_rect(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let rect = find_rect_value(state.last_response_value.as_ref())
        .ok_or_else(|| "Expect window_rect 没有在上一条响应里找到 rect/after_rect".to_string())?;
    let tolerance = payload
        .get("tolerance_px")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);

    for field in ["x", "y", "width", "height"] {
        let Some(expected) = payload.get(field).and_then(serde_json::Value::as_i64) else {
            continue;
        };
        let actual = rect
            .get(field)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("Expect window_rect 响应 rect 缺少 {field}"))?;
        if (actual - expected).abs() > tolerance {
            return Err(format!(
                "Expect window_rect {field}={expected} 不满足, actual={actual}, tolerance={tolerance}"
            ));
        }
    }
    Ok(())
}

fn expect_screenshot_exists(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let label = payload.get("label").and_then(serde_json::Value::as_str);
    let matched = state.last_artifacts.iter().any(|artifact| {
        artifact.path.exists()
            && label
                .map(|label| artifact.filename.contains(label))
                .unwrap_or(true)
    });
    if matched {
        Ok(())
    } else {
        Err(format!(
            "Expect screenshot_exists 不满足, label={label:?}, artifacts={}",
            state.last_artifacts.len()
        ))
    }
}

fn parse_response_payload_value(line: &str) -> Option<serde_json::Value> {
    let payload = line.trim_start().strip_prefix("@response ")?;
    serde_json::from_str(payload.trim()).ok()
}

fn summarize_response_value(
    value: Option<&serde_json::Value>,
    line: Option<&String>,
) -> serde_json::Value {
    serde_json::json!({
        "line": line,
        "value": value,
        "target_resolution": find_json_object_field(value, "target_resolution"),
    })
}

fn last_response_is_error(state: &UiScriptRunState) -> bool {
    response_error_code(state.last_response_value.as_ref())
        .map(|code| code != 0)
        .unwrap_or(false)
        || matches!(
            response_status_value(state.last_response_value.as_ref()),
            Some("error" | "failed")
        )
}

fn response_error_code(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(code) = value.get("code").and_then(serde_json::Value::as_i64) {
        return Some(code);
    }
    value
        .get("value")
        .and_then(|inner| inner.get("code"))
        .and_then(serde_json::Value::as_i64)
}

fn response_status_value(value: Option<&serde_json::Value>) -> Option<&str> {
    let value = value?;
    value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("value")
                .and_then(|inner| inner.get("status"))
                .and_then(serde_json::Value::as_str)
        })
}

fn ui_script_control_response_error_message(
    state: &UiScriptRunState,
    exchange: &ControlLineExchange,
) -> String {
    let detail = find_json_string_field(state.last_response_value.as_ref(), "error")
        .or_else(|| response_status_value(state.last_response_value.as_ref()).map(str::to_owned))
        .or_else(|| state.last_response_line.clone())
        .unwrap_or_else(|| "unknown control error".to_owned());
    format!(
        "UI script control step `{}` failed: {detail}",
        exchange.line
    )
}

fn find_json_string_field(value: Option<&serde_json::Value>, field: &str) -> Option<String> {
    let value = value?;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(field).and_then(serde_json::Value::as_str) {
                return Some(found.to_owned());
            }
            map.values()
                .find_map(|child| find_json_string_field(Some(child), field))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|child| find_json_string_field(Some(child), field)),
        _ => None,
    }
}

fn find_json_object_field<'a>(
    value: Option<&'a serde_json::Value>,
    field: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    let value = value?;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(field).and_then(serde_json::Value::as_object) {
                return Some(found);
            }
            map.values()
                .find_map(|child| find_json_object_field(Some(child), field))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|child| find_json_object_field(Some(child), field)),
        _ => None,
    }
}

fn find_rect_value(
    value: Option<&serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let value = value?;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(rect) = map
                .get("after_rect")
                .or_else(|| map.get("rect"))
                .and_then(serde_json::Value::as_object)
            {
                return Some(rect);
            }
            map.values().find_map(|child| find_rect_value(Some(child)))
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|child| find_rect_value(Some(child)))
        }
        _ => None,
    }
}

fn summarize_control_frames(frames: &[control_frames::ControlFrame]) -> Vec<serde_json::Value> {
    frames
        .iter()
        .map(|frame| match frame {
            control_frames::ControlFrame::ResponseLine(line) => serde_json::json!({
                "kind": "response",
                "line": line,
            }),
            control_frames::ControlFrame::SaveFile(frame) => serde_json::json!({
                "kind": "savefile",
                "filename": frame.filename,
                "mime": frame.mime,
                "width": frame.width,
                "height": frame.height,
            }),
            control_frames::ControlFrame::PtyReady(_) => serde_json::json!({"kind": "pty-ready"}),
            control_frames::ControlFrame::PtyOutput(_) => serde_json::json!({"kind": "pty-output"}),
            control_frames::ControlFrame::PtyExit(_) => serde_json::json!({"kind": "pty-exit"}),
            control_frames::ControlFrame::PtyClosed(_) => serde_json::json!({"kind": "pty-closed"}),
            control_frames::ControlFrame::PtyDetached(_) => {
                serde_json::json!({"kind": "pty-detached"})
            }
            control_frames::ControlFrame::PtyAttached(_) => {
                serde_json::json!({"kind": "pty-attached"})
            }
        })
        .collect()
}

fn summarize_artifacts(artifacts: &[ControlArtifactRecord]) -> Vec<serde_json::Value> {
    artifacts
        .iter()
        .map(|artifact| {
            serde_json::json!({
                "filename": artifact.filename,
                "mime": artifact.mime,
                "path": artifact.path,
                "width": artifact.width,
                "height": artifact.height,
            })
        })
        .collect()
}

fn write_ui_script_normalized_plan(
    run_dir: &Path,
    script_path: &Path,
    dry_run: &ui_script::UiScriptDryRun,
    run_id: &str,
) -> Result<(), String> {
    let steps = dry_run
        .steps
        .iter()
        .map(|step| {
            serde_json::json!({
                "index": step.index,
                "kind": step.kind,
                "effect": ui_script_effect_summary(&step.effect),
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "schema": "rdog.ui-script.normalized.v1",
        "run_id": run_id,
        "source_path": script_path,
        "steps": steps,
        "control_lines": dry_run.control_lines,
    });
    write_json_file(&run_dir.join("script.normalized.json"), &value)
}

fn write_ui_script_summary(
    run_dir: &Path,
    dry_run: &ui_script::UiScriptDryRun,
    run_id: &str,
    status: &str,
    completed_step_count: usize,
    failed_step_index: Option<usize>,
    error: Option<&str>,
) -> Result<(), String> {
    let value = serde_json::json!({
        "schema": "rdog.ui-script.run.v1",
        "run_id": run_id,
        "status": status,
        "step_count": dry_run.summary.step_count,
        "completed_step_count": completed_step_count,
        "failed_step_index": failed_step_index,
        "backend_request_count": dry_run.summary.backend_request_count,
        "semantic_action_count": dry_run.summary.semantic_action_count,
        "mouse_fallback_count": dry_run.summary.mouse_fallback_count,
        "verification_passed": status == "complete",
        "error": error,
        "updated_at_unix_ms": unix_time_ms(),
    });
    write_json_file(&run_dir.join("summary.json"), &value)
}

fn ui_script_effect_summary(effect: &ui_script::UiScriptDryRunEffect) -> serde_json::Value {
    match effect {
        ui_script::UiScriptDryRunEffect::Context(label) => {
            serde_json::json!({"kind": "context", "label": label})
        }
        ui_script::UiScriptDryRunEffect::Local(label) => {
            serde_json::json!({"kind": "local", "label": label})
        }
        ui_script::UiScriptDryRunEffect::Expect(payload) => {
            serde_json::json!({"kind": "expect", "payload": payload})
        }
        ui_script::UiScriptDryRunEffect::ControlLine(line) => {
            serde_json::json!({"kind": "control", "line": line})
        }
        ui_script::UiScriptDryRunEffect::Exit => serde_json::json!({"kind": "exit"}),
    }
}

fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|err| format!("序列化 JSON 文件失败: {}: {err}", path.display()))?;
    fs::write(path, format!("{content}\n"))
        .map_err(|err| format!("写入 JSON 文件失败: {}: {err}", path.display()))
}

fn build_ui_script_run_id() -> String {
    format!("uiscript-{}", unix_time_ms())
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
