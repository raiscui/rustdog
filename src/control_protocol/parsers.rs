use std::io;

pub mod ax;
mod cancel_seq;
mod computer_act;
mod key;
mod open_app;
mod pty;
mod screenshot;
mod wait;

pub(super) use self::ax::{
    parse_ax_action_payload, parse_ax_focus_payload, parse_ax_press_payload,
    parse_ax_press_sequence_payload, parse_ax_scroll_payload, parse_ax_set_value_payload,
    parse_ax_tree_payload, parse_type_text_payload,
};
pub(super) use self::cancel_seq::parse_cancel_payload;
pub(super) use self::computer_act::parse_computer_act_payload;
pub(super) use self::key::parse_key_payload;
pub(super) use self::open_app::parse_open_app_payload;
pub(super) use self::pty::{
    parse_pty_attach_payload, parse_pty_close_payload, parse_pty_detach_payload, parse_pty_payload,
};
pub(super) use self::screenshot::parse_screenshot_payload;
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

/// 解析无需 shell 引号的单个短格式字段.
///
/// 允许集刻意小于完整对象合同.空白、通配符、引号和控制符都必须回到
/// quoted/object payload,避免小模型生成的文本在进入 rdog 前被 shell 改写.
pub(crate) fn parse_compact_atom(kind: &str, input: &str) -> io::Result<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 短格式字段不能为空"),
        ));
    }

    if let Some(invalid) = value.chars().find(|character| {
        !character.is_alphanumeric()
            && !matches!(character, '_' | '-' | '.' | '/' | ':' | '+' | '=' | '@')
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 短格式包含不安全或歧义字符 `{invalid}`: {input}"),
        ));
    }

    Ok(value.to_owned())
}

/// 短格式 AX 命令允许的窗口选择器.
///
/// `WindowId` 保留现有 canonical identity 合同.`App` 只表达应用名,
/// 真正的窗口 ID 必须在执行动作前通过 fresh window query 唯一解析.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompactWindowSelector {
    WindowId(String),
    App(String),
}

/// 解析 `window_selector,value` 两字段短格式.
pub(crate) fn parse_compact_window_pair(
    kind: &str,
    input: &str,
) -> io::Result<(CompactWindowSelector, String)> {
    let mut fields = input.split(',');
    let selector = fields.next().unwrap_or_default();
    let value = fields.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 短格式必须是 window_selector,value: {input}"),
        )
    })?;

    if fields.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 短格式只允许两个字段: {input}"),
        ));
    }

    Ok((
        parse_compact_window_selector(kind, selector)?,
        parse_compact_atom(kind, value)?,
    ))
}

/// 解析 `window_selector,button...` AXButton有序短格式.
pub(crate) fn parse_compact_ax_button_sequence(
    kind: &str,
    input: &str,
) -> io::Result<(CompactWindowSelector, Vec<String>)> {
    let mut fields = input.split(',');
    let selector = parse_compact_window_selector(kind, fields.next().unwrap_or_default())?;
    let mut raw_values = fields.collect::<Vec<_>>();
    if raw_values
        .last()
        .is_some_and(|value| value.trim().is_empty())
    {
        raw_values.pop();
    }
    let values = raw_values
        .into_iter()
        .map(|value| parse_compact_atom(kind, value))
        .collect::<io::Result<Vec<_>>>()?;

    if values.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 至少需要一个 value: {input}"),
        ));
    }
    if values.len() > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 最多允许 32 个 value"),
        ));
    }

    Ok((selector, values))
}

pub(crate) fn parse_compact_window_selector(
    kind: &str,
    input: &str,
) -> io::Result<CompactWindowSelector> {
    let selector = parse_compact_atom(kind, input)?;
    if let Some(app) = selector.strip_prefix("app:") {
        // ponytail: ASCII-only gate. macOS Launch Services resolves apps by
        // bundle id or English display name; non-ASCII app names pass the
        // compact atom parser but then 0-match at the WindowServer layer
        // with no actionable error. Reject here so the model sees the
        // failure with a hint before issuing the control call.
        if app.is_empty() {
            return Err(invalid_compact_window_id(kind, &selector));
        }
        if !app.is_ascii() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{kind} 短格式 app:APP 必须是 ASCII 名称(macOS Launch Services 不支持非 ASCII);received: {app};请改用 Launch Services 英文名称(例如 app:Calculator 而不是 app:计算器)"
                ),
            ));
        }
        return Ok(CompactWindowSelector::App(app.to_owned()));
    }

    parse_compact_window_id(kind, &selector).map(CompactWindowSelector::WindowId)
}

fn parse_compact_window_id(kind: &str, input: &str) -> io::Result<String> {
    let window_id = parse_compact_atom(kind, input)?;
    let Some(rest) = window_id.strip_prefix("pid:") else {
        return Err(invalid_compact_window_id(kind, &window_id));
    };
    let Some((pid, window_index)) = rest.split_once("/window:") else {
        return Err(invalid_compact_window_id(kind, &window_id));
    };

    let pid = pid
        .parse::<i32>()
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| invalid_compact_window_id(kind, &window_id))?;
    let window_index = window_index
        .parse::<usize>()
        .map_err(|_| invalid_compact_window_id(kind, &window_id))?;

    Ok(format!("pid:{pid}/window:{window_index}"))
}

fn invalid_compact_window_id(kind: &str, window_id: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{kind} 短格式 window selector 必须是 app:APP 或 pid:<正整数>/window:<非负整数>: {window_id}"),
    )
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
