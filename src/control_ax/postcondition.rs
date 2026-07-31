//! control_ax 模块的 postcondition 验证 helper。
//!
//! 提供 fresh-capture + role-value 提取 + Unicode 归一化 + step 报告 builder
//! 这一族 postcondition 验证所需的纯函数原语。
//!
//! 编排 retry loop 的代码 (例如 `perform_ax_press_with_postcondition_with`)
//! 留在 press.rs / 其他 verb 模块,通过这些原语组合出 verb-specific 流程。
//!
//! 常量 `AX_POSTCONDITION_DEPTH` / `AX_POSTCONDITION_MAX_ELEMENTS` 仍在
//! `super::types`,因为它们被 tree.rs / 单元测试多处共享。

use std::io;

use super::tree::ax_snapshot_status_error;
use super::types::*;
use super::{invalid_input, AX_POSTCONDITION_DEPTH, AX_POSTCONDITION_MAX_ELEMENTS};

// ---- build_ax_press_postcondition_report (was lines 148-167) ----
pub(crate) fn build_ax_press_postcondition_report(
    postcondition: &AxPressPostcondition,
    steps: Vec<AxPressPostconditionStepReport>,
    verified: bool,
    error: Option<String>,
) -> AxPressPostconditionReport {
    AxPressPostconditionReport {
        kind: "ax-press",
        action: "press-until",
        performed: steps.iter().any(|step| step.performed),
        verified,
        status: if verified { "ok" } else { "failed" },
        role: postcondition.role.clone(),
        expected_value: postcondition.expected_value.clone(),
        attempt_count: steps.len(),
        max_attempts: postcondition.max_attempts,
        steps,
        error,
    }
}

// ---- observe_current_ax_values_with (was lines 170-201) ----
pub(crate) fn observe_current_ax_values_with(
    window_id: &str,
    role: &str,
    capture: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<Vec<String>> {
    // 通用 fresh 观察必须能深入 Calculator 等多层 AX 树拿到 AXStaticText 的 value。
    // 默认 AxTreeRequest 只到 depth=4,实测不足以覆盖 Calculator 结果节点。
    // 这里使用与 compact ax-find 一致的深度与上限,与 parser 侧契约对齐。
    let request = AxTreeRequest {
        depth: AX_POSTCONDITION_DEPTH,
        max_elements: AX_POSTCONDITION_MAX_ELEMENTS,
        include_values: true,
        ..AxTreeRequest::default()
    };
    let snapshot = capture(window_id, &request)?;
    if snapshot.capture_status != "complete" {
        return Err(ax_snapshot_status_error(&snapshot));
    }
    if snapshot.truncated {
        return Err(invalid_input(
            "AX guarded press fresh snapshot 被截断,无法证明postcondition",
        ));
    }

    let mut values = Vec::new();
    for window in &snapshot.windows {
        collect_ax_values_by_role(&window.elements, role, &mut values);
    }
    values.sort();
    values.dedup();
    Ok(values)
}

// ---- collect_ax_values_by_role (was lines 204-213) ----
pub(crate) fn collect_ax_values_by_role(elements: &[AxElement], role: &str, values: &mut Vec<String>) {
    for element in elements {
        if element.role == role && !element.value_redacted {
            if let Some(value) = element.value.as_deref() {
                values.push(normalize_ax_verification_value(value));
            }
        }
        collect_ax_values_by_role(&element.children, role, values);
    }
}

// ---- normalize_ax_verification_value (was lines 216-231) ----
pub(crate) fn normalize_ax_verification_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
        })
        .collect::<String>()
        .trim()
        .to_owned()
}
