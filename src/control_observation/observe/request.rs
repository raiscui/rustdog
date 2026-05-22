use crate::{
    control_ax::{
        parse_ax_depth, parse_ax_max_elements, parse_ax_mode_payload, parse_bool_literal, AxMode,
        AxWindow,
    },
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields,
    },
    control_window::WindowQuery,
};
use std::io;

const DEFAULT_OBSERVE_LIMIT: u16 = 20;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObserveMode {
    Hybrid,
    Visual,
    Ax,
    Window,
}

impl ObserveMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::Visual => "visual",
            Self::Ax => "ax",
            Self::Window => "window",
        }
    }

    fn from_payload(input: &str) -> io::Result<Self> {
        match parse_quoted_payload(input)?.as_str() {
            "hybrid" => Ok(Self::Hybrid),
            "visual" => Ok(Self::Visual),
            "ax" => Ok(Self::Ax),
            "window" => Ok(Self::Window),
            other => Err(invalid_data(format!("@observe.mode 不支持: {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObserveTarget {
    pub app: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub window_title_contains: Option<String>,
}

impl ObserveTarget {
    pub(super) fn is_empty(&self) -> bool {
        self.app.is_none()
            && self.bundle_id.is_none()
            && self.window_title.is_none()
            && self.window_title_contains.is_none()
    }

    pub(super) fn to_window_query(&self) -> WindowQuery {
        WindowQuery {
            app: self.app.clone(),
            bundle_id: self.bundle_id.clone(),
            title: self.window_title.clone(),
            title_contains: self.window_title_contains.clone(),
            ..WindowQuery::default()
        }
    }

    pub(super) fn matches_ax_window(&self, window: &AxWindow) -> bool {
        matches_optional(self.app.as_ref(), Some(window.process_name.as_str()))
            && matches_optional(self.window_title.as_ref(), window.title.as_deref())
            && matches_contains(self.window_title_contains.as_ref(), window.title.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveRequest {
    pub mode: ObserveMode,
    pub target: Option<ObserveTarget>,
    pub include_screenshot: bool,
    pub include_ax: bool,
    pub ax_required: bool,
    pub include_windows: bool,
    pub include_manifest: bool,
    pub include_refs: bool,
    pub include_selectors: bool,
    pub limit: u16,
    pub ax_mode: AxMode,
    pub ax_depth: u8,
    pub ax_max_elements: u16,
    pub ax_include_values: bool,
}

impl ObserveRequest {
    pub(super) fn for_mode(mode: ObserveMode) -> Self {
        let preset = AxMode::Interactive.preset();
        Self {
            mode,
            target: None,
            include_screenshot: matches!(mode, ObserveMode::Hybrid | ObserveMode::Visual),
            include_ax: matches!(mode, ObserveMode::Hybrid | ObserveMode::Ax),
            ax_required: false,
            include_windows: matches!(mode, ObserveMode::Hybrid | ObserveMode::Window),
            include_manifest: true,
            include_refs: true,
            include_selectors: true,
            limit: DEFAULT_OBSERVE_LIMIT,
            ax_mode: AxMode::Interactive,
            ax_depth: preset.depth,
            ax_max_elements: preset.max_elements,
            ax_include_values: preset.include_values,
        }
    }
}

impl Default for ObserveRequest {
    fn default() -> Self {
        Self::for_mode(ObserveMode::Hybrid)
    }
}

pub fn parse_observe_payload(input: &str) -> io::Result<ObserveRequest> {
    if input.trim().is_empty() {
        return Ok(ObserveRequest::default());
    }

    let inner = object_inner(input, "@observe")?;
    if inner.is_empty() {
        return Ok(ObserveRequest::default());
    }

    let mut mode = None::<ObserveMode>;
    let mut target = None::<ObserveTarget>;
    let mut include_screenshot = None::<bool>;
    let mut include_ax = None::<bool>;
    let mut ax_required = None::<bool>;
    let mut include_windows = None::<bool>;
    let mut include_manifest = None::<bool>;
    let mut include_refs = None::<bool>;
    let mut include_selectors = None::<bool>;
    let mut limit = None::<u16>;
    let mut ax_mode = None::<AxMode>;
    let mut ax_depth = None::<u8>;
    let mut ax_max_elements = None::<u16>;
    let mut ax_include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@observe",
                ObserveMode::from_payload(raw_value)?,
            )?,
            "target" => assign_once(
                &mut target,
                "target",
                "@observe",
                parse_observe_target(raw_value)?,
            )?,
            "include_screenshot" => assign_once(
                &mut include_screenshot,
                "include_screenshot",
                "@observe",
                parse_bool_literal("@observe", "include_screenshot", raw_value)?,
            )?,
            "include_ax" => assign_once(
                &mut include_ax,
                "include_ax",
                "@observe",
                parse_bool_literal("@observe", "include_ax", raw_value)?,
            )?,
            "ax_required" => assign_once(
                &mut ax_required,
                "ax_required",
                "@observe",
                parse_bool_literal("@observe", "ax_required", raw_value)?,
            )?,
            "include_windows" => assign_once(
                &mut include_windows,
                "include_windows",
                "@observe",
                parse_bool_literal("@observe", "include_windows", raw_value)?,
            )?,
            "include_manifest" => assign_once(
                &mut include_manifest,
                "include_manifest",
                "@observe",
                parse_bool_literal("@observe", "include_manifest", raw_value)?,
            )?,
            "include_refs" => assign_once(
                &mut include_refs,
                "include_refs",
                "@observe",
                parse_bool_literal("@observe", "include_refs", raw_value)?,
            )?,
            "include_selectors" => assign_once(
                &mut include_selectors,
                "include_selectors",
                "@observe",
                parse_bool_literal("@observe", "include_selectors", raw_value)?,
            )?,
            "limit" => assign_once(&mut limit, "limit", "@observe", parse_limit(raw_value)?)?,
            "ax_mode" => assign_once(
                &mut ax_mode,
                "ax_mode",
                "@observe",
                parse_ax_mode_payload("@observe", raw_value)?,
            )?,
            "ax_depth" => assign_once(
                &mut ax_depth,
                "ax_depth",
                "@observe",
                parse_ax_depth(raw_value)?,
            )?,
            "ax_max_elements" => assign_once(
                &mut ax_max_elements,
                "ax_max_elements",
                "@observe",
                parse_ax_max_elements(raw_value)?,
            )?,
            "ax_include_values" => assign_once(
                &mut ax_include_values,
                "ax_include_values",
                "@observe",
                parse_bool_literal("@observe", "ax_include_values", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@observe 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let mode = mode.unwrap_or(ObserveMode::Hybrid);
    let mut request = ObserveRequest::for_mode(mode);
    request.target = target;
    if let Some(ax_mode) = ax_mode {
        let preset = ax_mode.preset();
        request.ax_mode = ax_mode;
        request.ax_depth = preset.depth;
        request.ax_max_elements = preset.max_elements;
        request.ax_include_values = preset.include_values;
    }
    if let Some(value) = include_screenshot {
        request.include_screenshot = value;
    }
    if let Some(value) = include_ax {
        request.include_ax = value;
    }
    if let Some(value) = ax_required {
        request.ax_required = value;
    }
    if let Some(value) = include_windows {
        request.include_windows = value;
    }
    if let Some(value) = include_manifest {
        request.include_manifest = value;
    }
    if let Some(value) = include_refs {
        request.include_refs = value;
    }
    if let Some(value) = include_selectors {
        request.include_selectors = value;
    }
    if let Some(value) = limit {
        request.limit = value;
    }
    if let Some(value) = ax_depth {
        request.ax_depth = value;
    }
    if let Some(value) = ax_max_elements {
        request.ax_max_elements = value;
    }
    if let Some(value) = ax_include_values {
        request.ax_include_values = value;
    }
    Ok(request)
}

fn parse_observe_target(input: &str) -> io::Result<ObserveTarget> {
    let inner = object_inner(input, "@observe.target")?;
    if inner.is_empty() {
        return Err(invalid_data("@observe.target 不能为空对象"));
    }
    let mut target = ObserveTarget::default();
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "app" | "process" | "process_name" => assign_once(
                &mut target.app,
                "app",
                "@observe.target",
                parse_non_empty_string("@observe.target.app", raw_value)?,
            )?,
            "bundle_id" => assign_once(
                &mut target.bundle_id,
                "bundle_id",
                "@observe.target",
                parse_non_empty_string("@observe.target.bundle_id", raw_value)?,
            )?,
            "window_title" | "title" => assign_once(
                &mut target.window_title,
                "window_title",
                "@observe.target",
                parse_non_empty_string("@observe.target.window_title", raw_value)?,
            )?,
            "window_title_contains" | "title_contains" => assign_once(
                &mut target.window_title_contains,
                "window_title_contains",
                "@observe.target",
                parse_non_empty_string("@observe.target.window_title_contains", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@observe.target 不支持字段: {field_name}"
                )))
            }
        }
    }
    if target.is_empty() {
        return Err(invalid_data("@observe.target 至少需要一个查询字段"));
    }
    Ok(target)
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.trim().is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn parse_limit(input: &str) -> io::Result<u16> {
    let value = input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("@observe.limit 必须是无符号整数: {input}")))?;
    if value == 0 {
        return Err(invalid_data("@observe.limit 必须大于 0"));
    }
    Ok(value)
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!("{kind} 的 `{field_name}` 字段重复")));
    }
    *slot = Some(value);
    Ok(())
}

fn matches_optional(expected: Option<&String>, actual: Option<&str>) -> bool {
    expected
        .map(|expected| actual == Some(expected.as_str()))
        .unwrap_or(true)
}

fn matches_contains(expected: Option<&String>, actual: Option<&str>) -> bool {
    expected
        .map(|expected| {
            actual
                .map(|actual| actual.contains(expected))
                .unwrap_or(false)
        })
        .unwrap_or(true)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
