//! AX 捕获核心: 全局/窗口/子树捕获入口与 snapshot 内查找。
//!
//! 全部函数自 control_ax/tree.rs 迁入, 语义与签名保持不变。
//! observation-scoped 的 selector 富化与 ref 解析留在 control_ax 侧
//! (那是 verb 层职责, 见 tree.rs 现头注释)。

use crate::control_ax::query::AxFindRequest;
use crate::control_ax::types::{AxCapturedSubtree, AxElement, AxSnapshot, AxTarget, AxTreeRequest};
use crate::control_ax::AxBackend;
use crate::control_ax::{
    platform_capture_current_subtree, platform_capture_current_window, SystemAxBackend,
};
use crate::control_resource_lane::capture_resource_epochs;
use crate::control_window::resolve_unique_app_window_id;
use serde_json::json;
use std::io;

// ---- capture / platform-info functions ----

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
    platform_capture_current_subtree(target_id, request)
}

/// 从 AX backend id 中提取标准窗口 id。
///
/// menu-bar 等非窗口 AX root 返回 `None`,由调用方保留全局 capture。
pub fn ax_window_id_from_backend_id(backend_id: &str) -> Option<&str> {
    let window_id = backend_id
        .split_once("/path:")
        .map_or(backend_id, |(window_id, _)| window_id);
    let (pid, window_index) = window_id.strip_prefix("pid:")?.split_once("/window:")?;
    (pid.parse::<i32>().is_ok() && window_index.parse::<usize>().is_ok()).then_some(window_id)
}

/// 按请求的 window identity / scope 选择全局或定向窗口捕获。
pub fn capture_ax_find_snapshot(request: &AxFindRequest) -> io::Result<AxSnapshot> {
    capture_ax_find_snapshot_with(
        request,
        capture_default_ax_snapshot,
        capture_current_ax_window_snapshot,
    )
}

/// `capture_ax_find_snapshot` 的可注入变体, 测试用它替换真实捕获。
pub fn capture_ax_find_snapshot_with(
    request: &AxFindRequest,
    capture_global: impl FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    capture_window: impl FnOnce(&str, &AxTreeRequest) -> io::Result<AxSnapshot>,
) -> io::Result<AxSnapshot> {
    // App 菜单栏挂在 AXApplication 下,不是任意 AXWindow 的子树。
    // 因此必须保留 app-menu root,交给平台后端按应用筛选菜单快照。
    if matches!(
        request.tree.scope,
        crate::control_ax::types::AxTreeScope::AppMenu
    ) {
        return capture_global(&request.tree);
    }
    let Some(window) = request.window.as_ref() else {
        return capture_global(&request.tree);
    };
    let window_id = window.resolve_window_id()?;
    capture_window(&window_id, &request.tree)
}

/// 定向捕获单个窗口, 并把 capture-start 时刻的 resource epoch 快照嵌入 snapshot。
pub fn capture_current_ax_window_snapshot(
    window_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxSnapshot> {
    let resource_capture = capture_resource_epochs();
    let mut snapshot = platform_capture_current_window(window_id, request)?;
    snapshot.resource_epoch_capture = Some(resource_capture);
    Ok(snapshot)
}

/// 按 target 的已解析 window_id 选择定向或全局捕获 (semantic match 前置)。
pub fn capture_semantic_target_snapshot(
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

/// `capture_semantic_target_snapshot` 的可注入变体, 测试用它替换真实捕获。
pub fn capture_semantic_target_snapshot_with(
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

/// 将 Window API 的 app selector 转换为 AX 可直接消费的 canonical window ID.
///
/// app 名和 AX process 名可能因系统本地化而不同.因此 app 只在 Window API
/// 命名域中解析一次,随后必须清除,避免对正确 AX snapshot 做第二次异域字符串比较.
pub fn materialize_app_window_target(target: &AxTarget) -> io::Result<AxTarget> {
    materialize_app_window_target_with(target, resolve_unique_app_window_id)
}

/// `materialize_app_window_target` 的可注入变体, resolve_app 由测试提供。
pub fn materialize_app_window_target_with(
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

/// 在元素树中按 backend id 深度查找元素。
pub fn find_ax_element_by_id<'a>(
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

/// 把 snapshot 的 capture/permission 状态映射为结构化 io::Error。
pub fn ax_snapshot_status_error(snapshot: &AxSnapshot) -> io::Error {
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

/// capture 分发路由测试 (自 control_ax tests 迁入, 语义不变)。
///
/// 覆盖: window identity / app-menu / semantic target 的全局 vs 定向路由,
/// 以及"无 window identity 保持全局捕获"的默认行为。
#[cfg(test)]
mod capture_routing_tests {
    use super::*;
    use crate::control_ax::parse_ax_find_payload;
    use crate::control_ax::types::AxTreeScope;
    use std::cell::Cell;

    #[test]
    fn ax_find_window_identity_should_route_only_to_targeted_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let request =
            parse_ax_find_payload(r#"{window:{window_id:"pid:7/window:1"},role:"AXButton"}"#)
                .unwrap();

        let snapshot = capture_ax_find_snapshot_with(
            &request,
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |window_id, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                assert_eq!(window_id, "pid:7/window:1");
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "targeted");
        assert_eq!(global_calls.get(), 0);
        assert_eq!(targeted_calls.get(), 1);
    }

    #[test]
    fn ax_find_app_menu_should_pass_app_selector_to_capture_backend() {
        let request =
            parse_ax_find_payload(r#"{root:"app-menu",app:"Finder",role:"AXMenuBarItem"}"#)
                .expect("app menu request should parse");

        let snapshot = capture_ax_find_snapshot_with(
            &request,
            |tree| {
                assert_eq!(tree.scope, AxTreeScope::AppMenu);
                assert_eq!(tree.app_menu_app.as_deref(), Some("Finder"));
                Ok(AxSnapshot::complete("targeted-app-menu", Vec::new(), false))
            },
            |_, _| panic!("app menu must not be captured as a window subtree"),
        )
        .expect("app menu capture should succeed");

        assert_eq!(snapshot.platform, "targeted-app-menu");
    }

    #[test]
    fn semantic_target_window_id_should_route_only_to_targeted_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let target = AxTarget {
            window_id: Some("pid:7/window:1".to_owned()),
            role: Some("AXButton".to_owned()),
            description: Some("1".to_owned()),
            ..AxTarget::default()
        };

        let snapshot = capture_semantic_target_snapshot_with(
            &target,
            &AxTreeRequest::default(),
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |window_id, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                assert_eq!(window_id, "pid:7/window:1");
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "targeted");
        assert_eq!(global_calls.get(), 0);
        assert_eq!(targeted_calls.get(), 1);
    }

    #[test]
    fn semantic_target_without_window_id_should_keep_global_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let target = AxTarget {
            role: Some("AXButton".to_owned()),
            description: Some("1".to_owned()),
            ..AxTarget::default()
        };

        let snapshot = capture_semantic_target_snapshot_with(
            &target,
            &AxTreeRequest::default(),
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |_, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "global");
        assert_eq!(global_calls.get(), 1);
        assert_eq!(targeted_calls.get(), 0);
    }

    #[test]
    fn ax_find_without_window_identity_should_keep_global_capture() {
        let global_calls = Cell::new(0);
        let targeted_calls = Cell::new(0);
        let request = parse_ax_find_payload(r#"{role:"AXButton"}"#).unwrap();

        let snapshot = capture_ax_find_snapshot_with(
            &request,
            |_| {
                global_calls.set(global_calls.get() + 1);
                Ok(AxSnapshot::complete("global", Vec::new(), false))
            },
            |_, _| {
                targeted_calls.set(targeted_calls.get() + 1);
                Ok(AxSnapshot::complete("targeted", Vec::new(), false))
            },
        )
        .unwrap();

        assert_eq!(snapshot.platform, "global");
        assert_eq!(global_calls.get(), 1);
        assert_eq!(targeted_calls.get(), 0);
    }

    /// role value 收集: 深层命中 + 跳过脱敏 + 不做排序去重归一化 (调用方语义)。
    #[test]
    fn collect_ax_role_values_should_traverse_deeply_and_skip_redacted() {
        use crate::control_ax::types::{AxElement, AxWindow};

        fn leaf(role: &str, value: Option<&str>, redacted: bool) -> AxElement {
            AxElement {
                id: format!("id-{role}-{}", value.unwrap_or("none")),
                ref_id: None,
                role: role.to_owned(),
                subrole: None,
                name: None,
                value: value.map(str::to_owned),
                value_redacted: redacted,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: Vec::new(),
                children: Vec::new(),
            }
        }

        // depth 3 嵌套: group > group > AXStaticText("0"), 同层混入脱敏项与非目标 role。
        let mut deep = leaf("AXStaticText", Some("0"), false);
        for index in 1..=2 {
            deep = AxElement {
                id: format!("id-group-{index}"),
                children: vec![deep],
                ..leaf("AXGroup", None, false)
            };
        }
        let window = AxWindow {
            id: "pid:7/window:0".to_owned(),
            ref_id: None,
            pid: 7,
            process_name: "Demo".to_owned(),
            title: None,
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: None,
            elements: vec![
                deep,
                leaf("AXStaticText", Some("secret"), true),
                leaf("AXButton", Some("press-me"), false),
            ],
        };
        let snapshot = AxSnapshot::complete("test", vec![window], false);

        let mut values = collect_ax_role_values(&snapshot, "AXStaticText");
        assert_eq!(values, vec!["0".to_owned()]);
        // 多窗口去重排序是调用方语义, 这里原样保留重复。
        values.extend(collect_ax_role_values(&snapshot, "AXStaticText"));
        assert_eq!(values.len(), 2);
    }

    /// 纯度回归: backend id -> 窗口 id 的提取规则 (menu-bar root 返回 None)。
    #[test]
    fn ax_window_id_from_backend_id_should_extract_only_canonical_window_ids() {
        assert_eq!(
            ax_window_id_from_backend_id("pid:321/window:0"),
            Some("pid:321/window:0")
        );
        assert_eq!(
            ax_window_id_from_backend_id("pid:321/window:2/path:7"),
            Some("pid:321/window:2")
        );
        assert_eq!(ax_window_id_from_backend_id("app-menu:Finder"), None);
        assert_eq!(ax_window_id_from_backend_id("pid:not-a-pid/window:0"), None);
    }
}

/// 收集 snapshot 内指定 role 的全部元素 value (深度遍历, 跳过被脱敏的元素)。
///
/// 只做纯遍历: 不排序、不去重、不做任何语义归一化,
/// 调用方 (如 press postcondition 验证) 按自己的语义加工结果。
pub fn collect_ax_role_values(snapshot: &AxSnapshot, role: &str) -> Vec<String> {
    let mut values = Vec::new();
    for window in &snapshot.windows {
        collect_role_values_from_elements(&window.elements, role, &mut values);
    }
    values
}

/// `collect_ax_role_values` 的递归内核。
fn collect_role_values_from_elements(elements: &[AxElement], role: &str, values: &mut Vec<String>) {
    for element in elements {
        if element.role == role && !element.value_redacted {
            if let Some(value) = element.value.as_deref() {
                values.push(value.to_owned());
            }
        }
        collect_role_values_from_elements(&element.children, role, values);
    }
}
