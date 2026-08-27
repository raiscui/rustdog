//! ax_input 模块 - 文本与键盘输入的高层接口
//!
//! 生产路径只有一套完整配置 API: `type_text_with_config()` / `send_key_with_config()`,
//! 由调用方 (control_actions) 从 protocol 层解析出完整 Request 后传入。

use crate::control_ax::types::TypeTextReport;
use crate::control_ax::AxBackend; // 新增：直接使用 AxBackend
use crate::control_protocol::{KeyDelivery, KeyRequest};
use std::io;

// Re-export types for external usage
pub use crate::control_ax::types::{KeyDeliveryReport, TypeTextRequest};

/// 输入文本: 调用方提供完整配置 (模式 / 剪贴板开关 / target)。
///
/// 使用场景：
/// - 需要禁用剪贴板
/// - 需要指定特殊模式
/// - 需要自定义 target 配置
pub fn type_text_with_config(request: TypeTextRequest) -> io::Result<TypeTextReport> {
    // 直接调用底层实现，避免循环依赖
    crate::control_ax::SystemAxBackend.type_text(&request)
}

/// 发送按键: 调用方提供完整配置 (delivery / hold / mode)。
///
/// 使用场景：
/// - 需要 Global delivery
/// - 需要 WindowTargeted delivery
/// - 需要自定义 hold_ms
/// - 需要 Press-only 或 Release-only 模式
pub fn send_key_with_config(request: KeyRequest) -> io::Result<Option<KeyDeliveryReport>> {
    // 直接调用底层实现，避免循环依赖
    match request.delivery {
        KeyDelivery::Global => Ok(None),
        KeyDelivery::PidTargeted | KeyDelivery::WindowTargeted => {
            crate::control_ax::platform_key_delivery(&request).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_protocol::{KeyMode, KeyResponseMode};

    /// 编译时验证: 完整配置 API 的函数签名 (Ticket #11 后是唯一入口,
    /// 简单 API 包装层已删除)。
    #[test]
    fn api_signature() {
        let _: fn(TypeTextRequest) -> io::Result<TypeTextReport> = type_text_with_config;
        let _: fn(KeyRequest) -> io::Result<Option<KeyDeliveryReport>> = send_key_with_config;
    }

    /// Global delivery 是纯逻辑分支 (不进平台投递), 可真实断言返回 None。
    /// Pid/WindowTargeted 依赖 macOS AX 环境, 不在单测范围。
    #[test]
    fn send_key_global_delivery_returns_none() {
        let request = KeyRequest {
            key: "F12".to_string(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::Global,
            pid: None,
            window_id: None,
            response_mode: KeyResponseMode::Structured,
        };
        assert_eq!(send_key_with_config(request).unwrap(), None);
    }
}
