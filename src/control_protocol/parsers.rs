use std::io;

mod cancel_seq;
mod computer_act;
mod key;
mod open_app;
mod pty;
mod screenshot;
mod wait;

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

    // 2026-08-03 (评测回归): 模型常把对象语法的 `@window:N` 后缀塞进 compact
    // 语法 (例如 `app:Calculator,AXButton@window:0`), 旧行为把它原样当作
    // role/description 的一部分, 静默 0 匹配且无错误提示, 模型无法自纠。
    // 这里显式拒绝, 并提示正确写法 (窗口选择走 selector 前缀或对象语法)。
    if value.contains("@window:") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{kind} 短格式不支持 `@window:` 后缀: {input}; 请用 `app:APP,ROLE` 或 `pid:PID/window:N,ROLE` 选择窗口"
            ),
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

/// compact 短格式的已知字段前缀(前缀路由模型)。
///
/// LLM 会把对象语法的字段名带进 compact 短格式, 例如
/// `@ax-find:app:Safari,role:AXTextField` 或
/// `@ax-find:app:Safari,AXStaticText,include_values:true,limit:10`。
/// parser 按前缀把值路由到正确的字段槽位, 而不是把 `role:AXTextField`
/// 当作字面角色名 (旧行为会静默 0 匹配, 模型无法自纠)。
///
/// 这是 rdog 兼容 LLM 多样化写法的核心机制 (AI-native 优先)。
const KNOWN_COMPACT_PREFIXES: &[&str] = &[
    "app",
    "pid",
    "role",
    "description",
    "value",
    "name",
    "include_values",
    "limit",
    "depth",
    "max_elements",
    "mode",
    "expected_value",
    "max_attempts",
];

/// 解析后的 compact 字段集合: 位置字段 + 命名字段(前缀路由)。
///
/// - `positional`: 无前缀的裸值, 按位置回退到各命令的槽位
///   (第 1 个通常是窗口选择器, 第 2 个是主值)。
/// - `named`: 带已知前缀的字段, 按名路由到对应槽位。
///   保持出现顺序, 重复前缀由调用方检测报错。
#[derive(Debug, Default)]
pub(crate) struct ParsedCompactFields {
    pub positional: Vec<String>,
    pub named: Vec<(String, String)>,
}

/// 解析逗号分隔的 compact 字段列表, 识别已知前缀并拒绝未知前缀。
pub(crate) fn parse_compact_fields(kind: &str, input: &str) -> io::Result<ParsedCompactFields> {
    let mut fields = ParsedCompactFields::default();
    // 单个尾部逗号兼容旧 parse_compact_ax_button_sequence 行为;
    // 多个尾部逗号或中间空字段仍然报错。
    let fields_input = input.strip_suffix(',').unwrap_or(input);
    for raw in fields_input.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{kind} 短格式字段不能为空: {input}"),
            ));
        }
        match raw.split_once(':') {
            Some((name, value)) if KNOWN_COMPACT_PREFIXES.contains(&name) => {
                // 兼容带引号值: `app:"Terminal"` / `role:"AXButton"`。
                // compact 值本不允许引号, 剥掉外层引号是 LLM 写法的宽容处理。
                let value = value.trim();
                let value = value
                    .strip_prefix('"')
                    .and_then(|inner| inner.strip_suffix('"'))
                    .unwrap_or(value);
                let value = parse_compact_atom(kind, value)?;
                fields.named.push((name.to_owned(), value));
            }
            Some((name, _)) => {
                if raw.contains("@window:") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{kind} 短格式不支持 `@window:` 后缀: {input}; 请用 `app:APP,ROLE` 或 `pid:PID/window:N,ROLE` 选择窗口"
                        ),
                    ));
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{kind} 短格式未知字段前缀 `{name}:`; 支持的前缀: {}; 无前缀字段按位置解析: {input}",
                        KNOWN_COMPACT_PREFIXES.join(" / ")
                    ),
                ));
            }
            None => {
                let value = parse_compact_atom(kind, raw)?;
                fields.positional.push(value);
            }
        }
    }
    Ok(fields)
}

impl ParsedCompactFields {
    /// 按名取一个字段值; 重复出现报错 (模型二选一)。
    pub(crate) fn take_named(&mut self, kind: &str, name: &str) -> io::Result<Option<String>> {
        let mut found = None::<String>;
        let mut duplicate = false;
        self.named.retain(|(n, value)| {
            if n == name {
                if found.is_some() {
                    duplicate = true;
                }
                found = Some(value.clone());
                false
            } else {
                true
            }
        });
        if duplicate {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{kind} 短格式 `{name}:` 字段重复, 请只写一次"),
            ));
        }
        Ok(found)
    }

    /// 按位置顺序取一个无前缀字段值。
    pub(crate) fn take_positional(&mut self, kind: &str, what: &str) -> io::Result<Option<String>> {
        if self.positional.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.positional.remove(0)))
    }

    /// 命名优先取字段; 命名与位置同槽位都提供时 -> 冲突报错。
    ///
    /// 注意不能写成 `take_named()?.or(take_positional()?)`:
    /// `Option::or` 的参数是 eager 求值, 会静默消费位置字段,
    /// 导致 `app:APP,AXButton,role:AXStaticText` 这类冲突被吞掉。
    pub(crate) fn take_named_or_positional(
        &mut self,
        kind: &str,
        name: &str,
        slot: &str,
    ) -> io::Result<Option<String>> {
        match self.take_named(kind, name)? {
            Some(named) => {
                if self.take_positional(kind, slot)?.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{kind} 短格式 `{slot}` 字段冲突: 位置字段与 `{name}:` 同时提供, 请只写一种"
                        ),
                    ));
                }
                Ok(Some(named))
            }
            None => self.take_positional(kind, slot),
        }
    }

    /// 剩余未消费字段(位置或命名)统一报错, 提示字段放错位置。
    pub(crate) fn ensure_empty(&self, kind: &str) -> io::Result<()> {
        if !self.positional.is_empty() || !self.named.is_empty() {
            let mut leftovers = self.positional.clone();
            leftovers.extend(self.named.iter().map(|(n, v)| format!("{n}:{v}")));
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{kind} 短格式存在无法识别的多余字段: {}", leftovers.join(", ")),
            ));
        }
        Ok(())
    }
}

/// 从 compact 字段中解析窗口选择器。
///
/// 支持三种来源(互斥, 重复报错):
/// - 命名 `app:APP`
/// - 命名 `pid:PID/window:INDEX`
/// - 位置第 1 个无前缀字段 (兼容旧写法, 如 `pid:123/window:0,AXButton`)
pub(crate) fn resolve_compact_selector(
    kind: &str,
    fields: &mut ParsedCompactFields,
) -> io::Result<CompactWindowSelector> {
    let named_app = fields.take_named(kind, "app")?;
    let named_pid = fields.take_named(kind, "pid")?;
    match (named_app, named_pid) {
        (Some(app), None) => {
            if app.is_empty() {
                return Err(invalid_compact_window_id(kind, &format!("app:{app}")));
            }
            if !app.is_ascii() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{kind} 短格式 app:APP 必须是 ASCII 名称(macOS Launch Services 不支持非 ASCII);received: {app};请改用 Launch Services 英文名称"
                    ),
                ));
            }
            Ok(CompactWindowSelector::App(app))
        }
        (None, Some(pid)) => {
            let window_id = parse_compact_window_id(kind, &format!("pid:{pid}"))?;
            Ok(CompactWindowSelector::WindowId(window_id))
        }
        (Some(_), Some(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 短格式窗口选择器冲突: `app:` 与 `pid:` 只能写一个"),
        )),
        (None, None) => {
            // 无命名 selector 时, 位置第 1 个字段按旧语义尝试解析
            // (如 `pid:123/window:0,AXButton` 的位置式写法)。
            match fields.take_positional(kind, "window selector")? {
                Some(selector) => parse_compact_window_selector(kind, &selector),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{kind} 短格式缺少窗口选择器 (app:APP 或 pid:PID/window:INDEX)"),
                )),
            }
        }
    }
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
