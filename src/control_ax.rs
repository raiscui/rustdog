use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use serde::Serialize;
use serde_json::json;
use std::io;

pub const AX_SCHEMA: &str = "rdog.ax.v1";
pub const DEFAULT_AX_DEPTH: u8 = 4;
pub const DEFAULT_AX_MAX_ELEMENTS: u16 = 1000;
pub const DEFAULT_AX_INCLUDE_VALUES: bool = true;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxTreeRequest {
    pub scope: AxTreeScope,
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

impl Default for AxTreeRequest {
    fn default() -> Self {
        Self {
            scope: AxTreeScope::Windows,
            depth: DEFAULT_AX_DEPTH,
            max_elements: DEFAULT_AX_MAX_ELEMENTS,
            include_values: DEFAULT_AX_INCLUDE_VALUES,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AxTreeScope {
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxPressRequest {
    pub target: AxTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AxTarget {
    pub id: Option<String>,
    pub process: Option<String>,
    pub window_title: Option<String>,
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl AxTarget {
    fn validate(&self) -> io::Result<()> {
        if self.id.is_some() {
            return Ok(());
        }

        if self.process.is_none()
            && self.window_title.is_none()
            && self.role.is_none()
            && self.subrole.is_none()
            && self.name.is_none()
            && self.description.is_none()
        {
            return Err(invalid_data("@ax-press target 不能为空"));
        }

        if self.role.is_none()
            && self.subrole.is_none()
            && self.name.is_none()
            && self.description.is_none()
        {
            return Err(invalid_data(
                "@ax-press semantic target 必须至少包含 role/subrole/name/description 之一",
            ));
        }

        Ok(())
    }

    fn matches_window(&self, window: &AxWindow) -> bool {
        matches_optional(&self.process, Some(window.process_name.as_str()))
            && matches_optional(&self.window_title, window.title.as_deref())
    }

    fn matches_element(&self, element: &AxElement) -> bool {
        matches_optional(&self.role, Some(element.role.as_str()))
            && matches_optional(&self.subrole, element.subrole.as_deref())
            && matches_optional(&self.name, element.name.as_deref())
            && matches_optional(&self.description, element.description.as_deref())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct AxRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxSnapshot {
    pub schema: &'static str,
    pub platform: String,
    pub capture_status: String,
    pub permission_status: String,
    pub coordinate_space: &'static str,
    pub window_count: usize,
    pub element_count: usize,
    pub truncated: bool,
    pub windows: Vec<AxWindow>,
}

impl AxSnapshot {
    pub fn complete(
        platform: impl Into<String>,
        mut windows: Vec<AxWindow>,
        truncated: bool,
    ) -> Self {
        windows.sort_by(|a, b| {
            a.pid
                .cmp(&b.pid)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| a.title.cmp(&b.title))
        });
        let element_count = windows.iter().map(AxWindow::element_count).sum();
        Self {
            schema: AX_SCHEMA,
            platform: platform.into(),
            capture_status: "complete".to_owned(),
            permission_status: "granted".to_owned(),
            coordinate_space: "os-logical",
            window_count: windows.len(),
            element_count,
            truncated,
            windows,
        }
    }

    pub fn permission_denied(platform: impl Into<String>) -> Self {
        Self::empty_status(platform, "permission_denied", "denied")
    }

    pub fn unsupported() -> Self {
        Self::empty_status("unsupported", "unsupported", "unknown")
    }

    fn empty_status(
        platform: impl Into<String>,
        capture_status: impl Into<String>,
        permission_status: impl Into<String>,
    ) -> Self {
        Self {
            schema: AX_SCHEMA,
            platform: platform.into(),
            capture_status: capture_status.into(),
            permission_status: permission_status.into(),
            coordinate_space: "os-logical",
            window_count: 0,
            element_count: 0,
            truncated: false,
            windows: Vec::new(),
        }
    }

    pub fn to_tree_value_json(&self) -> io::Result<String> {
        let value = json!({
            "kind": "ax-tree",
            "schema": self.schema,
            "platform": self.platform,
            "capture_status": self.capture_status,
            "permission_status": self.permission_status,
            "coordinate_space": self.coordinate_space,
            "window_count": self.window_count,
            "element_count": self.element_count,
            "truncated": self.truncated,
            "windows": self.windows,
        });
        serde_json::to_string(&value)
            .map_err(|err| io::Error::other(format!("AX tree response 序列化失败: {err}")))
    }

    fn contains_element_id(&self, target_id: &str) -> bool {
        self.windows.iter().any(|window| {
            window.id == target_id
                || window
                    .elements
                    .iter()
                    .any(|element| element.contains_id(target_id))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxWindow {
    pub id: String,
    pub pid: i32,
    pub process_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    pub elements: Vec<AxElement>,
}

impl AxWindow {
    fn element_count(&self) -> usize {
        self.elements.iter().map(AxElement::tree_count).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxElement {
    pub id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub value_redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub actions: Vec<String>,
    pub ax_path: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AxElement>,
}

impl AxElement {
    fn tree_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(AxElement::tree_count)
            .sum::<usize>()
    }

    fn contains_id(&self, target_id: &str) -> bool {
        self.id == target_id
            || self
                .children
                .iter()
                .any(|child| child.contains_id(target_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxActionReport {
    pub kind: &'static str,
    pub action: &'static str,
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub performed: bool,
    pub status: &'static str,
}

impl AxActionReport {
    pub fn press(backend: impl Into<String>, target_id: Option<String>) -> Self {
        Self {
            kind: "ax",
            action: "press",
            backend: backend.into(),
            target_id,
            performed: true,
            status: "ok",
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX action response 序列化失败: {err}")))
    }
}

pub trait AxBackend {
    fn snapshot(&self, request: &AxTreeRequest) -> io::Result<AxSnapshot>;
    fn press(&self, request: &AxPressRequest) -> io::Result<AxActionReport>;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct SystemAxBackend;

impl AxBackend for SystemAxBackend {
    fn snapshot(&self, request: &AxTreeRequest) -> io::Result<AxSnapshot> {
        platform_snapshot(request)
    }

    fn press(&self, request: &AxPressRequest) -> io::Result<AxActionReport> {
        platform_press(request)
    }
}

pub fn capture_default_ax_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    SystemAxBackend.snapshot(request)
}

pub fn perform_default_ax_press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    SystemAxBackend.press(request)
}

pub fn current_ax_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unsupported"
    }
}

pub fn parse_ax_tree_payload(input: &str) -> io::Result<AxTreeRequest> {
    let inner = object_inner(input, "@ax-tree")?;
    if inner.is_empty() {
        return Ok(AxTreeRequest::default());
    }

    let mut scope = None::<AxTreeScope>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "scope" => assign_once(
                &mut scope,
                "scope",
                "@ax-tree",
                parse_ax_tree_scope(raw_value)?,
            )?,
            "depth" => assign_once(&mut depth, "depth", "@ax-tree", parse_ax_depth(raw_value)?)?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@ax-tree",
                parse_ax_max_elements(raw_value)?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@ax-tree",
                parse_bool_literal("@ax-tree", "include_values", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-tree 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxTreeRequest {
        scope: scope.unwrap_or(AxTreeScope::Windows),
        depth: depth.unwrap_or(DEFAULT_AX_DEPTH),
        max_elements: max_elements.unwrap_or(DEFAULT_AX_MAX_ELEMENTS),
        include_values: include_values.unwrap_or(DEFAULT_AX_INCLUDE_VALUES),
    })
}

pub fn parse_ax_press_payload(input: &str) -> io::Result<AxPressRequest> {
    let inner = object_inner(input, "@ax-press")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-press 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-press",
                parse_ax_target(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-press 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxPressRequest {
        target: required_field(target, "@ax-press", "target")?,
    })
}

pub fn resolve_target_id_in_snapshot(
    snapshot: &AxSnapshot,
    target: &AxTarget,
) -> io::Result<String> {
    if let Some(id) = &target.id {
        if snapshot.contains_element_id(id) {
            return Ok(id.clone());
        }
        return Err(invalid_input(format!(
            "@ax-press target id 已失效或不存在: {id}"
        )));
    }

    target.validate().map_err(to_invalid_input)?;
    let mut matches = Vec::<String>::new();

    for window in &snapshot.windows {
        if !target.matches_window(window) {
            continue;
        }
        collect_matching_element_ids(target, &window.elements, &mut matches);
        if matches.len() > 1 {
            return Err(invalid_input("@ax-press semantic target 匹配到多个元素"));
        }
    }

    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(invalid_input("@ax-press semantic target 未匹配到元素")),
        _ => Err(invalid_input("@ax-press semantic target 匹配到多个元素")),
    }
}

fn collect_matching_element_ids(
    target: &AxTarget,
    elements: &[AxElement],
    matches: &mut Vec<String>,
) {
    for element in elements {
        if target.matches_element(element) {
            matches.push(element.id.clone());
        }
        collect_matching_element_ids(target, &element.children, matches);
    }
}

fn parse_ax_target(input: &str) -> io::Result<AxTarget> {
    let inner = object_inner(input, "@ax-press target")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-press target 不能为空"));
    }

    let mut target = AxTarget::default();
    let mut id_seen = false;
    let mut process_seen = false;
    let mut window_title_seen = false;
    let mut role_seen = false;
    let mut subrole_seen = false;
    let mut name_seen = false;
    let mut description_seen = false;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "id" => {
                reject_duplicate(&mut id_seen, "@ax-press target", "id")?;
                target.id = Some(parse_non_empty_string("@ax-press target.id", raw_value)?);
            }
            "process" | "process_name" => {
                reject_duplicate(&mut process_seen, "@ax-press target", "process")?;
                target.process = Some(parse_non_empty_string(
                    "@ax-press target.process",
                    raw_value,
                )?);
            }
            "window_title" | "title" => {
                reject_duplicate(&mut window_title_seen, "@ax-press target", "window_title")?;
                target.window_title = Some(parse_non_empty_string(
                    "@ax-press target.window_title",
                    raw_value,
                )?);
            }
            "role" => {
                reject_duplicate(&mut role_seen, "@ax-press target", "role")?;
                target.role = Some(parse_non_empty_string("@ax-press target.role", raw_value)?);
            }
            "subrole" => {
                reject_duplicate(&mut subrole_seen, "@ax-press target", "subrole")?;
                target.subrole = Some(parse_non_empty_string(
                    "@ax-press target.subrole",
                    raw_value,
                )?);
            }
            "name" => {
                reject_duplicate(&mut name_seen, "@ax-press target", "name")?;
                target.name = Some(parse_non_empty_string("@ax-press target.name", raw_value)?);
            }
            "description" => {
                reject_duplicate(&mut description_seen, "@ax-press target", "description")?;
                target.description = Some(parse_non_empty_string(
                    "@ax-press target.description",
                    raw_value,
                )?);
            }
            _ => {
                return Err(invalid_data(format!(
                    "@ax-press target 包含未知字段: {field_name}"
                )))
            }
        }
    }

    target.validate()?;
    Ok(target)
}

fn parse_ax_tree_scope(input: &str) -> io::Result<AxTreeScope> {
    let scope = parse_quoted_payload(input)?;
    match scope.to_ascii_lowercase().as_str() {
        "windows" => Ok(AxTreeScope::Windows),
        _ => Err(invalid_data(format!(
            "@ax-tree 当前只支持 scope=\"windows\": {scope}"
        ))),
    }
}

fn parse_ax_depth(input: &str) -> io::Result<u8> {
    let depth = input
        .parse::<u8>()
        .map_err(|_| invalid_data(format!("@ax-tree 的 `depth` 必须是无符号整数: {input}")))?;
    if depth == 0 {
        return Err(invalid_data("@ax-tree 的 `depth` 必须大于 0"));
    }
    Ok(depth)
}

fn parse_ax_max_elements(input: &str) -> io::Result<u16> {
    let max_elements = input.parse::<u16>().map_err(|_| {
        invalid_data(format!(
            "@ax-tree 的 `max_elements` 必须是无符号整数: {input}"
        ))
    })?;
    if max_elements == 0 {
        return Err(invalid_data("@ax-tree 的 `max_elements` 必须大于 0"));
    }
    Ok(max_elements)
}

pub(crate) fn parse_bool_literal(kind: &str, field_name: &str, input: &str) -> io::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_data(format!(
            "{kind} 的 `{field_name}` 必须是 true 或 false: {input}"
        ))),
    }
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn matches_optional(expected: &Option<String>, actual: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual == Some(expected.as_str()),
        None => true,
    }
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!(
            "{kind} 对象 payload 的 `{field_name}` 字段重复"
        )));
    }
    *slot = Some(value);
    Ok(())
}

fn reject_duplicate(seen: &mut bool, kind: &str, field_name: &str) -> io::Result<()> {
    if *seen {
        return Err(invalid_data(format!("{kind} 的 `{field_name}` 字段重复")));
    }
    *seen = true;
    Ok(())
}

fn required_field<T>(value: Option<T>, kind: &str, field_name: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{kind} 对象 payload 缺少必填字段 `{field_name}`")))
}

fn to_invalid_input(err: io::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.to_string())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(target_os = "macos")]
fn platform_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    macos::snapshot(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_snapshot(_request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AX snapshot 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
fn platform_press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    macos::press(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_press(_request: &AxPressRequest) -> io::Result<AxActionReport> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "AXPress 当前只支持 macOS",
    ))
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ax_snapshot_should_count_nested_elements_and_render_tree_response() {
        let snapshot = AxSnapshot::complete(
            "macos",
            vec![AxWindow {
                id: "pid:1/window:0".to_owned(),
                pid: 1,
                process_name: "System Information".to_owned(),
                title: Some("关于本机".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: Some(AxRect {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 200,
                }),
                focused: Some(true),
                elements: vec![AxElement {
                    id: "pid:1/window:0/path:0".to_owned(),
                    role: "AXButton".to_owned(),
                    subrole: None,
                    name: Some("关闭".to_owned()),
                    value: None,
                    value_redacted: false,
                    description: Some("关闭按钮".to_owned()),
                    rect: None,
                    enabled: Some(true),
                    actions: vec!["AXPress".to_owned()],
                    ax_path: vec![0],
                    children: Vec::new(),
                }],
            }],
            false,
        );

        assert_eq!(snapshot.window_count, 1);
        assert_eq!(snapshot.element_count, 1);
        let value = snapshot.to_tree_value_json().unwrap();
        assert!(value.contains(r#""kind":"ax-tree""#));
        assert!(value.contains(r#""schema":"rdog.ax.v1""#));
    }

    #[test]
    fn resolve_target_should_reject_stale_or_ambiguous_locators() {
        let button = |id: &str| AxElement {
            id: id.to_owned(),
            role: "AXButton".to_owned(),
            subrole: None,
            name: Some("OK".to_owned()),
            value: None,
            value_redacted: false,
            description: None,
            rect: None,
            enabled: Some(true),
            actions: vec!["AXPress".to_owned()],
            ax_path: vec![0],
            children: Vec::new(),
        };
        let snapshot = AxSnapshot::complete(
            "macos",
            vec![AxWindow {
                id: "pid:1/window:0".to_owned(),
                pid: 1,
                process_name: "App".to_owned(),
                title: Some("Win".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: None,
                elements: vec![
                    button("pid:1/window:0/path:0"),
                    button("pid:1/window:0/path:1"),
                ],
            }],
            false,
        );

        let target = AxTarget {
            id: Some("pid:1/window:0/path:404".to_owned()),
            ..AxTarget::default()
        };
        assert_eq!(
            resolve_target_id_in_snapshot(&snapshot, &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let target = AxTarget {
            process: Some("App".to_owned()),
            window_title: Some("Win".to_owned()),
            role: Some("AXButton".to_owned()),
            name: Some("OK".to_owned()),
            ..AxTarget::default()
        };
        assert_eq!(
            resolve_target_id_in_snapshot(&snapshot, &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn secure_element_should_serialize_redaction_without_value() {
        let element = AxElement {
            id: "pid:1/window:0/path:0".to_owned(),
            role: "AXSecureTextField".to_owned(),
            subrole: None,
            name: Some("Password".to_owned()),
            value: None,
            value_redacted: true,
            description: None,
            rect: None,
            enabled: Some(true),
            actions: Vec::new(),
            ax_path: vec![0],
            children: Vec::new(),
        };
        let value = serde_json::to_value(&element).unwrap();
        assert_eq!(value["value_redacted"], true);
        assert!(value.get("value").is_none());
    }

    #[test]
    fn parse_ax_tree_payload_should_validate_limits() {
        assert_eq!(
            parse_ax_tree_payload(
                r#"{scope:"windows",depth:4,max_elements:1000,include_values:false}"#
            )
            .unwrap(),
            AxTreeRequest {
                scope: AxTreeScope::Windows,
                depth: 4,
                max_elements: 1000,
                include_values: false,
            }
        );
        assert!(parse_ax_tree_payload(r#"{depth:0}"#).is_err());
        assert!(parse_ax_tree_payload(r#"{max_elements:0}"#).is_err());
    }

    #[test]
    fn parse_ax_press_payload_should_require_target() {
        assert_eq!(
            parse_ax_press_payload(r#"{target:{id:"pid:1/window:0/path:0"}}"#).unwrap(),
            AxPressRequest {
                target: AxTarget {
                    id: Some("pid:1/window:0/path:0".to_owned()),
                    ..AxTarget::default()
                },
            }
        );
        assert!(parse_ax_press_payload(r#"{target:{}}"#).is_err());
        assert!(parse_ax_press_payload(r#"{target:{process:"App"}}"#).is_err());
    }
}
