// =====================================================================
// changes-first fixture prototype
//
// 目标: 只有在 before/after 使用同一组稳定 identity 时才返回精简 changes。
// identity 不可信时返回 full,避免把“整棵树被替换”误报成少量局部变化。
// 这个模块暂时只做纯函数和 fixture 回归,不改变现有 @computer-act wire。
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
}

pub(crate) fn decide_changes_first(
    before: &Value,
    after: &Value,
    max_depth: usize,
) -> ChangesFirstDecision {
    let before = normalize_snapshot(before);
    let after = normalize_snapshot(after);
    let before_windows = windows_index(&before);
    let after_windows = windows_index(&after);
    let paired_window_ids = before_windows
        .keys()
        .filter(|id| after_windows.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    let compared_windows = before_windows.len().max(after_windows.len());

    if paired_window_ids.is_empty() {
        return full_decision(
            ChangesFirstReason::NoStableWindowIdentity,
            compared_windows,
            0,
            0,
        );
    }
    if paired_window_ids.len() != before_windows.len()
        || paired_window_ids.len() != after_windows.len()
    {
        return full_decision(
            ChangesFirstReason::WindowIdentityChanged,
            compared_windows,
            paired_window_ids.len(),
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
        return full_decision(
            ChangesFirstReason::InsufficientElementIdentity,
            compared_windows,
            paired_window_ids.len(),
            paired_elements,
        )
        .with_compared_elements(compared_elements);
    }

    ChangesFirstDecision {
        mode: ChangesFirstMode::Changes,
        reason: ChangesFirstReason::TrustedStableIdentity,
        paired_windows: paired_window_ids.len(),
        compared_windows,
        paired_elements,
        compared_elements,
        diff: Some(compute_diff(&before, &after, max_depth)),
    }
}

fn full_decision(
    reason: ChangesFirstReason,
    compared_windows: usize,
    paired_windows: usize,
    paired_elements: usize,
) -> ChangesFirstDecision {
    ChangesFirstDecision {
        mode: ChangesFirstMode::Full,
        reason,
        paired_windows,
        compared_windows,
        paired_elements,
        compared_elements: 0,
        diff: None,
    }
}

impl ChangesFirstDecision {
    fn with_compared_elements(mut self, compared_elements: usize) -> Self {
        self.compared_elements = compared_elements;
        self
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
    use serde_json::json;

    fn before_fixture() -> serde_json::Value {
        json!({
            "kind": "ax-tree",
            "schema": "rdog.ax.v1",
            "observation": {"id": "before"},
            "windows": [{
                "id": "pid:42/window:0",
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
        after["observation"]["id"] = json!("after");
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
            "windows": [{"id": "pid:42/window:0", "title": "旧标题", "elements": []}]
        });
        let after = json!({
            "schema": "rdog.ax.v1",
            "windows": [{"id": "pid:42/window:0", "title": "新标题", "elements": []}]
        });

        let decision = decide_changes_first(&before, &after, 4);

        assert_eq!(decision.mode, ChangesFirstMode::Changes);
        assert_eq!(decision.reason, ChangesFirstReason::TrustedStableIdentity);
        assert_eq!(decision.paired_elements, 0);
        assert_eq!(decision.compared_elements, 0);
        assert!(decision.diff.is_some());
    }
}
