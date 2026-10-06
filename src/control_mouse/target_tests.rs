use super::{
    request::{
        ClickRequest, DragRequest, MouseAnchor, MouseButtonName, MouseCoordinateSpace,
        MouseEndpoint, MousePoint, MouseRefTarget, MouseScreenshotTarget, MouseSelectorTarget,
    },
    screenshot_frame::{
        clear_frame_cache_for_test, remember_screenshot_frame, remember_screenshot_frame_at,
        with_frame_cache_test_guard, ScreenshotFrame, ScreenshotImageToClickScale,
    },
    target::{prepare_click_request, prepare_drag_request, PreparedMouseRequest},
};
use crate::control_display_scope::DisplayRect;
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

/// 4K@200% 单屏: 图 3840×2160 ↔ os-logical 1920×1080 (标尺 0.5)。
/// 2026-10-05 现场就是这块屏 —— agent 报图上像素 (1104, 212), daemon 换算成
/// (552, 106) 再走既有 os-logical 流程。
#[test]
fn screenshot_pixel_click_should_resolve_through_frame_scale() {
    with_frame_cache_test_guard(|| {
        clear_frame_cache_for_test();
        remember_screenshot_frame(ScreenshotFrame::composite(
            "target-composite",
            3840,
            2160,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            ScreenshotImageToClickScale { x: 0.5, y: 0.5 },
        ));

        let prepared = prepare_click_request(&ClickRequest {
            x: None,
            y: None,
            target: Some(MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "target-composite".to_owned(),
                point: MousePoint { x: 1104, y: 212 },
            })),
            guard: None,
            button: MouseButtonName::Left,
            count: 1,
            hold_ms: 80,
            interval_ms: 120,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .expect("screenshot pixel click should resolve");

        let PreparedMouseRequest::Ready {
            request,
            target_resolution: Some(resolution),
        } = prepared
        else {
            panic!("screenshot pixel click should be ready");
        };
        assert_eq!(request.x, Some(552));
        assert_eq!(request.y, Some(106));
        assert_eq!(
            resolution["source"].as_str(),
            Some("screenshot_image_pixel")
        );
        assert_eq!(resolution["image_point"]["x"].as_i64(), Some(1104));
        assert_eq!(
            resolution["coordinate_space"].as_str(),
            Some("os-logical")
        );
    });
}

/// 副屏在主屏左侧时换算会得到负的 os-logical 坐标。截图端点只负责换算, 不越权
/// 拦截 —— "本平台是否支持负坐标" 由 plan 层那道既有闸管 (见
/// `plan::ensure_supported_absolute_coordinate`)。
#[test]
fn screenshot_pixel_drag_should_resolve_offset_display() {
    with_frame_cache_test_guard(|| {
        clear_frame_cache_for_test();
        remember_screenshot_frame(ScreenshotFrame::composite(
            "target-negative",
            3840,
            2160,
            DisplayRect {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            ScreenshotImageToClickScale { x: 0.5, y: 0.5 },
        ));

        let prepared = prepare_drag_request(&DragRequest {
            from: MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "target-negative".to_owned(),
                point: MousePoint { x: 100, y: 200 },
            }),
            to: MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "target-negative".to_owned(),
                point: MousePoint { x: 300, y: 400 },
            }),
            guard: None,
            button: MouseButtonName::Left,
            duration_ms: 450,
            steps: 24,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .expect("drag between screenshot pixels should resolve");

        let PreparedMouseRequest::Ready {
            request,
            target_resolution: Some(resolution),
        } = prepared
        else {
            panic!("screenshot pixel drag should be ready");
        };
        let MouseEndpoint::Coordinate(from) = request.from else {
            panic!("prepared drag endpoint should be normalized to a coordinate");
        };
        assert_eq!((from.x, from.y), (-1870, 100));
        assert_eq!(
            resolution["from"]["source"].as_str(),
            Some("screenshot_image_pixel")
        );
    });
}

/// 像素点出图: 放行等于把"图上看着在这儿"换成屏幕上一个无关的点, 那正是本组
/// 要消灭的失败形态, 所以必须当场拒。
#[test]
fn screenshot_pixel_outside_image_should_not_prepare_mouse_action() {
    with_frame_cache_test_guard(|| {
        clear_frame_cache_for_test();
        remember_screenshot_frame(ScreenshotFrame::composite(
            "target-out-of-image",
            3840,
            2160,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            ScreenshotImageToClickScale { x: 0.5, y: 0.5 },
        ));

        let err = prepare_click_request(&ClickRequest {
            x: None,
            y: None,
            target: Some(MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "target-out-of-image".to_owned(),
                point: MousePoint { x: 3840, y: 100 },
            })),
            guard: None,
            button: MouseButtonName::Left,
            count: 1,
            hold_ms: 80,
            interval_ms: 120,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .expect_err("out-of-image pixel should fail before mouse action");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("SCREENSHOT_POINT_OUT_OF_IMAGE"));
    });
}

#[test]
fn unknown_screenshot_id_should_ask_for_a_new_screenshot() {
    with_frame_cache_test_guard(|| {
        clear_frame_cache_for_test();

        let err = prepare_click_request(&ClickRequest {
            x: None,
            y: None,
            target: Some(MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "never-captured".to_owned(),
                point: MousePoint { x: 10, y: 20 },
            })),
            guard: None,
            button: MouseButtonName::Left,
            count: 1,
            hold_ms: 80,
            interval_ms: 120,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .expect_err("unknown screenshot id should fail before mouse action");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("SCREENSHOT_FRAME_UNAVAILABLE"));
        assert!(err.to_string().contains("请重新截图"), "{err}");
    });
}

/// TTL 到点后按"重新截图"处理, 而不是拿旧坐标系硬点 —— 缩放/布局多半已经变了。
#[test]
fn expired_screenshot_id_should_ask_for_a_new_screenshot() {
    with_frame_cache_test_guard(|| {
        clear_frame_cache_for_test();
        // 登记时间戳取 epoch 起点: 真实 now 必然已过 TTL。
        remember_screenshot_frame_at(
            ScreenshotFrame::composite(
                "target-expired",
                3840,
                2160,
                DisplayRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                ScreenshotImageToClickScale { x: 0.5, y: 0.5 },
            ),
            0,
        );

        let err = prepare_click_request(&ClickRequest {
            x: None,
            y: None,
            target: Some(MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
                screenshot_id: "target-expired".to_owned(),
                point: MousePoint { x: 10, y: 20 },
            })),
            guard: None,
            button: MouseButtonName::Left,
            count: 1,
            hold_ms: 80,
            interval_ms: 120,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .expect_err("expired screenshot id should fail before mouse action");

        assert!(err.to_string().contains("SCREENSHOT_FRAME_UNAVAILABLE"));
        assert!(err.to_string().contains("请重新截图"), "{err}");
    });
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
