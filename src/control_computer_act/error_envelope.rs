//! `@computer-act` error envelope E2 (ADR-0004 §Considered Options E2, ticket 15)。
//!
//! 错误响应统一形状:
//! ```json
//! {
//!   "ok": false,
//!   "error_code": "permission_denied",
//!   "error_message": "...",
//!   "retry": {
//!     "strategy": "manual_only",   // 5 档之一
//!     "hint": "请在系统设置授予 accessibility 权限"
//!   },
//!   "evidence": {
//!     "missing_capability": "accessibility"   // per error_code 不同
//!   }
//! }
//! ```
//!
//! Strategy 枚举 (ADR-0004 E2):
//! - `never`: 不要重试 (e.g., permission_denied)
//! - `re_observe_then_retry`: 重新 observe 后重试 (e.g., observation_expired)
//! - `change_locator`: 改定位策略 (e.g., target_not_found, match_count:0)
//! - `reconnect_then_retry`: 重连后重试 (e.g., infrastructure)
//! - `manual_only`: 必须人工介入 (e.g., verify_failed)
//! - `wait_and_retry`: 等待后重试 (e.g., timeout)
//!
//! 单一真相源: 所有 caller 通过 `error_envelope(error_code, msg, evidence)` 构造响应,
//! 不允许自己手写 error_code / retry 字段。

use serde_json::{json, Value};

/// 5+1 档重试策略 (跟 ADR-0004 E2 对齐)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryStrategy {
    Never,
    ReObserveThenRetry,
    ChangeLocator,
    ReconnectThenRetry,
    ManualOnly,
    WaitAndRetry,
}

impl RetryStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::ReObserveThenRetry => "re_observe_then_retry",
            Self::ChangeLocator => "change_locator",
            Self::ReconnectThenRetry => "reconnect_then_retry",
            Self::ManualOnly => "manual_only",
            Self::WaitAndRetry => "wait_and_retry",
        }
    }
}

/// 9 个标准 error_code (ADR-0004 + ADR-0005 + ticket 15 acceptance)。
///
/// 每档对应一个默认 retry strategy + 默认 evidence key (caller 可以覆盖 evidence)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComputerActErrorCode {
    PermissionDenied,
    ObservationExpired,
    TargetNotFound,
    VerifyFailed,
    InvalidArgs,
    PlatformUnsupported,
    UnknownAction,
    Infrastructure,
    Cancelled,
    Timeout,
    InvalidVerify,
}

impl ComputerActErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::ObservationExpired => "observation_expired",
            Self::TargetNotFound => "target_not_found",
            Self::VerifyFailed => "verify_failed",
            Self::InvalidArgs => "invalid_args",
            Self::PlatformUnsupported => "platform_unsupported",
            Self::UnknownAction => "unknown_action",
            Self::Infrastructure => "infrastructure",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::InvalidVerify => "invalid_verify",
        }
    }

    /// 默认 retry strategy (跟 ADR-0004 E2 对齐)。
    pub fn default_retry_strategy(self) -> RetryStrategy {
        match self {
            Self::PermissionDenied => RetryStrategy::Never,
            Self::ObservationExpired => RetryStrategy::ReObserveThenRetry,
            Self::TargetNotFound => RetryStrategy::ChangeLocator,
            Self::VerifyFailed => RetryStrategy::ManualOnly,
            Self::InvalidArgs => RetryStrategy::Never,
            Self::PlatformUnsupported => RetryStrategy::ManualOnly,
            Self::UnknownAction => RetryStrategy::Never,
            Self::Infrastructure => RetryStrategy::ReconnectThenRetry,
            Self::Cancelled => RetryStrategy::Never,
            Self::Timeout => RetryStrategy::WaitAndRetry,
            Self::InvalidVerify => RetryStrategy::Never,
        }
    }

    /// 默认 retry hint (按 error_code 给出可读建议)。
    pub fn default_hint(self) -> &'static str {
        match self {
            Self::PermissionDenied => "请在系统设置授予缺失的能力 (accessibility / screen_recording / window_server)",
            Self::ObservationExpired => "重新调 @observe 拿新 observation_id, 然后重试同一动作",
            Self::TargetNotFound => "改用更宽的 selector 或换坐标定位 (e.g., 从 start_box 改 target.ref)",
            Self::VerifyFailed => "动作执行成功但 GUI 未变化, 检查 selector / 改用 verify=always 看截图",
            Self::InvalidArgs => "检查 args 字段 (类型 / 必填 / 数值范围), 跟 schema 对齐",
            Self::PlatformUnsupported => "当前 OS 不支持该动作, 升级到支持平台或换替代动作",
            Self::UnknownAction => "检查 action 字段是否是 13 闭集之一 (open_app / click / type 等)",
            Self::Infrastructure => "daemon 可能短暂不可用, 重连 (kill+restart) 后重试",
            Self::Cancelled => "请求已被 @cancel#seq 取消, 不需要重试",
            Self::Timeout => "等一小段时间后重试, 或调高 timeout_ms",
            Self::InvalidVerify => "verify 字段必须是 none / best_effort / always 之一",
        }
    }

    /// 默认 evidence key (caller 可以扩展)。
    pub fn default_evidence_key(self) -> Option<&'static str> {
        match self {
            Self::PermissionDenied => Some("missing_capability"),
            Self::VerifyFailed => Some("verification"),
            Self::Timeout => Some("last_step"),
            _ => None,
        }
    }
}

/// 构造 E2 错误 envelope。
///
/// `evidence` 是 caller 自填的对象,会自动 merge 默认 evidence key 对应的字段。
/// 例如 `error_envelope(PermissionDenied, "...", Some({"missing_capability": "accessibility"}))`
/// 会返回 `{error_code:"permission_denied", retry:{strategy:"never", hint:"..."}, evidence:{missing_capability:"accessibility"}}`。
pub(crate) fn error_envelope(
    code: ComputerActErrorCode,
    message: impl Into<String>,
    evidence: Option<Value>,
) -> Value {
    let mut env = json!({
        "ok": false,
        "error_code": code.as_str(),
        "error_message": message.into(),
        "retry": {
            "strategy": code.default_retry_strategy().as_str(),
            "hint": code.default_hint(),
        },
    });

    let mut evidence_obj = match evidence {
        Some(Value::Object(m)) => m,
        Some(other) => {
            // 非 object evidence → 包成 {"value": other}
            let mut m = serde_json::Map::new();
            m.insert("value".into(), other);
            m
        }
        None => serde_json::Map::new(),
    };

    // 默认 evidence key: 如果 caller 没填,补一个 null 占位 (客户端可识别结构)
    if let Some(key) = code.default_evidence_key() {
        if !evidence_obj.contains_key(key) {
            evidence_obj.insert(key.into(), Value::Null);
        }
    }

    if !evidence_obj.is_empty() {
        env["evidence"] = Value::Object(evidence_obj);
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_have_correct_strings() {
        assert_eq!(ComputerActErrorCode::PermissionDenied.as_str(), "permission_denied");
        assert_eq!(ComputerActErrorCode::ObservationExpired.as_str(), "observation_expired");
        assert_eq!(ComputerActErrorCode::VerifyFailed.as_str(), "verify_failed");
        assert_eq!(ComputerActErrorCode::Timeout.as_str(), "timeout");
        assert_eq!(ComputerActErrorCode::InvalidVerify.as_str(), "invalid_verify");
    }

    #[test]
    fn retry_strategies_match_adr_0004_e2() {
        // 5 档策略 (ADR-0004 E2): never / re_observe_then_retry / change_locator /
        // reconnect_then_retry / manual_only + ticket 16 加的 wait_and_retry
        assert_eq!(ComputerActErrorCode::PermissionDenied.default_retry_strategy().as_str(), "never");
        assert_eq!(ComputerActErrorCode::ObservationExpired.default_retry_strategy().as_str(), "re_observe_then_retry");
        assert_eq!(ComputerActErrorCode::TargetNotFound.default_retry_strategy().as_str(), "change_locator");
        assert_eq!(ComputerActErrorCode::VerifyFailed.default_retry_strategy().as_str(), "manual_only");
        assert_eq!(ComputerActErrorCode::InvalidArgs.default_retry_strategy().as_str(), "never");
        assert_eq!(ComputerActErrorCode::PlatformUnsupported.default_retry_strategy().as_str(), "manual_only");
        assert_eq!(ComputerActErrorCode::UnknownAction.default_retry_strategy().as_str(), "never");
        assert_eq!(ComputerActErrorCode::Infrastructure.default_retry_strategy().as_str(), "reconnect_then_retry");
        assert_eq!(ComputerActErrorCode::Cancelled.default_retry_strategy().as_str(), "never");
        assert_eq!(ComputerActErrorCode::Timeout.default_retry_strategy().as_str(), "wait_and_retry");
        assert_eq!(ComputerActErrorCode::InvalidVerify.default_retry_strategy().as_str(), "never");
    }

    #[test]
    fn default_evidence_key_present_for_special_codes() {
        assert_eq!(ComputerActErrorCode::PermissionDenied.default_evidence_key(), Some("missing_capability"));
        assert_eq!(ComputerActErrorCode::VerifyFailed.default_evidence_key(), Some("verification"));
        assert_eq!(ComputerActErrorCode::Timeout.default_evidence_key(), Some("last_step"));
        assert_eq!(ComputerActErrorCode::UnknownAction.default_evidence_key(), None);
    }

    #[test]
    fn error_envelope_shape() {
        let env = error_envelope(ComputerActErrorCode::PermissionDenied, "ax permission denied", None);
        assert_eq!(env["ok"], false);
        assert_eq!(env["error_code"], "permission_denied");
        assert_eq!(env["error_message"], "ax permission denied");
        assert_eq!(env["retry"]["strategy"], "never");
        assert!(env["retry"]["hint"].is_string());
        // permission_denied → evidence.missing_capability = null 占位 (没填具体能力)
        assert_eq!(env["evidence"]["missing_capability"], Value::Null);
    }

    #[test]
    fn error_envelope_with_explicit_evidence() {
        let mut ev = serde_json::Map::new();
        ev.insert("missing_capability".into(), json!("accessibility"));
        let env = error_envelope(
            ComputerActErrorCode::PermissionDenied,
            "ax permission denied",
            Some(Value::Object(ev)),
        );
        assert_eq!(env["evidence"]["missing_capability"], "accessibility");
    }

    #[test]
    fn error_envelope_without_evidence_omits_field_when_no_default_key() {
        // unknown_action 没有默认 evidence key, caller 也没传 → evidence 整个字段 omit
        let env = error_envelope(ComputerActErrorCode::UnknownAction, "no such action", None);
        assert!(env.get("evidence").is_none(),
            "evidence field omitted when no default key + caller didn't provide");
    }

    #[test]
    fn error_envelope_non_object_evidence_wrapped_as_value() {
        // 防御: caller 传 string / array / null 当 evidence → 包成 {"value": ...}
        let env = error_envelope(
            ComputerActErrorCode::InvalidArgs,
            "bad arg",
            Some(json!("just a string")),
        );
        assert_eq!(env["evidence"]["value"], "just a string");
    }

    #[test]
    fn verify_failed_carries_ax_diff_evidence() {
        let mut ev = serde_json::Map::new();
        let ax_diff = json!({"windows_added": 0, "elements_added": 0, "changed": 0});
        ev.insert("verification".into(), json!({"ax_diff": ax_diff}));
        let env = error_envelope(
            ComputerActErrorCode::VerifyFailed,
            "GUI did not change",
            Some(Value::Object(ev)),
        );
        assert_eq!(env["error_code"], "verify_failed");
        assert_eq!(env["retry"]["strategy"], "manual_only");
        assert_eq!(env["evidence"]["verification"]["ax_diff"]["changed"], 0);
    }
}
