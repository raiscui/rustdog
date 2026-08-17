use std::{
    collections::BTreeMap,
    io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;

use crate::{
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_protocol::{parse_control_line, ControlCommand, ControlParseResult},
};

mod process;

use self::process::{execute_cmd_step, execute_script_step};

pub(crate) const FLOW_SCHEMA_V1: &str = "rdog.flow.v1";
pub(crate) const DEFAULT_FLOW_TIMEOUT_MS: u64 = 30_000;
pub(crate) const MAX_FLOW_TIMEOUT_MS: u64 = 120_000;
pub(crate) const DEFAULT_FLOW_MAX_STEPS: usize = 64;
pub(crate) const MAX_FLOW_STEPS: usize = 256;
pub(crate) const DEFAULT_FLOW_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_FLOW_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_GUI_TRANSACTION_ACTIONS: usize = 20;

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
    /// ticket 19: allow @computer-act as ControlLine step (默认 false, deny-by-default)
    #[serde(default)]
    pub(crate) allow_computer_act: bool,
    #[serde(default = "default_flow_timeout_ms")]
    pub(crate) timeout_ms: u64,
    #[serde(default = "default_flow_max_steps")]
    pub(crate) max_steps: usize,
    #[serde(default = "default_flow_max_output_bytes")]
    pub(crate) max_output_bytes: usize,
    #[serde(default)]
    pub(crate) execution: FlowExecutionPolicy,
}

/// `@flow` 内的 GUI 执行模式。
///
/// 严格后台只改变 side-effect gate,不改变 observe/query 或 AX value 的读取和
/// 语义操作。默认 interactive 保持现有 flow 行为,避免无请求地收紧旧调用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowExecutionPolicy {
    #[serde(default)]
    pub(crate) strict_background: bool,
}

impl Default for FlowExecutionPolicy {
    fn default() -> Self {
        Self {
            strict_background: false,
        }
    }
}

impl Default for FlowPolicy {
    fn default() -> Self {
        Self {
            allow_shell: false,
            allow_file_read: false,
            // ticket 19: deny-by-default, 必须显式 allow_computer_act:true
            allow_computer_act: false,
            timeout_ms: DEFAULT_FLOW_TIMEOUT_MS,
            max_steps: DEFAULT_FLOW_MAX_STEPS,
            max_output_bytes: DEFAULT_FLOW_MAX_OUTPUT_BYTES,
            execution: FlowExecutionPolicy::default(),
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
    GuiTransaction(FlowGuiTransactionStep),
    SleepMs(u64),
    Expect(FlowExpectStep),
    SaveArtifact(FlowSaveArtifactStep),
    Exit,
}

/// 同一 GUI resource 的 checked action 序列。
///
/// 第一条 action 必须携带真实 `{ref, observation_id}`、request-level
/// `observation_id` 和 `epoch`。后续 action 的三个字段都从 `$successor` 绑定,
/// 避免 agent 在一个事务中手工复制漂移 ref。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowGuiTransactionStep {
    pub(crate) actions: Vec<String>,
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
    /// ticket 20: response_field_equals 用, 期望 value (JSON 值, 跟 serde_json::Value 比较)
    #[serde(default)]
    pub(crate) value: Option<serde_json::Value>,
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
    /// ticket 20: 断言 response_value 上 path 指向的字段 == value
    #[serde(rename = "response_field_equals")]
    ResponseFieldEquals,
    /// ticket 20: 断言 response_value 上 path 指向的字段的 stringified 值 contains substring
    #[serde(rename = "response_path_contains")]
    ResponsePathContains,
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
    pub(crate) checked_transactions: Vec<FlowTransactionReport>,
    pub(crate) trace_record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowTransactionReport {
    pub(crate) step_index: usize,
    pub(crate) total_actions: usize,
    pub(crate) completed_actions: usize,
    pub(crate) stopped_at: Option<usize>,
    pub(crate) successor_target: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
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
            "checked_transactions": self
                .checked_transactions
                .iter()
                .map(|transaction| {
                    serde_json::json!({
                        "step_index": transaction.step_index,
                        "total_actions": transaction.total_actions,
                        "completed_actions": transaction.completed_actions,
                        "stopped_at": transaction.stopped_at,
                        "successor_target": transaction.successor_target,
                        "error": transaction.error,
                    })
                })
                .collect::<Vec<_>>(),
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
            FlowStep::GuiTransaction(step) => match control_line_executor.as_deref_mut() {
                Some(executor) => state.execute_gui_transaction(index, step, executor),
                None => Err(
                    "GuiTransaction runtime 需要 control_core executor,当前 shell lane 未提供"
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
    let mut has_computer_act_step = false;
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
            FlowStep::ControlLine(line) => {
                validate_control_line_step(index, line)?;
                validate_flow_execution_policy(index, line, &request.policy.execution)?;
                // ticket 19: 标记 @computer-act ControlLine 让后续 policy 校验
                if control_line_kind(line).as_deref() == Some("computer-act") {
                    has_computer_act_step = true;
                }
            }
            FlowStep::GuiTransaction(step) => {
                validate_gui_transaction_step(index, step)?;
                for (action_index, line) in step.actions.iter().enumerate() {
                    validate_flow_execution_policy(index, line, &request.policy.execution)
                        .map_err(|err| {
                            invalid_data(format!(
                                "@flow.steps[{index}].GuiTransaction.actions[{action_index}] 严格后台校验失败: {err}"
                            ))
                        })?;
                }
                has_computer_act_step = true;
            }
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
    // ticket 19: @computer-act ControlLine opt-in gate (跟 allow_shell / allow_file_read 同款 deny-by-default)
    if has_computer_act_step && !request.policy.allow_computer_act {
        return Err(invalid_data(
            "@flow 包含 @computer-act ControlLine 时必须显式设置 policy.allow_computer_act:true",
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
        FlowExpectKind::ResponseFieldEquals => {
            require_expected_field(index, "Expect.path", step.path.as_deref())?;
            // value 必填 (用 Option<Value>::is_none 判断, 因为 false / null / 0 / "" 都是合法)
            if step.value.is_none() {
                return Err(invalid_data(format!(
                    "@flow.steps[{index}].Expect.value 对 response_field_equals 必填"
                )));
            }
        }
        FlowExpectKind::ResponsePathContains => {
            require_expected_field(index, "Expect.path", step.path.as_deref())?;
            require_expected_field(index, "Expect.contains", step.contains.as_deref())?;
        }
    }
    Ok(())
}

/// 校验 checked GUI transaction 的固定边界。
///
/// 首 action 必须是真实 ref + observation + request-level epoch。后续 action
/// 只能消费上一步返回的 successor,这样事务不会偷偷退化成多个独立坐标动作。
fn validate_gui_transaction_step(index: usize, step: &FlowGuiTransactionStep) -> io::Result<()> {
    if step.actions.is_empty() {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].GuiTransaction.actions 不能为空"
        )));
    }
    if step.actions.len() > MAX_GUI_TRANSACTION_ACTIONS {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].GuiTransaction.actions 数量 {} 超过上限 {MAX_GUI_TRANSACTION_ACTIONS}",
            step.actions.len()
        )));
    }

    let first = step.actions.first().expect("actions checked non-empty");
    let first_request = parse_gui_transaction_action(index, 0, first)?;
    let target = first_request
        .args
        .get("target")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            invalid_data(format!(
                "@flow.steps[{index}].GuiTransaction.actions[0] 必须携带 args.target {{ref,observation_id}}"
            ))
        })?;
    require_transaction_target_field(index, 0, target, "ref")?;
    require_transaction_target_field(index, 0, target, "observation_id")?;
    let target_observation_id = target
        .get("observation_id")
        .and_then(serde_json::Value::as_str)
        .expect("target observation_id validated");
    if first_request.observation_id.as_deref() != Some(target_observation_id) {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].GuiTransaction.actions[0] 顶层 observation_id 必须与 args.target.observation_id 一致"
        )));
    }
    if first_request.epoch.is_none() {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].GuiTransaction.actions[0] 必须携带 request-level epoch"
        )));
    }
    if target
        .keys()
        .any(|key| key != "ref" && key != "observation_id")
    {
        return Err(invalid_data(format!(
            "@flow.steps[{index}].GuiTransaction.actions[0].args.target 只支持 ref 和 observation_id"
        )));
    }

    for (action_index, line) in step.actions.iter().enumerate().skip(1) {
        if !has_successor_marker(line) {
            return Err(invalid_data(format!(
                "@flow.steps[{index}].GuiTransaction.actions[{action_index}] 必须使用 target:$successor,observation_id:$successor 和 epoch:$successor"
            )));
        }
        validate_successor_template(index, action_index, line)?;
        let template_successor = serde_json::json!({
            "ref": "@successor",
            "observation_id": "successor-observation",
            "epoch": 0,
        });
        let materialized = materialize_successor_action(line, &template_successor).map_err(|err| {
            invalid_data(format!(
                "@flow.steps[{index}].GuiTransaction.actions[{action_index}] successor 模板无效: {err}"
            ))
        })?;
        parse_gui_transaction_action(index, action_index, &materialized)?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlowExecutionPolicyViolation {
    code: &'static str,
    message: String,
}

fn validate_flow_execution_policy(
    index: usize,
    line: &str,
    policy: &FlowExecutionPolicy,
) -> io::Result<()> {
    if !policy.strict_background {
        return Ok(());
    }
    enforce_strict_background_line(line, policy).map_err(|violation| {
        invalid_data(format!(
            "@flow.steps[{index}] {}: {}",
            violation.code, violation.message
        ))
    })
}

fn enforce_strict_background_line(
    line: &str,
    policy: &FlowExecutionPolicy,
) -> Result<(), FlowExecutionPolicyViolation> {
    if !policy.strict_background {
        return Ok(());
    }

    let parsed = match parse_control_line(line) {
        Ok(parsed) => parsed,
        Err(_) if has_successor_marker(line) => {
            return enforce_strict_background_template(line);
        }
        Err(err) => {
            return Err(FlowExecutionPolicyViolation {
                code: "invalid_execution_request",
                message: format!("严格后台策略无法解析 control request: {err}"),
            })
        }
    };
    let ControlParseResult::Control(request) = parsed else {
        return Ok(());
    };
    command_execution_policy_violation(&request.command).map_or(Ok(()), Err)
}

fn enforce_strict_background_template(line: &str) -> Result<(), FlowExecutionPolicyViolation> {
    let action = template_field(line, "action").unwrap_or_default();
    if matches!(
        action.as_str(),
        "click"
            | "doubleclick"
            | "triple_click"
            | "right_single"
            | "hover"
            | "scroll"
            | "drag"
            | "hotkey"
            | "hotkey_click"
    ) {
        return Err(FlowExecutionPolicyViolation {
            code: "physical_input_prohibited",
            message: format!(
                "strict_background 禁止 @computer-act action={action},因为它需要 physical input"
            ),
        });
    }
    if matches!(action.as_str(), "open_app" | "open_url") {
        return Err(FlowExecutionPolicyViolation {
            code: "foreground_prohibited",
            message: format!(
                "strict_background 禁止 @computer-act action={action},因为它可能激活前台窗口"
            ),
        });
    }
    if action == "type" {
        let mode = template_field(line, "mode").unwrap_or_else(|| "ax-value".to_owned());
        if mode != "ax-value" && mode != "ax_value" {
            return Err(FlowExecutionPolicyViolation {
                code: "physical_input_prohibited",
                message: format!("strict_background 只允许 type 的 AX value 模式,当前 mode={mode}"),
            });
        }
    }
    Ok(())
}

fn command_execution_policy_violation(
    command: &ControlCommand,
) -> Option<FlowExecutionPolicyViolation> {
    match command {
        ControlCommand::Key(_) | ControlCommand::Paste(_) => Some(
            FlowExecutionPolicyViolation {
                code: "physical_input_prohibited",
                message: "strict_background 禁止 raw keyboard 或 legacy paste".to_owned(),
            },
        ),
        ControlCommand::MouseMove(_)
        | ControlCommand::MouseButton(_)
        | ControlCommand::Click(_)
        | ControlCommand::Drag(_)
        | ControlCommand::Wheel(_) => Some(FlowExecutionPolicyViolation {
            code: "physical_input_prohibited",
            message: "strict_background 禁止 raw pointer action".to_owned(),
        }),
        ControlCommand::WindowActivate(_)
        | ControlCommand::WindowResize(_)
        | ControlCommand::OpenApp(_) => Some(FlowExecutionPolicyViolation {
            code: "foreground_prohibited",
            message: "strict_background 禁止 activate/raise/open-app; window-resize 也不能隐式恢复前台窗口".to_owned(),
        }),
        ControlCommand::AxFocus(request) if request.activate => Some(
            FlowExecutionPolicyViolation {
                code: "foreground_prohibited",
                message: "strict_background 禁止 @ax-focus activate:true".to_owned(),
            },
        ),
        ControlCommand::TypeText(request)
            if !matches!(request.mode, crate::control_ax::TypeTextMode::AxValue) =>
        {
            Some(FlowExecutionPolicyViolation {
                code: "physical_input_prohibited",
                message: "strict_background 只允许 @type-text 的 AX value 模式".to_owned(),
            })
        }
        ControlCommand::ComputerAct(request) => computer_act_policy_violation(request),
        ControlCommand::Composite(commands) => commands
            .iter()
            .find_map(command_execution_policy_violation),
        _ => None,
    }
}

fn computer_act_policy_violation(
    request: &crate::control_protocol::ComputerActRequest,
) -> Option<FlowExecutionPolicyViolation> {
    if matches!(
        request.action.as_str(),
        "click"
            | "doubleclick"
            | "triple_click"
            | "right_single"
            | "hover"
            | "scroll"
            | "drag"
            | "hotkey"
            | "hotkey_click"
    ) {
        return Some(FlowExecutionPolicyViolation {
            code: "physical_input_prohibited",
            message: format!(
                "strict_background 禁止 @computer-act action={},因为它需要 physical input",
                request.action
            ),
        });
    }
    if matches!(request.action.as_str(), "open_app" | "open_url") {
        return Some(FlowExecutionPolicyViolation {
            code: "foreground_prohibited",
            message: format!(
                "strict_background 禁止 @computer-act action={},因为它可能激活前台窗口",
                request.action
            ),
        });
    }
    if request.action == "type" {
        let mode = request
            .args
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ax-value");
        if request.args.get("target").is_none() || !matches!(mode, "ax-value" | "ax_value") {
            return Some(FlowExecutionPolicyViolation {
                code: "physical_input_prohibited",
                message: "strict_background 只允许带 target 的 AX value type,禁止 legacy/keyboard/clipboard 输入".to_owned(),
            });
        }
    }
    None
}

fn template_field(line: &str, field: &str) -> Option<String> {
    let normalized = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    for prefix in [format!("{field}:\""), format!("\"{field}\":\"")] {
        if let Some(prefix_start) = normalized.find(&prefix) {
            let start = prefix_start + prefix.len();
            let end = normalized[start..].find('"')? + start;
            return Some(normalized[start..end].to_owned());
        }
    }
    None
}

fn parse_gui_transaction_action(
    step_index: usize,
    action_index: usize,
    line: &str,
) -> io::Result<crate::control_protocol::ComputerActRequest> {
    let parsed = parse_control_line(line).map_err(|err| {
        invalid_data(format!(
            "@flow.steps[{step_index}].GuiTransaction.actions[{action_index}] 解析失败: {err}"
        ))
    })?;
    let ControlParseResult::Control(request) = parsed else {
        return Err(invalid_data(format!(
            "@flow.steps[{step_index}].GuiTransaction.actions[{action_index}] 必须是显式 @computer-act request"
        )));
    };
    if !matches!(request.command, ControlCommand::ComputerAct(_)) {
        return Err(invalid_data(format!(
            "@flow.steps[{step_index}].GuiTransaction.actions[{action_index}] 只能使用 @computer-act"
        )));
    }
    match request.command {
        ControlCommand::ComputerAct(request) => Ok(request),
        _ => unreachable!("command kind checked above"),
    }
}

fn require_transaction_target_field(
    step_index: usize,
    action_index: usize,
    target: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> io::Result<()> {
    let valid = target
        .get(field)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if valid {
        Ok(())
    } else {
        Err(invalid_data(format!(
            "@flow.steps[{step_index}].GuiTransaction.actions[{action_index}].args.target.{field} 必须是非空字符串"
        )))
    }
}

fn validate_successor_template(
    step_index: usize,
    action_index: usize,
    line: &str,
) -> io::Result<()> {
    // 模板先做最小结构检查。具体 ref/observation/epoch 在 runtime 绑定上一响应时注入。
    let has_target = line.contains("target:\"$successor\"")
        || line.contains("target:'$successor'")
        || line.contains("\"target\":\"$successor\"");
    let has_observation_id = line.contains("observation_id:\"$successor\"")
        || line.contains("observation_id:'$successor'")
        || line.contains("\"observation_id\":\"$successor\"");
    let has_epoch = line.contains("epoch:$successor")
        || line.contains("epoch:\"$successor\"")
        || line.contains("\"epoch\":\"$successor\"");
    if !has_target || !has_observation_id || !has_epoch {
        return Err(invalid_data(format!(
            "@flow.steps[{step_index}].GuiTransaction.actions[{action_index}] successor 模板必须同时包含 target:$successor、observation_id:$successor 和 epoch:$successor"
        )));
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
            Self::GuiTransaction(_) => "GuiTransaction",
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
    checked_transactions: Vec<FlowTransactionReport>,
    trace_records: Vec<serde_json::Value>,
    outbound_frames: Vec<ControlFrame>,
    execution_policy: FlowExecutionPolicy,
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
            checked_transactions: Vec::new(),
            trace_records: Vec::new(),
            outbound_frames: Vec::new(),
            execution_policy: request.policy.execution,
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
            // ticket 20: JSON-pointer-like path navigation
            FlowExpectKind::ResponseFieldEquals => {
                let path = step
                    .path
                    .as_deref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.path 缺失"))?;
                let expected = step
                    .value
                    .as_ref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.value 缺失"))?;
                let value = self.response_values.last().ok_or_else(|| {
                    "还没有可用于 response_field_equals 的 ControlLine response".to_owned()
                })?;
                let actual = json_pointer_lookup(value, path)
                    .ok_or_else(|| format!("path `{path}` 在最新 response 中不存在"))?;
                if &actual == expected {
                    Ok(())
                } else {
                    Err(format!("path `{path}` 期望 {expected}, 实际 {actual}"))
                }
            }
            FlowExpectKind::ResponsePathContains => {
                let path = step
                    .path
                    .as_deref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.path 缺失"))?;
                let expected_substring = step
                    .contains
                    .as_deref()
                    .ok_or_else(|| format!("@flow.steps[{index}].Expect.contains 缺失"))?;
                let value = self.response_values.last().ok_or_else(|| {
                    "还没有可用于 response_path_contains 的 ControlLine response".to_owned()
                })?;
                let actual = json_pointer_lookup(value, path)
                    .ok_or_else(|| format!("path `{path}` 在最新 response 中不存在"))?;
                let actual_str = json_value_to_string(&actual);
                if actual_str.contains(expected_substring) {
                    Ok(())
                } else {
                    Err(format!(
                        "path `{path}` 实际值 `{actual_str}` 不包含期望子串 `{expected_substring}`"
                    ))
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
        if let Err(violation) = enforce_strict_background_line(line, &self.execution_policy) {
            let response = serde_json::json!({
                "ok": false,
                "error_code": violation.code,
                "error_message": violation.message,
                "retry": {
                    "strategy": "never",
                    "hint": "strict_background 只允许语义 AX 操作,请改用 AX value/action 或关闭该 flow policy"
                },
                "evidence": {
                    "execution_policy": "strict_background"
                }
            });
            self.record_response_line(format!("@response {response}"));
            return Err(format!("{}: {}", violation.code, violation.message));
        }
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

    fn execute_gui_transaction(
        &mut self,
        step_index: usize,
        step: &FlowGuiTransactionStep,
        executor: &mut dyn FnMut(&str) -> ControlExecutionOutcome,
    ) -> Result<(), String> {
        let mut successor_target = None;
        let mut completed_actions = 0;

        for (action_index, line) in step.actions.iter().enumerate() {
            let materialized = if has_successor_marker(line) {
                let target = successor_target.as_ref().ok_or_else(|| {
                    format!(
                        "GuiTransaction action {action_index} 需要上一条 action 的 successor_target"
                    )
                })?;
                materialize_successor_action(line, target)?
            } else {
                line.clone()
            };

            let response_count_before = self.response_values.len();
            self.execute_control_line(step_index, &materialized, executor)?;
            let Some(response) = self.response_values.get(response_count_before) else {
                return self.fail_gui_transaction(
                    step_index,
                    step,
                    completed_actions,
                    Some(action_index),
                    successor_target,
                    "action 没有产生可检查的 @response".to_owned(),
                );
            };

            let response_code = response_code(response).unwrap_or(64);
            if response_code != 0 {
                return self.fail_gui_transaction(
                    step_index,
                    step,
                    completed_actions,
                    Some(action_index),
                    successor_target,
                    format!("action response code 为 {response_code}"),
                );
            }

            successor_target = response_successor_target(response);
            if successor_target.is_none() {
                return self.fail_gui_transaction(
                    step_index,
                    step,
                    completed_actions,
                    Some(action_index),
                    successor_target,
                    "action 成功但没有 successor_target,事务拒绝继续".to_owned(),
                );
            }
            completed_actions += 1;
        }

        self.checked_transactions.push(FlowTransactionReport {
            step_index,
            total_actions: step.actions.len(),
            completed_actions,
            stopped_at: None,
            successor_target,
            error: None,
        });
        Ok(())
    }

    fn fail_gui_transaction(
        &mut self,
        step_index: usize,
        step: &FlowGuiTransactionStep,
        completed_actions: usize,
        stopped_at: Option<usize>,
        successor_target: Option<serde_json::Value>,
        error: String,
    ) -> Result<(), String> {
        self.checked_transactions.push(FlowTransactionReport {
            step_index,
            total_actions: step.actions.len(),
            completed_actions,
            stopped_at,
            successor_target,
            error: Some(error.clone()),
        });
        Err(error)
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
            checked_transactions: self.checked_transactions,
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

fn has_successor_marker(line: &str) -> bool {
    (line.contains("target:\"$successor\"")
        || line.contains("target:'$successor'")
        || line.contains("\"target\":\"$successor\""))
        && (line.contains("observation_id:\"$successor\"")
            || line.contains("observation_id:'$successor'")
            || line.contains("\"observation_id\":\"$successor\""))
        && (line.contains("epoch:$successor")
            || line.contains("epoch:\"$successor\"")
            || line.contains("\"epoch\":\"$successor\""))
}

/// 将 successor target 注入下一条 `@computer-act` 模板。
///
/// 只替换约定的三个占位符,保留 action id 和其余 args 原文。epoch 放在 request
/// 顶层,不放进 target,以匹配现有 computer-act parser 和 type action 的严格字段边界。
fn materialize_successor_action(
    line: &str,
    successor_target: &serde_json::Value,
) -> Result<String, String> {
    let target = successor_target
        .as_object()
        .ok_or_else(|| "successor_target 不是对象".to_owned())?;
    let ref_id = target
        .get("ref")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "successor_target 缺少非空 ref".to_owned())?;
    let observation_id = target
        .get("observation_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "successor_target 缺少非空 observation_id".to_owned())?;
    let epoch = target
        .get("epoch")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "successor_target 缺少非负整数 epoch".to_owned())?;

    let target_object = format!(
        "{{ref:\"{}\",observation_id:\"{}\"}}",
        escape_flow_string(ref_id),
        escape_flow_string(observation_id)
    );
    let materialized = replace_successor_token(line, "target", &target_object)?;
    let materialized = replace_successor_token(
        &materialized,
        "observation_id",
        &format!("\"{}\"", escape_flow_string(observation_id)),
    )?;
    replace_successor_token(&materialized, "epoch", &epoch.to_string())
}

fn replace_successor_token(line: &str, field: &str, replacement: &str) -> Result<String, String> {
    let candidates = [
        format!("{field}:$successor"),
        format!("{field}:\"$successor\""),
        format!("{field}:'$successor'"),
        format!("\"{field}\":\"$successor\""),
    ];
    let mut matches = candidates
        .iter()
        .filter_map(|candidate| line.find(candidate).map(|index| (index, candidate)))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!("successor 模板的 {field} 占位符必须恰好出现一次"));
    }
    let (index, candidate) = matches.remove(0);
    let mut output = String::with_capacity(line.len() + replacement.len());
    output.push_str(&line[..index]);
    output.push_str(field);
    output.push(':');
    output.push_str(replacement);
    output.push_str(&line[index + candidate.len()..]);
    Ok(output)
}

fn escape_flow_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn response_code(response: &serde_json::Value) -> Option<i32> {
    if let Some(code) = response.get("code").and_then(serde_json::Value::as_i64) {
        return i32::try_from(code).ok();
    }
    let value = response.get("value").unwrap_or(response);
    if let Some(ok) = value.get("ok").and_then(serde_json::Value::as_bool) {
        return Some(if ok { 0 } else { 64 });
    }
    value.as_i64().and_then(|code| i32::try_from(code).ok())
}

fn response_successor_target(response: &serde_json::Value) -> Option<serde_json::Value> {
    let value = response.get("value").unwrap_or(response);
    let target = value.get("successor_target")?.as_object()?;
    let ref_id = target.get("ref")?.as_str()?.to_owned();
    let observation_id = target.get("observation_id")?.as_str()?.to_owned();
    let epoch = target.get("epoch")?.as_u64()?;
    Some(serde_json::json!({
        "ref": ref_id,
        "observation_id": observation_id,
        "epoch": epoch,
    }))
}

/// ticket 20: JSON-pointer-like path navigation。 支持 `$.foo.bar` / `$.foo[0].bar`。
///
/// 语义:
/// - `$` = root
/// - `.key` / `[index]` = 走下层
/// - 不支持 `..` / `$ref` / 其它 RFC 6901 高级特性 (rdog 简化为 dot path, 跟 Mano-CUA 风格一致)
fn json_pointer_lookup(root: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let stripped = path.trim_start_matches('$').trim_start_matches('.');
    if stripped.is_empty() {
        return Some(root.clone());
    }
    let mut current = root.clone();
    // 按 . 分割, 每段再处理可能的 [N] 索引
    for segment in stripped.split('.') {
        if segment.is_empty() {
            continue;
        }
        // 处理 `name[0][1]` 这种混合
        let mut name_part = segment;
        while let Some(idx_start) = name_part.find('[') {
            let name = &name_part[..idx_start];
            let idx_end = name_part.find(']').unwrap_or(name_part.len());
            let idx_str = &name_part[idx_start + 1..idx_end];
            let idx: usize = idx_str.parse().ok()?;
            if !name.is_empty() {
                current = current.get(name)?.clone();
            }
            current = current.get(idx)?.clone();
            name_part = &name_part[idx_end + 1..];
            if name_part.is_empty() {
                break;
            }
        }
        if !name_part.is_empty() {
            current = current.get(name_part)?.clone();
        }
    }
    Some(current)
}

/// ticket 20: JSON value → 字符串, 用于 path_contains 子串匹配。
/// Object/Array 序列化成 compact JSON, 其它原样。
fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        // Object / Array → compact JSON 字符串 (跟 serialize 行为一致)
        other => other.to_string(),
    }
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
