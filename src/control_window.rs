use crate::{
    control_ax::AxRect,
    control_observation::selector::{
        AppSelector, DurableSelectorDraft, SelectorEnvelope, SelectorKind, SelectorRect,
        SelectorRedaction, WindowSelector,
    },
    control_observation::{
        observation_ref_name, record_observation_with_selectors, ObservationHeader,
        ObservationRefEntry, ObservationRoot,
    },
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields,
    },
};
use serde::Serialize;
use serde_json::json;
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

pub const WINDOW_SCHEMA: &str = "rdog.window.v1";
pub const WINDOW_COORDINATE_SPACE: &str = "os-logical";
const DEFAULT_WINDOW_FIND_LIMIT: u16 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowFindRequest {
    pub query: WindowQuery,
    pub limit: u16,
    pub include_state: bool,
    pub include_recipes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowActivateRequest {
    pub target: WindowCommandTarget,
    pub recipe: Option<String>,
    pub steps: Vec<String>,
    pub allow_ambiguous: bool,
    pub select: Option<WindowSelectPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCloseRequest {
    pub target: WindowCommandTarget,
    pub strategy: WindowCloseStrategy,
    pub allow_ambiguous: bool,
    pub select: Option<WindowSelectPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowCommandTarget {
    pub window_id: Option<String>,
    pub ref_id: Option<String>,
    pub observation_id: Option<String>,
    pub query: WindowQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowQuery {
    pub app: Option<String>,
    pub app_contains: Option<String>,
    pub bundle_id: Option<String>,
    pub pid: Option<i32>,
    pub title: Option<String>,
    pub title_contains: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WindowCloseStrategy {
    Graceful,
    Terminate,
    Kill,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WindowSelectPolicy {
    Frontmost,
    First,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowFindResponse {
    pub kind: &'static str,
    pub schema: &'static str,
    pub platform: String,
    pub status: String,
    pub capabilities: WindowCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<ObservationHeader>,
    pub match_count: usize,
    pub returned_count: usize,
    pub snapshot_id: String,
    pub observed_at_unix_ms: u64,
    pub matches: Vec<WindowCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowActionReport {
    pub kind: &'static str,
    pub schema: &'static str,
    pub platform: String,
    pub action: &'static str,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination_attempted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    pub steps: Vec<WindowActionStepReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowCapabilities {
    pub find: String,
    pub activate: String,
    pub close: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_switch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowCandidate {
    pub window_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_id: Option<String>,
    pub locator_lifetime: &'static str,
    pub app: WindowAppDescriptor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<AxRect>,
    pub coordinate_space: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<WindowState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipes: Option<WindowRecipes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowAppDescriptor {
    pub name: String,
    pub pid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    pub hidden: bool,
    pub frontmost: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowState {
    pub occluded: bool,
    pub minimized: bool,
    pub app_hidden: bool,
    pub current_space: bool,
    pub fullscreen_space: bool,
    pub interactable: bool,
    pub confidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowRecipes {
    pub to_interact: Vec<&'static str>,
    pub to_close_gracefully: Vec<&'static str>,
    pub to_force_close: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowActionStepReport {
    pub step: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshotMeta {
    pub snapshot_id: String,
    pub observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowResolvedTargetRect {
    pub window_id: String,
    pub rect: Option<AxRect>,
}

impl WindowSnapshotMeta {
    pub fn now() -> Self {
        let observed_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            snapshot_id: format!("window-snapshot-{observed_at_unix_ms}"),
            observed_at_unix_ms,
        }
    }
}

impl WindowQuery {
    fn is_empty(&self) -> bool {
        self.app.is_none()
            && self.app_contains.is_none()
            && self.bundle_id.is_none()
            && self.pid.is_none()
            && self.title.is_none()
            && self.title_contains.is_none()
    }

    pub fn validate_for_find(&self) -> io::Result<()> {
        if self.is_empty() {
            return Err(invalid_data("@window-find 至少需要一个查询字段"));
        }
        Ok(())
    }

    pub fn validate_for_execute(&self, kind: &str) -> io::Result<()> {
        if self.is_empty() {
            return Err(invalid_data(format!(
                "{kind} 至少需要 `window_id`、`target.ref + observation_id` 或一个查询字段"
            )));
        }
        Ok(())
    }

    pub fn matches_candidate(&self, candidate: &WindowCandidate) -> bool {
        matches_optional(self.app.as_ref(), Some(candidate.app.name.as_str()))
            && matches_contains(
                self.app_contains.as_ref(),
                Some(candidate.app.name.as_str()),
            )
            && matches_optional(self.bundle_id.as_ref(), candidate.app.bundle_id.as_deref())
            && matches_pid(self.pid, candidate.app.pid)
            && matches_optional(self.title.as_ref(), candidate.title.as_deref())
            && matches_contains(self.title_contains.as_ref(), candidate.title.as_deref())
    }
}

impl WindowCommandTarget {
    fn validate_for_execute(&self, kind: &str) -> io::Result<()> {
        let has_window_id = self.window_id.is_some();
        let has_ref = self.ref_id.is_some();
        let has_observation_id = self.observation_id.is_some();
        let has_query = !self.query.is_empty();

        if has_window_id {
            if has_ref || has_observation_id || has_query {
                return Err(invalid_data(format!(
                    "{kind} target.window_id 不能与 ref / observation_id / query 混用"
                )));
            }
            return Ok(());
        }

        if has_ref || has_observation_id {
            if !has_ref || !has_observation_id {
                return Err(invalid_data(format!(
                    "{kind} target.ref 必须和 observation_id 一起出现"
                )));
            }
            if has_query {
                return Err(invalid_data(format!(
                    "{kind} target.ref 不能和 query locator 混用"
                )));
            }
            return Ok(());
        }
        self.query.validate_for_execute(kind)
    }
}

impl WindowCloseStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Terminate => "terminate",
            Self::Kill => "kill",
        }
    }

    fn from_literal(input: &str) -> io::Result<Self> {
        let value = parse_quoted_payload(input)?;
        match value.to_ascii_lowercase().as_str() {
            "graceful" => Ok(Self::Graceful),
            "terminate" => Ok(Self::Terminate),
            "kill" => Ok(Self::Kill),
            _ => Err(invalid_data(format!(
                "@window-close 的 `strategy` 只支持 \"graceful\" | \"terminate\" | \"kill\": {value}"
            ))),
        }
    }
}

impl WindowSelectPolicy {
    fn from_literal(input: &str) -> io::Result<Self> {
        let value = parse_quoted_payload(input)?;
        match value.to_ascii_lowercase().as_str() {
            "frontmost" => Ok(Self::Frontmost),
            "first" => Ok(Self::First),
            _ => Err(invalid_data(format!(
                "window select 只支持 \"frontmost\" | \"first\": {value}"
            ))),
        }
    }
}

impl WindowCapabilities {
    pub fn complete() -> Self {
        Self {
            find: "complete".to_owned(),
            activate: "complete".to_owned(),
            close: "complete".to_owned(),
            space_switch: Some("limited".to_owned()),
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn unsupported() -> Self {
        Self {
            find: "unsupported".to_owned(),
            activate: "unsupported".to_owned(),
            close: "unsupported".to_owned(),
            space_switch: None,
        }
    }
}

impl WindowFindResponse {
    #[cfg(not(target_os = "macos"))]
    pub fn unsupported(platform: impl Into<String>) -> Self {
        let meta = WindowSnapshotMeta::now();
        Self {
            kind: "window-find",
            schema: WINDOW_SCHEMA,
            platform: platform.into(),
            status: "unsupported".to_owned(),
            capabilities: WindowCapabilities::unsupported(),
            observation: None,
            match_count: 0,
            returned_count: 0,
            snapshot_id: meta.snapshot_id,
            observed_at_unix_ms: meta.observed_at_unix_ms,
            matches: Vec::new(),
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("window find response 序列化失败: {err}")))
    }
}

impl WindowActionReport {
    #[cfg(not(target_os = "macos"))]
    pub fn unsupported(action: &'static str, platform: impl Into<String>) -> Self {
        Self {
            kind: "window-action",
            schema: WINDOW_SCHEMA,
            platform: platform.into(),
            action,
            status: "unsupported".to_owned(),
            window_id: None,
            snapshot_id: None,
            observed_at_unix_ms: None,
            strategy: None,
            target_pid: None,
            process_scope: None,
            termination_attempted: None,
            failed_step: None,
            steps: Vec::new(),
        }
    }

    pub fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("window action response 序列化失败: {err}")))
    }
}

pub trait WindowBackend {
    fn find(&self, request: &WindowFindRequest) -> io::Result<WindowFindResponse>;
    fn activate(&self, request: &WindowActivateRequest) -> io::Result<WindowActionReport>;
    fn close(&self, request: &WindowCloseRequest) -> io::Result<WindowActionReport>;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct SystemWindowBackend;

impl WindowBackend for SystemWindowBackend {
    fn find(&self, request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
        platform_find(request)
    }

    fn activate(&self, request: &WindowActivateRequest) -> io::Result<WindowActionReport> {
        platform_activate(request)
    }

    fn close(&self, request: &WindowCloseRequest) -> io::Result<WindowActionReport> {
        platform_close(request)
    }
}

pub fn execute_default_window_find(request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
    SystemWindowBackend.find(request)
}

pub fn execute_default_window_activate(
    request: &WindowActivateRequest,
) -> io::Result<WindowActionReport> {
    SystemWindowBackend.activate(request)
}

pub fn execute_default_window_close(
    request: &WindowCloseRequest,
) -> io::Result<WindowActionReport> {
    SystemWindowBackend.close(request)
}

pub fn resolve_default_window_target_rect(
    target: &WindowCommandTarget,
) -> io::Result<WindowResolvedTargetRect> {
    target.validate_for_execute("@mouse target")?;
    platform_resolve_target_rect(target)
}

pub fn parse_window_find_payload(input: &str) -> io::Result<WindowFindRequest> {
    let inner = object_inner(input, "@window-find")?;
    if inner.is_empty() {
        return Err(invalid_data("@window-find 对象 payload 不能为空"));
    }

    let mut limit = None::<u16>;
    let mut include_state = None::<bool>;
    let mut include_recipes = None::<bool>;
    let mut query = WindowQuery::default();

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "limit" => assign_once(&mut limit, "limit", "@window-find", parse_limit(raw_value)?)?,
            "include_state" => assign_once(
                &mut include_state,
                "include_state",
                "@window-find",
                parse_bool(raw_value, "@window-find", "include_state")?,
            )?,
            "include_recipes" => assign_once(
                &mut include_recipes,
                "include_recipes",
                "@window-find",
                parse_bool(raw_value, "@window-find", "include_recipes")?,
            )?,
            _ => parse_window_query_field(
                "@window-find",
                field_name.as_str(),
                raw_value,
                &mut query,
            )?,
        }
    }

    query.validate_for_find()?;
    Ok(WindowFindRequest {
        query,
        limit: limit.unwrap_or(DEFAULT_WINDOW_FIND_LIMIT),
        include_state: include_state.unwrap_or(true),
        include_recipes: include_recipes.unwrap_or(true),
    })
}

pub fn parse_window_activate_payload(input: &str) -> io::Result<WindowActivateRequest> {
    let inner = object_inner(input, "@window-activate")?;
    if inner.is_empty() {
        return Err(invalid_data("@window-activate 对象 payload 不能为空"));
    }

    let mut target = WindowCommandTarget::default();
    let mut target_object_seen = false;
    let mut recipe = None::<String>;
    let mut steps = None::<Vec<String>>;
    let mut allow_ambiguous = None::<bool>;
    let mut select = None::<WindowSelectPolicy>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "window_id" => assign_once(
                &mut target.window_id,
                "window_id",
                "@window-activate",
                parse_non_empty_string("@window-activate.window_id", raw_value)?,
            )?,
            "target" => {
                if target != WindowCommandTarget::default() {
                    return Err(invalid_data(
                        "@window-activate 不能同时使用 `target` 和根级窗口定位字段",
                    ));
                }
                target = parse_window_target_payload(raw_value, "@window-activate")?;
                target_object_seen = true;
            }
            "recipe" => assign_once(
                &mut recipe,
                "recipe",
                "@window-activate",
                parse_non_empty_string("@window-activate.recipe", raw_value)?,
            )?,
            "steps" => assign_once(
                &mut steps,
                "steps",
                "@window-activate",
                parse_string_array(raw_value, "@window-activate.steps")?,
            )?,
            "allow_ambiguous" => assign_once(
                &mut allow_ambiguous,
                "allow_ambiguous",
                "@window-activate",
                parse_bool(raw_value, "@window-activate", "allow_ambiguous")?,
            )?,
            "select" => assign_once(
                &mut select,
                "select",
                "@window-activate",
                WindowSelectPolicy::from_literal(raw_value)?,
            )?,
            _ => {
                if target_object_seen {
                    return Err(invalid_data(
                        "@window-activate 不能同时使用 `target` 和根级窗口定位字段",
                    ));
                }
                parse_window_query_field(
                    "@window-activate",
                    field_name.as_str(),
                    raw_value,
                    &mut target.query,
                )?
            }
        }
    }

    target.validate_for_execute("@window-activate")?;
    Ok(WindowActivateRequest {
        target,
        recipe,
        steps: steps.unwrap_or_default(),
        allow_ambiguous: allow_ambiguous.unwrap_or(false),
        select,
    })
}

pub fn parse_window_close_payload(input: &str) -> io::Result<WindowCloseRequest> {
    let inner = object_inner(input, "@window-close")?;
    if inner.is_empty() {
        return Err(invalid_data("@window-close 对象 payload 不能为空"));
    }

    let mut target = WindowCommandTarget::default();
    let mut target_object_seen = false;
    let mut strategy = None::<WindowCloseStrategy>;
    let mut allow_ambiguous = None::<bool>;
    let mut select = None::<WindowSelectPolicy>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "window_id" => assign_once(
                &mut target.window_id,
                "window_id",
                "@window-close",
                parse_non_empty_string("@window-close.window_id", raw_value)?,
            )?,
            "target" => {
                if target != WindowCommandTarget::default() {
                    return Err(invalid_data(
                        "@window-close 不能同时使用 `target` 和根级窗口定位字段",
                    ));
                }
                target = parse_window_target_payload(raw_value, "@window-close")?;
                target_object_seen = true;
            }
            "strategy" => assign_once(
                &mut strategy,
                "strategy",
                "@window-close",
                WindowCloseStrategy::from_literal(raw_value)?,
            )?,
            "allow_ambiguous" => assign_once(
                &mut allow_ambiguous,
                "allow_ambiguous",
                "@window-close",
                parse_bool(raw_value, "@window-close", "allow_ambiguous")?,
            )?,
            "select" => assign_once(
                &mut select,
                "select",
                "@window-close",
                WindowSelectPolicy::from_literal(raw_value)?,
            )?,
            _ => {
                if target_object_seen {
                    return Err(invalid_data(
                        "@window-close 不能同时使用 `target` 和根级窗口定位字段",
                    ));
                }
                parse_window_query_field(
                    "@window-close",
                    field_name.as_str(),
                    raw_value,
                    &mut target.query,
                )?
            }
        }
    }

    target.validate_for_execute("@window-close")?;
    Ok(WindowCloseRequest {
        target,
        strategy: strategy.unwrap_or(WindowCloseStrategy::Graceful),
        allow_ambiguous: allow_ambiguous.unwrap_or(false),
        select,
    })
}

pub fn invalid_json_error(kind: &'static str, code: i32, error: impl Into<String>) -> io::Error {
    let json = json!({
        "kind": kind,
        "code": code,
        "error": error.into(),
    });
    io::Error::new(io::ErrorKind::InvalidInput, json.to_string())
}

pub fn ambiguous_error(
    action: &'static str,
    candidates: &[WindowCandidate],
    strategy: Option<WindowCloseStrategy>,
) -> io::Error {
    let candidates = candidates
        .iter()
        .map(|candidate| {
            json!({
                "window_id": candidate.window_id,
                "pid": candidate.app.pid,
                "app": candidate.app.name,
                "title": candidate.title,
            })
        })
        .collect::<Vec<_>>();
    let json = json!({
        "kind": "window-ambiguous",
        "action": action,
        "code": 64,
        "error": "window target matched multiple windows",
        "strategy": strategy.map(WindowCloseStrategy::as_str),
        "match_count": candidates.len(),
        "candidates": candidates,
    });
    io::Error::new(io::ErrorKind::InvalidInput, json.to_string())
}

pub fn stale_error(window_id: &str) -> io::Error {
    let json = json!({
        "kind": "window-stale",
        "code": 64,
        "error": format!("window_id 已失效或无法解析: {window_id}"),
        "window_id": window_id,
    });
    io::Error::new(io::ErrorKind::InvalidInput, json.to_string())
}

pub(crate) fn attach_window_observation(
    candidates: &mut [WindowCandidate],
    source_command: &str,
    platform: &str,
) -> io::Result<ObservationHeader> {
    let mut refs = Vec::with_capacity(candidates.len());
    let mut selector_drafts = Vec::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let ref_id = observation_ref_name(index + 1);
        candidate.ref_id = Some(ref_id.clone());
        refs.push(ObservationRefEntry {
            ref_id: ref_id.clone(),
            backend_id: candidate.window_id.clone(),
            kind: "window".to_owned(),
        });
        selector_drafts.push(window_candidate_selector_draft(
            platform, candidate, &ref_id,
        ));
    }

    record_observation_with_selectors(
        "window",
        source_command,
        ObservationRoot {
            schema: WINDOW_SCHEMA.to_owned(),
            platform: platform.to_owned(),
            coordinate_space: WINDOW_COORDINATE_SPACE.to_owned(),
        },
        refs,
        selector_drafts,
    )
}

fn window_candidate_selector_draft(
    platform: &str,
    candidate: &WindowCandidate,
    ref_id: &str,
) -> DurableSelectorDraft {
    DurableSelectorDraft::new(
        ref_id.to_owned(),
        SelectorKind::Window,
        candidate.window_id.clone(),
        SelectorEnvelope {
            platform: platform.to_owned(),
            app: Some(AppSelector {
                name: candidate.app.name.clone(),
                bundle_id: candidate.app.bundle_id.clone(),
                pid_hint: Some(candidate.app.pid),
            }),
            window: Some(WindowSelector {
                title: candidate.title.clone(),
                role: "window".to_owned(),
                rect: candidate.rect.map(selector_rect_from_ax_rect),
            }),
            element: None,
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

fn selector_rect_from_ax_rect(rect: AxRect) -> SelectorRect {
    SelectorRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn parse_window_target_payload(input: &str, kind: &str) -> io::Result<WindowCommandTarget> {
    let inner = object_inner(input, "window target")?;
    if inner.is_empty() {
        return Err(invalid_data(format!("{kind}.target 不能为空")));
    }

    let mut target = WindowCommandTarget::default();
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        parse_window_target_field(kind, field_name.as_str(), raw_value.trim(), &mut target)?;
    }

    target.validate_for_execute(kind)?;
    Ok(target)
}

fn parse_window_target_field(
    kind: &str,
    field_name: &str,
    raw_value: &str,
    target: &mut WindowCommandTarget,
) -> io::Result<()> {
    match field_name {
        "window_id" => assign_once(
            &mut target.window_id,
            "window_id",
            kind,
            parse_non_empty_string(&format!("{kind}.target.window_id"), raw_value)?,
        ),
        "ref" | "ref_id" => assign_once(
            &mut target.ref_id,
            "ref",
            kind,
            parse_non_empty_string(&format!("{kind}.target.ref"), raw_value)?,
        ),
        "observation_id" => assign_once(
            &mut target.observation_id,
            "observation_id",
            kind,
            parse_non_empty_string(&format!("{kind}.target.observation_id"), raw_value)?,
        ),
        _ => parse_window_query_field(kind, field_name, raw_value, &mut target.query),
    }
}

fn parse_window_query_field(
    kind: &str,
    field_name: &str,
    raw_value: &str,
    query: &mut WindowQuery,
) -> io::Result<()> {
    match field_name {
        "app" | "process" | "process_name" => assign_once(
            &mut query.app,
            "app",
            kind,
            parse_non_empty_string(&format!("{kind}.app"), raw_value)?,
        ),
        "app_contains" | "process_contains" => assign_once(
            &mut query.app_contains,
            "app_contains",
            kind,
            parse_non_empty_string(&format!("{kind}.app_contains"), raw_value)?,
        ),
        "bundle_id" => assign_once(
            &mut query.bundle_id,
            "bundle_id",
            kind,
            parse_non_empty_string(&format!("{kind}.bundle_id"), raw_value)?,
        ),
        "pid" => assign_once(&mut query.pid, "pid", kind, parse_pid(raw_value, kind)?),
        "title" | "window_title" => assign_once(
            &mut query.title,
            "title",
            kind,
            parse_non_empty_string(&format!("{kind}.title"), raw_value)?,
        ),
        "title_contains" | "window_title_contains" => assign_once(
            &mut query.title_contains,
            "title_contains",
            kind,
            parse_non_empty_string(&format!("{kind}.title_contains"), raw_value)?,
        ),
        _ => Err(invalid_data(format!(
            "{kind} 对象 payload 包含未知字段: {field_name}"
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
    let value = input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("window limit 必须是无符号整数: {input}")))?;
    if value == 0 {
        return Err(invalid_data("window limit 必须大于 0"));
    }
    Ok(value)
}

fn parse_pid(input: &str, kind: &str) -> io::Result<i32> {
    input
        .parse::<i32>()
        .map_err(|_| invalid_data(format!("{kind} 的 `pid` 必须是整数: {input}")))
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn matches_optional(expected: Option<&String>, actual: Option<&str>) -> bool {
    match expected {
        Some(expected) => actual == Some(expected.as_str()),
        None => true,
    }
}

fn matches_contains(expected: Option<&String>, actual: Option<&str>) -> bool {
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

fn matches_pid(expected: Option<i32>, actual: i32) -> bool {
    match expected {
        Some(expected) => expected == actual,
        None => true,
    }
}

#[cfg(target_os = "macos")]
fn platform_find(request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
    macos::find(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_find(_request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
    Ok(WindowFindResponse::unsupported("unsupported"))
}

#[cfg(target_os = "macos")]
fn platform_activate(request: &WindowActivateRequest) -> io::Result<WindowActionReport> {
    macos::activate(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_activate(_request: &WindowActivateRequest) -> io::Result<WindowActionReport> {
    Ok(WindowActionReport::unsupported("activate", "unsupported"))
}

#[cfg(target_os = "macos")]
fn platform_close(request: &WindowCloseRequest) -> io::Result<WindowActionReport> {
    macos::close(request)
}

#[cfg(not(target_os = "macos"))]
fn platform_close(_request: &WindowCloseRequest) -> io::Result<WindowActionReport> {
    Ok(WindowActionReport::unsupported("close", "unsupported"))
}

#[cfg(target_os = "macos")]
fn platform_resolve_target_rect(
    target: &WindowCommandTarget,
) -> io::Result<WindowResolvedTargetRect> {
    macos::resolve_target_rect(target)
}

#[cfg(not(target_os = "macos"))]
fn platform_resolve_target_rect(
    _target: &WindowCommandTarget,
) -> io::Result<WindowResolvedTargetRect> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前平台不支持 window target rect resolver",
    ))
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_find_should_accept_common_query_fields() {
        let request = parse_window_find_payload(
            r#"{app:"Terminal",title_contains:"rdog",limit:10,include_state:true,include_recipes:false}"#,
        )
        .unwrap();
        assert_eq!(request.query.app.as_deref(), Some("Terminal"));
        assert_eq!(request.query.title_contains.as_deref(), Some("rdog"));
        assert_eq!(request.limit, 10);
        assert!(request.include_state);
        assert!(!request.include_recipes);
    }

    #[test]
    fn parse_window_activate_should_support_recipe_and_steps() {
        let request = parse_window_activate_payload(
            r#"{window_id:"pid:1/window:0",recipe:"to_interact",steps:["activate_app","raise_window"]}"#,
        )
        .unwrap();
        assert_eq!(request.target.window_id.as_deref(), Some("pid:1/window:0"));
        assert_eq!(request.recipe.as_deref(), Some("to_interact"));
        assert_eq!(request.steps, vec!["activate_app", "raise_window"]);

        let request = parse_window_activate_payload(
            r#"{target:{ref:"@e1",observation_id:"obs-1"},recipe:"to_interact"}"#,
        )
        .unwrap();
        assert_eq!(request.target.ref_id.as_deref(), Some("@e1"));
        assert_eq!(request.target.observation_id.as_deref(), Some("obs-1"));
        assert!(parse_window_activate_payload(r#"{target:{ref:"@e1"}}"#).is_err());
        assert!(parse_window_activate_payload(
            r#"{target:{ref:"@e1",observation_id:"obs-1",app:"Terminal"}}"#
        )
        .is_err());
    }

    #[test]
    fn parse_window_close_should_parse_strategy() {
        let request =
            parse_window_close_payload(r#"{window_id:"pid:1/window:0",strategy:"kill"}"#).unwrap();
        assert_eq!(request.strategy, WindowCloseStrategy::Kill);
    }

    #[test]
    fn parse_window_payloads_should_reject_unknown_or_duplicate_fields() {
        assert!(parse_window_find_payload(r#"{app:"Terminal",app:"Finder"}"#).is_err());
        assert!(parse_window_activate_payload(r#"{window_id:"x",unknown:"field"}"#).is_err());
        assert!(parse_window_close_payload(r#"{strategy:"bad",window_id:"x"}"#).is_err());
    }

    #[test]
    fn ambiguous_and_stale_errors_should_encode_json_payload() {
        let candidate = WindowCandidate {
            window_id: "pid:1/window:0".to_owned(),
            ref_id: None,
            locator_lifetime: "short_lived",
            app: WindowAppDescriptor {
                name: "Terminal".to_owned(),
                pid: 1,
                bundle_id: None,
                hidden: false,
                frontmost: false,
            },
            title: Some("rdog".to_owned()),
            rect: None,
            coordinate_space: WINDOW_COORDINATE_SPACE,
            state: None,
            recipes: None,
        };
        let ambiguous = ambiguous_error("close", &[candidate], Some(WindowCloseStrategy::Graceful));
        assert_eq!(ambiguous.kind(), io::ErrorKind::InvalidInput);
        assert!(ambiguous
            .to_string()
            .contains("\"kind\":\"window-ambiguous\""));

        let stale = stale_error("pid:1/window:404");
        assert!(stale.to_string().contains("\"kind\":\"window-stale\""));
    }

    #[test]
    fn window_find_response_should_serialize_schema_and_snapshot() {
        let mut matches = vec![WindowCandidate {
            window_id: "pid:1/window:0".to_owned(),
            ref_id: None,
            locator_lifetime: "short_lived",
            app: WindowAppDescriptor {
                name: "Terminal".to_owned(),
                pid: 1,
                bundle_id: None,
                hidden: false,
                frontmost: false,
            },
            title: Some("rdog".to_owned()),
            rect: None,
            coordinate_space: WINDOW_COORDINATE_SPACE,
            state: None,
            recipes: None,
        }];
        let observation = attach_window_observation(&mut matches, "@window-find", "macos").unwrap();
        let response = WindowFindResponse {
            kind: "window-find",
            schema: WINDOW_SCHEMA,
            platform: "macos".to_owned(),
            status: "complete".to_owned(),
            capabilities: WindowCapabilities::complete(),
            observation: Some(observation),
            match_count: 1,
            returned_count: 1,
            snapshot_id: "window-snapshot-1".to_owned(),
            observed_at_unix_ms: 1,
            matches,
        };
        let json = response.to_value_json().unwrap();
        assert!(json.contains("\"schema\":\"rdog.window.v1\""));
        assert!(json.contains("\"snapshot_id\":\"window-snapshot-1\""));
        assert!(json.contains("\"source_command\":\"@window-find\""));
        assert!(json.contains("\"selector_count\":1"));
        assert!(json.contains("\"ref\":\"@e1\""));
    }
}
