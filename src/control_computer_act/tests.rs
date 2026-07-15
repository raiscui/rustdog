//! 13 个 routing 单测: 验证 `route_computer_act_action` 把 `@computer-act`
//! action 正确翻译成底层 `ControlCommand`。
//!
//! 不调用底层 executor (那是 Phase C ticket 06-10 的工作); ticket 04
//! 的范围是 routing 表 + 参数转换。

use serde_json::json;

use super::{route_computer_act_action, RoutedCommand};
use crate::control_protocol::{
    ControlCommand, KeyMode, OpenAppRequest, PasteRequestKind, WaitRequest,
};
use crate::control_mouse::{
    DragRequest, MouseButtonName, MouseCoordinateSpace, MouseEndpoint, MousePoint,
    MouseMoveRequest, MouseRefTarget, WheelRequest,
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
            assert!(matches!(req.target, Some(MouseEndpoint::Coordinate(MousePoint { x: 100, y: 200 }))));
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
            assert!(matches!(target, Some(MouseEndpoint::Coordinate(MousePoint { x: 300, y: 400 }))));
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
        ControlCommand::Paste(req) => {
            match req.kind {
                PasteRequestKind::LegacyTextInjection(text) => {
                    assert_eq!(text, "hello world");
                }
                _ => panic!("expected LegacyTextInjection, got other kind"),
            }
        }
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
fn hotkey_click_routes_to_script_composite() {
    let r = route("hotkey_click", json!({"start_box": [10, 20], "key": "shift"}));
    assert_eq!(r.dispatched_to, "@key+@click+@key");
    match r.command {
        ControlCommand::Script(text) => {
            assert_eq!(text, "key down shift; click 10 20; key up shift");
        }
        c => panic!("expected Script, got {c:?}"),
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
        ControlCommand::Wheel(WheelRequest { delta_x, delta_y, x, y, coordinate_space, .. }) => {
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
        ControlCommand::Drag(DragRequest { from, to, duration_ms, steps, .. }) => {
            assert!(matches!(from, MouseEndpoint::Coordinate(MousePoint { x: 100, y: 200 })));
            assert!(matches!(to, MouseEndpoint::Coordinate(MousePoint { x: 400, y: 500 })));
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
