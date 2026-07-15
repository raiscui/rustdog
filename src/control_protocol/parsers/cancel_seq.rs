use std::io;

use super::{object_inner, split_object_field, split_object_fields};
use crate::control_protocol::CancelRequest;

pub(crate) fn parse_cancel_payload(input: &str) -> io::Result<CancelRequest> {
    let trimmed = input.trim();

    // `@cancel#seq` 只接受对象 payload;字符串 payload 直接报错。
    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@cancel#seq payload 必须是对象,实际收到: {input}"),
        ));
    }

    let inner = object_inner(trimmed, "@cancel#seq")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@cancel#seq 对象 payload 不能为空,需要 target_seq 字段",
        ));
    }

    let mut target_seq: Option<u64> = None;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = field_name.trim().to_ascii_lowercase();
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target_seq" => {
                if target_seq.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@cancel#seq payload 的 `target_seq` 字段重复",
                    ));
                }
                let parsed = raw_value.parse::<i64>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@cancel#seq 的 `target_seq` 必须是整数: {raw_value}"),
                    )
                })?;
                if parsed < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@cancel#seq 的 `target_seq` 不能为负数: {parsed}"),
                    ));
                }
                target_seq = Some(parsed as u64);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@cancel#seq payload 包含未知字段: {other}"),
                ));
            }
        }
    }

    match target_seq {
        Some(seq) => Ok(CancelRequest { target_seq: seq }),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@cancel#seq payload 缺少 `target_seq` 字段",
        )),
    }
}
