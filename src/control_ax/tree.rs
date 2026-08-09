//! control_ax 模块的 AX 树/捕获/解析相关函数。
//!
//! 收纳 capture / resolve / current platform / find-in-snapshot 这一族
//! 只读或轻状态操作。状态变更的 verb (press / focus / value / scroll /
//! type-text / key) 留在 control_ax.rs 或后续 commit 搬到 press.rs / input.rs。
//!
//! 模块布局:
//! - public API: `capture_*` / `resolve_*` / `current_ax_platform`
//! - helpers (pub(crate)): AX snapshot selector 构造、target 解析、error mapping
//! - platform_* (本 commit 暂未搬,留在 control_ax.rs)

use crate::{
    control_observation::selector::{
        AppSelector, DurableSelectorDraft, ElementSelector, SelectorEnvelope, SelectorKind,
        SelectorRect, SelectorRedaction, WindowSelector,
    },
    control_observation::{
        observation_ref_name, resolve_observation_ref, stale_observation_ref_error,
        ObservationRefEntry,
    },
    control_window::resolve_unique_app_window_id,
};
use serde_json::json;
use std::io;

use super::query::AxFindRequest;
use super::types::*;
use super::{
    invalid_input, platform_capture_current_window, platform_resolve_current_target_rect,
    to_invalid_input, AxBackend,
};

// ---- commit 2 留下的 capture / platform-info functions ----

/// 当前 daemon 进程对应的 AX 后端平台标识。
pub fn current_ax_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unsupported"
    }
}

/// 默认全屏 AX 树捕获入口。
pub fn capture_default_ax_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    SystemAxBackend.snapshot(request)
}

/// 捕获当前 target_id 对应的子树。
pub fn capture_current_ax_subtree(
    target_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxCapturedSubtree> {
    super::platform_capture_current_subtree(target_id, request)
}

// ---- collect_element_refs (was lines 378-425) ----
pub(crate) fn collect_element_refs(
    platform: &str,
    app_selector: &AppSelector,
    window_selector: &WindowSelector,
    next_ref_index: &mut usize,
    elements: &mut [AxElement],
    refs: &mut Vec<ObservationRefEntry>,
    selector_drafts: &mut Vec<DurableSelectorDraft>,
) {
    for element in elements {
        let ref_id = match &element.ref_id {
            Some(ref_id) => {
                reserve_existing_ref_index(ref_id, next_ref_index);
                ref_id.clone()
            }
            None => {
                let ref_id = observation_ref_name(*next_ref_index);
                *next_ref_index += 1;
                element.ref_id = Some(ref_id.clone());
                ref_id
            }
        };
        refs.push(ObservationRefEntry {
            ref_id: ref_id.clone(),
            backend_id: element.id.clone(),
            kind: "element".to_owned(),
        });
        selector_drafts.push(element_selector_draft(
            platform,
            app_selector,
            window_selector,
            element,
            &ref_id,
        ));

        if !element.children.is_empty() {
            collect_element_refs(
                platform,
                app_selector,
                window_selector,
                next_ref_index,
                &mut element.children,
                refs,
                selector_drafts,
            );
        }
    }
}

// ---- window_selector_draft (was lines 427-441) ----
pub(crate) fn window_selector_draft(
    platform: &str,
    window: &AxWindow,
    ref_id: &str,
) -> DurableSelectorDraft {
    DurableSelectorDraft::new(
        ref_id.to_owned(),
        SelectorKind::AxWindow,
        window.id.clone(),
        SelectorEnvelope {
            platform: platform.to_owned(),
            app: Some(app_selector_for_window(window)),
            window: Some(window_selector_for_ax_window(window)),
            element: None,
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

// ---- element_selector_draft (was lines 443-470) ----
pub(crate) fn element_selector_draft(
    platform: &str,
    app_selector: &AppSelector,
    window_selector: &WindowSelector,
    element: &AxElement,
    ref_id: &str,
) -> DurableSelectorDraft {
    DurableSelectorDraft::new(
        ref_id.to_owned(),
        SelectorKind::AxElement,
        element.id.clone(),
        SelectorEnvelope {
            platform: platform.to_owned(),
            app: Some(app_selector.clone()),
            window: Some(window_selector.clone()),
            element: Some(ElementSelector {
                role: element.role.clone(),
                subrole: element.subrole.clone(),
                name: element.name.clone(),
                description: element.description.clone(),
                actions: element.actions.clone(),
                ax_path: element.ax_path.clone(),
            }),
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

// ---- app_selector_for_window (was lines 472-478) ----
pub(crate) fn app_selector_for_window(window: &AxWindow) -> AppSelector {
    AppSelector {
        name: window.process_name.clone(),
        bundle_id: None,
        pid_hint: Some(window.pid),
    }
}

// ---- window_selector_for_ax_window (was lines 480-486) ----
pub(crate) fn window_selector_for_ax_window(window: &AxWindow) -> WindowSelector {
    WindowSelector {
        title: window.title.clone(),
        role: window.role.clone(),
        rect: window.rect.map(selector_rect_from_ax_rect),
    }
}

// ---- selector_rect_from_ax_rect (was lines 488-495) ----
pub(crate) fn selector_rect_from_ax_rect(rect: AxRect) -> SelectorRect {
    SelectorRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

// ---- reserve_existing_ref_index (was lines 497-505) ----
pub(crate) fn reserve_existing_ref_index(ref_id: &str, next_ref_index: &mut usize) {
    let Some(index) = ref_id
        .strip_prefix("@e")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return;
    };
    *next_ref_index = (*next_ref_index).max(index.saturating_add(1));
}

// ---- capture_ax_find_snapshot (was lines 849-855) ----
pub fn capture_ax_find_snapshot(request: &AxFindRequest) -> io::Result<AxSnapshot> {
    capture_ax_find_snapshot_with(
        request,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
    )
}

// ---- capture_ax_find_snapshot_with (was lines 857-867) ----
pub(crate) fn capture_ax_find_snapshot_with(
    request: &AxFindRequest,
    capture_global: impl FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    capture_window: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<AxSnapshot> {
    // App 菜单栏挂在 AXApplication 下,不是任意 AXWindow 的子树。
    // 因此必须保留 app-menu root,交给平台后端按应用筛选菜单快照。
    if matches!(request.tree.scope, AxTreeScope::AppMenu) {
        return capture_global(&request.tree);
    }
    let Some(window) = request.window.as_ref() else {
        return capture_global(&request.tree);
    };
    let window_id = window.resolve_window_id()?;
    capture_window(&window_id, &request.tree)
}

// ---- capture_current_ax_window_snapshot (was lines 869-874) ----
pub fn capture_current_ax_window_snapshot(
    window_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxSnapshot> {
    platform_capture_current_window(window_id, request)
}

// ---- capture_semantic_target_snapshot (was lines 876-886) ----
pub(crate) fn capture_semantic_target_snapshot(
    target: &AxTarget,
    request: &AxTreeRequest,
) -> io::Result<AxSnapshot> {
    capture_semantic_target_snapshot_with(
        target,
        request,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
    )
}

// ---- capture_semantic_target_snapshot_with (was lines 888-900) ----
pub(crate) fn capture_semantic_target_snapshot_with(
    target: &AxTarget,
    request: &AxTreeRequest,
    capture_global: impl FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    capture_window: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<AxSnapshot> {
    match target.window_id.as_deref() {
        // 已解析的 window_id 是本次动作的归属边界.只读取目标窗口,
        // 避免无关应用的 AX 状态干扰 semantic match.
        Some(window_id) => capture_window(window_id, request),
        None => capture_global(request),
    }
}

// ---- resolve_current_ax_target_rect (was lines 903-945) ----
pub fn resolve_current_ax_target_rect(target: &AxTarget) -> io::Result<AxResolvedTargetRect> {
    let target = materialize_app_window_target(target)?;
    if let Some(target_id) = direct_ax_target_id(&target)? {
        return platform_resolve_current_target_rect(&target_id);
    }

    let request = AxTreeRequest {
        depth: 8,
        max_elements: 5000,
        include_values: false,
        ..AxTreeRequest::default()
    };
    let snapshot = capture_semantic_target_snapshot(&target, &request)?;
    if snapshot.capture_status != "complete" {
        return Err(ax_snapshot_status_error(&snapshot));
    }

    let target_id = resolve_target_id_in_snapshot(&snapshot, &target)?;
    for window in &snapshot.windows {
        if window.id == target_id {
            return Ok(AxResolvedTargetRect {
                target_id,
                target_type: "window",
                window_id: Some(window.id.clone()),
                rect: window.rect,
            });
        }

        if let Some(element) = find_ax_element_by_id(&window.elements, &target_id) {
            return Ok(AxResolvedTargetRect {
                target_id,
                target_type: "element",
                window_id: Some(window.id.clone()),
                rect: element.rect,
            });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("AX target id 已失效或不存在: {target_id}"),
    ))
}

// ---- direct_ax_target_id (was lines 947-961) ----
pub(crate) fn direct_ax_target_id(target: &AxTarget) -> io::Result<Option<String>> {
    target.validate()?;

    if let Some(id) = target.id.as_deref() {
        return Ok(Some(id.to_owned()));
    }

    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        return resolve_observation_ref(observation_id, ref_id).map(|entry| Some(entry.backend_id));
    }

    Ok(None)
}

// ---- materialize_app_window_target (was lines 963-969) ----
/// 将 Window API 的 app selector 转换为 AX 可直接消费的 canonical window ID.
///
/// app 名和 AX process 名可能因系统本地化而不同.因此 app 只在 Window API
/// 命名域中解析一次,随后必须清除,避免对正确 AX snapshot 做第二次异域字符串比较.
pub(crate) fn materialize_app_window_target(target: &AxTarget) -> io::Result<AxTarget> {
    materialize_app_window_target_with(target, resolve_unique_app_window_id)
}

// ---- materialize_app_window_target_with (was lines 971-985) ----
pub(crate) fn materialize_app_window_target_with(
    target: &AxTarget,
    resolve_app: impl FnOnce(&str) -> io::Result<String>,
) -> io::Result<AxTarget> {
    target.validate()?;
    let Some(app) = target.app.as_deref() else {
        return Ok(target.clone());
    };

    let mut materialized = target.clone();
    materialized.window_id = Some(resolve_app(app)?);
    materialized.app = None;
    materialized.validate()?;
    Ok(materialized)
}

// ---- resolve_target_id_in_snapshot (was lines 1769-1813) ----
pub fn resolve_target_id_in_snapshot(
    snapshot: &AxSnapshot,
    target: &AxTarget,
) -> io::Result<String> {
    target.validate().map_err(to_invalid_input)?;

    if let Some(id) = &target.id {
        if snapshot.contains_element_id(id) {
            return Ok(id.clone());
        }
        return Err(invalid_input(format!("AX target id 已失效或不存在: {id}")));
    }

    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        let entry = resolve_observation_ref(observation_id, ref_id)?;
        if snapshot.contains_element_id(&entry.backend_id) {
            return Ok(entry.backend_id);
        }
        return Err(stale_observation_ref_error(
            observation_id,
            ref_id,
            format!("backend id 已不在当前 AX snapshot 中: {}", entry.backend_id),
        ));
    }

    let mut matches = Vec::<String>::new();

    for window in &snapshot.windows {
        if !target.matches_window(window) {
            continue;
        }
        collect_matching_element_ids(target, &window.elements, &mut matches);
        if matches.len() > 1 {
            return Err(invalid_input("AX semantic target 匹配到多个元素"));
        }
    }

    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(invalid_input("AX semantic target 未匹配到元素")),
        _ => Err(invalid_input("AX semantic target 匹配到多个元素")),
    }
}

// ---- collect_matching_element_ids (was lines 1815-1826) ----
pub(crate) fn collect_matching_element_ids(
    target: &AxTarget,
    elements: &[AxElement],
    matches: &mut Vec<String>,
) {
    for element in elements {
        if target.matches_element(element) {
            matches.push(element.id.clone());
        }
        collect_matching_element_ids(target, &element.children, matches);
    }
}

// ---- find_ax_element_by_id (was lines 1828-1838) ----
pub(crate) fn find_ax_element_by_id<'a>(
    elements: &'a [AxElement],
    target_id: &str,
) -> Option<&'a AxElement> {
    for element in elements {
        if element.id == target_id {
            return Some(element);
        }
        if let Some(found) = find_ax_element_by_id(&element.children, target_id) {
            return Some(found);
        }
    }
    None
}

// ---- ax_snapshot_status_error (was lines 1840-1855) ----
pub(crate) fn ax_snapshot_status_error(snapshot: &AxSnapshot) -> io::Error {
    let kind = match snapshot.capture_status.as_str() {
        "permission_denied" => io::ErrorKind::PermissionDenied,
        "unsupported" => io::ErrorKind::Unsupported,
        _ => io::ErrorKind::Other,
    };
    let value = json!({
        "kind": "ax-target-resolution",
        "error_code": "AX_SNAPSHOT_UNAVAILABLE",
        "capture_status": snapshot.capture_status.as_str(),
        "permission_status": snapshot.permission_status.as_str(),
        "platform": snapshot.platform.as_str(),
        "message": "AX snapshot 不可用,无法解析 mouse target rect",
    });
    io::Error::new(kind, value.to_string())
}
