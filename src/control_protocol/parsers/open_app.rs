use std::io;

use super::{
    object_inner, parse_quoted_payload, split_object_field, split_object_fields,
};
use crate::control_protocol::OpenAppRequest;

/// `@open-app` 默认 wait_ms:让被启动 app 有时间完成初次绘制/加载。
///
/// 1500ms 是经验值,够大多数 app 完成 launch,不够的 caller 自己传更大的值。
pub(crate) const DEFAULT_OPEN_APP_WAIT_MS: u64 = 1500;

pub(crate) fn parse_open_app_payload(input: &str) -> io::Result<OpenAppRequest> {
    let trimmed = input.trim();

    // `@open-app` 只接受对象 payload;字符串 payload 直接报错。
    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@open-app payload 必须是对象,实际收到: {input}"),
        ));
    }

    let inner = object_inner(trimmed, "@open-app")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@open-app 对象 payload 不能为空,需要 app_name 字段",
        ));
    }

    let mut app_name: Option<String> = None;
    let mut wait_ms: Option<u64> = None;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = field_name.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "app_name" => {
                if app_name.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@open-app payload 的 `app_name` 字段重复",
                    ));
                }
                let parsed = parse_quoted_payload(raw_value)?;
                if parsed.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@open-app 的 `app_name` 不能为空字符串",
                    ));
                }
                app_name = Some(parsed);
            }
            "wait_ms" => {
                if wait_ms.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@open-app payload 的 `wait_ms` 字段重复",
                    ));
                }
                let parsed = raw_value.parse::<i64>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@open-app 的 `wait_ms` 必须是整数: {raw_value}"),
                    )
                })?;
                if parsed < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@open-app 的 `wait_ms` 不能为负数: {parsed}"),
                    ));
                }
                wait_ms = Some(parsed as u64);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@open-app payload 包含未知字段: {other}"),
                ));
            }
        }
    }

    match app_name {
        Some(name) => Ok(OpenAppRequest {
            app_name: name,
            wait_ms: wait_ms.unwrap_or(DEFAULT_OPEN_APP_WAIT_MS),
        }),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@open-app payload 缺少 `app_name` 字段",
        )),
    }
}
