//! `@computer-act` verify tier (ADR-0004 V3): `none` / `best_effort` / `always`。
//!
//! ticket 12 + ticket 13 实现:
//! - `VerifyPolicy::None`: 不跑 verification,response 不带 `verification` key
//! - `VerifyPolicy::BestEffort`: 跑 AX-tree diff,response 携带
//!   `verification.method:"ax_diff"` + `verification.ax_diff.{added, removed, changed}`,
//!   同时 `density.{dispatch_ms,verify_ms}` 分别记录两个阶段耗时
//! - `VerifyPolicy::Always`: ticket 14 实现 (full screenshot + AX + windows)
//!
//! 单一真相源: `parse_verify_policy` 是 verify 字段 → VerifyPolicy 的唯一入口,
//! 所有 dispatcher 都通过它,避免字符串分散比对。
//!
//! ticket 11 占位: 当前 `verification: null` 占位 → ticket 12 改 None 时 omit 字段,
//! `best_effort` 改真正跑 AX diff。

use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

use crate::ax_diff::diff::compute_diff;
use crate::control_ax::{
    ax_window_id_from_backend_id, capture_current_ax_window_snapshot, capture_default_ax_snapshot,
    query::matches_query, AxFindQuery, AxSnapshot, AxTreeRequest,
};
use crate::control_observation::{build_observe_bundle, ObserveRequest};
use crate::control_protocol::{ComputerActPostcondition, ComputerActPostconditionKind};

/// ADR-0004 V3: 三档 verify policy。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerifyPolicy {
    /// ticket 12: 默认,不带 `verification` key。
    None,
    /// ticket 13: AX-tree diff,不带 screenshot。
    BestEffort,
    /// ticket 14: full screenshot + AX + windows (本轮不实现,占位)。
    Always,
}

impl VerifyPolicy {
    /// wire 字符串 → policy。无效值返回 `InvalidVerify` 错误,不让 caller 静默降级。
    pub fn from_wire_str(s: &str) -> io::Result<Self> {
        match s {
            "none" => Ok(Self::None),
            "best_effort" => Ok(Self::BestEffort),
            "always" => Ok(Self::Always),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@computer-act.verify 不支持: {other}; 必须是 none / best_effort / always"),
            )),
        }
    }

    /// 序列化回 wire 字符串 (测试 roundtrip 用; production caller 暂时只走 None → "none" 分支)。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::BestEffort => "best_effort",
            Self::Always => "always",
        }
    }
}

/// `request.verify` 字段 → `VerifyPolicy` 入口 (单一真相源)。
///
/// - `None` (字段缺省) → `VerifyPolicy::None` (ticket 12 acceptance criteria)
/// - `Some("none")` → `VerifyPolicy::None`
/// - `Some("best_effort")` → `VerifyPolicy::BestEffort`
/// - `Some("always")` → `VerifyPolicy::Always`
/// - 其它 → `InvalidVerify` 错误 (写进 response `error_code: "invalid_verify"`)
pub(crate) fn parse_verify_policy(raw: Option<&str>) -> io::Result<VerifyPolicy> {
    match raw {
        None => Ok(VerifyPolicy::None),
        Some(s) => VerifyPolicy::from_wire_str(s),
    }
}

/// AX diff 摘要,喂给 response `verification.ax_diff`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AxDiffSummary {
    pub windows_added: usize,
    pub windows_removed: usize,
    pub windows_modified: usize,
    pub elements_added: usize,
    pub elements_removed: usize,
    pub elements_modified: usize,
    /// AX diff 实际耗时 (毫秒)
    pub verify_ms: u64,
    /// 底层 dispatch 耗时 (毫秒,跟 verify 拆分)
    pub dispatch_ms: u64,
    /// 完整的 DiffReport JSON (给客户端扩展用;ticket 18 trace 时也用)
    pub full_report: Value,
}

impl AxDiffSummary {
    /// 拿 empty AX snapshot 对比生成 "zero" summary (verify 跑了但 GUI 没变化)。
    pub fn empty(dispatch_ms: u64, verify_ms: u64) -> Self {
        // full_report 占位为空对象 (verify 失败 fallback 时不暴露内部 ax_diff 结构)
        let mut full_report = serde_json::Map::new();
        full_report.insert("windows_added".into(), Value::from(0));
        full_report.insert("windows_removed".into(), Value::from(0));
        full_report.insert("windows_modified".into(), Value::from(0));
        full_report.insert("elements_added".into(), Value::from(0));
        full_report.insert("elements_removed".into(), Value::from(0));
        full_report.insert("elements_modified".into(), Value::from(0));
        Self {
            windows_added: 0,
            windows_removed: 0,
            windows_modified: 0,
            elements_added: 0,
            elements_removed: 0,
            elements_modified: 0,
            verify_ms,
            dispatch_ms,
            full_report: Value::Object(full_report),
        }
    }
}

/// `verify:"best_effort"` 完整执行流程:
/// 1. 抓 pre-action AX snapshot (空 windows 列表就 fallback 到 empty summary)
/// 2. caller 跑 dispatch (这段耗时由 caller 测量后传 `dispatch_ms`)
/// 3. 抓 post-action AX snapshot
/// 4. `ax_diff::compute_diff` 计算 DiffReport
/// 5. 返回 `AxDiffSummary`
///
/// 任意一步 IO 失败不会 panic,而是 fallback 到 empty summary + `verify_unavailable` 标记。
/// 这是为了不让 verify 错误污染 `ok:true` 的 dispatch 结果 (跟 dispatch 错误分离)。
fn capture_target_snapshot_with(
    target_backend_id: Option<&str>,
    capture_global: impl FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    capture_window: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<AxSnapshot> {
    let request = AxTreeRequest::default();
    match target_backend_id.and_then(ax_window_id_from_backend_id) {
        Some(window_id) => capture_window(window_id, &request),
        None => capture_global(&request),
    }
}

pub(crate) fn capture_pre_snapshot(target_backend_id: Option<&str>) -> io::Result<AxSnapshot> {
    capture_target_snapshot_with(
        target_backend_id,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
    )
}

fn capture_successor_snapshot_with(
    target_backend_id: Option<&str>,
    capture_global: impl FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    capture_window: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<AxSnapshot> {
    let snapshot = capture_target_snapshot_with(target_backend_id, capture_global, capture_window)?;
    if snapshot.capture_status != "complete" {
        return Err(io::Error::other(format!(
            "successor AX capture 不完整: {}",
            snapshot.capture_status
        )));
    }
    snapshot.with_observation("@computer-act")
}

pub(crate) fn capture_successor_snapshot(
    target_backend_id: Option<&str>,
) -> io::Result<AxSnapshot> {
    capture_successor_snapshot_with(
        target_backend_id,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
    )
}

/// 从 successor snapshot 中找回原目标的本地 ref。
///
/// `@eN` 只在单个 observation 内有效,因此下一次 mutation 必须使用这里返回的
/// 新 ref,不能假定旧 ref 在全量 AX snapshot 中仍指向同一 backend。
pub(crate) fn build_successor_target(snapshot: &AxSnapshot, backend_id: &str) -> Option<Value> {
    fn find_element_ref<'a>(
        elements: &'a [crate::control_ax::AxElement],
        backend_id: &str,
    ) -> Option<&'a str> {
        elements.iter().find_map(|element| {
            if element.id == backend_id {
                element.ref_id.as_deref()
            } else {
                find_element_ref(&element.children, backend_id)
            }
        })
    }

    let header = snapshot.observation.as_ref()?;
    let ref_id = snapshot.windows.iter().find_map(|window| {
        if window.id == backend_id {
            window.ref_id.as_deref()
        } else {
            find_element_ref(&window.elements, backend_id)
        }
    })?;

    Some(serde_json::json!({
        "ref": ref_id,
        "observation_id": header.observation_id,
        "epoch": header.created_at_unix_ms,
    }))
}

#[cfg(test)]
mod successor_capture_tests {
    use super::*;
    use crate::control_ax::{AxElement, AxWindow};
    use std::cell::Cell;

    fn target_snapshot() -> AxSnapshot {
        AxSnapshot::complete(
            "test",
            vec![AxWindow {
                id: "pid:73060/window:0".to_string(),
                ref_id: None,
                pid: 73060,
                process_name: "TextEdit".to_string(),
                title: Some("Untitled".to_string()),
                role: "AXWindow".to_string(),
                subrole: None,
                rect: None,
                focused: Some(true),
                elements: vec![AxElement {
                    id: "pid:73060/window:0/path:0.0".to_string(),
                    ref_id: None,
                    role: "AXTextArea".to_string(),
                    subrole: None,
                    name: None,
                    value: Some(String::new()),
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![0, 0],
                    children: Vec::new(),
                }],
            }],
            false,
        )
    }

    #[test]
    fn successor_capture_uses_target_window_when_global_snapshot_would_be_truncated() {
        let global_called = Cell::new(false);
        let snapshot = capture_successor_snapshot_with(
            Some("pid:73060/window:0/path:0.0"),
            |_| {
                global_called.set(true);
                Ok(AxSnapshot::complete("test", Vec::new(), true))
            },
            |window_id, _| {
                assert_eq!(window_id, "pid:73060/window:0");
                Ok(target_snapshot())
            },
        )
        .expect("窗口级 successor capture 应成功");

        assert!(!global_called.get());
        assert!(build_successor_target(&snapshot, "pid:73060/window:0/path:0.0").is_some());
    }

    #[test]
    fn successor_target_is_not_fabricated_when_backend_is_absent() {
        let snapshot = target_snapshot()
            .with_observation("@computer-act")
            .expect("测试 observation 应创建成功");

        assert!(build_successor_target(&snapshot, "pid:73060/window:0/path:9").is_none());
    }
}

pub(crate) fn run_best_effort_verify(
    pre: &AxSnapshot,
    successor: &AxSnapshot,
    dispatch_ms: u64,
) -> AxDiffSummary {
    let verify_start = Instant::now();
    let pre_value = serde_json::to_value(pre).unwrap_or(Value::Null);
    let post_value = serde_json::to_value(successor).unwrap_or(Value::Null);

    let report = compute_diff(&pre_value, &post_value, 64);
    let verify_ms = verify_start.elapsed().as_millis() as u64;
    let full_report = serde_json::to_value(&report).unwrap_or(Value::Null);

    AxDiffSummary {
        windows_added: report.windows_added,
        windows_removed: report.windows_removed,
        windows_modified: report.windows_modified,
        elements_added: report.elements_added,
        elements_removed: report.elements_removed,
        elements_modified: report.elements_modified,
        verify_ms,
        dispatch_ms,
        full_report,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostconditionReport {
    pub kind: ComputerActPostconditionKind,
    pub status: &'static str,
    pub matched: bool,
    pub match_count: usize,
}

pub(crate) fn evaluate_postcondition(
    snapshot: &AxSnapshot,
    condition: &ComputerActPostcondition,
) -> PostconditionReport {
    let match_count = snapshot
        .windows
        .iter()
        .map(|window| count_window_matches(window, &condition.query))
        .sum();
    let matched = match_count > 0;
    let verified = match condition.kind {
        ComputerActPostconditionKind::Exists => matched,
        ComputerActPostconditionKind::NotExists => !matched,
    };
    PostconditionReport {
        kind: condition.kind,
        status: if verified { "verified" } else { "failed" },
        matched,
        match_count,
    }
}

fn count_window_matches(window: &crate::control_ax::AxWindow, query: &AxFindQuery) -> usize {
    fn count_elements(
        window: &crate::control_ax::AxWindow,
        elements: &[crate::control_ax::AxElement],
        query: &AxFindQuery,
    ) -> usize {
        elements
            .iter()
            .map(|element| {
                usize::from(matches_query(window, element, query))
                    + count_elements(window, &element.children, query)
            })
            .sum()
    }
    count_elements(window, &window.elements, query)
}

pub(crate) fn render_postcondition(report: &PostconditionReport) -> Value {
    serde_json::json!({
        "kind": match report.kind {
            ComputerActPostconditionKind::Exists => "exists",
            ComputerActPostconditionKind::NotExists => "not_exists",
        },
        "status": report.status,
        "matched": report.matched,
        "match_count": report.match_count,
    })
}

pub(crate) fn render_unavailable_postcondition(condition: &ComputerActPostcondition) -> Value {
    serde_json::json!({
        "kind": match condition.kind {
            ComputerActPostconditionKind::Exists => "exists",
            ComputerActPostconditionKind::NotExists => "not_exists",
        },
        "status": "unavailable",
        "matched": false,
        "match_count": 0,
    })
}

/// ticket 14: screenshot 体积阈值。超 2MB 标 `screenshot_truncated:true`,
/// 不截断图像 (因为 client 可能需要完整图做 OCR);只标 false 警示 client 自己截。
pub(crate) const ALWAYS_VERIFY_SCREENSHOT_LIMIT_BYTES: usize = 2 * 1024 * 1024;

/// `verify:"always"` 完整观察产物。
///
/// 跟 `AxDiffSummary` 互补:
/// - `AxDiffSummary` 只带 diff 摘要 (轻量,best_effort 用)
/// - `AlwaysVerifySummary` 带完整 post-action 观察 + 同样的 ax_diff
#[derive(Debug, Clone)]
pub(crate) struct AlwaysVerifySummary {
    /// post-action 全量 observe 的 JSON 值 (来自 `ObserveBundle.value`)。
    /// ticket 14 不直接渲染 (response 只取 screenshot_id / ax_tree_id / windows 字段),
    /// 但保留给 ticket 18 trace / 后续可能加 `verification.observation.full` 扩展。
    /// 接入前保持 allow, 避免 test 编译的 dead-code 噪音。
    #[allow(dead_code)]
    pub observation_block: Value,
    /// screenshot_id (来自 observe.visual 段或 observation 段)
    pub screenshot_id: Option<String>,
    /// ax_tree_id (跟 observation_id 同源;观察 capture 时统一一个 id)
    pub ax_tree_id: Option<String>,
    /// windows 状态列表 (来自 observe.windows 段)
    pub windows: Value,
    /// 截图是否超 2 MB 阈值 (只标记,不截断)
    pub screenshot_truncated: bool,
    /// AX diff (跟 best_effort 同口径)
    pub ax_diff: AxDiffSummary,
}

/// `verify:"always"` 完整执行流:
/// 1. 抓 pre-AX (用于 diff, 轻量)
/// 2. caller 跑 dispatch (dispatch_ms 由 caller 传)
/// 3. 抓 post-observe (full screenshot + AX + windows, 走 `build_observe_bundle` Hybrid 模式)
/// 4. 计算 pre/post AX diff
/// 5. 测 screenshot 体积, 超阈值标 truncated
/// 6. 返回 `AlwaysVerifySummary`
///
/// 任意一步失败 fallback 到 empty summary (verify 错误不污染 dispatch 结果)。
pub(crate) fn run_always_verify(
    pre: &AxSnapshot,
    successor: &AxSnapshot,
    dispatch_ms: u64,
) -> AlwaysVerifySummary {
    use std::time::Instant;
    let verify_start = Instant::now();
    let mut ax_diff = run_best_effort_verify(pre, successor, dispatch_ms);

    // successor 已提供唯一 post-action AX。这里仅补 screenshot/windows。
    let mut observe_request = ObserveRequest::default();
    observe_request.include_ax = false;
    let observe_bundle = match build_observe_bundle(None, &observe_request) {
        Ok(b) => b,
        Err(_) => {
            ax_diff.verify_ms = verify_start.elapsed().as_millis() as u64;
            return AlwaysVerifySummary {
                observation_block: Value::Null,
                screenshot_id: None,
                ax_tree_id: successor
                    .observation
                    .as_ref()
                    .map(|header| header.observation_id.clone()),
                windows: Value::Null,
                screenshot_truncated: false,
                ax_diff,
            };
        }
    };

    // 4. 抽取 screenshot_id / ax_tree_id / windows
    let screenshot_id = observe_bundle
        .value
        .get("visual")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .or_else(|| {
            observe_bundle
                .value
                .get("observation")
                .and_then(|v| v.get("observation_id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
    let ax_tree_id = successor
        .observation
        .as_ref()
        .map(|header| header.observation_id.clone());
    let windows = observe_bundle
        .value
        .get("windows")
        .cloned()
        .unwrap_or(Value::Null);

    // 5. 测 screenshot 体积: base64 长度 * 3/4 ≈ 字节数
    let screenshot_bytes = observe_bundle
        .value
        .get("visual")
        .and_then(|v| v.get("image"))
        .and_then(|v| v.get("base64"))
        .and_then(|v| v.as_str())
        .map(|s| s.len() * 3 / 4)
        .unwrap_or(0);
    let screenshot_truncated = screenshot_bytes > ALWAYS_VERIFY_SCREENSHOT_LIMIT_BYTES;

    let total_verify_ms = verify_start.elapsed().as_millis() as u64;
    ax_diff.verify_ms = total_verify_ms;
    ax_diff.dispatch_ms = dispatch_ms;

    AlwaysVerifySummary {
        observation_block: observe_bundle.value,
        screenshot_id,
        ax_tree_id,
        windows,
        screenshot_truncated,
        ax_diff,
        // suppress unused warning if any
        ..summary_with_total_verify(total_verify_ms)
    }
}

fn summary_with_total_verify(total_verify_ms: u64) -> AlwaysVerifySummary {
    AlwaysVerifySummary {
        observation_block: Value::Null,
        screenshot_id: None,
        ax_tree_id: None,
        windows: Value::Null,
        screenshot_truncated: false,
        ax_diff: AxDiffSummary::empty(0, total_verify_ms),
    }
}

/// 把 `AlwaysVerifySummary` 渲染成 response `verification.method:"full"` block。
///
/// ADR-0004 V3 + ticket 14 acceptance:
/// ```json
/// "verification": {
///   "method": "full",
///   "observation": {
///     "screenshot_id": "...",
///     "ax_tree_id": "...",
///     "windows": [...],
///     "screenshot_truncated": false
///   },
///   "ax_diff": { ... }
/// }
/// ```
pub(crate) fn render_always_verification(summary: &AlwaysVerifySummary) -> Value {
    let mut observation = serde_json::Map::new();
    if let Some(sid) = &summary.screenshot_id {
        observation.insert("screenshot_id".into(), Value::String(sid.clone()));
    } else {
        observation.insert("screenshot_id".into(), Value::Null);
    }
    if let Some(axid) = &summary.ax_tree_id {
        observation.insert("ax_tree_id".into(), Value::String(axid.clone()));
    } else {
        observation.insert("ax_tree_id".into(), Value::Null);
    }
    observation.insert("windows".into(), summary.windows.clone());
    observation.insert(
        "screenshot_truncated".into(),
        Value::Bool(summary.screenshot_truncated),
    );

    serde_json::json!({
        "method": "full",
        "observation": Value::Object(observation),
        "ax_diff": {
            "windows_added": summary.ax_diff.windows_added,
            "windows_removed": summary.ax_diff.windows_removed,
            "windows_modified": summary.ax_diff.windows_modified,
            "elements_added": summary.ax_diff.elements_added,
            "elements_removed": summary.ax_diff.elements_removed,
            "elements_modified": summary.ax_diff.elements_modified,
            "changed": summary.ax_diff.windows_modified + summary.ax_diff.elements_modified,
        },
    })
}

/// 把 `AxDiffSummary` 渲染成 response `verification` 字段的 JSON 值。
///
/// ADR-0004 V3 形状:
/// ```json
/// "verification": {
///   "method": "ax_diff",
///   "ax_diff": {
///     "added": N, "removed": N, "changed": N,
///     "windows_added": N, "windows_removed": N, "windows_modified": N,
///     "elements_added": N, "elements_removed": N, "elements_modified": N
///   }
/// }
/// ```
///
/// `None` policy 直接返回 `None`,caller 不写 verification 字段 (ticket 12 acceptance)。
/// 通过 `AxDiffSummary` 推导 `verification.status` (feature/computer-act-outcome-3state)。
///
/// - 任意结构或字段变化 → "verified"
/// - 完全无变化 → "failed"
///
/// AX diff 不能证明业务结果在动作前已存在,因此不生成 `preexisting`。
fn verification_status_for_diff(summary: &AxDiffSummary) -> &'static str {
    let changed = summary.windows_modified + summary.elements_modified;
    let morphed = summary.windows_added
        + summary.windows_removed
        + summary.elements_added
        + summary.elements_removed;
    if changed + morphed > 0 {
        "verified"
    } else {
        "failed"
    }
}

pub(crate) fn render_verification(
    policy: VerifyPolicy,
    diff_summary: Option<&AxDiffSummary>,
    always_summary: Option<&AlwaysVerifySummary>,
) -> Option<Value> {
    match policy {
        VerifyPolicy::None => None,
        VerifyPolicy::BestEffort => {
            let summary = diff_summary?;
            // AX diff 只表达动作前后是否变化。业务条件由 postcondition 单独判断。
            let status = verification_status_for_diff(summary);
            Some(serde_json::json!({
                "method": "ax_diff",
                "status": status,
                "ax_diff": {
                    "windows_added": summary.windows_added,
                    "windows_removed": summary.windows_removed,
                    "windows_modified": summary.windows_modified,
                    "elements_added": summary.elements_added,
                    "elements_removed": summary.elements_removed,
                    "elements_modified": summary.elements_modified,
                    // "changed" 是三态的 brief summary (windows_modified + elements_modified)
                    "changed": summary.windows_modified + summary.elements_modified,
                },
                // 完整 DiffReport 也带 (客户端按需展开;ticket 18 trace 时复用)
                "report": summary.full_report,
            }))
        }
        VerifyPolicy::Always => {
            let summary = always_summary?;
            Some(render_always_verification(summary))
        }
    }
}

/// 把 `density` 字段渲染成 JSON 值。
///
/// ADR-0006 §3: `density` 包含 `dispatch_ms` / `verify_ms` / `implicit_observe_ms`,

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_ax::{AxElement, AxWindow};
    use crate::control_protocol::{ComputerActPostcondition, ComputerActPostconditionKind};

    // --- VerifyPolicy parsing ---

    #[test]
    fn parse_verify_policy_none_for_missing_field() {
        assert_eq!(parse_verify_policy(None).unwrap(), VerifyPolicy::None);
    }

    #[test]
    fn postcondition_exists_and_not_exists_share_ax_matcher() {
        let snapshot = AxSnapshot::complete(
            "test",
            vec![AxWindow {
                id: "pid:42/window:0".to_owned(),
                ref_id: None,
                pid: 42,
                process_name: "Calculator".to_owned(),
                title: Some("Calculator".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: Some(true),
                elements: vec![AxElement {
                    id: "pid:42/window:0/path:1".to_owned(),
                    ref_id: None,
                    role: "AXStaticText".to_owned(),
                    subrole: None,
                    name: Some("Result".to_owned()),
                    value: Some("42".to_owned()),
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![1],
                    children: Vec::new(),
                }],
            }],
            false,
        );
        let query = AxFindQuery {
            role: Some("AXStaticText".to_owned()),
            value: Some("42".to_owned()),
            ..AxFindQuery::default()
        };

        let exists = evaluate_postcondition(
            &snapshot,
            &ComputerActPostcondition {
                kind: ComputerActPostconditionKind::Exists,
                query: query.clone(),
            },
        );
        assert_eq!(exists.status, "verified");
        assert_eq!(exists.match_count, 1);

        let not_exists = evaluate_postcondition(
            &snapshot,
            &ComputerActPostcondition {
                kind: ComputerActPostconditionKind::NotExists,
                query,
            },
        );
        assert_eq!(not_exists.status, "failed");
        assert!(not_exists.matched);

        let absent = evaluate_postcondition(
            &snapshot,
            &ComputerActPostcondition {
                kind: ComputerActPostconditionKind::NotExists,
                query: AxFindQuery {
                    name: Some("Missing".to_owned()),
                    ..AxFindQuery::default()
                },
            },
        );
        assert_eq!(absent.status, "verified");
        assert_eq!(absent.match_count, 0);
    }

    #[test]
    fn parse_verify_policy_accepts_all_three_wire_strings() {
        assert_eq!(
            parse_verify_policy(Some("none")).unwrap(),
            VerifyPolicy::None
        );
        assert_eq!(
            parse_verify_policy(Some("best_effort")).unwrap(),
            VerifyPolicy::BestEffort
        );
        assert_eq!(
            parse_verify_policy(Some("always")).unwrap(),
            VerifyPolicy::Always
        );
    }

    #[test]
    fn parse_verify_policy_rejects_unknown_values() {
        let err = parse_verify_policy(Some("maybe")).unwrap_err();
        assert!(err.to_string().contains("不支持"));
        assert!(err.to_string().contains("maybe"));
    }

    #[test]
    fn verify_policy_as_wire_str_roundtrips() {
        for p in [
            VerifyPolicy::None,
            VerifyPolicy::BestEffort,
            VerifyPolicy::Always,
        ] {
            let s = p.as_wire_str();
            assert_eq!(VerifyPolicy::from_wire_str(s).unwrap(), p);
        }
    }

    // --- AxDiffSummary ---

    #[test]
    fn empty_summary_zeros_all_fields() {
        let s = AxDiffSummary::empty(120, 45);
        assert_eq!(s.dispatch_ms, 120);
        assert_eq!(s.verify_ms, 45);
        assert_eq!(s.windows_added, 0);
        assert_eq!(s.elements_added, 0);
    }

    // --- render_verification ---

    #[test]
    fn render_verification_none_returns_none() {
        // ticket 12 acceptance: None policy 不写 verification 字段。
        assert!(render_verification(VerifyPolicy::None, None, None).is_none());
        assert!(
            render_verification(VerifyPolicy::None, Some(&AxDiffSummary::empty(0, 0)), None)
                .is_none()
        );
    }

    #[test]
    fn render_verification_best_effort_emits_method_and_summary() {
        let summary = AxDiffSummary::empty(100, 30);
        let rendered = render_verification(VerifyPolicy::BestEffort, Some(&summary), None)
            .expect("must produce value");
        assert_eq!(rendered["method"], "ax_diff");
        assert_eq!(rendered["ax_diff"]["windows_added"], 0);
        assert_eq!(rendered["ax_diff"]["elements_added"], 0);
        assert_eq!(rendered["ax_diff"]["changed"], 0);
        // full report 也带,客户端可扩展
        assert!(rendered["report"].is_object());
    }

    #[test]
    fn render_verification_best_effort_without_summary_returns_none() {
        // 防御:caller 漏传 summary 时不要 panic
        assert!(render_verification(VerifyPolicy::BestEffort, None, None).is_none());
    }

    #[test]
    fn render_verification_always_is_deferred_to_ticket_14() {
        // ticket 14 实现;本轮返回 None (等同 no verification block)
        assert!(render_verification(VerifyPolicy::Always, None, None).is_none());
    }

    // --- Always (ticket 14) ---

    #[test]
    fn screenshot_limit_threshold_is_2mb() {
        // 防止后续误改: 2MB 是 ticket 14 acceptance criteria 硬约束。
        assert_eq!(ALWAYS_VERIFY_SCREENSHOT_LIMIT_BYTES, 2 * 1024 * 1024);
    }

    #[test]
    fn render_always_verification_shape() {
        let summary = AlwaysVerifySummary {
            observation_block: serde_json::json!({"kind": "observe"}),
            screenshot_id: Some("screenshot-1234".to_string()),
            ax_tree_id: Some("obs-5678".to_string()),
            windows: serde_json::json!([{"id": "win-1", "title": "Calculator"}]),
            screenshot_truncated: false,
            ax_diff: AxDiffSummary::empty(100, 50),
        };
        let rendered = render_always_verification(&summary);
        assert_eq!(rendered["method"], "full");
        assert_eq!(rendered["observation"]["screenshot_id"], "screenshot-1234");
        assert_eq!(rendered["observation"]["ax_tree_id"], "obs-5678");
        assert_eq!(rendered["observation"]["screenshot_truncated"], false);
        assert!(rendered["observation"]["windows"].is_array());
        assert!(rendered["ax_diff"].is_object());
        assert_eq!(rendered["ax_diff"]["elements_added"], 0);
    }

    #[test]
    fn render_always_verification_screenshot_truncated_propagates() {
        let summary = AlwaysVerifySummary {
            observation_block: Value::Null,
            screenshot_id: Some("s".to_string()),
            ax_tree_id: Some("a".to_string()),
            windows: Value::Null,
            screenshot_truncated: true, // 超 2MB 阈值
            ax_diff: AxDiffSummary::empty(0, 0),
        };
        let rendered = render_always_verification(&summary);
        assert_eq!(rendered["observation"]["screenshot_truncated"], true);
    }

    #[test]
    fn render_verification_always_dispatches_to_always_renderer() {
        let summary = AlwaysVerifySummary {
            observation_block: Value::Null,
            screenshot_id: Some("s".to_string()),
            ax_tree_id: Some("a".to_string()),
            windows: Value::Array(vec![]),
            screenshot_truncated: false,
            ax_diff: AxDiffSummary::empty(100, 200),
        };
        let rendered = render_verification(VerifyPolicy::Always, None, Some(&summary))
            .expect("Always should produce value");
        assert_eq!(rendered["method"], "full");
        assert_eq!(rendered["observation"]["screenshot_id"], "s");
    }

    #[test]
    fn render_verification_always_without_summary_returns_none() {
        // 防御:caller 漏传 always_summary 时不要 panic
        assert!(render_verification(VerifyPolicy::Always, None, None).is_none());
        assert!(render_verification(
            VerifyPolicy::Always,
            Some(&AxDiffSummary::empty(0, 0)),
            None
        )
        .is_none());
    }

    // ====== verification_status_for_diff tests (feature/computer-act-outcome-3state) ======
    #[test]
    fn verification_status_verified_when_modified_greater_than_zero() {
        // changed > 0 → "verified" (AX diff 显示有变化, 动作生效)
        let summary = AxDiffSummary {
            windows_added: 0,
            windows_removed: 0,
            windows_modified: 1,
            elements_added: 0,
            elements_removed: 0,
            elements_modified: 2,
            verify_ms: 0,
            dispatch_ms: 0,
            full_report: Value::Null,
        };
        assert_eq!(verification_status_for_diff(&summary), "verified");
    }

    #[test]
    fn verification_status_verified_when_added_or_removed() {
        // 元素增加或删除同样是可验证的动作后变化。
        let summary = AxDiffSummary {
            windows_added: 1,
            windows_removed: 0,
            windows_modified: 0,
            elements_added: 2,
            elements_removed: 1,
            elements_modified: 0,
            verify_ms: 0,
            dispatch_ms: 0,
            full_report: Value::Null,
        };
        assert_eq!(verification_status_for_diff(&summary), "verified");
    }

    #[test]
    fn verification_status_failed_when_no_changes() {
        // 全 0 → "failed" (AX 完全没动)
        let summary = AxDiffSummary::empty(0, 0);
        assert_eq!(verification_status_for_diff(&summary), "failed");
    }
}
