use std::io;

use super::{object_inner, split_object_field, split_object_fields};
use crate::control_protocol::WaitRequest;

pub(crate) fn parse_wait_payload(input: &str) -> io::Result<WaitRequest> {
    let trimmed = input.trim();

    // `@wait` 只接受对象 payload;字符串 payload 直接报错。
    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@wait payload 必须是对象,实际收到: {input}"),
        ));
    }

    let inner = object_inner(trimmed, "@wait")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@wait 对象 payload 不能为空,需要 duration_ms 字段",
        ));
    }

    let mut duration_ms: Option<u64> = None;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        // 字段名容错 (normalize_object_field_name 由 dispatch 路径负责,
        // 这里只比对规范化后的名字)。
        let field_name = field_name.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "duration_ms" => {
                if duration_ms.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@wait payload 的 `duration_ms` 字段重复",
                    ));
                }
                let parsed = raw_value.parse::<i64>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@wait 的 `duration_ms` 必须是整数: {raw_value}"),
                    )
                })?;
                if parsed < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@wait 的 `duration_ms` 不能为负数: {parsed}"),
                    ));
                }
                duration_ms = Some(parsed as u64);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@wait payload 包含未知字段: {other}"),
                ));
            }
        }
    }

    match duration_ms {
        Some(value) => Ok(WaitRequest { duration_ms: value }),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@wait payload 缺少 `duration_ms` 字段",
        )),
    }
}
