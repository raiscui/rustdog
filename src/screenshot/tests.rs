use super::*;
use crate::control_ax::{
    AxElement, AxRect, AxSnapshot, AxWindow, DEFAULT_AX_DEPTH, DEFAULT_AX_MAX_ELEMENTS,
};
use serde_json::Value;
use std::time::Duration;

#[test]
fn build_virtual_bounds_should_union_negative_and_positive_display_rects() {
    let bounds = build_virtual_bounds([
        LogicalRect {
            x: -100,
            y: 0,
            width: 100,
            height: 80,
        },
        LogicalRect {
            x: 0,
            y: 20,
            width: 120,
            height: 60,
        },
    ])
    .expect("virtual bounds should build");

    assert_eq!(
        bounds,
        LogicalRect {
            x: -100,
            y: 0,
            width: 220,
            height: 80,
        }
    );
}

#[test]
fn build_screenshot_bundle_should_map_os_rect_to_image_rect_and_preserve_gaps() {
    let displays = vec![
        fake_display(
            "left",
            LogicalRect {
                x: -100,
                y: 0,
                width: 100,
                height: 80,
            },
            Size {
                width: 100,
                height: 80,
            },
            Rgba([255, 0, 0, 255]),
        ),
        fake_display(
            "right",
            LogicalRect {
                x: 0,
                y: 20,
                width: 120,
                height: 60,
            },
            Size {
                width: 120,
                height: 60,
            },
            Rgba([0, 255, 0, 255]),
        ),
    ];

    let bundle = build_screenshot_bundle(displays, "fixed").expect("bundle should build");
    let manifest = manifest_value(&bundle);

    assert_eq!(manifest["virtual_bounds"]["x"], -100);
    assert_eq!(manifest["virtual_bounds"]["y"], 0);
    assert_eq!(manifest["virtual_bounds"]["width"], 220);
    assert_eq!(manifest["virtual_bounds"]["height"], 80);
    assert_eq!(manifest["display_count"], 2);
    assert_eq!(manifest["displays"].as_array().unwrap().len(), 2);

    assert_eq!(manifest["displays"][0]["image_rect"]["x"], 0);
    assert_eq!(manifest["displays"][0]["image_rect"]["y"], 0);
    assert_eq!(manifest["displays"][0]["display_id"], "left");
    assert_eq!(
        manifest["displays"][0]["id"],
        manifest["displays"][0]["display_id"]
    );
    assert_eq!(
        manifest["displays"][0]["display_id_stability"],
        DISPLAY_ID_STABILITY_SESSION
    );
    assert_eq!(
        manifest["displays"][0]["primary"],
        manifest["displays"][0]["is_primary"]
    );
    assert_eq!(manifest["displays"][1]["image_rect"]["x"], 100);
    assert_eq!(manifest["displays"][1]["image_rect"]["y"], 20);
    assert_eq!(manifest["gaps"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["gaps"][0]["x"], 100);
    assert_eq!(manifest["gaps"][0]["y"], 0);
    assert_eq!(manifest["gaps"][0]["width"], 120);
    assert_eq!(manifest["gaps"][0]["height"], 20);
}

#[test]
fn logical_composite_should_resize_native_capture_to_logical_rect() {
    let displays = vec![fake_display_with_native(
        "retina",
        LogicalRect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        },
        Size {
            width: 200,
            height: 100,
        },
        2.0,
        Rgba([0, 0, 255, 255]),
    )];

    let bundle = build_screenshot_bundle(displays, "fixed").expect("bundle should build");
    let manifest = manifest_value(&bundle);

    assert_eq!(bundle.composite.width(), 100);
    assert_eq!(bundle.composite.height(), 50);
    assert_eq!(manifest["displays"][0]["native_capture_size"]["width"], 200);
    assert_eq!(
        manifest["displays"][0]["native_capture_size"]["height"],
        100
    );
    assert_eq!(manifest["displays"][0]["scale_factor"], 2.0);
    assert_eq!(manifest["displays"][0]["resize_applied"], true);
}

#[test]
fn build_screenshot_outcome_should_emit_composite_image_manifest_and_bundle_response() {
    let request = ScreenshotRequest::default();
    let displays = vec![
        fake_display(
            "left",
            LogicalRect {
                x: -10,
                y: 0,
                width: 10,
                height: 4,
            },
            Size {
                width: 10,
                height: 4,
            },
            Rgba([255, 0, 0, 255]),
        ),
        fake_display(
            "right",
            LogicalRect {
                x: 0,
                y: 0,
                width: 12,
                height: 4,
            },
            Size {
                width: 12,
                height: 4,
            },
            Rgba([0, 255, 0, 255]),
        ),
    ];

    let outcome = build_composite_screenshot_outcome_with_id(Some(7), &request, displays, "123")
        .expect("outcome should build");

    assert_eq!(outcome.outbound_frames.len(), 3);
    let image_filename = match &outcome.outbound_frames[0] {
        ControlFrame::SaveFile(frame) => {
            assert_eq!(frame.request_id, Some(7));
            assert!(frame.filename.contains("virtual-desktop"));
            assert!(frame.filename.ends_with(".jpg"));
            assert_eq!(frame.mime, "image/jpeg");
            assert_eq!(frame.encoding, "base64");
            assert_eq!(frame.quality, Some(75));
            assert_eq!(frame.width, Some(22));
            assert_eq!(frame.height, Some(4));
            frame.filename.clone()
        }
        other => panic!("expected first frame to be SaveFile, got {other:?}"),
    };

    let manifest_filename = match &outcome.outbound_frames[1] {
        ControlFrame::SaveFile(frame) => {
            assert_eq!(frame.request_id, Some(7));
            assert!(frame.filename.contains("manifest"));
            assert!(frame.filename.ends_with(".json"));
            assert_eq!(frame.mime, "application/json");
            assert_eq!(frame.encoding, "base64");
            assert_eq!(frame.quality, None);
            assert_eq!(frame.width, None);
            assert_eq!(frame.height, None);
            let decoded = BASE64_STANDARD
                .decode(&frame.data)
                .expect("manifest should be base64 json");
            let manifest: Value =
                serde_json::from_slice(&decoded).expect("manifest should parse as json");
            assert_eq!(manifest["schema"], "rdog.screenshot.v1");
            assert_eq!(manifest["layout"], "composite");
            assert_eq!(manifest["coordinate_space"], "os-logical");
            assert_eq!(manifest["image_coordinate_space"], "virtual-logical-pixels");
            assert_eq!(manifest["capture_status"], "complete");
            assert_eq!(manifest["partial"], false);
            assert_eq!(manifest["display_count"], 2);
            assert_eq!(manifest["displays"].as_array().unwrap().len(), 2);
            frame.filename.clone()
        }
        other => panic!("expected second frame to be SaveFile, got {other:?}"),
    };

    match &outcome.outbound_frames[2] {
        ControlFrame::ResponseLine(line) => {
            assert!(line.contains(r#""kind":"screenshot-bundle""#));
            assert!(line.contains(r#""layout":"composite""#));
            assert!(line.contains(r#""coordinate_space":"os-logical""#));
            assert!(line.contains(&format!(r#""image":"{image_filename}""#)));
            assert!(line.contains(&format!(r#""manifest":"{manifest_filename}""#)));
            assert!(line.contains(r#""display_count":2"#));
        }
        other => panic!("expected third frame to be ResponseLine, got {other:?}"),
    }
}

#[test]
fn build_screenshot_bundle_should_embed_ax_snapshot_when_provided() {
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        Size {
            width: 10,
            height: 10,
        },
        Rgba([0, 255, 0, 255]),
    )];
    let bundle = build_screenshot_bundle_with_ax(displays, "fixed", Some(fake_ax_snapshot()))
        .expect("bundle should build");
    let manifest = manifest_value(&bundle);

    assert_eq!(manifest["accessibility"]["schema"], "rdog.ax.v1");
    assert_eq!(manifest["accessibility"]["capture_status"], "complete");
    assert_eq!(manifest["accessibility"]["coordinate_space"], "os-logical");
    assert_eq!(manifest["accessibility"]["window_count"], 1);
    assert_eq!(
        manifest["accessibility"]["windows"][0]["process_name"],
        "System Information"
    );
    assert_eq!(
        manifest["accessibility"]["windows"][0]["elements"][0]["actions"][0],
        "AXPress"
    );
}

#[test]
fn execute_composite_screenshot_request_should_not_call_ax_when_include_false() {
    let request = ScreenshotRequest::default();
    let mut ax_called = false;
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let outcome = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(displays),
        |_| {
            ax_called = true;
            Ok(fake_ax_snapshot())
        },
    )
    .expect("screenshot should succeed");

    assert_eq!(outcome.outbound_frames.len(), 3);
    assert!(!ax_called, "include_ax=false must not call AX provider");
    let manifest = manifest_from_outcome(&outcome);
    assert!(manifest.get("accessibility").is_none());
}

#[test]
fn execute_composite_screenshot_request_should_embed_ax_when_requested() {
    let request = ScreenshotRequest {
        include_ax: true,
        ..ScreenshotRequest::default()
    };
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let outcome = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(displays),
        |ax_request| {
            assert_eq!(ax_request.depth, DEFAULT_AX_DEPTH);
            assert_eq!(ax_request.max_elements, DEFAULT_AX_MAX_ELEMENTS);
            assert!(ax_request.include_values);
            Ok(fake_ax_snapshot())
        },
    )
    .expect("screenshot should succeed");

    let manifest = manifest_from_outcome(&outcome);
    assert_eq!(manifest["accessibility"]["capture_status"], "complete");
    assert_eq!(
        manifest["accessibility"]["observation"]["source_command"],
        "@screenshot include_ax"
    );
    assert_eq!(manifest["accessibility"]["observation"]["scope"], "ax");
    assert_eq!(
        manifest["accessibility"]["observation"]["selector_count"],
        2
    );
    assert_eq!(manifest["accessibility"]["windows"][0]["ref"], "@e1");
    assert_eq!(
        manifest["accessibility"]["windows"][0]["elements"][0]["ref"],
        "@e2"
    );
}

#[test]
fn execute_composite_screenshot_request_should_apply_ax_mode_defaults() {
    let request = ScreenshotRequest {
        include_ax: true,
        ax_mode: crate::control_ax::AxMode::Interactive,
        ax_depth: crate::control_ax::AX_INTERACTIVE_DEPTH,
        ax_max_elements: crate::control_ax::AX_INTERACTIVE_MAX_ELEMENTS,
        ax_include_values: crate::control_ax::AX_INTERACTIVE_INCLUDE_VALUES,
        ..ScreenshotRequest::default()
    };
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let outcome = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(displays),
        |ax_request| {
            assert_eq!(ax_request.depth, crate::control_ax::AX_INTERACTIVE_DEPTH);
            assert_eq!(
                ax_request.max_elements,
                crate::control_ax::AX_INTERACTIVE_MAX_ELEMENTS
            );
            assert!(!ax_request.include_values);
            Ok(fake_ax_snapshot())
        },
    )
    .expect("screenshot should succeed");

    let manifest = manifest_from_outcome(&outcome);
    assert_eq!(manifest["accessibility"]["capture_status"], "complete");
}

#[test]
fn execute_composite_screenshot_request_should_degrade_ax_permission_denied_when_optional() {
    let request = ScreenshotRequest {
        include_ax: true,
        ax_required: false,
        ..ScreenshotRequest::default()
    };
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let outcome = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(displays),
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "AX denied")),
    )
    .expect("optional AX denial should keep screenshot bundle");

    let manifest = manifest_from_outcome(&outcome);
    assert_eq!(
        manifest["accessibility"]["capture_status"],
        "permission_denied"
    );
    assert_eq!(manifest["accessibility"]["permission_status"], "denied");
}

#[test]
fn execute_composite_screenshot_request_should_fail_ax_permission_denied_when_required() {
    let request = ScreenshotRequest {
        include_ax: true,
        ax_required: true,
        ..ScreenshotRequest::default()
    };
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let err = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(displays),
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "AX denied")),
    )
    .expect_err("required AX denial should fail screenshot");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn execute_composite_screenshot_request_should_fail_stale_repeated_frame_before_ax() {
    let request = ScreenshotRequest {
        include_ax: true,
        ..ScreenshotRequest::default()
    };
    let mut last = None;
    let stale_displays = || {
        vec![fake_display(
            "one",
            LogicalRect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            Size {
                width: 2,
                height: 2,
            },
            Rgba([7, 8, 9, 255]),
        )]
    };

    let first = execute_composite_screenshot_request_with_capture_and_ax(
        Some(1),
        &request,
        || Ok(stale_displays()),
        |_| Ok(fake_ax_snapshot()),
    )
    .expect("first screenshot should establish freshness baseline");
    assert_eq!(first.outbound_frames.len(), 3);

    let mut ax_called = false;
    let guarded_first = execute_composite_screenshot_request_with_capture_ax_and_freshness(
        Some(2),
        &request,
        || Ok(stale_displays()),
        |_| {
            ax_called = true;
            Ok(fake_ax_snapshot())
        },
        |displays| {
            reject_stale_composite_fingerprint(composite_capture_fingerprint(displays), &mut last)
        },
    )
    .expect("first guarded frame should establish local baseline");
    assert_eq!(guarded_first.outbound_frames.len(), 3);

    ax_called = false;
    let err = execute_composite_screenshot_request_with_capture_ax_and_freshness(
        Some(3),
        &request,
        || Ok(stale_displays()),
        |_| {
            ax_called = true;
            Ok(fake_ax_snapshot())
        },
        |displays| {
            reject_stale_composite_fingerprint(composite_capture_fingerprint(displays), &mut last)
        },
    )
    .expect_err("second identical guarded frame should fail as stale");

    assert!(
        !ax_called,
        "stale visual frame should stop before AX capture"
    );
    assert_eq!(err.kind(), io::ErrorKind::Other);

    let payload: Value =
        serde_json::from_str(&err.to_string()).expect("stale error should be structured json");
    assert_eq!(payload["kind"], "screenshot-stale-frame");
    assert_eq!(payload["error_code"], "SCREENSHOT_STALE_FRAME");
    assert_eq!(
        payload["guard_policy"],
        "reject-consecutive-identical-composite-fingerprint"
    );
    assert_eq!(payload["display_count"], 1);
    assert_eq!(payload["displays"][0]["id"], "one");
    assert_eq!(payload["displays"][0]["backend"], "fake");
    assert!(payload["displays"][0]["pixel_hash"]
        .as_str()
        .is_some_and(|hash| !hash.is_empty()));
}

#[test]
fn stale_freshness_guard_should_allow_changed_frame_after_error() {
    let mut last = None;
    let first = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];
    let changed = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([255, 255, 255, 255]),
    )];

    let first_fingerprint = composite_capture_fingerprint(&first);
    reject_stale_composite_fingerprint(first_fingerprint.clone(), &mut last)
        .expect("first frame should be accepted");
    reject_stale_composite_fingerprint(first_fingerprint, &mut last)
        .expect_err("same frame should be rejected");
    reject_stale_composite_fingerprint(composite_capture_fingerprint(&changed), &mut last)
        .expect("changed frame should be accepted after stale error");
}

#[test]
fn stale_freshness_guard_should_allow_after_cache_ttl() {
    // 长间隔的同 hash 请求应该被 cache TTL 早退放行,
    // 不再被 stale guard 误判。这覆盖 daemon 跑 N 小时后
    // "用户视角的第一次请求"的实际场景。
    let mut last = None;
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    // 第一次请求:建立 baseline
    reject_stale_composite_fingerprint(composite_capture_fingerprint(&displays), &mut last)
        .expect("first frame should be accepted");

    // 模拟 "31s 后" 的同 hash 请求:用 Instant::now() + 31s 不需要真的 sleep
    let stale_displays_hash = displays.clone();
    let fp_first = composite_capture_fingerprint(&stale_displays_hash);
    let fp_later = CompositeCaptureFingerprint {
        captured_at: fp_first.captured_at + Duration::from_secs(31),
        display_count: fp_first.display_count,
        display_fingerprints: fp_first.display_fingerprints.clone(),
    };

    reject_stale_composite_fingerprint(fp_later, &mut last)
        .expect("31s 后的同 hash 请求应该被 TTL 早退放行,不再 stale");
}

#[test]
fn stale_freshness_guard_should_still_reject_within_ttl() {
    // 短间隔的同 hash 请求仍然走真 stale 检测(保留原有的诊断能力)。
    let mut last = None;
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];

    let fp1 = composite_capture_fingerprint(&displays);
    let fp2 = CompositeCaptureFingerprint {
        captured_at: fp1.captured_at + Duration::from_secs(5),
        display_count: fp1.display_count,
        display_fingerprints: fp1.display_fingerprints.clone(),
    };

    reject_stale_composite_fingerprint(fp1, &mut last).expect("first frame should be accepted");
    reject_stale_composite_fingerprint(fp2, &mut last)
        .expect_err("5s 内的同 hash 请求仍应被拒,保留 stale detection");
}

#[test]
fn stale_freshness_guard_should_allow_changed_frame_within_ttl() {
    // 短间隔 + 不同 hash → 屏真的变了,放行。
    let mut last = None;
    let first = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([0, 0, 0, 255]),
    )];
    let changed = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        Size {
            width: 1,
            height: 1,
        },
        Rgba([255, 255, 255, 255]),
    )];

    let fp1 = composite_capture_fingerprint(&first);
    let fp2 = CompositeCaptureFingerprint {
        captured_at: fp1.captured_at + Duration::from_secs(2),
        display_count: 1,
        display_fingerprints: vec![display_capture_fingerprint(&changed[0])],
    };

    reject_stale_composite_fingerprint(fp1, &mut last).expect("first frame should be accepted");
    reject_stale_composite_fingerprint(fp2, &mut last).expect("2s 后不同 hash 应该放行");
}

#[test]
fn build_screenshot_outcome_should_emit_primary_single_image_for_primary_request() {
    let image = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 255]));
    let request = ScreenshotRequest {
        display: ScreenshotDisplaySelector::Primary,
        layout: ScreenshotLayout::Single,
        ..ScreenshotRequest::default()
    };
    let outcome =
        build_primary_screenshot_outcome(Some(7), &request, image).expect("outcome should build");

    assert_eq!(outcome.outbound_frames.len(), 2);
    match &outcome.outbound_frames[0] {
        ControlFrame::SaveFile(frame) => {
            assert_eq!(frame.request_id, Some(7));
            assert_eq!(frame.mime, "image/jpeg");
            assert_eq!(frame.quality, Some(75));
            assert_eq!(frame.width, Some(2));
            assert_eq!(frame.height, Some(1));
            assert!(!frame.data.is_empty());
        }
        other => panic!("expected first frame to be SaveFile, got {other:?}"),
    }
    match &outcome.outbound_frames[1] {
        ControlFrame::ResponseLine(line) => {
            assert_eq!(line, r#"@response {"id":7,"value":0}"#);
        }
        other => panic!("expected second frame to be ResponseLine, got {other:?}"),
    }
}

#[test]
fn execute_primary_screenshot_request_should_validate_before_capture() {
    let request = ScreenshotRequest {
        display: ScreenshotDisplaySelector::All,
        layout: ScreenshotLayout::Single,
        ..ScreenshotRequest::default()
    };
    let mut capture_called = false;

    let err = execute_primary_screenshot_request_with_capture(Some(1), &request, || {
        capture_called = true;
        Ok(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])))
    })
    .expect_err("invalid primary request should fail before capture");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(!capture_called, "invalid request must not trigger capture");
}

#[test]
fn execute_composite_screenshot_request_should_validate_before_capture() {
    let request = ScreenshotRequest {
        display: ScreenshotDisplaySelector::Primary,
        layout: ScreenshotLayout::Composite,
        ..ScreenshotRequest::default()
    };
    let mut capture_called = false;

    let err = execute_composite_screenshot_request_with_capture(Some(1), &request, || {
        capture_called = true;
        Ok(vec![fake_display(
            "one",
            LogicalRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            Size {
                width: 1,
                height: 1,
            },
            Rgba([0, 0, 0, 255]),
        )])
    })
    .expect_err("invalid composite request should fail before capture");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(!capture_called, "invalid request must not trigger capture");
}

#[test]
fn execute_screenshot_request_with_capture_should_preserve_permission_denied() {
    let request = ScreenshotRequest::default();
    let err = execute_composite_screenshot_request_with_capture(Some(1), &request, || {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "screen recording denied",
        ))
    })
    .expect_err("permission denied should bubble up");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("screen recording denied"));
}

#[test]
fn build_screenshot_bundle_should_reject_rotated_display() {
    let mut display = fake_display(
        "rotated",
        LogicalRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        Size {
            width: 10,
            height: 10,
        },
        Rgba([255, 0, 0, 255]),
    );
    display.metadata.rotation = 90.0;

    let err = build_screenshot_bundle(vec![display], "fixed")
        .expect_err("rotated display should be unsupported");

    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn manifest_should_use_snake_case_schema_fields() {
    let displays = vec![fake_display(
        "one",
        LogicalRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
        Size {
            width: 10,
            height: 10,
        },
        Rgba([0, 255, 0, 255]),
    )];
    let bundle = build_screenshot_bundle(displays, "fixed").expect("bundle should build");
    let json = String::from_utf8(bundle.manifest_json).expect("manifest should be utf-8");

    assert!(json.contains("virtual_bounds"));
    assert!(json.contains("image_rect"));
    assert!(json.contains("coordinate_space"));
    assert!(!json.contains("virtualBounds"));
    assert!(!json.contains("imageRect"));
    assert!(!json.contains("coordinateSpace"));
}

#[test]
fn real_capture_smoke_should_capture_or_report_permission_denied() {
    reset_screenshot_freshness_cache_for_tests();
    let request = ScreenshotRequest::default();

    match execute_screenshot_request(Some(99), &request) {
        Ok(outcome) => {
            assert_eq!(outcome.outbound_frames.len(), 3);
            match &outcome.outbound_frames[0] {
                ControlFrame::SaveFile(frame) => {
                    assert_eq!(frame.request_id, Some(99));
                    assert_eq!(frame.mime, "image/jpeg");
                    assert_eq!(frame.quality, Some(75));
                    assert!(frame.width.is_some());
                    assert!(frame.height.is_some());
                    assert!(!frame.data.is_empty());
                }
                other => panic!("expected first frame to be SaveFile, got {other:?}"),
            }
            match &outcome.outbound_frames[1] {
                ControlFrame::SaveFile(frame) => {
                    assert_eq!(frame.request_id, Some(99));
                    assert_eq!(frame.mime, "application/json");
                    assert!(!frame.data.is_empty());
                }
                other => panic!("expected second frame to be SaveFile, got {other:?}"),
            }
        }
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("real screenshot smoke hit permission boundary: {err}");
        }
        Err(err)
            if err.kind() == io::ErrorKind::NotFound
                && (err.to_string().contains("没有可截图的显示器")
                    || err.to_string().contains("未找到可截图显示器")) =>
        {
            eprintln!("real screenshot smoke hit no-display boundary: {err}");
        }
        Err(err) => panic!("real screenshot smoke failed unexpectedly: {err}"),
    }
}

fn manifest_value(bundle: &ScreenshotBundle) -> Value {
    serde_json::from_slice(&bundle.manifest_json).expect("manifest should parse")
}

fn manifest_from_outcome(outcome: &ControlExecutionOutcome) -> Value {
    match &outcome.outbound_frames[1] {
        ControlFrame::SaveFile(frame) => {
            let decoded = BASE64_STANDARD
                .decode(&frame.data)
                .expect("manifest should be base64 json");
            serde_json::from_slice(&decoded).expect("manifest should parse")
        }
        other => panic!("expected second frame to be SaveFile, got {other:?}"),
    }
}

fn fake_ax_snapshot() -> AxSnapshot {
    AxSnapshot::complete(
        "macos",
        vec![AxWindow {
            id: "pid:123/window:0".to_owned(),
            ref_id: None,
            pid: 123,
            process_name: "System Information".to_owned(),
            title: Some("关于本机".to_owned()),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: Some(AxRect {
                x: 10,
                y: 20,
                width: 320,
                height: 240,
            }),
            focused: Some(true),
            elements: vec![AxElement {
                id: "pid:123/window:0/path:0".to_owned(),
                ref_id: None,
                role: "AXButton".to_owned(),
                subrole: Some("AXCloseButton".to_owned()),
                name: Some("关闭".to_owned()),
                value: None,
                value_redacted: false,
                description: Some("关闭按钮".to_owned()),
                rect: Some(AxRect {
                    x: 16,
                    y: 26,
                    width: 12,
                    height: 12,
                }),
                enabled: Some(true),
                actions: vec!["AXPress".to_owned()],
                ax_path: vec![0],
                children: Vec::new(),
            }],
        }],
        false,
    )
}

fn fake_display(
    id: &str,
    os_rect: LogicalRect,
    native_size: Size,
    color: Rgba<u8>,
) -> CapturedDisplay {
    fake_display_with_native(id, os_rect, native_size, 1.0, color)
}

fn fake_display_with_native(
    id: &str,
    os_rect: LogicalRect,
    native_size: Size,
    scale_factor: f32,
    color: Rgba<u8>,
) -> CapturedDisplay {
    let image = RgbaImage::from_pixel(native_size.width, native_size.height, color);
    CapturedDisplay {
        metadata: CapturedDisplayMetadata {
            id: id.to_owned(),
            name: format!("Display {id}"),
            is_primary: id == "one" || id == "left" || id == "retina",
            backend: ScreenshotBackend::Fake,
            os_rect,
            native_capture_size: native_size,
            scale_factor,
            rotation: 0.0,
        },
        image,
    }
}
