use serde_json::{json, Value};
use std::io;

use super::{
    collect_selector_candidates, durable_selector_snapshot, selector::PERMANENT_SELECTOR_SCHEMA,
    DurableSelectorLastSeen, PermanentSelector, SelectorKind, SelectorMatchMode,
};

pub const SELECTOR_REFIND_SCHEMA: &str = "rdog.selector.refind.v1";
pub const SELECTOR_SCORE_SCHEMA: &str = "rdog.selector.score.v1";
pub const DEFAULT_REFIND_LIMIT: u16 = 10;
pub const DEFAULT_REFIND_MIN_CONFIDENCE_MILLI: u16 = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorRefindPolicy {
    Safe,
    Manual,
}

impl SelectorRefindPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorRefindSource {
    pub observation_id: String,
    pub ref_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorRefindRequest {
    pub selector_id: String,
    pub limit: u16,
    pub policy: SelectorRefindPolicy,
    pub min_confidence_milli: u16,
    pub include_explanations: bool,
    pub include_history: bool,
    pub source: Option<SelectorRefindSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectorRefindDecision {
    pub selector_id: String,
    pub decision: String,
    pub policy: SelectorRefindPolicy,
    pub min_confidence_milli: u16,
    pub candidate_count: usize,
    pub fresh_target: Option<SelectorFreshTarget>,
    pub verify_hint: Option<Value>,
    pub scoring_version: String,
    pub audit_value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorFreshTarget {
    pub observation_id: String,
    pub ref_id: String,
}

/// 构建 `@selector-refind` 的只读恢复决策响应。
///
/// 这个命令不执行任何 GUI action。它只把 stable selector 恢复到新的
/// observation ref,并把是否能安全自动 rebound 的理由暴露给 agent。
pub fn build_selector_refind_response_json(request: &SelectorRefindRequest) -> io::Result<String> {
    serialize_refind_value(build_selector_refind_value(request)?)
}

pub fn build_selector_refind_decision(
    request: &SelectorRefindRequest,
) -> io::Result<SelectorRefindDecision> {
    let value = build_selector_refind_value(request)?;
    Ok(SelectorRefindDecision {
        selector_id: request.selector_id.clone(),
        decision: value
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("blocked")
            .to_owned(),
        policy: request.policy,
        min_confidence_milli: request.min_confidence_milli,
        candidate_count: value
            .get("match_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        fresh_target: fresh_target_from_value(&value),
        verify_hint: value.get("verify_hint").cloned(),
        scoring_version: value
            .get("scoring_version")
            .and_then(Value::as_str)
            .unwrap_or(SELECTOR_SCORE_SCHEMA)
            .to_owned(),
        audit_value: value,
    })
}

fn build_selector_refind_value(request: &SelectorRefindRequest) -> io::Result<Value> {
    let snapshot = durable_selector_snapshot(&request.selector_id, request.include_history);
    let (selector, last_seen, history) = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => return Ok(snapshot_error_response(request, &err)),
    };

    if selector.schema != PERMANENT_SELECTOR_SCHEMA {
        return Ok(blocked_response(
            request,
            last_seen,
            history,
            vec![blocker(
                "SELECTOR_SCHEMA_UNSUPPORTED",
                format!("不支持的 selector schema: {}", selector.schema),
                "schema",
            )],
        ));
    }

    let candidates =
        match collect_selector_candidates(&selector, request.limit, request.include_explanations) {
            Ok(candidates) => candidates,
            Err(err) => {
                return Ok(blocked_response(
                    request,
                    last_seen,
                    history,
                    vec![backend_blocker(&err)],
                ));
            }
        };

    Ok(refind_response_from_candidates(
        request, &selector, last_seen, history, candidates,
    ))
}

fn fresh_target_from_value(value: &Value) -> Option<SelectorFreshTarget> {
    let fresh_target = value.get("fresh_target")?;
    Some(SelectorFreshTarget {
        observation_id: fresh_target
            .get("observation_id")
            .and_then(Value::as_str)?
            .to_owned(),
        ref_id: fresh_target.get("ref").and_then(Value::as_str)?.to_owned(),
    })
}

fn refind_response_from_candidates(
    request: &SelectorRefindRequest,
    selector: &PermanentSelector,
    last_seen: Option<DurableSelectorLastSeen>,
    history: Option<Vec<DurableSelectorLastSeen>>,
    candidates: Vec<Value>,
) -> Value {
    if candidates.is_empty() {
        return not_found_response(request, last_seen, history);
    }

    let mut scored = candidates
        .into_iter()
        .map(|candidate| score_candidate(selector, candidate, request.min_confidence_milli))
        .collect::<Vec<_>>();
    sort_scored_candidates(&mut scored);

    let is_single_safe_candidate = scored.len() == 1
        && request.policy == SelectorRefindPolicy::Safe
        && scored
            .first()
            .map(|candidate| candidate.auto_rebind_eligible)
            .unwrap_or(false);

    if is_single_safe_candidate {
        let selected = scored.remove(0).value;
        let fresh_target = selected.get("observation").cloned().unwrap_or(Value::Null);
        return base_response(request, "rebound", last_seen, history)
            .with_field("match_count", json!(1))
            .with_field("selected_candidate", selected)
            .with_field("fresh_target", fresh_target.clone())
            .with_field("verify_hint", verify_hint_for_candidate(&fresh_target))
            .with_field(
                "recovery_recipe",
                recovery_recipe(request, Some(&fresh_target)),
            )
            .with_field("candidates", json!([]))
            .into_value();
    }

    mark_non_rebound_candidate_reasons(request, &mut scored);
    let candidates = scored
        .into_iter()
        .map(|candidate| candidate.value)
        .collect::<Vec<_>>();
    base_response(request, "needs_disambiguation", last_seen, history)
        .with_field("match_count", json!(candidates.len()))
        .with_field("candidates", json!(candidates))
        .with_field("recovery_recipe", recovery_recipe(request, None))
        .into_value()
}

fn not_found_response(
    request: &SelectorRefindRequest,
    last_seen: Option<DurableSelectorLastSeen>,
    history: Option<Vec<DurableSelectorLastSeen>>,
) -> Value {
    base_response(request, "not_found", last_seen, history)
        .with_field("match_count", json!(0))
        .with_field("candidates", json!([]))
        .with_field("recovery_recipe", recovery_recipe(request, None))
        .into_value()
}

fn blocked_response(
    request: &SelectorRefindRequest,
    last_seen: Option<DurableSelectorLastSeen>,
    history: Option<Vec<DurableSelectorLastSeen>>,
    blockers: Vec<Value>,
) -> Value {
    let permission = blocker_messages(&blockers, "permission");
    let capability = blocker_messages(&blockers, "capability");
    let backend = blocker_messages(&blockers, "backend");
    let schema = blocker_messages(&blockers, "schema");
    base_response(request, "blocked", last_seen, history)
        .with_field("match_count", json!(0))
        .with_field("candidates", json!([]))
        .with_field("blockers", json!(blockers))
        .with_field("permission", json!(permission))
        .with_field("capability", json!(capability))
        .with_field("backend", json!(backend))
        .with_field("schema", json!(schema))
        .with_field("recovery_recipe", recovery_recipe(request, None))
        .into_value()
}

fn snapshot_error_response(request: &SelectorRefindRequest, err: &io::Error) -> Value {
    let payload = serde_json::from_str::<Value>(&err.to_string()).unwrap_or_else(|_| {
        json!({
            "error_code": "SELECTOR_REFIND_FAILED",
            "message": err.to_string(),
        })
    });
    let error_code = payload
        .get("error_code")
        .and_then(Value::as_str)
        .unwrap_or("SELECTOR_REFIND_FAILED");
    let message = payload
        .get("message")
        .or_else(|| payload.get("error"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| err.to_string());

    match error_code {
        "SELECTOR_NOT_FOUND" => base_response(request, "not_found", None, None)
            .with_field("match_count", json!(0))
            .with_field("candidates", json!([]))
            .with_field("recovery_recipe", recovery_recipe(request, None))
            .with_field(
                "diagnostics",
                json!({"error_code": error_code, "message": message}),
            )
            .into_value(),
        "SELECTOR_BACKEND_UNSUPPORTED" => blocked_response(
            request,
            None,
            None,
            vec![blocker(error_code, message, "backend")],
        ),
        "SELECTOR_STALE" => blocked_response(
            request,
            None,
            None,
            vec![blocker(error_code, message, "schema")],
        ),
        _ => blocked_response(
            request,
            None,
            None,
            vec![blocker(error_code, message, "backend")],
        ),
    }
}

#[derive(Debug)]
struct ScoredCandidate {
    confidence_milli: u16,
    hard_gate_violations: usize,
    history_proximity_milli: u16,
    stable_key: String,
    auto_rebind_eligible: bool,
    value: Value,
}

fn score_candidate(
    selector: &PermanentSelector,
    mut candidate: Value,
    min_confidence_milli: u16,
) -> ScoredCandidate {
    let matched_fields = string_array_field(&candidate, "matched_fields");
    let missing_fields = string_array_field(&candidate, "missing_fields");
    let mut score = 0u16;
    let mut reason_codes = Vec::<String>::new();

    for field in &matched_fields {
        if let Some(component) = score_component(selector, field) {
            score = score.saturating_add(component.weight_milli);
            reason_codes.push(component.reason_code);
        }
    }

    let mut reject_reasons = Vec::<String>::new();
    let mut hard_gate_violations = 0usize;
    if candidate_kind(&candidate) != Some(expected_candidate_kind(selector)) {
        reject_reasons.push("hard_mismatch.kind".to_owned());
        hard_gate_violations += 1;
    }
    for field in missing_fields.iter().filter(|field| is_hard_field(field)) {
        reject_reasons.push(format!("hard_missing.{field}"));
        hard_gate_violations += 1;
    }
    if is_truthy(&candidate, "hidden") {
        reject_reasons.push("blocked.hidden".to_owned());
        hard_gate_violations += 1;
    }
    if is_false(&candidate, "enabled") {
        reject_reasons.push("blocked.disabled".to_owned());
        hard_gate_violations += 1;
    }
    if missing_fields
        .iter()
        .any(|field| field == "element.actions")
    {
        reject_reasons.push("blocked.action_unsupported".to_owned());
        hard_gate_violations += 1;
    }
    if !candidate_has_fresh_observation(&candidate) {
        reject_reasons.push("hard_missing.fresh_target".to_owned());
        hard_gate_violations += 1;
    }

    let history_proximity_milli = history_hint_score(selector, &candidate);
    score = score.saturating_add(history_proximity_milli);
    if history_proximity_milli > 0 {
        reason_codes.push("hint.backend_id.match".to_owned());
    }

    if hard_gate_violations > 0 {
        score = score.min(800);
    }
    let confidence_band = confidence_band(score, min_confidence_milli);
    let auto_rebind_eligible = hard_gate_violations == 0 && confidence_band == "high";

    if let Some(object) = candidate.as_object_mut() {
        object.insert("scoring_version".to_owned(), json!(SELECTOR_SCORE_SCHEMA));
        object.insert("confidence".to_owned(), json!(milli_to_ratio(score)));
        object.insert("confidence_milli".to_owned(), json!(score));
        object.insert("confidence_band".to_owned(), json!(confidence_band));
        object.insert("reason_codes".to_owned(), json!(reason_codes));
        object.insert("reject_reasons".to_owned(), json!(reject_reasons));
        object.insert(
            "auto_rebind_eligible".to_owned(),
            json!(auto_rebind_eligible),
        );
    }

    ScoredCandidate {
        confidence_milli: score,
        hard_gate_violations,
        history_proximity_milli,
        stable_key: stable_candidate_key(&candidate),
        auto_rebind_eligible,
        value: candidate,
    }
}

struct ScoreComponent {
    weight_milli: u16,
    reason_code: String,
}

fn score_component(selector: &PermanentSelector, field: &str) -> Option<ScoreComponent> {
    let (weight_milli, reason_code) = match field {
        "app.bundle_id" => (200, "app.bundle_id.exact".to_owned()),
        "app.name" => (
            match_mode_weight(
                selector
                    .constraints
                    .app
                    .as_ref()
                    .map(|_| SelectorMatchMode::Exact),
                120,
                80,
            ),
            "app.name.exact".to_owned(),
        ),
        "window.title" => {
            let mode = selector
                .constraints
                .window
                .as_ref()
                .and_then(|window| window.title_match);
            let suffix = match mode.unwrap_or(SelectorMatchMode::Exact) {
                SelectorMatchMode::Exact => "exact",
                SelectorMatchMode::Contains => "contains",
            };
            (
                match_mode_weight(mode, 180, 100),
                format!("window.title.{suffix}"),
            )
        }
        "window.role" => (80, "window.role.exact".to_owned()),
        "element.role" => (150, "element.role.exact".to_owned()),
        "element.subrole" => (60, "element.subrole.exact".to_owned()),
        "element.name" => {
            let mode = selector
                .constraints
                .element
                .as_ref()
                .and_then(|element| element.name_match);
            let suffix = match mode.unwrap_or(SelectorMatchMode::Exact) {
                SelectorMatchMode::Exact => "exact",
                SelectorMatchMode::Contains => "contains",
            };
            (
                match_mode_weight(mode, 180, 100),
                format!("element.name.{suffix}"),
            )
        }
        "element.description" => {
            let mode = selector
                .constraints
                .element
                .as_ref()
                .and_then(|element| element.description_match);
            let suffix = match mode.unwrap_or(SelectorMatchMode::Exact) {
                SelectorMatchMode::Exact => "exact",
                SelectorMatchMode::Contains => "contains",
            };
            (
                match_mode_weight(mode, 80, 40),
                format!("element.description.{suffix}"),
            )
        }
        "element.actions" => (80, "element.action.supported".to_owned()),
        _ if field.starts_with("anchor.") => (60, format!("{field}.match")),
        _ => return None,
    };
    Some(ScoreComponent {
        weight_milli,
        reason_code,
    })
}

fn match_mode_weight(mode: Option<SelectorMatchMode>, exact: u16, contains: u16) -> u16 {
    match mode.unwrap_or(SelectorMatchMode::Exact) {
        SelectorMatchMode::Exact => exact,
        SelectorMatchMode::Contains => contains,
    }
}

fn expected_candidate_kind(selector: &PermanentSelector) -> &'static str {
    match selector.kind {
        SelectorKind::AxElement => "ax-element",
        SelectorKind::AxWindow | SelectorKind::Window => "window",
    }
}

fn candidate_kind(candidate: &Value) -> Option<&str> {
    candidate.get("kind").and_then(Value::as_str)
}

fn is_hard_field(field: &str) -> bool {
    matches!(
        field,
        "app.bundle_id"
            | "app.name"
            | "window.title"
            | "window.role"
            | "element.role"
            | "element.name"
            | "element.actions"
    )
}

fn history_hint_score(selector: &PermanentSelector, candidate: &Value) -> u16 {
    let Some(expected) = selector.hints.backend_id.as_deref() else {
        return 0;
    };
    let actual = candidate.get("backend_id").and_then(Value::as_str);
    if actual == Some(expected) {
        40
    } else {
        0
    }
}

fn confidence_band(score: u16, min_confidence_milli: u16) -> &'static str {
    if score >= min_confidence_milli {
        "high"
    } else if score >= 650 {
        "medium"
    } else {
        "low"
    }
}

fn sort_scored_candidates(candidates: &mut [ScoredCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .confidence_milli
            .cmp(&left.confidence_milli)
            .then_with(|| left.hard_gate_violations.cmp(&right.hard_gate_violations))
            .then_with(|| {
                right
                    .history_proximity_milli
                    .cmp(&left.history_proximity_milli)
            })
            .then_with(|| left.stable_key.cmp(&right.stable_key))
    });
}

fn mark_non_rebound_candidate_reasons(
    request: &SelectorRefindRequest,
    candidates: &mut [ScoredCandidate],
) {
    let high_eligible_count = candidates
        .iter()
        .filter(|candidate| candidate.auto_rebind_eligible)
        .count();
    let reason = if request.policy == SelectorRefindPolicy::Manual {
        Some("policy.manual")
    } else if candidates.len() > 1 {
        Some(if high_eligible_count > 1 {
            "ambiguous.high_confidence_peer"
        } else {
            "ambiguous.candidate_set_not_single"
        })
    } else {
        None
    };
    let Some(reason) = reason else {
        return;
    };

    for candidate in candidates {
        if candidate.auto_rebind_eligible {
            candidate.auto_rebind_eligible = false;
            if let Some(object) = candidate.value.as_object_mut() {
                object.insert("auto_rebind_eligible".to_owned(), json!(false));
                let mut reject_reasons = object
                    .get("reject_reasons")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                reject_reasons.push(json!(reason));
                object.insert("reject_reasons".to_owned(), Value::Array(reject_reasons));
            }
        }
    }
}

fn stable_candidate_key(candidate: &Value) -> String {
    candidate
        .get("candidate_id")
        .and_then(Value::as_str)
        .or_else(|| candidate.get("backend_id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn candidate_has_fresh_observation(candidate: &Value) -> bool {
    let Some(observation) = candidate.get("observation") else {
        return false;
    };
    observation
        .get("observation_id")
        .and_then(Value::as_str)
        .is_some()
        && observation.get("ref").and_then(Value::as_str).is_some()
}

fn string_array_field(candidate: &Value, field_name: &str) -> Vec<String> {
    candidate
        .get(field_name)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn is_truthy(candidate: &Value, field_name: &str) -> bool {
    candidate
        .get(field_name)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn is_false(candidate: &Value, field_name: &str) -> bool {
    candidate
        .get(field_name)
        .and_then(Value::as_bool)
        .map(|value| !value)
        .unwrap_or(false)
}

fn verify_hint_for_candidate(fresh_target: &Value) -> Value {
    let observation_id = fresh_target
        .get("observation_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let ref_id = fresh_target
        .get("ref")
        .and_then(Value::as_str)
        .unwrap_or_default();
    json!({
        "required_before_action": true,
        "recommended": true,
        "command": format!(
            "@ax-get:{{target:{{ref:\"{ref_id}\",observation_id:\"{observation_id}\"}}}}"
        ),
    })
}

fn recovery_recipe(request: &SelectorRefindRequest, fresh_target: Option<&Value>) -> Value {
    let mut steps = vec![
        json!({
            "step": "selector-get",
            "command": format!("@selector-get:{{selector_id:\"{}\",include_history:true}}", request.selector_id),
        }),
        json!({
            "step": "selector-refind",
            "command": format!("@selector-refind:{{selector_id:\"{}\",policy:\"safe\",include_explanations:true}}", request.selector_id),
        }),
    ];
    if let Some(fresh_target) = fresh_target {
        steps.push(json!({
            "step": "verify",
            "command": verify_hint_for_candidate(fresh_target)["command"].clone(),
        }));
        steps.push(json!({
            "step": "explicit-action",
            "note": "verify 通过后再发送 @ax-action / @ax-set-value / @window-activate 等 side-effect 命令",
        }));
    } else {
        steps.push(json!({
            "step": "re-observe",
            "command": "@screenshot:{include_ax:true,ax_required:false,ax_mode:\"interactive\"}",
        }));
    }
    json!(steps)
}

fn blocker(
    code: impl Into<String>,
    message: impl Into<String>,
    category: impl Into<String>,
) -> Value {
    let code = code.into();
    let message = message.into();
    let category = category.into();
    json!({
        "code": code,
        "message": message.clone(),
        "category": category,
        "detail": message,
    })
}

fn blocker_messages(blockers: &[Value], category: &str) -> Vec<Value> {
    blockers
        .iter()
        .filter(|blocker| blocker.get("category").and_then(Value::as_str) == Some(category))
        .map(|blocker| {
            json!({
                "code": blocker.get("code").cloned().unwrap_or(Value::Null),
                "message": blocker.get("message").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn backend_blocker(err: &io::Error) -> Value {
    match err.kind() {
        io::ErrorKind::PermissionDenied => blocker("PERM_DENIED", err.to_string(), "permission"),
        io::ErrorKind::Unsupported => {
            blocker("SELECTOR_BACKEND_UNSUPPORTED", err.to_string(), "backend")
        }
        _ => blocker("SELECTOR_REFIND_FAILED", err.to_string(), "backend"),
    }
}

fn base_response(
    request: &SelectorRefindRequest,
    decision: &str,
    last_seen: Option<DurableSelectorLastSeen>,
    history: Option<Vec<DurableSelectorLastSeen>>,
) -> ResponseBuilder {
    ResponseBuilder::new(json!({
        "kind": "selector-refind",
        "schema": SELECTOR_REFIND_SCHEMA,
        "scoring_version": SELECTOR_SCORE_SCHEMA,
        "status": "complete",
        "selector_id": request.selector_id,
        "decision": decision,
        "policy": request.policy.as_str(),
        "threshold": milli_to_ratio(request.min_confidence_milli),
        "source": request.source.as_ref().map(|source| {
            json!({
                "observation_id": source.observation_id,
                "ref": source.ref_id,
            })
        }),
        "last_seen": last_seen,
        "history": history,
    }))
}

struct ResponseBuilder {
    value: Value,
}

impl ResponseBuilder {
    fn new(value: Value) -> Self {
        Self { value }
    }

    fn with_field(mut self, key: &str, field_value: Value) -> Self {
        if let Some(object) = self.value.as_object_mut() {
            object.insert(key.to_owned(), field_value);
        }
        self
    }

    fn into_value(self) -> Value {
        self.value
    }
}

fn serialize_refind_value(value: Value) -> io::Result<String> {
    serde_json::to_string(&value)
        .map_err(|err| io::Error::other(format!("selector-refind response 序列化失败: {err}")))
}

fn milli_to_ratio(value: u16) -> f64 {
    (value as f64) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_observation::selector::{
        AppSelectorConstraints, ElementSelectorConstraints, PermanentSelector, SelectorConstraints,
        SelectorHints, SelectorRedaction, SelectorSource, WindowSelectorConstraints,
    };

    fn request() -> SelectorRefindRequest {
        SelectorRefindRequest {
            selector_id: "sel-v1-test".to_owned(),
            limit: 10,
            policy: SelectorRefindPolicy::Safe,
            min_confidence_milli: DEFAULT_REFIND_MIN_CONFIDENCE_MILLI,
            include_explanations: true,
            include_history: false,
            source: None,
        }
    }

    fn selector() -> PermanentSelector {
        PermanentSelector {
            schema: PERMANENT_SELECTOR_SCHEMA.to_owned(),
            selector_id: "sel-v1-test".to_owned(),
            fingerprint: "sha256:test".to_owned(),
            kind: SelectorKind::AxElement,
            platform: "macos".to_owned(),
            constraints: SelectorConstraints {
                app: Some(AppSelectorConstraints {
                    name: "System Settings".to_owned(),
                    bundle_id: Some("com.apple.systempreferences".to_owned()),
                }),
                window: Some(WindowSelectorConstraints {
                    role: "AXWindow".to_owned(),
                    title: Some("储存空间".to_owned()),
                    title_match: Some(SelectorMatchMode::Exact),
                }),
                element: Some(ElementSelectorConstraints {
                    role: "AXButton".to_owned(),
                    subrole: None,
                    name: Some("储存空间".to_owned()),
                    name_match: Some(SelectorMatchMode::Exact),
                    description: None,
                    description_match: None,
                    actions: vec!["AXPress".to_owned()],
                }),
                anchors: Vec::new(),
            },
            hints: SelectorHints {
                backend_id: Some("pid:123/window:0/path:7.3".to_owned()),
                ..SelectorHints::default()
            },
            source: SelectorSource {
                observation_id: "obs-old".to_owned(),
                ref_id: "@e1".to_owned(),
                draft_selector_id: "sel-obs-old--e1".to_owned(),
            },
            redaction: SelectorRedaction::metadata_only(),
        }
    }

    fn high_candidate(id: &str, backend_id: &str, ref_id: &str) -> Value {
        json!({
            "candidate_id": id,
            "backend_id": backend_id,
            "kind": "ax-element",
            "role": "AXButton",
            "name": "储存空间",
            "matched_fields": [
                "app.bundle_id",
                "app.name",
                "window.title",
                "element.role",
                "element.name",
                "element.actions"
            ],
            "missing_fields": [],
            "observation": {
                "observation_id": "obs-new",
                "ref": ref_id
            },
            "source": "ax-backend"
        })
    }

    fn fixture(name: &str) -> Value {
        let contents = match name {
            "rebound" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_rebound_v1.json"
            ),
            "needs_disambiguation" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_needs_disambiguation_v1.json"
            ),
            "not_found" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_not_found_v1.json"
            ),
            "blocked_permission" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_blocked_permission_v1.json"
            ),
            "blocked_backend" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_blocked_backend_v1.json"
            ),
            "blocked_schema" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_blocked_schema_v1.json"
            ),
            "verify_skip_audit" => include_str!(
                "../../tests/fixtures/observation_selectors/selector_refind_verify_skip_audit_v1.json"
            ),
            _ => unreachable!("unknown selector refind fixture"),
        };
        serde_json::from_str(contents).unwrap()
    }

    #[test]
    fn refind_rebound_should_match_golden_fixture() {
        let actual = refind_response_from_candidates(
            &request(),
            &selector(),
            None,
            None,
            vec![high_candidate("cand-1", "pid:123/window:0/path:7.3", "@e4")],
        );

        assert_eq!(actual, fixture("rebound"));
    }

    #[test]
    fn refind_multiple_candidates_should_require_disambiguation() {
        let actual = refind_response_from_candidates(
            &request(),
            &selector(),
            None,
            None,
            vec![
                high_candidate("cand-2", "pid:999/window:0/path:1", "@e5"),
                high_candidate("cand-1", "pid:123/window:0/path:7.3", "@e4"),
            ],
        );

        assert_eq!(actual, fixture("needs_disambiguation"));
    }

    #[test]
    fn refind_not_found_should_match_golden_fixture() {
        let actual = refind_response_from_candidates(&request(), &selector(), None, None, vec![]);

        assert_eq!(actual, fixture("not_found"));
    }

    #[test]
    fn refind_candidate_without_fresh_target_should_not_rebound() {
        let mut candidate = high_candidate("cand-1", "pid:123/window:0/path:7.3", "@e4");
        candidate["observation"] = Value::Null;

        let actual =
            refind_response_from_candidates(&request(), &selector(), None, None, vec![candidate]);

        assert_eq!(actual["decision"], "needs_disambiguation");
        assert!(actual.get("fresh_target").is_none());
        assert_eq!(actual["candidates"][0]["confidence_band"], "medium");
        assert_eq!(actual["candidates"][0]["auto_rebind_eligible"], false);
        assert!(actual["candidates"][0]["reject_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "hard_missing.fresh_target"));
    }

    #[test]
    fn refind_blocked_variants_should_match_golden_fixtures() {
        let request = request();
        assert_eq!(
            blocked_response(
                &request,
                None,
                None,
                vec![blocker(
                    "PERM_DENIED",
                    "Accessibility permission denied",
                    "permission",
                )],
            ),
            fixture("blocked_permission")
        );
        assert_eq!(
            blocked_response(
                &request,
                None,
                None,
                vec![blocker(
                    "SELECTOR_BACKEND_UNSUPPORTED",
                    "AX backend unsupported",
                    "backend",
                )],
            ),
            fixture("blocked_backend")
        );
        assert_eq!(
            blocked_response(
                &request,
                None,
                None,
                vec![blocker(
                    "SELECTOR_SCHEMA_UNSUPPORTED",
                    "unsupported selector schema",
                    "schema",
                )],
            ),
            fixture("blocked_schema")
        );
    }

    #[test]
    fn verify_skip_audit_contract_should_match_golden_fixture() {
        let actual = json!({
            "kind": "selector-refind-verify-skip-audit",
            "schema": "rdog.selector.verify-skip-audit.v1",
            "selector_id": "sel-v1-test",
            "fresh_target": {
                "observation_id": "obs-new",
                "ref": "@e4"
            },
            "skip_reason": "caller accepted risk for read-only verification fixture",
            "request_id": 42,
            "actor": "test",
            "timestamp_unix_ms": 1770000000000u64
        });

        assert_eq!(actual, fixture("verify_skip_audit"));
    }
}
