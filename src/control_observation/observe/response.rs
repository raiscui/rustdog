use super::{
    producer::ProducedSections,
    refs::{collect_ref_samples, selector_count},
    request::ObserveRequest,
    ObserveBundle, OBSERVE_SCHEMA,
};
use crate::{control_ax::AxSnapshot, control_observation::ObservationHeader};
use serde_json::{json, Value};
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
pub(super) struct ObserveResponse {
    pub(super) savefile_frames: Vec<crate::control_frames::ControlFrame>,
    pub(super) response_line: String,
}

#[cfg(test)]
pub(super) fn render_observe_response(
    request_id: Option<u64>,
    request: &ObserveRequest,
    produced: ProducedSections,
) -> io::Result<ObserveResponse> {
    let bundle = build_observe_bundle_from_sections(request, produced)?;
    let response_line = render_observe_bundle_response_line(request_id, &bundle.value)?;

    Ok(ObserveResponse {
        savefile_frames: bundle.savefile_frames,
        response_line,
    })
}

pub(super) fn build_observe_bundle_from_sections(
    request: &ObserveRequest,
    produced: ProducedSections,
) -> io::Result<ObserveBundle> {
    let observed_at_unix_ms = current_unix_ms();
    let accessibility_value = produced
        .accessibility
        .as_ref()
        .map(|snapshot| accessibility_section(snapshot, request));
    let refs = if request.include_refs {
        collect_ref_samples(
            produced.accessibility.as_ref(),
            produced.windows.as_ref(),
            request.limit as usize,
        )?
    } else {
        json!({"count": 0, "sample": []})
    };
    let selector_count = selector_count(
        produced.primary_observation.as_ref(),
        produced.window_observation.as_ref(),
        produced.accessibility.as_ref(),
    );
    let selectors = if request.include_selectors {
        json!({"count": selector_count, "sample": []})
    } else {
        json!({"count": 0, "sample": []})
    };
    let visual_value = produced
        .visual
        .unwrap_or_else(|| json!({"status": "not_requested"}));
    let windows_value = produced
        .windows
        .unwrap_or_else(|| json!({"status": "not_requested"}));
    let accessibility_value =
        accessibility_value.unwrap_or_else(|| json!({"status": "not_requested"}));
    let status = observe_status(&visual_value, &accessibility_value, &windows_value);

    let value = json!({
        "kind": "observe",
        "schema": OBSERVE_SCHEMA,
        "status": status,
        "mode": request.mode.as_str(),
        "observed_at_unix_ms": observed_at_unix_ms,
        "primary_observation_source": primary_observation_source(
            produced.primary_observation.as_ref(),
            produced.accessibility.as_ref(),
            produced.window_observation.as_ref(),
            request.include_screenshot,
        ),
        "observation": produced.primary_observation,
        "windows": windows_value,
        "accessibility": accessibility_value,
        "visual": visual_value,
        "refs": refs,
        "selectors": selectors,
        "recovery": {
            "selector_refind_available": true,
            "scoring_version": "rdog.selector.score.v1",
        },
    });
    Ok(ObserveBundle {
        savefile_frames: produced.savefile_frames,
        value,
    })
}

pub(super) fn render_observe_bundle_response_line(
    request_id: Option<u64>,
    value: &Value,
) -> io::Result<String> {
    let value_json = serde_json::to_string(value)
        .map_err(|err| io::Error::other(format!("observe response 序列化失败: {err}")))?;
    Ok(render_structured_response(request_id, &value_json))
}

fn accessibility_section(snapshot: &AxSnapshot, request: &ObserveRequest) -> Value {
    json!({
        "status": snapshot.capture_status,
        "target_applied": request
            .target
            .as_ref()
            .map(|target| !target.is_empty() && target.bundle_id.is_none())
            .unwrap_or(false),
        "schema": snapshot.schema,
        "platform": snapshot.platform,
        "capture_status": snapshot.capture_status,
        "permission_status": snapshot.permission_status,
        "coordinate_space": snapshot.coordinate_space,
        "observation": snapshot.observation,
        "window_count": snapshot.window_count,
        "element_count": snapshot.element_count,
        "truncated": snapshot.truncated,
        "windows": snapshot.windows,
    })
}

fn observe_status(visual: &Value, accessibility: &Value, windows: &Value) -> &'static str {
    let statuses = [visual, accessibility, windows]
        .into_iter()
        .filter_map(|section| section.get("status").and_then(Value::as_str))
        .filter(|status| *status != "not_requested")
        .collect::<Vec<_>>();
    if statuses.is_empty() {
        "unsupported"
    } else if statuses.iter().all(|status| *status == "complete") {
        "complete"
    } else if statuses.iter().all(|status| *status == "permission_denied") {
        "permission_denied"
    } else if statuses.iter().all(|status| *status == "unsupported") {
        "unsupported"
    } else {
        "partial"
    }
}

fn primary_observation_source(
    primary: Option<&ObservationHeader>,
    accessibility: Option<&AxSnapshot>,
    window: Option<&ObservationHeader>,
    include_screenshot: bool,
) -> &'static str {
    let Some(primary) = primary else {
        return "none";
    };
    if accessibility
        .and_then(|snapshot| snapshot.observation.as_ref())
        .map(|observation| observation.observation_id.as_str())
        == Some(primary.observation_id.as_str())
    {
        "accessibility"
    } else if window.map(|observation| observation.observation_id.as_str())
        == Some(primary.observation_id.as_str())
    {
        "windows"
    } else if include_screenshot {
        "visual"
    } else {
        "unknown"
    }
}

fn render_structured_response(request_id: Option<u64>, value_json: &str) -> String {
    match request_id {
        Some(request_id) => format!(r#"@response {{"id":{request_id},"value":{value_json}}}"#),
        None => format!("@response {value_json}"),
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
