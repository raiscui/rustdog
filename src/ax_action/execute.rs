//! AX action execute 层：强类型函数，负责业务逻辑和平台调用。
//!
//! 每个函数接受强类型 request，返回强类型 report。
//! 业务逻辑校验在函数开头进行，不依赖外部预校验。

use std::io;

use crate::control_ax::types::{
    AxActionReport, AxActionRequest, AxFocusReport, AxFocusRequest, AxPerformedActionReport,
    AxPressPostconditionReport, AxPressRequest, AxScrollReport, AxScrollRequest, AxSetValueReport,
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
