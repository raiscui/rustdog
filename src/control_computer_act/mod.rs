//! `@computer-act` meta-command 的 dispatcher + 13 动作 routing 表。
//!
//! 设计目标 (ADR-0001 ~ 0006, ticket 04):
//! - 接受 Mano-CUA 16 动作中的 13 个 daemon-side 闭集
//! - 把每个 action 翻译成底层 `ControlCommand` (Click / Drag / Wheel / ...)
//! - 调度到底层 primitive (Phase C ticket 06-10 完整覆盖, ticket 04 仅 skeleton)
//! - unknown action 返回 `error_code:"unknown_action"`
//!
//! ticket 04 范围:
//! - routing 表覆盖全部 13 action
//! - 响应 envelope 包含所有后续 ticket 字段 (`observation_id` / `verification` /
//!   `observation_used` / `density` / `trace_summary` / `trace_savefile`), 现为 `null`
//! - 默认 timeout 表调用 lookup function (ticket 16 填充具体值)
//!
//! 不在 ticket 04 范围:
//! - implicit_observe (ticket 11)
//! - verify 三档 (ticket 12-14)
//! - 错误 envelope E2 (ticket 15)
//! - timeout 表具体值 (ticket 16)
//! - density / trace 字段填充 (ticket 17-18)

use std::io;

use serde_json::Value;

use crate::control_actions::ActionExecutionResult;
use crate::control_mouse::MouseEndpoint;
use crate::control_mouse::MouseRefTarget;
use crate::cancellation::CancellationToken;
use crate::control_protocol::{
    ComputerActRequest, ControlCommand, KeyMode, KeyRequest, OpenAppRequest,
    WaitRequest,
};

// ticket 11 implicit_observe plumbing (TTL 5s, ADR-0005 L3)
#[path = "implicit_observe.rs"]
mod implicit_observe;
pub(crate) use implicit_observe::{
    render_observation_used, render_top_level_observation_id,
    resolve_or_re_observe_with_wall_clock,
};

// ticket 12 + ticket 13 verify 三档 (ADR-0004 V3)
#[path = "verify.rs"]
mod verify;
pub(crate) use verify::{
    parse_verify_policy, render_verification, run_always_verify,
    run_best_effort_verify, VerifyPolicy,
};

// ticket 17 density metrics (ADR-0006 §Consequences)
#[path = "density.rs"]
mod density;
pub(crate) use density::{compute_verification_passed, render_density, ComputerActDensity};

// ticket 18 trace_summary inline + trace_savefile opt-in (ADR-0006 §Consequences)
#[path = "trace.rs"]
mod trace;
pub(crate) use trace::{
    render_trace_summary, write_trace_savefile, FullTrace, FullTraceDispatch,
    FullTraceImplicitObserve, SubStep, TraceStatus, TraceSummary,
};

// ticket 15 error envelope E2 (ADR-0004 §Considered Options E2)
#[path = "error_envelope.rs"]
pub(crate) mod error_envelope;
pub(crate) use error_envelope::{
    error_envelope, ComputerActErrorCode, RetryStrategy,
};

// ticket 16 per-action timeout table (ADR-0005 §3)
#[path = "timeout.rs"]
mod timeout;
pub(crate) use timeout::{
    resolve_timeout, TimeoutWatcher,
};

/// `control_computer_act` 把 action + args 翻译成的中间结果。
///
/// `dispatched_to` 是底层 primitive 的人类可读标签 (`@click` / `@key` 等),
/// `command` 是要 dispatch 的 `ControlCommand`。
pub(crate) struct RoutedCommand {
    pub dispatched_to: &'static str,
    pub command: ControlCommand,
}

/// 路由层错误。Execute 阶段包成 E2 envelope, ticket 15 完善。
#[derive(Debug)]
pub(crate) enum ComputerActRouteError {
    UnknownAction(String),
    InvalidArgs(String),
}

/// 13 动作 routing 表入口。
///
/// 调用方负责把 `args` 字段 (serde_json::Value) 喂进来;内部按 action 名分发。
pub(crate) fn route_computer_act_action(
    action: &str,
    args: &Value,
) -> Result<RoutedCommand, ComputerActRouteError> {
    match action {
        "open_app" => Ok(RoutedCommand {
            dispatched_to: "@open-app",
            command: route_open_app(args)?,
        }),
        "open_url" => Ok(RoutedCommand {
            dispatched_to: "@cmd",
            command: route_open_url(args)?,
        }),
        "click" => route_click(args, 1, "left"),
        "doubleclick" => route_click(args, 2, "left"),
        "triple_click" => route_click(args, 3, "left"),
        "right_single" => route_click(args, 1, "right"),
        "hover" => Ok(RoutedCommand {
            dispatched_to: "@mouse-move",
            command: route_hover(args)?,
        }),
        "type" => Ok(RoutedCommand {
            dispatched_to: "@type-text",
            command: route_type(args)?,
        }),
        "hotkey" => Ok(RoutedCommand {
            dispatched_to: "@key",
            command: route_hotkey(args)?,
        }),
        "hotkey_click" => Ok(RoutedCommand {
            dispatched_to: "@key+@click+@key",
            command: route_hotkey_click(args)?,
        }),
        "scroll" => Ok(RoutedCommand {
            dispatched_to: "@wheel",
            command: route_scroll(args)?,
        }),
        "drag" => Ok(RoutedCommand {
            dispatched_to: "@drag",
            command: route_drag(args)?,
        }),
        "wait" => Ok(RoutedCommand {
            dispatched_to: "@wait",
            command: route_wait(args)?,
        }),
        other => Err(ComputerActRouteError::UnknownAction(other.to_string())),
    }
}

/// 默认 timeout 表 (ms)。ticket 16 替换为 per-action 派生公式。
fn default_timeout_ms_for_action(_action: &str) -> u64 {
    30000
}

/// 解析 start_box: 期望 `[x, y]` 数组 (Mano-CUA normalized [0, 1000])。
/// 后续 ticket 11 把它转换为底层 primitive 的 os-logical 像素坐标。
fn parse_start_box(args: &Value) -> Result<(u16, u16), ComputerActRouteError> {
    let start_box = args.get("start_box").and_then(|v| v.as_array()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("missing start_box [x, y]".to_string())
    })?;
    if start_box.len() != 2 {
        return Err(ComputerActRouteError::InvalidArgs(format!(
            "start_box 必须是 [x, y],实际长度 {}",
            start_box.len()
        )));
    }
    let x = start_box[0].as_u64().ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("start_box[0] 必须是整数".to_string())
    })? as u16;
    let y = start_box[1].as_u64().ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("start_box[1] 必须是整数".to_string())
    })? as u16;
    Ok((x, y))
}

/// 解析 ref 目标: `{ref:"@e1", observation_id:"obs-..."}`。
/// ticket 11 会做完整 implicit_observe 联动;04 只做结构识别。
fn parse_ref_target(args: &Value) -> Result<MouseEndpoint, ComputerActRouteError> {
    let target = args.get("target").and_then(|v| v.as_object()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("missing target {ref, observation_id}".to_string())
    })?;
    let ref_id = target.get("ref").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("target.ref 必须是字符串".to_string())
    })?.to_string();
    let observation_id = target.get("observation_id").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("target.observation_id 必须是字符串".to_string())
    })?.to_string();
    Ok(MouseEndpoint::ObservationRef(MouseRefTarget {
        ref_id,
        observation_id,
        anchor: crate::control_mouse::MouseAnchor::Center,
    }))
}

/// 解析 target: 优先 `target.ref` (ref-based),否则 `start_box` (coord-based)。
fn parse_endpoint(args: &Value) -> Result<MouseEndpoint, ComputerActRouteError> {
    if args.get("target").is_some() {
        return parse_ref_target(args);
    }
    let (x, y) = parse_start_box(args)?;
    // ticket 11 之前,start_box → pixel 转换是 1:1 占位 (后续 ticket 改 1000→pixel)
    Ok(MouseEndpoint::Coordinate(crate::control_mouse::MousePoint {
        x: x as i32,
        y: y as i32,
    }))
}

fn route_open_app(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let app_name = args.get("app_name").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("open_app 缺少 app_name".to_string())
    })?.to_string();
    let wait_ms = args.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(1500);
    Ok(ControlCommand::OpenApp(OpenAppRequest { app_name, wait_ms }))
}

fn route_open_url(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    // open_url 折叠为 `@cmd "open <url>"` (macOS),后续 LP1 跟进 Linux/Windows。
    // 这条路由只生成 command 字符串,实际 Script 执行在 dispatcher 阶段。
    let url = args.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("open_url 缺少 url".to_string())
    })?.to_string();
    Ok(ControlCommand::Script(format!("open {url}")))
}

fn route_click(
    args: &Value,
    count: u8,
    button: &str,
) -> Result<RoutedCommand, ComputerActRouteError> {
    let endpoint = parse_endpoint(args)?;
    let button_name = match button {
        "left" => crate::control_mouse::MouseButtonName::Left,
        "right" => crate::control_mouse::MouseButtonName::Right,
        other => {
            return Err(ComputerActRouteError::InvalidArgs(format!(
                "click 未知 button: {other}"
            )))
        }
    };
    let click_req = crate::control_mouse::ClickRequest {
        x: None,
        y: None,
        target: Some(endpoint),
        guard: None,
        button: button_name,
        count,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: crate::control_mouse::MouseCoordinateSpace::OsLogical,
    };
    Ok(RoutedCommand {
        dispatched_to: "@click",
        command: ControlCommand::Click(click_req),
    })
}

fn route_hover(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let endpoint = parse_endpoint(args)?;
    let (x, y) = match &endpoint {
        MouseEndpoint::Coordinate(p) => (Some(p.x), Some(p.y)),
        _ => (None, None),
    };
    Ok(ControlCommand::MouseMove(crate::control_mouse::MouseMoveRequest {
        x,
        y,
        dx: None,
        dy: None,
        target: Some(endpoint),
        guard: None,
        coordinate_space: crate::control_mouse::MouseCoordinateSpace::OsLogical,
    }))
}

fn route_type(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let content = args.get("content").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("type 缺少 content".to_string())
    })?.to_string();
    // ticket 04 skeleton: 总是走 @paste (无 target),后续 ticket 07 加 ref→@type-text 分流。
    Ok(ControlCommand::Paste(crate::control_protocol::PasteRequest::legacy_text(content)))
}

fn route_hotkey(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let key = args.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("hotkey 缺少 key".to_string())
    })?.to_string();
    Ok(ControlCommand::Key(crate::control_protocol::KeyRequest::legacy(
        key,
        200,
        crate::control_protocol::KeyMode::PressRelease,
    )))
}

fn route_hotkey_click(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    // hotkey_click 是组合动作: 按下 modifier, click target, 释放 modifier。
    // ticket 08 + 21 实现: 3 个 sub-command 串成 Composite, dispatch_underlying
    // 顺序执行, 任一失败回滚 (release modifier)。
    let key = args.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("hotkey_click 缺少 key".to_string())
    })?.to_string();
    let (x, y) = parse_start_box(args)?;

    let key_down = ControlCommand::Key(KeyRequest::legacy(&key, 200, KeyMode::Press));
    let click = ControlCommand::Click(crate::control_mouse::ClickRequest {
        x: Some(x as i32),
        y: Some(y as i32),
        target: None,
        guard: None,
        button: crate::control_mouse::MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: crate::control_mouse::MouseCoordinateSpace::OsLogical,
    });
    let key_up = ControlCommand::Key(KeyRequest::legacy(&key, 200, KeyMode::Release));
    Ok(ControlCommand::Composite(vec![key_down, click, key_up]))
}

fn route_scroll(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let (x, y) = parse_start_box(args)?;
    let direction = args.get("direction").and_then(|v| v.as_str()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("scroll 缺少 direction (down/up/left/right)".to_string())
    })?;
    let amount = args.get("amount").and_then(|v| v.as_u64()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("scroll 缺少 amount".to_string())
    })? as i32;
    // positive amount = down (delta_y < 0 表示向下滚动手势); 简化映射,后续 ticket 09 校准。
    let (delta_x, delta_y) = match direction {
        "down" => (0, -amount),
        "up" => (0, amount),
        "left" => (amount, 0),
        "right" => (-amount, 0),
        other => {
            return Err(ComputerActRouteError::InvalidArgs(format!(
                "scroll 未知 direction: {other}"
            )))
        }
    };
    Ok(ControlCommand::Wheel(crate::control_mouse::WheelRequest {
        x: Some(x as i32),
        y: Some(y as i32),
        target: None,
        guard: None,
        delta_x,
        delta_y,
        coordinate_space: crate::control_mouse::MouseCoordinateSpace::OsLogical,
    }))
}

fn route_drag(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let (x1, y1) = parse_start_box(args)?;
    let end_box = args.get("end_box").and_then(|v| v.as_array()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("drag 缺少 end_box [x, y]".to_string())
    })?;
    if end_box.len() != 2 {
        return Err(ComputerActRouteError::InvalidArgs(format!(
            "end_box 必须是 [x, y],实际长度 {}",
            end_box.len()
        )));
    }
    let x2 = end_box[0].as_u64().ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("end_box[0] 必须是整数".to_string())
    })? as i32;
    let y2 = end_box[1].as_u64().ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("end_box[1] 必须是整数".to_string())
    })? as i32;
    let from = MouseEndpoint::Coordinate(crate::control_mouse::MousePoint { x: x1 as i32, y: y1 as i32 });
    let to = MouseEndpoint::Coordinate(crate::control_mouse::MousePoint { x: x2, y: y2 });
    Ok(ControlCommand::Drag(crate::control_mouse::DragRequest {
        from,
        to,
        guard: None,
        button: crate::control_mouse::MouseButtonName::Left,
        duration_ms: 450,
        steps: 24,
        coordinate_space: crate::control_mouse::MouseCoordinateSpace::OsLogical,
    }))
}

fn route_wait(args: &Value) -> Result<ControlCommand, ComputerActRouteError> {
    let duration_ms = args.get("duration_ms").and_then(|v| v.as_u64()).ok_or_else(|| {
        ComputerActRouteError::InvalidArgs("wait 缺少 duration_ms".to_string())
    })?;
    Ok(ControlCommand::Wait(WaitRequest { duration_ms }))
}

/// `execute_computer_act` 是 `@computer-act` 的 executor。
///
/// 流程 (skeleton 范围):
/// 1. routing 阶段: `action` + `args` → underlying `ControlCommand` (13 动作闭集)
/// 2. dispatch 阶段: 调底层 primitive 的 `execute_*` 函数
/// 3. response 阶段: 包成 `rdog.computer-act.v1` envelope, 包含 6 个后续 ticket
///    字段占位 (`null`)
pub(crate) fn execute_computer_act(
    request: &ComputerActRequest,
    cancel: Option<&CancellationToken>,
) -> io::Result<ActionExecutionResult> {
    use std::time::Instant;
    use serde_json::json;

    let start = Instant::now();
    let _ = default_timeout_ms_for_action(&request.action); // ticket 16 替换

    // ticket 11 implicit_observe: 在 routing 之前解析 args.target / start_box,
    // 校验 observation_id TTL,过期自动 re-observe,outcome 写到 response 顶层。
    // ticket 11 阶段不动 args 结构 (real observe 接入后才替换 start_box → target.ref)。
    let implicit_observe_start = Instant::now();
    let implicit_outcome = resolve_or_re_observe_with_wall_clock(&request.args);
    let implicit_observe_ms = implicit_observe_start.elapsed().as_millis() as u64;

    // ticket 12/13: parse verify policy (None 时不写 verification 字段,best_effort 时跑 AX diff)。
    let verify_policy = match parse_verify_policy(request.verify.as_deref()) {
        Ok(p) => p,
        Err(err) => {
            let mut evidence = serde_json::Map::new();
            evidence.insert(
                "verify".into(),
                request.verify.clone().map(Value::String).unwrap_or(Value::Null),
            );
            return Ok(ActionExecutionResult {
                exit_code: 64,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: Some(error_envelope(
                    ComputerActErrorCode::InvalidVerify,
                    err.to_string(),
                    Some(Value::Object(evidence)),
                ).to_string()),
            });
        }
    };

    let routed = match route_computer_act_action(&request.action, &request.args) {
        Ok(r) => r,
        Err(ComputerActRouteError::UnknownAction(action)) => {
            return Ok(ActionExecutionResult {
                exit_code: 64,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: Some(json!({
                    "ok": false,
                    "action": action,
                    "error_code": "unknown_action",
                    "error_message": format!("unknown @computer-act action: {action}"),
                    "evidence": { "action": action },
                    "duration_ms": start.elapsed().as_millis() as u64,
                }).to_string()),
            });
        }
        Err(ComputerActRouteError::InvalidArgs(msg)) => {
            let mut evidence = serde_json::Map::new();
            evidence.insert("action".into(), Value::String(request.action.clone()));
            evidence.insert("args".into(), request.args.clone());
            return Ok(ActionExecutionResult {
                exit_code: 64,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: Some(error_envelope(
                    ComputerActErrorCode::InvalidArgs,
                    msg,
                    Some(Value::Object(evidence)),
                ).to_string()),
            });
        }
    };

    // ticket 16: timeout watcher (spawn background thread, 命中后 signal cancel_token)。
    // 跟 ticket 03 cancellation 整合: dispatch_underlying 拿 cancel, 命中后由
    // 底层 primitive 决定怎么处理 (e.g., @wait sleep_cancellable 返回 Err)。
    let effective_timeout_ms = resolve_timeout(
        &request.action,
        &request.args,
        request.timeout_ms,
    );
    let timeout_token = CancellationToken::new();
    let _timeout_watcher = TimeoutWatcher::start(effective_timeout_ms, timeout_token.clone());
    let effective_cancel: Option<&CancellationToken> = Some(&timeout_token);

    // 调度到底层 primitive (ticket 13: 拆出 dispatch_ms,verify 用)
    let dispatch_start = Instant::now();
    let underlying_result = dispatch_underlying(routed.command, effective_cancel)?;
    let dispatch_ms = dispatch_start.elapsed().as_millis() as u64;
    let duration_ms = start.elapsed().as_millis() as u64;

    // ticket 16: timeout 检查。如果 timeout_token fired 且 dispatch 仍 ok → 算 timeout。
    // 注意: 即使 dispatch 出错, 也可能是因为 cancel 触发了底层 primitive 早退;
    // 这种情况下 exit_code != 0, 底层 primitive 已经返回错误, 我们也归类为 timeout。
    let timeout_fired = timeout_token.is_cancelled();
    if timeout_fired {
        let mut evidence = serde_json::Map::new();
        evidence.insert(
            "last_step".into(),
            Value::String("dispatch".to_string()),
        );
        evidence.insert(
            "timeout_ms".into(),
            Value::Number(effective_timeout_ms.into()),
        );
        evidence.insert(
            "elapsed_ms".into(),
            Value::Number(dispatch_ms.into()),
        );
        return Ok(ActionExecutionResult {
            exit_code: 64,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(error_envelope(
                ComputerActErrorCode::Timeout,
                format!(
                    "action {} exceeded timeout ({}ms after dispatch)",
                    request.action, effective_timeout_ms
                ),
                Some(Value::Object(evidence)),
            ).to_string()),
        });
    }

    // ticket 13/14: verify 三档分别跑不同 verify 流程
    // - BestEffort: AX diff only (轻量)
    // - Always: full observe (screenshot + AX + windows + AX diff)
    // - None: 不跑 verify
    let verify_summary = match verify_policy {
        VerifyPolicy::BestEffort => Some(run_best_effort_verify(dispatch_ms)),
        _ => None,
    };
    let always_summary = match verify_policy {
        VerifyPolicy::Always => Some(run_always_verify(dispatch_ms)),
        _ => None,
    };
    let verify_ms = match verify_policy {
        VerifyPolicy::BestEffort => verify_summary.as_ref().map(|s| s.verify_ms),
        VerifyPolicy::Always => Some(always_summary.as_ref().map(|s| s.ax_diff.verify_ms).unwrap_or(0)),
        VerifyPolicy::None => None,
    };

    // 包成 computer-act envelope
    let underlying_json_str = underlying_result
        .response_value_json
        .clone()
        .unwrap_or_else(|| "{}".to_string());
    let underlying_value: serde_json::Value =
        serde_json::from_str(&underlying_json_str).unwrap_or_else(|_| json!({}));

    let ok = underlying_result.exit_code == 0;

    // ticket 17: 构造 ComputerActDensity (含 verification_passed) - 必须在 json! macro 之前
    let verification_passed = compute_verification_passed(
        verify_policy,
        verify_summary.as_ref().or_else(|| {
            always_summary.as_ref().map(|s| &s.ax_diff)
        }),
    );

    // ticket 18: 构造 inline trace_summary (4 段耗时)
    let trace_summary = TraceSummary::build(
        implicit_observe_ms,
        if implicit_observe_ms > 0 { TraceStatus::Ok } else { TraceStatus::Skipped },
        0, // ref_resolve: ticket 18 阶段测量 (start_box → ref 解析); 暂时占 0
        dispatch_ms,
        ok,
        verify_ms,
        !matches!(verify_policy, VerifyPolicy::None),
    );
    let trace_summary_json = render_trace_summary(&trace_summary);

    let density_metrics = ComputerActDensity::new(
        dispatch_ms,
        implicit_observe_ms,
        matches!(
            implicit_outcome,
            crate::control_computer_act::implicit_observe::ImplicitObserveOutcome::Fresh { .. }
                | crate::control_computer_act::implicit_observe::ImplicitObserveOutcome::StaleReObserved { .. }
        ),
        verify_ms,
        verification_passed,
        trace_summary.step_count(),
    );

    let mut payload = json!({
        "ok": ok,
        "action": request.action,
        "dispatched_to": routed.dispatched_to,
        "duration_ms": duration_ms,
        // ticket 11 填充 observation_id / observation_used;
        // ticket 12/13/14 填充 verification;
        // ticket 17 填充 density;
        // ticket 18 填充 trace_summary
        "observation_id": render_top_level_observation_id(&implicit_outcome)
            .map(Value::String)
            .unwrap_or(Value::Null),
        "observation_used": render_observation_used(&implicit_outcome)
            .unwrap_or(Value::Null),
        "density": render_density(&density_metrics),
        "trace_summary": trace_summary_json,
    });

    // ticket 12/13/14: verify=none 时不写 verification 字段;best_effort 写 ax_diff 摘要;
    // always 走 AlwaysVerifySummary 路径 (full observe + ax_diff)。
    if let Some(v) = render_verification(
        verify_policy,
        verify_summary.as_ref(),
        always_summary.as_ref(),
    ) {
        payload["verification"] = v;
    }

    // ticket 18: trace_summary 总是带 (即使 verify=none 也占 4 段);trace_savefile
    // 仅在 request.trace == Some("savefile") 时存在
    // ticket 18: trace_savefile 仅在 request.trace == Some("savefile") 时存在
    if request.trace.as_deref() == Some("savefile") {
        // opt-in 落盘: 走 rdog_downloads/trace-{ts}-{id}.json
        let full_trace = FullTrace {
            implicit_observe: FullTraceImplicitObserve {
                elapsed_ms: implicit_observe_ms,
                sub_steps: vec![
                    SubStep::ok("ax_tree_scan", implicit_observe_ms),
                    SubStep::skipped("screenshot_capture"), // ticket 11 阶段不抓 screenshot
                    SubStep::skipped("ref_resolution"),     // ticket 11 阶段不解析 ref
                ],
            },
            dispatch: FullTraceDispatch {
                elapsed_ms: dispatch_ms,
                dispatched_to: routed.dispatched_to.to_string(),
                ok,
                sub_steps: vec![
                    SubStep::ok("route_action", 0),
                    SubStep::ok("dispatch_underlying", dispatch_ms),
                ],
            },
            verify: verify_summary.as_ref().map(|s| s.full_report.clone()),
            verification_passed,
        };
        match write_trace_savefile(None, &full_trace) {
            Ok(path) => {
                payload["trace_savefile"] = Value::String(path);
            }
            Err(_) => {
                // 写盘失败不污染 dispatch ok:true (跟 implicit_observe / verify 失败
                // 同口径,observability 错误透明降级)
            }
        }
    }
    if !ok {
        // 底层错误透传 — ticket 15 把 error_code / retry 包装到 E2 envelope。
        if let Some(err_code) = underlying_value.get("error_code") {
            payload["error_code"] = err_code.clone();
        }
        if let Some(err_msg) = underlying_value.get("error_message") {
            payload["error_message"] = err_msg.clone();
        }
        if let Some(evidence) = underlying_value.get("evidence") {
            payload["evidence"] = evidence.clone();
        }
    } else if let Some(inner_dispatched) = underlying_value.get("dispatched_to") {
        // 嵌套 dispatched_to (e.g., @type-text 内部用 @paste) 暴露给客户端
        payload["inner_dispatched_to"] = inner_dispatched.clone();
    }

    Ok(ActionExecutionResult {
        exit_code: if ok { 0 } else { underlying_result.exit_code },
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(payload.to_string()),
    })
}

/// 调度到底层 primitive 的 executor 函数 (skeleton: 直接调已知 execute_* 函数)。
///
/// 后续 ticket (Phase C/D/E) 会有更复杂的调度 (e.g. multi-step, cancellation
/// propagation, verify),ticket 04 是 minimal skeleton。
fn dispatch_underlying(
    command: ControlCommand,
    cancel: Option<&CancellationToken>,
) -> io::Result<ActionExecutionResult> {
    use crate::control_actions::{
        execute_cancel, execute_key, execute_open_app, execute_paste,
        execute_script, execute_type_text, execute_wait,
    };
    use crate::control_mouse::prepare_click_request;
    use crate::control_mouse::prepare_drag_request;
    use crate::control_mouse::prepare_mouse_move_request;
    use crate::control_mouse::prepare_wheel_request;

    match command {
        ControlCommand::Click(req) => {
            crate::control_actions::execute_prepared_mouse_request(
                prepare_click_request(&req)?,
                crate::control_mouse::build_click_plan,
            )
        }
        ControlCommand::Drag(req) => {
            crate::control_actions::execute_prepared_mouse_request(
                prepare_drag_request(&req)?,
                crate::control_mouse::build_drag_plan,
            )
        }
        ControlCommand::Wheel(req) => {
            crate::control_actions::execute_prepared_mouse_request(
                prepare_wheel_request(&req)?,
                crate::control_mouse::build_wheel_plan,
            )
        }
        ControlCommand::MouseMove(req) => crate::control_actions::execute_prepared_mouse_request(
            prepare_mouse_move_request(&req)?,
            crate::control_mouse::build_mouse_move_plan,
        ),
        ControlCommand::Key(req) => execute_key(&req, None),
        ControlCommand::Paste(req) => execute_paste(&req),
        ControlCommand::TypeText(req) => execute_type_text(&req),
        ControlCommand::Wait(req) => execute_wait(&req, cancel),
        ControlCommand::OpenApp(req) => execute_open_app(&req),
        ControlCommand::Script(text) => {
            // `open_url` 路由生成 `@cmd "open <url>"` 形式, 走 shell。
            execute_script("/bin/sh", &text)
        }
        ControlCommand::Cancel(req) => {
            // computer-act 内不允许 cancel 自身 (语义上无意义), 但 routing 可能
            // 错误地到达这里。ticket 15 完善。
            execute_cancel(&req, &crate::cancellation::CancelRegistry::new())
        }
        ControlCommand::Composite(cmds) => {
            // ticket 08 + 21: composite 顺序执行 (e.g., hotkey_click =
            // key down + click + key up)。任一失败: 已经执行的 key down 要
            // 释放 (modifier release), 然后返回错误。
            let mut executed: Vec<&ControlCommand> = Vec::new();
            for cmd in cmds.iter() {
                let result = dispatch_underlying(cmd.clone(), cancel)?;
                executed.push(cmd);
                if result.exit_code != 0 {
                    // 回滚: 对所有已执行的 key down 发 key up
                    for done_cmd in &executed {
                        if let ControlCommand::Key(kr) = done_cmd {
                            if matches!(kr.mode, KeyMode::Press) {
                                let release = ControlCommand::Key(KeyRequest::legacy(
                                    &kr.key, 200, KeyMode::Release,
                                ));
                                let _ = dispatch_underlying(release, cancel);
                            }
                        }
                    }
                    return Ok(result); // 返回第一个失败的 result
                }
            }
            // 全部成功: 返回最后一条的 result (保留 exit_code 等)
            if let Some(last) = cmds.last() {
                dispatch_underlying(last.clone(), cancel)
            } else {
                // 空 Composite (理论不应到达, defensive)
                Ok(ActionExecutionResult {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    response_value_json: Some("{}".to_string()),
                })
            }
        }
        // 不应到达的分支 (routing 应该只生成上面 9 类)
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("computer-act 路由到了未支持的底层命令: {other:?}"),
        )),
    }
}

#[cfg(test)]
mod tests;
