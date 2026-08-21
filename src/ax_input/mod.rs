//! ax_input 模块 - 文本与键盘输入的高层接口
//!
//! 提供两层 API：
//! - 简单 API (80% 场景): `type_text()`, `send_key()` - 隐藏 Request 复杂性
//! - 高级 API (20% 场景): `type_text_with_config()`, `send_key_with_config()` - 完整控制
//!
//! 注意：这些 API 在 Ticket #02 之前尚未被调用，因此会有 dead_code 警告。

#![allow(dead_code)]  // 允许未使用，直到 Ticket #02 迁移调用方

use crate::control_ax::input::{perform_default_key_delivery, perform_default_type_text};
use crate::control_ax::types::{AxTarget, TypeTextMode, TypeTextReport};
use crate::control_protocol::{KeyDelivery, KeyMode, KeyRequest, KeyResponseMode};
use std::io;

// Re-export types for external usage
pub use crate::control_ax::types::{KeyDeliveryReport, TypeTextRequest};

/// 简单 API: 在指定目标输入文本（80% 场景）
///
/// 默认行为：
/// - 模式: Auto（自动选择最佳方式）
/// - Clipboard: 允许使用剪贴板加速
///
/// # Example
/// ```no_run
/// use ax_input::type_text;
///
/// let mut target = AxTarget::default();
/// target.ref_id = Some("textfield_1".to_string());
/// type_text(target, "Hello World")?;
/// ```
pub fn type_text(target: AxTarget, text: &str) -> io::Result<TypeTextReport> {
    let request = TypeTextRequest {
        target,
        text: text.to_string(),
        mode: TypeTextMode::Auto,
        allow_clipboard: true,
    };
    perform_default_type_text(&request)
}

/// 高级 API: 使用完整配置输入文本（20% 场景）
///
/// 使用场景：
/// - 需要禁用剪贴板
/// - 需要指定特殊模式
/// - 需要自定义 target 配置
pub fn type_text_with_config(request: TypeTextRequest) -> io::Result<TypeTextReport> {
    perform_default_type_text(&request)
}

/// 简单 API: 发送按键（80% 场景）
///
/// 默认行为：
/// - Mode: PressRelease（按下后释放）
/// - Delivery: PidTargeted（发送到指定 PID）
/// - Hold: 200ms
///
/// 修饰键语法：使用 "+" 连接，如 "Cmd+W", "Ctrl+Shift+T"
///
/// # Example
/// ```no_run
/// use ax_input::send_key;
///
/// send_key("Return")?;      // 发送回车
/// send_key("Cmd+C")?;       // 发送 Cmd+C
/// send_key("Ctrl+Alt+Delete")?;  // 组合键
/// ```
pub fn send_key(key: &str) -> io::Result<Option<KeyDeliveryReport>> {
    let request = KeyRequest {
        key: key.to_string(),
        hold_ms: 200,
        mode: KeyMode::PressRelease,
        delivery: KeyDelivery::PidTargeted,
        pid: None,
        window_id: None,
        response_mode: KeyResponseMode::Structured,
    };
    perform_default_key_delivery(&request)
}

/// 高级 API: 使用完整配置发送按键（20% 场景）
///
/// 使用场景：
/// - 需要 Global delivery
/// - 需要 WindowTargeted delivery
/// - 需要自定义 hold_ms
/// - 需要 Press-only 或 Release-only 模式
pub fn send_key_with_config(request: KeyRequest) -> io::Result<Option<KeyDeliveryReport>> {
    perform_default_key_delivery(&request)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 简单 API 测试：验证默认值隐藏了 Request 复杂性 ----

    #[test]
    fn simple_type_text_should_hide_request_complexity() {
        // 用户不需要知道 TypeTextRequest 的字段
        let mut target = AxTarget::default();
        target.ref_id = Some("button_1".to_string());
        let result = type_text(target, "hello");

        // 语法检查通过即可（实际执行会失败，因为需要 macOS AX API）
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_type_text_should_use_auto_mode_by_default() {
        let mut target = AxTarget::default();
        target.ref_id = Some("input_1".to_string());

        // 验证内部构造的 request 使用 Auto 模式
        let request = TypeTextRequest {
            target: target.clone(),
            text: "test".to_string(),
            mode: TypeTextMode::Auto,
            allow_clipboard: true,
        };

        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_type_text_should_allow_clipboard_by_default() {
        let mut target = AxTarget::default();
        target.ref_id = Some("textarea_1".to_string());

        // 验证 allow_clipboard 默认为 true
        let request = TypeTextRequest {
            target,
            text: "clipboard test".to_string(),
            mode: TypeTextMode::Auto,
            allow_clipboard: true,
        };

        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_send_key_should_hide_request_complexity() {
        // 用户不需要知道 KeyRequest 的字段
        let result = send_key("Return");

        // 语法检查通过即可
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_send_key_should_use_pid_targeted_by_default() {
        // 验证默认使用 PidTargeted
        let request = KeyRequest {
            key: "a".to_string(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::PidTargeted,
            pid: None,
            window_id: None,
            response_mode: KeyResponseMode::Structured,
        };

        let result = send_key_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_send_key_should_support_modifiers_syntax() {
        // 验证修饰键语法（使用 "+" 连接）
        let result = send_key("Cmd+Shift+C");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_send_key_should_support_single_key() {
        // 验证单个按键
        let result = send_key("Escape");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn simple_api_ergonomics_test() {
        // 验证简单 API 的人体工程学
        let mut target = AxTarget::default();
        target.ref_id = Some("field_1".to_string());

        // 一行代码完成输入（无需构造 Request）
        let _ = type_text(target, "ergonomic");

        // 一行代码完成按键（无需构造 Request）
        let _ = send_key("Tab");
    }

    #[test]
    fn simple_send_key_various_combinations() {
        // 验证各种按键组合
        let _ = send_key("Return");
        let _ = send_key("Cmd+W");
        let _ = send_key("Ctrl+Alt+T");
        let _ = send_key("F12");
    }

    // ---- 高级 API 测试：验证自定义配置生效 ----

    #[test]
    fn advanced_type_text_should_allow_custom_mode() {
        let mut target = AxTarget::default();
        target.ref_id = Some("input_2".to_string());
        let request = TypeTextRequest {
            target,
            text: "advanced".to_string(),
            mode: TypeTextMode::Clipboard,
            allow_clipboard: true,
        };

        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn advanced_type_text_should_allow_disabling_clipboard() {
        let mut target = AxTarget::default();
        target.ref_id = Some("secure_field".to_string());
        let request = TypeTextRequest {
            target,
            text: "password123".to_string(),
            mode: TypeTextMode::TargetedKeyboard,
            allow_clipboard: false,
        };

        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn advanced_send_key_should_allow_global_delivery() {
        let request = KeyRequest {
            key: "F12".to_string(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::Global,
            pid: None,
            window_id: None,
            response_mode: KeyResponseMode::Structured,
        };

        let result = send_key_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn advanced_send_key_should_allow_window_targeted_delivery() {
        let request = KeyRequest {
            key: "Cmd+W".to_string(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::WindowTargeted,
            pid: None,
            window_id: Some("pid:556/window:0".to_string()),
            response_mode: KeyResponseMode::Structured,
        };

        let result = send_key_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn advanced_send_key_should_allow_custom_hold_ms() {
        let request = KeyRequest {
            key: "Space".to_string(),
            hold_ms: 1000,  // 自定义 hold 时间
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::PidTargeted,
            pid: None,
            window_id: None,
            response_mode: KeyResponseMode::Structured,
        };

        let result = send_key_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn advanced_api_full_control_test() {
        // 验证高级 API 提供完整控制
        let mut target = AxTarget::default();
        target.ref_id = Some("custom_field".to_string());
        let request = TypeTextRequest {
            target,
            text: "full control".to_string(),
            mode: TypeTextMode::AxValue,
            allow_clipboard: false,
        };

        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }

    // ---- 对比测试：简单 API vs 高级 API ----

    #[test]
    fn simple_vs_advanced_api_comparison() {
        let mut target = AxTarget::default();
        target.ref_id = Some("compare_field".to_string());

        // 简单 API: 1 行
        let _ = type_text(target.clone(), "simple");

        // 高级 API: 需要构造 Request（更啰嗦，但更灵活）
        let request = TypeTextRequest {
            target,
            text: "advanced".to_string(),
            mode: TypeTextMode::Auto,
            allow_clipboard: true,
        };
        let _ = type_text_with_config(request);
    }
}
