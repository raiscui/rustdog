//! `@computer-act` trace observability (ADR-0006 §Consequences, ticket 18)。
//!
//! 两个产物:
//! - `trace_summary`: inline 摘要,每个 response 都带,4 段耗时 (隐式 observe / ref resolve /
//!   dispatch / verify)
//! - `trace_savefile`: opt-in 落盘路径,仅当 request `trace:"savefile"` 时存在;
//!   走 rdog 现有的 `@savefile` 机制,写到 `rdog_downloads/trace-{id}.json`
//!
//! 设计:
//! - `trace_summary` 4 entry 严格按 ticket 18 acceptance: 即使 verify=none 也要占位
//!   `{step:"verify", status:"skipped"}`
//! - `trace_savefile` 仅在 explicit request 触发;不触发时整个 field omit (跟 ticket 12
//!   verification 一致的 omit 风格)
//! - implicit_observe entry 携带 sub-steps: `screenshot_capture / ax_tree_scan /
//!   ref_resolution`;dispatch entry 携带 sub-steps: 实际 dispatch 路径

use serde_json::{json, Value};
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::control_frames::default_savefile_directory;

/// 4 段 trace 步骤枚举,跟 ticket 18 acceptance 严格对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceStepKind {
    ImplicitObserve,
    RefResolve,
    Dispatch,
    Verify,
}

impl TraceStepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImplicitObserve => "implicit_observe",
            Self::RefResolve => "ref_resolve",
            Self::Dispatch => "dispatch",
            Self::Verify => "verify",
        }
    }
}

/// 单步 trace 摘要 (inline 写到 response.trace_summary)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceStep {
    pub kind: TraceStepKind,
    pub elapsed_ms: u64,
    pub status: TraceStatus,
}

/// Step 状态:
/// - `Ok`: 实际跑了,成功
/// - `Skipped`: 没跑 (e.g., verify=none 时 verify step 是 skipped; 或者 start_box 没给时
///   implicit_observe 的某些 sub-step 是 skipped)
/// - `Failed`: 跑了但失败 (e.g., dispatch 错误)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceStatus {
    Ok,
    Skipped,
    Failed,
}

impl TraceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

/// 4 段 trace 摘要,inline 写到 response.trace_summary。
///
/// ticket 18 acceptance: 严格 4 entry (即使 verify=none 也要 verify:skipped)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraceSummary {
    pub steps: Vec<TraceStep>,
}

impl TraceSummary {
    /// 构造 4 段 trace。verify 步骤如果 policy=none 标 skipped。
    pub fn build(
        implicit_observe_elapsed_ms: u64,
        ref_resolve_status: TraceStatus,
        ref_resolve_elapsed_ms: u64,
        dispatch_elapsed_ms: u64,
        dispatch_ok: bool,
        verify_elapsed_ms: Option<u64>,
        verify_policy_active: bool,
    ) -> Self {
        let verify_step = match (verify_elapsed_ms, verify_policy_active) {
            (Some(ms), true) => TraceStep {
                kind: TraceStepKind::Verify,
                elapsed_ms: ms,
                status: TraceStatus::Ok,
            },
            (None, false) => TraceStep {
                kind: TraceStepKind::Verify,
                elapsed_ms: 0,
                status: TraceStatus::Skipped,
            },
            // 不一致状态 (policy 跑但 ms=None) → 标 failed
            _ => TraceStep {
                kind: TraceStepKind::Verify,
                elapsed_ms: 0,
                status: TraceStatus::Failed,
            },
        };
        Self {
            steps: vec![
                TraceStep {
                    kind: TraceStepKind::ImplicitObserve,
                    elapsed_ms: implicit_observe_elapsed_ms,
                    status: if implicit_observe_elapsed_ms > 0 {
                        TraceStatus::Ok
                    } else {
                        TraceStatus::Skipped
                    },
                },
                TraceStep {
                    kind: TraceStepKind::RefResolve,
                    elapsed_ms: ref_resolve_elapsed_ms,
                    status: ref_resolve_status,
                },
                TraceStep {
                    kind: TraceStepKind::Dispatch,
                    elapsed_ms: dispatch_elapsed_ms,
                    status: if dispatch_ok {
                        TraceStatus::Ok
                    } else {
                        TraceStatus::Failed
                    },
                },
                verify_step,
            ],
        }
    }

    /// trace_step_count (跟 density.trace_step_count 同步)
    pub fn step_count(&self) -> u32 {
        self.steps.len() as u32
    }
}

/// 把 `TraceSummary` 渲染成 response.trace_summary 数组。
///
/// 形状 (ticket 18 acceptance):
/// ```json
/// "trace_summary": [
///   {"step": "implicit_observe", "elapsed_ms": 5, "status": "ok"},
///   {"step": "ref_resolve", "elapsed_ms": 0, "status": "skipped"},
///   {"step": "dispatch", "elapsed_ms": 100, "status": "ok"},
///   {"step": "verify", "elapsed_ms": 50, "status": "ok"}
/// ]
/// ```
pub(crate) fn render_trace_summary(summary: &TraceSummary) -> Value {
    let arr: Vec<Value> = summary
        .steps
        .iter()
        .map(|s| {
            json!({
                "step": s.kind.as_str(),
                "elapsed_ms": s.elapsed_ms,
                "status": s.status.as_str(),
            })
        })
        .collect();
    Value::Array(arr)
}

/// Full trace (含 sub-steps + AX diff report),落盘到 savefile 时用的结构。
///
/// sub-steps:
/// - implicit_observe: `screenshot_capture` / `ax_tree_scan` / `ref_resolution`
/// - dispatch: 实际 dispatch 路径 (e.g., `@click` / `@open-app` / `@cmd`)
#[derive(Debug, Clone)]
pub(crate) struct FullTrace {
    pub implicit_observe: FullTraceImplicitObserve,
    pub dispatch: FullTraceDispatch,
    pub verify: Option<Value>, // best_effort 时是 ax_diff full_report;always 时是 observation_block
    pub verification_passed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FullTraceImplicitObserve {
    pub elapsed_ms: u64,
    pub sub_steps: Vec<SubStep>,
}

#[derive(Debug, Clone)]
pub(crate) struct FullTraceDispatch {
    pub elapsed_ms: u64,
    pub dispatched_to: String,
    pub ok: bool,
    pub sub_steps: Vec<SubStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubStep {
    pub name: String,
    pub elapsed_ms: u64,
    pub status: TraceStatus,
}

impl SubStep {
    pub fn ok(name: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            name: name.into(),
            elapsed_ms,
            status: TraceStatus::Ok,
        }
    }
    pub fn skipped(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            elapsed_ms: 0,
            status: TraceStatus::Skipped,
        }
    }
}

/// 写 full trace 到 savefile。
///
/// 路径: `rdog_downloads/trace-{request_id_or_ts}.json`,由 `default_savefile_directory()` 决定根。
/// 返回写入路径,让 caller 写到 response.trace_savefile。
pub(crate) fn write_trace_savefile(
    request_id: Option<u64>,
    trace: &FullTrace,
) -> io::Result<String> {
    let dir = default_savefile_directory()?;
    std::fs::create_dir_all(&dir)?;

    let ts_ms: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = request_id.unwrap_or(ts_ms);
    let path: PathBuf = dir.join(format!("trace-{ts_ms}-{id}.json"));

    let json = serde_json::to_string_pretty(&trace_to_value(trace))
        .map_err(|e| io::Error::other(format!("trace JSON 序列化失败: {e}")))?;
    std::fs::write(&path, json)?;

    Ok(path.to_string_lossy().to_string())
}

fn trace_to_value(trace: &FullTrace) -> Value {
    json!({
        "implicit_observe": {
            "elapsed_ms": trace.implicit_observe.elapsed_ms,
            "sub_steps": trace.implicit_observe.sub_steps.iter().map(sub_step_to_value).collect::<Vec<_>>(),
        },
        "dispatch": {
            "elapsed_ms": trace.dispatch.elapsed_ms,
            "dispatched_to": trace.dispatch.dispatched_to,
            "ok": trace.dispatch.ok,
            "sub_steps": trace.dispatch.sub_steps.iter().map(sub_step_to_value).collect::<Vec<_>>(),
        },
        "verify": trace.verify,
        "verification_passed": trace.verification_passed,
    })
}

fn sub_step_to_value(s: &SubStep) -> Value {
    json!({
        "name": s.name,
        "elapsed_ms": s.elapsed_ms,
        "status": s.status.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_summary_has_exactly_4_entries() {
        let summary = TraceSummary::build(5, TraceStatus::Skipped, 0, 100, true, Some(50), true);
        assert_eq!(summary.steps.len(), 4);
        assert_eq!(summary.step_count(), 4);
    }

    #[test]
    fn trace_summary_verify_skipped_when_policy_none() {
        let summary = TraceSummary::build(5, TraceStatus::Skipped, 0, 100, true, None, false);
        let verify_step = summary.steps.iter().find(|s| s.kind == TraceStepKind::Verify).unwrap();
        assert_eq!(verify_step.status, TraceStatus::Skipped);
        assert_eq!(verify_step.elapsed_ms, 0);
    }

    #[test]
    fn trace_summary_dispatch_failed_on_error() {
        let summary = TraceSummary::build(5, TraceStatus::Ok, 5, 100, false, None, false);
        let dispatch_step = summary.steps.iter().find(|s| s.kind == TraceStepKind::Dispatch).unwrap();
        assert_eq!(dispatch_step.status, TraceStatus::Failed);
    }

    #[test]
    fn trace_summary_implicit_observe_status_skipped_when_zero_ms() {
        // 没触发 implicit_observe → elapsed=0 → status=skipped
        let summary = TraceSummary::build(0, TraceStatus::Ok, 5, 100, true, None, false);
        let implicit = summary.steps.iter().find(|s| s.kind == TraceStepKind::ImplicitObserve).unwrap();
        assert_eq!(implicit.status, TraceStatus::Skipped);
    }

    #[test]
    fn render_trace_summary_shape() {
        let summary = TraceSummary::build(5, TraceStatus::Skipped, 0, 100, true, Some(50), true);
        let rendered = render_trace_summary(&summary);
        let arr = rendered.as_array().expect("must be array");
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["step"], "implicit_observe");
        assert_eq!(arr[0]["elapsed_ms"], 5);
        assert_eq!(arr[0]["status"], "ok");
        assert_eq!(arr[1]["step"], "ref_resolve");
        assert_eq!(arr[1]["status"], "skipped");
        assert_eq!(arr[2]["step"], "dispatch");
        assert_eq!(arr[2]["elapsed_ms"], 100);
        assert_eq!(arr[3]["step"], "verify");
        assert_eq!(arr[3]["status"], "ok");
    }

    #[test]
    fn sub_step_ok_factory() {
        let s = SubStep::ok("ax_tree_scan", 15);
        assert_eq!(s.name, "ax_tree_scan");
        assert_eq!(s.elapsed_ms, 15);
        assert_eq!(s.status, TraceStatus::Ok);
    }

    #[test]
    fn sub_step_skipped_factory() {
        let s = SubStep::skipped("ref_resolution");
        assert_eq!(s.status, TraceStatus::Skipped);
        assert_eq!(s.elapsed_ms, 0);
    }
}
