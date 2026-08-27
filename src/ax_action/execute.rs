//! AX action execute 层：强类型函数，负责业务逻辑和平台调用。
//!
//! 每个函数接受强类型 request，返回强类型 report。
//! 业务逻辑校验在函数开头进行，不依赖外部预校验。

use std::io;

use crate::control_ax::types::{
    AxActionReport, AxActionName, AxActionRequest, AxElement, AxFocusReport, AxFocusRequest,
    AxPerformedActionReport, AxPressPostcondition, AxPressPostconditionReport,
    AxPressPostconditionStepReport, AxPressRequest, AxPressSequenceReport,
    AxPressSequenceRequest, AxPressSequenceStepReport, AxSnapshot, AxScrollReport,
    AxScrollRequest, AxSetValueReport, AxSetValueRequest, AxTreeRequest,
    AX_POSTCONDITION_DEPTH, AX_POSTCONDITION_MAX_ELEMENTS,
};
use crate::control_ax::tree::{ax_snapshot_status_error, materialize_app_window_target_with};
use crate::control_ax::{capture_current_ax_window_snapshot, invalid_data, invalid_input};
use crate::control_window::resolve_unique_app_window_id;

/// 执行 AX press action。
///
/// # 参数
/// - `request`: 包含 target 和可选的 postcondition
///
/// # 返回
/// - `Ok(report)`: 成功执行（可能包含 postcondition 验证结果）
/// - `Err(e)`: 执行失败
///
/// # 逻辑
/// - 如果 request 包含 postcondition，调用 press_with_postcondition 逻辑
/// - 否则调用普通 press 逻辑
pub fn press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    if let Some(ref postcondition) = request.postcondition {
        // 调用 postcondition 版本，提取出 base report
        let postcondition_report = press_with_postcondition(request)?;

        // 如果 postcondition 未验证成功，返回错误
        if !postcondition_report.verified {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Postcondition 验证失败: role={}, expected={}, attempts={}/{}",
                    postcondition.role,
                    postcondition.expected_value,
                    postcondition_report.attempt_count,
                    postcondition_report.max_attempts
                ),
            ));
        }

        // 将 AxPressPostconditionReport 转换为 AxActionReport
        Ok(AxActionReport {
            kind: postcondition_report.kind,
            action: postcondition_report.action.to_string(),
            backend: "ax".to_string(),
            target_id: None,
            description: None,
            hint: None,
            performed: postcondition_report.performed,
            status: postcondition_report.status,
        })
    } else {
        // 普通 press
        press_plain(request)
    }
}

/// 执行带 postcondition 验证的 press action。
///
/// 内部使用，由 `press()` 调用。也可被需要完整 postcondition report 的调用方直接使用。
pub fn press_with_postcondition(
    request: &AxPressRequest,
) -> io::Result<AxPressPostconditionReport> {
    perform_ax_press_with_postcondition_with(
        request,
        resolve_unique_app_window_id,
        press_plain,
        |window_id, role| {
            observe_current_ax_values_with(window_id, role, capture_current_ax_window_snapshot)
        },
    )
}

/// 无 postcondition 的 press 底层实现 (自 control_ax 迁入)。
///
/// 直接调用平台 backend 执行 AXPress, 并为清除类按钮附加通用 continue hint。
fn press_plain(request: &AxPressRequest) -> io::Result<AxActionReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    let report = SystemAxBackend.perform_action(&AxActionRequest {
        target: request.target.clone(),
        action: AxActionName::Press,
    })?;
    let mut report = AxActionReport::press(
        report.backend,
        report.target_id,
        request.target.description.clone(),
    );
    // 2026-08-04 (清除类 hint): 清除类按钮被按下后, 给 agent 一句通用引导,
    // 防止"清除子目标完成 -> 流程断裂" (模型清除后迷失, 不继续剩余输入)。
    // 文案不含任何任务知识, 任何"清除后要继续"的场景都成立。
    report.hint = clear_action_hint(request.target.description.as_deref());
    Ok(report)
}

/// 清除类操作的 continue hint (纯函数, 便于单测)。
fn clear_action_hint(description: Option<&str>) -> Option<String> {
    description
        .filter(|desc| is_clear_action_description(desc))
        .map(|_| CLEAR_ACTION_HINT.to_string())
}

/// 清除类操作完成后的通用 continue 引导 (共享给 @ax-press 与 @key)。
pub(crate) const CLEAR_ACTION_HINT: &str =
    "clear completed; the task is not finished until the remaining input steps and the final confirm action are done";

/// 判断按钮描述是否为"清除类"操作 (删除 / 全部清除 / Clear / AC 等)。
///
/// 清除类操作通常是任务的中间步骤 (清掉旧状态), 完成清除后 agent 容易
/// 把子目标当成任务终点。这里只做通用语义匹配, 不含任何具体任务知识。
fn is_clear_action_description(description: &str) -> bool {
    let normalized = description.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "删除" | "全部清除" | "清除" | "清空" | "clear" | "all clear" | "ac" | "delete" | "del"
    )
}

/// guarded press 的核心逻辑 (自 control_ax 迁入, 依赖全部注入, 可测试)。
fn perform_ax_press_with_postcondition_with(
    request: &AxPressRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
    mut perform: impl FnMut(&AxPressRequest) -> io::Result<AxActionReport>,
    mut observe: impl FnMut(&str, &str) -> io::Result<Vec<String>>,
) -> io::Result<AxPressPostconditionReport> {
    let postcondition = request
        .postcondition
        .as_ref()
        .ok_or_else(|| invalid_data("AX guarded press 缺少 postcondition"))?;
    let target = materialize_app_window_target_with(&request.target, resolve_app)?;
    let window_id = target
        .window_id
        .as_deref()
        .ok_or_else(|| invalid_data("AX guarded press 必须使用 app:APP 或 pid:PID/window:INDEX"))?;
    let expected_value = normalize_ax_verification_value(&postcondition.expected_value);
    let mut steps = Vec::with_capacity(postcondition.max_attempts);

    for index in 0..postcondition.max_attempts {
        let action = match perform(&AxPressRequest {
            target: target.clone(),
            postcondition: None,
        }) {
            Ok(action) => action,
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressPostconditionStepReport {
                    index,
                    performed: false,
                    verified: false,
                    target_id: None,
                    observed_values: Vec::new(),
                    error: Some(error.clone()),
                });
                return Ok(build_ax_press_postcondition_report(
                    postcondition,
                    steps,
                    false,
                    Some(error),
                ));
            }
        };

        let observed_values = match observe(window_id, &postcondition.role) {
            Ok(values) => values,
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressPostconditionStepReport {
                    index,
                    performed: action.performed,
                    verified: false,
                    target_id: action.target_id,
                    observed_values: Vec::new(),
                    error: Some(error.clone()),
                });
                return Ok(build_ax_press_postcondition_report(
                    postcondition,
                    steps,
                    false,
                    Some(error),
                ));
            }
        };
        let verified = observed_values
            .iter()
            .any(|value| normalize_ax_verification_value(value) == expected_value);
        steps.push(AxPressPostconditionStepReport {
            index,
            performed: action.performed,
            verified,
            target_id: action.target_id,
            observed_values,
            error: None,
        });
        if verified {
            return Ok(build_ax_press_postcondition_report(
                postcondition,
                steps,
                true,
                None,
            ));
        }
    }

    let error = format!(
        "AX postcondition 未在{}次动作内满足: role={}, expected_value={}",
        postcondition.max_attempts, postcondition.role, postcondition.expected_value
    );
    Ok(build_ax_press_postcondition_report(
        postcondition,
        steps,
        false,
        Some(error),
    ))
}

/// 组装 guarded press 的最终报告。
fn build_ax_press_postcondition_report(
    postcondition: &AxPressPostcondition,
    steps: Vec<AxPressPostconditionStepReport>,
    verified: bool,
    error: Option<String>,
) -> AxPressPostconditionReport {
    AxPressPostconditionReport {
        kind: "ax-press",
        action: "press-until",
        performed: steps.iter().any(|step| step.performed),
        verified,
        status: if verified { "ok" } else { "failed" },
        role: postcondition.role.clone(),
        expected_value: postcondition.expected_value.clone(),
        attempt_count: steps.len(),
        max_attempts: postcondition.max_attempts,
        steps,
        error,
    }
}

/// 对目标窗口做一次通用 fresh 观察, 收集指定 role 的全部 value。
fn observe_current_ax_values_with(
    window_id: &str,
    role: &str,
    capture: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<Vec<String>> {
    // 通用 fresh 观察必须能深入 Calculator 等多层 AX 树拿到 AXStaticText 的 value。
    // 默认 AxTreeRequest 只到 depth=4,实测不足以覆盖 Calculator 结果节点。
    // 这里使用与 compact ax-find 一致的深度与上限,与 parser 侧契约对齐。
    let request = AxTreeRequest {
        depth: AX_POSTCONDITION_DEPTH,
        max_elements: AX_POSTCONDITION_MAX_ELEMENTS,
        include_values: true,
        ..AxTreeRequest::default()
    };
    let snapshot = capture(window_id, &request)?;
    if snapshot.capture_status != "complete" {
        return Err(ax_snapshot_status_error(&snapshot));
    }
    if snapshot.truncated {
        return Err(invalid_input(
            "AX guarded press fresh snapshot 被截断,无法证明postcondition",
        ));
    }

    let mut values = Vec::new();
    for window in &snapshot.windows {
        collect_ax_values_by_role(&window.elements, role, &mut values);
    }
    values.sort();
    values.dedup();
    Ok(values)
}

/// 递归收集指定 role 的元素 value (跳过被脱敏的元素)。
fn collect_ax_values_by_role(elements: &[AxElement], role: &str, values: &mut Vec<String>) {
    for element in elements {
        if element.role == role && !element.value_redacted {
            if let Some(value) = element.value.as_deref() {
                values.push(normalize_ax_verification_value(value));
            }
        }
        collect_ax_values_by_role(&element.children, role, values);
    }
}

/// 剥离 bidi 控制字符后 trim, 用于 postcondition 期望值与观察值的可比对。
fn normalize_ax_verification_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// 执行通用 AX action (Ticket #04)。
///
/// 支持的 action: Press / Open / Confirm / Cancel / ShowMenu / ScrollToVisible。
///
/// 这是 `control_ax::perform_default_ax_action` 的新家:
/// 直接调用平台 backend, 不再经过 control_ax 的 deprecated facade。
pub fn perform_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.perform_action(request)
}

/// 设置 AX 元素的值 (Ticket #05)。
///
/// 支持 Replace / Append 两种写入模式, 由 request.mode 决定。
pub fn set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.set_value(request)
}

/// 聚焦 AX 元素或窗口 (Ticket #05)。
///
/// 注意: 这里只做 AX 层聚焦。窗口激活 (activate) 由调用方
/// 在 control_actions 中先行处理, 不属于本函数职责。
pub fn focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.focus(request)
}

/// 滚动 AX 元素 (Ticket #05)。
pub fn scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.scroll(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ax::types::AxTarget;

    #[test]
    fn test_press_without_postcondition() {
        // 注意：这是单元测试框架，实际执行会失败（因为没有真实 AX 环境）
        // 这里只测试函数签名和错误处理路径
        let request = AxPressRequest {
            target: AxTarget {
                id: Some("test-id".to_owned()),
                ..AxTarget::default()
            },
            postcondition: None,
        };

        // 预期会失败（没有真实环境），但能测试代码路径
        let result = press(&request);
        assert!(result.is_err(), "没有真实 AX 环境应该失败");
    }

    #[test]
    fn test_press_api_signature() {
        // 编译时验证：确保函数签名正确
        let _: fn(&AxPressRequest) -> io::Result<AxActionReport> = press;
        let _: fn(&AxPressRequest) -> io::Result<AxPressPostconditionReport> =
            press_with_postcondition;
    }
}

/// 执行 AX press 序列（原子操作）。
///
/// # 原子性
/// 序列中任一步骤失败，立即停止并返回失败报告。这是与单次 press 的关键区别。
///
/// # 窗口归属固化
/// 若第一个 target 使用 `app` selector，会在执行前解析一次并把所有步骤
/// 固化为同一个 `window_id`，避免执行中途漂移到另一窗口。
///
/// # 参数
/// - `resolve_app`: 将 app selector 解析为 window_id 的函数（如 `control_ax::resolve_unique_app_window_id`）
pub fn press_sequence(
    request: &AxPressSequenceRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
) -> AxPressSequenceReport {
    let request = match materialize_press_sequence_request(request, resolve_app) {
        Ok(request) => request,
        Err(error) => {
            let steps = request.targets.first().map_or_else(Vec::new, |target| {
                vec![AxPressSequenceStepReport::failed(
                    0,
                    target.description.clone().unwrap_or_default(),
                    error.to_string(),
                )]
            });
            return AxPressSequenceReport {
                kind: "ax-press-sequence",
                action: "press-sequence",
                performed: false,
                status: "failed",
                step_count: request.targets.len(),
                steps,
                failed_index: Some(0),
                error: Some(error.to_string()),
            };
        }
    };
    perform_press_sequence_with(&request, press)
}

/// 固化 press sequence 的窗口归属。
///
/// 若使用 app selector，在第一个 side effect 前解析一次并把所有步骤
/// 固化为同一个 window_id，避免执行中途漂移到另一窗口。
fn materialize_press_sequence_request(
    request: &AxPressSequenceRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
) -> io::Result<AxPressSequenceRequest> {
    let first = request
        .targets
        .first()
        .ok_or_else(|| invalid_data("@ax-press-sequence 至少需要一个 target"))?;
    let Some(app) = first.app.as_deref() else {
        for target in &request.targets {
            target.validate()?;
        }
        return Ok(request.clone());
    };

    let window_id = resolve_app(app)?;
    let mut targets = Vec::with_capacity(request.targets.len());
    for target in &request.targets {
        if target.app.as_deref() != Some(app) {
            return Err(invalid_data(
                "@ax-press-sequence 的所有 target 必须使用同一个 app selector",
            ));
        }
        let mut target = target.clone();
        target.app = None;
        target.window_id = Some(window_id.clone());
        target.validate()?;
        targets.push(target);
    }
    Ok(AxPressSequenceRequest { targets })
}

/// 执行 press sequence 的核心逻辑（可测试）。
///
/// 接受一个 perform 函数，遍历 targets 逐个执行。任一步骤失败立即停止。
fn perform_press_sequence_with(
    request: &AxPressSequenceRequest,
    mut perform: impl FnMut(&AxPressRequest) -> io::Result<AxActionReport>,
) -> AxPressSequenceReport {
    let mut steps = Vec::with_capacity(request.targets.len());
    for (index, target) in request.targets.iter().enumerate() {
        let description = target.description.clone().unwrap_or_default();
        match perform(&AxPressRequest {
            target: target.clone(),
            postcondition: None,
        }) {
            Ok(report) => {
                steps.push(AxPressSequenceStepReport::success(
                    index,
                    description,
                    report,
                ));
            }
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressSequenceStepReport::failed(
                    index,
                    description,
                    error.clone(),
                ));
                return AxPressSequenceReport {
                    kind: "ax-press-sequence",
                    action: "press-sequence",
                    performed: false,
                    status: "failed",
                    step_count: request.targets.len(),
                    steps,
                    failed_index: Some(index),
                    error: Some(error),
                };
            }
        }
    }

    AxPressSequenceReport {
        kind: "ax-press-sequence",
        action: "press-sequence",
        performed: true,
        status: "ok",
        step_count: request.targets.len(),
        steps,
        failed_index: None,
        error: None,
    }
}

#[cfg(test)]
mod press_sequence_tests {
    use super::*;

    /// 从 control_ax 搬迁的原子性回归: app selector 只解析一次,
    /// 且中途失败必须立即停止并保留已完成步骤。
    #[test]
    fn press_sequence_should_resolve_app_once_and_preserve_partial_failure() {
        use crate::control_ax::{parse_ax_press_sequence_payload, AxActionReport};
        use std::cell::Cell;

        let request = parse_ax_press_sequence_payload("app:Calculator,1,加,2").unwrap();
        let resolve_calls = Cell::new(0usize);
        let request = materialize_press_sequence_request(&request, |app| {
            resolve_calls.set(resolve_calls.get() + 1);
            assert_eq!(app, "Calculator");
            Ok("pid:123/window:0".to_owned())
        })
        .unwrap();

        assert_eq!(resolve_calls.get(), 1, "app selector 只能解析一次");
        assert!(request.targets.iter().all(|target| {
            target.app.is_none() && target.window_id.as_deref() == Some("pid:123/window:0")
        }));

        let mut call_index = 0usize;
        let report = perform_press_sequence_with(&request, |_| {
            let index = call_index;
            call_index += 1;
            if index == 1 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "目标歧义"));
            }
            Ok(AxActionReport::press(
                "test",
                Some(format!("pid:123/window:0/path:{index}")),
                None,
            ))
        });

        assert!(!report.performed);
        assert_eq!(report.status, "failed");
        assert_eq!(report.step_count, 3);
        assert_eq!(report.steps.len(), 2, "失败后不再执行剩余步骤");
        assert!(report.steps[0].performed);
        assert!(!report.steps[1].performed);
        assert_eq!(report.steps[1].description, "加");
        assert_eq!(report.steps[1].error.as_deref(), Some("目标歧义"));
        assert_eq!(report.failed_index, Some(1));
        assert_eq!(report.error.as_deref(), Some("目标歧义"));
        assert_eq!(call_index, 2, "第 3 步不应被调用");
    }

    /// 全部成功时 status 为 ok, 且 failed_index 为空。
    #[test]
    fn press_sequence_should_report_ok_when_all_steps_succeed() {
        use crate::control_ax::{parse_ax_press_sequence_payload, AxActionReport};

        let request = parse_ax_press_sequence_payload("app:Demo,1,2").unwrap();
        let request =
            materialize_press_sequence_request(&request, |_| Ok("pid:9/window:0".to_owned()))
                .unwrap();

        let report = perform_press_sequence_with(&request, |_| {
            Ok(AxActionReport::press("test", None, None))
        });

        assert!(report.performed);
        assert_eq!(report.status, "ok");
        assert_eq!(report.step_count, 2);
        assert_eq!(report.steps.len(), 2);
        assert!(report.failed_index.is_none());
        assert!(report.error.is_none());
    }

    /// 混用不同 app selector 必须被拒绝, 避免执行中途漂移窗口。
    #[test]
    fn press_sequence_should_reject_mixed_app_selectors() {
        use crate::control_ax::types::AxTarget;

        let request = AxPressSequenceRequest {
            targets: vec![
                AxTarget {
                    app: Some("A".to_owned()),
                    description: Some("x".to_owned()),
                    ..AxTarget::default()
                },
                AxTarget {
                    app: Some("B".to_owned()),
                    description: Some("y".to_owned()),
                    ..AxTarget::default()
                },
            ],
        };

        let err = materialize_press_sequence_request(&request, |_| Ok("pid:1/window:0".to_owned()))
            .unwrap_err();
        assert!(err.to_string().contains("同一个 app selector"));
    }

    /// 空 target 列表在 materialize 阶段就应失败。
    #[test]
    fn press_sequence_should_reject_empty_targets() {
        let request = AxPressSequenceRequest {
            targets: Vec::new(),
        };
        let report = press_sequence(&request, |_| Ok("pid:1/window:0".to_owned()));

        assert!(!report.performed);
        assert_eq!(report.status, "failed");
        assert_eq!(report.step_count, 0);
        assert!(report.steps.is_empty(), "没有 target 时不产出步骤报告");
    }
}

/// 自 control_ax 迁入的 press 实现层测试。
///
/// 覆盖: 清除类 hint / guarded press 重试与 fail-closed / bidi 值比对 / 深层 fresh 观察。
#[cfg(test)]
mod press_tests {
    use super::*;
    use crate::control_ax::parse_ax_press_payload;
    use crate::control_ax::types::AxWindow;
    use std::cell::Cell;

    #[test]
    fn clear_action_press_should_include_continue_hint() {
        // 清除类按钮被按下后, 响应带通用 hint, 防止"清除后流程断裂"。
        let hint = clear_action_hint(Some("删除")).expect("clear must hint");
        assert!(
            hint.contains("not finished") && hint.contains("final confirm"),
            "hint must guide continue: {hint}"
        );

        // 非清除按钮不加 hint。
        assert!(clear_action_hint(Some("1")).is_none());
        assert!(clear_action_hint(Some("加")).is_none());
        assert!(clear_action_hint(None).is_none());

        // 英文清除语义同样命中。
        for desc in ["clear", "all clear", "AC", "delete", "全部清除", "清空"] {
            assert!(
                clear_action_hint(Some(desc)).is_some(),
                "{desc} must be treated as clear action"
            );
        }
    }

    #[test]
    fn guarded_ax_press_should_stop_when_fresh_postcondition_matches() {
        let request = parse_ax_press_payload("app:Demo,重置,AXStaticText,ready,3").unwrap();
        let resolve_calls = Cell::new(0usize);
        let press_calls = Cell::new(0usize);
        let observe_calls = Cell::new(0usize);

        let report = perform_ax_press_with_postcondition_with(
            &request,
            |app| {
                resolve_calls.set(resolve_calls.get() + 1);
                assert_eq!(app, "Demo");
                Ok("pid:321/window:0".to_owned())
            },
            |_| {
                press_calls.set(press_calls.get() + 1);
                Ok(AxActionReport::press(
                    "test",
                    Some(format!("pid:321/window:0/path:{}", press_calls.get())),
                    None,
                ))
            },
            |window_id, role| {
                observe_calls.set(observe_calls.get() + 1);
                assert_eq!(window_id, "pid:321/window:0");
                assert_eq!(role, "AXStaticText");
                Ok(if observe_calls.get() == 1 {
                    vec!["pending".to_owned()]
                } else {
                    vec!["ready".to_owned()]
                })
            },
        )
        .unwrap();

        assert_eq!(resolve_calls.get(), 1);
        assert_eq!(press_calls.get(), 2);
        assert_eq!(observe_calls.get(), 2);
        assert!(report.performed);
        assert!(report.verified);
        assert_eq!(report.status, "ok");
        assert_eq!(report.attempt_count, 2);
        assert_eq!(report.steps.len(), 2);
        assert!(report.steps.iter().all(|step| step.performed));
        assert!(!report.steps[0].verified);
        assert!(report.steps[1].verified);
    }

    #[test]
    fn guarded_ax_press_should_fail_closed_at_attempt_limit() {
        let request = parse_ax_press_payload("pid:321/window:0,重置,AXStaticText,ready,3").unwrap();
        let press_calls = Cell::new(0usize);

        let report = perform_ax_press_with_postcondition_with(
            &request,
            |_| panic!("window_id target不应解析app"),
            |_| {
                press_calls.set(press_calls.get() + 1);
                Ok(AxActionReport::press(
                    "test",
                    Some("pid:321/window:0/path:1".to_owned()),
                    None,
                ))
            },
            |_, _| Ok(vec!["pending".to_owned()]),
        )
        .unwrap();

        assert_eq!(press_calls.get(), 3);
        assert!(report.performed);
        assert!(!report.verified);
        assert_eq!(report.status, "failed");
        assert_eq!(report.attempt_count, 3);
        assert_eq!(report.steps.len(), 3);
        assert!(report
            .error
            .as_deref()
            .is_some_and(|error| { error.contains("未在3次动作内满足") }));
    }

    #[test]
    fn ax_postcondition_comparison_should_remove_bidi_controls() {
        assert_eq!(normalize_ax_verification_value("\u{200e}0\u{200f}"), "0");
        assert_eq!(
            normalize_ax_verification_value("\u{2066}ready\u{2069}"),
            "ready"
        );
    }

    #[test]
    fn observe_current_ax_values_should_reach_deeply_nested_static_text() {
        // Calculator 等应用的 AXStaticText result value 常位于 depth >= 5。
        // 这里手工构造一个 depth=6 的 snapshot,验证通用 fresh 观察能取到。
        fn leaf(role: &str, value: &str) -> AxElement {
            AxElement {
                id: format!("id-{role}"),
                ref_id: None,
                role: role.to_owned(),
                subrole: None,
                name: None,
                value: Some(value.to_owned()),
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: Vec::new(),
                children: Vec::new(),
            }
        }

        let mut nested = leaf("AXStaticText", "0");
        for index in 1..=6 {
            nested = AxElement {
                id: format!("id-group-{index}"),
                ref_id: None,
                role: "AXGroup".to_owned(),
                subrole: None,
                name: None,
                value: None,
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: Vec::new(),
                children: vec![nested],
            };
        }
        let window = AxWindow {
            id: "pid:7/window:0".to_owned(),
            ref_id: None,
            pid: 7,
            process_name: "Calculator".to_owned(),
            title: Some("Calculator".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![nested],
        };
        let snapshot = AxSnapshot::complete("test", vec![window], false);

        let captured_request_depth = Cell::new(0u8);
        let values = observe_current_ax_values_with("pid:7/window:0", "AXStaticText", |_, req| {
            captured_request_depth.set(req.depth);
            Ok(snapshot.clone())
        })
        .unwrap();

        assert_eq!(captured_request_depth.get(), AX_POSTCONDITION_DEPTH);
        assert!(captured_request_depth.get() >= 6);
        assert_eq!(values, vec!["0".to_owned()]);
    }
}
