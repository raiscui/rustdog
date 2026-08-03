use std::io;

use super::{object_inner, parse_quoted_payload, split_object_field, split_object_fields};
use crate::control_protocol::ComputerActRequest;

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
    let mut observation_id: Option<String> = None;
    let mut timeout_ms: Option<u64> = None;
    let mut trace: Option<String> = None;

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
            format!(
                "@computer-act schema 必须是 {COMPUTER_ACT_SCHEMA_V1},实际收到: {schema}"
            ),
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
        observation_id,
        timeout_ms,
        trace,
    })
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
