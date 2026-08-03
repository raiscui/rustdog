use std::io;

use super::{
    normalize_object_field_name, parse_i32_field, parse_non_empty_string, parse_quoted_payload,
    require_non_empty_payload, split_object_field, split_object_fields,
};
use crate::control_protocol::{
    KeyDelivery, KeyMode, KeyRequest, KeyResponseMode, DEFAULT_KEY_HOLD_MS,
};

pub(crate) fn parse_key_payload(input: &str) -> io::Result<KeyRequest> {
    let trimmed = input.trim();

    if trimmed.starts_with('"') {
        let key = parse_quoted_payload(trimmed)?;
        return default_key_request(key);
    }

    if trimmed.starts_with('{') {
        return parse_key_object_payload(trimmed);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("控制指令 payload 必须是字符串或对象: {input}"),
    ))
}

fn default_key_request(key: String) -> io::Result<KeyRequest> {
    require_non_empty_payload("key", key, |key| {
        KeyRequest::legacy(key, DEFAULT_KEY_HOLD_MS, KeyMode::PressRelease)
    })
}

fn parse_key_object_payload(input: &str) -> io::Result<KeyRequest> {
    let inner = input
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("控制指令对象 payload 必须使用大括号包裹: {input}"),
            )
        })?
        .trim();

    let mut key = None::<String>;
    let mut hold_ms = None::<u64>;
    let mut mode = None::<KeyMode>;
    let mut delivery = None::<KeyDelivery>;
    let mut pid = None::<i32>;
    let mut window_id = None::<String>;
    let mut delivery_seen = false;

    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@key 对象 payload 不能为空",
        ));
    }

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "key" => {
                if key.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `key` 字段重复",
                    ));
                }
                key = Some(parse_quoted_payload(raw_value)?);
            }
            "hold_ms" => {
                if hold_ms.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `hold_ms` 字段重复",
                    ));
                }
                hold_ms = Some(parse_hold_ms(raw_value)?);
            }
            "mode" => {
                if mode.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `mode` 字段重复",
                    ));
                }
                mode = Some(parse_key_mode(raw_value)?);
            }
            "delivery" => {
                if delivery.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `delivery` 字段重复",
                    ));
                }
                delivery_seen = true;
                delivery = Some(parse_key_delivery(raw_value)?);
            }
            "pid" => {
                if pid.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `pid` 字段重复",
                    ));
                }
                pid = Some(parse_i32_field("@key", "pid", raw_value)?);
            }
            "window_id" => {
                if window_id.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@key 对象 payload 的 `window_id` 字段重复",
                    ));
                }
                window_id = Some(parse_non_empty_string("@key.window_id", raw_value)?);
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@key 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    let key = key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@key 对象 payload 缺少必填字段 `key`",
        )
    })?;

    let delivery = delivery.unwrap_or(KeyDelivery::Global);
    validate_key_delivery(delivery, pid, window_id.as_deref())?;
    let response_mode = if delivery_seen || pid.is_some() || window_id.is_some() {
        KeyResponseMode::Structured
    } else {
        KeyResponseMode::Legacy
    };

    default_key_request(key).map(|defaulted| KeyRequest {
        key: defaulted.key,
        hold_ms: hold_ms.unwrap_or(DEFAULT_KEY_HOLD_MS),
        mode: mode.unwrap_or(KeyMode::PressRelease),
        delivery,
        pid,
        window_id,
        response_mode,
    })
}

fn parse_key_delivery(input: &str) -> io::Result<KeyDelivery> {
    let value = parse_quoted_payload(input)?;
    match value.to_ascii_lowercase().as_str() {
        "global" => Ok(KeyDelivery::Global),
        "pid-targeted" | "pid_targeted" => Ok(KeyDelivery::PidTargeted),
        "window-targeted" | "window_targeted" => Ok(KeyDelivery::WindowTargeted),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "@key 的 `delivery` 只支持 \"global\" | \"pid-targeted\" | \"window-targeted\": {value}"
            ),
        )),
    }
}

fn validate_key_delivery(
    delivery: KeyDelivery,
    pid: Option<i32>,
    window_id: Option<&str>,
) -> io::Result<()> {
    match delivery {
        KeyDelivery::Global => {
            if pid.is_some() || window_id.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@key delivery:\"global\" 不能同时携带 `pid` 或 `window_id`",
                ));
            }
        }
        KeyDelivery::PidTargeted => {
            if pid.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@key delivery:\"pid-targeted\" 缺少必填字段 `pid`",
                ));
            }
            if window_id.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@key delivery:\"pid-targeted\" 不能同时携带 `window_id`",
                ));
            }
        }
        KeyDelivery::WindowTargeted => {
            if window_id.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@key delivery:\"window-targeted\" 缺少必填字段 `window_id`",
                ));
            }
            if pid.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@key delivery:\"window-targeted\" 不能同时携带 `pid`",
                ));
            }
        }
    }

    Ok(())
}

fn parse_hold_ms(input: &str) -> io::Result<u64> {
    input.parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@key 的 `hold_ms` 必须是无符号整数: {input}"),
        )
    })
}

fn parse_key_mode(input: &str) -> io::Result<KeyMode> {
    let mode = parse_quoted_payload(input)?;
    match mode.to_ascii_lowercase().as_str() {
        "press_release" => Ok(KeyMode::PressRelease),
        "press" => Ok(KeyMode::Press),
        "release" => Ok(KeyMode::Release),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@key 的 `mode` 不支持该值: {mode}"),
        )),
    }
}
