use super::*;
use serde_json::Value;
use std::cell::{Cell, RefCell};

#[test]
fn web_act_should_inherit_display_scope_from_find_request() {
    let request = parse_web_act_payload(
        r#"{match:{text:"首页"},scope:{display:{id:"d2"}},action:"press",verify:true}"#,
    )
    .unwrap();

    assert!(request.find.display_scope.is_some());
    assert!(
        parse_web_act_payload(r#"{match:{text:"首页"},display_id:"d2",action:"press"}"#)
            .unwrap_err()
            .to_string()
            .contains("scope")
    );
}

#[test]
fn web_act_should_press_unique_page_target_and_verify_with_fresh_ax() {
    let request = parse_web_act_payload(
        r#"{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true,limit:5}"#,
    )
    .unwrap();
    let action_calls = RefCell::new(Vec::new());

    let json = build_web_act_response_json_with(
        &snapshot_with_page_link("首页", "pid:1/window:0/path:web.0", true),
        &request,
        |action| {
            action_calls.borrow_mut().push(action.clone());
            Ok(AxPerformedActionReport::success(
                "test",
                action.target.id.clone(),
                action.action,
            ))
        },
        || {
            Ok(snapshot_with_page_link(
                "首页",
                "pid:1/window:0/path:web.0",
                true,
            ))
        },
        no_web_area_refresh,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["kind"], "web-act");
    assert_eq!(value["schema"], WEB_ACT_SCHEMA);
    assert_eq!(value["status"], "complete");
    assert_eq!(value["performed"], true);
    assert_eq!(value["verified"], true);
    assert_eq!(value["selected_match"]["description"], "首页");
    assert_eq!(value["action_result"]["action"], "AXPress");
    assert_eq!(value["verification"]["status"], "matched");
    assert_eq!(value["verification"]["same_target_id"], true);

    let calls = action_calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].action, AxActionName::Press);
    assert_eq!(
        calls[0].target.id.as_deref(),
        Some("pid:1/window:0/path:web.0")
    );
}

#[test]
fn web_act_should_verify_with_refreshed_web_area_subtree_before_full_snapshot() {
    let request =
        parse_web_act_payload(r#"{match:{text:"首页"},action:"press",verify:true}"#).unwrap();
    let verify_snapshot_calls = Cell::new(0usize);
    let refresh_calls = Cell::new(0usize);

    let json = build_web_act_response_json_with(
        &snapshot_with_page_link("首页", "pid:1/window:0/path:web.0", true),
        &request,
        |action| {
            Ok(AxPerformedActionReport::success(
                "test",
                action.target.id.clone(),
                action.action,
            ))
        },
        || {
            verify_snapshot_calls.set(verify_snapshot_calls.get() + 1);
            Ok(snapshot_with_page_link(
                "首页",
                "pid:1/window:0/path:web.0",
                true,
            ))
        },
        |target_id, _tree_request| {
            refresh_calls.set(refresh_calls.get() + 1);
            assert_eq!(target_id, "pid:1/window:0/path:web");
            Ok(Some(AxCapturedSubtree {
                element: web_area_with_page_link("首页", "pid:1/window:0/path:web.0", true),
                truncated: false,
            }))
        },
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["status"], "complete");
    assert_eq!(value["performed"], true);
    assert_eq!(value["verified"], true);
    assert_eq!(value["verification"]["status"], "matched");
    assert_eq!(value["verification"]["same_target_id"], true);
    assert_eq!(value["verification"]["observation"], Value::Null);
    assert_eq!(refresh_calls.get(), 1);
    assert_eq!(verify_snapshot_calls.get(), 0);
    assert!(trace_contains(&value, "verify", "matched"));
}

#[test]
fn web_act_should_not_act_when_match_is_ambiguous() {
    let request =
        parse_web_act_payload(r#"{match:{text:"首页"},action:"press",verify:true}"#).unwrap();
    let action_calls = RefCell::new(0usize);
    let json = build_web_act_response_json_with(
        &snapshot_with_duplicate_links(),
        &request,
        |_action| {
            *action_calls.borrow_mut() += 1;
            Ok(AxPerformedActionReport::success(
                "test",
                Some("unexpected".to_owned()),
                AxActionName::Press,
            ))
        },
        || Ok(snapshot_with_duplicate_links()),
        no_web_area_refresh,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["status"], "needs_disambiguation");
    assert_eq!(value["performed"], false);
    assert_eq!(value["error_code"], "WEB_MATCH_AMBIGUOUS");
    assert_eq!(value["match_count"], 2);
    assert_eq!(*action_calls.borrow(), 0);
}

#[test]
fn web_act_should_block_when_axpress_is_unavailable() {
    let request =
        parse_web_act_payload(r#"{match:{text:"首页"},action:"press",verify:true}"#).unwrap();
    let action_calls = RefCell::new(0usize);
    let json = build_web_act_response_json_with(
        &snapshot_with_page_link("首页", "pid:1/window:0/path:web.0", false),
        &request,
        |_action| {
            *action_calls.borrow_mut() += 1;
            Ok(AxPerformedActionReport::success(
                "test",
                Some("unexpected".to_owned()),
                AxActionName::Press,
            ))
        },
        || {
            Ok(snapshot_with_page_link(
                "首页",
                "pid:1/window:0/path:web.0",
                false,
            ))
        },
        no_web_area_refresh,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["status"], "blocked");
    assert_eq!(value["performed"], false);
    assert_eq!(value["error_code"], "WEB_ACTION_UNAVAILABLE");
    assert_eq!(*action_calls.borrow(), 0);
}

#[test]
fn web_act_should_report_verification_failure_after_action() {
    let request =
        parse_web_act_payload(r#"{match:{text:"首页"},action:"press",verify:true}"#).unwrap();
    let json = build_web_act_response_json_with(
        &snapshot_with_page_link("首页", "pid:1/window:0/path:web.0", true),
        &request,
        |action| {
            Ok(AxPerformedActionReport::success(
                "test",
                action.target.id.clone(),
                action.action,
            ))
        },
        || {
            Ok(snapshot_with_page_link(
                "发现",
                "pid:1/window:0/path:web.1",
                true,
            ))
        },
        no_web_area_refresh,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["status"], "verification_failed");
    assert_eq!(value["performed"], true);
    assert_eq!(value["verified"], false);
    assert_eq!(value["error_code"], "WEB_ACTION_VERIFICATION_FAILED");
    assert_eq!(value["verification"]["status"], "not_found");
}

#[test]
fn web_act_should_refind_once_when_first_action_target_is_stale() {
    let request =
        parse_web_act_payload(r#"{match:{text:"首页"},action:"press",verify:true}"#).unwrap();
    let action_calls = RefCell::new(Vec::new());
    let attempt = Cell::new(0usize);

    let json = build_web_act_response_json_with(
        &snapshot_with_page_link("首页", "pid:1/window:0/path:stale", true),
        &request,
        |action| {
            action_calls.borrow_mut().push(action.clone());
            if attempt.get() == 0 {
                attempt.set(1);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "AX target id 已失效或不存在",
                ));
            }
            Ok(AxPerformedActionReport::success(
                "test",
                action.target.id.clone(),
                action.action,
            ))
        },
        || {
            Ok(snapshot_with_page_link(
                "首页",
                "pid:1/window:0/path:fresh",
                true,
            ))
        },
        no_web_area_refresh,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["status"], "complete");
    assert_eq!(value["performed"], true);
    assert_eq!(value["verified"], true);
    assert_eq!(value["selected_match"]["id"], "pid:1/window:0/path:fresh");
    assert_eq!(
        value["action_result"]["target_id"],
        "pid:1/window:0/path:fresh"
    );

    let calls = action_calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].target.id.as_deref(),
        Some("pid:1/window:0/path:stale")
    );
    assert_eq!(
        calls[1].target.id.as_deref(),
        Some("pid:1/window:0/path:fresh")
    );
}

fn snapshot_with_duplicate_links() -> AxSnapshot {
    let mut snapshot = snapshot_with_page_link("首页", "pid:1/window:0/path:web.0", true);
    snapshot.windows[0].elements[0].children.push(page_link(
        "pid:1/window:0/path:web.1",
        "首页",
        true,
    ));
    snapshot
}

fn snapshot_with_page_link(text: &str, id: &str, pressable: bool) -> AxSnapshot {
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
            elements: vec![web_area_with_page_link(text, id, pressable)],
        }],
        false,
    )
}

fn web_area_with_page_link(text: &str, id: &str, pressable: bool) -> AxElement {
    AxElement {
        id: "pid:1/window:0/path:web".to_owned(),
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
        children: vec![page_link(id, text, pressable)],
    }
}

fn page_link(id: &str, description: &str, pressable: bool) -> AxElement {
    AxElement {
        id: id.to_owned(),
        ref_id: None,
        role: "AXLink".to_owned(),
        subrole: None,
        name: Some(description.to_owned()),
        value: None,
        value_redacted: false,
        description: Some(description.to_owned()),
        rect: Some(AxRect {
            x: 16,
            y: 274,
            width: 116,
            height: 48,
        }),
        enabled: Some(true),
        actions: pressable
            .then(|| "AXPress".to_owned())
            .into_iter()
            .collect(),
        ax_path: vec![0],
        children: Vec::new(),
    }
}

fn no_web_area_refresh(
    _target_id: &str,
    _request: &AxTreeRequest,
) -> io::Result<Option<AxCapturedSubtree>> {
    Ok(None)
}

fn trace_contains(value: &Value, step: &str, status: &str) -> bool {
    value["trace"]
        .as_array()
        .expect("trace should be array")
        .iter()
        .any(|entry| entry["step"] == step && entry["status"] == status)
}
