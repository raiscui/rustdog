// ax_diff/diff.rs
//
// 计算结构化 diff。设计成两 pass 独立:
//   Pass 1 (window-level shallow): 只比较 window 自身字段
//     (id / pid / process_name / title / role / focused / rect),
//     跳过 elements / children。这样 nested 元素改动不会把整个
//     window 标 modified, 进而让 element 级计数被吞掉。
//   Pass 2 (element-level deep): 对 union window id 集合内每个 wid,
//     不管 window 自身是否变, 都递归配对 element 树, 独立报告
//     added / removed / modified。element 自身字段比对走 shallow,
//     跳过 children (因为 children 内部 element 已经被
//     collect_element_index 递归独立配对)。

use crate::ax_diff::types::{
    DiffReport, ElementDiff, ElementDiffKind, FieldChange, WindowDiff, WindowDiffKind,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn compute_diff(before: &Value, after: &Value, _max_depth: usize) -> DiffReport {
    let mut report = DiffReport::default();
    let before_windows = windows_index(before);
    let after_windows = windows_index(after);
    let mut all_window_ids: Vec<String> = before_windows
        .keys()
        .chain(after_windows.keys())
        .cloned()
        .collect();
    all_window_ids.sort();
    all_window_ids.dedup();

    // Pass 1: window-level shallow diff。
    for wid in &all_window_ids {
        match (before_windows.get(wid), after_windows.get(wid)) {
            (Some(b), None) => {
                report.windows_removed += 1;
                report.windows.push(WindowDiff {
                    id: wid.clone(),
                    kind: WindowDiffKind::Removed,
                    before: Some(b.clone()),
                    after: None,
                    changed_fields: Vec::new(),
                });
            }
            (None, Some(a)) => {
                report.windows_added += 1;
                report.windows.push(WindowDiff {
                    id: wid.clone(),
                    kind: WindowDiffKind::Added,
                    before: None,
                    after: Some(a.clone()),
                    changed_fields: Vec::new(),
                });
            }
            (Some(b), Some(a)) => {
                let mut changed = Vec::new();
                collect_window_field_changes(&mut changed, b, a);
                if !changed.is_empty() {
                    report.windows_modified += 1;
                    report.windows.push(WindowDiff {
                        id: wid.clone(),
                        kind: WindowDiffKind::Modified,
                        before: Some(b.clone()),
                        after: Some(a.clone()),
                        changed_fields: changed,
                    });
                }
            }
            (None, None) => {}
        }
    }

    // Pass 2: element-level deep diff, 与 windows_modified 完全独立。
    for wid in &all_window_ids {
        let before_win = before_windows.get(wid);
        let after_win = after_windows.get(wid);
        match (before_win, after_win) {
            (Some(b), Some(a)) => {
                let before_elements = collect_element_index(b);
                let after_elements = collect_element_index(a);
                let mut all_eids: Vec<String> = before_elements
                    .keys()
                    .chain(after_elements.keys())
                    .cloned()
                    .collect();
                all_eids.sort();
                all_eids.dedup();
                for eid in all_eids {
                    match (before_elements.get(&eid), after_elements.get(&eid)) {
                        (Some(b), None) => {
                            report.elements.insert(
                                eid.clone(),
                                ElementDiff {
                                    window_id: wid.clone(),
                                    kind: ElementDiffKind::Removed,
                                    before: Some(b.clone()),
                                    after: None,
                                    changed_fields: Vec::new(),
                                },
                            );
                            report.elements_removed += 1;
                        }
                        (None, Some(a)) => {
                            report.elements.insert(
                                eid.clone(),
                                ElementDiff {
                                    window_id: wid.clone(),
                                    kind: ElementDiffKind::Added,
                                    before: None,
                                    after: Some(a.clone()),
                                    changed_fields: Vec::new(),
                                },
                            );
                            report.elements_added += 1;
                        }
                        (Some(b), Some(a)) => {
                            // element-level diff 只看 element 自身 shallow 字段,
                            // 跳过 children 数组 —— children 内部的 element
                            // 已经被 collect_element_index 递归配对, 各自独立
                            // 计入 elements_modified/added/removed, 不应该
                            // 把嵌套 child 改动冒泡到 outer element。
                            let mut changed = Vec::new();
                            collect_element_field_changes(&mut changed, b, a);
                            if !changed.is_empty() {
                                report.elements.insert(
                                    eid.clone(),
                                    ElementDiff {
                                        window_id: wid.clone(),
                                        kind: ElementDiffKind::Modified,
                                        before: Some(b.clone()),
                                        after: Some(a.clone()),
                                        changed_fields: changed,
                                    },
                                );
                                report.elements_modified += 1;
                            }
                        }
                        (None, None) => {}
                    }
                }
            }
            (Some(b), None) => {
                for eid in collect_element_ids(b) {
                    report.elements.insert(
                        eid.clone(),
                        ElementDiff {
                            window_id: wid.clone(),
                            kind: ElementDiffKind::Removed,
                            before: Some(lookup_element(b, &eid).cloned().unwrap_or(Value::Null)),
                            after: None,
                            changed_fields: Vec::new(),
                        },
                    );
                    report.elements_removed += 1;
                }
            }
            (None, Some(a)) => {
                for eid in collect_element_ids(a) {
                    report.elements.insert(
                        eid.clone(),
                        ElementDiff {
                            window_id: wid.clone(),
                            kind: ElementDiffKind::Added,
                            before: None,
                            after: Some(lookup_element(a, &eid).cloned().unwrap_or(Value::Null)),
                            changed_fields: Vec::new(),
                        },
                    );
                    report.elements_added += 1;
                }
            }
            (None, None) => {}
        }
    }
    report
}

fn windows_index(snapshot: &Value) -> BTreeMap<String, Value> {
    let mut idx = BTreeMap::new();
    if let Some(windows) = snapshot.get("windows").and_then(|w| w.as_array()) {
        for w in windows {
            if let Some(id) = w.get("id").and_then(|v| v.as_str()) {
                idx.insert(id.to_string(), w.clone());
            }
        }
    }
    idx
}

fn collect_element_ids(window: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(elements) = window.get("elements").and_then(|e| e.as_array()) {
        for e in elements {
            collect_element_ids_into(e, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn collect_element_ids_into(value: &Value, out: &mut Vec<String>) {
    if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
        out.push(id.to_string());
    }
    if let Some(children) = value.get("children").and_then(|c| c.as_array()) {
        for c in children {
            collect_element_ids_into(c, out);
        }
    }
}

fn collect_element_index(window: &Value) -> BTreeMap<String, Value> {
    let mut idx = BTreeMap::new();
    if let Some(elements) = window.get("elements").and_then(|e| e.as_array()) {
        for e in elements {
            collect_element_index_into(e, &mut idx);
        }
    }
    idx
}

fn collect_element_index_into(value: &Value, idx: &mut BTreeMap<String, Value>) {
    if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
        idx.insert(id.to_string(), value.clone());
    }
    if let Some(children) = value.get("children").and_then(|c| c.as_array()) {
        for c in children {
            collect_element_index_into(c, idx);
        }
    }
}

fn lookup_element<'a>(window: &'a Value, eid: &str) -> Option<&'a Value> {
    let elements = window.get("elements").and_then(|e| e.as_array())?;
    for e in elements {
        if let Some(found) = lookup_element_in_subtree(e, eid) {
            return Some(found);
        }
    }
    None
}

fn lookup_element_in_subtree<'a>(value: &'a Value, eid: &str) -> Option<&'a Value> {
    if value.get("id").and_then(|v| v.as_str()) == Some(eid) {
        return Some(value);
    }
    if let Some(children) = value.get("children").and_then(|c| c.as_array()) {
        for c in children {
            if let Some(found) = lookup_element_in_subtree(c, eid) {
                return Some(found);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------
// 字段级 shallow 比对
// ---------------------------------------------------------------------

fn collect_window_field_changes(out: &mut Vec<FieldChange>, before: &Value, after: &Value) {
    // 只比较 window 自身的 shallow 字段,跳过 elements / children 这种
    // 嵌套结构,避免元素级改动把 window 标 modified。
    let (Some(b), Some(a)) = (before.as_object(), after.as_object()) else {
        if before != after {
            out.push(FieldChange {
                field: "(root)".to_string(),
                before: before.clone(),
                after: after.clone(),
            });
        }
        return;
    };
    let mut keys: Vec<&String> = b.keys().chain(a.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        if matches!(
            k.as_str(),
            "elements" | "children" | "ref" | "ax_path" | "observation"
        ) {
            continue;
        }
        let bv = b.get(k).cloned().unwrap_or(Value::Null);
        let av = a.get(k).cloned().unwrap_or(Value::Null);
        if bv != av {
            out.push(FieldChange {
                field: k.clone(),
                before: bv,
                after: av,
            });
        }
    }
}

fn collect_element_field_changes(out: &mut Vec<FieldChange>, before: &Value, after: &Value) {
    // element shallow 字段比对。跳过 children / ref / ax_path,
    // 因为 children 内部 element 由 collect_element_index 独立配对。
    // actions / 等标量数组走 diff_scalar_array 集合语义,这样
    // 新增/删除单个 action 不会被当作整体变化淹没。
    let (Some(b), Some(a)) = (before.as_object(), after.as_object()) else {
        if before != after {
            out.push(FieldChange {
                field: "(root)".to_string(),
                before: before.clone(),
                after: after.clone(),
            });
        }
        return;
    };
    let mut keys: Vec<&String> = b.keys().chain(a.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        if matches!(
            k.as_str(),
            "children" | "ref" | "ax_path" | "id" | "observation"
        ) {
            continue;
        }
        let bv = b.get(k).cloned().unwrap_or(Value::Null);
        let av = a.get(k).cloned().unwrap_or(Value::Null);
        if bv == av {
            continue;
        }
        match (&bv, &av) {
            (Value::Array(barr), Value::Array(aarr))
                if is_scalar_array(barr) || is_scalar_array(aarr) =>
            {
                diff_scalar_array(barr, aarr, k, out);
            }
            _ => {
                out.push(FieldChange {
                    field: k.clone(),
                    before: bv,
                    after: av,
                });
            }
        }
    }
}

fn is_scalar_array(arr: &[Value]) -> bool {
    !arr.is_empty()
        && arr
            .iter()
            .all(|v| v.is_string() || v.is_number() || v.is_boolean())
}

fn diff_scalar_array(before: &[Value], after: &[Value], prefix: &str, out: &mut Vec<FieldChange>) {
    // 把标量数组当作 multiset 处理: 报 added / removed 单项,
    // 这样 actions / ax_required 之类数组的新增项不会被淹没。
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for v in before {
        let key = v.to_string();
        *counts.entry(key).or_insert(0) -= 1;
    }
    for v in after {
        let key = v.to_string();
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for (k, c) in &counts {
        if *c > 0 {
            for _ in 0..*c {
                added.push(k.clone());
            }
        } else if *c < 0 {
            for _ in 0..(-*c) {
                removed.push(k.clone());
            }
        }
    }
    if !added.is_empty() || !removed.is_empty() {
        out.push(FieldChange {
            field: format!("{prefix}.added"),
            before: Value::Array(Vec::new()),
            after: serde_json::to_value(added).unwrap_or(Value::Array(Vec::new())),
        });
        out.push(FieldChange {
            field: format!("{prefix}.removed"),
            before: serde_json::to_value(removed).unwrap_or(Value::Array(Vec::new())),
            after: Value::Array(Vec::new()),
        });
    }
}
