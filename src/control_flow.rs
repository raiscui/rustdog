use std::{
    collections::BTreeMap,
    io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;

use crate::control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame};

mod process;

use self::process::{execute_cmd_step, execute_script_step};

pub(crate) const FLOW_SCHEMA_V1: &str = "rdog.flow.v1";
pub(crate) const DEFAULT_FLOW_TIMEOUT_MS: u64 = 30_000;
pub(crate) const MAX_FLOW_TIMEOUT_MS: u64 = 120_000;
pub(crate) const DEFAULT_FLOW_MAX_STEPS: usize = 64;
pub(crate) const MAX_FLOW_STEPS: usize = 256;
pub(crate) const DEFAULT_FLOW_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_FLOW_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// daemon-side `@flow` 的第一层结构。
///
/// 这里只定义协议 schema 和 parser 可验证的不变量。真正执行 step 的 runtime
/// 会在后续 story 接入,避免 parser 层提前承担副作用。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowRequest {
    pub(crate) schema: String,
    #[serde(default)]
    pub(crate) policy: FlowPolicy,
    pub(crate) steps: Vec<FlowStep>,
    #[serde(default)]
    pub(crate) options: FlowOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowPolicy {
    #[serde(default)]
    pub(crate) allow_shell: bool,
    #[serde(default)]
    pub(crate) allow_file_read: bool,
    #[serde(default = "default_flow_timeout_ms")]
    pub(crate) timeout_ms: u64,
    #[serde(default = "default_flow_max_steps")]
    pub(crate) max_steps: usize,
    #[serde(default = "default_flow_max_output_bytes")]
    pub(crate) max_output_bytes: usize,
}

impl Default for FlowPolicy {
    fn default() -> Self {
        Self {
            allow_shell: false,
            allow_file_read: false,
            timeout_ms: DEFAULT_FLOW_TIMEOUT_MS,
            max_steps: DEFAULT_FLOW_MAX_STEPS,
            max_output_bytes: DEFAULT_FLOW_MAX_OUTPUT_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowOptions {
    #[serde(default)]
    pub(crate) trace: FlowTraceMode,
}

impl Default for FlowOptions {
    fn default() -> Self {
        Self {
            trace: FlowTraceMode::Summary,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub(crate) enum FlowTraceMode {
    #[serde(rename = "summary")]
    Summary,
    #[serde(rename = "savefile")]
    SaveFile,
}

impl Default for FlowTraceMode {
    fn default() -> Self {
        Self::Summary
    }
}

/// v1 使用 serde 的 externally-tagged enum。
///
/// JSON 形状保持为 `{"Cmd":{...}}` / `{"ControlLine":"@ping"}` 这种
/// 单 key step,方便从 iced_emg 风格迁移,也方便后续 trace 直接记录 step kind。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) enum FlowStep {
    Cmd(FlowCmdStep),
    Script(FlowScriptStep),
    ControlLine(String),
    SleepMs(u64),
    Expect(FlowExpectStep),
    SaveArtifact(FlowSaveArtifactStep),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowCmdStep {
    pub(crate) run: String,
    pub(crate) shell: Option<String>,
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) capture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowScriptStep {
    pub(crate) text: String,
    pub(crate) shell: Option<String>,
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) capture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowExpectStep {
    pub(crate) kind: FlowExpectKind,
    pub(crate) capture: Option<String>,
    pub(crate) code: Option<i32>,
    pub(crate) contains: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) artifact: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub(crate) enum FlowExpectKind {
    #[serde(rename = "response_status")]
    ResponseStatus,
    #[serde(rename = "response_contains")]
    ResponseContains,
    #[serde(rename = "control_status")]
    ControlStatus,
    #[serde(rename = "cmd_exit_code")]
    CmdExitCode,
    #[serde(rename = "cmd_stdout_contains")]
    CmdStdoutContains,
    #[serde(rename = "cmd_stderr_contains")]
    CmdStderrContains,
    #[serde(rename = "file_exists")]
    FileExists,
    #[serde(rename = "artifact_exists")]
    ArtifactExists,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowSaveArtifactStep {
    pub(crate) path: String,
    pub(crate) mime: Option<String>,
    pub(crate) filename: Option<String>,
    pub(crate) max_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowRunReport {
    pub(crate) schema: String,
    pub(crate) total_steps: usize,
    pub(crate) completed_steps: usize,
    pub(crate) exit_requested: bool,
    pub(crate) failed_step: Option<FlowStepFailure>,
    pub(crate) captures: BTreeMap<String, FlowCommandResult>,
    pub(crate) response_lines: Vec<String>,
    pub(crate) artifacts: Vec<String>,
    pub(crate) trace_record_count: usize,
}

impl FlowRunReport {
    pub(crate) fn is_success(&self) -> bool {
        self.failed_step.is_none()
    }

    pub(crate) fn to_value(&self) -> serde_json::Value {
        let captures = self
            .captures
            .iter()
            .map(|(name, result)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "exit_code": result.exit_code,
                        "stdout": result.stdout,
                        "stderr": result.stderr,
                        "duration_ms": result.duration_ms,
                        "timed_out": result.timed_out,
                        "truncated": result.truncated,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let failed_step = self.failed_step.as_ref().map(|failure| {
            serde_json::json!({
                "index": failure.index,
                "kind": failure.kind,
                "message": failure.message,
            })
        });

        serde_json::json!({
            "schema": self.schema,
            "status": if self.is_success() { "ok" } else { "failed" },
            "total_steps": self.total_steps,
            "completed_steps": self.completed_steps,
            "exit_requested": self.exit_requested,
            "failed_step": failed_step,
            "captures": captures,
            "response_count": self.response_lines.len(),
            "artifacts": self.artifacts,
            "trace_record_count": self.trace_record_count,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowStepFailure {
    pub(crate) index: usize,
    pub(crate) kind: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowCommandResult {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) duration_ms: u64,
    pub(crate) timed_out: bool,
    pub(crate) truncated: bool,
}

pub(crate) fn parse_flow_payload(input: &str) -> io::Result<FlowRequest> {
    let value = serde_json::from_str::<serde_json::Value>(input).map_err(|err| {
        invalid_data(format!(
            "@flow payload 必须是严格 JSON object,无法解析: {err}"
        ))
    })?;

    let object = value
        .as_object()
        .ok_or_else(|| invalid_data("@flow payload 必须是严格 JSON object"))?;
    if !object.contains_key("schema") {
        return Err(invalid_data("@flow.schema 必填"));
    }
    if !object.contains_key("steps") {
        return Err(invalid_data("@flow.steps 必填"));
    }

    let request = serde_json::from_value::<FlowRequest>(value)
        .map_err(|err| invalid_data(format!("@flow payload 结构无效: {err}")))?;

    validate_flow_request(request)
}

pub(crate) fn execute_flow_request(
    request_id: Option<u64>,
    request: &FlowRequest,
    default_shell: &str,
    mut control_line_executor: impl FnMut(&str) -> ControlExecutionOutcome,
) -> ControlExecutionOutcome {
    let mut control_line_executor =
        &mut control_line_executor as &mut dyn FnMut(&str) -> ControlExecutionOutcome;
    let output = execute_flow_runtime(
        request_id,
        request,
        default_shell,
        Some(&mut control_line_executor),
    );
    let mut frames = output.outbound_frames;
    if let Some(frame) =
        build_trace_savefile_frame(request_id, request, &output.report, &output.trace_records)
    {
        frames.push(ControlFrame::SaveFile(frame));
    }
    frames.push(ControlFrame::ResponseLine(build_flow_response_line(
        request_id,
        &output.report,
    )));
    ControlExecutionOutcome {
        outbound_frames: frames,
    }
}

#[cfg(test)]
pub(crate) fn execute_flow_shell_lane(request: &FlowRequest, default_shell: &str) -> FlowRunReport {
    execute_flow_runtime(None, request, default_shell, None).report
}

fn execute_flow_runtime(
    request_id: Option<u64>,
    request: &FlowRequest,
    default_shell: &str,
    mut control_line_executor: Option<&mut dyn FnMut(&str) -> ControlExecutionOutcome>,
) -> FlowRuntimeOutput {
    let mut state = FlowRuntimeState::new(request);
    let flow_deadline = Instant::now() + Duration::from_millis(request.policy.timeout_ms);

    for (index, step) in request.steps.iter().enumerate() {
        if let Err(message) = ensure_flow_has_time(flow_deadline) {
            state.fail(index, step.kind_name(), message);
            break;
        }

        let result = match step {
            FlowStep::Cmd(step) => execute_cmd_step(
                step,
                default_shell,
                &request.policy,
                remaining_duration(flow_deadline),
            )
            .map(|result| {
                if let Some(capture) = step.capture.as_deref() {
                    state.captures.insert(capture.to_owned(), result);
                }
            }),
            FlowStep::Script(step) => execute_script_step(
                step,
                default_shell,
                &request.policy,
                remaining_duration(flow_deadline),
            )
            .map(|result| {
                if let Some(capture) = step.capture.as_deref() {
                    state.captures.insert(capture.to_owned(), result);
                }
            }),
            FlowStep::SleepMs(ms) => execute_sleep_step(*ms, flow_deadline),
            FlowStep::Expect(step) => state.evaluate_expect(index, step),
            FlowStep::Exit => {
                state.exit_requested = true;
                Ok(())
            }
            FlowStep::ControlLine(line) => match control_line_executor.as_deref_mut() {
                Some(executor) => state.execute_control_line(index, line, executor),
                None => Err(
                    "ControlLine runtime 需要 control_core executor,当前 shell lane 未提供"
                        .to_owned(),
                ),
            },
            FlowStep::SaveArtifact(step) => state.save_artifact(request_id, index, step),
        };

        match result {
            Ok(()) => {
                state.completed_steps += 1;
                state.record_trace(index, step.kind_name(), "ok", None);
                if state.exit_requested {
                    break;
                }
            }
            Err(message) => {
                state.record_trace(index, step.kind_name(), "failed", Some(&message));
                state.fail(index, step.kind_name(), message);
                break;
            }
        }
    }

    state.finish()
}

fn validate_flow_request(request: FlowRequest) -> io::Result<FlowRequest> {
    if request.schema != FLOW_SCHEMA_V1 {
        return Err(invalid_data(format!(
            "@flow.schema 必须是 \"{FLOW_SCHEMA_V1}\",实际是 \"{}\"",
            request.schema
        )));
    }

    validate_policy(&request.policy)?;
    if request.steps.is_empty() {
        return Err(invalid_data("@flow.steps 不能为空"));
    }
    if request.steps.len() > request.policy.max_steps {
        return Err(invalid_data(format!(
            "@flow.steps 数量 {} 超过 policy.max_steps {}",
            request.steps.len(),
            request.policy.max_steps
        )));
    }

    let mut has_shell_step = false;
    let mut has_file_read_step = false;
    for (index, step) in request.steps.iter().enumerate() {
        match step {
            FlowStep::Cmd(step) => {
                has_shell_step = true;
                validate_cmd_step(index, step)?;
            }
            FlowStep::Script(step) => {
                has_shell_step = true;
                validate_script_step(index, step)?;
            }
            FlowStep::ControlLine(line) => validate_control_line_step(index, line)?,
            FlowStep::SleepMs(ms) => validate_step_timeout(index, "SleepMs", Some(*ms))?,
            FlowStep::Expect(step) => validate_expect_step(index, step)?,
            FlowStep::SaveArtifact(step) => {
                has_file_read_step = true;
                validate_save_artifact_step(index, step)?;
            }
            FlowStep::Exit => {}
        }
    }

    if has_shell_step && !request.policy.allow_shell {
        return Err(invalid_data(
            "@flow 包含 Cmd/Script 时必须显式设置 policy.allow_shell:true",
        ));
    }
    if has_file_read_step && !request.policy.allow_file_read {
        return Err(invalid_data(
            "@flow 包含 SaveArtifact 时必须显式设置 policy.allow_file_read:true",
        ));
    }

    Ok(request)
}

fn validate_policy(policy: &FlowPolicy) -> io::Result<()> {
    if policy.timeout_ms == 0 || policy.timeout_ms > MAX_FLOW_TIMEOUT_MS {
        return Err(invalid_data(format!(
            "@flow.policy.timeout_ms 必须在 1..={MAX_FLOW_TIMEOUT_MS} 之间"
        )));
    }
    if policy.max_steps == 0 || policy.max_steps > MAX_FLOW_STEPS {
        return Err(invalid_data(format!(
            "@flow.policy.max_steps 必须在 1..={MAX_FLOW_STEPS} 之间"
        )));
    }
    if policy.max_output_bytes == 0 || policy.max_output_bytes > MAX_FLOW_OUTPUT_BYTES {
        return Err(invalid_data(format!(
            "@flow.policy.max_output_bytes 必须在 1..={MAX_FLOW_OUTPUT_BYTES} 之间"
        )));
    }
    Ok(())
}

fn validate_cmd_step(index: usize, step: &FlowCmdStep) -> io::Result<()> {
    require_non_empty_flow_string(index, "Cmd.run", &step.run)?;
    validate_optional_non_empty(index, "Cmd.shell", step.shell.as_deref())?;
    validate_optional_non_empty(index, "Cmd.cwd", step.cwd.as_deref())?;
    validate_optional_non_empty(index, "Cmd.capture", step.capture.as_deref())?;
    validate_env(index, "Cmd.env", &step.env)?;
    validate_step_timeout(index, "Cmd.timeout_ms", step.timeout_ms)
}

fn validate_script_step(index: usize, step: &FlowScriptStep) -> io::Result<()> {
    require_non_empty_flow_string(index, "Script.text", &step.text)?;
    validate_optional_non_empty(index, "Script.shell", step.shell.as_deref())?;
    validate_optional_non_empty(index, "Script.cwd", step.cwd.as_deref())?;
    validate_optional_non_empty(index, "Script.capture", step.capture.as_deref())?;
    validate_env(index, "Script.env", &step.env)?;
    validate_step_timeout(index, "Script.timeout_ms", step.timeout_ms)
}

fn validate_control_line_step(index: usize, line: &str) -> io::Result<()> {
    require_non_empty_flow_string(index, "ControlLine", line)?;
    let kind = control_line_kind(line).ok_or_else(|| {
        invalid_data(format!(
            "@flow.steps[{index}].ControlLine 必须是显式 control request"
        ))
    })?;

    match kind.as_str() {
        "flow" => Err(invalid_data(format!(
            "@flow.steps[{index}].ControlLine v1 不允许 nested @flow"
        ))),
        "pty" | "pty-close" | "pty-detach" | "pty-attach" => Err(invalid_data(format!(
            "@flow.steps[{index}].ControlLine v1 不支持 @pty 系列"
        ))),
        "cmd" | "script" => Err(invalid_data(format!(
            "@flow.steps[{index}].ControlLine 不允许绕过 shell policy;请使用 Cmd/Script step"
        ))),
        _ => Ok(()),
    }
}

fn validate_expect_step(index: usize, step: &FlowExpectStep) -> io::Result<()> {
    match step.kind {
        FlowExpectKind::ResponseStatus | FlowExpectKind::ControlStatus => {
            if step.code.is_none() {
                return Err(invalid_data(format!(
                    "@flow.steps[{index}].Expect.code 对 {:?} 必填",
                    step.kind
                )));
            }
        }
        FlowExpectKind::ResponseContains => {
            require_expected_field(index, "Expect.contains", step.contains.as_deref())?
        }
        FlowExpectKind::CmdExitCode => {
            require_expected_field(index, "Expect.capture", step.capture.as_deref())?;
            if step.code.is_none() {
                return Err(invalid_data(format!(
                    "@flow.steps[{index}].Expect.code 对 cmd_exit_code 必填"
                )));
            }
        }
        FlowExpectKind::CmdStdoutContains | FlowExpectKind::CmdStderrContains => {
            require_expected_field(index, "Expect.capture", step.capture.as_deref())?;
            require_expected_field(index, "Expect.contains", step.contains.as_deref())?;
        }
        FlowExpectKind::FileExists => {
            require_expected_field(index, "Expect.path", step.path.as_deref())?
        }
        FlowExpectKind::ArtifactExists => {
            require_expected_field(index, "Expect.artifact", step.artifact.as_deref())?
        }
    }
    Ok(())
}

fn validate_save_artifact_step(index: usize, step: &FlowSaveArtifactStep) -> io::Result<()> {
    require_non_empty_flow_string(index, "SaveArtifact.path", &step.path)?;
    validate_optional_non_empty(index, "SaveArtifact.mime", step.mime.as_deref())?;
    validate_optional_non_empty(index, "SaveArtifact.filename", step.filename.as_deref())?;
    if let Some(max_bytes) = step.max_bytes {
        if max_bytes == 0 || max_bytes > MAX_FLOW_OUTPUT_BYTES {
            return Err(invalid_data(format!(
                "@flow.steps[{index}].SaveArtifact.max_bytes 必须在 1..={MAX_FLOW_OUTPUT_BYTES} 之间"
            )));
        }
    }
    Ok(())
}

fn validate_step_timeout(index: usize, field: &str, timeout_ms: Option<u64>) -> io::Result<()> {
    if let Some(timeout_ms) = timeout_ms {
        if timeout_ms == 0 || timeout_ms > MAX_FLOW_TIMEOUT_MS {
            return Err(invalid_data(format!(
                "@flow.steps[{index}].{field} 必须在 1..={MAX_FLOW_TIMEOUT_MS} 之间"
            )));
        }
    }
    Ok(())
}

fn validate_env(index: usize, field: &str, env: &BTreeMap<String, String>) -> io::Result<()> {
    for key in env.keys() {
        require_non_empty_flow_string(index, field, key)?;
    }
    Ok(())
}

fn require_expected_field(index: usize, field: &str, value: Option<&str>) -> io::Result<()> {
    match value {
        Some(value) => require_non_empty_flow_string(index, field, value),
        None => Err(invalid_data(format!("@flow.steps[{index}].{field} 必填"))),
    }
}

fn require_non_empty_flow_string(index: usize, field: &str, value: &str) -> io::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].{field} 不能为空"
        )));
    }
    Ok(())
}

fn validate_optional_non_empty(index: usize, field: &str, value: Option<&str>) -> io::Result<()> {
    if let Some(value) = value {
        require_non_empty_flow_string(index, field, value)?;
    }
    Ok(())
}

impl FlowStep {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Cmd(_) => "Cmd",
            Self::Script(_) => "Script",
            Self::ControlLine(_) => "ControlLine",
            Self::SleepMs(_) => "SleepMs",
            Self::Expect(_) => "Expect",
            Self::SaveArtifact(_) => "SaveArtifact",
            Self::Exit => "Exit",
        }
    }
}

struct FlowRuntimeState {
    schema: String,
    total_steps: usize,
    completed_steps: usize,
    exit_requested: bool,
    failed_step: Option<FlowStepFailure>,
    captures: BTreeMap<String, FlowCommandResult>,
    response_lines: Vec<String>,
    response_values: Vec<serde_json::Value>,
    artifacts: Vec<String>,
    trace_records: Vec<serde_json::Value>,
    outbound_frames: Vec<ControlFrame>,
}

struct FlowRuntimeOutput {
    report: FlowRunReport,
    outbound_frames: Vec<ControlFrame>,
    trace_records: Vec<serde_json::Value>,
}

impl FlowRuntimeState {
    fn new(request: &FlowRequest) -> Self {
        Self {
            schema: request.schema.clone(),
            total_steps: request.steps.len(),
            completed_steps: 0,
            exit_requested: false,
            failed_step: None,
            captures: BTreeMap::new(),
            response_lines: Vec::new(),
            response_values: Vec::new(),
            artifacts: Vec::new(),
            trace_records: Vec::new(),
            outbound_frames: Vec::new(),
        }
    }

    fn fail(&mut self, index: usize, kind: &str, message: String) {
        self.failed_step = Some(FlowStepFailure {
            index,
            kind: kind.to_owned(),
            message,
        });
    }

    fn evaluate_expect(&self, index: usize, step: &FlowExpectStep) -> Result<(), String> {
        match step.kind {
            FlowExpectKind::CmdExitCode => {
                let capture = require_expect_capture(index, step)?;
                let result = self.require_capture(index, capture)?;
                let expected = step
                    .code
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.code 缺失"))?;
                if result.exit_code == Some(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "capture `{capture}` exit_code 期望 {expected},实际 {:?}",
                        result.exit_code
                    ))
                }
            }
            FlowExpectKind::CmdStdoutContains => {
                let capture = require_expect_capture(index, step)?;
                let result = self.require_capture(index, capture)?;
                let expected = require_expect_contains(index, step)?;
                if result.stdout.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "capture `{capture}` stdout 不包含期望文本 `{expected}`"
                    ))
                }
            }
            FlowExpectKind::CmdStderrContains => {
                let capture = require_expect_capture(index, step)?;
                let result = self.require_capture(index, capture)?;
                let expected = require_expect_contains(index, step)?;
                if result.stderr.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "capture `{capture}` stderr 不包含期望文本 `{expected}`"
                    ))
                }
            }
            FlowExpectKind::ResponseStatus | FlowExpectKind::ControlStatus => {
                // v1 中 `control_status` 是兼容别名,两者都检查最新 inner @response 的 code。
                let expected = step.code.ok_or_else(|| {
                    format!("@flow.steps[{index}].Expect.code 对 {:?} 必填", step.kind)
                })?;
                let actual = self.latest_response_code().unwrap_or(0);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "control response code 期望 {expected},实际 {actual}"
                    ))
                }
            }
            FlowExpectKind::ResponseContains => {
                let expected = require_expect_contains(index, step)?;
                let Some(line) = self.response_lines.last() else {
                    return Err("还没有可用于 response_contains 的 ControlLine response".to_owned());
                };
                if line.contains(expected) {
                    Ok(())
                } else {
                    Err(format!("最新 control response 不包含期望文本 `{expected}`"))
                }
            }
            FlowExpectKind::FileExists => {
                let path = step
                    .path
                    .as_deref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.path 缺失"))?;
                if Path::new(path).exists() {
                    Ok(())
                } else {
                    Err(format!("daemon 本机文件不存在: {path}"))
                }
            }
            FlowExpectKind::ArtifactExists => {
                let artifact = step
                    .artifact
                    .as_deref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.artifact 缺失"))?;
                if self.artifacts.iter().any(|name| name == artifact) {
                    Ok(())
                } else {
                    Err(format!("artifact 不存在: {artifact}"))
                }
            }
        }
    }

    fn execute_control_line(
        &mut self,
        _index: usize,
        line: &str,
        executor: &mut dyn FnMut(&str) -> ControlExecutionOutcome,
    ) -> Result<(), String> {
        let outcome = executor(line);
        for frame in outcome.outbound_frames {
            match frame {
                ControlFrame::ResponseLine(line) => {
                    self.record_response_line(line);
                }
                ControlFrame::SaveFile(frame) => {
                    self.artifacts.push(frame.filename.clone());
                    self.outbound_frames.push(ControlFrame::SaveFile(frame));
                }
                ControlFrame::PtyReady(_)
                | ControlFrame::PtyOutput(_)
                | ControlFrame::PtyExit(_)
                | ControlFrame::PtyClosed(_)
                | ControlFrame::PtyDetached(_)
                | ControlFrame::PtyAttached(_) => {
                    return Err("ControlLine v1 不支持 PTY outbound frame".to_owned())
                }
            }
        }
        Ok(())
    }

    fn save_artifact(
        &mut self,
        request_id: Option<u64>,
        index: usize,
        step: &FlowSaveArtifactStep,
    ) -> Result<(), String> {
        let path = Path::new(&step.path);
        let metadata = std::fs::metadata(path).map_err(|err| {
            format!("@flow.steps[{index}].SaveArtifact 读取 metadata 失败: {err}")
        })?;
        if !metadata.is_file() {
            return Err(format!(
                "@flow.steps[{index}].SaveArtifact 只支持 regular file: {}",
                step.path
            ));
        }
        let max_bytes = step.max_bytes.unwrap_or(MAX_FLOW_OUTPUT_BYTES);
        if metadata.len() > max_bytes as u64 {
            return Err(format!(
                "@flow.steps[{index}].SaveArtifact 文件大小 {} 超过 max_bytes {max_bytes}",
                metadata.len()
            ));
        }

        let bytes = std::fs::read(path)
            .map_err(|err| format!("@flow.steps[{index}].SaveArtifact 读取文件失败: {err}"))?;
        let filename = step
            .filename
            .clone()
            .or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .ok_or_else(|| {
                format!(
                    "@flow.steps[{index}].SaveArtifact 无法从路径推导 filename: {}",
                    step.path
                )
            })?;
        let frame = SaveFileFrame {
            request_id,
            filename: filename.clone(),
            mime: step
                .mime
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_owned()),
            encoding: "base64".to_owned(),
            data: BASE64_STANDARD.encode(bytes),
            quality: None,
            width: None,
            height: None,
        };
        self.artifacts.push(filename);
        self.outbound_frames.push(ControlFrame::SaveFile(frame));
        Ok(())
    }

    fn require_capture(&self, index: usize, capture: &str) -> Result<&FlowCommandResult, String> {
        self.captures.get(capture).ok_or_else(|| {
            format!("@flow.steps[{index}].Expect 引用了不存在的 capture `{capture}`")
        })
    }

    fn record_response_line(&mut self, line: String) {
        if let Some(value) = parse_response_value(&line) {
            self.response_values.push(value);
        }
        self.response_lines.push(line);
    }

    fn latest_response_code(&self) -> Option<i32> {
        let value = self.response_values.last()?;
        value
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                value
                    .get("value")
                    .and_then(|value| value.get("code"))
                    .and_then(serde_json::Value::as_i64)
            })
            .and_then(|code| i32::try_from(code).ok())
    }

    fn record_trace(&mut self, index: usize, kind: &str, status: &str, error: Option<&str>) {
        self.trace_records.push(serde_json::json!({
            "index": index,
            "kind": kind,
            "status": status,
            "error": error,
        }));
    }

    fn finish(self) -> FlowRuntimeOutput {
        let trace_record_count = self.trace_records.len();
        let report = FlowRunReport {
            schema: self.schema,
            total_steps: self.total_steps,
            completed_steps: self.completed_steps,
            exit_requested: self.exit_requested,
            failed_step: self.failed_step,
            captures: self.captures,
            response_lines: self.response_lines,
            artifacts: self.artifacts,
            trace_record_count,
        };
        FlowRuntimeOutput {
            report,
            outbound_frames: self.outbound_frames,
            trace_records: self.trace_records,
        }
    }
}

fn build_flow_response_line(request_id: Option<u64>, report: &FlowRunReport) -> String {
    let payload = match request_id {
        Some(id) => serde_json::json!({
            "id": id,
            "value": report.to_value(),
        }),
        None => serde_json::json!({
            "value": report.to_value(),
        }),
    };
    format!("@response {}", payload)
}

fn build_trace_savefile_frame(
    request_id: Option<u64>,
    request: &FlowRequest,
    report: &FlowRunReport,
    trace_records: &[serde_json::Value],
) -> Option<SaveFileFrame> {
    if request.options.trace != FlowTraceMode::SaveFile {
        return None;
    }

    let mut jsonl = String::new();
    for record in trace_records {
        jsonl.push_str(&record.to_string());
        jsonl.push('\n');
    }
    let id_label = request_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "no-id".to_owned());

    Some(SaveFileFrame {
        request_id,
        filename: format!("flow-trace-{id_label}.jsonl"),
        mime: "application/jsonl".to_owned(),
        encoding: "base64".to_owned(),
        data: BASE64_STANDARD.encode(jsonl.as_bytes()),
        quality: None,
        width: None,
        height: None,
    })
    .filter(|_| report.trace_record_count > 0)
}

fn execute_sleep_step(ms: u64, flow_deadline: Instant) -> Result<(), String> {
    let duration = Duration::from_millis(ms);
    let remaining = remaining_duration(flow_deadline);
    if duration > remaining {
        thread::sleep(remaining);
        return Err(format!(
            "SleepMs:{ms} 超过 @flow.policy.timeout_ms 剩余时间"
        ));
    }
    thread::sleep(duration);
    Ok(())
}

fn ensure_flow_has_time(flow_deadline: Instant) -> Result<(), String> {
    if Instant::now() >= flow_deadline {
        Err("@flow.policy.timeout_ms 已耗尽".to_owned())
    } else {
        Ok(())
    }
}

fn remaining_duration(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn require_expect_capture<'a>(index: usize, step: &'a FlowExpectStep) -> Result<&'a str, String> {
    step.capture
        .as_deref()
        .ok_or_else(|| format!("@flow.steps[{index}].Expect.capture 缺失"))
}

fn require_expect_contains<'a>(index: usize, step: &'a FlowExpectStep) -> Result<&'a str, String> {
    step.contains
        .as_deref()
        .ok_or_else(|| format!("@flow.steps[{index}].Expect.contains 缺失"))
}

fn parse_response_value(line: &str) -> Option<serde_json::Value> {
    let payload = line.trim_start().strip_prefix("@response ")?;
    serde_json::from_str::<serde_json::Value>(payload).ok()
}

fn control_line_kind(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let control = trimmed.strip_prefix('@')?;
    if control.starts_with('@') {
        return None;
    }
    let header = control
        .split_once(':')
        .map_or(control, |(header, _)| header);
    let kind = header
        .split_once('#')
        .map_or(header, |(kind, _)| kind)
        .trim();
    if kind.is_empty() {
        return None;
    }
    Some(kind.to_ascii_lowercase())
}

fn default_flow_timeout_ms() -> u64 {
    DEFAULT_FLOW_TIMEOUT_MS
}

fn default_flow_max_steps() -> usize {
    DEFAULT_FLOW_MAX_STEPS
}

fn default_flow_max_output_bytes() -> usize {
    DEFAULT_FLOW_MAX_OUTPUT_BYTES
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
