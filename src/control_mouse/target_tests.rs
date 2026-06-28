use super::{
    request::{
        ClickRequest, DragRequest, MouseAnchor, MouseButtonName, MouseCoordinateSpace,
        MouseEndpoint, MousePoint, MouseRefTarget, MouseSelectorTarget,
    },
    target::{prepare_click_request, prepare_drag_request, PreparedMouseRequest},
};
use crate::control_observation::{
    record_observation, ObservationRefEntry, ObservationRoot, SelectorRefindPolicy,
};
use serde_json::Value;
use std::io;

#[test]
fn coordinate_click_should_prepare_coordinate_fallback_resolution() {
    let prepared = prepare_click_request(&ClickRequest {
        x: Some(10),
        y: Some(20),
        target: None,
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect("coordinate click should prepare");

    let PreparedMouseRequest::Ready {
        request,
        target_resolution: Some(resolution),
    } = prepared
    else {
        panic!("coordinate click should be ready");
    };
    assert_eq!(request.x, Some(10));
    assert_eq!(request.y, Some(20));
    assert_eq!(resolution["source"].as_str(), Some("coordinate_fallback"));
    assert_eq!(resolution["point"]["x"].as_i64(), Some(10));
}

#[test]
fn selector_without_auto_refind_should_return_no_action_handoff() {
    let prepared = prepare_click_request(&ClickRequest {
        x: None,
        y: None,
        target: Some(MouseEndpoint::Selector(MouseSelectorTarget {
            selector_id: "sel-v1-test".to_owned(),
            auto_refind: false,
            policy: SelectorRefindPolicy::Safe,
            min_confidence_milli: 900,
            anchor: MouseAnchor::Center,
        })),
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect("selector handoff should prepare");

    let PreparedMouseRequest::NoAction {
        response_value_json,
    } = prepared
    else {
        panic!("selector without auto_refind should not be ready");
    };
    let value: Value =
        serde_json::from_str(&response_value_json).expect("handoff response should parse");
    assert_eq!(value["performed"].as_bool(), Some(false));
    assert_eq!(
        value["target_resolution"]["gate_decision"].as_str(),
        Some("handoff_required")
    );
    assert!(value["recovery_command"]
        .as_str()
        .expect("recovery command should exist")
        .starts_with("@selector-refind:"));
}

#[test]
fn selector_auto_refind_without_match_should_still_return_no_action() {
    let prepared = prepare_click_request(&ClickRequest {
        x: None,
        y: None,
        target: Some(MouseEndpoint::Selector(MouseSelectorTarget {
            selector_id: "sel-v1-missing".to_owned(),
            auto_refind: true,
            policy: SelectorRefindPolicy::Safe,
            min_confidence_milli: 900,
            anchor: MouseAnchor::Center,
        })),
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect("missing selector refind should be reported as no-action");

    let PreparedMouseRequest::NoAction {
        response_value_json,
    } = prepared
    else {
        panic!("missing selector should not be ready");
    };
    let value: Value =
        serde_json::from_str(&response_value_json).expect("no-action response should parse");
    assert_eq!(value["performed"].as_bool(), Some(false));
    assert_eq!(
        value["target_resolution"]["source"].as_str(),
        Some("selector_refind")
    );
    assert_eq!(
        value["target_resolution"]["selector_refind"]["decision"].as_str(),
        Some("blocked")
    );
}

#[test]
fn drag_selector_handoff_should_return_no_action_instead_of_error() {
    let prepared = prepare_drag_request(&DragRequest {
        from: MouseEndpoint::Selector(MouseSelectorTarget {
            selector_id: "sel-v1-drag".to_owned(),
            auto_refind: false,
            policy: SelectorRefindPolicy::Safe,
            min_confidence_milli: 900,
            anchor: MouseAnchor::Center,
        }),
        to: MouseEndpoint::Coordinate(MousePoint { x: 10, y: 20 }),
        guard: None,
        button: MouseButtonName::Left,
        duration_ms: 450,
        steps: 24,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect("drag selector handoff should be a structured no-action response");

    let PreparedMouseRequest::NoAction {
        response_value_json,
    } = prepared
    else {
        panic!("drag selector handoff should not be ready");
    };
    let value: Value =
        serde_json::from_str(&response_value_json).expect("drag no-action should parse");
    assert_eq!(value["action"].as_str(), Some("drag"));
    assert_eq!(value["performed"].as_bool(), Some(false));
    assert_eq!(
        value["target_resolution"]["gate_decision"].as_str(),
        Some("handoff_required")
    );
}

#[test]
fn drag_selector_auto_refind_without_match_should_return_no_action() {
    let prepared = prepare_drag_request(&DragRequest {
        from: MouseEndpoint::Coordinate(MousePoint { x: 10, y: 20 }),
        to: MouseEndpoint::Selector(MouseSelectorTarget {
            selector_id: "sel-v1-drag-missing".to_owned(),
            auto_refind: true,
            policy: SelectorRefindPolicy::Safe,
            min_confidence_milli: 900,
            anchor: MouseAnchor::Center,
        }),
        guard: None,
        button: MouseButtonName::Left,
        duration_ms: 450,
        steps: 24,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect("drag missing selector should be reported as no-action");

    let PreparedMouseRequest::NoAction {
        response_value_json,
    } = prepared
    else {
        panic!("drag missing selector should not be ready");
    };
    let value: Value =
        serde_json::from_str(&response_value_json).expect("drag no-action should parse");
    assert_eq!(value["action"].as_str(), Some("drag"));
    assert_eq!(value["performed"].as_bool(), Some(false));
    assert_eq!(
        value["target_resolution"]["source"].as_str(),
        Some("selector_refind")
    );
    assert_eq!(
        value["target_resolution"]["selector_refind"]["decision"].as_str(),
        Some("blocked")
    );
}

#[test]
fn unsupported_observation_scope_should_not_prepare_mouse_action() {
    let header = record_observation(
        "visual",
        "@observe",
        ObservationRoot {
            schema: "rdog.visual.v1".to_owned(),
            platform: "macos".to_owned(),
            coordinate_space: "os-logical".to_owned(),
        },
        vec![ObservationRefEntry {
            ref_id: "@e1".to_owned(),
            backend_id: "visual:1".to_owned(),
            kind: "visual".to_owned(),
        }],
    )
    .expect("observation should record");

    let err = prepare_click_request(&ClickRequest {
        x: None,
        y: None,
        target: Some(MouseEndpoint::ObservationRef(MouseRefTarget {
            observation_id: header.observation_id,
            ref_id: "@e1".to_owned(),
            anchor: MouseAnchor::Center,
        })),
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .expect_err("unsupported observation scope should fail before mouse action");

    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    assert!(err.to_string().contains("UNSUPPORTED_OBSERVATION_SCOPE"));
}
