use super::{request::ObserveRequest, OBSERVE_SCHEMA};
use crate::{
    control_ax::{
        capture_default_ax_snapshot, current_ax_platform, AxElement, AxSnapshot, AxTreeRequest,
        AxWindow,
    },
    control_display_scope::{
        display_intersects_rect, resolve_display_scope, resolve_observation_window_ref,
        DisplayRect, DisplayScopeResolution, DisplaySelector,
    },
    control_frames::ControlFrame,
    control_observation::{record_observation_with_selectors, ObservationHeader, ObservationRoot},
    control_protocol::ScreenshotRequest,
    control_window::{
        execute_default_window_find, resolve_default_window_target_rect, WindowCommandTarget,
        WindowFindRequest,
    },
    screenshot::{current_display_summaries, execute_screenshot_bundle_request},
};
use serde_json::{json, Value};
use std::io;

pub(super) struct ProducedSections {
    pub(super) savefile_frames: Vec<ControlFrame>,
    pub(super) visual: Option<Value>,
    pub(super) accessibility: Option<AxSnapshot>,
    pub(super) windows: Option<Value>,
    pub(super) window_observation: Option<ObservationHeader>,
    pub(super) primary_observation: Option<ObservationHeader>,
    pub(super) display_scope_resolution: Option<DisplayScopeResolution>,
}

pub(super) fn produce_observe_sections(
    request_id: Option<u64>,
    request: &ObserveRequest,
) -> io::Result<ProducedSections> {
    let mut savefile_frames = Vec::new();
    let mut visual = None;
    let mut accessibility = None;
    let mut windows = None;
    let mut window_observation = None;
    let display_scope_resolution = request
        .display_scope
        .as_ref()
        .map(resolve_observe_display_scope)
        .transpose()?;

    if request.include_windows {
        let (section, observation) =
            collect_window_section(request, display_scope_resolution.as_ref())?;
        windows = Some(section);
        window_observation = observation;
    }

    if request.include_ax {
        accessibility = Some(capture_observe_ax_snapshot(request)?);
    }

    if request.include_screenshot {
        let screenshot_request = ScreenshotRequest {
            include_ax: false,
            ax_required: false,
            ..ScreenshotRequest::default()
        };
        let (mut frames, summary) =
            execute_screenshot_bundle_request(request_id, &screenshot_request)?;
        if !request.include_manifest {
            frames.retain(|frame| {
                !matches!(frame, ControlFrame::SaveFile(savefile) if savefile.mime == "application/json")
            });
        }
        savefile_frames.extend(frames);
        let visual_scope_applied = display_scope_resolution.is_some() && false;
        visual = Some(json!({
            "status": "complete",
            "target_applied": false,
            "kind": summary.kind,
            "layout": summary.layout,
            "coordinate_space": summary.coordinate_space,
            "image": summary.image,
            "manifest": request.include_manifest.then_some(summary.manifest),
            "manifest_included": request.include_manifest,
            "display_count": summary.display_count,
            "scope_applied": visual_scope_applied,
            "scope_reason": display_scope_resolution
                .is_some()
                .then_some("metadata_only"),
            "resolved_display_id": display_scope_resolution
                .as_ref()
                .map(|resolution| resolution.resolved.display_id.as_str()),
        }));
    }

    let primary_observation =
        select_primary_observation(request, accessibility.as_ref(), window_observation.as_ref())?;

    Ok(ProducedSections {
        savefile_frames,
        visual,
        accessibility,
        windows,
        window_observation,
        primary_observation,
        display_scope_resolution,
    })
}

fn collect_window_section(
    request: &ObserveRequest,
    display_scope_resolution: Option<&DisplayScopeResolution>,
) -> io::Result<(Value, Option<ObservationHeader>)> {
    let Some(target) = request.target.as_ref() else {
        return Ok((target_required_section(), None));
    };
    if target.is_empty() {
        return Ok((target_required_section(), None));
    }

    let response = execute_default_window_find(&WindowFindRequest {
        query: target.to_window_query(),
        display_scope: request.display_scope.clone(),
        limit: request.limit,
        include_state: true,
        include_recipes: false,
    })?;
    let observation = response.observation.clone();
    let value = json!({
        "status": response.status,
        "target_applied": true,
        "observation": response.observation,
        "snapshot_id": response.snapshot_id,
        "observed_at_unix_ms": response.observed_at_unix_ms,
        "match_count": response.match_count,
        "returned_count": response.returned_count,
        "display_scope": display_scope_resolution.map(crate::control_display_scope::display_scope_report),
        "items": response.matches,
    });
    Ok((value, observation))
}

fn target_required_section() -> Value {
    json!({
        "status": "skipped",
        "reason": "target_required",
        "target_applied": false,
    })
}

fn capture_observe_ax_snapshot(request: &ObserveRequest) -> io::Result<AxSnapshot> {
    let ax_request = AxTreeRequest {
        depth: request.ax_depth,
        max_elements: request.ax_max_elements,
        include_values: request.ax_include_values,
        ..AxTreeRequest::default()
    };
    let source_command = format!("@observe {}", request.mode.as_str());
    match capture_default_ax_snapshot(&ax_request) {
        Ok(snapshot) => filter_ax_snapshot(snapshot, request).with_observation(&source_command),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && !request.ax_required => {
            AxSnapshot::permission_denied(current_ax_platform()).with_observation(&source_command)
        }
        Err(err) if err.kind() == io::ErrorKind::Unsupported && !request.ax_required => {
            AxSnapshot::unsupported().with_observation(&source_command)
        }
        Err(err) => Err(err),
    }
}

fn filter_ax_snapshot(mut snapshot: AxSnapshot, request: &ObserveRequest) -> AxSnapshot {
    let Some(target) = request.target.as_ref() else {
        return snapshot;
    };
    if target.is_empty() || target.bundle_id.is_some() {
        return snapshot;
    }
    snapshot
        .windows
        .retain(|window| target.matches_ax_window(window));
    if let Some(display_scope) = request.display_scope.as_ref() {
        if let Ok(resolution) = resolve_observe_display_scope(display_scope) {
            snapshot.windows.retain(|window| {
                window
                    .rect
                    .map(DisplayRect::from)
                    .map(|rect| display_intersects_rect(&resolution.resolved, rect))
                    .unwrap_or(false)
            });
        }
    }
    snapshot.window_count = snapshot.windows.len();
    snapshot.element_count = snapshot.windows.iter().map(ax_window_element_count).sum();
    snapshot
}

fn resolve_observe_display_scope(
    scope: &crate::control_display_scope::DisplayScope,
) -> io::Result<DisplayScopeResolution> {
    let displays = current_display_summaries()?;
    resolve_display_scope(scope, &displays, |selector| {
        resolve_window_selector_rect(selector)
    })
}

fn resolve_window_selector_rect(selector: &DisplaySelector) -> io::Result<Option<DisplayRect>> {
    let window_id = match selector {
        DisplaySelector::WindowId(window_id) => window_id.clone(),
        DisplaySelector::WindowRef {
            observation_id,
            ref_id,
        } => resolve_observation_window_ref(observation_id, ref_id)?.window_id,
        _ => return Ok(None),
    };
    let resolved = resolve_default_window_target_rect(&WindowCommandTarget {
        window_id: Some(window_id),
        ..WindowCommandTarget::default()
    })?;
    Ok(resolved.rect.map(DisplayRect::from))
}

fn record_visual_observation(request: &ObserveRequest) -> io::Result<ObservationHeader> {
    record_observation_with_selectors(
        "observe.visual",
        &format!("@observe {}", request.mode.as_str()),
        ObservationRoot {
            schema: OBSERVE_SCHEMA.to_owned(),
            platform: std::env::consts::OS.to_owned(),
            coordinate_space: "os-logical".to_owned(),
        },
        Vec::new(),
        Vec::new(),
    )
}

pub(super) fn select_primary_observation(
    request: &ObserveRequest,
    accessibility: Option<&AxSnapshot>,
    window_observation: Option<&ObservationHeader>,
) -> io::Result<Option<ObservationHeader>> {
    if let Some(observation) = accessibility.and_then(|snapshot| snapshot.observation.clone()) {
        return Ok(Some(observation));
    }
    if let Some(observation) = window_observation {
        return Ok(Some(observation.clone()));
    }
    if request.include_screenshot {
        return record_visual_observation(request).map(Some);
    }
    Ok(None)
}

fn ax_window_element_count(window: &AxWindow) -> usize {
    window.elements.iter().map(ax_element_tree_count).sum()
}

fn ax_element_tree_count(element: &AxElement) -> usize {
    1 + element
        .children
        .iter()
        .map(ax_element_tree_count)
        .sum::<usize>()
}
