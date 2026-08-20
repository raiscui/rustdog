// =====================================================================
// changes-first fixture prototype
//
// 目标: 只有在 before/after 使用同一组稳定 identity 时才返回精简 changes。
// identity 不可信时返回 full,避免把“整棵树被替换”误报成少量局部变化。
// `@computer-act` 复用这里的纯决策,不维护第二套 diff 或 identity gate。
// =====================================================================

use crate::ax_diff::{diff::compute_diff, normalize::normalize_snapshot};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Pi prototype 使用的保守配对阈值。
///
/// ponytail: 先固定 75% 阈值完成可证伪 prototype;若真实 fixture 显示误拒绝率过高,
/// 再升级为按 root 类型校准的策略,不要现在引入配置层。
pub(crate) const MIN_STABLE_ELEMENT_PAIR_RATE: usize = 75;
pub(crate) const AX_IDENTITY_VERSION: &str = "rdog.ax.identity.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangesFirstMode {
    Changes,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangesFirstReason {
    TrustedStableIdentity,
    NoStableWindowIdentity,
    WindowIdentityChanged,
    InsufficientElementIdentity,
    DuplicateStableIdentity,
    UnknownElementIdentity,
    SchemaMismatch,
    PermissionDenied,
    Unsupported,
    IdentityTruncated,
    ResourceIdentityChanged,
    MissingIdentityMetadata,
    RootIdentityChanged,
    BaseCaptureUnavailable,
    SuccessorCaptureUnavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangesStatus {
    Changes,
    Full,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChangesSummary {
    pub(crate) status: ChangesStatus,
    pub(crate) base_observation_id: Option<String>,
    pub(crate) successor_observation_id: Option<String>,
    pub(crate) identity_version: &'static str,
    pub(crate) pairing_ratio: f64,
    pub(crate) added: Vec<String>,
    pub(crate) updated: Vec<String>,
    pub(crate) removed: Vec<String>,
    pub(crate) fallback_reason: Option<ChangesFirstReason>,
}

/// changes-first 的离线决策结果。
///
/// `paired_*` 和 `compared_*` 保留原始计数,让 fixture 和后续 live ledger 可以直接
/// 复核配对率,不把一个无法解释的布尔值当成可信度来源。
#[derive(Debug, Serialize)]
pub(crate) struct ChangesFirstDecision {
    pub(crate) mode: ChangesFirstMode,
    pub(crate) reason: ChangesFirstReason,
    pub(crate) paired_windows: usize,
    pub(crate) compared_windows: usize,
    pub(crate) paired_elements: usize,
    pub(crate) compared_elements: usize,
    pub(crate) diff: Option<crate::ax_diff::types::DiffReport>,
    pub(crate) changes: ChangesSummary,
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapshotChangesDecision {
    pub(crate) changes: ChangesSummary,
    pub(crate) diff: Option<crate::ax_diff::types::DiffReport>,
}

pub(crate) fn decide_changes_first(
    before: &Value,
    after: &Value,
    max_depth: usize,
) -> ChangesFirstDecision {
    let before_meta = SnapshotMeta::from_value(before);
    let after_meta = SnapshotMeta::from_value(after);
    let before = normalize_snapshot(before);
    let after = normalize_snapshot(after);
    let base_observation_id = before_meta.observation_id.clone();
    let successor_observation_id = after_meta.observation_id.clone();

    if before_meta.schema != after_meta.schema {
        return full_decision_with_ids(
            ChangesFirstReason::SchemaMismatch,
            before_meta,
            after_meta,
            0,
            0,
            0,
            0,
        );
    }
    if before_meta.missing_identity_metadata || after_meta.missing_identity_metadata {
        return full_decision_with_ids(
            ChangesFirstReason::MissingIdentityMetadata,
            before_meta,
            after_meta,
            0,
            0,
            0,
            0,
        );
    }
    if let Some(reason) = before_meta
        .capability_failure
        .or(after_meta.capability_failure)
    {
        return full_decision_with_ids(reason, before_meta, after_meta, 0, 0, 0, 0);
    }
    if before_meta.duplicate_identity || after_meta.duplicate_identity {
        return full_decision_with_ids(
            ChangesFirstReason::DuplicateStableIdentity,
            before_meta,
            after_meta,
            0,
            0,
            0,
            0,
        );
    }
    if before_meta.unknown_element_identity || after_meta.unknown_element_identity {
        return full_decision_with_ids(
            ChangesFirstReason::UnknownElementIdentity,
            before_meta,
            after_meta,
            0,
            0,
            0,
            0,
        );
    }
    let before_windows = windows_index(&before);
    let after_windows = windows_index(&after);
    let paired_window_ids = before_windows
        .keys()
        .filter(|id| after_windows.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let compared_windows = before_windows.len().max(after_windows.len());

    if paired_window_ids.is_empty() {
        return full_decision_with_ids(
            ChangesFirstReason::NoStableWindowIdentity,
            before_meta,
            after_meta,
            compared_windows,
            0,
            0,
            0,
        );
    }
    if paired_window_ids.len() != before_windows.len()
        || paired_window_ids.len() != after_windows.len()
    {
        return full_decision_with_ids(
            ChangesFirstReason::WindowIdentityChanged,
            before_meta,
            after_meta,
            compared_windows,
            paired_window_ids.len(),
            0,
            0,
        );
    }

    if paired_window_ids.iter().any(|window_id| {
        !same_resource_identity(before_windows[window_id], after_windows[window_id])
    }) {
        return full_decision_with_ids(
            ChangesFirstReason::ResourceIdentityChanged,
            before_meta,
            after_meta,
            compared_windows,
            paired_window_ids.len(),
            0,
            0,
        );
    }
    if paired_window_ids
        .iter()
        .any(|window_id| !same_root_identity(before_windows[window_id], after_windows[window_id]))
    {
        return full_decision_with_ids(
            ChangesFirstReason::RootIdentityChanged,
            before_meta,
            after_meta,
            compared_windows,
            paired_window_ids.len(),
            0,
            0,
        );
    }

    let (paired_elements, compared_elements) = paired_window_ids
        .iter()
        .map(|window_id| {
            let before_elements = element_ids(before_windows[window_id]);
            let after_elements = element_ids(after_windows[window_id]);
            let paired = before_elements.intersection(&after_elements).count();
            let compared = before_elements.len().max(after_elements.len());
            (paired, compared)
        })
        .fold(
            (0, 0),
            |(paired_total, compared_total), (paired, compared)| {
                (paired_total + paired, compared_total + compared)
            },
        );

    if compared_elements > 0
        && paired_elements.saturating_mul(100) / compared_elements < MIN_STABLE_ELEMENT_PAIR_RATE
    {
        return full_decision_with_ids(
            ChangesFirstReason::InsufficientElementIdentity,
            before_meta,
            after_meta,
            compared_windows,
            paired_window_ids.len(),
            paired_elements,
            compared_elements,
        );
    }

    let diff = compute_diff(&before, &after, max_depth);
    let changes = summary_from_diff(
        ChangesStatus::Changes,
        base_observation_id,
        successor_observation_id,
        paired_elements,
        compared_elements,
        &diff,
        None,
    );
    ChangesFirstDecision {
        mode: ChangesFirstMode::Changes,
        reason: ChangesFirstReason::TrustedStableIdentity,
        paired_windows: paired_window_ids.len(),
        compared_windows,
        paired_elements,
        compared_elements,
        diff: Some(diff),
        changes,
    }
}

/// 把现有 pre/successor snapshot 转为 `@computer-act` 可内联的可信 changes 摘要。
///
/// capture 缺失和 identity 不可信是两类不同事实。前者返回 unavailable,
/// 后者继续由 `decide_changes_first` 返回 full 和精确 fallback reason。
pub(crate) fn decide_snapshot_changes(
    before: Option<&crate::control_ax::AxSnapshot>,
    after: Option<&crate::control_ax::AxSnapshot>,
) -> SnapshotChangesDecision {
    let Some(before) = before else {
        return SnapshotChangesDecision {
            changes: unavailable_changes(ChangesFirstReason::BaseCaptureUnavailable, None, after),
            diff: None,
        };
    };
    let Some(after) = after else {
        return SnapshotChangesDecision {
            changes: unavailable_changes(
                ChangesFirstReason::SuccessorCaptureUnavailable,
                Some(before),
                None,
            ),
            diff: None,
        };
    };
    let before = serde_json::to_value(before).unwrap_or(Value::Null);
    let after = serde_json::to_value(after).unwrap_or(Value::Null);
    let decision = decide_changes_first(&before, &after, 64);
    SnapshotChangesDecision {
        changes: decision.changes,
        diff: decision.diff,
    }
}

fn unavailable_changes(
    reason: ChangesFirstReason,
    before: Option<&crate::control_ax::AxSnapshot>,
    after: Option<&crate::control_ax::AxSnapshot>,
) -> ChangesSummary {
    ChangesSummary {
        status: ChangesStatus::Unavailable,
        base_observation_id: before
            .and_then(|snapshot| snapshot.observation.as_ref())
            .map(|header| header.observation_id.clone()),
        successor_observation_id: after
            .and_then(|snapshot| snapshot.observation.as_ref())
            .map(|header| header.observation_id.clone()),
        identity_version: AX_IDENTITY_VERSION,
        pairing_ratio: 0.0,
        added: Vec::new(),
        updated: Vec::new(),
        removed: Vec::new(),
        fallback_reason: Some(reason),
    }
}

fn full_decision_with_ids(
    reason: ChangesFirstReason,
    before: SnapshotMeta,
    after: SnapshotMeta,
    compared_windows: usize,
    paired_windows: usize,
    paired_elements: usize,
    compared_elements: usize,
) -> ChangesFirstDecision {
    let pairing_ratio = if compared_elements == 0 {
        0.0
    } else {
        paired_elements as f64 / compared_elements as f64
    };
    ChangesFirstDecision {
        mode: ChangesFirstMode::Full,
        reason,
        paired_windows,
        compared_windows,
        paired_elements,
        compared_elements,
        diff: None,
        changes: ChangesSummary {
            status: ChangesStatus::Full,
            base_observation_id: before.observation_id,
            successor_observation_id: after.observation_id,
            identity_version: AX_IDENTITY_VERSION,
            pairing_ratio,
            added: Vec::new(),
            updated: Vec::new(),
            removed: Vec::new(),
            fallback_reason: Some(reason),
        },
    }
}

fn windows_index(snapshot: &Value) -> BTreeMap<String, &Value> {
    snapshot
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|window| {
            window
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), window))
        })
        .collect()
}

#[derive(Debug, Default)]
struct SnapshotMeta {
    schema: Option<String>,
    observation_id: Option<String>,
    missing_identity_metadata: bool,
    capability_failure: Option<ChangesFirstReason>,
    duplicate_identity: bool,
    unknown_element_identity: bool,
}

impl SnapshotMeta {
    fn from_value(snapshot: &Value) -> Self {
        let schema = snapshot
            .get("schema")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let observation_id = snapshot
            .get("observation")
            .and_then(Value::as_object)
            .and_then(|observation| {
                observation
                    .get("observation_id")
                    .or_else(|| observation.get("id"))
                    .and_then(Value::as_str)
            })
            .map(str::to_owned);
        let mut malformed_shape = false;
        let missing_identity_metadata = schema.as_deref() != Some("rdog.ax.v1")
            || observation_id.is_none()
            || snapshot
                .get("capture_status")
                .and_then(Value::as_str)
                .is_none()
            || snapshot
                .get("permission_status")
                .and_then(Value::as_str)
                .is_none();
        let capability_failure = if snapshot
            .get("capture_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "unsupported")
        {
            Some(ChangesFirstReason::Unsupported)
        } else if snapshot
            .get("permission_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "granted")
        {
            Some(ChangesFirstReason::PermissionDenied)
        } else if snapshot
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            Some(ChangesFirstReason::IdentityTruncated)
        } else if snapshot
            .get("capture_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "complete")
        {
            Some(ChangesFirstReason::Unsupported)
        } else {
            None
        };
        let mut window_ids = BTreeSet::new();
        let mut element_ids = BTreeSet::new();
        let mut duplicate_identity = false;
        let mut unknown_element_identity = false;
        if let Some(windows) = snapshot.get("windows").and_then(Value::as_array) {
            for window in windows {
                if let Some(id) = window.get("id").and_then(Value::as_str) {
                    if id.is_empty() {
                        duplicate_identity = true;
                    }
                    duplicate_identity |= !window_ids.insert(id.to_owned());
                } else {
                    duplicate_identity = true;
                }
                if let Some(elements) = window.get("elements").and_then(Value::as_array) {
                    for element in elements {
                        collect_identity_flags(
                            element,
                            &mut element_ids,
                            &mut duplicate_identity,
                            &mut unknown_element_identity,
                        );
                    }
                } else {
                    malformed_shape = true;
                }
            }
        } else {
            malformed_shape = true;
        }
        Self {
            schema,
            observation_id,
            missing_identity_metadata: missing_identity_metadata || malformed_shape,
            capability_failure,
            duplicate_identity,
            unknown_element_identity,
        }
    }
}

fn collect_identity_flags(
    element: &Value,
    ids: &mut BTreeSet<String>,
    duplicate: &mut bool,
    unknown: &mut bool,
) {
    if let Some(id) = element.get("id").and_then(Value::as_str) {
        if id.is_empty() {
            *duplicate = true;
        }
        *duplicate |= !ids.insert(id.to_owned());
    } else {
        *unknown = true;
    }
    if let Some(children) = element.get("children").and_then(Value::as_array) {
        for child in children {
            collect_identity_flags(child, ids, duplicate, unknown);
        }
    }
}

fn same_resource_identity(before: &Value, after: &Value) -> bool {
    let Some(before_pid) = before.get("pid").and_then(Value::as_i64) else {
        return false;
    };
    let Some(after_pid) = after.get("pid").and_then(Value::as_i64) else {
        return false;
    };
    let Some(before_process) = before.get("process_name").and_then(Value::as_str) else {
        return false;
    };
    let Some(after_process) = after.get("process_name").and_then(Value::as_str) else {
        return false;
    };
    before_pid > 0
        && after_pid > 0
        && !before_process.is_empty()
        && before_pid == after_pid
        && before_process == after_process
}

fn same_root_identity(before: &Value, after: &Value) -> bool {
    let before_roots = before
        .get("elements")
        .and_then(Value::as_array)
        .map(|elements| {
            elements
                .iter()
                .map(|element| element.get("id").and_then(Value::as_str))
                .collect::<Vec<_>>()
        });
    let after_roots = after
        .get("elements")
        .and_then(Value::as_array)
        .map(|elements| {
            elements
                .iter()
                .map(|element| element.get("id").and_then(Value::as_str))
                .collect::<Vec<_>>()
        });
    before_roots == after_roots
}

fn summary_from_diff(
    status: ChangesStatus,
    base_observation_id: Option<String>,
    successor_observation_id: Option<String>,
    paired_elements: usize,
    compared_elements: usize,
    diff: &crate::ax_diff::types::DiffReport,
    fallback_reason: Option<ChangesFirstReason>,
) -> ChangesSummary {
    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    for window in &diff.windows {
        match window.kind {
            crate::ax_diff::types::WindowDiffKind::Added => added.push(window.id.clone()),
            crate::ax_diff::types::WindowDiffKind::Removed => removed.push(window.id.clone()),
            crate::ax_diff::types::WindowDiffKind::Modified => updated.push(window.id.clone()),
        }
    }
    for (id, element) in &diff.elements {
        match element.kind {
            crate::ax_diff::types::ElementDiffKind::Added => added.push(id.clone()),
            crate::ax_diff::types::ElementDiffKind::Removed => removed.push(id.clone()),
            crate::ax_diff::types::ElementDiffKind::Modified => updated.push(id.clone()),
        }
    }
    let pairing_ratio = if compared_elements == 0 {
        1.0
    } else {
        paired_elements as f64 / compared_elements as f64
    };
    ChangesSummary {
        status,
        base_observation_id,
        successor_observation_id,
        identity_version: AX_IDENTITY_VERSION,
        pairing_ratio,
        added,
        updated,
        removed,
        fallback_reason,
    }
}

fn element_ids(window: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(elements) = window.get("elements").and_then(Value::as_array) {
        for element in elements {
            collect_element_ids(element, &mut ids);
        }
    }
    ids
}

fn collect_element_ids(element: &Value, ids: &mut BTreeSet<String>) {
    if let Some(id) = element.get("id").and_then(Value::as_str) {
        ids.insert(id.to_owned());
    }
    if let Some(children) = element.get("children").and_then(Value::as_array) {
        for child in children {
            collect_element_ids(child, ids);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_changes_first, ChangesFirstMode, ChangesFirstReason};
    use serde_json::{json, Value};

    fn before_fixture() -> serde_json::Value {
        json!({
            "kind": "ax-tree",
            "schema": "rdog.ax.v1",
            "capture_status": "complete",
            "permission_status": "granted",
            "truncated": false,
            "observation": {"observation_id": "before"},
            "windows": [{
                "id": "pid:42/window:0",
                "pid": 42,
                "process_name": "Test",
                "ref": "@e1",
                "elements": [
                    {"id": "pid:42/window:0/path:0", "ref": "@e2", "role": "AXWebArea", "children": [
                        {"id": "pid:42/window:0/path:0.0", "ref": "@e3", "role": "AXButton", "title": "保存"},
                        {"id": "pid:42/window:0/path:0.1", "ref": "@e4", "role": "AXButton", "title": "取消"}
                    ]}
                ]
            }]
        })
    }

    #[test]
    fn stable_fixture_returns_changes_view() {
        let mut after = before_fixture();
        after["observation"]["observation_id"] = json!("after");
        after["windows"][0]["elements"][0]["children"][0]["title"] = json!("已保存");
        after["windows"][0]["ref"] = json!("@new-window-ref");

        let decision = decide_changes_first(&before_fixture(), &after, 4);

        assert_eq!(decision.mode, ChangesFirstMode::Changes);
        assert_eq!(decision.reason, ChangesFirstReason::TrustedStableIdentity);
        assert_eq!(decision.paired_windows, 1);
        assert_eq!(decision.compared_windows, 1);
        assert_eq!(decision.paired_elements, 3);
        assert_eq!(decision.compared_elements, 3);
        assert!(decision.diff.is_some());
        assert!(matches!(
            &decision.changes.status,
            super::ChangesStatus::Changes
        ));
        assert_eq!(
            decision.changes.base_observation_id.as_deref(),
            Some("before")
        );
        assert_eq!(
            decision.changes.successor_observation_id.as_deref(),
            Some("after")
        );
        assert_eq!(decision.changes.identity_version, "rdog.ax.identity.v1");
        assert_eq!(decision.changes.pairing_ratio, 1.0);
        assert!(decision
            .changes
            .updated
            .iter()
            .any(|id| id.ends_with("0.0")));
    }

    #[test]
    fn replaced_root_returns_full_view() {
        let mut after = before_fixture();
        after["windows"][0]["id"] = json!("pid:42/window:1");
        after["windows"][0]["elements"][0]["id"] = json!("pid:42/window:1/path:0");

        let decision = decide_changes_first(&before_fixture(), &after, 4);

        assert_eq!(decision.mode, ChangesFirstMode::Full);
        assert_eq!(decision.reason, ChangesFirstReason::NoStableWindowIdentity);
        assert!(decision.diff.is_none());
    }

    #[test]
    fn low_element_identity_returns_full_view() {
        let mut after = before_fixture();
        after["windows"][0]["elements"][0]["children"] = json!([
            {"id": "pid:42/window:0/path:0.0", "role": "AXButton", "title": "已保存"},
            {"id": "new-element-1", "role": "AXButton", "title": "新建"},
            {"id": "new-element-2", "role": "AXButton", "title": "更多"}
        ]);

        let decision = decide_changes_first(&before_fixture(), &after, 4);

        assert_eq!(decision.mode, ChangesFirstMode::Full);
        assert_eq!(
            decision.reason,
            ChangesFirstReason::InsufficientElementIdentity
        );
        assert_eq!(decision.paired_elements, 2);
        assert_eq!(decision.compared_elements, 4);
        assert!(decision.diff.is_none());
    }

    #[test]
    fn window_only_change_is_trusted_without_elements() {
        let before = json!({
            "schema": "rdog.ax.v1",
            "capture_status": "complete",
            "permission_status": "granted",
            "truncated": false,
            "observation": {"observation_id": "before"},
            "windows": [{"id": "pid:42/window:0", "pid": 42, "process_name": "Test", "title": "旧标题", "elements": []}]
        });
        let after = json!({
            "schema": "rdog.ax.v1",
            "capture_status": "complete",
            "permission_status": "granted",
            "truncated": false,
            "observation": {"observation_id": "after"},
            "windows": [{"id": "pid:42/window:0", "pid": 42, "process_name": "Test", "title": "新标题", "elements": []}]
        });

        let decision = decide_changes_first(&before, &after, 4);

        assert_eq!(decision.mode, ChangesFirstMode::Changes);
        assert_eq!(decision.reason, ChangesFirstReason::TrustedStableIdentity);
        assert_eq!(decision.paired_elements, 0);
        assert_eq!(decision.compared_elements, 0);
        assert!(decision.diff.is_some());
    }

    fn ratio_fixture(element_count: usize) -> serde_json::Value {
        let children = (0..element_count.saturating_sub(1))
            .map(|index| {
                json!({
                    "id": format!("pid:42/window:0/path:0.{index}"),
                    "role": "AXButton"
                })
            })
            .collect::<Vec<_>>();
        let elements = vec![json!({
            "id": "pid:42/window:0/path:0",
            "role": "AXGroup",
            "children": children
        })];
        json!({
            "schema": "rdog.ax.v1",
            "capture_status": "complete",
            "permission_status": "granted",
            "truncated": false,
            "observation": {"observation_id": "obs"},
            "windows": [{
                "id": "pid:42/window:0",
                "pid": 42,
                "process_name": "Test",
                "elements": elements
            }]
        })
    }

    #[test]
    fn exactly_seventy_five_percent_pairing_returns_changes() {
        let before = ratio_fixture(4);
        let mut after = ratio_fixture(4);
        after["observation"]["observation_id"] = json!("after");
        after["windows"][0]["elements"][0]["children"]
            .as_array_mut()
            .unwrap()
            .pop();
        let decision = decide_changes_first(&before, &after, 4);
        assert_eq!(decision.mode, ChangesFirstMode::Changes);
        assert_eq!(decision.changes.pairing_ratio, 0.75);
    }

    #[test]
    fn below_seventy_five_percent_pairing_returns_full() {
        let before = ratio_fixture(50);
        let mut after = ratio_fixture(50);
        after["observation"]["observation_id"] = json!("after");
        after["windows"][0]["elements"][0]["children"]
            .as_array_mut()
            .unwrap()
            .truncate(36);
        let decision = decide_changes_first(&before, &after, 4);
        assert_eq!(decision.mode, ChangesFirstMode::Full);
        assert_eq!(
            decision.reason,
            ChangesFirstReason::InsufficientElementIdentity
        );
        assert_eq!(decision.changes.pairing_ratio, 0.74);
    }

    #[test]
    fn identity_risks_return_full_without_diff() {
        let before = ratio_fixture(2);

        let mut duplicate = before.clone();
        duplicate["windows"][0]["elements"][0]["children"][0]["id"] =
            duplicate["windows"][0]["elements"][0]["id"].clone();
        let decision = decide_changes_first(&before, &duplicate, 4);
        assert_eq!(decision.reason, ChangesFirstReason::DuplicateStableIdentity);
        assert!(decision.diff.is_none());

        let mut schema = before.clone();
        schema["schema"] = json!("rdog.ax.v2");
        let decision = decide_changes_first(&before, &schema, 4);
        assert_eq!(decision.reason, ChangesFirstReason::SchemaMismatch);

        let mut resource = before.clone();
        resource["windows"][0]["pid"] = json!(43);
        let decision = decide_changes_first(&before, &resource, 4);
        assert_eq!(decision.reason, ChangesFirstReason::ResourceIdentityChanged);

        let mut root = before.clone();
        root["windows"][0]["elements"][0]["id"] = json!("replacement-root");
        let decision = decide_changes_first(&before, &root, 4);
        assert_eq!(decision.reason, ChangesFirstReason::RootIdentityChanged);

        let mut denied = before.clone();
        denied["permission_status"] = json!("denied");
        let decision = decide_changes_first(&before, &denied, 4);
        assert_eq!(decision.reason, ChangesFirstReason::PermissionDenied);

        let mut truncated = before.clone();
        truncated["truncated"] = json!(true);
        let decision = decide_changes_first(&before, &truncated, 4);
        assert_eq!(decision.reason, ChangesFirstReason::IdentityTruncated);

        let mut unsupported = before.clone();
        unsupported["capture_status"] = json!("unsupported");
        let decision = decide_changes_first(&before, &unsupported, 4);
        assert_eq!(decision.reason, ChangesFirstReason::Unsupported);

        let mut unknown = before.clone();
        unknown["windows"][0]["elements"][0]["id"] = Value::Null;
        let decision = decide_changes_first(&before, &unknown, 4);
        assert_eq!(decision.reason, ChangesFirstReason::UnknownElementIdentity);

        let mut missing = before.clone();
        missing["observation"] = Value::Null;
        let decision = decide_changes_first(&before, &missing, 4);
        assert_eq!(decision.reason, ChangesFirstReason::MissingIdentityMetadata);

        let decision = decide_changes_first(&before, &Value::Null, 4);
        assert_eq!(decision.reason, ChangesFirstReason::SchemaMismatch);
    }
}
