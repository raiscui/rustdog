//! `@computer-act` density metrics (ADR-0006 §Consequences, ticket 17)。
//!
//! 字段名跟 `@gui-probe` 共享,这样客户端可以用同一份 schema 解读两个端点的 density 块。
//!
//! `@computer-act` 专属字段:
//! - `implicit_observe` (bool): 是否触发了 implicit_observe
//! - `implicit_observe_ms` (u64): implicit_observe 耗时
//! - `dispatch_ms` (u64): 底层 primitive dispatch 耗时
//! - `verify_ms` (u64?): verify 耗时 (verify=none 时 omit)
//!
//! 共享字段 (`@gui-probe` 也有):
//! - `backend_request_count`, `control_frame_count`, `elapsed_ms_total`,
//!   `semantic_action_count`, `mouse_fallback_count`, `stale_ref_recovery_count`,
//!   `verification_passed`, `false_success_count`, `payload_bytes`, `trace_step_count`
//!
//! ticket 17 当前只填本 dispatcher 知道的字段;部分字段 (mouse_fallback_count 等)
//! 在 ticket 21 e2e smoke 阶段才补真实值,本轮占 0。

use serde_json::{json, Value};

/// 4 段耗时 (跟 verify 拆开对齐 ADR-0006 §3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComputerActDensity {
    pub dispatch_ms: u64,
    pub implicit_observe_ms: u64,
    pub verify_ms: Option<u64>,
    /// 是否触发了 implicit_observe (start_box 路径 或 stale target+obs_id 路径)
    pub implicit_observe: bool,
    /// verify_passed: true iff verify != none && ax_diff non-empty
    pub verification_passed: bool,
    /// trace_summary 的步骤数 (ticket 18 填, 这里预留)
    pub trace_step_count: u32,
    /// elapsed_ms_total = dispatch_ms + implicit_observe_ms + verify_ms
    pub elapsed_ms_total: u64,
    /// payload_bytes (response JSON 序列化字节数, 这里用 0 占位,call site 算)
    pub payload_bytes: u64,
}

impl ComputerActDensity {
    /// dispatch 拆出来,verify 三档分别 set verify_ms。
    /// caller 在 mod.rs 跑完 dispatch + verify 后构造。
    pub fn new(
        dispatch_ms: u64,
        implicit_observe_ms: u64,
        implicit_observe: bool,
        verify_ms: Option<u64>,
        verification_passed: bool,
        trace_step_count: u32,
    ) -> Self {
        let elapsed_ms_total = dispatch_ms + implicit_observe_ms + verify_ms.unwrap_or(0);
        Self {
            dispatch_ms,
            implicit_observe_ms,
            verify_ms,
            implicit_observe,
            verification_passed,
            trace_step_count,
            elapsed_ms_total,
            payload_bytes: 0, // call site 算 (response_value_json.to_string().len())
        }
    }
}

/// 渲染 density 块 (ADR-0006 完整字段集)。
///
/// field 顺序固定 (先 dispatch_ms / implicit_observe_ms / verify_ms 三段耗时,
/// 再 implicit_observe / verification_passed / 共享计数);客户端按字段名读,
/// 顺序不重要但保持稳定方便日志对照。
pub(crate) fn render_density(d: &ComputerActDensity) -> Value {
    let mut obj = json!({
        // 3 段耗时 (核心)
        "dispatch_ms": d.dispatch_ms,
        "implicit_observe_ms": d.implicit_observe_ms,
        "implicit_observe": d.implicit_observe,

        // 总耗时 + 计数 (跟 @gui-probe 共享字段名)
        "elapsed_ms_total": d.elapsed_ms_total,
        "backend_request_count": 1,
        "control_frame_count": 1,
        "semantic_action_count": 1,

        // ticket 17 占位字段 (mouse_fallback_count / stale_ref_recovery_count /
        // false_success_count 等),ticket 21 真实 GUI 场景才补
        "mouse_fallback_count": 0,
        "stale_ref_recovery_count": 0,
        "false_success_count": 0,

        // verify 结果
        "verification_passed": d.verification_passed,

        // trace 协作 (ticket 18 同步填)
        "trace_step_count": d.trace_step_count,

        // response 体积 (call site 覆盖)
        "payload_bytes": d.payload_bytes,
    });

    // verify_ms 仅在 verify 跑了时存在 (omit vs null 占位,跟 ticket 12 一致)
    if let Some(v) = d.verify_ms {
        obj["verify_ms"] = json!(v);
    }

    obj
}

/// 从 `verify_policy` + `ax_diff_summary` 推导 `verification_passed`。
///
/// ADR-0006: `verification_passed` is true iff `verify` was not `none`
/// and the AX diff was non-empty。
pub(crate) fn compute_verification_passed(
    verify_policy: super::verify::VerifyPolicy,
    ax_diff: Option<&super::verify::AxDiffSummary>,
) -> bool {
    if matches!(verify_policy, super::verify::VerifyPolicy::None) {
        return false;
    }
    match ax_diff {
        None => false,
        Some(s) => {
            // non-empty = 至少一个 added/removed/modified > 0
            s.windows_added > 0
                || s.windows_removed > 0
                || s.windows_modified > 0
                || s.elements_added > 0
                || s.elements_removed > 0
                || s.elements_modified > 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_computer_act::verify::{AxDiffSummary, VerifyPolicy};

    #[test]
    fn render_density_contains_all_advisory_0006_fields() {
        let d = ComputerActDensity::new(100, 5, true, Some(50), true, 4);
        let rendered = render_density(&d);
        // 3 段耗时
        assert_eq!(rendered["dispatch_ms"], 100);
        assert_eq!(rendered["implicit_observe_ms"], 5);
        assert_eq!(rendered["implicit_observe"], true);
        assert_eq!(rendered["verify_ms"], 50);
        // 共享字段
        assert_eq!(rendered["backend_request_count"], 1);
        assert_eq!(rendered["control_frame_count"], 1);
        assert_eq!(rendered["semantic_action_count"], 1);
        assert_eq!(rendered["elapsed_ms_total"], 155);
        assert_eq!(rendered["verification_passed"], true);
        assert_eq!(rendered["trace_step_count"], 4);
    }

    #[test]
    fn render_density_omits_verify_ms_when_verify_none() {
        let d = ComputerActDensity::new(100, 5, false, None, false, 4);
        let rendered = render_density(&d);
        assert!(rendered.get("verify_ms").is_none(),
            "verify_ms omitted when verify=none (vs null placeholder)");
    }

    #[test]
    fn elapsed_ms_total_sums_three_stages() {
        let d = ComputerActDensity::new(100, 5, true, Some(50), true, 4);
        assert_eq!(d.elapsed_ms_total, 155);
        let d2 = ComputerActDensity::new(100, 5, false, None, false, 4);
        assert_eq!(d2.elapsed_ms_total, 105); // 100 + 5 + 0
    }

    #[test]
    fn verification_passed_false_for_none_policy() {
        assert!(!compute_verification_passed(VerifyPolicy::None, None));
        assert!(!compute_verification_passed(
            VerifyPolicy::None,
            Some(&AxDiffSummary::empty(0, 0))
        ));
    }

    #[test]
    fn verification_passed_false_for_empty_diff() {
        // best_effort 但 diff 全 0 → false (GUI 没变, verify 不算通过)
        let summary = AxDiffSummary::empty(100, 50);
        assert!(!compute_verification_passed(VerifyPolicy::BestEffort, Some(&summary)));
        assert!(!compute_verification_passed(VerifyPolicy::Always, Some(&summary)));
    }

    #[test]
    fn verification_passed_true_when_diff_has_any_change() {
        let mut summary = AxDiffSummary::empty(100, 50);
        summary.elements_added = 1; // 至少 1 个 element added
        assert!(compute_verification_passed(VerifyPolicy::BestEffort, Some(&summary)));
        assert!(compute_verification_passed(VerifyPolicy::Always, Some(&summary)));
    }
}
