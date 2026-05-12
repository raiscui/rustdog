use std::io;

use crate::control_frames::SaveFileFrame;

/// 行级控制协议的解析结果。
///
/// 这里故意只处理“一整行文本”的判定:
/// - `@...` 进入控制协议
/// - `@@...` 退回字面 shell 文本
/// - 其他内容保持普通 shell 文本语义
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlParseResult {
    LiteralShellLine(String),
    Control(ControlRequest),
}

/// 首版支持的控制命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCommand {
    Ping,
    Key(KeyRequest),
    Paste(String),
    Script(String),
    PtyOpen(PtyOpenRequest),
    PtyClose(PtyCloseRequest),
    PtyDetach(PtyDetachRequest),
    PtyAttach(PtyAttachRequest),
    Screenshot(ScreenshotRequest),
    SaveFile(SaveFileFrame),
}

pub const DEFAULT_KEY_HOLD_MS: u64 = 200;
pub const DEFAULT_SCREENSHOT_QUALITY: u8 = 75;

/// `@key` 的结构化请求。
///
/// 旧字符串写法会先被提升成这个结构:
/// - `@key:"right-option"`
///   -> `KeyRequest { key: "right-option", hold_ms: 200, mode: PressRelease }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequest {
    pub key: String,
    pub hold_ms: u64,
    pub mode: KeyMode,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyMode {
    PressRelease,
    Press,
    Release,
}

/// `@screenshot` 的最小请求形态。
///
/// v1 先只支持主显示器截图。
/// 参数先保持尽量少,确保 producer 能稳定接上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotRequest {
    pub quality: u8,
}

impl Default for ScreenshotRequest {
    fn default() -> Self {
        Self {
            quality: DEFAULT_SCREENSHOT_QUALITY,
        }
    }
}

/// 远程 PTY 会话打开请求。
///
/// `args` 是传给 `cmd` 的其余 argv,不再重复包含 `argv[0]`。
/// 这样对象协议只保留一个程序名真相源,看起来也更符合直觉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOpenRequest {
    pub cmd: String,
    pub args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

/// out-of-band PTY 关闭请求。
///
/// 这个请求不走 PTY stdin,避免 `~.` 这类 in-band escape 污染远端 TUI。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyCloseRequest {
    pub session_id: String,
}

/// out-of-band PTY detach 请求。
///
/// detach 只解绑当前 attached 控制端,不结束 PTY 进程。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyDetachRequest {
    pub session_id: String,
}

/// 重新接管一个 detached PTY session。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyAttachRequest {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

/// 一条结构化的 control 请求。
///
/// `request_id` 只作用于显式 `@...` 协议请求。
/// 普通 shell 行仍然按顺序流处理,不强行附会 id。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRequest {
    pub request_id: Option<u64>,
    pub command: ControlCommand,
}

/// 解析一行输入,决定它属于控制协议还是普通 shell 文本。
pub fn parse_control_line(line: &str) -> io::Result<ControlParseResult> {
    let line = line.trim_end_matches(['\r', '\n']);

    if let Some(escaped) = line.strip_prefix("@@") {
        return Ok(ControlParseResult::LiteralShellLine(format!("@{escaped}")));
    }

    if !line.starts_with('@') {
        return Ok(ControlParseResult::LiteralShellLine(line.to_owned()));
    }

    let command = line[1..].trim_start();
    let has_payload = command.contains(':');
    let (kind, request_id) = parse_control_header(command)?;

    if kind.eq_ignore_ascii_case("ping") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Ping,
        }));
    }

    if kind.eq_ignore_ascii_case("screenshot") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Screenshot(ScreenshotRequest::default()),
        }));
    }

    let Some((_, payload)) = command.split_once(':') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("无效控制指令: {line}"),
        ));
    };

    let payload = payload.trim();
    let control = match kind.trim().to_ascii_lowercase().as_str() {
        "key" => ControlCommand::Key(parse_key_payload(payload)?),
        "paste" => {
            let payload = parse_quoted_payload(payload)?;
            require_non_empty_payload("paste", payload, ControlCommand::Paste)?
        }
        "script" => {
            let payload = parse_quoted_payload(payload)?;
            require_non_empty_payload("script", payload, ControlCommand::Script)?
        }
        "cmd" => {
            let payload = parse_quoted_payload(payload)?;
            require_non_empty_payload("cmd", payload, ControlCommand::Script)?
        }
        "pty" => ControlCommand::PtyOpen(parse_pty_payload(payload)?),
        "pty-close" => ControlCommand::PtyClose(parse_pty_close_payload(payload)?),
        "pty-detach" => ControlCommand::PtyDetach(parse_pty_detach_payload(payload)?),
        "pty-attach" => ControlCommand::PtyAttach(parse_pty_attach_payload(payload)?),
        "screenshot" => ControlCommand::Screenshot(parse_screenshot_payload(payload)?),
        "savefile" => ControlCommand::SaveFile(SaveFileFrame::parse_object_payload(payload)?),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("不支持的控制指令类型: {}", kind.trim()),
            ))
        }
    };

    Ok(ControlParseResult::Control(ControlRequest {
        request_id,
        command: control,
    }))
}

fn parse_pty_payload(input: &str) -> io::Result<PtyOpenRequest> {
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

fn parse_pty_close_payload(input: &str) -> io::Result<PtyCloseRequest> {
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

fn parse_pty_detach_payload(input: &str) -> io::Result<PtyDetachRequest> {
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

fn parse_pty_attach_payload(input: &str) -> io::Result<PtyAttachRequest> {
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

fn object_inner<'a>(input: &'a str, kind: &str) -> io::Result<&'a str> {
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

fn parse_screenshot_payload(input: &str) -> io::Result<ScreenshotRequest> {
    let trimmed = input.trim();

    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot payload 当前必须是对象: {input}"),
        ));
    }

    let inner = trimmed
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@screenshot 对象 payload 必须使用大括号包裹: {input}"),
            )
        })?
        .trim();

    if inner.is_empty() {
        return Ok(ScreenshotRequest::default());
    }

    let mut quality = None::<u8>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "quality" => {
                if quality.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `quality` 字段重复",
                    ));
                }
                quality = Some(parse_screenshot_quality(raw_value)?);
            }
            "target" => {
                let target = parse_quoted_payload(raw_value)?;
                if !target.eq_ignore_ascii_case("display") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@screenshot 当前只支持 target=\"display\": {target}"),
                    ));
                }
            }
            "format" => {
                let format = parse_quoted_payload(raw_value)?;
                if !format.eq_ignore_ascii_case("jpeg") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@screenshot 当前只支持 format=\"jpeg\": {format}"),
                    ));
                }
            }
            "display" => {
                let display = parse_quoted_payload(raw_value)?;
                if !display.eq_ignore_ascii_case("primary") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@screenshot 当前只支持 display=\"primary\": {display}"),
                    ));
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@screenshot 对象 payload 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    Ok(ScreenshotRequest {
        quality: quality.unwrap_or(DEFAULT_SCREENSHOT_QUALITY),
    })
}

fn parse_screenshot_quality(input: &str) -> io::Result<u8> {
    let quality = input.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot 的 `quality` 必须是无符号整数: {input}"),
        )
    })?;

    if quality == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot 的 `quality` 必须在 1..=100 之间",
        ));
    }

    Ok(quality)
}

fn parse_key_payload(input: &str) -> io::Result<KeyRequest> {
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
    require_non_empty_payload("key", key, |key| KeyRequest {
        key,
        hold_ms: DEFAULT_KEY_HOLD_MS,
        mode: KeyMode::PressRelease,
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

    default_key_request(key).map(|defaulted| KeyRequest {
        key: defaulted.key,
        hold_ms: hold_ms.unwrap_or(DEFAULT_KEY_HOLD_MS),
        mode: mode.unwrap_or(KeyMode::PressRelease),
    })
}

fn split_object_fields(input: &str) -> io::Result<Vec<&str>> {
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

fn split_object_field(field: &str) -> io::Result<(&str, &str)> {
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

fn normalize_object_field_name(field_name: &str) -> io::Result<String> {
    let trimmed = field_name.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@key 对象字段名不能为空",
        ));
    }

    Ok(trimmed.trim_matches('"').to_ascii_lowercase())
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

fn parse_control_header(command: &str) -> io::Result<(&str, Option<u64>)> {
    let header = command
        .split_once(':')
        .map(|(header, _)| header)
        .unwrap_or(command)
        .trim();

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

fn require_non_empty_payload<T>(
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

fn parse_quoted_payload(input: &str) -> io::Result<String> {
    let bytes = input.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("控制指令 payload 必须使用双引号包裹: {input}"),
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
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("不支持的转义序列: \\{}", other as char),
                    ))
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
                    format!("控制指令 payload 后存在多余内容: {input}"),
                ));
            }
            other => result.push(other as char),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("未闭合的控制指令 payload: {input}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_should_route_plain_shell_lines_to_literal() {
        assert_eq!(
            parse_control_line("echo hi").unwrap(),
            ControlParseResult::LiteralShellLine("echo hi".to_owned())
        );
    }

    #[test]
    fn parse_should_unescape_double_at_to_literal_shell_line() {
        assert_eq!(
            parse_control_line("@@echo hi").unwrap(),
            ControlParseResult::LiteralShellLine("@echo hi".to_owned())
        );
    }

    #[test]
    fn parse_should_support_key_paste_script_cmd_and_screenshot() {
        assert_eq!(
            parse_control_line(r#"@key:"F11""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Key(KeyRequest {
                    key: "F11".to_owned(),
                    hold_ms: DEFAULT_KEY_HOLD_MS,
                    mode: KeyMode::PressRelease,
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@paste:"hello""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Paste("hello".to_owned()),
            })
        );
        assert_eq!(
            parse_control_line(r#"@script:"echo hi""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Script("echo hi".to_owned()),
            })
        );
        assert_eq!(
            parse_control_line(r#"@cmd:"echo hi""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Script("echo hi".to_owned()),
            })
        );
        assert_eq!(
            parse_control_line(
                r#"@savefile:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::SaveFile(SaveFileFrame {
                    request_id: None,
                    filename: "shot.jpg".to_owned(),
                    mime: "image/jpeg".to_owned(),
                    encoding: "base64".to_owned(),
                    data: "QUJD".to_owned(),
                    quality: None,
                    width: None,
                    height: None,
                }),
            })
        );
        assert_eq!(
            parse_control_line("@screenshot").unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Screenshot(ScreenshotRequest::default()),
            })
        );
        assert_eq!(
            parse_control_line(
                r#"@screenshot:{target:"display",display:"primary",format:"jpeg",quality:80}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Screenshot(ScreenshotRequest { quality: 80 }),
            })
        );
    }

    #[test]
    fn parse_should_support_pty_open_and_close_requests() {
        assert_eq!(
            parse_control_line(r#"@pty:"codex""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "codex".to_owned(),
                    args: vec![],
                    cols: 80,
                    rows: 24,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@pty:"codex resume 019e02de-8814-72a2-ab0c-b06263cc0fba""#)
                .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "codex".to_owned(),
                    args: vec![
                        "resume".to_owned(),
                        "019e02de-8814-72a2-ab0c-b06263cc0fba".to_owned()
                    ],
                    cols: 80,
                    rows: 24,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@pty:"/bin/sh -c 'printf hello world'""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), "printf hello world".to_owned()],
                    cols: 80,
                    rows: 24,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@pty:"/tmp/my\ helper --name \"fast mode\"""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "/tmp/my helper".to_owned(),
                    args: vec!["--name".to_owned(), "fast mode".to_owned()],
                    cols: 80,
                    rows: 24,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@pty:{cmd:"codex",args:["--profile","fast"],cols:120,rows:40}"#)
                .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "codex".to_owned(),
                    args: vec!["--profile".to_owned(), "fast".to_owned()],
                    cols: 120,
                    rows: 40,
                }),
            })
        );

        assert_eq!(
            parse_control_line(
                r#"@pty:{cmd:"codex",argv:["codex","--profile","fast"],cols:120,rows:40}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "codex".to_owned(),
                    args: vec!["--profile".to_owned(), "fast".to_owned()],
                    cols: 120,
                    rows: 40,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@pty-close:{session_id:"session-1"}"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyClose(PtyCloseRequest {
                    session_id: "session-1".to_owned(),
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@pty-detach:{session_id:"session-1"}"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyDetach(PtyDetachRequest {
                    session_id: "session-1".to_owned(),
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@pty-attach:"session-1""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyAttach(PtyAttachRequest {
                    session_id: "session-1".to_owned(),
                    cols: 80,
                    rows: 24,
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@pty-attach:{session_id:"session-1",cols:120,rows:40}"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyAttach(PtyAttachRequest {
                    session_id: "session-1".to_owned(),
                    cols: 120,
                    rows: 40,
                }),
            })
        );
    }

    #[test]
    fn parse_should_support_ping() {
        assert_eq!(
            parse_control_line("@ping").unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Ping,
            })
        );
    }

    #[test]
    fn parse_should_support_optional_request_ids() {
        assert_eq!(
            parse_control_line(r#"@ping#42"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(42),
                command: ControlCommand::Ping,
            })
        );
        assert_eq!(
            parse_control_line(r#"@key#7:"F11""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(7),
                command: ControlCommand::Key(KeyRequest {
                    key: "F11".to_owned(),
                    hold_ms: DEFAULT_KEY_HOLD_MS,
                    mode: KeyMode::PressRelease,
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@pty#9:"codex""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(9),
                command: ControlCommand::PtyOpen(PtyOpenRequest {
                    cmd: "codex".to_owned(),
                    args: vec![],
                    cols: 80,
                    rows: 24,
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@cmd#42:"printf READY""#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(42),
                command: ControlCommand::Script("printf READY".to_owned()),
            })
        );
        assert_eq!(
            parse_control_line(
                r#"@savefile#9:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#
            )
            .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(9),
                command: ControlCommand::SaveFile(SaveFileFrame {
                    request_id: None,
                    filename: "shot.jpg".to_owned(),
                    mime: "image/jpeg".to_owned(),
                    encoding: "base64".to_owned(),
                    data: "QUJD".to_owned(),
                    quality: None,
                    width: None,
                    height: None,
                }),
            })
        );
        assert_eq!(
            parse_control_line(r#"@screenshot#12"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(12),
                command: ControlCommand::Screenshot(ScreenshotRequest::default()),
            })
        );
    }

    #[test]
    fn parse_should_support_key_object_payloads() {
        assert_eq!(
            parse_control_line(r#"@key#7:{key:"right-option",hold_ms:200,mode:"press_release"}"#)
                .unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: Some(7),
                command: ControlCommand::Key(KeyRequest {
                    key: "right-option".to_owned(),
                    hold_ms: 200,
                    mode: KeyMode::PressRelease,
                }),
            })
        );

        assert_eq!(
            parse_control_line(r#"@key:{key:"right-option"}"#).unwrap(),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::Key(KeyRequest {
                    key: "right-option".to_owned(),
                    hold_ms: DEFAULT_KEY_HOLD_MS,
                    mode: KeyMode::PressRelease,
                }),
            })
        );
    }

    #[test]
    fn parse_should_reject_unknown_or_empty_or_multiline_payloads_or_bad_request_ids() {
        assert!(parse_control_line(r#"@unknown:"x""#).is_err());
        assert!(parse_control_line(r#"@key:"""#).is_err());
        assert!(parse_control_line("@script:\"printf a\\nb\"").is_err());
        assert!(parse_control_line(r#"@ping#:"x""#).is_err());
        assert!(parse_control_line(r#"@ping#abc"#).is_err());
        assert!(parse_control_line(r#"@ping#42:"x""#).is_err());
        assert!(parse_control_line(r#"@key:{hold_ms:200}"#).is_err());
        assert!(parse_control_line(r#"@key:{key:"x",hold_ms:"200"}"#).is_err());
        assert!(parse_control_line(r#"@key:{key:"x",mode:"tap"}"#).is_err());
        assert!(parse_control_line(r#"@key:{key:"x",unknown:1}"#).is_err());
        assert!(parse_control_line(r#"@pty:"""#).is_err());
        assert!(parse_control_line(r#"@pty:{cmd:"codex",args:[""]}"#).is_err());
        assert!(
            parse_control_line(r#"@pty:{cmd:"codex",args:["--a"],argv:["codex","--a"]}"#).is_err()
        );
        assert!(parse_control_line(r#"@pty:{cmd:"codex",argv:["other","--a"]}"#).is_err());
        assert!(parse_control_line(r#"@screenshot:{quality:0}"#).is_err());
        assert!(parse_control_line(r#"@screenshot:{format:"png"}"#).is_err());
        assert!(parse_control_line(r#"@screenshot:{display:"secondary"}"#).is_err());
    }
}
