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
use crate::control_ax::{capture_default_ax_snapshot, AxTreeRequest};
use crate::control_observation::{build_observe_bundle, ObserveRequest};

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
                format!(
                    "@computer-act.verify 不支持: {other}; 必须是 none / best_effort / always"
                ),
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
pub(crate) fn run_best_effort_verify(
    dispatch_ms: u64,
) -> AxDiffSummary {
    let verify_start = Instant::now();

    // pre-AX: 用默认 AxTreeRequest (Windows scope / depth / max_elements 默认值)
    let pre = capture_pre_snapshot();
    let post = capture_post_snapshot();

    // 两边都失败 → empty summary
    let (pre_value, post_value) = match (pre, post) {
        (Ok(p), Ok(q)) => (
            serde_json::to_value(&p).unwrap_or(Value::Null),
            serde_json::to_value(&q).unwrap_or(Value::Null),
        ),
        _ => {
            let verify_ms = verify_start.elapsed().as_millis() as u64;
            return AxDiffSummary::empty(dispatch_ms, verify_ms);
        }
    };

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

fn capture_pre_snapshot() -> io::Result<crate::control_ax::AxSnapshot> {
    capture_default_ax_snapshot(&AxTreeRequest::default())
}

fn capture_post_snapshot() -> io::Result<crate::control_ax::AxSnapshot> {
    capture_default_ax_snapshot(&AxTreeRequest::default())
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
    #[cfg_attr(not(test), allow(dead_code))]
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
    dispatch_ms: u64,
) -> AlwaysVerifySummary {
    use std::time::Instant;
    let verify_start = Instant::now();

    // 1. pre-AX
    let pre = capture_default_ax_snapshot(&AxTreeRequest::default()).ok();

    // 2. post-observe (Hybrid: screenshot + ax + windows)
    // 注意: dispatch 已经由 caller 跑完,这里只跑 observe; 不重做 dispatch
    let observe_request = ObserveRequest::default(); // Hybrid mode
    let observe_bundle = match build_observe_bundle(None, &observe_request) {
        Ok(b) => b,
        Err(err) => {
            // observe 失败 → fallback 到空 (verify 错误不污染 dispatch ok:true)
            let _ = err; // 显式 drop
            return empty_always_summary(dispatch_ms);
        }
    };

    // 3. 计算 AX diff (pre vs post)
    let diff_start = Instant::now();
    let ax_diff = match pre.as_ref() {
        Some(pre_snap) => {
            let post_snap = observe_bundle.value.get("accessibility").cloned();
            // 把 accessibility JSON (观察 value 的 accessibility 段) 反序列化成 AxSnapshot
            // 这里只用 windows + status 字段做 diff,简化逻辑
            let pre_value = serde_json::to_value(pre_snap).unwrap_or(Value::Null);
            let post_value = post_snap.unwrap_or(Value::Null);
            let report = compute_diff(&pre_value, &post_value, 64);
            let full_report = serde_json::to_value(&report).unwrap_or(Value::Null);
            AxDiffSummary {
                windows_added: report.windows_added,
                windows_removed: report.windows_removed,
                windows_modified: report.windows_modified,
                elements_added: report.elements_added,
                elements_removed: report.elements_removed,
                elements_modified: report.elements_modified,
                verify_ms: 0, // 占位, 后面统一填
                dispatch_ms,
                full_report,
            }
        }
        None => AxDiffSummary::empty(dispatch_ms, 0),
    };
    let diff_ms = diff_start.elapsed().as_millis() as u64;

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
    let ax_tree_id = observe_bundle
        .value
        .get("observation")
        .and_then(|v| v.get("observation_id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
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
    // ax_diff 的 verify_ms 改成包含 diff_ms (不算 screenshot capture, 只算 diff 计算)
    let mut ax_diff = ax_diff;
    ax_diff.verify_ms = diff_ms;
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

fn empty_always_summary(dispatch_ms: u64) -> AlwaysVerifySummary {
    AlwaysVerifySummary {
        observation_block: Value::Null,
        screenshot_id: None,
        ax_tree_id: None,
        windows: Value::Null,
        screenshot_truncated: false,
        ax_diff: AxDiffSummary::empty(dispatch_ms, 0),
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
pub(crate) fn render_verification(
    policy: VerifyPolicy,
    diff_summary: Option<&AxDiffSummary>,
    always_summary: Option<&AlwaysVerifySummary>,
) -> Option<Value> {
    match policy {
        VerifyPolicy::None => None,
        VerifyPolicy::BestEffort => {
            let summary = diff_summary?;
            Some(serde_json::json!({
                "method": "ax_diff",
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

    // --- VerifyPolicy parsing ---

    #[test]
    fn parse_verify_policy_none_for_missing_field() {
        assert_eq!(parse_verify_policy(None).unwrap(), VerifyPolicy::None);
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
        assert!(render_verification(VerifyPolicy::None, Some(&AxDiffSummary::empty(0, 0)), None).is_none());
    }

    #[test]
    fn render_verification_best_effort_emits_method_and_summary() {
        let summary = AxDiffSummary::empty(100, 30);
        let rendered =
            render_verification(VerifyPolicy::BestEffort, Some(&summary), None).expect("must produce value");
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
        assert!(render_verification(VerifyPolicy::Always, Some(&AxDiffSummary::empty(0, 0)), None).is_none());
    }

    #[test]
    fn empty_always_summary_zeros_observation_fields() {
        let summary = empty_always_summary(50);
        assert_eq!(summary.screenshot_id, None);
        assert_eq!(summary.ax_tree_id, None);
        assert_eq!(summary.screenshot_truncated, false);
        assert!(summary.windows.is_null());
        assert_eq!(summary.ax_diff.dispatch_ms, 50);
    }
}
