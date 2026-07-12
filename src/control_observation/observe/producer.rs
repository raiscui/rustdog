use super::{request::ObserveRequest, OBSERVE_SCHEMA};
use crate::{
    control_ax::{
        capture_default_ax_snapshot, current_ax_platform, AxElement, AxSnapshot, AxTreeRequest,
        AxWindow,
    },
    control_display_scope::{
        display_intersects_rect, resolve_display_scope, resolve_observation_window_ref,
        DisplayRect, DisplayScope, DisplayScopeResolution, DisplaySelector,
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
    let display_scope_resolution = if request.include_screenshot {
        let screenshot_request = ScreenshotRequest {
            include_ax: false,
            ax_required: false,
            ..ScreenshotRequest::default()
        };
        let mut screenshot = execute_screenshot_bundle_request(
            request_id,
            &screenshot_request,
            request.display_scope.as_ref(),
            resolve_window_selector_rect,
        )?;
        let display_scope_resolution = screenshot.display_scope_resolution.take();
        if !request.include_manifest {
            screenshot.frames.retain(|frame| {
                !matches!(frame, ControlFrame::SaveFile(savefile) if savefile.mime == "application/json")
            });
        }
        savefile_frames.extend(screenshot.frames);
        let visual_scope_applied = display_scope_resolution.is_some();
        visual = Some(json!({
            "status": "complete",
            "target_applied": false,
            "kind": screenshot.summary.kind,
            "layout": screenshot.summary.layout,
            "coordinate_space": screenshot.summary.coordinate_space,
            "image": screenshot.summary.image,
            "manifest": request.include_manifest.then_some(screenshot.summary.manifest),
            "manifest_included": request.include_manifest,
            "display_count": screenshot.summary.display_count,
            "scope_applied": visual_scope_applied,
            "resolved_display_id": display_scope_resolution
                .as_ref()
                .map(|resolution| resolution.resolved.display_id.as_str()),
        }));
        display_scope_resolution
    } else {
        request
            .display_scope
            .as_ref()
            .map(resolve_observe_display_scope)
            .transpose()?
    };

    if request.include_windows {
        let (section, observation) =
            collect_window_section(request, display_scope_resolution.as_ref())?;
        windows = Some(section);
        window_observation = observation;
    }

    if request.include_ax {
        accessibility = Some(capture_observe_ax_snapshot(
            request,
            display_scope_resolution.as_ref(),
        )?);
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
        display_scope: display_scope_resolution.map(|resolution| DisplayScope {
            display: DisplaySelector::Id(resolution.resolved.display_id.clone()),
        }),
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

fn capture_observe_ax_snapshot(
    request: &ObserveRequest,
    display_scope_resolution: Option<&DisplayScopeResolution>,
) -> io::Result<AxSnapshot> {
    let ax_request = AxTreeRequest {
        depth: request.ax_depth,
        max_elements: request.ax_max_elements,
        include_values: request.ax_include_values,
        ..AxTreeRequest::default()
    };
    let source_command = format!("@observe {}", request.mode.as_str());
    match capture_default_ax_snapshot(&ax_request) {
        Ok(snapshot) => filter_ax_snapshot(snapshot, request, display_scope_resolution)
            .with_observation(&source_command),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && !request.ax_required => {
            AxSnapshot::permission_denied(current_ax_platform()).with_observation(&source_command)
        }
        Err(err) if err.kind() == io::ErrorKind::Unsupported && !request.ax_required => {
            AxSnapshot::unsupported().with_observation(&source_command)
        }
        Err(err) => Err(err),
    }
}

fn filter_ax_snapshot(
    mut snapshot: AxSnapshot,
    request: &ObserveRequest,
    display_scope_resolution: Option<&DisplayScopeResolution>,
) -> AxSnapshot {
    if let Some(target) = request
        .target
        .as_ref()
        .filter(|target| !target.is_empty() && target.bundle_id.is_none())
    {
        snapshot
            .windows
            .retain(|window| target.matches_ax_window(window));
    }
    if let Some(resolution) = display_scope_resolution {
        snapshot.windows.retain(|window| {
            window
                .rect
                .map(DisplayRect::from)
                .map(|rect| display_intersects_rect(&resolution.resolved, rect))
                .unwrap_or(false)
        });
    }
    snapshot.window_count = snapshot.windows.len();
    snapshot.element_count = snapshot.windows.iter().map(ax_window_element_count).sum();
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_ax::AxWindow,
        control_display_scope::{DisplaySummary, DISPLAY_ID_STABILITY_SESSION},
    };

    #[test]
    fn scoped_ax_filter_should_apply_without_observe_target() {
        let request = ObserveRequest::default();
        let snapshot = two_display_snapshot();
        let resolution = right_display_resolution();

        let filtered = filter_ax_snapshot(snapshot, &request, Some(&resolution));

        assert_eq!(filtered.window_count, 1);
        assert_eq!(filtered.windows[0].id, "pid:2/window:0");
    }

    #[test]
    fn scoped_ax_filter_should_apply_when_bundle_target_cannot_filter_ax_process() {
        let mut request = ObserveRequest::default();
        request.target = Some(super::super::request::ObserveTarget {
            bundle_id: Some("com.example.Editor".to_owned()),
            ..super::super::request::ObserveTarget::default()
        });
        let snapshot = two_display_snapshot();
        let resolution = right_display_resolution();

        let filtered = filter_ax_snapshot(snapshot, &request, Some(&resolution));

        assert_eq!(filtered.window_count, 1);
        assert_eq!(filtered.windows[0].id, "pid:2/window:0");
    }

    fn two_display_snapshot() -> AxSnapshot {
        AxSnapshot::complete(
            "test",
            vec![
                ax_window("pid:1/window:0", 1, 10),
                ax_window("pid:2/window:0", 2, 1_200),
            ],
            false,
        )
    }

    fn ax_window(id: &str, pid: i32, x: i32) -> AxWindow {
        AxWindow {
            id: id.to_owned(),
            ref_id: None,
            pid,
            process_name: format!("fixture-{pid}"),
            title: Some(format!("window-{pid}")),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: Some(crate::control_ax::AxRect {
                x,
                y: 20,
                width: 500,
                height: 300,
            }),
            focused: Some(false),
            elements: Vec::new(),
        }
    }

    fn right_display_resolution() -> DisplayScopeResolution {
        DisplayScopeResolution {
            selector: serde_json::json!({"id": "d2"}),
            resolved: DisplaySummary {
                display_id: "d2".to_owned(),
                stable_key: Some("test:d2".to_owned()),
                primary: false,
                name: "right".to_owned(),
                os_rect: DisplayRect {
                    x: 1_000,
                    y: 0,
                    width: 1_000,
                    height: 800,
                },
                image_rect: DisplayRect {
                    x: 1_000,
                    y: 0,
                    width: 1_000,
                    height: 800,
                },
                scale_factor: 1.0,
                rotation: 0.0,
                display_id_stability: DISPLAY_ID_STABILITY_SESSION,
            },
            status: "applied",
            display_overlap_ratio: None,
        }
    }
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
