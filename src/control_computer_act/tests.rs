//! 13 个 routing 单测: 验证 `route_computer_act_action` 把 `@computer-act`
//! action 正确翻译成底层 `ControlCommand`。
//!
//! 不调用底层 executor (那是 Phase C ticket 06-10 的工作); ticket 04
//! 的范围是 routing 表 + 参数转换。

use serde_json::json;

use super::{route_computer_act_action, RoutedCommand};
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

use super::check_observation_epoch_fast_reject;
use crate::control_observation::{
    record_observation, ObservationRoot, ObservationRefEntry,
};
use crate::control_protocol::ComputerActRequest;

fn make_request(observation_id: Option<&str>, epoch: Option<u64>) -> ComputerActRequest {
    ComputerActRequest {
        schema: "rdog.computer-act.v1".to_string(),
        action: "wait".to_string(),
        args: json!({"duration_ms": 100}),
        verify: None,
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
        result.response_value_json.as_deref().expect("envelope is JSON"),
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
        result.response_value_json.as_deref().expect("envelope is JSON"),
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
