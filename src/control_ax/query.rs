use super::{
    assign_once, invalid_data, parse_ax_depth, parse_ax_max_elements, parse_ax_mode_payload,
    parse_ax_target, parse_bool_literal, required_field, resolve_target_id_in_snapshot, AxElement,
    AxMode, AxRect, AxSnapshot, AxTarget, AxTreeRequest, AxWindow, AX_SCHEMA,
};
use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use serde::Serialize;
use std::io;

const DEFAULT_AX_FIND_LIMIT: u16 = 20;
const DEFAULT_AX_FIND_MODE: AxMode = AxMode::Interactive;
const DEFAULT_AX_GET_MODE: AxMode = AxMode::Interactive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxFindRequest {
    pub tree: AxTreeRequest,
    pub query: AxFindQuery,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AxFindQuery {
    pub process: Option<String>,
    pub process_contains: Option<String>,
    pub window_title: Option<String>,
    pub window_title_contains: Option<String>,
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub name: Option<String>,
    pub name_contains: Option<String>,
    pub description: Option<String>,
    pub description_contains: Option<String>,
    pub value: Option<String>,
    pub value_contains: Option<String>,
    pub action: Option<String>,
}

impl AxFindQuery {
    fn validate(&self) -> io::Result<()> {
        if self.process.is_none()
            && self.process_contains.is_none()
            && self.window_title.is_none()
            && self.window_title_contains.is_none()
            && self.role.is_none()
            && self.subrole.is_none()
            && self.name.is_none()
            && self.name_contains.is_none()
            && self.description.is_none()
            && self.description_contains.is_none()
            && self.value.is_none()
            && self.value_contains.is_none()
            && self.action.is_none()
        {
            return Err(invalid_data("@ax-find 至少需要一个查询字段"));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxGetRequest {
    pub target: AxTarget,
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

impl AxGetRequest {
    pub fn tree_request(&self) -> AxTreeRequest {
        AxTreeRequest {
            depth: self.capture_depth(),
            max_elements: self.max_elements,
            include_values: self.include_values,
            ..AxTreeRequest::default()
        }
    }

    fn capture_depth(&self) -> u8 {
        let target_depth = self
            .target
            .id
            .as_deref()
            .and_then(target_id_path_depth)
            .unwrap_or(0);
        self.depth.saturating_add(target_depth)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AxFindResponse {
    kind: &'static str,
    schema: &'static str,
    platform: String,
    capture_status: String,
    permission_status: String,
    coordinate_space: &'static str,
    match_count: usize,
    returned_count: usize,
    truncated: bool,
    matches: Vec<AxFindMatch>,
}

impl AxFindResponse {
    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX find response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AxFindMatch {
    id: String,
    window_id: String,
    pid: i32,
    process_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_title: Option<String>,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    value_redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    actions: Vec<String>,
    ax_path: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AxGetResponse {
    kind: &'static str,
    schema: &'static str,
    platform: String,
    capture_status: String,
    permission_status: String,
    coordinate_space: &'static str,
    target_id: String,
    target_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<AxWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    element: Option<AxElement>,
}

impl AxGetResponse {
    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("AX get response 序列化失败: {err}")))
    }
}

pub fn parse_ax_find_payload(input: &str) -> io::Result<AxFindRequest> {
    let inner = object_inner(input, "@ax-find")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-find 对象 payload 不能为空"));
    }

    let mut mode = None::<AxMode>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;
    let mut limit = None::<u16>;
    let mut query = AxFindQuery::default();

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-find",
                parse_ax_mode_payload("@ax-find", raw_value)?,
            )?,
            "depth" => assign_once(&mut depth, "depth", "@ax-find", parse_ax_depth(raw_value)?)?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@ax-find",
                parse_ax_max_elements(raw_value)?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@ax-find",
                parse_bool_literal("@ax-find", "include_values", raw_value)?,
            )?,
            "limit" => assign_once(&mut limit, "limit", "@ax-find", parse_limit(raw_value)?)?,
            "process" | "process_name" => assign_once(
                &mut query.process,
                "process",
                "@ax-find",
                parse_non_empty_string("@ax-find.process", raw_value)?,
            )?,
            "process_contains" => assign_once(
                &mut query.process_contains,
                "process_contains",
                "@ax-find",
                parse_non_empty_string("@ax-find.process_contains", raw_value)?,
            )?,
            "window_title" | "title" => assign_once(
                &mut query.window_title,
                "window_title",
                "@ax-find",
                parse_non_empty_string("@ax-find.window_title", raw_value)?,
            )?,
            "window_title_contains" | "title_contains" => assign_once(
                &mut query.window_title_contains,
                "window_title_contains",
                "@ax-find",
                parse_non_empty_string("@ax-find.window_title_contains", raw_value)?,
            )?,
            "role" => assign_once(
                &mut query.role,
                "role",
                "@ax-find",
                parse_non_empty_string("@ax-find.role", raw_value)?,
            )?,
            "subrole" => assign_once(
                &mut query.subrole,
                "subrole",
                "@ax-find",
                parse_non_empty_string("@ax-find.subrole", raw_value)?,
            )?,
            "name" => assign_once(
                &mut query.name,
                "name",
                "@ax-find",
                parse_non_empty_string("@ax-find.name", raw_value)?,
            )?,
            "name_contains" => assign_once(
                &mut query.name_contains,
                "name_contains",
                "@ax-find",
                parse_non_empty_string("@ax-find.name_contains", raw_value)?,
            )?,
            "description" => assign_once(
                &mut query.description,
                "description",
                "@ax-find",
                parse_non_empty_string("@ax-find.description", raw_value)?,
            )?,
            "description_contains" => assign_once(
                &mut query.description_contains,
                "description_contains",
                "@ax-find",
                parse_non_empty_string("@ax-find.description_contains", raw_value)?,
            )?,
            "value" => assign_once(
                &mut query.value,
                "value",
                "@ax-find",
                parse_non_empty_string("@ax-find.value", raw_value)?,
            )?,
            "value_contains" => assign_once(
                &mut query.value_contains,
                "value_contains",
                "@ax-find",
                parse_non_empty_string("@ax-find.value_contains", raw_value)?,
            )?,
            "action" => assign_once(
                &mut query.action,
                "action",
                "@ax-find",
                parse_non_empty_string("@ax-find.action", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-find 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    query.validate()?;
    let preset = mode.unwrap_or(DEFAULT_AX_FIND_MODE).preset();
    Ok(AxFindRequest {
        tree: AxTreeRequest {
            depth: depth.unwrap_or(preset.depth),
            max_elements: max_elements.unwrap_or(preset.max_elements),
            include_values: include_values.unwrap_or(preset.include_values),
            ..AxTreeRequest::default()
        },
        query,
        limit: limit.unwrap_or(DEFAULT_AX_FIND_LIMIT),
    })
}

pub fn parse_ax_get_payload(input: &str) -> io::Result<AxGetRequest> {
    let inner = object_inner(input, "@ax-get")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-get 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut mode = None::<AxMode>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-get",
                parse_ax_target(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-get",
                parse_ax_mode_payload("@ax-get", raw_value)?,
            )?,
            "depth" => assign_once(&mut depth, "depth", "@ax-get", parse_ax_depth(raw_value)?)?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@ax-get",
                parse_ax_max_elements(raw_value)?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@ax-get",
                parse_bool_literal("@ax-get", "include_values", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-get 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let preset = mode.unwrap_or(DEFAULT_AX_GET_MODE).preset();
    Ok(AxGetRequest {
        target: required_field(target, "@ax-get", "target")?,
        depth: depth.unwrap_or(preset.depth),
        max_elements: max_elements.unwrap_or(preset.max_elements),
        include_values: include_values.unwrap_or(preset.include_values),
    })
}

pub fn build_ax_find_response_json(
    snapshot: &AxSnapshot,
    request: &AxFindRequest,
) -> io::Result<String> {
    let mut matches = Vec::new();
    let mut match_count = 0usize;

    for window in &snapshot.windows {
        collect_matches(
            window,
            &request.query,
            request.limit,
            &mut match_count,
            &mut matches,
        );
    }

    let response = AxFindResponse {
        kind: "ax-find",
        schema: AX_SCHEMA,
        platform: snapshot.platform.clone(),
        capture_status: snapshot.capture_status.clone(),
        permission_status: snapshot.permission_status.clone(),
        coordinate_space: snapshot.coordinate_space,
        match_count,
        returned_count: matches.len(),
        truncated: snapshot.truncated || match_count > matches.len(),
        matches,
    };
    response.to_value_json()
}

pub fn build_ax_get_response_json(
    snapshot: &AxSnapshot,
    request: &AxGetRequest,
) -> io::Result<String> {
    let target_id = resolve_target_id_in_snapshot(snapshot, &request.target)?;

    for window in &snapshot.windows {
        if window.id == target_id {
            return AxGetResponse {
                kind: "ax-get",
                schema: AX_SCHEMA,
                platform: snapshot.platform.clone(),
                capture_status: snapshot.capture_status.clone(),
                permission_status: snapshot.permission_status.clone(),
                coordinate_space: snapshot.coordinate_space,
                target_id,
                target_type: "window",
                window_id: Some(window.id.clone()),
                pid: Some(window.pid),
                process_name: Some(window.process_name.clone()),
                window_title: window.title.clone(),
                window: Some(window.clone()),
                element: None,
            }
            .to_value_json();
        }

        if let Some(element) = find_element_by_id(&window.elements, &target_id) {
            return AxGetResponse {
                kind: "ax-get",
                schema: AX_SCHEMA,
                platform: snapshot.platform.clone(),
                capture_status: snapshot.capture_status.clone(),
                permission_status: snapshot.permission_status.clone(),
                coordinate_space: snapshot.coordinate_space,
                target_id,
                target_type: "element",
                window_id: Some(window.id.clone()),
                pid: Some(window.pid),
                process_name: Some(window.process_name.clone()),
                window_title: window.title.clone(),
                window: None,
                element: Some(element.clone()),
            }
            .to_value_json();
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("@ax-get target id 已失效或不存在: {target_id}"),
    ))
}

fn collect_matches(
    window: &AxWindow,
    query: &AxFindQuery,
    limit: u16,
    match_count: &mut usize,
    matches: &mut Vec<AxFindMatch>,
) {
    for element in &window.elements {
        collect_element_matches(window, element, query, limit, match_count, matches);
    }
}

fn collect_element_matches(
    window: &AxWindow,
    element: &AxElement,
    query: &AxFindQuery,
    limit: u16,
    match_count: &mut usize,
    matches: &mut Vec<AxFindMatch>,
) {
    if matches_query(window, element, query) {
        *match_count += 1;
        if matches.len() < usize::from(limit) {
            matches.push(AxFindMatch {
                id: element.id.clone(),
                window_id: window.id.clone(),
                pid: window.pid,
                process_name: window.process_name.clone(),
                window_title: window.title.clone(),
                role: element.role.clone(),
                subrole: element.subrole.clone(),
                name: element.name.clone(),
                value: element.value.clone(),
                value_redacted: element.value_redacted,
                description: element.description.clone(),
                rect: element.rect,
                enabled: element.enabled,
                actions: element.actions.clone(),
                ax_path: element.ax_path.clone(),
            });
        }
    }

    for child in &element.children {
        collect_element_matches(window, child, query, limit, match_count, matches);
    }
}

fn matches_query(window: &AxWindow, element: &AxElement, query: &AxFindQuery) -> bool {
    matches_exact(&query.process, Some(window.process_name.as_str()))
        && matches_contains(&query.process_contains, Some(window.process_name.as_str()))
        && matches_exact(&query.window_title, window.title.as_deref())
        && matches_contains(&query.window_title_contains, window.title.as_deref())
        && matches_exact(&query.role, Some(element.role.as_str()))
        && matches_exact(&query.subrole, element.subrole.as_deref())
        && matches_exact(&query.name, element.name.as_deref())
        && matches_contains(&query.name_contains, element.name.as_deref())
        && matches_exact(&query.description, element.description.as_deref())
        && matches_contains(&query.description_contains, element.description.as_deref())
        && matches_exact(&query.value, element.value.as_deref())
        && matches_contains(&query.value_contains, element.value.as_deref())
        && matches_action(&query.action, &element.actions)
}

fn matches_exact(expected: &Option<String>, actual: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual == Some(expected.as_str()),
        None => true,
    }
}

fn matches_contains(expected: &Option<String>, actual: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual
            .map(|actual| {
                actual
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase())
            })
            .unwrap_or(false),
        None => true,
    }
}

fn matches_action(expected: &Option<String>, actions: &[String]) -> bool {
    match expected {
        Some(expected) => actions.iter().any(|action| action == expected),
        None => true,
    }
}

fn find_element_by_id<'a>(elements: &'a [AxElement], target_id: &str) -> Option<&'a AxElement> {
    for element in elements {
        if element.id == target_id {
            return Some(element);
        }
        if let Some(found) = find_element_by_id(&element.children, target_id) {
            return Some(found);
        }
    }
    None
}

fn parse_limit(input: &str) -> io::Result<u16> {
    let limit = input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("@ax-find 的 `limit` 必须是无符号整数: {input}")))?;
    if limit == 0 {
        return Err(invalid_data("@ax-find 的 `limit` 必须大于 0"));
    }
    Ok(limit)
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn target_id_path_depth(target_id: &str) -> Option<u8> {
    let path = target_id
        .split('/')
        .find_map(|part| part.strip_prefix("path:"))?;
    let depth = path.split('.').count();
    u8::try_from(depth).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(id: &str, name: &str) -> AxElement {
        AxElement {
            id: id.to_owned(),
            role: "AXButton".to_owned(),
            subrole: None,
            name: Some(name.to_owned()),
            value: None,
            value_redacted: false,
            description: None,
            rect: Some(AxRect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }),
            enabled: Some(true),
            actions: vec!["AXPress".to_owned()],
            ax_path: vec![0],
            children: Vec::new(),
        }
    }

    fn snapshot() -> AxSnapshot {
        AxSnapshot::complete(
            "macos",
            vec![AxWindow {
                id: "pid:1/window:0".to_owned(),
                pid: 1,
                process_name: "Terminal".to_owned(),
                title: Some("rdog".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: Some(true),
                elements: vec![button("pid:1/window:0/path:0", "Cancel")],
            }],
            false,
        )
    }

    #[test]
    fn parse_ax_find_payload_should_use_light_mode_defaults() {
        let request = parse_ax_find_payload(r#"{role:"AXButton",name_contains:"can"}"#).unwrap();
        assert_eq!(request.tree.depth, AxMode::Interactive.preset().depth);
        assert_eq!(
            request.tree.max_elements,
            AxMode::Interactive.preset().max_elements
        );
        assert!(!request.tree.include_values);
        assert_eq!(request.limit, DEFAULT_AX_FIND_LIMIT);
        assert_eq!(request.query.name_contains.as_deref(), Some("can"));
    }

    #[test]
    fn find_response_should_return_compact_matches() {
        let request =
            parse_ax_find_payload(r#"{role:"AXButton",name_contains:"cancel",limit:5}"#).unwrap();
        let value = build_ax_find_response_json(&snapshot(), &request).unwrap();
        assert!(value.contains(r#""kind":"ax-find""#));
        assert!(value.contains(r#""match_count":1"#));
        assert!(value.contains(r#""name":"Cancel""#));
    }

    #[test]
    fn get_response_should_return_requested_window_or_element() {
        let request =
            parse_ax_get_payload(r#"{target:{id:"pid:1/window:0"},mode:"windows"}"#).unwrap();
        let value = build_ax_get_response_json(&snapshot(), &request).unwrap();
        assert!(value.contains(r#""target_type":"window""#));

        let request =
            parse_ax_get_payload(r#"{target:{id:"pid:1/window:0/path:0"},depth:1}"#).unwrap();
        let value = build_ax_get_response_json(&snapshot(), &request).unwrap();
        assert!(value.contains(r#""target_type":"element""#));
        assert!(value.contains(r#""name":"Cancel""#));
    }

    #[test]
    fn get_request_should_capture_deep_enough_for_target_path() {
        let request =
            parse_ax_get_payload(r#"{target:{id:"pid:1/window:0/path:1.2.3"},depth:2}"#).unwrap();
        assert_eq!(request.tree_request().depth, 5);
    }
}
