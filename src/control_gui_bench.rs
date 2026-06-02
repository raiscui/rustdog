use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use crate::control_web::{
    build_default_web_act_response_json, build_default_web_find_response_json,
    parse_web_act_payload, parse_web_find_payload,
};
use serde::Serialize;
use serde_json::Value;
use std::{fs, io, path::PathBuf};

#[cfg(test)]
mod tests;

pub const GUI_BENCH_SCHEMA: &str = "rdog.gui-bench.v1";
const COMPUTER_USE_DENSITY_SUITE: &str = "computer-use-density";
const XHS_LEFT_NAV_HOME_CASE: &str = "xhs-left-nav-home";
const ALL_VARIANTS: &str = "all";
const XHS_LEFT_NAV_HOME_FIXTURE: &str =
    include_str!("../tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiBenchRequest {
    pub suite: String,
    pub case_name: String,
    pub variant: String,
    pub runner: GuiBenchRunner,
    pub allow_side_effects: bool,
    pub write_artifact: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GuiBenchRunner {
    Fixture,
    Live,
}

impl GuiBenchRunner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GuiBenchResponse {
    kind: &'static str,
    schema: &'static str,
    status: &'static str,
    runner: &'static str,
    suite: String,
    #[serde(rename = "case")]
    case_name: String,
    variant: String,
    allow_side_effects: bool,
    variant_count: usize,
    dense_target_passed: bool,
    threshold_failures: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thresholds: Option<GuiBenchThresholds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<GuiBenchMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checks: Option<Vec<GuiBenchCheck>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps_summary: Option<GuiBenchStepsSummary>,
    runs: Vec<GuiBenchRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact: Option<GuiBenchArtifact>,
    trace: Vec<GuiBenchTraceStep>,
}

impl GuiBenchResponse {
    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("gui-bench response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GuiBenchRun {
    variant: String,
    status: String,
    dense_target_passed: bool,
    threshold_failures: Vec<String>,
    thresholds: GuiBenchThresholds,
    metrics: GuiBenchMetrics,
    checks: Vec<GuiBenchCheck>,
    steps_summary: GuiBenchStepsSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_replay: Option<GuiBenchLiveReplay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchThresholds {
    max_backend_request_count: u64,
    max_agent_decision_points: u64,
    required_scope: Option<String>,
    required_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchArtifact {
    path: String,
    encoding: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct GuiBenchLiveReplay {
    command: String,
    response_kind: Option<String>,
    response_schema: Option<String>,
    response_status: Option<String>,
    performed: Option<bool>,
    verified: Option<bool>,
    match_count: Option<u64>,
    returned_count: Option<u64>,
    error_code: Option<String>,
    message: Option<String>,
    passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchMetrics {
    backend_request_count: u64,
    control_frame_count: u64,
    agent_decision_points: u64,
    semantic_action_count: u64,
    mouse_fallback_count: u64,
    stale_ref_recovery_count: u64,
    stale_visual_block_count: u64,
    verification_passed: bool,
    false_success_count: u64,
    trace_step_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchCheck {
    name: &'static str,
    passed: bool,
    expected: String,
    actual: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchStepsSummary {
    step_count: usize,
    first_command: Option<String>,
    last_command: Option<String>,
    semantic_action_commands: Vec<String>,
    mouse_fallback_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuiBenchTraceStep {
    step: &'static str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn parse_gui_bench_payload(input: &str) -> io::Result<GuiBenchRequest> {
    let inner = object_inner(input, "@gui-bench")?;
    if inner.is_empty() {
        return Err(invalid_data("@gui-bench 对象 payload 不能为空"));
    }

    let mut suite = None::<String>;
    let mut case_name = None::<String>;
    let mut variant = None::<String>;
    let mut runner = None::<GuiBenchRunner>;
    let mut allow_side_effects = None::<bool>;
    let mut write_artifact = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "suite" => assign_once(
                &mut suite,
                "suite",
                "@gui-bench",
                parse_non_empty_string(raw_value, "@gui-bench.suite")?,
            )?,
            "case" => assign_once(
                &mut case_name,
                "case",
                "@gui-bench",
                parse_non_empty_string(raw_value, "@gui-bench.case")?,
            )?,
            "variant" => assign_once(
                &mut variant,
                "variant",
                "@gui-bench",
                parse_non_empty_string(raw_value, "@gui-bench.variant")?,
            )?,
            "runner" => assign_once(
                &mut runner,
                "runner",
                "@gui-bench",
                parse_runner(raw_value, "@gui-bench.runner")?,
            )?,
            "allow_side_effects" => assign_once(
                &mut allow_side_effects,
                "allow_side_effects",
                "@gui-bench",
                parse_bool(raw_value, "@gui-bench.allow_side_effects")?,
            )?,
            "write_artifact" => assign_once(
                &mut write_artifact,
                "write_artifact",
                "@gui-bench",
                parse_bool(raw_value, "@gui-bench.write_artifact")?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@gui-bench 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(GuiBenchRequest {
        suite: required_field(suite, "@gui-bench", "suite")?,
        case_name: required_field(case_name, "@gui-bench", "case")?,
        variant: required_field(variant, "@gui-bench", "variant")?,
        runner: runner.unwrap_or(GuiBenchRunner::Fixture),
        allow_side_effects: allow_side_effects.unwrap_or(false),
        write_artifact: write_artifact.unwrap_or(false),
    })
}

pub fn build_gui_bench_response_json(request: &GuiBenchRequest) -> io::Result<String> {
    match request.runner {
        GuiBenchRunner::Fixture => build_fixture_gui_bench_response_json(request),
        GuiBenchRunner::Live => build_live_gui_bench_response_json(request),
    }
}

fn build_fixture_gui_bench_response_json(request: &GuiBenchRequest) -> io::Result<String> {
    // 默认 runner 只跑内置 fixture。
    // 这里不启动浏览器、不读取 AX、不执行鼠标或键盘动作。
    let mut trace = vec![trace_step(
        "parse-request",
        "ok",
        Some(format!(
            "suite={},case={},variant={}",
            request.suite, request.case_name, request.variant
        )),
    )];

    let fixture_text = resolve_fixture_text(request, &mut trace)?;
    let fixture = serde_json::from_str::<Value>(fixture_text).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("computer-use density fixture JSON 无效: {err}"),
        )
    })?;
    trace.push(trace_step("load-fixture", "ok", None));

    let selected_variants = select_variants(&fixture, &request.variant)?;
    trace.push(trace_step(
        "select-variants",
        "ok",
        Some(format!("variant_count={}", selected_variants.len())),
    ));

    let thresholds = thresholds_from_fixture(&fixture)?;
    let runs = selected_variants
        .iter()
        .map(|variant| build_run(variant, &thresholds))
        .collect::<io::Result<Vec<_>>>()?;
    let dense_target_passed = runs.iter().all(|run| run.dense_target_passed);
    let threshold_failures = collect_response_failures(&runs);
    let single_run = (runs.len() == 1).then(|| runs[0].clone());

    trace.push(trace_step(
        "evaluate-density",
        if dense_target_passed {
            "passed"
        } else {
            "threshold_failed"
        },
        Some(format!("failed_checks={}", threshold_failures.len())),
    ));

    let mut response = GuiBenchResponse {
        kind: "gui-bench",
        schema: GUI_BENCH_SCHEMA,
        status: "complete",
        runner: GuiBenchRunner::Fixture.as_str(),
        suite: request.suite.clone(),
        case_name: request.case_name.clone(),
        variant: request.variant.clone(),
        allow_side_effects: request.allow_side_effects,
        variant_count: runs.len(),
        dense_target_passed,
        threshold_failures,
        thresholds: single_run.as_ref().map(|run| run.thresholds.clone()),
        metrics: single_run.as_ref().map(|run| run.metrics.clone()),
        checks: single_run.as_ref().map(|run| run.checks.clone()),
        steps_summary: single_run.as_ref().map(|run| run.steps_summary.clone()),
        runs,
        artifact: None,
        trace,
    };

    if request.write_artifact {
        let artifact = prepare_artifact(request)?;
        response.trace.push(trace_step(
            "write-artifact",
            "ok",
            Some(artifact.path.clone()),
        ));
        response.artifact = Some(artifact);
        write_artifact(&response)?;
    }

    response.to_value_json()
}

fn resolve_fixture_text<'a>(
    request: &GuiBenchRequest,
    trace: &mut Vec<GuiBenchTraceStep>,
) -> io::Result<&'a str> {
    if request.suite != COMPUTER_USE_DENSITY_SUITE {
        return Err(invalid_data(format!(
            "@gui-bench Phase 3A 暂不支持 suite `{}`",
            request.suite
        )));
    }
    if request.case_name != XHS_LEFT_NAV_HOME_CASE {
        return Err(invalid_data(format!(
            "@gui-bench Phase 3A 暂不支持 case `{}`",
            request.case_name
        )));
    }
    trace.push(trace_step(
        "resolve-fixture",
        "ok",
        Some("tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json".to_owned()),
    ));
    Ok(XHS_LEFT_NAV_HOME_FIXTURE)
}

fn build_live_gui_bench_response_json(request: &GuiBenchRequest) -> io::Result<String> {
    build_live_gui_bench_response_json_with(
        request,
        build_default_web_find_response_json,
        build_default_web_act_response_json,
    )
}

fn build_live_gui_bench_response_json_with<F, A>(
    request: &GuiBenchRequest,
    mut build_web_find: F,
    mut build_web_act: A,
) -> io::Result<String>
where
    F: FnMut(&crate::control_web::WebFindRequest) -> io::Result<String>,
    A: FnMut(&crate::control_web::WebActRequest) -> io::Result<String>,
{
    // Phase 3D 的 live runner 是显式 opt-in。
    // 任何真实 GUI 读取或 action 都必须先通过 runner + consent 双门闸。
    let mut trace = vec![trace_step(
        "parse-request",
        "ok",
        Some(format!(
            "suite={},case={},variant={},runner={}",
            request.suite,
            request.case_name,
            request.variant,
            request.runner.as_str()
        )),
    )];

    if !request.allow_side_effects {
        return Err(invalid_data(
            "@gui-bench runner:\"live\" 必须显式设置 allow_side_effects:true",
        ));
    }
    if request.variant == ALL_VARIANTS {
        return Err(invalid_data(
            "@gui-bench runner:\"live\" 不支持 variant:\"all\",请一次只 replay 一个 variant",
        ));
    }

    let fixture_text = resolve_fixture_text(request, &mut trace)?;
    let fixture = serde_json::from_str::<Value>(fixture_text).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("computer-use density fixture JSON 无效: {err}"),
        )
    })?;
    trace.push(trace_step("load-fixture", "ok", None));

    let selected_variants = select_variants(&fixture, &request.variant)?;
    if selected_variants.len() != 1 {
        return Err(invalid_data("@gui-bench live replay 必须选择单个 variant"));
    }

    let thresholds = thresholds_from_fixture(&fixture)?;
    let mut run = build_run(selected_variants[0], &thresholds)?;
    let replay = replay_live_variant(
        &request.variant,
        &run.steps_summary,
        &mut trace,
        &mut build_web_find,
        &mut build_web_act,
    )?;
    run.live_replay = Some(replay.clone());
    if !replay.passed {
        if !run
            .threshold_failures
            .iter()
            .any(|name| name == "live_replay")
        {
            run.threshold_failures.push("live_replay".to_owned());
        }
        run.dense_target_passed = false;
        run.checks.push(GuiBenchCheck {
            name: "live_replay",
            passed: false,
            expected: "complete response with verified side-effect result when applicable"
                .to_owned(),
            actual: replay_actual_summary(&replay),
        });
    }

    let runs = vec![run];
    let dense_target_passed = runs.iter().all(|run| run.dense_target_passed);
    let threshold_failures = collect_response_failures(&runs);
    trace.push(trace_step(
        "evaluate-live-replay",
        if dense_target_passed {
            "passed"
        } else {
            "threshold_failed"
        },
        Some(format!("failed_checks={}", threshold_failures.len())),
    ));

    let single_run = runs[0].clone();
    let mut response = GuiBenchResponse {
        kind: "gui-bench",
        schema: GUI_BENCH_SCHEMA,
        status: if dense_target_passed {
            "complete"
        } else {
            "threshold_failed"
        },
        runner: GuiBenchRunner::Live.as_str(),
        suite: request.suite.clone(),
        case_name: request.case_name.clone(),
        variant: request.variant.clone(),
        allow_side_effects: request.allow_side_effects,
        variant_count: runs.len(),
        dense_target_passed,
        threshold_failures,
        thresholds: Some(single_run.thresholds.clone()),
        metrics: Some(single_run.metrics.clone()),
        checks: Some(single_run.checks.clone()),
        steps_summary: Some(single_run.steps_summary.clone()),
        runs,
        artifact: None,
        trace,
    };

    if request.write_artifact {
        let artifact = prepare_artifact(request)?;
        response.trace.push(trace_step(
            "write-artifact",
            "ok",
            Some(artifact.path.clone()),
        ));
        response.artifact = Some(artifact);
        write_artifact(&response)?;
    }

    response.to_value_json()
}

fn replay_live_variant<F, A>(
    variant: &str,
    steps_summary: &GuiBenchStepsSummary,
    trace: &mut Vec<GuiBenchTraceStep>,
    build_web_find: &mut F,
    build_web_act: &mut A,
) -> io::Result<GuiBenchLiveReplay>
where
    F: FnMut(&crate::control_web::WebFindRequest) -> io::Result<String>,
    A: FnMut(&crate::control_web::WebActRequest) -> io::Result<String>,
{
    let command = steps_summary
        .first_command
        .as_deref()
        .ok_or_else(|| invalid_data("live replay variant 缺少可执行 command"))?;

    trace.push(trace_step(
        "live-replay",
        "attempt",
        Some(format!("variant={variant},command={command}")),
    ));

    let response_json = if command.starts_with("@web-find") {
        let payload = command_payload(command, "@web-find")?;
        let request = parse_web_find_payload(payload)?;
        build_web_find(&request)?
    } else if command.starts_with("@web-act") {
        let payload = command_payload(command, "@web-act")?;
        let request = parse_web_act_payload(payload)?;
        build_web_act(&request)?
    } else {
        return Err(invalid_data(format!(
            "@gui-bench live replay 暂不支持执行命令: {command}"
        )));
    };

    let response = serde_json::from_str::<Value>(&response_json).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("live replay response JSON 无效: {err}"),
        )
    })?;
    let replay = summarize_live_replay(command, &response);
    trace.push(trace_step(
        "live-replay",
        if replay.passed { "passed" } else { "failed" },
        Some(replay_actual_summary(&replay)),
    ));
    Ok(replay)
}

fn summarize_live_replay(command: &str, response: &Value) -> GuiBenchLiveReplay {
    let response_kind = optional_response_string(response, "kind");
    let response_schema = optional_response_string(response, "schema");
    let response_status = optional_response_string(response, "status");
    let performed = response.get("performed").and_then(Value::as_bool);
    let verified = response.get("verified").and_then(Value::as_bool);
    let match_count = response.get("match_count").and_then(Value::as_u64);
    let returned_count = response.get("returned_count").and_then(Value::as_u64);
    let error_code = optional_response_string(response, "error_code");
    let message = optional_response_string(response, "message");
    let passed = match response_kind.as_deref() {
        Some("web-find") => {
            response_status.as_deref() == Some("complete") && match_count != Some(0)
        }
        Some("web-act") => {
            response_status.as_deref() == Some("complete")
                && performed == Some(true)
                && verified == Some(true)
        }
        _ => false,
    };

    GuiBenchLiveReplay {
        command: command.to_owned(),
        response_kind,
        response_schema,
        response_status,
        performed,
        verified,
        match_count,
        returned_count,
        error_code,
        message,
        passed,
    }
}

fn replay_actual_summary(replay: &GuiBenchLiveReplay) -> String {
    format!(
        "kind={:?},status={:?},performed={:?},verified={:?},match_count={:?},error_code={:?}",
        replay.response_kind,
        replay.response_status,
        replay.performed,
        replay.verified,
        replay.match_count,
        replay.error_code
    )
}

fn optional_response_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn command_payload<'a>(command: &'a str, kind: &str) -> io::Result<&'a str> {
    command
        .strip_prefix(kind)
        .and_then(|tail| tail.split_once(':').map(|(_, payload)| payload.trim()))
        .ok_or_else(|| invalid_data(format!("live replay command payload 无效: {command}")))
}

fn thresholds_from_fixture(fixture: &Value) -> io::Result<GuiBenchThresholds> {
    let dense_target = fixture
        .get("dense_target")
        .ok_or_else(|| invalid_data("fixture 缺少 dense_target"))?;

    Ok(GuiBenchThresholds {
        max_backend_request_count: required_u64(
            dense_target,
            "max_backend_request_count",
            "dense_target.max_backend_request_count",
        )?,
        max_agent_decision_points: required_u64(
            dense_target,
            "max_agent_decision_points",
            "dense_target.max_agent_decision_points",
        )?,
        required_scope: optional_string(dense_target, "required_scope"),
        required_action: optional_string(dense_target, "required_action"),
    })
}

fn metrics_from_variant(variant: &Value) -> io::Result<GuiBenchMetrics> {
    let metrics = variant
        .get("metrics")
        .ok_or_else(|| invalid_data("fixture variant 缺少 metrics"))?;

    Ok(GuiBenchMetrics {
        backend_request_count: required_u64(
            metrics,
            "backend_request_count",
            "metrics.backend_request_count",
        )?,
        control_frame_count: required_u64(
            metrics,
            "control_frame_count",
            "metrics.control_frame_count",
        )?,
        agent_decision_points: required_u64(
            metrics,
            "agent_decision_points",
            "metrics.agent_decision_points",
        )?,
        semantic_action_count: required_u64(
            metrics,
            "semantic_action_count",
            "metrics.semantic_action_count",
        )?,
        mouse_fallback_count: required_u64(
            metrics,
            "mouse_fallback_count",
            "metrics.mouse_fallback_count",
        )?,
        stale_ref_recovery_count: required_u64(
            metrics,
            "stale_ref_recovery_count",
            "metrics.stale_ref_recovery_count",
        )?,
        stale_visual_block_count: required_u64(
            metrics,
            "stale_visual_block_count",
            "metrics.stale_visual_block_count",
        )?,
        verification_passed: required_bool(
            metrics,
            "verification_passed",
            "metrics.verification_passed",
        )?,
        false_success_count: required_u64(
            metrics,
            "false_success_count",
            "metrics.false_success_count",
        )?,
        trace_step_count: required_u64(metrics, "trace_step_count", "metrics.trace_step_count")?,
    })
}

fn steps_from_variant(variant: &Value) -> io::Result<Vec<GuiBenchStep>> {
    let steps = variant
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_data("fixture variant 缺少 steps 数组"))?;

    steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            Ok(GuiBenchStep {
                command: required_string(step, "command", &format!("steps[{index}].command"))?,
            })
        })
        .collect()
}

fn select_variants<'a>(fixture: &'a Value, variant_name: &str) -> io::Result<Vec<&'a Value>> {
    let variants = fixture
        .get("variants")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_data("fixture 缺少 variants 数组"))?;

    if variant_name == ALL_VARIANTS {
        if variants.is_empty() {
            return Err(invalid_data("fixture variants 数组不能为空"));
        }
        return Ok(variants.iter().collect());
    }

    variants
        .iter()
        .find(|variant| variant.get("variant").and_then(Value::as_str) == Some(variant_name))
        .map(|variant| vec![variant])
        .ok_or_else(|| invalid_data(format!("fixture 未找到 variant `{variant_name}`")))
}

fn build_run(variant: &Value, thresholds: &GuiBenchThresholds) -> io::Result<GuiBenchRun> {
    let variant_name = required_string(variant, "variant", "variant.variant")?;
    let status = required_string(variant, "status", "variant.status")?;
    let metrics = metrics_from_variant(variant)?;
    let steps = steps_from_variant(variant)?;
    let steps_summary = summarize_steps(&steps);
    let checks = build_checks(thresholds, &metrics, &steps_summary);
    let threshold_failures = checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| check.name.to_owned())
        .collect::<Vec<_>>();
    let dense_target_passed = threshold_failures.is_empty();

    Ok(GuiBenchRun {
        variant: variant_name,
        status,
        dense_target_passed,
        threshold_failures,
        thresholds: thresholds.clone(),
        metrics,
        checks,
        steps_summary,
        live_replay: None,
    })
}

fn collect_response_failures(runs: &[GuiBenchRun]) -> Vec<String> {
    runs.iter()
        .flat_map(|run| {
            run.threshold_failures
                .iter()
                .map(|failure| format!("{}:{failure}", run.variant))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GuiBenchStep {
    command: String,
}

fn summarize_steps(steps: &[GuiBenchStep]) -> GuiBenchStepsSummary {
    let semantic_action_commands = steps
        .iter()
        .filter(|step| is_semantic_action_command(&step.command))
        .map(|step| step.command.clone())
        .collect::<Vec<_>>();
    let mouse_fallback_commands = steps
        .iter()
        .filter(|step| is_mouse_fallback_command(&step.command))
        .map(|step| step.command.clone())
        .collect::<Vec<_>>();

    GuiBenchStepsSummary {
        step_count: steps.len(),
        first_command: steps.first().map(|step| step.command.clone()),
        last_command: steps.last().map(|step| step.command.clone()),
        semantic_action_commands,
        mouse_fallback_commands,
    }
}

fn build_checks(
    thresholds: &GuiBenchThresholds,
    metrics: &GuiBenchMetrics,
    steps_summary: &GuiBenchStepsSummary,
) -> Vec<GuiBenchCheck> {
    let mut checks = vec![
        GuiBenchCheck {
            name: "backend_request_count",
            passed: metrics.backend_request_count <= thresholds.max_backend_request_count,
            expected: format!("<= {}", thresholds.max_backend_request_count),
            actual: metrics.backend_request_count.to_string(),
        },
        GuiBenchCheck {
            name: "agent_decision_points",
            passed: metrics.agent_decision_points <= thresholds.max_agent_decision_points,
            expected: format!("<= {}", thresholds.max_agent_decision_points),
            actual: metrics.agent_decision_points.to_string(),
        },
        GuiBenchCheck {
            name: "mouse_fallback_count",
            passed: metrics.mouse_fallback_count == 0
                && steps_summary.mouse_fallback_commands.is_empty(),
            expected: "0".to_owned(),
            actual: metrics.mouse_fallback_count.to_string(),
        },
        GuiBenchCheck {
            name: "false_success_count",
            passed: metrics.false_success_count == 0,
            expected: "0".to_owned(),
            actual: metrics.false_success_count.to_string(),
        },
        GuiBenchCheck {
            name: "verification_passed",
            passed: metrics.verification_passed,
            expected: "true".to_owned(),
            actual: metrics.verification_passed.to_string(),
        },
    ];

    if let Some(required_action) = &thresholds.required_action {
        checks.push(GuiBenchCheck {
            name: "required_action",
            passed: steps_summary
                .semantic_action_commands
                .iter()
                .any(|command| command_satisfies_required_action(command, required_action)),
            expected: required_action.clone(),
            actual: steps_summary.semantic_action_commands.join("\n"),
        });
    }

    checks
}

fn is_semantic_action_command(command: &str) -> bool {
    command.starts_with("@ax-action")
        || command.starts_with("@ax-press")
        || command.starts_with("@web-act")
        || command.starts_with("@gui-act")
}

fn command_satisfies_required_action(command: &str, required_action: &str) -> bool {
    command.contains(required_action)
        || (required_action == "AXPress"
            && command.starts_with("@web-act")
            && command.contains(r#"action:"press""#))
}

fn is_mouse_fallback_command(command: &str) -> bool {
    command.starts_with("@mouse-")
        || command.starts_with("@click")
        || command.starts_with("@drag")
        || command.starts_with("@wheel")
}

fn parse_non_empty_string(input: &str, field: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{field} 不能为空字符串")));
    }
    Ok(value)
}

fn parse_bool(input: &str, field: &str) -> io::Result<bool> {
    match input.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(invalid_data(format!(
            "{field} 必须是 true 或 false: {other}"
        ))),
    }
}

fn parse_runner(input: &str, field: &str) -> io::Result<GuiBenchRunner> {
    match parse_non_empty_string(input, field)?.as_str() {
        "fixture" => Ok(GuiBenchRunner::Fixture),
        "live" => Ok(GuiBenchRunner::Live),
        other => Err(invalid_data(format!(
            "{field} 必须是 \"fixture\" 或 \"live\": {other}"
        ))),
    }
}

fn required_field<T>(value: Option<T>, command: &str, field: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{command} 缺少必填字段 `{field}`")))
}

fn assign_once<T>(slot: &mut Option<T>, field: &str, command: &str, value: T) -> io::Result<()> {
    if slot.replace(value).is_some() {
        return Err(invalid_data(format!("{command} 重复设置字段 `{field}`")));
    }
    Ok(())
}

fn required_u64(value: &Value, key: &str, label: &str) -> io::Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_data(format!("fixture 字段 `{label}` 必须是无符号整数")))
}

fn required_bool(value: &Value, key: &str, label: &str) -> io::Result<bool> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_data(format!("fixture 字段 `{label}` 必须是布尔值")))
}

fn required_string(value: &Value, key: &str, label: &str) -> io::Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid_data(format!("fixture 字段 `{label}` 必须是字符串")))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn prepare_artifact(request: &GuiBenchRequest) -> io::Result<GuiBenchArtifact> {
    let dir = PathBuf::from("target").join("rdog-bench");
    fs::create_dir_all(&dir)?;
    let mut filename = format!(
        "{}__{}__{}",
        sanitize_filename_component(&request.suite),
        sanitize_filename_component(&request.case_name),
        sanitize_filename_component(&request.variant)
    );
    if request.runner != GuiBenchRunner::Fixture {
        filename.push_str("__");
        filename.push_str(&sanitize_filename_component(request.runner.as_str()));
    }
    filename.push_str(".json");
    let path = dir.join(filename);

    Ok(GuiBenchArtifact {
        path: path.display().to_string(),
        encoding: "utf-8-json",
    })
}

fn write_artifact(response: &GuiBenchResponse) -> io::Result<()> {
    let Some(artifact) = &response.artifact else {
        return Err(invalid_data("gui-bench artifact path 缺失"));
    };
    let json = serde_json::to_string_pretty(response)
        .map_err(|err| io::Error::other(format!("gui-bench artifact 序列化失败: {err}")))?;
    fs::write(&artifact.path, format!("{json}\n"))
}

fn sanitize_filename_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

fn trace_step(
    step: &'static str,
    status: &'static str,
    detail: Option<String>,
) -> GuiBenchTraceStep {
    GuiBenchTraceStep {
        step,
        status,
        detail,
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
