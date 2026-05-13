use super::*;
use serde_json::Value;

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
        Err(err) => panic!("real screenshot smoke failed unexpectedly: {err}"),
    }
}

fn manifest_value(bundle: &ScreenshotBundle) -> Value {
    serde_json::from_slice(&bundle.manifest_json).expect("manifest should parse")
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
