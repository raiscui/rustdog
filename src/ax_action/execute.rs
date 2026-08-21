//! AX action execute 层：强类型函数，负责业务逻辑和平台调用。
//!
//! 每个函数接受强类型 request，返回强类型 report。
//! 业务逻辑校验在函数开头进行，不依赖外部预校验。

use std::io;

use crate::control_ax::types::{
    AxActionReport, AxActionRequest, AxFocusReport, AxFocusRequest, AxPerformedActionReport,
    AxPressPostconditionReport, AxPressRequest, AxPressSequenceReport, AxPressSequenceRequest,
    AxPressSequenceStepReport, AxScrollReport, AxScrollRequest, AxSetValueReport,
    AxSetValueRequest,
};
use crate::control_ax::{
    perform_default_ax_press as legacy_press,
    perform_default_ax_press_with_postcondition as legacy_press_with_postcondition,
};

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
#[allow(dead_code)] // Ticket #03 启用后使用
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
                    postcondition.max_attempts
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
        // 普通 press（复用现有实现）
        legacy_press(request)
    }
}

/// 执行带 postcondition 验证的 press action。
///
/// 内部使用，由 `press()` 调用。也可被需要完整 postcondition report 的调用方直接使用。
#[allow(dead_code)] // Ticket #03 启用后使用
pub fn press_with_postcondition(
    request: &AxPressRequest,
) -> io::Result<AxPressPostconditionReport> {
    // 复用现有实现
    legacy_press_with_postcondition(request)
}

/// 执行通用 AX action (Ticket #04)。
///
/// 支持的 action: Press / Open / Confirm / Cancel / ShowMenu / ScrollToVisible。
///
/// 这是 `control_ax::perform_default_ax_action` 的新家:
/// 直接调用平台 backend, 不再经过 control_ax 的 deprecated facade。
#[allow(dead_code)] // routing 表启用后使用
pub fn perform_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.perform_action(request)
}

/// 设置 AX 元素的值 (Ticket #05)。
///
/// 支持 Replace / Append 两种写入模式, 由 request.mode 决定。
#[allow(dead_code)] // routing 表启用后使用
pub fn set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.set_value(request)
}

/// 聚焦 AX 元素或窗口 (Ticket #05)。
///
/// 注意: 这里只做 AX 层聚焦。窗口激活 (activate) 由调用方
/// 在 control_actions 中先行处理, 不属于本函数职责。
#[allow(dead_code)] // routing 表启用后使用
pub fn focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    use crate::control_ax::{AxBackend, SystemAxBackend};

    SystemAxBackend.focus(request)
}

/// 滚动 AX 元素 (Ticket #05)。
#[allow(dead_code)] // routing 表启用后使用
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
    use crate::control_ax::invalid_data;

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
