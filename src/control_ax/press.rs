//! control_ax 模块的状态变更 verb。
//!
//! 收纳 ax-press / ax-press-sequence / ax-action / ax-set-value / ax-focus /
//! ax-scroll 这一族 state-mutating 操作。
//!
//! type-text / key delivery 已在 input.rs。
//! capture / resolve / platform-info 在 tree.rs。
//!
//! postcondition 验证逻辑内嵌在 perform_ax_press_with_postcondition_with 中,
//! 后续 commit 可考虑拆出 postcondition.rs。

use std::io;

use crate::control_window::resolve_unique_app_window_id;
use serde_json::json;

use super::types::*;
use super::tree::{
    ax_snapshot_status_error, capture_current_ax_window_snapshot, materialize_app_window_target_with,
};
use super::{
    invalid_data, invalid_input, platform_focus, platform_perform_action, platform_scroll,
    platform_set_value, resolve_current_ax_target_rect, resolve_target_id_in_snapshot,
    AxBackend,
};

// ---- perform_default_ax_press (was lines 753-759) ----
pub fn perform_default_ax_press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    let report = SystemAxBackend.perform_action(&AxActionRequest {
        target: request.target.clone(),
        action: AxActionName::Press,
    })?;
    Ok(AxActionReport::press(report.backend, report.target_id))
}

// ---- perform_default_ax_press_with_postcondition (was lines 761-772) ----
pub fn perform_default_ax_press_with_postcondition(
    request: &AxPressRequest,
) -> io::Result<AxPressPostconditionReport> {
    perform_ax_press_with_postcondition_with(
        request,
        resolve_unique_app_window_id,
        perform_default_ax_press,
        |window_id, role| {
            observe_current_ax_values_with(window_id, role, capture_current_ax_window_snapshot)
        },
    )
}

// ---- perform_ax_press_with_postcondition_with (was lines 774-868) ----
pub(crate) fn perform_ax_press_with_postcondition_with(
    request: &AxPressRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
    mut perform: impl FnMut(&AxPressRequest) -> io::Result<AxActionReport>,
    mut observe: impl FnMut(&str, &str) -> io::Result<Vec<String>>,
) -> io::Result<AxPressPostconditionReport> {
    let postcondition = request
        .postcondition
        .as_ref()
        .ok_or_else(|| invalid_data("AX guarded press 缺少 postcondition"))?;
    let target = materialize_app_window_target_with(&request.target, resolve_app)?;
    let window_id = target
        .window_id
        .as_deref()
        .ok_or_else(|| invalid_data("AX guarded press 必须使用 app:APP 或 pid:PID/window:INDEX"))?;
    let expected_value = normalize_ax_verification_value(&postcondition.expected_value);
    let mut steps = Vec::with_capacity(postcondition.max_attempts);

    for index in 0..postcondition.max_attempts {
        let action = match perform(&AxPressRequest {
            target: target.clone(),
            postcondition: None,
        }) {
            Ok(action) => action,
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressPostconditionStepReport {
                    index,
                    performed: false,
                    verified: false,
                    target_id: None,
                    observed_values: Vec::new(),
                    error: Some(error.clone()),
                });
                return Ok(build_ax_press_postcondition_report(
                    postcondition,
                    steps,
                    false,
                    Some(error),
                ));
            }
        };

        let observed_values = match observe(window_id, &postcondition.role) {
            Ok(values) => values,
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressPostconditionStepReport {
                    index,
                    performed: action.performed,
                    verified: false,
                    target_id: action.target_id,
                    observed_values: Vec::new(),
                    error: Some(error.clone()),
                });
                return Ok(build_ax_press_postcondition_report(
                    postcondition,
                    steps,
                    false,
                    Some(error),
                ));
            }
        };
        let verified = observed_values
            .iter()
            .any(|value| normalize_ax_verification_value(value) == expected_value);
        steps.push(AxPressPostconditionStepReport {
            index,
            performed: action.performed,
            verified,
            target_id: action.target_id,
            observed_values,
            error: None,
        });
        if verified {
            return Ok(build_ax_press_postcondition_report(
                postcondition,
                steps,
                true,
                None,
            ));
        }
    }

    let error = format!(
        "AX postcondition 未在{}次动作内满足: role={}, expected_value={}",
        postcondition.max_attempts, postcondition.role, postcondition.expected_value
    );
    Ok(build_ax_press_postcondition_report(
        postcondition,
        steps,
        false,
        Some(error),
    ))
}

// ---- build_ax_press_postcondition_report (was lines 870-889) ----
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

// ---- observe_current_ax_values_with (was lines 891-922) ----
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

// ---- collect_ax_values_by_role (was lines 924-933) ----
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

// ---- normalize_ax_verification_value (was lines 935-950) ----
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

// ---- perform_default_ax_press_sequence (was lines 952-978) ----
pub fn perform_default_ax_press_sequence(
    request: &AxPressSequenceRequest,
) -> AxPressSequenceReport {
    let request = match materialize_press_sequence_request(request) {
        Ok(request) => request,
        Err(error) => {
            let steps = request.targets.first().map_or_else(Vec::new, |target| {
                vec![AxPressSequenceStepReport::failed(
                    0,
                    target.description.clone().unwrap_or_default(),
                    error.to_string(),
                )]
            });
            return AxPressSequenceReport {
                kind: "ax-press-sequence",
                action: "press-sequence",
                performed: false,
                status: "failed",
                step_count: request.targets.len(),
                steps,
                failed_index: Some(0),
                error: Some(error.to_string()),
            };
        }
    };
    perform_ax_press_sequence_with(&request, perform_default_ax_press)
}

// ---- materialize_press_sequence_request (was lines 980-986) ----
/// sequence 只允许一套窗口归属.若使用 app selector,在第一个 side effect 前
/// 解析一次并把所有步骤固化为同一个 window_id,避免执行中途漂移到另一窗口.
pub(crate) fn materialize_press_sequence_request(
    request: &AxPressSequenceRequest,
) -> io::Result<AxPressSequenceRequest> {
    materialize_press_sequence_request_with(request, resolve_unique_app_window_id)
}

// ---- materialize_press_sequence_request_with (was lines 988-1018) ----
pub(crate) fn materialize_press_sequence_request_with(
    request: &AxPressSequenceRequest,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
) -> io::Result<AxPressSequenceRequest> {
    let first = request
        .targets
        .first()
        .ok_or_else(|| invalid_data("@ax-press-sequence 至少需要一个 target"))?;
    let Some(app) = first.app.as_deref() else {
        for target in &request.targets {
            target.validate()?;
        }
        return Ok(request.clone());
    };

    let window_id = resolve_app(app)?;
    let mut targets = Vec::with_capacity(request.targets.len());
    for target in &request.targets {
        if target.app.as_deref() != Some(app) {
            return Err(invalid_data(
                "@ax-press-sequence 的所有 target 必须使用同一个 app selector",
            ));
        }
        let mut target = target.clone();
        target.app = None;
        target.window_id = Some(window_id.clone());
        target.validate()?;
        targets.push(target);
    }
    Ok(AxPressSequenceRequest { targets })
}

// ---- perform_ax_press_sequence_with (was lines 1020-1069) ----
pub(crate) fn perform_ax_press_sequence_with(
    request: &AxPressSequenceRequest,
    mut perform: impl FnMut(&AxPressRequest) -> io::Result<AxActionReport>,
) -> AxPressSequenceReport {
    let mut steps = Vec::with_capacity(request.targets.len());
    for (index, target) in request.targets.iter().enumerate() {
        let description = target.description.clone().unwrap_or_default();
        match perform(&AxPressRequest {
            target: target.clone(),
            postcondition: None,
        }) {
            Ok(report) => {
                steps.push(AxPressSequenceStepReport::success(
                    index,
                    description,
                    report,
                ));
            }
            Err(error) => {
                let error = error.to_string();
                steps.push(AxPressSequenceStepReport::failed(
                    index,
                    description,
                    error.clone(),
                ));
                return AxPressSequenceReport {
                    kind: "ax-press-sequence",
                    action: "press-sequence",
                    performed: false,
                    status: "failed",
                    step_count: request.targets.len(),
                    steps,
                    failed_index: Some(index),
                    error: Some(error),
                };
            }
        }
    }

    AxPressSequenceReport {
        kind: "ax-press-sequence",
        action: "press-sequence",
        performed: true,
        status: "ok",
        step_count: request.targets.len(),
        steps,
        failed_index: None,
        error: None,
    }
}

// ---- perform_default_ax_action (was lines 1071-1073) ----
pub fn perform_default_ax_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    SystemAxBackend.perform_action(request)
}

// ---- perform_default_ax_set_value (was lines 1075-1077) ----
pub fn perform_default_ax_set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    SystemAxBackend.set_value(request)
}

// ---- perform_default_ax_focus (was lines 1080-1082) ----
pub fn perform_default_ax_focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    SystemAxBackend.focus(request)
}

// ---- perform_default_ax_scroll (was lines 1084-1086) ----
pub fn perform_default_ax_scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    SystemAxBackend.scroll(request)
}
