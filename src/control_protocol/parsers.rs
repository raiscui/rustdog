use std::io;

mod cancel_seq;
mod key;
mod open_app;
mod pty;
mod screenshot;
mod wait;

pub(super) use self::key::parse_key_payload;
pub(super) use self::pty::{
    parse_pty_attach_payload, parse_pty_close_payload, parse_pty_detach_payload, parse_pty_payload,
};
pub(super) use self::screenshot::parse_screenshot_payload;
pub(super) use self::cancel_seq::parse_cancel_payload;
pub(super) use self::open_app::parse_open_app_payload;
pub(super) use self::wait::parse_wait_payload;

pub(crate) fn object_inner<'a>(input: &'a str, kind: &str) -> io::Result<&'a str> {
    let trimmed = input.trim();
    trimmed
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .map(str::trim)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{kind} payload 必须是对象: {input}"),
            )
        })
}

fn parse_i32_field(kind: &str, field_name: &str, input: &str) -> io::Result<i32> {
    input.trim().parse::<i32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 的 `{field_name}` 必须是整数: {input}"),
        )
    })
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 不能为空字符串"),
        ));
    }
    Ok(value)
}

pub(crate) fn split_object_fields(input: &str) -> io::Result<Vec<&str>> {
    let mut fields = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut square_depth = 0usize;
    let mut object_depth = 0usize;

    for (index, byte) in input.as_bytes().iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'[' if !in_string => square_depth += 1,
            b']' if !in_string => {
                square_depth = square_depth.checked_sub(1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("对象 payload 存在多余的 `]`: {input}"),
                    )
                })?;
            }
            b'{' if !in_string => object_depth += 1,
            b'}' if !in_string => {
                object_depth = object_depth.checked_sub(1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("对象 payload 存在多余的 `}}`: {input}"),
                    )
                })?;
            }
            b',' if !in_string && square_depth == 0 && object_depth == 0 => {
                let field = input[start..index].trim();
                if field.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@key 对象 payload 存在空字段: {input}"),
                    ));
                }
                fields.push(field);
                start = index + 1;
            }
            _ => {}
        }
    }

    if in_string {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@key 对象 payload 存在未闭合字符串: {input}"),
        ));
    }
    if square_depth != 0 || object_depth != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("对象 payload 存在未闭合的数组或对象: {input}"),
        ));
    }

    let tail = input[start..].trim();
    if tail.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@key 对象 payload 末尾存在空字段: {input}"),
        ));
    }
    fields.push(tail);
    Ok(fields)
}

pub(crate) fn split_object_field(field: &str) -> io::Result<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    let mut square_depth = 0usize;
    let mut object_depth = 0usize;

    for (index, byte) in field.as_bytes().iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'[' if !in_string => square_depth += 1,
            b']' if !in_string => {
                square_depth = square_depth.checked_sub(1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("对象字段存在多余的 `]`: {field}"),
                    )
                })?;
            }
            b'{' if !in_string => object_depth += 1,
            b'}' if !in_string => {
                object_depth = object_depth.checked_sub(1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("对象字段存在多余的 `}}`: {field}"),
                    )
                })?;
            }
            b':' if !in_string && square_depth == 0 && object_depth == 0 => {
                let field_name = field[..index].trim();
                let field_value = field[index + 1..].trim();
                if field_name.is_empty() || field_value.is_empty() {
                    break;
                }
                return Ok((field_name, field_value));
            }
            _ => {}
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@key 对象字段格式非法: {field}"),
    ))
}

pub(crate) fn normalize_object_field_name(field_name: &str) -> io::Result<String> {
    let trimmed = field_name.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@key 对象字段名不能为空",
        ));
    }

    Ok(trimmed.trim_matches('"').to_ascii_lowercase())
}

pub(super) fn parse_control_header(command: &str) -> io::Result<(&str, Option<u64>)> {
    let header = command
        .split_once(':')
        .map(|(header, _)| header)
        .unwrap_or(command)
        .trim();

    // 特殊处理: `@cancel#seq#5:{target_seq:1}` 这种命令名本身含 `#`
    // 的复合命令。常规 split_once('#') 会把 `cancel#seq` 拆成 kind=`cancel`
    // request_id=`seq`,所以这里先尝试把 `cancel#seq` 整体识别出来。
    if let Some(rest) = header.strip_prefix("cancel#seq") {
        if let Some(request_id_str) = rest.strip_prefix('#') {
            let request_id = parse_request_id(request_id_str.trim(), command)?;
            return Ok(("cancel#seq", Some(request_id)));
        }
        // 没有 `#<request_id>` 后缀 — 这是 `@cancel#seq` 无 request_id 形式
        return Ok(("cancel#seq", None));
    }

    if let Some((kind, request_id)) = header.split_once('#') {
        let request_id = parse_request_id(request_id.trim(), command)?;
        return Ok((kind.trim(), Some(request_id)));
    }

    Ok((header, None))
}

fn parse_request_id(input: &str, command: &str) -> io::Result<u64> {
    if input.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("控制指令 request id 不能为空: {command}"),
        ));
    }

    input.parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("控制指令 request id 必须是无符号整数: {command}"),
        )
    })
}

pub(super) fn require_non_empty_payload<T>(
    kind: &str,
    payload: String,
    constructor: impl FnOnce(String) -> T,
) -> io::Result<T> {
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@{kind} 的 payload 不能为空"),
        ));
    }

    if payload.contains('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@{kind} 首版不支持多行 payload"),
        ));
    }

    Ok(constructor(payload))
}

pub(crate) fn parse_quoted_payload(input: &str) -> io::Result<String> {
    if !input.starts_with('"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("控制指令 payload 必须使用双引号包裹: {input}"),
        ));
    }

    let mut escaped = false;
    let mut result = String::new();

    for (index, ch) in input.char_indices().skip(1) {
        if escaped {
            match ch {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                other => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("不支持的转义序列: \\{other}"),
                    ))
                }
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => {
                if input[index + 1..].trim().is_empty() {
                    return Ok(result);
                }

                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("控制指令 payload 后存在多余内容: {input}"),
                ));
            }
            other => result.push(other),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("未闭合的控制指令 payload: {input}"),
    ))
}
