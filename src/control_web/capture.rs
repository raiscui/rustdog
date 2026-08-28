use super::{
    build_web_find_response_json_with_refresh, select_target_window, WebFindRequest,
    WindowSelection,
};
use crate::ax_query::{
    capture_current_ax_subtree, capture_current_ax_window_snapshot, capture_default_ax_snapshot,
};
use crate::control_ax::{AxCapturedSubtree, AxSnapshot, AxTreeRequest};
use crate::control_observation::resolve_observation_ref;
use std::io;

/// 使用当前系统 AX backend 构建默认 `@web-find` 响应。
///
/// capture 选择集中在本模块,避免 response matching 与平台取证逻辑继续耦合。
pub(super) fn build_default_web_find_response_json(request: &WebFindRequest) -> io::Result<String> {
    build_web_find_response_json_with_captures(
        request,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
        |target_id, tree_request| capture_current_ax_subtree(target_id, tree_request).map(Some),
    )
}

/// 以可注入 capture backend 跑完整 web-find response 路径。
///
/// 显式 window target 优先直接捕获目标窗口,避免共享 global 预算在目标
/// `AXWebArea` 展开前耗尽。active-browser 仍需全局快照做消歧。
pub(super) fn build_web_find_response_json_with_captures<G, W, R>(
    request: &WebFindRequest,
    mut capture_global: G,
    mut capture_window: W,
    refresh_web_area: R,
) -> io::Result<String>
where
    G: FnMut(&AxTreeRequest) -> io::Result<AxSnapshot>,
    W: FnMut(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
    R: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    let snapshot = capture_web_snapshot_with(request, &mut capture_global, &mut capture_window)?;
    build_web_find_response_json_with_refresh(&snapshot, request, refresh_web_area)
}

/// 按 Web 请求的窗口目标选择 global 或 targeted AX snapshot。
///
/// `@web-find` 与 `@web-act` 必须复用这一真相源。否则只读查找能命中,
/// 真正动作或重试却可能重新退回被共享预算截断的 global snapshot。
pub(super) fn capture_web_snapshot_with<G, W>(
    request: &WebFindRequest,
    capture_global: &mut G,
    capture_window: &mut W,
) -> io::Result<AxSnapshot>
where
    G: FnMut(&AxTreeRequest) -> io::Result<AxSnapshot>,
    W: FnMut(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
{
    let tree_request = request.tree_request();
    let snapshot = match capture_target_window_id(request) {
        Some(window_id) => match capture_window(&window_id, &tree_request) {
            Ok(snapshot) => snapshot,
            // targeted identity 已过期或 backend 暂时不可用时,回退原 global 路径。
            // 这样下游仍能返回现有的 BROWSER_WINDOW_NOT_FOUND / WINDOW_REF_INVALID blocker。
            Err(_) => capture_global(&tree_request)?,
        },
        None => {
            // active-browser没有可直接下推的窗口身份,必须先用全局快照完成消歧。
            // 这里只取fresh backend id;最终匹配仍在targeted快照上重新执行完整门禁。
            let global_snapshot = capture_global(&tree_request)?;
            let target_window_id = {
                let mut trace = Vec::new();
                match select_target_window(&global_snapshot, request, &mut trace) {
                    WindowSelection::Selected { window, .. } => Some(window.id.clone()),
                    _ => None,
                }
            };

            match target_window_id {
                Some(window_id) => capture_window(&window_id, &tree_request)
                    // targeted backend失败时保留global结果,由既有response路径返回blocker。
                    .unwrap_or(global_snapshot),
                None => global_snapshot,
            }
        }
    };
    Ok(snapshot)
}

/// 只在 target 已给出可解析的显式窗口身份时返回 backend id。
///
/// invalid / stale ref 不在 capture 层改写错误协议;它们回到原 global response 路径,
/// 由 `select_target_window` 生成已有的结构化 blocker。
fn capture_target_window_id(request: &WebFindRequest) -> Option<String> {
    if let Some(window_id) = request.target.window_id.as_ref() {
        return Some(window_id.clone());
    }

    let window_ref = request.target.window_ref.as_deref()?;
    let observation_id = request.target.observation_id.as_deref()?;
    let entry = resolve_observation_ref(observation_id, window_ref).ok()?;
    (entry.kind == "window").then_some(entry.backend_id)
}
