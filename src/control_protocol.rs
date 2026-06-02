use std::io;

mod parsers;

pub(crate) use self::parsers::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};

use self::parsers::{
    parse_control_header, parse_key_payload, parse_pty_attach_payload, parse_pty_close_payload,
    parse_pty_detach_payload, parse_pty_payload, parse_screenshot_payload,
    require_non_empty_payload,
};

use crate::control_ax::{
    parse_ax_action_payload, parse_ax_find_payload, parse_ax_focus_payload, parse_ax_get_payload,
    parse_ax_press_payload, parse_ax_scroll_payload, parse_ax_set_value_payload,
    parse_ax_tree_payload, parse_type_text_payload, AxActionRequest, AxFindRequest, AxFocusRequest,
    AxGetRequest, AxMode, AxPressRequest, AxScrollRequest, AxSetValueRequest, AxTreeRequest,
    TypeTextRequest, DEFAULT_AX_DEPTH, DEFAULT_AX_INCLUDE_VALUES, DEFAULT_AX_MAX_ELEMENTS,
};
use crate::control_bootstrap::{parse_bootstrap_payload, BootstrapRequest};
use crate::control_frames::SaveFileFrame;
use crate::control_gui_bench::{parse_gui_bench_payload, GuiBenchRequest};
use crate::control_mouse::{
    parse_click_payload, parse_drag_payload, parse_mouse_button_payload, parse_mouse_move_payload,
    parse_wheel_payload, ClickRequest, DragRequest, MouseButtonRequest, MouseMoveRequest,
    WheelRequest,
};
use crate::control_observation::{
    parse_observe_payload, ObserveRequest, SelectorGetRequest, SelectorRefindPolicy,
    SelectorRefindRequest, SelectorRefindSource, SelectorResolveRequest,
};
use crate::control_web::{
    parse_web_act_payload, parse_web_find_payload, WebActRequest, WebFindRequest,
};
use crate::control_window::{
    parse_window_activate_payload, parse_window_close_payload, parse_window_find_payload,
    WindowActivateRequest, WindowCloseRequest, WindowFindRequest,
};

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
    Paste(PasteRequest),
    Script(String),
    PtyOpen(PtyOpenRequest),
    PtyClose(PtyCloseRequest),
    PtyDetach(PtyDetachRequest),
    PtyAttach(PtyAttachRequest),
    Screenshot(ScreenshotRequest),
    MouseMove(MouseMoveRequest),
    MouseButton(MouseButtonRequest),
    Click(ClickRequest),
    Drag(DragRequest),
    Wheel(WheelRequest),
    AxTree(AxTreeRequest),
    AxFind(AxFindRequest),
    AxGet(AxGetRequest),
    AxFocus(AxFocusRequest),
    AxScroll(AxScrollRequest),
    AxAction(AxActionRequest),
    AxPress(AxPressRequest),
    AxSetValue(AxSetValueRequest),
    TypeText(TypeTextRequest),
    WindowFind(WindowFindRequest),
    WindowActivate(WindowActivateRequest),
    WindowClose(WindowCloseRequest),
    WebFind(WebFindRequest),
    WebAct(WebActRequest),
    GuiBench(GuiBenchRequest),
    Bootstrap(BootstrapRequest),
    Capabilities,
    Observe(ObserveRequest),
    SelectorGet(SelectorGetRequest),
    SelectorResolve(SelectorResolveRequest),
    SelectorRefind(SelectorRefindRequest),
    SaveFile(SaveFileFrame),
}

pub const DEFAULT_KEY_HOLD_MS: u64 = 200;
pub const DEFAULT_SCREENSHOT_QUALITY: u8 = 75;

/// `@paste` 的结构化请求。
///
/// 当前语义分两条线:
/// - 裸 `@paste` / `@paste#id`: 当前远端前台焦点的热键粘贴
/// - `@paste:"text"`: legacy 文本注入兼容层,不建议作为普通文本输入路径
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteRequest {
    pub kind: PasteRequestKind,
}

impl PasteRequest {
    pub fn hotkey() -> Self {
        Self {
            kind: PasteRequestKind::GlobalHotkey,
        }
    }

    pub fn legacy_text(text: impl Into<String>) -> Self {
        Self {
            kind: PasteRequestKind::LegacyTextInjection(text.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteRequestKind {
    GlobalHotkey,
    LegacyTextInjection(String),
}

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
    pub delivery: KeyDelivery,
    pub pid: Option<i32>,
    pub window_id: Option<String>,
    pub response_mode: KeyResponseMode,
}

impl KeyRequest {
    pub fn legacy(key: impl Into<String>, hold_ms: u64, mode: KeyMode) -> Self {
        Self {
            key: key.into(),
            hold_ms,
            mode,
            delivery: KeyDelivery::Global,
            pid: None,
            window_id: None,
            response_mode: KeyResponseMode::Legacy,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyMode {
    PressRelease,
    Press,
    Release,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyDelivery {
    Global,
    PidTargeted,
    WindowTargeted,
}

impl KeyDelivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::PidTargeted => "pid-targeted",
            Self::WindowTargeted => "window-targeted",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyResponseMode {
    Legacy,
    Structured,
}

/// `@screenshot` 的结构化请求。
///
/// 默认走完整虚拟桌面截图,这样截图证据和后续鼠标坐标能共享同一套
/// `os-logical` 坐标语义。显式 `display:"primary"` 保留为兼容入口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotRequest {
    pub target: ScreenshotTarget,
    pub display: ScreenshotDisplaySelector,
    pub layout: ScreenshotLayout,
    pub coordinate_space: ScreenshotCoordinateSpace,
    pub quality: u8,
    pub include_ax: bool,
    pub ax_required: bool,
    pub ax_mode: AxMode,
    pub ax_depth: u8,
    pub ax_max_elements: u16,
    pub ax_include_values: bool,
}

impl Default for ScreenshotRequest {
    fn default() -> Self {
        Self {
            target: ScreenshotTarget::Display,
            display: ScreenshotDisplaySelector::All,
            layout: ScreenshotLayout::Composite,
            coordinate_space: ScreenshotCoordinateSpace::OsLogical,
            quality: DEFAULT_SCREENSHOT_QUALITY,
            include_ax: false,
            ax_required: false,
            ax_mode: AxMode::Full,
            ax_depth: DEFAULT_AX_DEPTH,
            ax_max_elements: DEFAULT_AX_MAX_ELEMENTS,
            ax_include_values: DEFAULT_AX_INCLUDE_VALUES,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScreenshotTarget {
    Display,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScreenshotDisplaySelector {
    All,
    Primary,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScreenshotLayout {
    Composite,
    Single,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ScreenshotCoordinateSpace {
    OsLogical,
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

    if kind.eq_ignore_ascii_case("paste") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Paste(PasteRequest::hotkey()),
        }));
    }

    if kind.eq_ignore_ascii_case("capabilities") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Capabilities,
        }));
    }

    if kind.eq_ignore_ascii_case("bootstrap") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Bootstrap(BootstrapRequest::default()),
        }));
    }

    if kind.eq_ignore_ascii_case("observe") && !has_payload {
        return Ok(ControlParseResult::Control(ControlRequest {
            request_id,
            command: ControlCommand::Observe(ObserveRequest::default()),
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
            require_non_empty_payload("paste", payload, |payload| {
                ControlCommand::Paste(PasteRequest::legacy_text(payload))
            })?
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
        "mouse-move" => ControlCommand::MouseMove(parse_mouse_move_payload(payload)?),
        "mouse-button" => ControlCommand::MouseButton(parse_mouse_button_payload(payload)?),
        "click" => ControlCommand::Click(parse_click_payload(payload)?),
        "drag" => ControlCommand::Drag(parse_drag_payload(payload)?),
        "wheel" => ControlCommand::Wheel(parse_wheel_payload(payload)?),
        "ax-tree" => ControlCommand::AxTree(parse_ax_tree_payload(payload)?),
        "ax-find" => ControlCommand::AxFind(parse_ax_find_payload(payload)?),
        "ax-get" => ControlCommand::AxGet(parse_ax_get_payload(payload)?),
        "ax-focus" => ControlCommand::AxFocus(parse_ax_focus_payload(payload)?),
        "ax-scroll" => ControlCommand::AxScroll(parse_ax_scroll_payload(payload)?),
        "ax-action" => ControlCommand::AxAction(parse_ax_action_payload(payload)?),
        "ax-press" => ControlCommand::AxPress(parse_ax_press_payload(payload)?),
        "ax-set-value" => ControlCommand::AxSetValue(parse_ax_set_value_payload(payload)?),
        "type-text" => ControlCommand::TypeText(parse_type_text_payload(payload)?),
        "window-find" => ControlCommand::WindowFind(parse_window_find_payload(payload)?),
        "window-activate" => {
            ControlCommand::WindowActivate(parse_window_activate_payload(payload)?)
        }
        "window-close" => ControlCommand::WindowClose(parse_window_close_payload(payload)?),
        "web-find" => ControlCommand::WebFind(parse_web_find_payload(payload)?),
        "web-act" => ControlCommand::WebAct(parse_web_act_payload(payload)?),
        "gui-bench" => ControlCommand::GuiBench(parse_gui_bench_payload(payload)?),
        "bootstrap" => ControlCommand::Bootstrap(parse_bootstrap_payload(payload)?),
        "observe" => ControlCommand::Observe(parse_observe_payload(payload)?),
        "selector-get" => ControlCommand::SelectorGet(parse_selector_get_payload(payload)?),
        "selector-resolve" => {
            ControlCommand::SelectorResolve(parse_selector_resolve_payload(payload)?)
        }
        "selector-refind" => {
            ControlCommand::SelectorRefind(parse_selector_refind_payload(payload)?)
        }
        "capabilities" => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@capabilities 不接受 payload,请直接发送 @capabilities 或 @capabilities#id",
            ))
        }
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

fn parse_selector_get_payload(input: &str) -> io::Result<SelectorGetRequest> {
    let inner = object_inner(input, "@selector-get")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-get 对象 payload 不能为空",
        ));
    }

    let mut selector_id = None::<String>;
    let mut include_history = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        match field_name.as_str() {
            "selector_id" => assign_selector_field(
                &mut selector_id,
                "selector_id",
                "@selector-get",
                parse_quoted_payload(raw_value.trim())?,
            )?,
            "include_history" => assign_selector_field(
                &mut include_history,
                "include_history",
                "@selector-get",
                parse_selector_bool("@selector-get", "include_history", raw_value.trim())?,
            )?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@selector-get 不支持字段: {field_name}"),
                ));
            }
        }
    }

    Ok(SelectorGetRequest {
        selector_id: require_selector_string("@selector-get.selector_id", selector_id)?,
        include_history: include_history.unwrap_or(false),
    })
}

fn parse_selector_resolve_payload(input: &str) -> io::Result<SelectorResolveRequest> {
    let inner = object_inner(input, "@selector-resolve")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-resolve 对象 payload 不能为空",
        ));
    }

    let mut selector_id = None::<String>;
    let mut limit = None::<u16>;
    let mut dry_run = None::<bool>;
    let mut include_explanations = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "selector_id" => assign_selector_field(
                &mut selector_id,
                "selector_id",
                "@selector-resolve",
                parse_quoted_payload(raw_value)?,
            )?,
            "limit" => assign_selector_field(
                &mut limit,
                "limit",
                "@selector-resolve",
                parse_selector_limit("@selector-resolve", raw_value)?,
            )?,
            "dry_run" => assign_selector_field(
                &mut dry_run,
                "dry_run",
                "@selector-resolve",
                parse_selector_bool("@selector-resolve", "dry_run", raw_value)?,
            )?,
            "include_explanations" => assign_selector_field(
                &mut include_explanations,
                "include_explanations",
                "@selector-resolve",
                parse_selector_bool("@selector-resolve", "include_explanations", raw_value)?,
            )?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@selector-resolve 不支持字段: {field_name}"),
                ));
            }
        }
    }

    Ok(SelectorResolveRequest {
        selector_id: require_selector_string("@selector-resolve.selector_id", selector_id)?,
        limit: limit.unwrap_or(10),
        dry_run: dry_run.unwrap_or(true),
        include_explanations: include_explanations.unwrap_or(true),
    })
}

fn parse_selector_refind_payload(input: &str) -> io::Result<SelectorRefindRequest> {
    let inner = object_inner(input, "@selector-refind")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-refind 对象 payload 不能为空",
        ));
    }

    let mut selector_id = None::<String>;
    let mut limit = None::<u16>;
    let mut policy = None::<SelectorRefindPolicy>;
    let mut min_confidence_milli = None::<u16>;
    let mut include_explanations = None::<bool>;
    let mut include_history = None::<bool>;
    let mut source = None::<SelectorRefindSource>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "selector_id" => assign_selector_field(
                &mut selector_id,
                "selector_id",
                "@selector-refind",
                parse_quoted_payload(raw_value)?,
            )?,
            "limit" => assign_selector_field(
                &mut limit,
                "limit",
                "@selector-refind",
                parse_selector_limit("@selector-refind", raw_value)?,
            )?,
            "policy" => assign_selector_field(
                &mut policy,
                "policy",
                "@selector-refind",
                parse_selector_refind_policy(raw_value)?,
            )?,
            "min_confidence" => assign_selector_field(
                &mut min_confidence_milli,
                "min_confidence",
                "@selector-refind",
                parse_selector_min_confidence(raw_value)?,
            )?,
            "include_explanations" => assign_selector_field(
                &mut include_explanations,
                "include_explanations",
                "@selector-refind",
                parse_selector_bool("@selector-refind", "include_explanations", raw_value)?,
            )?,
            "include_history" => assign_selector_field(
                &mut include_history,
                "include_history",
                "@selector-refind",
                parse_selector_bool("@selector-refind", "include_history", raw_value)?,
            )?,
            "source" => assign_selector_field(
                &mut source,
                "source",
                "@selector-refind",
                parse_selector_refind_source(raw_value)?,
            )?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@selector-refind 不支持字段: {field_name}"),
                ));
            }
        }
    }

    Ok(SelectorRefindRequest {
        selector_id: require_selector_string("@selector-refind.selector_id", selector_id)?,
        limit: limit.unwrap_or(crate::control_observation::refind::DEFAULT_REFIND_LIMIT),
        policy: policy.unwrap_or(SelectorRefindPolicy::Safe),
        min_confidence_milli: min_confidence_milli
            .unwrap_or(crate::control_observation::refind::DEFAULT_REFIND_MIN_CONFIDENCE_MILLI),
        include_explanations: include_explanations.unwrap_or(true),
        include_history: include_history.unwrap_or(false),
        source,
    })
}

fn parse_selector_refind_policy(input: &str) -> io::Result<SelectorRefindPolicy> {
    match parse_quoted_payload(input)?.as_str() {
        "safe" => Ok(SelectorRefindPolicy::Safe),
        "manual" => Ok(SelectorRefindPolicy::Manual),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@selector-refind.policy 不支持: {other}"),
        )),
    }
}

fn parse_selector_min_confidence(input: &str) -> io::Result<u16> {
    let value = input.parse::<f64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-refind.min_confidence 必须是 0.0 到 1.0 之间的数字",
        )
    })?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-refind.min_confidence 必须在 0.0 到 1.0 之间",
        ));
    }
    Ok((value * 1000.0).round() as u16)
}

fn parse_selector_refind_source(input: &str) -> io::Result<SelectorRefindSource> {
    let inner = object_inner(input, "@selector-refind.source")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@selector-refind.source 不能为空对象",
        ));
    }

    let mut observation_id = None::<String>;
    let mut ref_id = None::<String>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "observation_id" => assign_selector_field(
                &mut observation_id,
                "observation_id",
                "@selector-refind.source",
                parse_quoted_payload(raw_value)?,
            )?,
            "ref" | "ref_id" => assign_selector_field(
                &mut ref_id,
                "ref",
                "@selector-refind.source",
                parse_quoted_payload(raw_value)?,
            )?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@selector-refind.source 不支持字段: {field_name}"),
                ));
            }
        }
    }

    Ok(SelectorRefindSource {
        observation_id: require_selector_string(
            "@selector-refind.source.observation_id",
            observation_id,
        )?,
        ref_id: require_selector_string("@selector-refind.source.ref", ref_id)?,
    })
}

fn assign_selector_field<T>(
    slot: &mut Option<T>,
    field_name: &str,
    kind: &str,
    value: T,
) -> io::Result<()> {
    if slot.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 字段重复: {field_name}"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn require_selector_string(kind: &str, value: Option<String>) -> io::Result<String> {
    let value = value.unwrap_or_default();
    if value.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 不能为空"),
        ));
    }
    Ok(value)
}

fn parse_selector_bool(kind: &str, field_name: &str, input: &str) -> io::Result<bool> {
    match input {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind}.{field_name} 必须是 true 或 false"),
        )),
    }
}

fn parse_selector_limit(kind: &str, input: &str) -> io::Result<u16> {
    let value = input.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind}.limit 必须是正整数"),
        )
    })?;
    if value == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind}.limit 必须大于 0"),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
