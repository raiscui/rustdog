//! `outcome` 三态 (rdog 计算机控制 feature/computer-act-outcome-3state)。
//!
//! 源自 pi-computer-use `ActOutcome = "worked" | "didnt" | "unknown"`,
//! 解决的是 Phase F-2 把 verify_failed 改成 `ok:false + error_code:verify_failed`
//! 之后丢失的两个关键区分:
//!
//! 1. **dispatch succeeded vs action took effect**: 旧 ok 把两者塞进同一字段
//!    (verified 失败也返 false), 客户端分不清 "动作没派发" vs "动作派发了但 GUI 没变"。
//! 2. **客户无法独立 retry verify failed**: 旧 ok:false 让 client 把它当成
//!    错误自动 retry, 但 verify_failed retry 只是在同一 view 上重新看,
//!    不会让动作生效, 真正的修法是修 selector / 改 verify=always 看 screenshot。
//!
//! 修正语义:
//! - `ok: true` 永远表示 dispatch 成功 (底层 primitive 返回了成功响应)
//! - `outcome` 字段表示语义结果 (postcondition 三态)
//! - `outcome: "didnt"` 替代 `ok: false + error_code: verify_failed`
//! - `error_code` 顶层只在 dispatch 失败时存在 (permission / infra / timeout / etc.)
//!
//! mapping 规则: 见 `compute_outcome` doc.
//!
//! 单一真相源: 所有 caller 通过 `compute_outcome` + `render_outcome` 取字符串,
//! 不在 dispatcher / response builder 里手写 `outcome` 字段.

use serde::{Deserialize, Serialize};

/// 三态 outcome (pi-computer-use `ActOutcome` 同构)。
///
/// 注意: `Didnt` 仍然表示"动作派发了, 但 postcondition 没满足", 不是错误.
/// 客户端应该:
/// - `worked` → 继续
/// - `didnt` → 修 selector / 改 verify / 看 screenshot, 不自动 retry
/// - `unknown` → 自己 re-observe 确认, 或调 verify=always
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComputerActOutcome {
    /// dispatch 成功 + (verify=None OR verify_passed). 客户端认为动作生效.
    Worked,
    /// dispatch 成功 + verify_failed. 客户端必须看 verification 段找原因.
    Didnt,
    /// dispatch 成功 + verify 没执行 (timeout / cancel / policy 错误 /
    /// 底层 verify helper 不可用). 客户端需要 re-observe 或自检.
    Unknown,
}

impl ComputerActOutcome {
    /// wire 字符串 (snake_case). 跟 `serde(rename_all = "snake_case")` 对齐.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Worked => "worked",
            Self::Didnt => "didnt",
            Self::Unknown => "unknown",
        }
    }
}

/// 推导 outcome 的输入.
///
/// 故意 strait-laced: 只接 4 个 bool/字段, 不接 verify_policy 整个 enum.
/// 理由: dispatch outcome 是 4 维 orthogonal 信号, 跟 policy 解耦更易测.
pub(crate) struct OutcomeInputs {
    /// 底层 dispatch 是否成功 (execute_* 返回 exit_code == 0).
    pub dispatch_ok: bool,
    /// verify policy 是否请求 verify (None policy = false).
    pub verify_requested: bool,
    /// verify 实际是否跑完 (timeout / cancel = false).
    pub verify_ran: bool,
    /// verify 实际是否通过 (verify_failed = false).
    pub verify_passed: bool,
}

/// 推导 outcome.
///
/// 决策表:
/// | dispatch | verify_req | verify_run | verify_pass | outcome |
/// |----------|------------|------------|-------------|---------|
/// | false    | -          | -          | -           | Unknown (dispatch 失败, outcome 字段会被 client 忽略但保留占位) |
/// | true     | false      | -          | -           | Worked  (no verify = 默认 dispatch 成功) |
/// | true     | true       | false      | -           | Unknown (verify 没跑, 不知道结果) |
/// | true     | true       | true       | false       | Didnt   (verify 跑了, 失败) |
/// | true     | true       | true       | true        | Worked  (verify 跑了, 通过) |
pub(crate) fn compute_outcome(inputs: &OutcomeInputs) -> ComputerActOutcome {
    if !inputs.dispatch_ok {
        // dispatch 失败: outcome 字段仍然写入 (按"未跑"对待), 客户端读 ok 决定是否重试.
        return ComputerActOutcome::Unknown;
    }
    if !inputs.verify_requested {
        return ComputerActOutcome::Worked;
    }
    if !inputs.verify_ran {
        return ComputerActOutcome::Unknown;
    }
    if inputs.verify_passed {
        ComputerActOutcome::Worked
    } else {
        ComputerActOutcome::Didnt
    }
}

/// 渲染 outcome 字段 JSON 值. 总是返回 Some, 永远写到顶层.
pub(crate) fn render_outcome(outcome: ComputerActOutcome) -> serde_json::Value {
    serde_json::Value::String(outcome.as_wire_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(
        dispatch_ok: bool,
        verify_requested: bool,
        verify_ran: bool,
        verify_passed: bool,
    ) -> OutcomeInputs {
        OutcomeInputs {
            dispatch_ok,
            verify_requested,
            verify_ran,
            verify_passed,
        }
    }

    #[test]
    fn outcome_dispatch_failed_returns_unknown() {
        assert_eq!(compute_outcome(&inputs(false, false, false, false)), ComputerActOutcome::Unknown);
        assert_eq!(compute_outcome(&inputs(false, true, true, true)), ComputerActOutcome::Unknown);
    }

    #[test]
    fn outcome_no_verify_returns_worked() {
        // 默认情况: verify=None, dispatch 成功.
        assert_eq!(
            compute_outcome(&inputs(true, false, false, false)),
            ComputerActOutcome::Worked
        );
    }

    #[test]
    fn outcome_verify_didnt_run_returns_unknown() {
        // verify=best_effort 请求了, 但 verify 路径被 timeout 跳过.
        assert_eq!(
            compute_outcome(&inputs(true, true, false, false)),
            ComputerActOutcome::Unknown
        );
    }

    #[test]
    fn outcome_verify_passed_returns_worked() {
        assert_eq!(
            compute_outcome(&inputs(true, true, true, true)),
            ComputerActOutcome::Worked
        );
    }

    #[test]
    fn outcome_verify_failed_returns_didnt() {
        assert_eq!(
            compute_outcome(&inputs(true, true, true, false)),
            ComputerActOutcome::Didnt
        );
    }

    #[test]
    fn outcome_wire_strings_match_pi_computer_use() {
        assert_eq!(ComputerActOutcome::Worked.as_wire_str(), "worked");
        assert_eq!(ComputerActOutcome::Didnt.as_wire_str(), "didnt");
        assert_eq!(ComputerActOutcome::Unknown.as_wire_str(), "unknown");
    }

    #[test]
    fn outcome_render_returns_string_value() {
        let v = render_outcome(ComputerActOutcome::Didnt);
        assert_eq!(v, serde_json::Value::String("didnt".to_owned()));
    }
}
