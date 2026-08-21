//! AX action 模块：统一的 action 执行接口。
//!
//! # 架构
//! - `protocol.rs`: JSON/compact → 强类型 struct (纯数据转换)
//! - `execute.rs`: 强类型函数，业务逻辑 + 平台调用
//! - `mod.rs`: Routing 表 + 统一入口
//!
//! # 双 API 设计
//! - 动态 API: `execute_ax_action(action, payload)` - RPC 边界使用
//! - 强类型 API: `press(request)` - 内部调用使用

mod execute;
mod protocol;

use serde_json::Value;
use std::io;

// Re-export 强类型 API
// pub use execute::{press, press_with_postcondition}; // 暂未使用
pub use execute::perform_action;

/// Action routing 表的单个 entry。
#[allow(dead_code)] // Ticket #03 启用后使用
struct ActionRoute {
    name: &'static str,
    parser: fn(&Value) -> io::Result<Value>,
    executor: fn(&Value) -> io::Result<Value>,
}

/// 统一的 action routing 表。
///
/// 每个 entry 包含：
/// - name: action 名称（字符串）
/// - parser: payload → parsed value
/// - executor: parsed value → result
///
/// 设计为 `const` 数据结构，零运行时开销。
#[allow(dead_code)] // Ticket #03 启用后使用
const ACTION_ROUTES: &[ActionRoute] = &[
    ActionRoute {
        name: "press",
        parser: parse_press_dynamic,
        executor: execute_press_dynamic,
    },
    // 通用 actions (Ticket #04)
    ActionRoute {
        name: "Press",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
    ActionRoute {
        name: "Open",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
    ActionRoute {
        name: "Confirm",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
    ActionRoute {
        name: "Cancel",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
    ActionRoute {
        name: "ShowMenu",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
    ActionRoute {
        name: "ScrollToVisible",
        parser: parse_action_dynamic,
        executor: execute_action_dynamic,
    },
];

/// 动态 API：根据 action 名称执行对应的 handler。
///
/// # 参数
/// - `action`: action 名称（如 "press"）
/// - `payload`: JSON payload
///
/// # 返回
/// - `Ok(result)`: 执行成功，返回 JSON 结果
/// - `Err(ActionNotFound)`: 未知的 action 名称
/// - `Err(InvalidData)`: payload 解析失败
/// - `Err(Other)`: 执行失败
///
/// # 示例
/// ```ignore
/// let payload = json!({"target": {"id": "..."}});
/// let result = execute_ax_action("press", &payload)?;
/// ```
#[allow(dead_code)] // Ticket #03 启用后使用
pub fn execute_ax_action(action: &str, payload: &Value) -> io::Result<Value> {
    // 在 routing 表中查找 action
    let route = ACTION_ROUTES
        .iter()
        .find(|r| r.name == action)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("未知的 action: {}", action),
            )
        })?;

    // 解析 payload
    let parsed = (route.parser)(payload)?;

    // 执行 action
    (route.executor)(&parsed)
}

/// press action 的动态 parser wrapper。
fn parse_press_dynamic(payload: &Value) -> io::Result<Value> {
    let req = protocol::parse_press(payload)?;
    // 将 AxPressRequest 序列化为 Value（内部传递格式）
    serde_json::to_value(&req).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无法序列化 AxPressRequest: {}", e),
        )
    })
}

/// press action 的动态 executor wrapper。
fn execute_press_dynamic(parsed: &Value) -> io::Result<Value> {
    // 从 Value 反序列化为 AxPressRequest
    let req: crate::control_ax::types::AxPressRequest = serde_json::from_value(parsed.clone())
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("无法反序列化 AxPressRequest: {}", e),
            )
        })?;

    // 调用强类型函数
    let report = execute::press(&req)?;

    // 序列化结果
    serde_json::to_value(&report).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("无法序列化 AxActionReport: {}", e),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_routing_table_finds_press() {
        let route = ACTION_ROUTES.iter().find(|r| r.name == "press");
        assert!(route.is_some(), "routing 表应该包含 press action");
    }

    #[test]
    fn test_execute_ax_action_unknown() {
        let payload = json!({});
        let result = execute_ax_action("unknown_action", &payload);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_execute_ax_action_press_invalid_payload() {
        let payload = json!({"target": {}}); // 缺少必填字段

        let result = execute_ax_action("press", &payload);
        assert!(result.is_err(), "无效 payload 应该返回错误");
    }
}

/// 通用 action 的动态 parser wrapper (Ticket #04)。
fn parse_action_dynamic(payload: &Value) -> io::Result<Value> {
    let req = protocol::parse_action(payload)?;
    serde_json::to_value(&req).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无法序列化 AxActionRequest: {}", e),
        )
    })
}

/// 通用 action 的动态 executor wrapper (Ticket #04)。
fn execute_action_dynamic(parsed: &Value) -> io::Result<Value> {
    let req: crate::control_ax::types::AxActionRequest = serde_json::from_value(parsed.clone())
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("无法反序列化 AxActionRequest: {}", e),
            )
        })?;

    let report = execute::perform_action(&req)?;

    serde_json::to_value(&report).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("无法序列化 AxPerformedActionReport: {}", e),
        )
    })
}
