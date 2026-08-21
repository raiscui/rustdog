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
pub use execute::{focus, perform_action, scroll, set_value};

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
    // 专用 actions (Ticket #05)
    ActionRoute {
        name: "set_value",
        parser: parse_set_value_dynamic,
        executor: execute_set_value_dynamic,
    },
    ActionRoute {
        name: "focus",
        parser: parse_focus_dynamic,
        executor: execute_focus_dynamic,
    },
    ActionRoute {
        name: "scroll",
        parser: parse_scroll_dynamic,
        executor: execute_scroll_dynamic,
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

/// 为一个 action 生成一对 dynamic parser / executor。
///
/// routing 表统一用 `fn(&Value) -> io::Result<Value>` 签名, 所以每个 action
/// 都要一层 `Value` <-> 强类型的转换。这些转换逐字相同, 只有类型和函数名不同,
/// 用宏声明掉这层样板: 每个 action 一行, 而不是两个手写函数。
macro_rules! dynamic_route {
    ($parse_dyn:ident, $exec_dyn:ident, $req_ty:ty, $parse:path, $exec:path) => {
        fn $parse_dyn(payload: &Value) -> io::Result<Value> {
            let req = $parse(payload)?;
            serde_json::to_value(&req).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("无法序列化 {}: {e}", stringify!($req_ty)),
                )
            })
        }

        fn $exec_dyn(parsed: &Value) -> io::Result<Value> {
            let req: $req_ty = serde_json::from_value(parsed.clone()).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("无法反序列化 {}: {e}", stringify!($req_ty)),
                )
            })?;
            let report = $exec(&req)?;
            serde_json::to_value(&report).map_err(|e| {
                io::Error::other(format!(
                    "无法序列化 {} 的执行结果: {e}",
                    stringify!($req_ty)
                ))
            })
        }
    };
}

use crate::control_ax::types::{
    AxActionRequest, AxFocusRequest, AxPressRequest, AxScrollRequest, AxSetValueRequest,
};

dynamic_route!(
    parse_press_dynamic,
    execute_press_dynamic,
    AxPressRequest,
    protocol::parse_press,
    execute::press
);
dynamic_route!(
    parse_action_dynamic,
    execute_action_dynamic,
    AxActionRequest,
    protocol::parse_action,
    execute::perform_action
);
dynamic_route!(
    parse_set_value_dynamic,
    execute_set_value_dynamic,
    AxSetValueRequest,
    protocol::parse_set_value,
    execute::set_value
);
dynamic_route!(
    parse_focus_dynamic,
    execute_focus_dynamic,
    AxFocusRequest,
    protocol::parse_focus,
    execute::focus
);
dynamic_route!(
    parse_scroll_dynamic,
    execute_scroll_dynamic,
    AxScrollRequest,
    protocol::parse_scroll,
    execute::scroll
);

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
    /// routing 表覆盖检查: 每个已迁移 action 都必须能被查到。
    ///
    /// 新增 action 时只加 ACTION_ROUTES 一行数据, 这个测试会自动覆盖它,
    /// 前提是同时把名字加进下面的清单。
    #[test]
    fn test_routing_table_covers_all_migrated_actions() {
        let expected = [
            "press",
            "Press",
            "Open",
            "Confirm",
            "Cancel",
            "ShowMenu",
            "ScrollToVisible",
            "set_value",
            "focus",
            "scroll",
        ];

        for name in expected {
            assert!(
                ACTION_ROUTES.iter().any(|r| r.name == name),
                "routing 表缺少 action: {name}"
            );
        }
        assert_eq!(
            ACTION_ROUTES.len(),
            expected.len(),
            "routing 表条目数与预期清单不符, 新增 action 后请同步这个测试"
        );
    }

    /// 每个 action 名字只能出现一次, 否则后一条永远不会被命中。
    #[test]
    fn test_routing_table_has_no_duplicate_names() {
        for (i, route) in ACTION_ROUTES.iter().enumerate() {
            let dup = ACTION_ROUTES[..i].iter().any(|r| r.name == route.name);
            assert!(!dup, "routing 表存在重复 action 名: {}", route.name);
        }
    }

    /// 三个专用 action 的无效 payload 都应被 parser 拒绝, 不会走到平台调用。
    #[test]
    fn test_specialized_actions_reject_invalid_payload() {
        for name in ["set_value", "focus", "scroll"] {
            let result = execute_ax_action(name, &json!({"bogus": true}));
            assert!(result.is_err(), "{name} 对无效 payload 应返回错误");
        }
    }
}
