//! control_ax 模块的 AX target 解析与 observation 注册富化。
//!
//! 模块布局 (阶段 3 拆分后):
//! - 纯 capture / 查找函数已迁往 `crate::ax_query` (无状态捕获核心)
//! - 本文件只保留认识 observation 的桥接层:
//!   - selector draft 构造 (供 AxSnapshot::with_observation 注册 refs/selectors)
//!   - target 解析 (observation ref -> backend id, snapshot 内 semantic 匹配)
//!
//! 依赖方向: control_ax (verb 层) -> { ax_query, control_observation }。

use crate::{
    control_observation::selector::{
        AppSelector, DurableSelectorDraft, ElementSelector, SelectorEnvelope, SelectorKind,
        SelectorRect, SelectorRedaction, WindowSelector,
    },
    control_observation::{
        observation_ref_name, resolve_observation_ref, stale_observation_ref_error,
        ObservationRefEntry,
    },
};
use std::io;

use super::types::*;
use super::AxTarget;
use crate::ax_query::{
    ax_snapshot_status_error, capture_semantic_target_snapshot, find_ax_element_by_id,
    materialize_app_window_target,
};

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

// ---- resolve_current_ax_target_rect (was lines 903-945) ----
pub fn resolve_current_ax_target_rect(target: &AxTarget) -> io::Result<AxResolvedTargetRect> {
    let target = materialize_app_window_target(target)?;
    if let Some(target_id) = direct_ax_target_id(&target)? {
        return super::platform_resolve_current_target_rect(&target_id);
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

// ---- resolve_target_id_in_snapshot (was lines 1769-1813) ----
pub fn resolve_target_id_in_snapshot(
    snapshot: &AxSnapshot,
    target: &AxTarget,
) -> io::Result<String> {
    target.validate().map_err(super::to_invalid_input)?;

    if let Some(id) = &target.id {
        if snapshot.contains_element_id(id) {
            return Ok(id.clone());
        }
        return Err(super::invalid_input(format!(
            "AX target id 已失效或不存在: {id}"
        )));
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
            return Err(super::invalid_input("AX semantic target 匹配到多个元素"));
        }
    }

    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(super::invalid_input("AX semantic target 未匹配到元素")),
        _ => Err(super::invalid_input("AX semantic target 匹配到多个元素")),
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
