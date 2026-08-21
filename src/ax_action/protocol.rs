//! AX action protocol 层：负责 JSON/compact 格式到强类型 struct 的反序列化。
//!
//! 本文件只做数据转换，不含业务逻辑校验（由 execute.rs 负责）。

use serde_json::Value;
use std::io;

use crate::control_ax::parse_ax_press_payload;
use crate::control_ax::types::{
    AxActionRequest, AxFocusRequest, AxPressPostcondition, AxPressRequest, AxScrollRequest,
    AxSetValueRequest, AxTarget,
};

/// 解析 press action 的 payload。
///
/// 支持两种格式：
/// 1. JSON: `{"target": {"id": "..."}, "postcondition": {...}}`
/// 2. Compact: `app:APP,description:删除`
///
/// # 向后兼容
/// 旧格式（不含 postcondition 字段）仍能正确解析，postcondition 为 None。
#[allow(dead_code)] // Ticket #03 启用后使用
pub fn parse_press(payload: &Value) -> io::Result<AxPressRequest> {
    if let Some(s) = payload.as_str() {
        // Compact format: "app:APP,description:删除"
        parse_ax_press_payload(s)
    } else if let Some(obj) = payload.as_object() {
        // JSON format: {"target": {...}, "postcondition": {...}}
        let target_value = obj
            .get("target")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "缺少 target 字段"))?;

        // 解析 target
        let target: AxTarget = serde_json::from_value(target_value.clone()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("无法解析 target: {}", e),
            )
        })?;

        // 解析 postcondition（可选）
        let postcondition = if let Some(pc_value) = obj.get("postcondition") {
            let pc: AxPressPostcondition =
                serde_json::from_value(pc_value.clone()).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("无法解析 postcondition: {}", e),
                    )
                })?;
            Some(pc)
        } else {
            None
        };

        Ok(AxPressRequest {
            target,
            postcondition,
        })
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload 必须是字符串或对象",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_press_json() {
        let payload = json!({
            "target": {"id": "pid:1/window:0/path:0"}
        });

        let req = parse_press(&payload).unwrap();
        assert_eq!(req.target.id.as_deref(), Some("pid:1/window:0/path:0"));
        assert!(req.postcondition.is_none());
    }

    #[test]
    fn test_parse_press_with_postcondition() {
        let payload = json!({
            "target": {"id": "pid:1/window:0/path:0"},
            "postcondition": {
                "role": "AXStaticText",
                "expected_value": "0",
                "max_attempts": 3
            }
        });

        let req = parse_press(&payload).unwrap();
        assert_eq!(req.target.id.as_deref(), Some("pid:1/window:0/path:0"));
        assert!(req.postcondition.is_some());

        let pc = req.postcondition.unwrap();
        assert_eq!(pc.role, "AXStaticText");
        assert_eq!(pc.expected_value, "0");
        assert_eq!(pc.max_attempts, 3);
    }

    #[test]
    fn test_parse_press_compact_format() {
        let payload = json!("app:Calculator,description:等于");

        let req = parse_press(&payload).unwrap();
        assert_eq!(req.target.app.as_deref(), Some("Calculator"));
        assert_eq!(req.target.description.as_deref(), Some("等于"));
    }

    /// 向后兼容性测试：旧格式（不含 postcondition）仍能解析
    #[test]
    fn test_parse_press_backward_compatible() {
        let old_payload = json!({
            "target": {"ref": "@e2", "observation_id": "obs-1"}
        });

        let req = parse_press(&old_payload).unwrap();
        eprintln!("DEBUG: parsed target = {:?}", req.target);
        assert_eq!(req.target.ref_id.as_deref(), Some("@e2"));
        assert_eq!(req.target.observation_id.as_deref(), Some("obs-1"));
        assert!(req.postcondition.is_none(), "旧格式不含 postcondition 字段");
    }
}

/// 解析通用 action 的 payload。
///
/// 支持两种格式：
/// 1. JSON Value 对象: `{"target": {"id": "..."}, "action": "Press"}` (serde)
/// 2. rdog 对象字面量字符串: `{target:{app:"X",description:"等于"},action:"Press"}`
///
/// # 与 press 的差异
/// `@ax-action` 从来不支持 `app:X,description:Y` 这种裸 compact 写法,
/// 它的 line-control 语法本身就要求对象字面量。这里保持同一契约,
/// 不额外发明新格式。
#[allow(dead_code)] // routing 表启用后使用
pub fn parse_action(payload: &Value) -> io::Result<AxActionRequest> {
    if let Some(s) = payload.as_str() {
        // Compact format: "app:APP,description:删除,action:Press"
        crate::control_ax::parse_ax_action_payload(s)
    } else {
        // JSON format
        serde_json::from_value(payload.clone()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("无效的 action payload: {}", e),
            )
        })
    }
}

/// 解析 set-value payload (Ticket #05)。
///
/// 字符串走 line-control 对象字面量, JSON Value 走 serde。
#[allow(dead_code)] // routing 表启用后使用
pub fn parse_set_value(payload: &Value) -> io::Result<AxSetValueRequest> {
    if let Some(s) = payload.as_str() {
        crate::control_ax::parse_ax_set_value_payload(s)
    } else {
        serde_json::from_value(payload.clone()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("无效的 set-value payload: {e}"),
            )
        })
    }
}

/// 解析 focus payload (Ticket #05)。
#[allow(dead_code)] // routing 表启用后使用
pub fn parse_focus(payload: &Value) -> io::Result<AxFocusRequest> {
    if let Some(s) = payload.as_str() {
        crate::control_ax::parse_ax_focus_payload(s)
    } else {
        serde_json::from_value(payload.clone()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("无效的 focus payload: {e}"),
            )
        })
    }
}

/// 解析 scroll payload (Ticket #05)。
#[allow(dead_code)] // routing 表启用后使用
pub fn parse_scroll(payload: &Value) -> io::Result<AxScrollRequest> {
    if let Some(s) = payload.as_str() {
        crate::control_ax::parse_ax_scroll_payload(s)
    } else {
        serde_json::from_value(payload.clone()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("无效的 scroll payload: {e}"),
            )
        })
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_action_json() {
        let payload = json!({
            "target": {"id": "pid:1/window:0/path:0"},
            "action": "Press"
        });

        let req = parse_action(&payload).unwrap();
        assert_eq!(req.target.id.as_deref(), Some("pid:1/window:0/path:0"));
        assert_eq!(req.action, crate::control_ax::types::AxActionName::Press);
    }

    /// 字符串 payload 走 line-control 对象字面量语法, 与 `@ax-action` 原有契约一致。
    #[test]
    fn test_parse_action_object_literal_string() {
        let payload = json!(r#"{target:{app:"Calculator",description:"等于"},action:"Press"}"#);

        let req = parse_action(&payload).unwrap();
        assert_eq!(req.target.app.as_deref(), Some("Calculator"));
        assert_eq!(req.target.description.as_deref(), Some("等于"));
        assert_eq!(req.action, crate::control_ax::types::AxActionName::Press);
    }

    /// 裸 compact 写法不被 `@ax-action` 接受, 这是既有契约, 不是回归。
    #[test]
    fn test_parse_action_rejects_bare_compact() {
        let payload = json!("app:Calculator,description:等于,action:Press");
        assert!(parse_action(&payload).is_err());
    }

    #[test]
    fn test_parse_action_all_variants() {
        let actions = vec![
            "Press",
            "Open",
            "Confirm",
            "Cancel",
            "ShowMenu",
            "ScrollToVisible",
        ];

        for action_name in actions {
            let payload = json!({
                "target": {"id": "test"},
                "action": action_name
            });

            let result = parse_action(&payload);
            assert!(result.is_ok(), "应该能解析 action: {}", action_name);
        }
    }
}
