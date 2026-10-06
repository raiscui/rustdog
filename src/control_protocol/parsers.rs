use std::io;

mod agent;
mod cancel_seq;
mod computer_act;
mod key;
mod open_app;
mod pty;
mod screenshot;
mod task;
mod wait;

pub(super) use self::agent::{parse_agent_ack_payload, parse_agent_name_payload};
pub(super) use self::cancel_seq::parse_cancel_payload;
pub(super) use self::computer_act::parse_computer_act_payload;
pub(super) use self::key::parse_key_payload;
pub(super) use self::open_app::parse_open_app_payload;
pub(super) use self::pty::{
    parse_pty_attach_payload, parse_pty_close_payload, parse_pty_detach_payload, parse_pty_payload,
};
pub(super) use self::screenshot::parse_screenshot_payload;
pub(super) use self::task::{
    parse_spawn_payload, parse_task_id_payload, parse_task_output_payload,
};
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
    "root",
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
    pub(crate) fn take_positional(
        &mut self,
        _kind: &str,
        _what: &str,
    ) -> io::Result<Option<String>> {
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
                format!(
                    "{kind} 短格式存在无法识别的多余字段: {}",
                    leftovers.join(", ")
                ),
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

/// quoted payload 的两处放宽维度: 闭合引号之后的尾巴, 以及未知反斜杠转义。
///
/// 这两维必须绑在一个枚举里而不是两个布尔开关 —— 它们是同一个契约的两种形态:
/// 「这条 payload 是不是 shell 文本」。是 shell 文本就整体放宽, 不是就维持严格
/// 契约, 中间态(尾巴放宽但转义收紧, 或反之)没有对应的真实场景, 只会让调用方
/// 以为自己拿到了 shell 语义却仍被反斜杠拦下。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotedPayloadPolicy {
    /// 严格契约: 闭合引号后必须整行结束, 未知转义序列报错。
    /// 绝大多数控制命令 (窗口标题 / 按键名 / 目标选择器) 的既有契约。
    Strict,
    /// `@cmd` 的 shell 文本契约:
    /// - 闭合引号后允许继续有内容, 剩余原文原样拼在引号内容之后
    ///   (`"X=" + (Get-Item ...)` 是合法写法, 远程 Windows 真机 2026-10-05 实战:
    ///   `@cmd:"X=" + ...` 直接吃到 code 64);
    /// - 只有 `\"` 是转义, 其余 `\` 一律按字面反斜杠还原 (见
    ///   `parse_quoted_payload_with` 的转义分支)。
    ShellText,
}

pub(crate) fn parse_quoted_payload(input: &str) -> io::Result<String> {
    parse_quoted_payload_with(input, QuotedPayloadPolicy::Strict)
}

fn parse_quoted_payload_with(input: &str, policy: QuotedPayloadPolicy) -> io::Result<String> {
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
            // 两种契约对"反斜杠意味着什么"的判断根本不同, 所以先按契约算出
            // 该落成哪个字符, 算不出来再按契约决定是报错还是按字面还原。
            //
            // 关键分歧在 `\n` / `\r` / `\t` / `\\`: 严格契约把它们当转义, 而
            // shell 文本里它们是路径的日常形态 —— `\notes.txt` 的 `\n`、
            // `\temp` 的 `\t`、UNC 的 `\\server\share` 的 `\\`。按转义处理会把
            // 一条合法 Windows 路径改写成带换行符的垃圾, 且协议层无从察觉。
            let unescaped = match (policy, ch) {
                (_, '"') => Some('"'),
                (QuotedPayloadPolicy::Strict, '\\') => Some('\\'),
                (QuotedPayloadPolicy::Strict, 'n') => Some('\n'),
                (QuotedPayloadPolicy::Strict, 'r') => Some('\r'),
                (QuotedPayloadPolicy::Strict, 't') => Some('\t'),
                _ => None,
            };

            match unescaped {
                Some(character) => result.push(character),
                None => match policy {
                    // 严格契约: 反斜杠只可能是转义符, 认不出就报错。
                    QuotedPayloadPolicy::Strict => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("不支持的转义序列: \\{ch}"),
                        ))
                    }
                    // shell 文本契约: 只有 `\"` 走转义通道 (协议引号必须能出现在
                    // payload 内部), 其余一律还原成字面 `\` + 原字符。这样
                    // Windows 路径能逐字节穿过协议层, 而不是被判成"未支持的
                    // 转义"或被偷偷改写。
                    QuotedPayloadPolicy::ShellText => {
                        result.push('\\');
                        result.push(ch);
                    }
                },
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => {
                // 尾部不 trim 就判空: KeepAsRawText 要把空白原样保留,
                // 尾随空白是调用方 shell 文本的分隔依据。
                if input[index + 1..].trim().is_empty() {
                    return Ok(result);
                }

                return match policy {
                    QuotedPayloadPolicy::Strict => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("控制指令 payload 后存在多余内容: {input}"),
                    )),
                    // 剩余原文直接拼上去, 不注入任何分隔符 —— `;` `|` `+`
                    // 这些连接符属于调用方的 shell 文本, 我们插一个空格进去
                    // 就等于替用户改写了命令。
                    QuotedPayloadPolicy::ShellText => {
                        result.push_str(&input[index + 1..]);
                        Ok(result)
                    }
                };
            }
            other => result.push(other),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("未闭合的控制指令 payload: {input}"),
    ))
}

/// 解析显式 `@cmd` 的 shell 文本。
///
/// `@cmd` 是既有裸 shell lane 的 request-id 入口,因此允许非对象的单行 raw
/// payload;其它控制命令仍继续使用严格的 quoted/object 契约。
pub(super) fn parse_cmd_payload(input: &str) -> io::Result<String> {
    let trimmed = input.trim();

    if trimmed.starts_with('"') {
        let payload = parse_quoted_payload_with(trimmed, QuotedPayloadPolicy::ShellText)?;
        // 剥掉调用方自己加的引号之后还要再判一次: `@cmd:"C:\a b\x.exe" --silent`
        // 还原出来仍然是劈开的裸路径, 放过去只会把报错换成 shell 那句更难查的
        // CommandNotFoundException。
        return reject_split_windows_path(&payload);
    }

    let payload = reject_split_windows_path(trimmed)?;

    if trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "@cmd 对象 payload 还没有承接的字段: `@cmd` 的执行面只带一段单行 shell 文本 \
                 (ControlCommand::Script 是 String 载荷), 选 shell / 设超时 / 写临时脚本都没有落点。\
                 当前支持 raw 或 quoted 两种形式; 长脚本先用 `@put-file` + `@file-chunk` + \
                 `@file-end` 传成被控端上的 .ps1, 再 `@cmd:& \"<脚本路径>\"` 执行。payload: {input}"
            ),
        ));
    }

    if trimmed.is_empty() || input.contains(['\r', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@cmd payload 必须是非空、单行的 quoted 或 raw shell 文本: {input}"),
        ));
    }

    Ok(payload)
}

fn reject_split_windows_path(payload: &str) -> io::Result<String> {
    match windows_unquoted_space_path_hint(payload) {
        Some(hint) => Err(io::Error::new(io::ErrorKind::InvalidData, hint)),
        None => Ok(payload.to_owned()),
    }
}

/// Windows 解释器把 payload 按空白拆 token, 带空格的裸路径必然被劈成两段:
/// `C:\Users\x\Uninstall Yino.exe` 会被当成 `C:\Users\x\Uninstall` 这个命令去找,
/// 报出来的是 `CommandNotFoundException` —— 与真实病因 (路径写法) 毫无关联。
///
/// 判据只问一件事: **可执行路径到达后缀之前有没有跨过空白**。payload 以盘符开头,
/// 且第一个可执行后缀之前就已经出现空格 —— 这条命令在 bash/zsh/sh/pwsh/cmd 下
/// 都不可能跑通, 所以在协议层拦下不是"解释器语义检查", 而是拦下一条必然失败
/// 的命令。路径后面跟多少参数都拦得住, 因为空格在参数之前就已经出现了。
fn windows_unquoted_space_path_hint(payload: &str) -> Option<String> {
    let first = payload.split_whitespace().next()?;
    if !starts_with_windows_drive(first) {
        return None;
    }

    let (suffix_start, suffix_end) = first_windows_executable_suffix_span(payload)?;
    if !payload[..suffix_start].contains(' ') {
        return None;
    }

    let path = payload[..suffix_end].trim_end();
    Some(format!(
        "payload 里的 Windows 可执行路径含空格却没有加引号, 任何 shell 都会把 \
         `{path}` 的第一段当命令名去找: PowerShell 下写 `& \"{path}\" args` \
         (调用运算符), cmd.exe 下写 `\"\"{path}\"\" args` (整体双引号); \
         rdog 只提示不替你改写命令。payload: {payload}"
    ))
}

fn starts_with_windows_drive(token: &str) -> bool {
    // 盘符一定是 ASCII, 所以按字节判断既安全又比 char 迭代直白。
    let bytes = token.as_bytes();
    bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

/// 找出文本里最早出现的可执行后缀, 返回 (后缀起点, 后缀终点) 字节下标。
///
/// `.cmd` / `.ps1` 与 `.exe` 在 Windows 上是同一类坑 (裸 token 规则一致), 漏掉它们
/// 会让使用者以为规则只对可执行文件生效。
fn first_windows_executable_suffix_span(text: &str) -> Option<(usize, usize)> {
    const EXECUTABLE_SUFFIXES: [&str; 4] = [".exe", ".ps1", ".bat", ".cmd"];
    // 只折 ASCII 大写, 字节长度不变, 所以在 folded 上的下标可直接用于 text。
    let folded = text.to_ascii_lowercase();
    EXECUTABLE_SUFFIXES
        .iter()
        .filter_map(|suffix| folded.find(*suffix).map(|start| (start, suffix)))
        .min_by_key(|(start, _)| *start)
        .map(|(start, suffix)| (start, start + suffix.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------
    // @cmd quoted 尾巴宽容 (quoted 前缀 + 剩余原文)
    // ------------------------------------------------------------

    #[test]
    fn cmd_quoted_payload_should_keep_text_after_closing_quote() {
        // 远程 Windows 实战踩到的写法: 字符串字面量后面还跟着表达式。
        let payload = parse_cmd_payload(r#""X=" + (Get-Item C:\Windows\System32)"#).unwrap();
        assert_eq!(payload, r#"X= + (Get-Item C:\Windows\System32)"#);
    }

    #[test]
    fn cmd_quoted_payload_should_not_inject_separator_into_tail() {
        // 分隔符属于调用方的 shell 文本, 我们插空格就等于替用户改写命令。
        let payload = parse_cmd_payload(r#""a";"b""#).unwrap();
        assert_eq!(payload, r#"a;"b""#);
    }

    #[test]
    fn cmd_quoted_payload_should_still_unescape_inside_quotes() {
        let payload = parse_cmd_payload(r#""a\"b" ; ls"#).unwrap();
        assert_eq!(payload, r#"a"b ; ls"#);
    }

    #[test]
    fn cmd_quoted_payload_without_tail_should_stay_strict() {
        assert_eq!(parse_cmd_payload(r#""echo hi""#).unwrap(), "echo hi");
    }

    #[test]
    fn cmd_quoted_payload_should_still_reject_unclosed_quote() {
        let err = parse_cmd_payload(r#""echo hi"#).unwrap_err();
        assert!(err.to_string().contains("未闭合"));
    }

    #[test]
    fn strict_quoted_payload_should_still_reject_trailing_content() {
        // 非 @cmd 的命令保持原有严格契约: 共用扫描核心不代表契约一起放宽。
        let err = parse_quoted_payload(r#""a" junk"#).unwrap_err();
        assert!(err.to_string().contains("多余内容"));
    }

    // ------------------------------------------------------------
    // @cmd raw / 对象 / 多行
    // ------------------------------------------------------------

    #[test]
    fn cmd_raw_payload_should_pass_through() {
        assert_eq!(
            parse_cmd_payload("Get-Process | Where-Object Name -eq rdog").unwrap(),
            "Get-Process | Where-Object Name -eq rdog"
        );
    }

    #[test]
    fn cmd_object_payload_should_report_missing_wiring() {
        let err = parse_cmd_payload(r#"{"command":"ls","shell":"powershell"}"#).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("还没有承接的字段"));
        // 长脚本的真解法要指向已存在的上传 lane, 不能只说"不支持"。
        assert!(message.contains("@put-file"));
    }

    #[test]
    fn cmd_payload_should_still_reject_multiline() {
        // 控制协议是行式的, 换行必然被当成新行, 这里必须继续拒绝。
        let err = parse_cmd_payload("echo a\necho b").unwrap_err();
        assert!(err.to_string().contains("单行"));
    }

    // ------------------------------------------------------------
    // Windows 路径提示
    // ------------------------------------------------------------

    #[test]
    fn cmd_windows_path_with_spaces_should_hint_both_shells() {
        let err = parse_cmd_payload(r"C:\Users\vdcoo\yino-test\Uninstall Yino.exe").unwrap_err();
        let message = err.to_string();
        assert!(message.contains(r#"& "C:\Users\vdcoo\yino-test\Uninstall Yino.exe""#));
        assert!(message.contains(r#"""C:\Users\vdcoo\yino-test\Uninstall Yino.exe""#));
    }

    #[test]
    fn cmd_windows_path_with_spaces_before_flag_should_hint() {
        let err = parse_cmd_payload(r"C:\Program Files\Yino\uninstall.exe --silent").unwrap_err();
        assert!(err.to_string().contains("调用运算符"));
    }

    #[test]
    fn cmd_windows_path_without_spaces_should_pass() {
        // 单 token 裸路径在 PowerShell / cmd 下本来就能跑, 拦下就是假阳性。
        assert_eq!(
            parse_cmd_payload(r"C:\Windows\System32\sc.exe query").unwrap(),
            r"C:\Windows\System32\sc.exe query"
        );
    }

    #[test]
    fn cmd_windows_path_with_plain_args_should_still_hint() {
        // 空格出现在路径里, 参数有几个都不改变结局: PowerShell 只会去找
        // `C:\Program` 这个命令。
        let err = parse_cmd_payload(r"C:\Program Files\Yino\app.exe run local").unwrap_err();
        assert!(err
            .to_string()
            .contains(r#"& "C:\Program Files\Yino\app.exe" args"#));
    }

    #[test]
    fn cmd_windows_path_without_spaces_and_with_args_should_pass() {
        assert_eq!(
            parse_cmd_payload(r"C:\Windows\System32\sc.exe query TermService").unwrap(),
            r"C:\Windows\System32\sc.exe query TermService"
        );
    }

    #[test]
    fn cmd_non_windows_shell_text_should_never_hint() {
        assert_eq!(
            parse_cmd_payload("cargo build --release").unwrap(),
            "cargo build --release"
        );
        assert_eq!(
            parse_cmd_payload("Write-Output hello world").unwrap(),
            "Write-Output hello world"
        );
    }

    #[test]
    fn cmd_quoted_windows_path_should_be_rechecked_after_unescaping() {
        // 引号是 rdog 自己剥掉的, 剥完还是劈开的裸路径就必须在这里拦下。
        let err = parse_cmd_payload(r#""C:\Program Files\Yino\uninstall.exe" --silent"#)
            .unwrap_err();
        assert!(err.to_string().contains("调用运算符"));
    }

    #[test]
    fn cmd_quoted_windows_path_with_args_should_pass() {
        // 单 token 路径 + flag: 剥掉引号后本来就能跑, 不该误伤。
        assert_eq!(
            parse_cmd_payload(r#""C:\Yino\uninstall.exe" --silent"#).unwrap(),
            r"C:\Yino\uninstall.exe --silent"
        );
    }

    #[test]
    fn cmd_quoted_string_constant_should_pass() {
        // 字符串常量: 首个 token 不是盘符, 或者路径不是可执行后缀, 判据都不该命中。
        assert_eq!(
            parse_cmd_payload(r#"Write-Output "C:\Program Files\Yino\uninstall.exe""#).unwrap(),
            r#"Write-Output "C:\Program Files\Yino\uninstall.exe""#
        );
        assert_eq!(
            parse_cmd_payload(r#""C:\Program Files\Yino\notes.txt""#).unwrap(),
            r"C:\Program Files\Yino\notes.txt"
        );
    }
}
