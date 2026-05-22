use super::request::{
    ClickRequest, DragRequest, MouseAnchor, MouseCoordinateSpace, MouseEndpoint, MouseMoveRequest,
    MousePoint, MouseRefTarget, MouseSelectorTarget, WheelRequest,
};
use crate::{
    control_ax::{resolve_current_ax_target_rect, AxRect, AxTarget},
    control_observation::{
        build_selector_refind_decision, resolve_observation_ref_with_header,
        stale_observation_ref_error, ObservationHeader, ObservationRefEntry, SelectorRefindRequest,
        SelectorRefindSource, DEFAULT_REFIND_LIMIT,
    },
    control_window::{resolve_default_window_target_rect, WindowCommandTarget},
};
use serde_json::{json, Value};
use std::io;

#[derive(Debug, Clone, PartialEq)]
pub enum PreparedMouseRequest<T> {
    Ready {
        request: T,
        target_resolution: Option<Value>,
    },
    NoAction {
        response_value_json: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedEndpoint {
    Point {
        point: MousePoint,
        resolution: Value,
    },
    NoAction {
        response_value_json: String,
    },
}

pub fn prepare_mouse_move_request(
    request: &MouseMoveRequest,
) -> io::Result<PreparedMouseRequest<MouseMoveRequest>> {
    let Some(endpoint) = request.target.as_ref() else {
        let target_resolution = if request.coordinate_space == MouseCoordinateSpace::OsLogical {
            request
                .x
                .zip(request.y)
                .map(|(x, y)| coordinate_resolution(MousePoint { x, y }, "coordinate_fallback"))
        } else {
            None
        };
        return Ok(PreparedMouseRequest::Ready {
            request: request.clone(),
            target_resolution,
        });
    };

    let prepared = prepare_endpoint(endpoint, "move")?;
    let PreparedEndpoint::Point { point, resolution } = prepared else {
        return Ok(no_action_from_endpoint(prepared));
    };
    Ok(PreparedMouseRequest::Ready {
        request: MouseMoveRequest {
            x: Some(point.x),
            y: Some(point.y),
            dx: None,
            dy: None,
            target: None,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        },
        target_resolution: Some(resolution),
    })
}

pub fn prepare_click_request(
    request: &ClickRequest,
) -> io::Result<PreparedMouseRequest<ClickRequest>> {
    let prepared = match request.target.as_ref() {
        Some(endpoint) => prepare_endpoint(endpoint, "click")?,
        None => {
            let point = MousePoint {
                x: required_coordinate(request.x, "@click", "x")?,
                y: required_coordinate(request.y, "@click", "y")?,
            };
            PreparedEndpoint::Point {
                point,
                resolution: coordinate_resolution(point, "coordinate_fallback"),
            }
        }
    };

    let PreparedEndpoint::Point { point, resolution } = prepared else {
        return Ok(no_action_from_endpoint(prepared));
    };
    Ok(PreparedMouseRequest::Ready {
        request: ClickRequest {
            x: Some(point.x),
            y: Some(point.y),
            target: None,
            button: request.button,
            count: request.count,
            hold_ms: request.hold_ms,
            interval_ms: request.interval_ms,
            coordinate_space: request.coordinate_space,
        },
        target_resolution: Some(resolution),
    })
}

pub fn prepare_drag_request(
    request: &DragRequest,
) -> io::Result<PreparedMouseRequest<DragRequest>> {
    let from = prepare_endpoint(&request.from, "drag")?;
    let PreparedEndpoint::Point {
        point: from_point,
        resolution: from_resolution,
    } = from
    else {
        return Ok(no_action_from_endpoint(from));
    };

    let to = prepare_endpoint(&request.to, "drag")?;
    let PreparedEndpoint::Point {
        point: to_point,
        resolution: to_resolution,
    } = to
    else {
        return Ok(no_action_from_endpoint(to));
    };

    Ok(PreparedMouseRequest::Ready {
        request: DragRequest {
            from: MouseEndpoint::Coordinate(from_point),
            to: MouseEndpoint::Coordinate(to_point),
            button: request.button,
            duration_ms: request.duration_ms,
            steps: request.steps,
            coordinate_space: request.coordinate_space,
        },
        target_resolution: Some(json!({
            "from": from_resolution,
            "to": to_resolution,
        })),
    })
}

pub fn prepare_wheel_request(
    request: &WheelRequest,
) -> io::Result<PreparedMouseRequest<WheelRequest>> {
    let Some(endpoint) = request.target.as_ref() else {
        let target_resolution = request
            .x
            .zip(request.y)
            .map(|(x, y)| coordinate_resolution(MousePoint { x, y }, "coordinate_fallback"));
        return Ok(PreparedMouseRequest::Ready {
            request: request.clone(),
            target_resolution,
        });
    };

    let prepared = prepare_endpoint(endpoint, "wheel")?;
    let PreparedEndpoint::Point { point, resolution } = prepared else {
        return Ok(no_action_from_endpoint(prepared));
    };
    Ok(PreparedMouseRequest::Ready {
        request: WheelRequest {
            x: Some(point.x),
            y: Some(point.y),
            target: None,
            delta_x: request.delta_x,
            delta_y: request.delta_y,
            coordinate_space: request.coordinate_space,
        },
        target_resolution: Some(resolution),
    })
}

fn prepare_endpoint(
    endpoint: &MouseEndpoint,
    action: &'static str,
) -> io::Result<PreparedEndpoint> {
    match endpoint {
        MouseEndpoint::Coordinate(point) => Ok(PreparedEndpoint::Point {
            point: *point,
            resolution: coordinate_resolution(*point, "coordinate_fallback"),
        }),
        MouseEndpoint::ObservationRef(target) => resolve_observation_ref_endpoint(target, action),
        MouseEndpoint::Selector(target) => selector_endpoint_handoff(target, action),
    }
}

fn resolve_observation_ref_endpoint(
    target: &MouseRefTarget,
    action: &'static str,
) -> io::Result<PreparedEndpoint> {
    let (header, entry) =
        resolve_observation_ref_with_header(&target.observation_id, &target.ref_id)?;
    let (backend_target_id, backend_kind, rect) = resolve_current_rect(&header, &entry, target)?;
    let Some(rect) = rect else {
        return Err(target_rect_unavailable_error(action, target, &entry));
    };
    let point = point_for_anchor(rect, target.anchor);

    Ok(PreparedEndpoint::Point {
        point,
        resolution: json!({
            "source": "observation_ref",
            "observation_id": target.observation_id.as_str(),
            "ref": target.ref_id.as_str(),
            "observation_scope": header.scope,
            "backend_id": entry.backend_id.as_str(),
            "backend_target_id": backend_target_id,
            "kind": entry.kind.as_str(),
            "backend_kind": backend_kind,
            "anchor": anchor_value(target.anchor),
            "rect": rect_value(rect),
            "point": point_value(point),
            "coordinate_space": "os-logical",
        }),
    })
}

fn resolve_current_rect(
    header: &ObservationHeader,
    entry: &ObservationRefEntry,
    target: &MouseRefTarget,
) -> io::Result<(String, &'static str, Option<AxRect>)> {
    match header.scope.as_str() {
        "ax" => {
            let resolved = resolve_current_ax_target_rect(&AxTarget {
                id: Some(entry.backend_id.clone()),
                ..AxTarget::default()
            })
            .map_err(|err| map_stale_ref_error(err, target, &entry.backend_id))?;
            Ok((resolved.target_id, resolved.target_type, resolved.rect))
        }
        "window" => {
            let resolved = resolve_default_window_target_rect(&WindowCommandTarget {
                window_id: Some(entry.backend_id.clone()),
                ..WindowCommandTarget::default()
            })
            .map_err(|err| map_stale_ref_error(err, target, &entry.backend_id))?;
            Ok((resolved.window_id, "window", resolved.rect))
        }
        scope => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            json!({
                "kind": "mouse-target-resolution",
                "error_code": "UNSUPPORTED_OBSERVATION_SCOPE",
                "performed": false,
                "observation_id": target.observation_id.as_str(),
                "ref": target.ref_id.as_str(),
                "observation_scope": scope,
                "backend_id": entry.backend_id.as_str(),
                "message": "mouse target resolver 暂不支持该 observation scope",
            })
            .to_string(),
        )),
    }
}

fn selector_endpoint_handoff(
    target: &MouseSelectorTarget,
    action: &'static str,
) -> io::Result<PreparedEndpoint> {
    if target.auto_refind {
        return selector_endpoint_refind(target, action);
    }

    let recovery_command = selector_refind_command(target);
    let value = json!({
        "kind": "mouse",
        "action": action,
        "status": "no_action",
        "performed": false,
        "target_resolution": {
            "source": "selector",
            "selector_id": target.selector_id.as_str(),
            "auto_refind": target.auto_refind,
            "policy": target.policy.as_str(),
            "min_confidence": (target.min_confidence_milli as f64) / 1000.0,
            "anchor": anchor_value(target.anchor),
            "gate_decision": "handoff_required",
            "gate_reason": "auto_refind_not_enabled",
        },
        "recovery_command": recovery_command,
        "verify_hint": {
            "required_before_action": true,
            "recommended": true,
        },
    });
    Ok(PreparedEndpoint::NoAction {
        response_value_json: value.to_string(),
    })
}

fn selector_endpoint_refind(
    target: &MouseSelectorTarget,
    action: &'static str,
) -> io::Result<PreparedEndpoint> {
    let decision = build_selector_refind_decision(&SelectorRefindRequest {
        selector_id: target.selector_id.clone(),
        limit: DEFAULT_REFIND_LIMIT,
        policy: target.policy,
        min_confidence_milli: target.min_confidence_milli,
        include_explanations: true,
        include_history: false,
        source: None::<SelectorRefindSource>,
    })?;

    let Some(fresh_target) = decision.fresh_target.clone() else {
        return selector_refind_no_action(target, action, decision);
    };
    if decision.decision != "rebound" {
        return selector_refind_no_action(target, action, decision);
    }

    let prepared = match resolve_observation_ref_endpoint(
        &MouseRefTarget {
            observation_id: fresh_target.observation_id,
            ref_id: fresh_target.ref_id,
            anchor: target.anchor,
        },
        action,
    ) {
        Ok(prepared) => prepared,
        Err(err) => return selector_refind_verify_failed_no_action(target, action, decision, err),
    };
    let PreparedEndpoint::Point { point, resolution } = prepared else {
        return selector_refind_no_action(target, action, decision);
    };

    Ok(PreparedEndpoint::Point {
        point,
        resolution: json!({
            "source": "selector_refind",
            "selector_id": target.selector_id.as_str(),
            "auto_refind": true,
            "policy": target.policy.as_str(),
            "min_confidence": (target.min_confidence_milli as f64) / 1000.0,
            "gate_decision": "verified_rebound",
            "gate_reason": "selector_refind_rebound_verified",
            "selector_refind": selector_refind_audit_value(&decision),
            "fresh_target_resolution": resolution,
            "verify_hint": decision.verify_hint,
            "verify_result": {
                "status": "ok",
                "method": "resolve_fresh_target_to_current_rect",
            },
        }),
    })
}

fn selector_refind_verify_failed_no_action(
    target: &MouseSelectorTarget,
    action: &'static str,
    decision: crate::control_observation::SelectorRefindDecision,
    err: io::Error,
) -> io::Result<PreparedEndpoint> {
    let value = json!({
        "kind": "mouse",
        "action": action,
        "status": "no_action",
        "performed": false,
        "target_resolution": {
            "source": "selector_refind",
            "selector_id": target.selector_id.as_str(),
            "auto_refind": true,
            "policy": target.policy.as_str(),
            "min_confidence": (target.min_confidence_milli as f64) / 1000.0,
            "anchor": anchor_value(target.anchor),
            "gate_decision": "no_action",
            "gate_reason": "selector_refind_verify_failed",
            "selector_refind": selector_refind_audit_value(&decision),
            "verify_result": {
                "status": "failed",
                "error": err.to_string(),
            },
        },
        "recovery_command": selector_refind_command(target),
    });
    Ok(PreparedEndpoint::NoAction {
        response_value_json: value.to_string(),
    })
}

fn selector_refind_no_action(
    target: &MouseSelectorTarget,
    action: &'static str,
    decision: crate::control_observation::SelectorRefindDecision,
) -> io::Result<PreparedEndpoint> {
    let gate_reason = match decision.decision.as_str() {
        "not_found" => "selector_refind_not_found",
        "blocked" => "selector_refind_blocked",
        "needs_disambiguation" => "selector_refind_needs_disambiguation",
        other => other,
    };
    let value = json!({
        "kind": "mouse",
        "action": action,
        "status": "no_action",
        "performed": false,
        "target_resolution": {
            "source": "selector_refind",
            "selector_id": target.selector_id.as_str(),
            "auto_refind": true,
            "policy": target.policy.as_str(),
            "min_confidence": (target.min_confidence_milli as f64) / 1000.0,
            "anchor": anchor_value(target.anchor),
            "gate_decision": "no_action",
            "gate_reason": gate_reason,
            "selector_refind": selector_refind_audit_value(&decision),
            "verify_result": {
                "status": "skipped",
                "reason": gate_reason,
            },
        },
        "recovery_command": selector_refind_command(target),
    });
    Ok(PreparedEndpoint::NoAction {
        response_value_json: value.to_string(),
    })
}

fn selector_refind_audit_value(
    decision: &crate::control_observation::SelectorRefindDecision,
) -> Value {
    json!({
        "decision": decision.decision.as_str(),
        "scoring_version": decision.scoring_version.as_str(),
        "policy": decision.policy.as_str(),
        "min_confidence": (decision.min_confidence_milli as f64) / 1000.0,
        "candidate_count": decision.candidate_count,
        "fresh_target": decision.fresh_target.as_ref().map(|target| {
            json!({
                "observation_id": target.observation_id.as_str(),
                "ref": target.ref_id.as_str(),
            })
        }),
        "verify_hint": decision.verify_hint.clone(),
        "audit": decision.audit_value.clone(),
    })
}

fn no_action_from_endpoint<T>(endpoint: PreparedEndpoint) -> PreparedMouseRequest<T> {
    let PreparedEndpoint::NoAction {
        response_value_json,
    } = endpoint
    else {
        unreachable!("point endpoint cannot be converted into no-action");
    };
    PreparedMouseRequest::NoAction {
        response_value_json,
    }
}

fn map_stale_ref_error(err: io::Error, target: &MouseRefTarget, backend_id: &str) -> io::Error {
    match err.kind() {
        io::ErrorKind::InvalidInput | io::ErrorKind::NotFound => stale_observation_ref_error(
            &target.observation_id,
            &target.ref_id,
            format!("backend id 已不在当前 backend snapshot 中: {backend_id}"),
        ),
        _ => err,
    }
}

fn target_rect_unavailable_error(
    action: &'static str,
    target: &MouseRefTarget,
    entry: &ObservationRefEntry,
) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        json!({
            "kind": "mouse-target-resolution",
            "action": action,
            "error_code": "TARGET_RECT_UNAVAILABLE",
            "performed": false,
            "observation_id": target.observation_id.as_str(),
            "ref": target.ref_id.as_str(),
            "backend_id": entry.backend_id.as_str(),
            "kind": entry.kind.as_str(),
            "message": "目标当前没有可用 rect,mouse action 未执行",
        })
        .to_string(),
    )
}

fn coordinate_resolution(point: MousePoint, source: &'static str) -> Value {
    json!({
        "source": source,
        "point": point_value(point),
        "coordinate_space": "os-logical",
    })
}

fn point_for_anchor(rect: AxRect, anchor: MouseAnchor) -> MousePoint {
    let half_width = (rect.width / 2).min(i32::MAX as u32) as i32;
    let half_height = (rect.height / 2).min(i32::MAX as u32) as i32;
    let width = rect.width.min(i32::MAX as u32) as i32;
    let height = rect.height.min(i32::MAX as u32) as i32;

    match anchor {
        MouseAnchor::Center => MousePoint {
            x: rect.x.saturating_add(half_width),
            y: rect.y.saturating_add(half_height),
        },
        MouseAnchor::TopLeft => MousePoint {
            x: rect.x,
            y: rect.y,
        },
        MouseAnchor::TopRight => MousePoint {
            x: rect.x.saturating_add(width),
            y: rect.y,
        },
        MouseAnchor::BottomLeft => MousePoint {
            x: rect.x,
            y: rect.y.saturating_add(height),
        },
        MouseAnchor::BottomRight => MousePoint {
            x: rect.x.saturating_add(width),
            y: rect.y.saturating_add(height),
        },
        MouseAnchor::Offset { dx, dy } => MousePoint {
            x: rect.x.saturating_add(dx),
            y: rect.y.saturating_add(dy),
        },
    }
}

fn anchor_value(anchor: MouseAnchor) -> Value {
    match anchor {
        MouseAnchor::Center => json!({"kind": "center"}),
        MouseAnchor::TopLeft => json!({"kind": "top_left"}),
        MouseAnchor::TopRight => json!({"kind": "top_right"}),
        MouseAnchor::BottomLeft => json!({"kind": "bottom_left"}),
        MouseAnchor::BottomRight => json!({"kind": "bottom_right"}),
        MouseAnchor::Offset { dx, dy } => {
            json!({"kind": "offset", "dx": dx, "dy": dy, "offset_origin": "rect_top_left"})
        }
    }
}

fn rect_value(rect: AxRect) -> Value {
    json!({
        "x": rect.x,
        "y": rect.y,
        "width": rect.width,
        "height": rect.height,
    })
}

fn point_value(point: MousePoint) -> Value {
    json!({
        "x": point.x,
        "y": point.y,
    })
}

fn selector_refind_command(target: &MouseSelectorTarget) -> String {
    format!(
        "@selector-refind:{{selector_id:{},policy:{},include_explanations:true}}",
        quote_control_string(&target.selector_id),
        quote_control_string(target.policy.as_str())
    )
}

fn quote_control_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

fn required_coordinate(value: Option<i32>, kind: &str, field_name: &str) -> io::Result<i32> {
    value.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{kind} 缺少 `{field_name}`"),
        )
    })
}
