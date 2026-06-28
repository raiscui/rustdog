use super::*;
use crate::control_observation::{record_observation, ObservationRefEntry, ObservationRoot};
use serde_json::Value;
use std::cell::Cell;

#[test]
fn parse_web_find_should_accept_active_browser_match_and_roles() {
    let request = parse_web_find_payload(
        r#"{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}"#,
    )
    .unwrap();

    assert_eq!(request.target.browser, WebFindBrowserTarget::Active);
    assert_eq!(request.target.window_id, None);
    assert_eq!(request.query.text, "首页");
    assert_eq!(request.roles, vec!["AXLink", "AXButton"]);
    assert_eq!(request.limit, 10);
    assert_eq!(request.depth, DEFAULT_WEB_FIND_DEPTH);
}

#[test]
fn parse_web_find_should_accept_display_scope_and_reject_display_id() {
    let request =
        parse_web_find_payload(r#"{match:{text:"首页"},scope:{display:{id:"d2"}}}"#).unwrap();

    assert!(request.display_scope.is_some());
    assert!(
        parse_web_find_payload(r#"{match:{text:"首页"},display_id:"d2"}"#)
            .unwrap_err()
            .to_string()
            .contains("scope")
    );
}

#[test]
fn parse_web_find_should_accept_window_id_target() {
    let request =
        parse_web_find_payload(r#"{target:{window_id:"pid:96405/window:3"},match:{text:"首页"}}"#)
            .unwrap();

    assert_eq!(
        request.target.window_id.as_deref(),
        Some("pid:96405/window:3")
    );
    assert_eq!(request.target.browser, WebFindBrowserTarget::Active);
    assert_eq!(request.target.scope_str(), "target_window_web_area");
}

#[test]
fn parse_web_find_should_accept_window_ref_target() {
    let request = parse_web_find_payload(
        r#"{target:{window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"}}"#,
    )
    .unwrap();

    assert_eq!(request.target.window_ref.as_deref(), Some("@e1"));
    assert_eq!(request.target.observation_id.as_deref(), Some("obs-123"));
    assert_eq!(request.target.window_id, None);
    assert_eq!(request.target.scope_str(), "target_window_web_area");
    assert!(parse_web_find_payload(r#"{target:{window_ref:"@e1"},match:{text:"首页"}}"#).is_err());
    assert!(
        parse_web_find_payload(r#"{target:{observation_id:"obs-123"},match:{text:"首页"}}"#)
            .is_err()
    );
    assert!(parse_web_find_payload(
        r#"{target:{window_id:"pid:1/window:0",window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"}}"#
    )
    .is_err());
}

#[test]
fn web_find_should_search_inside_ax_web_area_only() {
    let request = parse_web_find_payload(r#"{match:{text:"首页"},limit:5}"#).unwrap();
    let value = response_value(&snapshot_with_page_link(), &request);

    assert_eq!(value["kind"], "web-find");
    assert_eq!(value["schema"], WEB_FIND_SCHEMA);
    assert_eq!(value["status"], "complete");
    assert_eq!(value["scope"], "active_web_area");
    assert_eq!(value["window"]["process_name"], "Google Chrome");
    assert_eq!(
        value["web_area"]["id"],
        "pid:1/window:0/path:0.0.0.0.1.0.0.0"
    );
    assert_eq!(value["match_count"], 1);
    assert_eq!(value["matches"][0]["role"], "AXLink");
    assert_eq!(value["matches"][0]["description"], "首页");
    assert_eq!(value["matches"][0]["matched_field"], "description");
    assert_eq!(value["matches"][0]["actions"][0], "AXPress");
    assert_eq!(value["observation"]["source_command"], "@web-find");
}

#[test]
fn web_find_should_resolve_window_ref_target() {
    let observation = record_test_observation_ref("@e1", "pid:1/window:1", "window");
    let request = parse_web_find_payload(&format!(
        r#"{{target:{{window_ref:"@e1",observation_id:"{}"}},match:{{text:"首页"}},limit:5}}"#,
        observation.observation_id
    ))
    .unwrap();
    let value = response_value(&snapshot_with_two_unfocused_browser_windows(), &request);

    assert_eq!(value["status"], "complete");
    assert_eq!(value["scope"], "target_window_web_area");
    assert_eq!(value["target"]["window_ref"], "@e1");
    assert_eq!(
        value["target"]["observation_id"].as_str(),
        Some(observation.observation_id.as_str())
    );
    assert_eq!(value["window"]["window_id"], "pid:1/window:1");
    assert_eq!(value["match_count"], 1);
    assert!(trace_contains(&value, "resolve-window-ref", "ok"));
    assert!(trace_contains(&value, "target-browser-window", "ok"));
}

#[test]
fn web_find_should_block_non_window_ref_target() {
    let observation = record_test_observation_ref("@e7", "pid:1/window:1/path:0.0", "element");
    let request = parse_web_find_payload(&format!(
        r#"{{target:{{window_ref:"@e7",observation_id:"{}"}},match:{{text:"首页"}},limit:5}}"#,
        observation.observation_id
    ))
    .unwrap();
    let value = response_value(&snapshot_with_two_unfocused_browser_windows(), &request);

    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "WINDOW_REF_INVALID");
    assert_eq!(value["match_count"], 0);
    assert!(trace_contains(&value, "resolve-window-ref", "blocked"));
}

#[test]
fn web_find_should_select_explicit_window_id_without_focused_window() {
    let request = parse_web_find_payload(
        r#"{target:{window_id:"pid:1/window:1"},match:{text:"首页"},limit:5}"#,
    )
    .unwrap();
    let value = response_value(&snapshot_with_two_unfocused_browser_windows(), &request);

    assert_eq!(value["status"], "complete");
    assert_eq!(value["scope"], "target_window_web_area");
    assert_eq!(value["target"]["window_id"], "pid:1/window:1");
    assert_eq!(value["window"]["window_id"], "pid:1/window:1");
    assert_eq!(value["match_count"], 1);
    assert_eq!(value["matches"][0]["window_id"], "pid:1/window:1");
    assert!(trace_contains(&value, "target-browser-window", "ok"));
}

#[test]
fn web_find_should_keep_active_browser_ambiguity_without_window_id() {
    let request = parse_web_find_payload(r#"{match:{text:"首页"},limit:5}"#).unwrap();
    let value = response_value(&snapshot_with_two_unfocused_browser_windows(), &request);

    assert_eq!(value["status"], "needs_disambiguation");
    assert_eq!(value["scope"], "active_web_area");
    assert_eq!(value["error_code"], "BROWSER_WINDOW_AMBIGUOUS");
    assert!(trace_contains(
        &value,
        "active-browser-window",
        "needs_disambiguation"
    ));
}

#[test]
fn web_find_should_promote_text_child_to_actionable_ancestor() {
    let request = parse_web_find_payload(r#"{match:{text:"发布"},limit:5}"#).unwrap();
    let value = response_value(&snapshot_with_nested_text(), &request);

    assert_eq!(value["status"], "complete");
    assert_eq!(value["matches"][0]["id"], "pid:1/window:0/path:0.0.0.1");
    assert_eq!(
        value["matches"][0]["matched_source_id"],
        "pid:1/window:0/path:0.0.0.1.0"
    );
    assert_eq!(value["matches"][0]["matched_field"], "name");
}

#[test]
fn web_find_should_return_structured_blocker_without_browser_window() {
    let request = parse_web_find_payload(r#"{match:{text:"首页"}}"#).unwrap();
    let snapshot = AxSnapshot::complete(
        "macos",
        vec![AxWindow {
            id: "pid:2/window:0".to_owned(),
            ref_id: None,
            pid: 2,
            process_name: "Finder".to_owned(),
            title: Some("Desktop".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: Vec::new(),
        }],
        false,
    );
    let value = response_value(&snapshot, &request);

    assert_eq!(value["status"], "not_found");
    assert_eq!(value["error_code"], "BROWSER_WINDOW_NOT_FOUND");
    assert_eq!(value["match_count"], 0);
}

#[test]
fn web_find_should_refresh_web_area_subtree_when_shallow_snapshot_misses_deep_target() {
    let request = parse_web_find_payload(
        r#"{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:5}"#,
    )
    .unwrap();
    let refresh_calls = Cell::new(0usize);

    let json = build_web_find_response_json_with_refresh(
        &snapshot_with_shallow_web_area(),
        &request,
        |target_id, tree_request| {
            refresh_calls.set(refresh_calls.get() + 1);
            assert_eq!(target_id, "pid:1/window:0/path:0.0.0.0.1.0.0.0");
            assert_eq!(tree_request.depth, DEFAULT_WEB_FIND_DEPTH);
            Ok(Some(AxCapturedSubtree {
                element: refreshed_web_area_with_deep_home_link(),
                truncated: false,
            }))
        },
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(refresh_calls.get(), 1);
    assert_eq!(value["status"], "complete");
    assert_eq!(value["match_count"], 1);
    assert_eq!(
        value["web_area"]["id"],
        "pid:1/window:0/path:0.0.0.0.1.0.0.0"
    );
    assert_eq!(
        value["matches"][0]["id"],
        "pid:1/window:0/path:0.0.0.0.1.0.0.0.0.0.0.1.1.0.0.0"
    );
    assert_eq!(value["matches"][0]["description"], "首页");
    assert_eq!(value["matches"][0]["actions"][0], "AXPress");
    assert!(trace_contains(
        &value,
        "refresh-web-area-subtree",
        "attempt"
    ));
    assert!(trace_contains(&value, "refresh-web-area-subtree", "ok"));
}

fn response_value(snapshot: &AxSnapshot, request: &WebFindRequest) -> Value {
    let json = build_web_find_response_json(snapshot, request).unwrap();
    serde_json::from_str(&json).unwrap()
}

fn record_test_observation_ref(
    ref_id: &str,
    backend_id: &str,
    kind: &str,
) -> crate::control_observation::ObservationHeader {
    record_observation(
        "window",
        "@window-find test",
        ObservationRoot {
            schema: "rdog.window.v1".to_owned(),
            platform: "macos".to_owned(),
            coordinate_space: "os-logical".to_owned(),
        },
        vec![ObservationRefEntry {
            ref_id: ref_id.to_owned(),
            backend_id: backend_id.to_owned(),
            kind: kind.to_owned(),
        }],
    )
    .unwrap()
}

fn snapshot_with_page_link() -> AxSnapshot {
    AxSnapshot::complete(
        "macos",
        vec![AxWindow {
            id: "pid:1/window:0".to_owned(),
            ref_id: None,
            pid: 1,
            process_name: "Google Chrome".to_owned(),
            title: Some("小红书 - Google Chrome".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![
                link("pid:1/window:0/path:toolbar", "Home", "AXLink"),
                AxElement {
                    id: "pid:1/window:0/path:0.0.0.0.1.0.0.0".to_owned(),
                    ref_id: None,
                    role: "AXWebArea".to_owned(),
                    subrole: None,
                    name: Some("小红书".to_owned()),
                    value: None,
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![0, 0, 0, 0, 1, 0, 0, 0],
                    children: vec![page_link("pid:1/window:0/path:0.0.0.0.1.0.0.0.0", "首页")],
                },
            ],
        }],
        false,
    )
}

fn snapshot_with_nested_text() -> AxSnapshot {
    AxSnapshot::complete(
        "macos",
        vec![AxWindow {
            id: "pid:1/window:0".to_owned(),
            ref_id: None,
            pid: 1,
            process_name: "Google Chrome".to_owned(),
            title: Some("小红书 - Google Chrome".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![AxElement {
                id: "pid:1/window:0/path:0".to_owned(),
                ref_id: None,
                role: "AXWebArea".to_owned(),
                subrole: None,
                name: None,
                value: None,
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: vec![0],
                children: vec![AxElement {
                    id: "pid:1/window:0/path:0.0.0.1".to_owned(),
                    ref_id: None,
                    role: "AXLink".to_owned(),
                    subrole: None,
                    name: None,
                    value: None,
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: vec!["AXPress".to_owned()],
                    ax_path: vec![0, 0, 0, 1],
                    children: vec![AxElement {
                        id: "pid:1/window:0/path:0.0.0.1.0".to_owned(),
                        ref_id: None,
                        role: "AXStaticText".to_owned(),
                        subrole: None,
                        name: Some("发布".to_owned()),
                        value: None,
                        value_redacted: false,
                        description: None,
                        rect: None,
                        enabled: Some(true),
                        actions: Vec::new(),
                        ax_path: vec![0, 0, 0, 1, 0],
                        children: Vec::new(),
                    }],
                }],
            }],
        }],
        false,
    )
}

fn snapshot_with_two_unfocused_browser_windows() -> AxSnapshot {
    AxSnapshot::complete(
        "macos",
        vec![
            AxWindow {
                id: "pid:1/window:0".to_owned(),
                ref_id: None,
                pid: 1,
                process_name: "Google Chrome".to_owned(),
                title: Some("文档 - Google Chrome".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: Some(false),
                elements: vec![AxElement {
                    id: "pid:1/window:0/path:0".to_owned(),
                    ref_id: None,
                    role: "AXWebArea".to_owned(),
                    subrole: None,
                    name: Some("文档".to_owned()),
                    value: None,
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![0],
                    children: vec![page_link("pid:1/window:0/path:0.0", "首页")],
                }],
            },
            AxWindow {
                id: "pid:1/window:1".to_owned(),
                ref_id: None,
                pid: 1,
                process_name: "Google Chrome".to_owned(),
                title: Some("小红书 - Google Chrome".to_owned()),
                role: "AXWindow".to_owned(),
                subrole: None,
                rect: None,
                focused: Some(false),
                elements: vec![AxElement {
                    id: "pid:1/window:1/path:0".to_owned(),
                    ref_id: None,
                    role: "AXWebArea".to_owned(),
                    subrole: None,
                    name: Some("小红书".to_owned()),
                    value: None,
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![0],
                    children: vec![page_link("pid:1/window:1/path:0.0", "首页")],
                }],
            },
        ],
        false,
    )
}

fn snapshot_with_shallow_web_area() -> AxSnapshot {
    AxSnapshot::complete(
        "macos",
        vec![AxWindow {
            id: "pid:1/window:0".to_owned(),
            ref_id: None,
            pid: 1,
            process_name: "Google Chrome".to_owned(),
            title: Some("小红书 - Google Chrome".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![AxElement {
                id: "pid:1/window:0/path:0.0.0.0.1.0.0.0".to_owned(),
                ref_id: None,
                role: "AXWebArea".to_owned(),
                subrole: None,
                name: Some("小红书".to_owned()),
                value: None,
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: Vec::new(),
                ax_path: vec![0, 0, 0, 0, 1, 0, 0, 0],
                children: Vec::new(),
            }],
        }],
        false,
    )
}

fn refreshed_web_area_with_deep_home_link() -> AxElement {
    AxElement {
        id: "pid:1/window:0/path:0.0.0.0.1.0.0.0".to_owned(),
        ref_id: None,
        role: "AXWebArea".to_owned(),
        subrole: None,
        name: Some("小红书".to_owned()),
        value: None,
        value_redacted: false,
        description: None,
        rect: None,
        enabled: Some(true),
        actions: Vec::new(),
        ax_path: vec![0, 0, 0, 0, 1, 0, 0, 0],
        children: vec![deep_home_link_container(
            0,
            "pid:1/window:0/path:0.0.0.0.1.0.0.0",
        )],
    }
}

fn deep_home_link_container(depth: usize, id_prefix: &str) -> AxElement {
    if depth == 7 {
        return page_link(
            "pid:1/window:0/path:0.0.0.0.1.0.0.0.0.0.0.1.1.0.0.0",
            "首页",
        );
    }

    let next_index = if depth == 3 || depth == 4 { 1 } else { 0 };
    let id = format!("{id_prefix}.{next_index}");
    AxElement {
        id: id.clone(),
        ref_id: None,
        role: "AXGroup".to_owned(),
        subrole: None,
        name: None,
        value: None,
        value_redacted: false,
        description: None,
        rect: None,
        enabled: Some(true),
        actions: Vec::new(),
        ax_path: vec![0],
        children: vec![deep_home_link_container(depth + 1, &id)],
    }
}

fn page_link(id: &str, description: &str) -> AxElement {
    AxElement {
        description: Some(description.to_owned()),
        ..link(id, description, "AXLink")
    }
}

fn trace_contains(value: &Value, step: &str, status: &str) -> bool {
    value["trace"]
        .as_array()
        .expect("trace should be array")
        .iter()
        .any(|entry| entry["step"] == step && entry["status"] == status)
}

fn link(id: &str, name: &str, role: &str) -> AxElement {
    AxElement {
        id: id.to_owned(),
        ref_id: None,
        role: role.to_owned(),
        subrole: None,
        name: Some(name.to_owned()),
        value: None,
        value_redacted: false,
        description: None,
        rect: Some(AxRect {
            x: 16,
            y: 274,
            width: 116,
            height: 48,
        }),
        enabled: Some(true),
        actions: vec!["AXPress".to_owned()],
        ax_path: vec![0],
        children: Vec::new(),
    }
}
