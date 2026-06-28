use crate::{
    control_ax::{
        capture_current_ax_subtree, capture_default_ax_snapshot, AxCapturedSubtree, AxElement,
        AxRect, AxSnapshot, AxTreeRequest, AxWindow,
    },
    control_display_scope::{
        display_intersects_rect, display_scope_report, parse_display_scope, resolve_display_scope,
        resolve_observation_window_ref, DisplayRect, DisplayScope, DisplaySelector,
    },
    control_observation::resolve_observation_ref,
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields,
    },
    screenshot::current_display_summaries,
};
use serde::Serialize;
use std::{borrow::Cow, collections::HashSet, io};

mod act;
mod parse;

pub use self::act::{build_default_web_act_response_json, parse_web_act_payload, WebActRequest};
pub use self::parse::parse_web_find_payload;
pub(super) use self::parse::{parse_web_find_match, parse_web_find_target};

pub const WEB_FIND_SCHEMA: &str = "rdog.web-find.v1";
const DEFAULT_WEB_FIND_LIMIT: u16 = 20;
const DEFAULT_WEB_FIND_DEPTH: u8 = 8;
const DEFAULT_WEB_FIND_MAX_ELEMENTS: u16 = 2000;
const DEFAULT_WEB_FIND_INCLUDE_VALUES: bool = true;

const DEFAULT_WEB_FIND_ROLES: &[&str] = &["AXLink", "AXButton", "AXMenuButton", "AXGroup"];
const BROWSER_PROCESS_NAMES: &[&str] = &[
    "Google Chrome",
    "Chrome",
    "Safari",
    "Safari Technology Preview",
    "Arc",
    "Microsoft Edge",
    "Brave Browser",
    "Firefox",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFindRequest {
    pub target: WebFindTarget,
    pub query: WebFindQuery,
    pub display_scope: Option<DisplayScope>,
    pub roles: Vec<String>,
    pub limit: u16,
    pub depth: u8,
    pub max_elements: u16,
    pub include_values: bool,
}

impl WebFindRequest {
    pub fn tree_request(&self) -> AxTreeRequest {
        AxTreeRequest {
            depth: self.depth,
            max_elements: self.max_elements,
            include_values: self.include_values,
            ..AxTreeRequest::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFindTarget {
    pub browser: WebFindBrowserTarget,
    pub app: Option<String>,
    pub window_id: Option<String>,
    pub window_ref: Option<String>,
    pub observation_id: Option<String>,
    pub window_title_contains: Option<String>,
}

impl Default for WebFindTarget {
    fn default() -> Self {
        Self {
            browser: WebFindBrowserTarget::Active,
            app: None,
            window_id: None,
            window_ref: None,
            observation_id: None,
            window_title_contains: None,
        }
    }
}

impl WebFindTarget {
    fn scope_str(&self) -> &'static str {
        if self.window_id.is_some() || self.window_ref.is_some() {
            "target_window_web_area"
        } else {
            "active_web_area"
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WebFindBrowserTarget {
    Active,
}

impl WebFindBrowserTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFindQuery {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct WebFindResponse {
    kind: &'static str,
    schema: &'static str,
    status: &'static str,
    scope: &'static str,
    platform: String,
    capture_status: String,
    permission_status: String,
    coordinate_space: &'static str,
    target: WebFindTargetReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<crate::control_observation::ObservationHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_scope: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<WebFindWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    web_area: Option<WebFindArea>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    match_count: usize,
    returned_count: usize,
    truncated: bool,
    matches: Vec<WebFindMatch>,
    trace: Vec<WebFindTraceStep>,
}

impl WebFindResponse {
    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("web-find response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WebFindTargetReport {
    browser: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_title_contains: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WebFindWindow {
    window_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    ref_id: Option<String>,
    pid: i32,
    process_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focused: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WebFindArea {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    ref_id: Option<String>,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rect: Option<AxRect>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct WebFindMatch {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    ref_id: Option<String>,
    window_id: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    value_redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    matched_field: &'static str,
    matched_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rect: Option<AxRect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    actions: Vec<String>,
    ax_path: Vec<usize>,
    confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WebFindTraceStep {
    step: &'static str,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub fn build_default_web_find_response_json(request: &WebFindRequest) -> io::Result<String> {
    let snapshot = capture_default_ax_snapshot(&request.tree_request())?;
    build_web_find_response_json_with_refresh(&snapshot, request, |target_id, tree_request| {
        capture_current_ax_subtree(target_id, tree_request).map(Some)
    })
}

#[cfg(test)]
fn build_web_find_response_json(
    snapshot: &AxSnapshot,
    request: &WebFindRequest,
) -> io::Result<String> {
    build_web_find_response_json_with_refresh(snapshot, request, |_target_id, _tree_request| {
        Ok(None)
    })
}

fn build_web_find_response_json_with_refresh<F>(
    snapshot: &AxSnapshot,
    request: &WebFindRequest,
    mut refresh_web_area: F,
) -> io::Result<String>
where
    F: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    let mut trace = vec![trace_step(
        "capture-ax-snapshot",
        snapshot.capture_status.clone(),
        None,
    )];
    let snapshot = snapshot.clone().with_observation("@web-find")?;

    if snapshot.capture_status != "complete" {
        return response_with_status(
            &snapshot,
            request,
            "blocked",
            Some("AX_SNAPSHOT_UNAVAILABLE"),
            Some("AX snapshot 不可用,无法执行 @web-find".to_owned()),
            None,
            None,
            None,
            Vec::new(),
            trace,
            0,
            snapshot.truncated,
        );
    }

    let resolution = resolve_web_matches(&snapshot, request, &mut trace, &mut refresh_web_area);
    let WebMatchResolution::Resolved {
        selected_window,
        web_area,
        matches,
        match_count,
        truncated,
        display_scope,
    } = resolution
    else {
        match resolution {
            WebMatchResolution::BrowserWindowNotFound(message) => {
                return response_with_status(
                    &snapshot,
                    request,
                    "not_found",
                    Some("BROWSER_WINDOW_NOT_FOUND"),
                    Some(message),
                    None,
                    None,
                    None,
                    Vec::new(),
                    trace,
                    0,
                    snapshot.truncated,
                )
            }
            WebMatchResolution::BrowserWindowAmbiguous(message) => {
                return response_with_status(
                    &snapshot,
                    request,
                    "needs_disambiguation",
                    Some("BROWSER_WINDOW_AMBIGUOUS"),
                    Some(message),
                    None,
                    None,
                    None,
                    Vec::new(),
                    trace,
                    0,
                    snapshot.truncated,
                )
            }
            WebMatchResolution::BrowserWindowRefInvalid(message) => {
                return response_with_status(
                    &snapshot,
                    request,
                    "blocked",
                    Some("WINDOW_REF_INVALID"),
                    Some(message),
                    None,
                    None,
                    None,
                    Vec::new(),
                    trace,
                    0,
                    snapshot.truncated,
                )
            }
            WebMatchResolution::DisplayScopeInvalid(message) => {
                return response_with_status(
                    &snapshot,
                    request,
                    "blocked",
                    Some("DISPLAY_SCOPE_INVALID"),
                    Some(message),
                    None,
                    None,
                    None,
                    Vec::new(),
                    trace,
                    0,
                    snapshot.truncated,
                )
            }
            WebMatchResolution::WebAreaNotFound { selected_window } => {
                return response_with_status(
                    &snapshot,
                    request,
                    "not_found",
                    Some("AX_WEB_AREA_NOT_FOUND"),
                    Some("目标浏览器窗口里没有找到 AXWebArea".to_owned()),
                    None,
                    Some(selected_window),
                    None,
                    Vec::new(),
                    trace,
                    0,
                    snapshot.truncated,
                )
            }
            WebMatchResolution::Resolved { .. } => unreachable!("resolved branch already handled"),
        }
    };

    let status = if match_count == 0 {
        "not_found"
    } else {
        "complete"
    };
    let error_code = if match_count == 0 {
        Some("WEB_MATCH_NOT_FOUND")
    } else {
        None
    };
    let message = if match_count == 0 {
        Some(format!(
            "AXWebArea 内没有找到文本匹配 `{}` 的 page-owned 控件",
            request.query.text
        ))
    } else {
        None
    };

    response_with_status(
        &snapshot,
        request,
        status,
        error_code,
        message,
        display_scope,
        Some(selected_window),
        Some(web_area.as_ref()),
        matches,
        trace,
        match_count,
        truncated,
    )
}

enum WebMatchResolution<'a> {
    Resolved {
        selected_window: &'a AxWindow,
        web_area: Cow<'a, AxElement>,
        matches: Vec<WebFindMatch>,
        match_count: usize,
        truncated: bool,
        display_scope: Option<serde_json::Value>,
    },
    BrowserWindowNotFound(String),
    BrowserWindowAmbiguous(String),
    BrowserWindowRefInvalid(String),
    DisplayScopeInvalid(String),
    WebAreaNotFound {
        selected_window: &'a AxWindow,
    },
}

fn resolve_web_matches<'a, F>(
    snapshot: &'a AxSnapshot,
    request: &WebFindRequest,
    trace: &mut Vec<WebFindTraceStep>,
    refresh_web_area: &mut F,
) -> WebMatchResolution<'a>
where
    F: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    let (selected_window, display_scope) = match select_target_window(snapshot, request, trace) {
        WindowSelection::Selected {
            window,
            display_scope,
        } => (window, display_scope),
        WindowSelection::NotFound(message) => {
            return WebMatchResolution::BrowserWindowNotFound(message);
        }
        WindowSelection::Ambiguous(message) => {
            return WebMatchResolution::BrowserWindowAmbiguous(message);
        }
        WindowSelection::InvalidRef(message) => {
            return WebMatchResolution::BrowserWindowRefInvalid(message);
        }
        WindowSelection::DisplayScopeInvalid(message) => {
            return WebMatchResolution::DisplayScopeInvalid(message);
        }
    };

    let Some(web_area) = find_web_area(&selected_window.elements) else {
        trace.push(trace_step("find-ax-web-area", "not_found", None));
        return WebMatchResolution::WebAreaNotFound { selected_window };
    };
    trace.push(trace_step(
        "find-ax-web-area",
        "ok",
        Some(web_area.id.clone()),
    ));

    let mut matches = Vec::new();
    let mut seen_target_ids = HashSet::new();
    let mut match_count = 0usize;
    collect_web_matches(
        selected_window,
        web_area,
        request,
        &mut match_count,
        &mut matches,
        &mut seen_target_ids,
        &[],
    );
    trace.push(trace_step(
        "match-page-content",
        if match_count == 0 { "not_found" } else { "ok" },
        Some(format!("match_count={match_count}")),
    ));

    if match_count == 0 {
        if let Some((refreshed_web_area, refreshed_matches, refreshed_count, refreshed_truncated)) =
            refresh_web_area_matches(selected_window, web_area, request, trace, refresh_web_area)
        {
            return WebMatchResolution::Resolved {
                selected_window,
                web_area: Cow::Owned(refreshed_web_area),
                truncated: snapshot.truncated
                    || refreshed_truncated
                    || refreshed_count > refreshed_matches.len(),
                matches: refreshed_matches,
                match_count: refreshed_count,
                display_scope,
            };
        }
    }

    WebMatchResolution::Resolved {
        selected_window,
        web_area: Cow::Borrowed(web_area),
        truncated: snapshot.truncated || match_count > matches.len(),
        matches,
        match_count,
        display_scope,
    }
}

fn refresh_web_area_matches<F>(
    selected_window: &AxWindow,
    web_area: &AxElement,
    request: &WebFindRequest,
    trace: &mut Vec<WebFindTraceStep>,
    refresh_web_area: &mut F,
) -> Option<(AxElement, Vec<WebFindMatch>, usize, bool)>
where
    F: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    let tree_request = request.tree_request();
    trace.push(trace_step(
        "refresh-web-area-subtree",
        "attempt",
        Some(web_area.id.clone()),
    ));

    let refreshed = match refresh_web_area(&web_area.id, &tree_request) {
        Ok(Some(refreshed)) => refreshed,
        Ok(None) => {
            trace.push(trace_step("refresh-web-area-subtree", "skipped", None));
            return None;
        }
        Err(err) => {
            trace.push(trace_step(
                "refresh-web-area-subtree",
                "error",
                Some(err.to_string()),
            ));
            return None;
        }
    };

    let mut matches = Vec::new();
    let mut seen_target_ids = HashSet::new();
    let mut match_count = 0usize;
    collect_web_matches(
        selected_window,
        &refreshed.element,
        request,
        &mut match_count,
        &mut matches,
        &mut seen_target_ids,
        &[],
    );
    trace.push(trace_step(
        "refresh-web-area-subtree",
        if match_count == 0 { "not_found" } else { "ok" },
        Some(format!("match_count={match_count}")),
    ));

    Some((refreshed.element, matches, match_count, refreshed.truncated))
}

fn response_with_status(
    snapshot: &AxSnapshot,
    request: &WebFindRequest,
    status: &'static str,
    error_code: Option<&'static str>,
    message: Option<String>,
    display_scope: Option<serde_json::Value>,
    window: Option<&AxWindow>,
    web_area: Option<&AxElement>,
    matches: Vec<WebFindMatch>,
    trace: Vec<WebFindTraceStep>,
    match_count: usize,
    truncated: bool,
) -> io::Result<String> {
    let returned_count = matches.len();
    let response = WebFindResponse {
        kind: "web-find",
        schema: WEB_FIND_SCHEMA,
        status,
        scope: request.target.scope_str(),
        platform: snapshot.platform.clone(),
        capture_status: snapshot.capture_status.clone(),
        permission_status: snapshot.permission_status.clone(),
        coordinate_space: snapshot.coordinate_space,
        target: WebFindTargetReport {
            browser: request.target.browser.as_str(),
            app: request.target.app.clone(),
            window_id: request.target.window_id.clone(),
            window_ref: request.target.window_ref.clone(),
            observation_id: request.target.observation_id.clone(),
            window_title_contains: request.target.window_title_contains.clone(),
        },
        observation: snapshot.observation.clone(),
        display_scope,
        window: window.map(web_find_window),
        web_area: web_area.map(web_find_area),
        error_code,
        message,
        match_count,
        returned_count,
        truncated,
        matches,
        trace,
    };
    response.to_value_json()
}

fn trace_step(
    step: &'static str,
    status: impl Into<String>,
    detail: Option<String>,
) -> WebFindTraceStep {
    WebFindTraceStep {
        step,
        status: status.into(),
        detail,
    }
}

enum WindowSelection<'a> {
    Selected {
        window: &'a AxWindow,
        display_scope: Option<serde_json::Value>,
    },
    NotFound(String),
    Ambiguous(String),
    InvalidRef(String),
    DisplayScopeInvalid(String),
}

fn select_target_window<'a>(
    snapshot: &'a AxSnapshot,
    request: &WebFindRequest,
    trace: &mut Vec<WebFindTraceStep>,
) -> WindowSelection<'a> {
    let explicit_window_id = match resolve_target_window_id(&request.target, trace) {
        Ok(window_id) => window_id,
        Err(message) => return WindowSelection::InvalidRef(message),
    };
    let mut candidates = snapshot
        .windows
        .iter()
        .filter(|window| matches_web_target(window, &request.target, explicit_window_id.as_deref()))
        .collect::<Vec<_>>();
    let before_display_scope = candidates.len();
    let mut display_scope = None;
    if let Some(scope) = request.display_scope.as_ref() {
        let displays = match current_display_summaries() {
            Ok(displays) => displays,
            Err(err) => return WindowSelection::DisplayScopeInvalid(err.to_string()),
        };
        let resolution = match resolve_display_scope(scope, &displays, |selector| {
            web_window_rect_for_display_selector(snapshot, selector)
        }) {
            Ok(resolution) => resolution,
            Err(err) => return WindowSelection::DisplayScopeInvalid(err.to_string()),
        };
        candidates.retain(|window| {
            window
                .rect
                .map(DisplayRect::from)
                .map(|rect| display_intersects_rect(&resolution.resolved, rect))
                .unwrap_or(false)
        });
        trace.push(trace_step(
            "display-scope",
            "applied",
            Some(format!(
                "resolved_display_id={},matched_before_filter={},matched_after_filter={}",
                resolution.resolved.display_id,
                before_display_scope,
                candidates.len()
            )),
        ));
        let mut report = display_scope_report(&resolution);
        report["matched_before_filter"] = serde_json::json!(before_display_scope);
        report["matched_after_filter"] = serde_json::json!(candidates.len());
        display_scope = Some(report);
    }

    if candidates.is_empty() {
        if let Some(window_id) = explicit_window_id.as_deref() {
            trace.push(trace_step(
                "target-browser-window",
                "not_found",
                Some(window_id.to_owned()),
            ));
            return WindowSelection::NotFound(format!(
                "没有找到指定窗口 `{window_id}` 对应的 browser AXWindow"
            ));
        }

        trace.push(trace_step("active-browser-window", "not_found", None));
        return WindowSelection::NotFound("没有找到匹配 target 的 browser AXWindow".to_owned());
    }

    if let Some(window_id) = explicit_window_id.as_deref() {
        let window = candidates[0];
        trace.push(trace_step(
            "target-browser-window",
            "ok",
            Some(window.id.clone()),
        ));
        if candidates.len() > 1 {
            trace.push(trace_step(
                "target-browser-window",
                "duplicate_id_ignored",
                Some(format!(
                    "window_id={window_id},candidate_count={}",
                    candidates.len()
                )),
            ));
        }
        return WindowSelection::Selected {
            window,
            display_scope,
        };
    }

    let focused = candidates
        .iter()
        .copied()
        .filter(|window| window.focused == Some(true))
        .collect::<Vec<_>>();
    match focused.as_slice() {
        [window] => {
            trace.push(trace_step(
                "active-browser-window",
                "ok",
                Some(window.id.clone()),
            ));
            WindowSelection::Selected {
                window,
                display_scope,
            }
        }
        [] if candidates.len() == 1 => {
            let window = candidates[0];
            trace.push(trace_step(
                "active-browser-window",
                "ok",
                Some(window.id.clone()),
            ));
            WindowSelection::Selected {
                window,
                display_scope,
            }
        }
        [] => {
            trace.push(trace_step(
                "active-browser-window",
                "needs_disambiguation",
                Some(format!("candidate_count={}", candidates.len())),
            ));
            WindowSelection::Ambiguous(format!(
                "找到 {} 个 browser 窗口,但没有唯一 focused 窗口",
                candidates.len()
            ))
        }
        _ => {
            trace.push(trace_step(
                "active-browser-window",
                "needs_disambiguation",
                Some(format!("focused_count={}", focused.len())),
            ));
            WindowSelection::Ambiguous(format!("找到 {} 个 focused browser 窗口", focused.len()))
        }
    }
}

fn web_window_rect_for_display_selector(
    snapshot: &AxSnapshot,
    selector: &DisplaySelector,
) -> io::Result<Option<DisplayRect>> {
    web_window_rect_for_display_selector_from_windows(&snapshot.windows, selector)
}

fn web_window_rect_for_display_selector_from_windows(
    windows: &[AxWindow],
    selector: &DisplaySelector,
) -> io::Result<Option<DisplayRect>> {
    let window_id = match selector {
        DisplaySelector::WindowId(window_id) => window_id.clone(),
        DisplaySelector::WindowRef {
            observation_id,
            ref_id,
        } => resolve_observation_window_ref(observation_id, ref_id)?.window_id,
        _ => return Ok(None),
    };
    Ok(windows
        .iter()
        .find(|window| window.id == window_id)
        .and_then(|window| window.rect)
        .map(DisplayRect::from))
}

fn matches_web_target(
    window: &AxWindow,
    target: &WebFindTarget,
    explicit_window_id: Option<&str>,
) -> bool {
    let window_id_matches = explicit_window_id
        .map(|window_id| window.id == window_id)
        .unwrap_or(true);
    let app_matches = target
        .app
        .as_ref()
        .map(|app| window.process_name == *app)
        .unwrap_or_else(|| is_known_browser_process(&window.process_name));
    let title_matches = target
        .window_title_contains
        .as_ref()
        .map(|needle| contains_text(window.title.as_deref(), needle))
        .unwrap_or(true);
    window_id_matches && app_matches && title_matches
}

fn resolve_target_window_id(
    target: &WebFindTarget,
    trace: &mut Vec<WebFindTraceStep>,
) -> Result<Option<String>, String> {
    if let Some(window_id) = target.window_id.as_ref() {
        return Ok(Some(window_id.clone()));
    }

    let Some(window_ref) = target.window_ref.as_ref() else {
        return Ok(None);
    };
    let observation_id = target
        .observation_id
        .as_deref()
        .ok_or_else(|| "target.window_ref 缺少配套 observation_id,无法解析窗口 ref".to_owned())?;
    trace.push(trace_step(
        "resolve-window-ref",
        "attempt",
        Some(format!("observation_id={observation_id},ref={window_ref}")),
    ));

    let entry = resolve_observation_ref(observation_id, window_ref).map_err(|err| {
        trace.push(trace_step(
            "resolve-window-ref",
            "error",
            Some(err.to_string()),
        ));
        format!("target.window_ref 无法解析: {err}")
    })?;
    if entry.kind != "window" {
        trace.push(trace_step(
            "resolve-window-ref",
            "blocked",
            Some(format!(
                "kind={},backend_id={}",
                entry.kind, entry.backend_id
            )),
        ));
        return Err(format!(
            "target.window_ref `{window_ref}` 指向 `{}` ref,不是 window ref",
            entry.kind
        ));
    }

    trace.push(trace_step(
        "resolve-window-ref",
        "ok",
        Some(entry.backend_id.clone()),
    ));
    Ok(Some(entry.backend_id))
}

fn is_known_browser_process(process_name: &str) -> bool {
    BROWSER_PROCESS_NAMES
        .iter()
        .any(|browser| process_name.eq_ignore_ascii_case(browser))
}

fn find_web_area(elements: &[AxElement]) -> Option<&AxElement> {
    for element in elements {
        if element.role == "AXWebArea" {
            return Some(element);
        }
        if let Some(found) = find_web_area(&element.children) {
            return Some(found);
        }
    }
    None
}

fn collect_web_matches(
    window: &AxWindow,
    element: &AxElement,
    request: &WebFindRequest,
    match_count: &mut usize,
    matches: &mut Vec<WebFindMatch>,
    seen_target_ids: &mut HashSet<String>,
    ancestors: &[&AxElement],
) {
    if let Some(text_match) = match_text(element, &request.query.text) {
        if let Some(target) = resolve_actionable_target(element, ancestors, &request.roles) {
            if seen_target_ids.insert(target.id.clone()) {
                *match_count += 1;
                if matches.len() < usize::from(request.limit) {
                    matches.push(web_find_match(window, target, element, text_match));
                }
            }
        }
    }

    let mut next_ancestors = ancestors.to_vec();
    next_ancestors.push(element);
    for child in &element.children {
        collect_web_matches(
            window,
            child,
            request,
            match_count,
            matches,
            seen_target_ids,
            &next_ancestors,
        );
    }
}

fn resolve_actionable_target<'a>(
    element: &'a AxElement,
    ancestors: &[&'a AxElement],
    roles: &[String],
) -> Option<&'a AxElement> {
    if is_allowed_target(element, roles) {
        return Some(element);
    }

    ancestors
        .iter()
        .rev()
        .copied()
        .find(|ancestor| is_allowed_target(ancestor, roles))
}

fn is_allowed_target(element: &AxElement, roles: &[String]) -> bool {
    roles.iter().any(|role| role == &element.role)
        || element.actions.iter().any(|action| action == "AXPress")
}

#[derive(Debug, Copy, Clone)]
struct MatchedText<'a> {
    field: &'static str,
    text: &'a str,
}

fn match_text<'a>(element: &'a AxElement, needle: &str) -> Option<MatchedText<'a>> {
    [
        ("description", element.description.as_deref()),
        ("name", element.name.as_deref()),
        ("value", element.value.as_deref()),
    ]
    .into_iter()
    .find_map(|(field, value)| {
        value
            .filter(|value| contains_text(Some(value), needle))
            .map(|text| MatchedText { field, text })
    })
}

fn contains_text(actual: Option<&str>, needle: &str) -> bool {
    actual
        .map(|actual| actual.to_lowercase().contains(&needle.to_lowercase()))
        .unwrap_or(false)
}

fn web_find_window(window: &AxWindow) -> WebFindWindow {
    WebFindWindow {
        window_id: window.id.clone(),
        ref_id: window.ref_id.clone(),
        pid: window.pid,
        process_name: window.process_name.clone(),
        title: window.title.clone(),
        role: window.role.clone(),
        rect: window.rect,
        focused: window.focused,
    }
}

fn web_find_area(element: &AxElement) -> WebFindArea {
    WebFindArea {
        id: element.id.clone(),
        ref_id: element.ref_id.clone(),
        role: element.role.clone(),
        rect: element.rect,
    }
}

fn web_find_match(
    window: &AxWindow,
    target: &AxElement,
    source: &AxElement,
    text_match: MatchedText<'_>,
) -> WebFindMatch {
    WebFindMatch {
        id: target.id.clone(),
        ref_id: target.ref_id.clone(),
        window_id: window.id.clone(),
        role: target.role.clone(),
        name: target.name.clone(),
        value: target.value.clone(),
        value_redacted: target.value_redacted,
        description: target.description.clone(),
        matched_field: text_match.field,
        matched_text: text_match.text.to_owned(),
        matched_source_id: (target.id != source.id).then(|| source.id.clone()),
        rect: target.rect,
        enabled: target.enabled,
        actions: target.actions.clone(),
        ax_path: target.ax_path.clone(),
        confidence: if target.id == source.id { 0.98 } else { 0.9 },
    }
}

fn parse_string_array(input: &str, kind: &str) -> io::Result<Vec<String>> {
    let inner = input
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| invalid_data(format!("{kind} 必须是字符串数组: {input}")))?
        .trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    split_object_fields(inner)?
        .into_iter()
        .map(|value| parse_non_empty_string(kind, value))
        .collect()
}

fn parse_bool(input: &str, kind: &str, field_name: &str) -> io::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_data(format!(
            "{kind} 的 `{field_name}` 必须是 true 或 false: {input}"
        ))),
    }
}

fn parse_limit(input: &str) -> io::Result<u16> {
    let value = parse_u16(input, "limit")?;
    if value == 0 {
        return Err(invalid_data("@web-find.limit 必须大于 0"));
    }
    Ok(value)
}

fn parse_u8(input: &str, field_name: &str) -> io::Result<u8> {
    input
        .parse::<u8>()
        .map_err(|_| invalid_data(format!("@web-find.{field_name} 必须是无符号整数: {input}")))
}

fn parse_u16(input: &str, field_name: &str) -> io::Result<u16> {
    input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("@web-find.{field_name} 必须是无符号整数: {input}")))
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn required_field<T>(value: Option<T>, kind: &str, field_name: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{kind} 缺少 `{field_name}` 字段")))
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!("{kind} 的 `{field_name}` 字段重复")));
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
