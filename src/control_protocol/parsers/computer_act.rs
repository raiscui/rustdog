use std::io;

use super::{object_inner, parse_quoted_payload, split_object_field, split_object_fields};
use crate::control_ax::AxFindQuery;
use crate::control_protocol::{
    ComputerActPostcondition, ComputerActPostconditionKind, ComputerActRequest,
};

/// 当前唯一支持的 schema 版本;后续 v2/v3 走新 schema id, 不破坏 v1 client。
pub(crate) const COMPUTER_ACT_SCHEMA_V1: &str = "rdog.computer-act.v1";

/// `@computer-act` payload 顶层 7 个字段 (4 必填 + 3 可选)。
///
/// 必填:
/// - `schema`: 必须是 `rdog.computer-act.v1`
/// - `action`: 13 动作闭集之一 (Mano-CUA 子集)
/// - `args`: 动作特定参数, JSON object
///
/// 可选 (后续 ticket 11/12/16/18 填充):
/// - `verify`: verify policy (`none` / `best_effort` / `always`)
/// - `observation_id`: 跨轮复用 obs 时传
/// - `timeout_ms`: 覆盖 per-action class 默认 timeout
/// - `trace`: full trace 落盘触发 (`savefile`)
pub(crate) fn parse_computer_act_payload(input: &str) -> io::Result<ComputerActRequest> {
    let trimmed = input.trim();

    // `@computer-act` 只接受对象 payload;字符串 payload 直接报错。
    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@computer-act payload 必须是对象,实际收到: {input}"),
        ));
    }

    let inner = object_inner(trimmed, "@computer-act")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act 对象 payload 不能为空,需要 schema/action/args 字段",
        ));
    }

    let mut schema: Option<String> = None;
    let mut action: Option<String> = None;
    let mut args: Option<String> = None;
    let mut verify: Option<String> = None;
    let mut postcondition: Option<ComputerActPostcondition> = None;
    let mut observation_id: Option<String> = None;
    let mut timeout_ms: Option<u64> = None;
    let mut trace: Option<String> = None;
    let mut epoch: Option<u64> = None;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = field_name.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "schema" => {
                if schema.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `schema` 字段重复",
                    ));
                }
                schema = Some(parse_quoted_payload(raw_value)?);
            }
            "action" => {
                if action.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `action` 字段重复",
                    ));
                }
                action = Some(parse_quoted_payload(raw_value)?);
            }
            "args" => {
                if args.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `args` 字段重复",
                    ));
                }
                // `args` 用 rdog dict 语法 (unquoted keys, 类似 Mano-CUA 输出):
                // `{duration_ms:100, content:"text"}`。
                // 内部需要 serde_json::Value, 所以先把 unquoted key 加引号,
                // 再 JSON-parse。
                if !raw_value.starts_with('{') {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@computer-act 的 `args` 必须是对象: {raw_value}"),
                    ));
                }
                let json_str = rdog_dict_to_json_string(raw_value);
                args = Some(json_str);
            }
            "verify" => {
                if verify.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `verify` 字段重复",
                    ));
                }
                verify = Some(parse_quoted_payload(raw_value)?);
            }
            "postcondition" => {
                if postcondition.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `postcondition` 字段重复",
                    ));
                }
                postcondition = Some(parse_postcondition(raw_value)?);
            }
            "observation_id" => {
                if observation_id.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `observation_id` 字段重复",
                    ));
                }
                observation_id = Some(parse_quoted_payload(raw_value)?);
            }
            "timeout_ms" => {
                if timeout_ms.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `timeout_ms` 字段重复",
                    ));
                }
                let parsed = raw_value.parse::<i64>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@computer-act 的 `timeout_ms` 必须是整数: {raw_value}"),
                    )
                })?;
                if parsed < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@computer-act 的 `timeout_ms` 不能为负数: {parsed}"),
                    ));
                }
                timeout_ms = Some(parsed as u64);
            }
            "trace" => {
                if trace.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `trace` 字段重复",
                    ));
                }
                trace = Some(parse_quoted_payload(raw_value)?);
            }
            "epoch" => {
                if epoch.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@computer-act payload 的 `epoch` 字段重复",
                    ));
                }
                let parsed = raw_value.parse::<i64>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@computer-act 的 `epoch` 必须是整数: {raw_value}"),
                    )
                })?;
                if parsed < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@computer-act 的 `epoch` 不能为负数: {parsed}"),
                    ));
                }
                epoch = Some(parsed as u64);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@computer-act payload 包含未知字段: {other}"),
                ));
            }
        }
    }

    let schema = schema.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act payload 缺少 `schema` 字段",
        )
    })?;
    if schema != COMPUTER_ACT_SCHEMA_V1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@computer-act schema 必须是 {COMPUTER_ACT_SCHEMA_V1},实际收到: {schema}"),
        ));
    }
    let action = action.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act payload 缺少 `action` 字段",
        )
    })?;
    let args_str = args.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act payload 缺少 `args` 字段",
        )
    })?;
    let args_value: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@computer-act 的 `args` 不是合法 JSON: {e}"),
        )
    })?;
    if !args_value.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act 的 `args` 必须是 JSON object",
        ));
    }

    Ok(ComputerActRequest {
        schema,
        action,
        args: args_value,
        verify,
        postcondition,
        observation_id,
        timeout_ms,
        trace,
        epoch,
    })
}

fn parse_postcondition(raw: &str) -> io::Result<ComputerActPostcondition> {
    let value: serde_json::Value =
        serde_json::from_str(&rdog_dict_to_json_string(raw)).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@computer-act.postcondition 不是合法对象: {err}"),
            )
        })?;
    let object = value.as_object().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act.postcondition 必须是对象",
        )
    })?;
    if object.keys().any(|key| key != "kind" && key != "query") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@computer-act.postcondition 只支持 kind 和 query 字段",
        ));
    }
    let kind = match object.get("kind").and_then(serde_json::Value::as_str) {
        Some("exists") => ComputerActPostconditionKind::Exists,
        Some("not_exists") => ComputerActPostconditionKind::NotExists,
        Some(other) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@computer-act.postcondition.kind 不支持: {other}"),
            ))
        }
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@computer-act.postcondition 缺少字符串 kind",
            ))
        }
    };
    let query = parse_postcondition_query(object.get("query"))?;
    query.validate_with_context("@computer-act.postcondition.query")?;
    Ok(ComputerActPostcondition { kind, query })
}

fn parse_postcondition_query(value: Option<&serde_json::Value>) -> io::Result<AxFindQuery> {
    let object = value
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "@computer-act.postcondition.query 必须是对象",
            )
        })?;
    let mut query = AxFindQuery::default();
    for (field, value) in object {
        let value = value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@computer-act.postcondition.query.{field} 必须是非空字符串"),
                )
            })?;
        let slot = match field.as_str() {
            "process" => &mut query.process,
            "process_contains" => &mut query.process_contains,
            "window_title" => &mut query.window_title,
            "window_title_contains" => &mut query.window_title_contains,
            "role" => &mut query.role,
            "subrole" => &mut query.subrole,
            "name" => &mut query.name,
            "name_contains" => &mut query.name_contains,
            "description" => &mut query.description,
            "description_contains" => &mut query.description_contains,
            "value" => &mut query.value,
            "value_contains" => &mut query.value_contains,
            "action" => &mut query.action,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@computer-act.postcondition.query 包含未知字段: {other}"),
                ))
            }
        };
        *slot = Some(value.to_owned());
    }
    Ok(query)
}

/// 把 rdog dict 语法 (unquoted keys) 转换成标准 JSON 字符串。
///
/// 例: `{duration_ms:100, content:"text", ref:"@e1"}`
///   → `{"duration_ms":100, "content":"text", "ref":"@e1"}`
///
/// 假设: keys 是 word chars (`\w+`), values 已经是合法 JSON 字面量
/// (数字 / 布尔 / 字符串带引号 / null / 嵌套 {} 或 [])。
/// 这是 `@computer-act` args 的约束, 不适合通用 rdog 输入。
fn rdog_dict_to_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_string => {
                out.push(c);
                escaped = !escaped;
                continue;
            }
            '"' if in_string && !escaped => {
                out.push(c);
                in_string = false;
                continue;
            }
            '"' if !in_string => {
                out.push(c);
                in_string = true;
                continue;
            }
            _ if escaped => {
                out.push(c);
                escaped = false;
                continue;
            }
            _ => {}
        }
        if !in_string {
            // 尝试识别 `word:` 模式 (key 加引号)
            if c.is_alphanumeric() || c == '_' {
                let mut key = String::from(c);
                while let Some(&nc) = chars.peek() {
                    if nc.is_alphanumeric() || nc == '_' {
                        key.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // 看 key 后面是不是 `:`, 是的话加引号
                if chars.peek() == Some(&':') {
                    chars.next(); // consume ':'
                    out.push('"');
                    out.push_str(&key);
                    out.push('"');
                    out.push(':');
                } else {
                    out.push_str(&key);
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod dict_to_json_tests {
    use super::rdog_dict_to_json_string;

    #[test]
    fn should_quote_unquoted_keys() {
        assert_eq!(
            rdog_dict_to_json_string("{duration_ms:100}"),
            "{\"duration_ms\":100}"
        );
    }

    #[test]
    fn should_preserve_quoted_string_values() {
        assert_eq!(
            rdog_dict_to_json_string("{content:\"hello world\"}"),
            "{\"content\":\"hello world\"}"
        );
    }

    #[test]
    fn should_handle_mixed_keys_and_values() {
        assert_eq!(
            rdog_dict_to_json_string("{duration_ms:100, content:\"hi\"}"),
            "{\"duration_ms\":100, \"content\":\"hi\"}"
        );
    }
}
