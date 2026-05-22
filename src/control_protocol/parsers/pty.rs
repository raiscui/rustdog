use std::io;

use super::{
    normalize_object_field_name, object_inner, parse_quoted_payload, require_non_empty_payload,
    split_object_field, split_object_fields,
};
use crate::control_protocol::{
    PtyAttachRequest, PtyCloseRequest, PtyDetachRequest, PtyOpenRequest,
};

pub(crate) fn parse_pty_payload(input: &str) -> io::Result<PtyOpenRequest> {
    let trimmed = input.trim();

    if trimmed.starts_with('"') {
        let command_line = parse_pty_command_line_payload(trimmed)?;
        return default_pty_request(command_line);
    }

    if trimmed.starts_with('{') {
        return parse_pty_object_payload(trimmed);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@pty payload 必须是字符串或对象: {input}"),
    ))
}

fn parse_pty_object_payload(input: &str) -> io::Result<PtyOpenRequest> {
    let inner = object_inner(input, "@pty")?;
    let mut cmd = None::<String>;
    let mut args = None::<Vec<String>>;
    let mut argv = None::<Vec<String>>;
    let mut cols = None::<u16>;
    let mut rows = None::<u16>;

    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 对象 payload 不能为空",
        ));
    }

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "cmd" => cmd = Some(parse_quoted_payload(raw_value)?),
            "args" => args = Some(parse_string_array(raw_value)?),
            "argv" => argv = Some(parse_string_array(raw_value)?),
            "cols" => cols = Some(parse_pty_dimension("cols", raw_value)?),
            "rows" => rows = Some(parse_pty_dimension("rows", raw_value)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@pty 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    let cmd = require_non_empty_payload("pty.cmd", cmd.unwrap_or_default(), |value| value)?;
    if args.is_some() && argv.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 对象 payload 不能同时包含 `args` 和 legacy `argv`",
        ));
    }

    let args = if let Some(args) = args {
        validate_pty_args(&args)?;
        args
    } else if let Some(argv) = argv {
        normalize_legacy_pty_argv(&cmd, argv)?
    } else {
        Vec::new()
    };

    Ok(PtyOpenRequest {
        cmd,
        args,
        cols: cols.unwrap_or(80),
        rows: rows.unwrap_or(24),
    })
}

fn default_pty_request(cmd: String) -> io::Result<PtyOpenRequest> {
    let argv = split_shell_style_words(&cmd)?;
    let cmd =
        require_non_empty_payload("pty", argv.first().cloned().unwrap_or_default(), |value| {
            value
        })?;
    let args = argv.into_iter().skip(1).collect::<Vec<_>>();
    validate_pty_args(&args)?;

    Ok(PtyOpenRequest {
        cmd,
        args,
        cols: 80,
        rows: 24,
    })
}

fn split_shell_style_words(input: &str) -> io::Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut saw_token = false;

    while let Some(ch) = chars.next() {
        if in_single_quote {
            if ch == '\'' {
                in_single_quote = false;
            } else {
                current.push(ch);
            }
            saw_token = true;
            continue;
        }

        if in_double_quote {
            match ch {
                '"' => in_double_quote = false,
                '\\' => {
                    let Some(next) = chars.next() else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "@pty 字符串简写存在未完成的反斜杠转义",
                        ));
                    };
                    current.push(next);
                }
                _ => current.push(ch),
            }
            saw_token = true;
            continue;
        }

        match ch {
            '\'' => {
                in_single_quote = true;
                saw_token = true;
            }
            '"' => {
                in_double_quote = true;
                saw_token = true;
            }
            '\\' => {
                let Some(next) = chars.next() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@pty 字符串简写存在未完成的反斜杠转义",
                    ));
                };
                current.push(next);
                saw_token = true;
            }
            ch if ch.is_whitespace() => {
                if saw_token {
                    words.push(std::mem::take(&mut current));
                    saw_token = false;
                }
            }
            _ => {
                current.push(ch);
                saw_token = true;
            }
        }
    }

    if in_single_quote || in_double_quote {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 字符串简写存在未闭合的引号",
        ));
    }

    if saw_token {
        words.push(current);
    }

    if words.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 字符串简写不能为空",
        ));
    }

    Ok(words)
}

fn parse_pty_command_line_payload(input: &str) -> io::Result<String> {
    let bytes = input.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@pty 字符串 payload 必须使用双引号包裹: {input}"),
        ));
    }

    let mut escaped = false;
    let mut result = String::new();

    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if escaped {
            match byte {
                b'"' => result.push('"'),
                b'\\' => result.push('\\'),
                b'n' => result.push('\n'),
                b'r' => result.push('\r'),
                b't' => result.push('\t'),
                other => {
                    result.push('\\');
                    result.push(other as char);
                }
            }
            escaped = false;
            continue;
        }

        match byte {
            b'\\' => escaped = true,
            b'"' => {
                if input[index + 1..].trim().is_empty() {
                    return Ok(result);
                }

                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@pty 字符串 payload 后存在多余内容: {input}"),
                ));
            }
            other => result.push(other as char),
        }
    }

    if escaped {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 字符串 payload 存在未完成的反斜杠转义",
        ));
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("未闭合的 @pty 字符串 payload: {input}"),
    ))
}

fn validate_pty_args(args: &[String]) -> io::Result<()> {
    if args.iter().any(|item| item.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 的 args 不能包含空参数",
        ));
    }

    Ok(())
}

fn normalize_legacy_pty_argv(cmd: &str, argv: Vec<String>) -> io::Result<Vec<String>> {
    if argv.is_empty() || argv.iter().any(|item| item.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty 的 legacy argv 不能为空,且不能包含空参数",
        ));
    }

    if argv[0] != cmd {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "@pty 的 legacy argv[0] 必须与 cmd 一致: cmd={cmd}, argv[0]={}",
                argv[0]
            ),
        ));
    }

    Ok(argv.into_iter().skip(1).collect())
}

pub(crate) fn parse_pty_close_payload(input: &str) -> io::Result<PtyCloseRequest> {
    let inner = object_inner(input, "@pty-close")?;
    let mut session_id = None::<String>;

    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty-close 对象 payload 不能为空",
        ));
    }

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "session_id" => session_id = Some(parse_quoted_payload(raw_value)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@pty-close 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    Ok(PtyCloseRequest {
        session_id: require_non_empty_payload(
            "pty-close.session_id",
            session_id.unwrap_or_default(),
            |value| value,
        )?,
    })
}

pub(crate) fn parse_pty_detach_payload(input: &str) -> io::Result<PtyDetachRequest> {
    let inner = object_inner(input, "@pty-detach")?;
    let mut session_id = None::<String>;

    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty-detach 对象 payload 不能为空",
        ));
    }

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "session_id" => session_id = Some(parse_quoted_payload(raw_value)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@pty-detach 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    Ok(PtyDetachRequest {
        session_id: require_non_empty_payload(
            "pty-detach.session_id",
            session_id.unwrap_or_default(),
            |value| value,
        )?,
    })
}

pub(crate) fn parse_pty_attach_payload(input: &str) -> io::Result<PtyAttachRequest> {
    let trimmed = input.trim();

    if trimmed.starts_with('"') {
        let session_id = parse_quoted_payload(trimmed)?;
        return Ok(PtyAttachRequest {
            session_id: require_non_empty_payload("pty-attach", session_id, |value| value)?,
            cols: 80,
            rows: 24,
        });
    }

    let inner = object_inner(trimmed, "@pty-attach")?;
    let mut session_id = None::<String>;
    let mut cols = None::<u16>;
    let mut rows = None::<u16>;

    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@pty-attach 对象 payload 不能为空",
        ));
    }

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "session_id" => session_id = Some(parse_quoted_payload(raw_value)?),
            "cols" => cols = Some(parse_pty_dimension("cols", raw_value)?),
            "rows" => rows = Some(parse_pty_dimension("rows", raw_value)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@pty-attach 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    Ok(PtyAttachRequest {
        session_id: require_non_empty_payload(
            "pty-attach.session_id",
            session_id.unwrap_or_default(),
            |value| value,
        )?,
        cols: cols.unwrap_or(80),
        rows: rows.unwrap_or(24),
    })
}

fn parse_pty_dimension(field_name: &str, input: &str) -> io::Result<u16> {
    let value = input.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@pty 的 `{field_name}` 必须是无符号整数: {input}"),
        )
    })?;

    if value == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@pty 的 `{field_name}` 必须大于 0"),
        ));
    }

    Ok(value)
}

fn parse_string_array(input: &str) -> io::Result<Vec<String>> {
    let inner = input
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("字符串数组必须使用方括号包裹: {input}"),
            )
        })?
        .trim();

    if inner.is_empty() {
        return Ok(Vec::new());
    }

    split_object_fields(inner)?
        .into_iter()
        .map(parse_quoted_payload)
        .collect()
}
