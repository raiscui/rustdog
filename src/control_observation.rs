use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use std::path::Path;

pub mod durable;
pub mod observe;
pub mod refind;
pub mod selector;

pub use observe::{build_observe_outcome, parse_observe_payload, ObserveRequest};
pub use refind::{
    build_selector_refind_decision, build_selector_refind_response_json, SelectorRefindDecision,
    SelectorRefindPolicy, SelectorRefindRequest, SelectorRefindSource, DEFAULT_REFIND_LIMIT,
};

use crate::config::ObservationConfig;
use durable::{
    DurableObservationIdentity, DurableObservationPrivacy, DurableSelectorHint,
    DurableSelectorLastSeen, JsonlDurableObservationStore,
};
use selector::{
    DurableSelectorDraft, DurableSelectorRecord, PermanentSelector, SelectorKind, SelectorMatchMode,
};

const DEFAULT_OBSERVATION_TTL_MS: u64 = 300_000;
const DEFAULT_MAX_OBSERVATIONS: usize = 64;
const DEFAULT_MAX_REFS: usize = 20_000;
const DEFAULT_SELECTOR_HISTORY_LIMIT: usize = 32;

static OBSERVATION_STORE: OnceLock<Mutex<ObservationStore>> = OnceLock::new();
static DURABLE_OBSERVATION_STORE: OnceLock<Mutex<Option<JsonlDurableObservationStore>>> =
    OnceLock::new();

/// 一次 UI observation 的轻量头部。
///
/// P0 只承诺当前 daemon 进程内可解析。这里没有落盘字段,
/// daemon 重启后旧 `observation_id` 必须失效。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationHeader {
    pub observation_id: String,
    pub session_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub ttl_ms: u64,
    pub scope: String,
    pub source_command: String,
    pub root: ObservationRoot,
    pub ref_count: usize,
    pub selector_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRoot {
    pub schema: String,
    pub platform: String,
    pub coordinate_space: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRefEntry {
    pub ref_id: String,
    pub backend_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorGetRequest {
    pub selector_id: String,
    pub include_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorResolveRequest {
    pub selector_id: String,
    pub limit: u16,
    pub dry_run: bool,
    pub include_explanations: bool,
}

#[derive(Debug, Clone)]
struct StoredObservation {
    header: ObservationHeader,
    refs: HashMap<String, ObservationRefEntry>,
}

#[derive(Debug)]
pub struct ObservationStore {
    next_observation_sequence: u64,
    order: VecDeque<String>,
    observations: HashMap<String, StoredObservation>,
    ttl_ms: u64,
    max_observations: usize,
    max_refs: usize,
}

impl ObservationStore {
    pub fn new() -> Self {
        Self {
            next_observation_sequence: 1,
            order: VecDeque::new(),
            observations: HashMap::new(),
            ttl_ms: DEFAULT_OBSERVATION_TTL_MS,
            max_observations: DEFAULT_MAX_OBSERVATIONS,
            max_refs: DEFAULT_MAX_REFS,
        }
    }

    #[cfg(test)]
    pub fn with_limits(ttl_ms: u64, max_observations: usize, max_refs: usize) -> Self {
        Self {
            ttl_ms,
            max_observations,
            max_refs,
            ..Self::new()
        }
    }

    pub fn record(
        &mut self,
        scope: &str,
        source_command: &str,
        root: ObservationRoot,
        refs: Vec<ObservationRefEntry>,
        selector_count: usize,
        now_ms: u64,
    ) -> ObservationHeader {
        self.evict_expired(now_ms);

        let observation_id = format!("obs-{now_ms}-{}", self.next_observation_sequence);
        self.next_observation_sequence = self.next_observation_sequence.saturating_add(1);
        let ref_count = refs.len();
        let header = ObservationHeader {
            observation_id: observation_id.clone(),
            session_id: None,
            created_at_unix_ms: now_ms,
            ttl_ms: self.ttl_ms,
            scope: scope.to_owned(),
            source_command: source_command.to_owned(),
            root,
            ref_count,
            selector_count,
        };

        let refs = refs
            .into_iter()
            .map(|entry| (entry.ref_id.clone(), entry))
            .collect::<HashMap<_, _>>();
        self.order.push_back(observation_id.clone());
        self.observations.insert(
            observation_id,
            StoredObservation {
                header: header.clone(),
                refs,
            },
        );
        self.evict_over_capacity();
        header
    }

    pub fn resolve_ref(
        &mut self,
        observation_id: &str,
        ref_id: &str,
        now_ms: u64,
    ) -> io::Result<ObservationRefEntry> {
        self.evict_expired(now_ms);
        let Some(observation) = self.observations.get(observation_id) else {
            return Err(observation_ref_error(
                "OBSERVATION_EXPIRED",
                "observation 已过期或不存在",
                observation_id,
                Some(ref_id),
            ));
        };

        observation.refs.get(ref_id).cloned().ok_or_else(|| {
            observation_ref_error(
                "STALE_REF",
                "observation ref 已失效或不存在",
                observation_id,
                Some(ref_id),
            )
        })
    }

    pub fn resolve_ref_with_header(
        &mut self,
        observation_id: &str,
        ref_id: &str,
        now_ms: u64,
    ) -> io::Result<(ObservationHeader, ObservationRefEntry)> {
        self.evict_expired(now_ms);
        let Some(observation) = self.observations.get(observation_id) else {
            return Err(observation_ref_error(
                "OBSERVATION_EXPIRED",
                "observation 已过期或不存在",
                observation_id,
                Some(ref_id),
            ));
        };

        let entry = observation.refs.get(ref_id).cloned().ok_or_else(|| {
            observation_ref_error(
                "STALE_REF",
                "observation ref 已失效或不存在",
                observation_id,
                Some(ref_id),
            )
        })?;
        Ok((observation.header.clone(), entry))
    }

    fn evict_expired(&mut self, now_ms: u64) {
        let expired = self
            .observations
            .iter()
            .filter_map(|(id, observation)| {
                let expires_at = observation
                    .header
                    .created_at_unix_ms
                    .saturating_add(observation.header.ttl_ms);
                (expires_at <= now_ms).then(|| id.clone())
            })
            .collect::<Vec<_>>();

        for id in expired {
            self.remove_observation(&id);
        }
    }

    fn evict_over_capacity(&mut self) {
        while self.observations.len() > self.max_observations
            || self.total_ref_count() > self.max_refs
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.observations.remove(&oldest);
        }
    }

    fn remove_observation(&mut self, observation_id: &str) {
        self.observations.remove(observation_id);
        self.order.retain(|id| id != observation_id);
    }

    fn total_ref_count(&self) -> usize {
        self.observations
            .values()
            .map(|observation| observation.refs.len())
            .sum()
    }
}

impl Default for ObservationStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn observation_ref_name(index: usize) -> String {
    format!("@e{index}")
}

#[cfg(test)]
pub fn record_observation(
    scope: &str,
    source_command: &str,
    root: ObservationRoot,
    refs: Vec<ObservationRefEntry>,
) -> io::Result<ObservationHeader> {
    record_observation_with_selectors(scope, source_command, root, refs, Vec::new())
}

pub fn record_observation_with_selectors(
    scope: &str,
    source_command: &str,
    root: ObservationRoot,
    refs: Vec<ObservationRefEntry>,
    selector_drafts: Vec<DurableSelectorDraft>,
) -> io::Result<ObservationHeader> {
    let refs_for_durable = refs.clone();
    let selector_count = selector_drafts.len();
    with_global_store(|store| {
        Ok(store.record(
            scope,
            source_command,
            root,
            refs,
            selector_count,
            current_unix_ms(),
        ))
    })
    .and_then(|header| {
        let selectors = selector_drafts
            .into_iter()
            .map(|draft| draft.into_record(header.observation_id.clone()))
            .collect::<Vec<_>>();
        record_durable_observation_if_enabled(&header, &refs_for_durable, &selectors)?;
        Ok(header)
    })
}

pub fn resolve_observation_ref(
    observation_id: &str,
    ref_id: &str,
) -> io::Result<ObservationRefEntry> {
    with_global_store(|store| store.resolve_ref(observation_id, ref_id, current_unix_ms()))
}

pub fn resolve_observation_ref_with_header(
    observation_id: &str,
    ref_id: &str,
) -> io::Result<(ObservationHeader, ObservationRefEntry)> {
    with_global_store(|store| {
        store.resolve_ref_with_header(observation_id, ref_id, current_unix_ms())
    })
}

pub fn build_selector_get_response_json(request: &SelectorGetRequest) -> io::Result<String> {
    let (selector, last_seen, history) =
        durable_selector_snapshot(&request.selector_id, request.include_history)?;
    let value = json!({
        "kind": "selector-get",
        "schema": selector.schema,
        "status": "complete",
        "selector_id": request.selector_id,
        "selector": selector,
        "last_seen": last_seen,
        "history": history,
    });
    serde_json::to_string(&value)
        .map_err(|err| io::Error::other(format!("selector-get response 序列化失败: {err}")))
}

pub fn build_selector_resolve_response_json(
    request: &SelectorResolveRequest,
) -> io::Result<String> {
    if !request.dry_run {
        return Err(selector_error(
            io::ErrorKind::Unsupported,
            "SELECTOR_ACTION_DEFERRED",
            "P2 只支持 selector dry-run resolve,不执行 side-effect action",
            Some(&request.selector_id),
            Vec::new(),
        ));
    }

    let (selector, last_seen, _) = durable_selector_snapshot(&request.selector_id, false)?;
    let candidates =
        collect_selector_candidates(&selector, request.limit, request.include_explanations)
            .map_err(|err| selector_backend_error(&request.selector_id, err))?;

    finalize_selector_resolve_response_json(request, last_seen, candidates)
}

fn finalize_selector_resolve_response_json(
    request: &SelectorResolveRequest,
    last_seen: Option<DurableSelectorLastSeen>,
    candidates: Vec<serde_json::Value>,
) -> io::Result<String> {
    if candidates.is_empty() {
        return Err(selector_error(
            io::ErrorKind::NotFound,
            "SELECTOR_NOT_FOUND",
            "selector 当前没有命中候选",
            Some(&request.selector_id),
            candidates,
        ));
    }

    if candidates.len() > 1 {
        return Err(selector_error(
            io::ErrorKind::InvalidInput,
            "AMBIGUOUS_SELECTOR",
            "selector 当前命中多个候选,需要 agent 重新观察或收紧约束",
            Some(&request.selector_id),
            candidates,
        ));
    }

    let value = json!({
        "kind": "selector-resolve",
        "schema": "rdog.selector.resolve.v1",
        "status": "complete",
        "selector_id": request.selector_id,
        "dry_run": true,
        "match_count": candidates.len(),
        "candidates": candidates,
        "last_seen": last_seen,
    });
    serde_json::to_string(&value)
        .map_err(|err| io::Error::other(format!("selector-resolve response 序列化失败: {err}")))
}

pub fn stale_observation_ref_error(
    observation_id: &str,
    ref_id: &str,
    detail: impl AsRef<str>,
) -> io::Error {
    let detail = detail.as_ref();
    observation_ref_error(
        "STALE_REF",
        format!("observation ref 已失效: {detail}"),
        observation_id,
        Some(ref_id),
    )
}

pub fn initialize_durable_observation_state(
    config: &ObservationConfig,
    namespace: Option<&str>,
    daemon_name: &str,
) -> io::Result<()> {
    let state = if config.durable_enabled {
        let state_dir =
            durable::resolve_observation_state_dir(config.state_dir.as_deref(), daemon_name);
        Some(JsonlDurableObservationStore::open(
            state_dir,
            DurableObservationIdentity {
                namespace: namespace.map(str::to_owned),
                daemon_name: daemon_name.to_owned(),
            },
            DurableObservationPrivacy {
                persist_values: config.persist_values,
                persist_screenshots: config.persist_screenshots,
            },
            config.retention_observations,
            config.retention_bytes,
            config.write_ref_cache,
            current_unix_ms(),
        )?)
    } else {
        None
    };

    let store = DURABLE_OBSERVATION_STORE.get_or_init(|| Mutex::new(None));
    let mut guard = store
        .lock()
        .map_err(|_| io::Error::other("durable observation store lock poisoned"))?;
    *guard = state;
    Ok(())
}

#[cfg(test)]
pub fn initialize_durable_observation_state_for_tests(
    state_dir: &Path,
    retention_observations: usize,
) -> io::Result<()> {
    let config = ObservationConfig {
        durable_enabled: true,
        state_dir: Some(state_dir.to_path_buf()),
        retention_observations,
        ..ObservationConfig::default()
    };
    initialize_durable_observation_state(&config, Some("test"), "test.lab")
}

#[cfg(test)]
pub fn disable_durable_observation_state_for_tests() -> io::Result<()> {
    let store = DURABLE_OBSERVATION_STORE.get_or_init(|| Mutex::new(None));
    let mut guard = store
        .lock()
        .map_err(|_| io::Error::other("durable observation store lock poisoned"))?;
    *guard = None;
    Ok(())
}

fn with_global_store<T>(run: impl FnOnce(&mut ObservationStore) -> io::Result<T>) -> io::Result<T> {
    let store = OBSERVATION_STORE.get_or_init(|| Mutex::new(ObservationStore::new()));
    let mut guard = store
        .lock()
        .map_err(|_| io::Error::other("observation store lock poisoned"))?;
    run(&mut guard)
}

fn record_durable_observation_if_enabled(
    header: &ObservationHeader,
    refs: &[ObservationRefEntry],
    selectors: &[DurableSelectorRecord],
) -> io::Result<()> {
    with_durable_store(|store| match store {
        Some(store) => store.record_observation(header, refs, selectors),
        None => Ok(()),
    })
}

fn durable_selector_hint_for_ref(
    observation_id: &str,
    ref_id: Option<&str>,
) -> Option<DurableSelectorHint> {
    let ref_id = ref_id?;
    with_durable_store(|store| {
        Ok(store
            .as_ref()
            .and_then(|store| store.selector_hint_for_ref(observation_id, ref_id)))
    })
    .ok()
    .flatten()
}

pub(crate) fn durable_selector_snapshot(
    selector_id: &str,
    include_history: bool,
) -> io::Result<(
    PermanentSelector,
    Option<DurableSelectorLastSeen>,
    Option<Vec<DurableSelectorLastSeen>>,
)> {
    with_durable_store(|store| {
        let Some(store) = store.as_ref() else {
            return Err(selector_error(
                io::ErrorKind::Unsupported,
                "SELECTOR_BACKEND_UNSUPPORTED",
                "durable observation state 未启用,无法查询 permanent selector",
                Some(selector_id),
                Vec::new(),
            ));
        };
        let Some(selector) = store.selector_by_id(selector_id) else {
            return Err(selector_error(
                io::ErrorKind::InvalidInput,
                "SELECTOR_NOT_FOUND",
                "selector 不存在或已被清理",
                Some(selector_id),
                Vec::new(),
            ));
        };
        let last_seen = store.selector_last_seen(selector_id);
        if last_seen.is_none() {
            return Err(selector_error(
                io::ErrorKind::InvalidInput,
                "SELECTOR_STALE",
                "selector metadata 已损坏或缺少 last_seen 记录",
                Some(selector_id),
                Vec::new(),
            ));
        }
        let history = include_history
            .then(|| store.selector_history(selector_id, DEFAULT_SELECTOR_HISTORY_LIMIT));
        Ok((selector, last_seen, history))
    })
}

pub(crate) fn collect_selector_candidates(
    selector: &PermanentSelector,
    limit: u16,
    include_explanations: bool,
) -> io::Result<Vec<serde_json::Value>> {
    match selector.kind {
        SelectorKind::Window | SelectorKind::AxWindow => {
            resolve_window_selector_candidates(selector, limit, include_explanations)
        }
        SelectorKind::AxElement => {
            resolve_ax_selector_candidates(selector, limit, include_explanations)
        }
    }
}

fn resolve_window_selector_candidates(
    selector: &PermanentSelector,
    limit: u16,
    include_explanations: bool,
) -> io::Result<Vec<serde_json::Value>> {
    let request = crate::control_window::WindowFindRequest {
        query: window_query_from_selector(selector),
        limit,
        include_state: true,
        include_recipes: false,
    };
    let response = crate::control_window::execute_default_window_find(&request)?;
    let mut candidates = Vec::new();
    for (index, candidate) in response.matches.iter().enumerate() {
        let (matched_fields, missing_fields) = if include_explanations {
            window_match_explanation(selector, candidate)
        } else {
            (Vec::new(), Vec::new())
        };
        let observation = response.observation.as_ref().and_then(|observation| {
            candidate.ref_id.as_ref().map(|ref_id| {
                json!({
                    "observation_id": observation.observation_id,
                    "ref": ref_id,
                })
            })
        });
        candidates.push(json!({
            "candidate_id": format!("cand-{}", index + 1),
            "backend_id": candidate.window_id,
            "kind": "window",
            "role": selector.constraints.window.as_ref().map(|window| window.role.as_str()).unwrap_or("window"),
            "name": candidate.title,
            "matched_fields": matched_fields,
            "missing_fields": missing_fields,
            "observation": observation,
            "source": "window-backend",
        }));
    }
    Ok(candidates)
}

fn resolve_ax_selector_candidates(
    selector: &PermanentSelector,
    limit: u16,
    include_explanations: bool,
) -> io::Result<Vec<serde_json::Value>> {
    let request = ax_find_request_from_selector(selector, limit);
    let snapshot = crate::control_ax::capture_default_ax_snapshot(&request.tree)?;
    let response_json = crate::control_ax::build_ax_find_response_json(&snapshot, &request)?;
    let response: serde_json::Value =
        serde_json::from_str(&response_json).map_err(|err| io::Error::other(err.to_string()))?;
    let observation_id = response
        .get("observation")
        .and_then(|observation| observation.get("observation_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let matches = response
        .get("matches")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut candidates = Vec::new();
    for (index, candidate) in matches.into_iter().enumerate() {
        let (matched_fields, missing_fields) = if include_explanations {
            ax_match_explanation(selector, &candidate)
        } else {
            (Vec::new(), Vec::new())
        };
        let ref_id = candidate
            .get("ref")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let observation = observation_id.as_ref().and_then(|observation_id| {
            ref_id.as_ref().map(|ref_id| {
                json!({
                    "observation_id": observation_id,
                    "ref": ref_id,
                })
            })
        });
        candidates.push(json!({
            "candidate_id": format!("cand-{}", index + 1),
            "backend_id": candidate.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "kind": "ax-element",
            "role": candidate.get("role").cloned().unwrap_or(serde_json::Value::Null),
            "name": candidate.get("name").cloned().unwrap_or(serde_json::Value::Null),
            "matched_fields": matched_fields,
            "missing_fields": missing_fields,
            "observation": observation,
            "source": "ax-backend",
        }));
    }
    Ok(candidates)
}

fn window_query_from_selector(selector: &PermanentSelector) -> crate::control_window::WindowQuery {
    let mut query = crate::control_window::WindowQuery::default();
    if let Some(app) = selector.constraints.app.as_ref() {
        query.app = Some(app.name.clone());
        query.bundle_id = app.bundle_id.clone();
    }
    if let Some(window) = selector.constraints.window.as_ref() {
        match window.title_match.unwrap_or(SelectorMatchMode::Exact) {
            SelectorMatchMode::Exact => query.title = window.title.clone(),
            SelectorMatchMode::Contains => query.title_contains = window.title.clone(),
        }
    }
    query
}

fn ax_find_request_from_selector(
    selector: &PermanentSelector,
    limit: u16,
) -> crate::control_ax::AxFindRequest {
    let mut query = crate::control_ax::AxFindQuery::default();
    if let Some(app) = selector.constraints.app.as_ref() {
        query.process = Some(app.name.clone());
    }
    if let Some(window) = selector.constraints.window.as_ref() {
        match window.title_match.unwrap_or(SelectorMatchMode::Exact) {
            SelectorMatchMode::Exact => query.window_title = window.title.clone(),
            SelectorMatchMode::Contains => query.window_title_contains = window.title.clone(),
        }
        if matches!(selector.kind, SelectorKind::AxWindow) {
            query.role = Some(window.role.clone());
        }
    }
    if let Some(element) = selector.constraints.element.as_ref() {
        query.role = Some(element.role.clone());
        query.subrole = element.subrole.clone();
        match element.name_match.unwrap_or(SelectorMatchMode::Exact) {
            SelectorMatchMode::Exact => query.name = element.name.clone(),
            SelectorMatchMode::Contains => query.name_contains = element.name.clone(),
        }
        match element
            .description_match
            .unwrap_or(SelectorMatchMode::Exact)
        {
            SelectorMatchMode::Exact => query.description = element.description.clone(),
            SelectorMatchMode::Contains => query.description_contains = element.description.clone(),
        }
        query.action = element.actions.first().cloned();
    }
    crate::control_ax::AxFindRequest {
        tree: crate::control_ax::AxTreeRequest::default(),
        query,
        limit,
    }
}

fn selector_error(
    kind: io::ErrorKind,
    error_code: &'static str,
    message: impl AsRef<str>,
    selector_id: Option<&str>,
    candidates: Vec<serde_json::Value>,
) -> io::Error {
    let payload = json!({
        "kind": "selector-error",
        "status": "error",
        "error_code": error_code,
        "message": message.as_ref(),
        "selector_id": selector_id,
        "candidates": candidates,
        "suggestion": "先执行 @selector-get 或重新 observation,不要把 selector 当成已执行动作",
    });
    io::Error::new(kind, payload.to_string())
}

fn selector_backend_error(selector_id: &str, err: io::Error) -> io::Error {
    let error_code = match err.kind() {
        io::ErrorKind::PermissionDenied => "PERM_DENIED",
        io::ErrorKind::Unsupported => "SELECTOR_BACKEND_UNSUPPORTED",
        io::ErrorKind::NotFound => "SELECTOR_NOT_FOUND",
        _ => "SELECTOR_RESOLVE_FAILED",
    };
    selector_error(
        err.kind(),
        error_code,
        err.to_string(),
        Some(selector_id),
        Vec::new(),
    )
}

fn window_match_explanation(
    selector: &PermanentSelector,
    candidate: &crate::control_window::WindowCandidate,
) -> (Vec<String>, Vec<String>) {
    let mut matched = Vec::new();
    let mut missing = Vec::new();

    if let Some(app) = selector.constraints.app.as_ref() {
        push_match(
            &mut matched,
            &mut missing,
            "app.name",
            string_matches(
                &app.name,
                Some(candidate.app.name.as_str()),
                SelectorMatchMode::Exact,
            ),
        );
        if let Some(bundle_id) = app.bundle_id.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "app.bundle_id",
                string_matches(
                    bundle_id,
                    candidate.app.bundle_id.as_deref(),
                    SelectorMatchMode::Exact,
                ),
            );
        }
    }

    if let Some(window) = selector.constraints.window.as_ref() {
        if let Some(title) = window.title.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "window.title",
                string_matches(
                    title,
                    candidate.title.as_deref(),
                    window.title_match.unwrap_or(SelectorMatchMode::Exact),
                ),
            );
        }
    }

    (matched, missing)
}

fn ax_match_explanation(
    selector: &PermanentSelector,
    candidate: &serde_json::Value,
) -> (Vec<String>, Vec<String>) {
    let mut matched = Vec::new();
    let mut missing = Vec::new();

    if let Some(app) = selector.constraints.app.as_ref() {
        push_match(
            &mut matched,
            &mut missing,
            "app.name",
            string_matches(
                &app.name,
                candidate
                    .get("process_name")
                    .and_then(serde_json::Value::as_str),
                SelectorMatchMode::Exact,
            ),
        );
        if app.bundle_id.is_some() {
            missing.push("app.bundle_id".to_owned());
        }
    }

    if let Some(window) = selector.constraints.window.as_ref() {
        if let Some(title) = window.title.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "window.title",
                string_matches(
                    title,
                    candidate
                        .get("window_title")
                        .and_then(serde_json::Value::as_str),
                    window.title_match.unwrap_or(SelectorMatchMode::Exact),
                ),
            );
        }
        if matches!(selector.kind, SelectorKind::AxWindow) {
            push_match(
                &mut matched,
                &mut missing,
                "window.role",
                string_matches(
                    &window.role,
                    candidate.get("role").and_then(serde_json::Value::as_str),
                    SelectorMatchMode::Exact,
                ),
            );
        }
    }

    if let Some(element) = selector.constraints.element.as_ref() {
        push_match(
            &mut matched,
            &mut missing,
            "element.role",
            string_matches(
                &element.role,
                candidate.get("role").and_then(serde_json::Value::as_str),
                SelectorMatchMode::Exact,
            ),
        );
        if let Some(subrole) = element.subrole.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "element.subrole",
                string_matches(
                    subrole,
                    candidate.get("subrole").and_then(serde_json::Value::as_str),
                    SelectorMatchMode::Exact,
                ),
            );
        }
        if let Some(name) = element.name.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "element.name",
                string_matches(
                    name,
                    candidate.get("name").and_then(serde_json::Value::as_str),
                    element.name_match.unwrap_or(SelectorMatchMode::Exact),
                ),
            );
        }
        if let Some(description) = element.description.as_ref() {
            push_match(
                &mut matched,
                &mut missing,
                "element.description",
                string_matches(
                    description,
                    candidate
                        .get("description")
                        .and_then(serde_json::Value::as_str),
                    element
                        .description_match
                        .unwrap_or(SelectorMatchMode::Exact),
                ),
            );
        }
        if !element.actions.is_empty() {
            let actual_actions = candidate
                .get("actions")
                .and_then(serde_json::Value::as_array)
                .map(|actions| {
                    actions
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let has_actions = element
                .actions
                .iter()
                .all(|expected| actual_actions.iter().any(|actual| actual == expected));
            push_match(&mut matched, &mut missing, "element.actions", has_actions);
        }
    }

    (matched, missing)
}

fn push_match(matched: &mut Vec<String>, missing: &mut Vec<String>, field: &str, is_match: bool) {
    if is_match {
        matched.push(field.to_owned());
    } else {
        missing.push(field.to_owned());
    }
}

fn string_matches(expected: &str, actual: Option<&str>, mode: SelectorMatchMode) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    match mode {
        SelectorMatchMode::Exact => actual == expected,
        SelectorMatchMode::Contains => actual.contains(expected),
    }
}

fn with_durable_store<T>(
    run: impl FnOnce(&mut Option<JsonlDurableObservationStore>) -> io::Result<T>,
) -> io::Result<T> {
    let store = DURABLE_OBSERVATION_STORE.get_or_init(|| Mutex::new(None));
    let mut guard = store
        .lock()
        .map_err(|_| io::Error::other("durable observation store lock poisoned"))?;
    run(&mut guard)
}

fn observation_ref_error(
    error_code: &'static str,
    message: impl AsRef<str>,
    observation_id: &str,
    ref_id: Option<&str>,
) -> io::Error {
    let retry_command = "重新执行 @ax-find、@ax-tree、@screenshot include_ax 或 @window-find";
    let durable_hint = durable_selector_hint_for_ref(observation_id, ref_id);
    let payload = json!({
        "kind": "observation-ref-error",
        "code": 64,
        "error_code": error_code,
        "error": message.as_ref(),
        "message": message.as_ref(),
        "observation_id": observation_id,
        "ref": ref_id,
        "suggestion": "重新执行 @ax-find、@ax-tree 或 @screenshot include_ax 后再使用新的 ref",
        "retry_command": retry_command,
        "durable": durable_hint,
    });
    io::Error::new(io::ErrorKind::InvalidInput, payload.to_string())
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_observation::selector::{
        AppSelector, DurableSelectorDraft, SelectorEnvelope, SelectorKind, SelectorRedaction,
        WindowSelector,
    };
    use std::{
        env, fs,
        sync::{Mutex, MutexGuard},
    };

    static DURABLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn durable_test_lock() -> MutexGuard<'static, ()> {
        DURABLE_TEST_LOCK
            .lock()
            .expect("durable observation test lock should work")
    }

    fn root() -> ObservationRoot {
        ObservationRoot {
            schema: "rdog.ax.v1".to_owned(),
            platform: "macos".to_owned(),
            coordinate_space: "os-logical".to_owned(),
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = current_unix_ms();
        env::temp_dir().join(format!("rdog-observation-{name}-{nonce}"))
    }

    fn selector_draft(ref_id: &str) -> DurableSelectorDraft {
        DurableSelectorDraft::new(
            ref_id,
            SelectorKind::AxWindow,
            "pid:1/window:0",
            SelectorEnvelope {
                platform: "macos".to_owned(),
                app: Some(AppSelector {
                    name: "System Settings".to_owned(),
                    bundle_id: Some("com.apple.systempreferences".to_owned()),
                    pid_hint: Some(1),
                }),
                window: Some(WindowSelector {
                    title: Some("Storage".to_owned()),
                    role: "AXWindow".to_owned(),
                    rect: None,
                }),
                element: None,
                anchors: Vec::new(),
            },
            SelectorRedaction::metadata_only(),
        )
    }

    #[test]
    fn store_should_record_and_resolve_refs() {
        let mut store = ObservationStore::new();
        let header = store.record(
            "ax",
            "@ax-tree",
            root(),
            vec![ObservationRefEntry {
                ref_id: "@e1".to_owned(),
                backend_id: "pid:1/window:0".to_owned(),
                kind: "window".to_owned(),
            }],
            0,
            100,
        );

        assert_eq!(header.ref_count, 1);
        let resolved = store
            .resolve_ref(&header.observation_id, "@e1", 101)
            .unwrap();
        assert_eq!(resolved.backend_id, "pid:1/window:0");
    }

    #[test]
    fn store_should_return_structured_expired_error() {
        let mut store = ObservationStore::with_limits(10, 64, 20_000);
        let header = store.record("ax", "@ax-tree", root(), Vec::new(), 0, 100);

        let err = store
            .resolve_ref(&header.observation_id, "@e1", 111)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("OBSERVATION_EXPIRED"));
    }

    #[test]
    fn store_should_return_structured_stale_ref_error() {
        let mut store = ObservationStore::new();
        let header = store.record("ax", "@ax-tree", root(), Vec::new(), 0, 100);

        let err = store
            .resolve_ref(&header.observation_id, "@e404", 101)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("STALE_REF"));
    }

    #[test]
    fn store_should_evict_oldest_when_over_capacity() {
        let mut store = ObservationStore::with_limits(300_000, 1, 20_000);
        let first = store.record("ax", "@ax-tree", root(), Vec::new(), 0, 100);
        let second = store.record(
            "ax",
            "@ax-tree",
            root(),
            vec![ObservationRefEntry {
                ref_id: "@e1".to_owned(),
                backend_id: "pid:2/window:0".to_owned(),
                kind: "window".to_owned(),
            }],
            0,
            101,
        );

        assert!(store
            .resolve_ref(&first.observation_id, "@e1", 102)
            .unwrap_err()
            .to_string()
            .contains("OBSERVATION_EXPIRED"));
        assert_eq!(
            store
                .resolve_ref(&second.observation_id, "@e1", 102)
                .unwrap()
                .backend_id,
            "pid:2/window:0"
        );
    }

    #[test]
    fn durable_state_should_hint_but_not_revive_short_refs_after_restart() {
        let _guard = durable_test_lock();
        let dir = temp_dir("restart-hint");
        initialize_durable_observation_state_for_tests(&dir, 16).unwrap();

        let refs = vec![ObservationRefEntry {
            ref_id: "@e1".to_owned(),
            backend_id: "pid:1/window:0".to_owned(),
            kind: "window".to_owned(),
        }];
        let mut original_store = ObservationStore::new();
        let header = original_store.record("ax", "@ax-tree", root(), refs.clone(), 1, 100);
        let selector = selector_draft("@e1").into_record(header.observation_id.clone());
        record_durable_observation_if_enabled(&header, &refs, &[selector]).unwrap();

        initialize_durable_observation_state_for_tests(&dir, 16).unwrap();
        let mut fresh_store = ObservationStore::new();
        let err = fresh_store
            .resolve_ref(&header.observation_id, "@e1", 101)
            .unwrap_err();
        let payload: serde_json::Value = serde_json::from_str(&err.to_string()).unwrap();

        assert_eq!(payload["error_code"], "OBSERVATION_EXPIRED");
        assert_eq!(payload["durable"]["selector_hint_available"], true);
        assert!(payload["durable"]["selector_id"]
            .as_str()
            .unwrap()
            .starts_with("sel-v1-"));
        assert!(payload["durable"]["reobserve_commands"][0]
            .as_str()
            .unwrap()
            .starts_with("@window-find"));
        assert_eq!(payload["durable"]["refind_available"], true);
        assert!(payload["durable"]["refind_command"]
            .as_str()
            .unwrap()
            .starts_with("@selector-refind"));
        assert!(payload["durable"]["note"]
            .as_str()
            .unwrap()
            .contains("不表示动作已经执行"));
        disable_durable_observation_state_for_tests().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selector_get_should_return_permanent_selector_from_durable_state() {
        let _guard = durable_test_lock();
        let dir = temp_dir("selector-get");
        initialize_durable_observation_state_for_tests(&dir, 16).unwrap();
        let refs = vec![ObservationRefEntry {
            ref_id: "@e1".to_owned(),
            backend_id: "pid:1/window:0".to_owned(),
            kind: "window".to_owned(),
        }];
        let mut store = ObservationStore::new();
        let header = store.record("ax", "@ax-tree", root(), refs.clone(), 1, 100);
        let selector = selector_draft("@e1").into_record(header.observation_id.clone());
        let selector_id = selector.selector_id.clone();
        record_durable_observation_if_enabled(&header, &refs, &[selector]).unwrap();

        let response = build_selector_get_response_json(&SelectorGetRequest {
            selector_id: selector_id.clone(),
            include_history: false,
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(value["kind"], "selector-get");
        assert_eq!(value["selector_id"], selector_id);
        assert_eq!(value["selector"]["schema"], "rdog.selector.v1");
        assert_eq!(value["selector"]["source"]["ref"], "@e1");
        disable_durable_observation_state_for_tests().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selector_get_should_return_real_selector_history_when_requested() {
        let _guard = durable_test_lock();
        let dir = temp_dir("selector-history");
        initialize_durable_observation_state_for_tests(&dir, 16).unwrap();
        let refs = vec![ObservationRefEntry {
            ref_id: "@e1".to_owned(),
            backend_id: "pid:1/window:0".to_owned(),
            kind: "window".to_owned(),
        }];
        let mut store = ObservationStore::new();
        let first_header = store.record("ax", "@ax-tree", root(), refs.clone(), 1, 100);
        let first_selector = selector_draft("@e1").into_record(first_header.observation_id.clone());
        let selector_id = first_selector.selector_id.clone();
        record_durable_observation_if_enabled(&first_header, &refs, &[first_selector]).unwrap();

        let second_header = store.record("ax", "@ax-tree", root(), refs.clone(), 1, 200);
        let second_selector =
            selector_draft("@e2").into_record(second_header.observation_id.clone());
        record_durable_observation_if_enabled(&second_header, &refs, &[second_selector]).unwrap();

        let response = build_selector_get_response_json(&SelectorGetRequest {
            selector_id,
            include_history: true,
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();
        let history = value["history"].as_array().unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["observation_id"], second_header.observation_id);
        assert_eq!(history[0]["ref"], "@e2");
        assert_eq!(history[1]["observation_id"], first_header.observation_id);
        assert_eq!(history[1]["ref"], "@e1");
        disable_durable_observation_state_for_tests().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selector_resolve_should_reject_side_effectful_mode() {
        let _guard = durable_test_lock();
        let dir = temp_dir("selector-resolve-dry-run");
        initialize_durable_observation_state_for_tests(&dir, 16).unwrap();
        let refs = vec![ObservationRefEntry {
            ref_id: "@e1".to_owned(),
            backend_id: "pid:1/window:0".to_owned(),
            kind: "window".to_owned(),
        }];
        let mut store = ObservationStore::new();
        let header = store.record("ax", "@ax-tree", root(), refs.clone(), 1, 100);
        let selector = selector_draft("@e1").into_record(header.observation_id.clone());
        let selector_id = selector.selector_id.clone();
        record_durable_observation_if_enabled(&header, &refs, &[selector]).unwrap();

        let err = build_selector_resolve_response_json(&SelectorResolveRequest {
            selector_id,
            limit: 1,
            dry_run: false,
            include_explanations: true,
        })
        .unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&err.to_string()).unwrap();

        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert_eq!(value["error_code"], "SELECTOR_ACTION_DEFERRED");
        disable_durable_observation_state_for_tests().unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selector_resolve_finalize_should_return_structured_not_found() {
        let request = SelectorResolveRequest {
            selector_id: "sel-v1-empty".to_owned(),
            limit: 10,
            dry_run: true,
            include_explanations: true,
        };

        let err = finalize_selector_resolve_response_json(&request, None, Vec::new()).unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&err.to_string()).unwrap();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(value["kind"], "selector-error");
        assert_eq!(value["error_code"], "SELECTOR_NOT_FOUND");
        assert_eq!(value["candidates"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn selector_resolve_finalize_should_return_structured_ambiguous_error() {
        let request = SelectorResolveRequest {
            selector_id: "sel-v1-many".to_owned(),
            limit: 10,
            dry_run: true,
            include_explanations: true,
        };
        let candidates = vec![
            json!({"candidate_id":"cand-1","matched_fields":["app.name"],"missing_fields":[]}),
            json!({"candidate_id":"cand-2","matched_fields":["app.name"],"missing_fields":[]}),
        ];

        let err = finalize_selector_resolve_response_json(&request, None, candidates).unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&err.to_string()).unwrap();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(value["kind"], "selector-error");
        assert_eq!(value["error_code"], "AMBIGUOUS_SELECTOR");
        assert_eq!(value["candidates"].as_array().unwrap().len(), 2);
    }
}
