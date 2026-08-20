//! 13 个 routing 单测: 验证 `route_computer_act_action` 把 `@computer-act`
//! action 正确翻译成底层 `ControlCommand`。
//!
//! 不调用底层 executor (那是 Phase C ticket 06-10 的工作); ticket 04
//! 的范围是 routing 表 + 参数转换。

use serde_json::json;

use super::{route_computer_act_action, RoutedCommand};
use crate::control_ax::TypeTextMode;
use crate::control_mouse::{
    DragRequest, MouseButtonName, MouseCoordinateSpace, MouseEndpoint, MouseMoveRequest,
    MousePoint, MouseRefTarget, WheelRequest,
};
use crate::control_protocol::{
    ControlCommand, KeyMode, OpenAppRequest, PasteRequestKind, WaitRequest,
};

fn route(action: &str, args: serde_json::Value) -> RoutedCommand {
    route_computer_act_action(action, &args)
        .unwrap_or_else(|e| panic!("route({action}) failed: {e:?}"))
}

// --- open_app ---

#[test]
fn open_app_routes_to_open_app_with_default_wait_ms() {
    let r = route("open_app", json!({"app_name": "Calculator"}));
    assert_eq!(r.dispatched_to, "@open-app");
    match r.command {
        ControlCommand::OpenApp(OpenAppRequest { app_name, wait_ms }) => {
            assert_eq!(app_name, "Calculator");
            assert_eq!(wait_ms, 1500);
        }
        c => panic!("expected OpenApp, got {c:?}"),
    }
}

#[test]
fn open_app_routes_to_open_app_with_explicit_wait_ms() {
    let r = route("open_app", json!({"app_name": "Xcode", "wait_ms": 5000}));
    assert_eq!(r.dispatched_to, "@open-app");
    match r.command {
        ControlCommand::OpenApp(req) => {
            assert_eq!(req.app_name, "Xcode");
            assert_eq!(req.wait_ms, 5000);
        }
        c => panic!("expected OpenApp, got {c:?}"),
    }
}

// --- open_url ---

#[test]
fn open_url_routes_to_cmd_open() {
    let r = route("open_url", json!({"url": "https://example.com"}));
    assert_eq!(r.dispatched_to, "@cmd");
    match r.command {
        ControlCommand::Script(text) => {
            assert_eq!(text, "open https://example.com");
        }
        c => panic!("expected Script, got {c:?}"),
    }
}

// --- click family ---

#[test]
fn click_routes_to_click_with_count_1_and_left_button() {
    let r = route("click", json!({"start_box": [100, 200]}));
    assert_eq!(r.dispatched_to, "@click");
    match r.command {
        ControlCommand::Click(req) => {
            assert_eq!(req.count, 1);
            assert_eq!(req.button, MouseButtonName::Left);
            assert_eq!(req.hold_ms, 80);
            assert!(matches!(
                req.target,
                Some(MouseEndpoint::Coordinate(MousePoint { x: 100, y: 200 }))
            ));
        }
        c => panic!("expected Click, got {c:?}"),
    }
}

#[test]
fn doubleclick_routes_to_click_with_count_2() {
    let r = route("doubleclick", json!({"start_box": [50, 60]}));
    assert_eq!(r.dispatched_to, "@click");
    match r.command {
        ControlCommand::Click(req) => {
            assert_eq!(req.count, 2);
            assert_eq!(req.button, MouseButtonName::Left);
        }
        c => panic!("expected Click, got {c:?}"),
    }
}

#[test]
fn triple_click_routes_to_click_with_count_3() {
    let r = route("triple_click", json!({"start_box": [70, 80]}));
    assert_eq!(r.dispatched_to, "@click");
    match r.command {
        ControlCommand::Click(req) => {
            assert_eq!(req.count, 3);
            assert_eq!(req.button, MouseButtonName::Left);
        }
        c => panic!("expected Click, got {c:?}"),
    }
}

#[test]
fn right_single_routes_to_click_with_right_button() {
    let r = route("right_single", json!({"start_box": [10, 20]}));
    assert_eq!(r.dispatched_to, "@click");
    match r.command {
        ControlCommand::Click(req) => {
            assert_eq!(req.count, 1);
            assert_eq!(req.button, MouseButtonName::Right);
        }
        c => panic!("expected Click, got {c:?}"),
    }
}

#[test]
fn click_routes_ref_target_through_observation_ref() {
    let r = route(
        "click",
        json!({"target": {"ref": "@e5", "observation_id": "obs-123"}}),
    );
    match r.command {
        ControlCommand::Click(req) => match req.target {
            Some(MouseEndpoint::ObservationRef(MouseRefTarget {
                ref_id,
                observation_id,
                anchor,
            })) => {
                assert_eq!(ref_id, "@e5");
                assert_eq!(observation_id, "obs-123");
                assert!(matches!(anchor, crate::control_mouse::MouseAnchor::Center));
            }
            t => panic!("expected ObservationRef, got {t:?}"),
        },
        c => panic!("expected Click, got {c:?}"),
    }
}

// --- hover ---

#[test]
fn hover_routes_to_mouse_move() {
    let r = route("hover", json!({"start_box": [300, 400]}));
    assert_eq!(r.dispatched_to, "@mouse-move");
    match r.command {
        ControlCommand::MouseMove(MouseMoveRequest { x, y, target, .. }) => {
            assert_eq!(x, Some(300));
            assert_eq!(y, Some(400));
            assert!(matches!(
                target,
                Some(MouseEndpoint::Coordinate(MousePoint { x: 300, y: 400 }))
            ));
        }
        c => panic!("expected MouseMove, got {c:?}"),
    }
}

// --- type ---

#[test]
fn type_routes_to_paste_when_no_target() {
    let r = route("type", json!({"content": "hello world"}));
    assert_eq!(r.dispatched_to, "@type-text");
    match r.command {
        ControlCommand::Paste(req) => match req.kind {
            PasteRequestKind::LegacyTextInjection(text) => {
                assert_eq!(text, "hello world");
            }
            _ => panic!("expected LegacyTextInjection, got other kind"),
        },
        c => panic!("expected Paste, got {c:?}"),
    }
}

#[test]
fn type_with_ref_target_routes_to_ax_value_type_text() {
    let r = route(
        "type",
        json!({
            "target": {"ref": "@e3", "observation_id": "obs-1"},
            "content": "hello world"
        }),
    );
    assert_eq!(r.dispatched_to, "@type-text");
    match r.command {
        ControlCommand::TypeText(req) => {
            assert_eq!(req.target.ref_id.as_deref(), Some("@e3"));
            assert_eq!(req.target.observation_id.as_deref(), Some("obs-1"));
            assert_eq!(req.text, "hello world");
            assert_eq!(req.mode, TypeTextMode::AxValue);
            assert!(!req.allow_clipboard);
        }
        c => panic!("expected TypeText, got {c:?}"),
    }
}

#[test]
fn type_ref_target_accepts_explicit_auto_and_clipboard_gate() {
    let r = route(
        "type",
        json!({
            "target": {"ref": "@e3", "observation_id": "obs-1"},
            "content": "hello world",
            "mode": "auto",
            "allow_clipboard": true
        }),
    );
    match r.command {
        ControlCommand::TypeText(req) => {
            assert_eq!(req.mode, TypeTextMode::Auto);
            assert!(req.allow_clipboard);
        }
        c => panic!("expected TypeText, got {c:?}"),
    }

    let error = match route_computer_act_action(
        "type",
        &json!({
            "target": {"ref": "@e3", "observation_id": "obs-1"},
            "content": "hello world",
            "mode": "clipboard"
        }),
    ) {
        Err(error) => error,
        Ok(_) => panic!("clipboard must require explicit allow_clipboard"),
    };
    assert!(matches!(
        error,
        super::ComputerActRouteError::InvalidArgs(_)
    ));
}

#[test]
fn type_ref_target_rejects_semantic_target_fields() {
    let error = match route_computer_act_action(
        "type",
        &json!({
            "target": {"ref": "@e3", "observation_id": "obs-1", "role": "AXTextArea"},
            "content": "hello world"
        }),
    ) {
        Err(error) => error,
        Ok(_) => panic!("ref-backed target must not mix semantic fields"),
    };
    assert!(matches!(
        error,
        super::ComputerActRouteError::InvalidArgs(_)
    ));
}

// --- hotkey ---

#[test]
fn hotkey_routes_to_key() {
    let r = route("hotkey", json!({"key": "Cmd+C"}));
    assert_eq!(r.dispatched_to, "@key");
    match r.command {
        ControlCommand::Key(req) => {
            assert_eq!(req.key, "Cmd+C");
            assert!(matches!(req.mode, KeyMode::PressRelease));
            assert_eq!(req.hold_ms, 200);
        }
        c => panic!("expected Key, got {c:?}"),
    }
}

// --- hotkey_click (composite) ---

#[test]
fn hotkey_click_routes_to_composite_3_steps() {
    // ticket 08 + 21: hotkey_click 实现为 ControlCommand::Composite([key down, click, key up])
    let r = route(
        "hotkey_click",
        json!({"start_box": [10, 20], "key": "shift"}),
    );
    assert_eq!(r.dispatched_to, "@key+@click+@key");
    match r.command {
        ControlCommand::Composite(cmds) => {
            assert_eq!(cmds.len(), 3, "composite 应有 3 步");
            // step 1: key down (Press)
            match &cmds[0] {
                ControlCommand::Key(kr) => {
                    assert_eq!(kr.key, "shift");
                    assert!(matches!(kr.mode, KeyMode::Press));
                }
                c => panic!("step 0 expected Key(Press), got {c:?}"),
            }
            // step 2: click (10, 20)
            match &cmds[1] {
                ControlCommand::Click(req) => {
                    assert_eq!(req.x, Some(10));
                    assert_eq!(req.y, Some(20));
                    assert_eq!(req.count, 1);
                }
                c => panic!("step 1 expected Click, got {c:?}"),
            }
            // step 3: key up (Release)
            match &cmds[2] {
                ControlCommand::Key(kr) => {
                    assert_eq!(kr.key, "shift");
                    assert!(matches!(kr.mode, KeyMode::Release));
                }
                c => panic!("step 2 expected Key(Release), got {c:?}"),
            }
        }
        c => panic!("expected Composite, got {c:?}"),
    }
}

// --- scroll ---

#[test]
fn scroll_routes_to_wheel_with_negative_delta_y_for_down() {
    let r = route(
        "scroll",
        json!({"start_box": [100, 200], "direction": "down", "amount": 3}),
    );
    assert_eq!(r.dispatched_to, "@wheel");
    match r.command {
        ControlCommand::Wheel(WheelRequest {
            delta_x,
            delta_y,
            x,
            y,
            coordinate_space,
            ..
        }) => {
            assert_eq!(x, Some(100));
            assert_eq!(y, Some(200));
            assert_eq!(delta_x, 0);
            assert_eq!(delta_y, -3);
            assert_eq!(coordinate_space, MouseCoordinateSpace::OsLogical);
        }
        c => panic!("expected Wheel, got {c:?}"),
    }
}

// --- drag ---

#[test]
fn drag_routes_to_drag_with_from_to() {
    let r = route(
        "drag",
        json!({"start_box": [100, 200], "end_box": [400, 500]}),
    );
    assert_eq!(r.dispatched_to, "@drag");
    match r.command {
        ControlCommand::Drag(DragRequest {
            from,
            to,
            duration_ms,
            steps,
            ..
        }) => {
            assert!(matches!(
                from,
                MouseEndpoint::Coordinate(MousePoint { x: 100, y: 200 })
            ));
            assert!(matches!(
                to,
                MouseEndpoint::Coordinate(MousePoint { x: 400, y: 500 })
            ));
            assert_eq!(duration_ms, 450);
            assert_eq!(steps, 24);
        }
        c => panic!("expected Drag, got {c:?}"),
    }
}

// --- wait ---

#[test]
fn wait_routes_to_wait() {
    let r = route("wait", json!({"duration_ms": 200}));
    assert_eq!(r.dispatched_to, "@wait");
    match r.command {
        ControlCommand::Wait(WaitRequest { duration_ms }) => {
            assert_eq!(duration_ms, 200);
        }
        c => panic!("expected Wait, got {c:?}"),
    }
}

// --- unknown action ---

#[test]
fn unknown_action_returns_error() {
    let result = route_computer_act_action("teleport", &json!({}));
    assert!(matches!(
        result,
        Err(super::ComputerActRouteError::UnknownAction(_))
    ));
}

use super::{
    append_post_dispatch_evidence, check_observation_epoch_fast_reject,
    dispatch_with_resource_epoch, execute_computer_act, PostDispatchStatus,
};
use crate::control_ax::{AxElement, AxSnapshot, AxWindow};
use crate::control_observation::{
    record_observation, record_observation_with_selectors_from_capture, resolve_observation_header,
    resolve_observation_resource_epoch, ObservationRefEntry, ObservationRoot,
};
use crate::control_protocol::{
    ComputerActPostcondition, ComputerActPostconditionKind, ComputerActRequest,
};
use crate::control_resource_lane::{capture_resource_epochs, with_resource_write};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};

fn make_request(observation_id: Option<&str>, epoch: Option<u64>) -> ComputerActRequest {
    ComputerActRequest {
        schema: "rdog.computer-act.v1".to_string(),
        action: "wait".to_string(),
        args: json!({"duration_ms": 100}),
        verify: None,
        postcondition: None,
        observation_id: observation_id.map(str::to_owned),
        timeout_ms: None,
        trace: None,
        epoch,
    }
}

fn record_observation_at(_now_ms: u64, refs: Vec<ObservationRefEntry>) -> String {
    let root = ObservationRoot {
        schema: "rdog.observation.root.v1".to_string(),
        platform: "test".to_string(),
        coordinate_space: "os-logical".to_string(),
    };
    let header = record_observation("test", "@computer-act", root, refs)
        .expect("record_observation should succeed");
    header.observation_id
}

#[test]
fn epoch_check_returns_none_when_epoch_not_provided() {
    // 没传 epoch: 走原路径, fast-reject 钩子 no-op
    let request = make_request(Some("obs-123"), None);
    assert!(check_observation_epoch_fast_reject(&request).is_none());
}

#[test]
fn epoch_check_returns_none_when_observation_id_missing() {
    // 传了 epoch 但没 observation_id: 没有验证依据, no-op
    let request = make_request(None, Some(1700000000000));
    assert!(check_observation_epoch_fast_reject(&request).is_none());
}

#[test]
fn epoch_check_passes_when_epoch_matches_header() {
    // 记录 observation, epoch 用真实 created_at_unix_ms: 应该通过
    let observation_id = record_observation_at(
        1700000001000,
        vec![ObservationRefEntry {
            ref_id: "@e1".to_string(),
            backend_id: "pid:1/path:0".to_string(),
            kind: "ax".to_string(),
        }],
    );
    // created_at_unix_ms 由 global store 用 current_unix_ms 自动算, 不能直接控制.
    // 通过先 record 再读 header 拿真实 epoch, 然后用真实 epoch 校验.
    let header = crate::control_observation::resolve_observation_header(&observation_id)
        .expect("header should resolve");
    let request = make_request(Some(&observation_id), Some(header.created_at_unix_ms));
    assert!(
        check_observation_epoch_fast_reject(&request).is_none(),
        "matching epoch should pass through"
    );
}

#[test]
fn epoch_check_rejects_when_epoch_mismatches() {
    let observation_id = record_observation_at(
        1700000002000,
        vec![ObservationRefEntry {
            ref_id: "@e1".to_string(),
            backend_id: "pid:1/path:0".to_string(),
            kind: "ax".to_string(),
        }],
    );
    // 用错 epoch: 应该 fast-reject, 错误 envelope, exit_code 64
    let request = make_request(Some(&observation_id), Some(42));
    let result = check_observation_epoch_fast_reject(&request)
        .expect("mismatched epoch must produce envelope");
    assert_eq!(result.exit_code, 64);
    let envelope: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("envelope is JSON"),
    )
    .expect("envelope should be JSON");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error_code"], "stale_observation_epoch");
    assert_eq!(envelope["retry"]["strategy"], "re_observe_then_retry");
    assert_eq!(
        envelope["evidence"]["presented_epoch"],
        serde_json::Value::Number(42u64.into())
    );
    assert_eq!(
        envelope["evidence"]["observation_id"],
        serde_json::Value::String(observation_id.clone())
    );
    assert!(
        envelope["evidence"]["current_epoch"].is_number(),
        "current_epoch 应该被写入 evidence, 实际: {}",
        envelope["evidence"]
    );
}

#[test]
fn epoch_check_rejects_when_observation_absent() {
    // observation_id 不存在 -> resolve_observation_header 返回 OBSERVATION_EXPIRED
    let request = make_request(Some("obs-does-not-exist-9999"), Some(1700000003000));
    let result = check_observation_epoch_fast_reject(&request)
        .expect("missing observation must produce envelope");
    assert_eq!(result.exit_code, 64);
    let envelope: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("envelope is JSON"),
    )
    .expect("envelope should be JSON");
    assert_eq!(envelope["error_code"], "stale_observation_epoch");
    assert_eq!(
        envelope["evidence"]["observation_id"],
        serde_json::Value::String("obs-does-not-exist-9999".to_string())
    );
    assert!(
        envelope["evidence"].get("current_epoch").is_none(),
        "observation 不存在时不应有 current_epoch 字段"
    );
}

#[test]
fn same_observation_pid_allows_only_one_concurrent_mutation() {
    let observation_id = record_observation_at(
        0,
        vec![ObservationRefEntry {
            ref_id: "@e-resource-lane".to_string(),
            backend_id: "pid:910001/window:0/path:1".to_string(),
            kind: "ax".to_string(),
        }],
    );
    let snapshot = resolve_observation_resource_epoch(&observation_id, "@e-resource-lane")
        .expect("resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");
    let start = Arc::new(Barrier::new(2));
    let dispatched = Arc::new(AtomicUsize::new(0));

    let spawn = |start: Arc<Barrier>, dispatched: Arc<AtomicUsize>| {
        let snapshot = snapshot.clone();
        std::thread::spawn(move || {
            start.wait();
            dispatch_with_resource_epoch(Some(&snapshot), || {
                dispatched.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(20));
                Ok(crate::control_actions::ActionExecutionResult {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    response_value_json: Some("{}".to_string()),
                })
            })
            .expect("resource dispatch should return a response")
        })
    };
    let first = spawn(start.clone(), dispatched.clone());
    let second = spawn(start, dispatched.clone());
    let results = [
        first.join().expect("first mutation thread should finish"),
        second.join().expect("second mutation thread should finish"),
    ];

    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| result.exit_code == 0)
            .count(),
        1
    );
    let stale = results
        .iter()
        .find(|result| result.exit_code == 64)
        .expect("one mutation should be rejected as stale");
    let envelope: serde_json::Value = serde_json::from_str(
        stale
            .response_value_json
            .as_deref()
            .expect("stale response should contain JSON"),
    )
    .expect("stale response should be valid JSON");
    assert_eq!(envelope["error_code"], "stale_resource_epoch");
    assert_eq!(envelope["evidence"]["resource_key"], "pid:910001");
}

#[test]
fn new_observation_captures_incremented_resource_epoch() {
    let first_observation = record_observation_at(
        0,
        vec![ObservationRefEntry {
            ref_id: "@e-before-write".to_string(),
            backend_id: "pid:910002/window:0/path:1".to_string(),
            kind: "ax".to_string(),
        }],
    );
    let before = resolve_observation_resource_epoch(&first_observation, "@e-before-write")
        .expect("first resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");
    dispatch_with_resource_epoch(Some(&before), || {
        Ok(crate::control_actions::ActionExecutionResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some("{}".to_string()),
        })
    })
    .expect("first mutation should dispatch");

    let second_observation = record_observation_at(
        0,
        vec![ObservationRefEntry {
            ref_id: "@e-after-write".to_string(),
            backend_id: "pid:910002/window:0/path:2".to_string(),
            kind: "ax".to_string(),
        }],
    );
    let after = resolve_observation_resource_epoch(&second_observation, "@e-after-write")
        .expect("second resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");

    assert_eq!(after.epoch, before.epoch + 2);
}

#[test]
fn successor_observation_can_drive_next_same_pid_mutation() {
    let old_observation = record_observation_at(
        0,
        vec![ObservationRefEntry {
            ref_id: "@e-old".to_string(),
            backend_id: "pid:910005/window:0/path:1".to_string(),
            kind: "ax".to_string(),
        }],
    );
    let old_epoch = resolve_observation_resource_epoch(&old_observation, "@e-old")
        .expect("old resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");
    dispatch_with_resource_epoch(Some(&old_epoch), || {
        Ok(crate::control_actions::ActionExecutionResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some("{}".to_string()),
        })
    })
    .expect("first mutation should dispatch");

    let successor = AxSnapshot::complete(
        "test",
        vec![AxWindow {
            id: "pid:910005/window:0".to_string(),
            ref_id: None,
            pid: 910005,
            process_name: "fixture".to_string(),
            title: Some("fixture".to_string()),
            role: "AXWindow".to_string(),
            subrole: None,
            rect: None,
            focused: Some(true),
            elements: vec![AxElement {
                id: "pid:910005/window:0/path:2".to_string(),
                ref_id: None,
                role: "AXButton".to_string(),
                subrole: None,
                name: Some("Next".to_string()),
                value: None,
                value_redacted: false,
                description: None,
                rect: None,
                enabled: Some(true),
                actions: vec!["AXPress".to_string()],
                ax_path: vec![2],
                children: Vec::new(),
            }],
        }],
        false,
    )
    .with_observation("@computer-act")
    .expect("successor observation should be recorded");
    let header = successor
        .observation
        .expect("successor should contain observation header");
    let successor_epoch = resolve_observation_resource_epoch(&header.observation_id, "@e2")
        .expect("successor resource snapshot should resolve")
        .expect("successor element should have PID resource snapshot");

    assert_eq!(successor_epoch.epoch, old_epoch.epoch + 2);
    assert!(with_resource_write(&old_epoch, || -> std::io::Result<()> {
        panic!("old observation must remain stale")
    })
    .is_err());
    assert!(with_resource_write(&successor_epoch, || Ok(())).is_ok());
}

#[test]
fn successor_target_uses_the_new_snapshot_ref_for_the_same_backend() {
    let successor = AxSnapshot::complete(
        "test",
        vec![
            AxWindow {
                id: "pid:910006/window:0".to_string(),
                ref_id: None,
                pid: 910006,
                process_name: "other".to_string(),
                title: Some("other".to_string()),
                role: "AXWindow".to_string(),
                subrole: None,
                rect: None,
                focused: Some(false),
                elements: vec![AxElement {
                    id: "pid:910006/window:0/path:0".to_string(),
                    ref_id: None,
                    role: "AXButton".to_string(),
                    subrole: None,
                    name: Some("Other".to_string()),
                    value: None,
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: vec!["AXPress".to_string()],
                    ax_path: vec![0],
                    children: Vec::new(),
                }],
            },
            AxWindow {
                id: "pid:910007/window:0".to_string(),
                ref_id: None,
                pid: 910007,
                process_name: "target".to_string(),
                title: Some("target".to_string()),
                role: "AXWindow".to_string(),
                subrole: None,
                rect: None,
                focused: Some(true),
                elements: vec![AxElement {
                    id: "pid:910007/window:0/path:0".to_string(),
                    ref_id: None,
                    role: "AXTextArea".to_string(),
                    subrole: None,
                    name: None,
                    value: Some(String::new()),
                    value_redacted: false,
                    description: None,
                    rect: None,
                    enabled: Some(true),
                    actions: Vec::new(),
                    ax_path: vec![0],
                    children: Vec::new(),
                }],
            },
        ],
        false,
    )
    .with_observation("@computer-act")
    .expect("successor observation should be recorded");

    let target = super::verify::build_successor_target(&successor, "pid:910007/window:0/path:0")
        .expect("successor should expose the exact target ref");

    assert_eq!(target["ref"], "@e4");
    assert_eq!(
        target["observation_id"],
        successor
            .observation
            .as_ref()
            .expect("successor observation 应存在")
            .observation_id
    );
    let resource_epoch = resolve_observation_resource_epoch(
        target["observation_id"]
            .as_str()
            .expect("successor observation id 应为字符串"),
        target["ref"]
            .as_str()
            .expect("successor target ref 应为字符串"),
    )
    .expect("successor resource epoch 应可解析")
    .expect("PID-backed successor 必须携带 resource epoch");
    assert_eq!(target["epoch"], resource_epoch.epoch);
}

#[test]
fn post_dispatch_response_marks_missing_successor_unknown_without_fabrication() {
    let pre = AxSnapshot::complete("test", Vec::new(), false)
        .with_observation("@computer-act")
        .expect("pre observation 应创建成功");
    let changes = crate::ax_diff::changes_first::decide_snapshot_changes(Some(&pre), None);
    let request = ComputerActRequest {
        schema: "rdog.computer-act.v1".to_string(),
        action: "click".to_string(),
        args: json!({}),
        verify: None,
        postcondition: Some(ComputerActPostcondition {
            kind: ComputerActPostconditionKind::Exists,
            query: crate::control_ax::AxFindQuery {
                role: Some("AXButton".to_string()),
                ..Default::default()
            },
        }),
        observation_id: None,
        timeout_ms: None,
        trace: None,
        epoch: None,
    };
    let mut payload = json!({"ok": true});

    append_post_dispatch_evidence(
        &mut payload,
        &request,
        None,
        None,
        Some(&changes),
        PostDispatchStatus {
            dispatch_ok: true,
            successor_required: true,
            verify_policy: super::VerifyPolicy::None,
            verify_ran: false,
            verification_passed: false,
        },
    );

    assert_eq!(payload["changes"]["status"], "unavailable");
    assert_eq!(
        payload["changes"]["base_observation_id"],
        pre.observation
            .as_ref()
            .expect("pre observation 应存在")
            .observation_id
    );
    assert_eq!(payload["postcondition"]["status"], "unavailable");
    assert_eq!(payload["outcome"], "unknown");
    assert!(payload.get("successor_observation").is_none());
    assert!(payload.get("successor_target").is_none());
}

#[test]
fn observation_keeps_capture_start_epoch_when_write_finishes_during_capture() {
    let resource_key = "pid:910003";
    let capture = capture_resource_epochs();
    let capture_snapshot = capture.snapshot(resource_key);

    with_resource_write(&capture_snapshot, || Ok(()))
        .expect("capture-start snapshot should still be current")
        .expect("injected mutation should succeed");

    let header = record_observation_with_selectors_from_capture(
        "ax",
        "@ax-tree",
        ObservationRoot {
            schema: "rdog.ax.v1".to_string(),
            platform: "test".to_string(),
            coordinate_space: "os-logical".to_string(),
        },
        vec![ObservationRefEntry {
            ref_id: "@e-capture-start".to_string(),
            backend_id: format!("{resource_key}/window:0/path:1"),
            kind: "ax".to_string(),
        }],
        Vec::new(),
        &capture,
    )
    .expect("observation should record from capture-start token");
    let recorded = resolve_observation_resource_epoch(&header.observation_id, "@e-capture-start")
        .expect("recorded resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");

    assert_eq!(recorded, capture_snapshot);
    assert!(with_resource_write(&recorded, || -> std::io::Result<()> {
        panic!("old captured UI must not dispatch after a concurrent mutation")
    })
    .is_err());
}

#[test]
fn stale_resource_epoch_top_level_response_preserves_retry_contract() {
    let observation_id = record_observation_at(
        0,
        vec![ObservationRefEntry {
            ref_id: "@e-top-level-stale".to_string(),
            backend_id: "pid:910004/window:0/path:1".to_string(),
            kind: "ax".to_string(),
        }],
    );
    let snapshot = resolve_observation_resource_epoch(&observation_id, "@e-top-level-stale")
        .expect("resource snapshot should resolve")
        .expect("PID ref should have resource snapshot");
    with_resource_write(&snapshot, || Ok(()))
        .expect("first mutation should consume the epoch")
        .expect("first mutation should succeed");
    let header = resolve_observation_header(&observation_id).expect("header should resolve");
    let request = ComputerActRequest {
        schema: "rdog.computer-act.v1".to_string(),
        action: "click".to_string(),
        args: json!({
            "target": {
                "ref": "@e-top-level-stale",
                "observation_id": observation_id,
            }
        }),
        verify: None,
        postcondition: None,
        observation_id: Some(header.observation_id),
        timeout_ms: None,
        trace: None,
        epoch: Some(header.created_at_unix_ms),
    };

    let result = execute_computer_act(&request, None).expect("stale response should be returned");
    let envelope: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("top-level response should contain JSON"),
    )
    .expect("top-level response should be valid JSON");

    assert_eq!(result.exit_code, 64);
    assert_eq!(envelope["error_code"], "stale_resource_epoch");
    assert_eq!(envelope["retry"]["strategy"], "re_observe_then_retry");
}
