// ax_diff/types.rs
//
// ax_diff 子命令所有结构化类型集中在这里,让其它子模块只做"逻辑"工作。
// 这些类型同时被 serde 序列化成 JSON (--format json) 和被 text 渲染。

use serde::Serialize;
use std::collections::BTreeMap;

/// 顶层 summary 计数,方便 summary 输出模式。
#[derive(Debug, Default, Serialize)]
pub struct DiffReport {
    pub windows_added: usize,
    pub windows_removed: usize,
    pub windows_modified: usize,
    pub elements_added: usize,
    pub elements_removed: usize,
    pub elements_modified: usize,
    pub windows: Vec<WindowDiff>,
    pub elements: BTreeMap<String, ElementDiff>,
}

#[derive(Debug, Serialize)]
pub struct WindowDiff {
    pub id: String,
    pub kind: WindowDiffKind,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub changed_fields: Vec<FieldChange>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum WindowDiffKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Serialize)]
pub struct ElementDiff {
    pub window_id: String,
    pub kind: ElementDiffKind,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub changed_fields: Vec<FieldChange>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum ElementDiffKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
}

pub fn report_has_changes(r: &DiffReport) -> bool {
    r.windows_added
        + r.windows_removed
        + r.windows_modified
        + r.elements_added
        + r.elements_removed
        + r.elements_modified
        > 0
}
