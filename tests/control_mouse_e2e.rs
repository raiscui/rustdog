#![cfg(target_os = "macos")]

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use image::{imageops, ImageFormat, RgbImage};

#[path = "control_mouse_e2e/support.rs"]
mod support;

use support::{
    next_free_port, rdog_binary_path, start_daemon, temp_workdir, wait_until_port_is_busy,
    ControlSession,
};

const EXPECTED_ABOUT_MODEL: &str = "MacBook Air";
const EXPECTED_ABOUT_VERSION_NUMBER: &str = "15.7.5";
const ABOUT_WINDOW_MIN_CHANGED_PIXELS: u64 = 8_000;
const ABOUT_WINDOW_MIN_WIDTH: u32 = 280;
const ABOUT_WINDOW_MIN_HEIGHT: u32 = 180;
const APPLE_MENU_DROPDOWN_MIN_Y_OFFSET: i32 = 36;
const APPLE_MENU_MAX_PANEL_WIDTH: u32 = 240;
const ABOUT_THIS_MAC_ROW_CENTER_OFFSET: i32 = 18;

#[derive(Debug, Copy, Clone)]
struct LogicalRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl LogicalRect {
    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    fn contains_point(self, x: i32, y: i32) -> bool {
        i64::from(x) >= i64::from(self.x)
            && i64::from(x) < self.right()
            && i64::from(y) >= i64::from(self.y)
            && i64::from(y) < self.bottom()
    }

    fn expand_within(self, padding: i32, image_width: u32, image_height: u32) -> Self {
        let image_width = i32::try_from(image_width).expect("image width should fit i32");
        let image_height = i32::try_from(image_height).expect("image height should fit i32");
        let left = self.x.saturating_sub(padding).clamp(0, image_width);
        let top = self.y.saturating_sub(padding).clamp(0, image_height);
        let right = i32::try_from(self.right())
            .expect("rect right should fit i32")
            .saturating_add(padding)
            .clamp(0, image_width);
        let bottom = i32::try_from(self.bottom())
            .expect("rect bottom should fit i32")
            .saturating_add(padding)
            .clamp(0, image_height);

        Self {
            x: left,
            y: top,
            width: u32::try_from((right - left).max(0)).expect("expanded width should fit u32"),
            height: u32::try_from((bottom - top).max(0)).expect("expanded height should fit u32"),
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct OsPoint {
    x: i32,
    y: i32,
}

#[derive(Debug)]
struct ScreenshotArtifacts {
    manifest_path: PathBuf,
    image_path: PathBuf,
    manifest: serde_json::Value,
    image: RgbImage,
}

#[derive(Debug, Copy, Clone)]
struct ChangedRegion {
    rect: LogicalRect,
    changed_pixels: u64,
}

#[derive(Debug, Copy, Clone)]
struct MenuClickEvidence {
    menu_panel_image_rect: LogicalRect,
    about_this_mac_point: OsPoint,
}

#[derive(Debug)]
struct AboutWindowAccessibilityEvidence {
    rect: Option<LogicalRect>,
    text: String,
}

fn read_latest_screenshot_artifacts(workdir: &Path) -> ScreenshotArtifacts {
    let download_dir = workdir.join("rdog_downloads");
    let mut manifests = fs::read_dir(&download_dir)
        .expect("download directory should exist after screenshot")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();

    manifests.sort();
    let manifest_path = manifests
        .pop()
        .unwrap_or_else(|| panic!("expected at least one screenshot manifest in {download_dir:?}"));
    let manifest_text = fs::read_to_string(&manifest_path).expect("manifest should be readable");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest should be valid json");
    let screenshot_id = manifest["screenshot_id"]
        .as_str()
        .expect("manifest should include screenshot_id");
    let image_path = download_dir.join(format!("screenshot-{screenshot_id}-virtual-desktop.jpg"));
    assert!(
        image_path.exists(),
        "manifest {} should have matching image {}",
        manifest_path.display(),
        image_path.display()
    );

    let image = image::open(&image_path)
        .unwrap_or_else(|err| {
            panic!(
                "screenshot image {} should decode: {err}",
                image_path.display()
            )
        })
        .to_rgb8();

    ScreenshotArtifacts {
        manifest_path,
        image_path,
        manifest,
        image,
    }
}

fn assert_composite_os_logical_manifest(manifest: &serde_json::Value) {
    assert_eq!(manifest["schema"].as_str(), Some("rdog.screenshot.v1"));
    assert_eq!(manifest["layout"].as_str(), Some("composite"));
    assert_eq!(manifest["coordinate_space"].as_str(), Some("os-logical"));
}

fn manifest_i32(rect: &serde_json::Value, field: &str) -> i32 {
    let value = rect[field]
        .as_i64()
        .unwrap_or_else(|| panic!("manifest rect field `{field}` should be i64-compatible"));
    i32::try_from(value).unwrap_or_else(|_| panic!("manifest rect field `{field}` exceeds i32"))
}

fn manifest_u32(rect: &serde_json::Value, field: &str) -> u32 {
    let value = rect[field]
        .as_u64()
        .unwrap_or_else(|| panic!("manifest rect field `{field}` should be u64-compatible"));
    u32::try_from(value).unwrap_or_else(|_| panic!("manifest rect field `{field}` exceeds u32"))
}

fn manifest_rect(rect: &serde_json::Value) -> LogicalRect {
    LogicalRect {
        x: manifest_i32(rect, "x"),
        y: manifest_i32(rect, "y"),
        width: manifest_u32(rect, "width"),
        height: manifest_u32(rect, "height"),
    }
}

fn primary_display(manifest: &serde_json::Value) -> &serde_json::Value {
    let displays = manifest["displays"]
        .as_array()
        .expect("manifest displays should be an array");
    displays
        .iter()
        .find(|display| display["is_primary"].as_bool() == Some(true))
        .unwrap_or_else(|| displays.first().expect("manifest should contain a display"))
}

fn primary_os_rect(manifest: &serde_json::Value) -> LogicalRect {
    manifest_rect(&primary_display(manifest)["os_rect"])
}

fn primary_image_rect(manifest: &serde_json::Value) -> LogicalRect {
    manifest_rect(&primary_display(manifest)["image_rect"])
}

fn virtual_bounds(manifest: &serde_json::Value) -> LogicalRect {
    manifest_rect(&manifest["virtual_bounds"])
}

fn derive_apple_icon_click_target(manifest: &serde_json::Value) -> OsPoint {
    assert_composite_os_logical_manifest(manifest);

    let rect = primary_os_rect(manifest);

    // macOS 菜单栏在 primary display 的左上角。
    // 坐标仍然来自 screenshot manifest 的 os-logical rect。
    assert!(
        rect.width >= 180 && rect.height >= 80,
        "primary display is too small for stable Apple menu probing: {rect:?}"
    );

    OsPoint {
        x: rect
            .x
            .checked_add(14)
            .expect("Apple icon x coordinate should not overflow"),
        y: rect
            .y
            .checked_add(12)
            .expect("Apple icon y coordinate should not overflow"),
    }
}

fn derive_neutral_click_target(manifest: &serde_json::Value) -> OsPoint {
    assert_composite_os_logical_manifest(manifest);

    let rect = primary_os_rect(manifest);
    OsPoint {
        x: rect
            .x
            .checked_add(i32::try_from(rect.width / 2).expect("display width should fit i32"))
            .expect("neutral x coordinate should not overflow"),
        y: rect
            .y
            .checked_add(i32::try_from(rect.height / 2).expect("display height should fit i32"))
            .expect("neutral y coordinate should not overflow"),
    }
}

fn close_existing_about_this_mac_window() {
    let script = r#"tell application "System Events"
  repeat with procName in {"System Settings", "System Information"}
    if exists process procName then
      tell process procName
        repeat with w in windows
          if name of w is "关于本机" or name of w is "About This Mac" then
            repeat with b in buttons of w
              try
                if description of b is "关闭按钮" or description of b is "close button" then
                  perform action "AXPress" of b
                  exit repeat
                end if
              end try
            end repeat
          end if
        end repeat
      end tell
    end if
  end repeat
end tell"#;

    let _ = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn image_point_to_os(manifest: &serde_json::Value, x: i32, y: i32) -> OsPoint {
    let bounds = virtual_bounds(manifest);
    OsPoint {
        x: x.checked_add(bounds.x)
            .expect("image x to os x conversion should not overflow"),
        y: y.checked_add(bounds.y)
            .expect("image y to os y conversion should not overflow"),
    }
}

fn os_rect_to_image_rect(artifacts: &ScreenshotArtifacts, os_rect: LogicalRect) -> LogicalRect {
    let bounds = virtual_bounds(&artifacts.manifest);
    let image_x = os_rect
        .x
        .checked_sub(bounds.x)
        .expect("os x to image x conversion should not overflow");
    let image_y = os_rect
        .y
        .checked_sub(bounds.y)
        .expect("os y to image y conversion should not overflow");

    // AX 和 screenshot manifest 都使用 os-logical 语义。
    // composite image 只需要减去 virtual_bounds 即可回到截图像素坐标。
    LogicalRect {
        x: image_x,
        y: image_y,
        width: os_rect.width,
        height: os_rect.height,
    }
    .expand_within(0, artifacts.image.width(), artifacts.image.height())
}

fn full_image_rect(image: &RgbImage) -> LogicalRect {
    LogicalRect {
        x: 0,
        y: 0,
        width: image.width(),
        height: image.height(),
    }
}

fn color_diff_sum(a: image::Rgb<u8>, b: image::Rgb<u8>) -> u16 {
    u16::from(a[0].abs_diff(b[0])) + u16::from(a[1].abs_diff(b[1])) + u16::from(a[2].abs_diff(b[2]))
}

fn is_changed_pixel(before: &RgbImage, after: &RgbImage, x: u32, y: u32) -> bool {
    color_diff_sum(*before.get_pixel(x, y), *after.get_pixel(x, y)) > 45
}

fn changed_region(
    before: &RgbImage,
    after: &RgbImage,
    search_rect: LogicalRect,
    ignored_rect: Option<LogicalRect>,
) -> Option<ChangedRegion> {
    assert_eq!(
        before.dimensions(),
        after.dimensions(),
        "screenshots must share one virtual desktop image size"
    );

    let image_width = i32::try_from(after.width()).expect("image width should fit i32");
    let image_height = i32::try_from(after.height()).expect("image height should fit i32");
    let start_x = search_rect.x.clamp(0, image_width);
    let start_y = search_rect.y.clamp(0, image_height);
    let end_x = i32::try_from(search_rect.right())
        .expect("search rect right should fit i32")
        .clamp(0, image_width);
    let end_y = i32::try_from(search_rect.bottom())
        .expect("search rect bottom should fit i32")
        .clamp(0, image_height);

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut changed_pixels = 0_u64;

    for y in start_y..end_y {
        for x in start_x..end_x {
            if ignored_rect.is_some_and(|rect| rect.contains_point(x, y)) {
                continue;
            }

            let x_u32 = u32::try_from(x).expect("clamped x should fit u32");
            let y_u32 = u32::try_from(y).expect("clamped y should fit u32");
            if color_diff_sum(
                *before.get_pixel(x_u32, y_u32),
                *after.get_pixel(x_u32, y_u32),
            ) <= 45
            {
                continue;
            }

            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            changed_pixels += 1;
        }
    }

    if changed_pixels == 0 {
        return None;
    }

    Some(ChangedRegion {
        rect: LogicalRect {
            x: min_x,
            y: min_y,
            width: u32::try_from(max_x - min_x + 1).expect("changed width should fit u32"),
            height: u32::try_from(max_y - min_y + 1).expect("changed height should fit u32"),
        },
        changed_pixels,
    })
}

fn changed_components(
    before: &RgbImage,
    after: &RgbImage,
    search_rect: LogicalRect,
    ignored_rects: &[LogicalRect],
) -> Vec<ChangedRegion> {
    assert_eq!(
        before.dimensions(),
        after.dimensions(),
        "screenshots must share one virtual desktop image size"
    );

    let image_width = i32::try_from(after.width()).expect("image width should fit i32");
    let image_height = i32::try_from(after.height()).expect("image height should fit i32");
    let start_x = search_rect.x.clamp(0, image_width);
    let start_y = search_rect.y.clamp(0, image_height);
    let end_x = i32::try_from(search_rect.right())
        .expect("search rect right should fit i32")
        .clamp(0, image_width);
    let end_y = i32::try_from(search_rect.bottom())
        .expect("search rect bottom should fit i32")
        .clamp(0, image_height);

    let width = usize::try_from(after.width()).expect("image width should fit usize");
    let height = usize::try_from(after.height()).expect("image height should fit usize");
    let mut visited = vec![false; width * height];
    let mut regions = Vec::new();

    for y in start_y..end_y {
        for x in start_x..end_x {
            let x_u32 = u32::try_from(x).expect("clamped x should fit u32");
            let y_u32 = u32::try_from(y).expect("clamped y should fit u32");
            let index = pixel_index(width, x_u32, y_u32);

            if visited[index] || ignored_rects.iter().any(|rect| rect.contains_point(x, y)) {
                continue;
            }

            if !is_changed_pixel(before, after, x_u32, y_u32) {
                visited[index] = true;
                continue;
            }

            let mut queue = VecDeque::from([(x_u32, y_u32)]);
            visited[index] = true;
            let mut min_x = i32::try_from(x_u32).expect("x should fit i32");
            let mut min_y = i32::try_from(y_u32).expect("y should fit i32");
            let mut max_x = min_x;
            let mut max_y = min_y;
            let mut changed_pixels = 0_u64;

            while let Some((cx, cy)) = queue.pop_front() {
                let cx_i32 = i32::try_from(cx).expect("x should fit i32");
                let cy_i32 = i32::try_from(cy).expect("y should fit i32");
                changed_pixels += 1;
                min_x = min_x.min(cx_i32);
                min_y = min_y.min(cy_i32);
                max_x = max_x.max(cx_i32);
                max_y = max_y.max(cy_i32);

                for (nx, ny) in changed_neighbors(cx, cy, after.width(), after.height()) {
                    let nx_i32 = i32::try_from(nx).expect("x should fit i32");
                    let ny_i32 = i32::try_from(ny).expect("y should fit i32");
                    let next_index = pixel_index(width, nx, ny);

                    if nx_i32 < start_x
                        || nx_i32 >= end_x
                        || ny_i32 < start_y
                        || ny_i32 >= end_y
                        || visited[next_index]
                        || ignored_rects
                            .iter()
                            .any(|rect| rect.contains_point(nx_i32, ny_i32))
                    {
                        continue;
                    }

                    visited[next_index] = true;
                    if is_changed_pixel(before, after, nx, ny) {
                        queue.push_back((nx, ny));
                    }
                }
            }

            regions.push(ChangedRegion {
                rect: LogicalRect {
                    x: min_x,
                    y: min_y,
                    width: u32::try_from(max_x - min_x + 1).expect("changed width should fit u32"),
                    height: u32::try_from(max_y - min_y + 1)
                        .expect("changed height should fit u32"),
                },
                changed_pixels,
            });
        }
    }

    regions.sort_by_key(|region| std::cmp::Reverse(region.changed_pixels));
    regions
}

fn pixel_index(width: usize, x: u32, y: u32) -> usize {
    usize::try_from(y).expect("y should fit usize") * width
        + usize::try_from(x).expect("x should fit usize")
}

fn changed_neighbors(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut neighbors = Vec::with_capacity(4);

    if x > 0 {
        neighbors.push((x - 1, y));
    }
    if y > 0 {
        neighbors.push((x, y - 1));
    }
    if x + 1 < width {
        neighbors.push((x + 1, y));
    }
    if y + 1 < height {
        neighbors.push((x, y + 1));
    }

    neighbors.into_iter()
}

fn detect_about_this_mac_click_point(
    initial: &ScreenshotArtifacts,
    menu_open: &ScreenshotArtifacts,
) -> Option<MenuClickEvidence> {
    assert_composite_os_logical_manifest(&menu_open.manifest);

    let primary = primary_image_rect(&menu_open.manifest);
    let search_width = primary.width.min(420);
    let search_height = primary.height.min(460);
    let search_rect = LogicalRect {
        x: primary.x,
        y: primary
            .y
            .checked_add(18)
            .expect("menu search y should not overflow"),
        width: search_width,
        height: search_height.saturating_sub(18),
    };

    let region = changed_region(&initial.image, &menu_open.image, search_rect, None)?;

    if region.changed_pixels < 1_500 || region.rect.width < 120 || region.rect.height < 40 {
        return None;
    }

    let minimum_dropdown_top = primary
        .y
        .checked_add(APPLE_MENU_DROPDOWN_MIN_Y_OFFSET)
        .expect("Apple menu dropdown y should not overflow");
    let panel_top = minimum_dropdown_top;
    let panel_width = region.rect.width.min(APPLE_MENU_MAX_PANEL_WIDTH);

    // 这里不再猜 primary display 的绝对偏移。
    // 差分区域会包含 Apple 菜单栏高亮,所以先跳过菜单栏,再取下拉面板第一行中心。
    let click_image_x = region
        .rect
        .x
        .checked_add(i32::try_from(panel_width / 2).expect("menu width should fit i32"))
        .expect("About This Mac image x should not overflow");
    let click_image_y = panel_top
        .checked_add(ABOUT_THIS_MAC_ROW_CENTER_OFFSET)
        .expect("About This Mac image y should not overflow");
    let about_this_mac_point = image_point_to_os(&menu_open.manifest, click_image_x, click_image_y);

    Some(MenuClickEvidence {
        menu_panel_image_rect: LogicalRect {
            x: region.rect.x,
            y: panel_top,
            width: panel_width,
            height: region
                .rect
                .height
                .saturating_sub(u32::try_from(panel_top - region.rect.y).unwrap_or(0)),
        },
        about_this_mac_point,
    })
}

fn capture_apple_menu_evidence(
    control: &mut ControlSession,
    workdir: &Path,
    initial_artifacts: &ScreenshotArtifacts,
    apple_icon: OsPoint,
    first_request_id: u64,
) -> (String, ScreenshotArtifacts, MenuClickEvidence, u64) {
    let mut request_id = first_request_id;
    let mut transcript = String::new();

    for attempt_index in 0..3 {
        let move_id = request_id;
        let click_id = request_id + 1;
        let sleep_id = request_id + 2;
        let screenshot_id = request_id + 3;
        let wait_seconds = 0.4 + f64::from(attempt_index) * 0.2;
        let open_menu_script = format!(
            r#"@mouse-move#{move_id}:{{x:{apple_x},y:{apple_y},coordinate_space:"os-logical"}}
@click#{click_id}:{{x:{apple_x},y:{apple_y},button:"left",count:1,hold_ms:120,coordinate_space:"os-logical"}}
@cmd#{sleep_id}:"sleep {wait_seconds:.1}"
@screenshot#{screenshot_id}
"#,
            apple_x = apple_icon.x,
            apple_y = apple_icon.y,
        );

        control.send(&open_menu_script);
        let output = control.wait_for_all(
            "Apple menu screenshot",
            &[
                &format!(r#""id":{move_id}"#),
                &format!(r#""id":{click_id}"#),
                &format!(r#""id":{screenshot_id}"#),
                "screenshot-bundle",
            ],
            Duration::from_secs(8),
        );
        transcript.push_str(&output);

        assert_control_output_success("Apple menu screenshot", &output);
        assert!(
            output.contains("screenshot-bundle"),
            "Apple menu screenshot should provide visual evidence before deriving About This Mac target\n{output}"
        );

        let menu_artifacts = read_latest_screenshot_artifacts(workdir);
        if let Some(click_evidence) =
            detect_about_this_mac_click_point(initial_artifacts, &menu_artifacts)
        {
            return (transcript, menu_artifacts, click_evidence, request_id + 4);
        }

        eprintln!(
            "Apple menu screenshot attempt {} did not expose a stable menu panel; retrying with another Apple icon click",
            attempt_index + 1
        );
        request_id += 4;
    }

    panic!("Apple menu panel was not visible after repeated screenshots\n{transcript}");
}

fn assert_control_output_success(label: &str, output: &str) {
    for line in output.lines() {
        let Some(payload) = line.trim().strip_prefix("@response ") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            continue;
        };

        if let Some(error) = value.get("error") {
            panic!("control response for {label} returned error {error}\n{output}");
        }

        if let Some(code) = value.get("code").and_then(serde_json::Value::as_i64) {
            assert_eq!(
                code, 0,
                "control response for {label} returned non-zero code {code}\n{output}"
            );
        }
    }
}

fn assert_about_window_visual_evidence(
    workdir: &Path,
    menu_artifacts: &ScreenshotArtifacts,
    final_artifacts: &ScreenshotArtifacts,
    menu_panel_image_rect: LogicalRect,
) {
    let accessibility = read_about_window_accessibility_evidence();
    let ax_about_window = accessibility
        .rect
        .map(|rect| ChangedRegion {
            rect: os_rect_to_image_rect(final_artifacts, rect),
            changed_pixels: 0,
        })
        .filter(|region| {
            region.rect.width >= ABOUT_WINDOW_MIN_WIDTH
                && region.rect.height >= ABOUT_WINDOW_MIN_HEIGHT
        });
    let (about_window, about_window_source) = if let Some(region) = ax_about_window {
        (region, "AX window geometry")
    } else if let Some(region) =
        detect_about_window_region(menu_artifacts, final_artifacts, menu_panel_image_rect)
    {
        (region, "screenshot diff")
    } else {
        let candidates = describe_about_window_candidates(
            menu_artifacts,
            final_artifacts,
            menu_panel_image_rect,
        );
        panic!(
                "About This Mac must open an independent window, but no large new window region was found\nmenu screenshot: {}\nfinal screenshot: {}\nAX text:\n{}\nchanged candidates:\n{}",
                menu_artifacts.image_path.display(),
                final_artifacts.image_path.display(),
                accessibility.text,
                candidates
            )
    };
    let ocr_input = save_about_window_crop(workdir, final_artifacts, about_window.rect);
    let ocr_text = ocr_image_text(&ocr_input);
    let accessibility_text = accessibility.text;
    let normalized_ocr_text = normalize_ocr_text(&ocr_text);
    let normalized_accessibility_text = normalize_ocr_text(&accessibility_text);
    let normalized_evidence_text =
        format!("{normalized_ocr_text}\n{normalized_accessibility_text}");
    let expected_model = normalize_ocr_text(EXPECTED_ABOUT_MODEL);
    let expected_version_number = normalize_ocr_text(EXPECTED_ABOUT_VERSION_NUMBER);

    eprintln!(
        "rdog mouse e2e About window OCR text from {} region {:?} source={} changed_pixels={}:\n{}",
        ocr_input.display(),
        about_window.rect,
        about_window_source,
        about_window.changed_pixels,
        ocr_text
    );
    eprintln!("rdog mouse e2e About window AX text:\n{accessibility_text}");

    assert!(
        normalized_evidence_text.contains(&expected_model),
        "About This Mac independent window evidence must include `{EXPECTED_ABOUT_MODEL}`\nnormalized OCR text:\n{normalized_ocr_text}\nnormalized AX text:\n{normalized_accessibility_text}",
    );
    assert!(
        normalized_evidence_text.contains(&expected_version_number),
        "About This Mac independent window evidence must include macOS version `{EXPECTED_ABOUT_VERSION_NUMBER}`\nnormalized OCR text:\n{normalized_ocr_text}\nnormalized AX text:\n{normalized_accessibility_text}",
    );
}

fn parse_about_window_rect(text: &str) -> Option<LogicalRect> {
    let rect_line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("WINDOW_RECT:"))?;
    let parts = rect_line.split(',').map(str::trim).collect::<Vec<_>>();

    if parts.len() != 4 {
        return None;
    }

    Some(LogicalRect {
        x: parts[0].parse().ok()?,
        y: parts[1].parse().ok()?,
        width: parts[2].parse().ok()?,
        height: parts[3].parse().ok()?,
    })
}

fn read_about_window_accessibility_evidence() -> AboutWindowAccessibilityEvidence {
    let script = r#"tell application "System Events"
  set outText to ""
  repeat with procName in {"System Information", "System Settings"}
    if exists process procName then
      tell process procName
        repeat with w in windows
          if name of w is "关于本机" or name of w is "About This Mac" then
            try
              set windowPosition to position of w
              set windowSize to size of w
              set outText to outText & "WINDOW_RECT:" & (item 1 of windowPosition as integer) & "," & (item 2 of windowPosition as integer) & "," & (item 1 of windowSize as integer) & "," & (item 2 of windowSize as integer) & linefeed
            end try
            set outText to outText & "PROCESS:" & procName & linefeed
            repeat with uiElement in entire contents of w
              try
                set elementValue to value of uiElement as text
                if elementValue is not "" then set outText to outText & elementValue & linefeed
              end try
              try
                set elementName to name of uiElement as text
                if elementName is not "" then set outText to outText & elementName & linefeed
              end try
              try
                set elementDescription to description of uiElement as text
                if elementDescription is not "" then set outText to outText & elementDescription & linefeed
              end try
            end repeat
          end if
        end repeat
      end tell
    end if
  end repeat
  return outText
end tell"#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .expect("osascript should run to read About This Mac accessibility evidence");

    assert!(
        output.status.success(),
        "osascript should read About This Mac accessibility evidence\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    AboutWindowAccessibilityEvidence {
        rect: parse_about_window_rect(&text),
        text,
    }
}

fn detect_about_window_region(
    menu_artifacts: &ScreenshotArtifacts,
    final_artifacts: &ScreenshotArtifacts,
    menu_panel_image_rect: LogicalRect,
) -> Option<ChangedRegion> {
    let ignored_menu_rect = menu_panel_image_rect.expand_within(
        32,
        final_artifacts.image.width(),
        final_artifacts.image.height(),
    );
    let regions = changed_components(
        &menu_artifacts.image,
        &final_artifacts.image,
        full_image_rect(&final_artifacts.image),
        &[ignored_menu_rect],
    );

    regions.into_iter().find(|region| {
        region.changed_pixels >= ABOUT_WINDOW_MIN_CHANGED_PIXELS
            && region.rect.width >= ABOUT_WINDOW_MIN_WIDTH
            && region.rect.height >= ABOUT_WINDOW_MIN_HEIGHT
    })
}

fn describe_about_window_candidates(
    menu_artifacts: &ScreenshotArtifacts,
    final_artifacts: &ScreenshotArtifacts,
    menu_panel_image_rect: LogicalRect,
) -> String {
    let ignored_menu_rect = menu_panel_image_rect.expand_within(
        32,
        final_artifacts.image.width(),
        final_artifacts.image.height(),
    );
    let regions = changed_components(
        &menu_artifacts.image,
        &final_artifacts.image,
        full_image_rect(&final_artifacts.image),
        &[ignored_menu_rect],
    );

    regions
        .into_iter()
        .take(8)
        .map(|region| {
            format!(
                "rect={:?}, changed_pixels={}",
                region.rect, region.changed_pixels
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_ocr_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn save_about_window_crop(
    workdir: &Path,
    artifacts: &ScreenshotArtifacts,
    window_rect: LogicalRect,
) -> PathBuf {
    let crop_rect =
        window_rect.expand_within(96, artifacts.image.width(), artifacts.image.height());
    let crop_x = u32::try_from(crop_rect.x).expect("crop x should fit u32");
    let crop_y = u32::try_from(crop_rect.y).expect("crop y should fit u32");
    let crop = imageops::crop_imm(
        &artifacts.image,
        crop_x,
        crop_y,
        crop_rect.width,
        crop_rect.height,
    )
    .to_image();
    let enlarged = imageops::resize(
        &crop,
        crop.width().saturating_mul(3),
        crop.height().saturating_mul(3),
        imageops::FilterType::CatmullRom,
    );
    let ocr_input = workdir.join("about-this-mac-window-crop-ocr.png");

    enlarged
        .save_with_format(&ocr_input, ImageFormat::Png)
        .unwrap_or_else(|err| panic!("OCR crop {} should save: {err}", ocr_input.display()));

    ocr_input
}

fn ocr_image_text(ocr_input: &Path) -> String {
    assert!(
        Command::new("tesseract")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("tesseract should be executable for About window OCR")
            .success(),
        "tesseract is required to verify About This Mac text"
    );

    let output = Command::new("tesseract")
        .arg(ocr_input)
        .arg("stdout")
        .args(["-l", "eng", "--psm", "6"])
        .output()
        .expect("tesseract should run for About window crop");

    assert!(
        output.status.success(),
        "tesseract should OCR About window crop {}\nstdout:\n{}\nstderr:\n{}",
        ocr_input.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[ignore = "moves and clicks the real macOS pointer; requires Screen Recording and Accessibility permissions"]
fn daemon_control_lane_should_click_apple_menu_about_this_mac_via_rdog_control() {
    close_existing_about_this_mac_window();

    let port = next_free_port();
    let binary = rdog_binary_path();
    let workdir = temp_workdir("apple-menu-about-this-mac");
    let mut daemon = start_daemon(&binary, &workdir, port);

    assert!(
        wait_until_port_is_busy(daemon.child_mut(), port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = ControlSession::spawn(&binary, &workdir, port);
    control.send("@screenshot#30\n");
    let screenshot_output = control.wait_for_all(
        "initial screenshot",
        &[r#""id":30"#, "screenshot-bundle", "os-logical"],
        Duration::from_secs(5),
    );

    let initial_artifacts = read_latest_screenshot_artifacts(&workdir);
    let apple_icon = derive_apple_icon_click_target(&initial_artifacts.manifest);
    let neutral_point = derive_neutral_click_target(&initial_artifacts.manifest);
    let (open_menu_output, menu_artifacts, click_evidence, next_request_id) =
        capture_apple_menu_evidence(&mut control, &workdir, &initial_artifacts, apple_icon, 31);

    assert!(
        !open_menu_output.contains(r#""code":77"#),
        "mouse E2E requires macOS Accessibility permission for the rdog daemon process\n{open_menu_output}"
    );

    eprintln!(
        "rdog mouse e2e evidence: workdir={}, initial_manifest={}, menu_manifest={}, initial_image={}, menu_image={}, apple_icon=({},{}) about_this_mac=({},{}) menu_panel_image_rect={:?}",
        workdir.display(),
        initial_artifacts.manifest_path.display(),
        menu_artifacts.manifest_path.display(),
        initial_artifacts.image_path.display(),
        menu_artifacts.image_path.display(),
        apple_icon.x,
        apple_icon.y,
        click_evidence.about_this_mac_point.x,
        click_evidence.about_this_mac_point.y,
        click_evidence.menu_panel_image_rect
    );

    let about = click_evidence.about_this_mac_point;
    let neutral_click_id = next_request_id;
    let normalize_sleep_id = next_request_id + 1;
    let move_apple_id = next_request_id + 2;
    let press_apple_id = next_request_id + 3;
    let press_sleep_id = next_request_id + 4;
    let move_about_id = next_request_id + 5;
    let hover_sleep_id = next_request_id + 6;
    let release_about_id = next_request_id + 7;
    let final_sleep_id = next_request_id + 8;
    let final_screenshot_id = next_request_id + 9;
    let click_about_script = format!(
        r#"@click#{neutral_click_id}:{{x:{neutral_x},y:{neutral_y},button:"left",count:1,hold_ms:80,coordinate_space:"os-logical"}}
@cmd#{normalize_sleep_id}:"sleep 0.2"
@mouse-move#{move_apple_id}:{{x:{apple_x},y:{apple_y},coordinate_space:"os-logical"}}
@mouse-button#{press_apple_id}:{{button:"left",mode:"press"}}
@cmd#{press_sleep_id}:"sleep 0.4"
@mouse-move#{move_about_id}:{{x:{about_x},y:{about_y},coordinate_space:"os-logical"}}
@cmd#{hover_sleep_id}:"sleep 0.2"
@mouse-button#{release_about_id}:{{button:"left",mode:"release"}}
@cmd#{final_sleep_id}:"sleep 2.0"
@screenshot#{final_screenshot_id}
"#,
        neutral_x = neutral_point.x,
        neutral_y = neutral_point.y,
        apple_x = apple_icon.x,
        apple_y = apple_icon.y,
        about_x = about.x,
        about_y = about.y,
    );

    control.send(&click_about_script);
    let click_about_output = control.wait_for_all(
        "About This Mac screenshot",
        &[
            &format!(r#""id":{neutral_click_id}"#),
            &format!(r#""id":{move_apple_id}"#),
            &format!(r#""id":{press_apple_id}"#),
            &format!(r#""id":{move_about_id}"#),
            &format!(r#""id":{release_about_id}"#),
            &format!(r#""id":{final_screenshot_id}"#),
            "screenshot-bundle",
        ],
        Duration::from_secs(10),
    );

    assert_control_output_success("initial screenshot", &screenshot_output);
    assert_control_output_success("Apple menu open", &open_menu_output);
    assert_control_output_success("About This Mac click", &click_about_output);
    assert!(open_menu_output.contains(r#""kind":"mouse""#));
    assert!(click_about_output.contains(r#""kind":"mouse""#));
    assert!(open_menu_output.contains(r#""action":"move""#));
    assert!(click_about_output.contains(r#""action":"move""#));
    assert!(open_menu_output.contains(r#""action":"click""#));
    assert!(click_about_output.contains(r#""action":"button""#));
    assert!(
        click_about_output.contains(&format!(r#""id":{final_screenshot_id}"#))
            && click_about_output.contains("screenshot-bundle"),
        "final screenshot should provide visual evidence after clicking About This Mac\n{click_about_output}"
    );

    let final_artifacts = read_latest_screenshot_artifacts(&workdir);
    assert_about_window_visual_evidence(
        &workdir,
        &menu_artifacts,
        &final_artifacts,
        click_evidence.menu_panel_image_rect,
    );

    drop(control);
    drop(daemon);
    close_existing_about_this_mac_window();
    let _ = fs::remove_dir_all(workdir);
}
