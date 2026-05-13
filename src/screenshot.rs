use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{
    codecs::jpeg::JpegEncoder,
    imageops::{overlay, resize, FilterType},
    DynamicImage, Rgba, RgbaImage,
};
use serde::Serialize;
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_protocol::{
        ScreenshotCoordinateSpace, ScreenshotDisplaySelector, ScreenshotLayout, ScreenshotRequest,
    },
};

/// 执行 `@screenshot` 请求。
///
/// 默认请求现在走 all-display composite 路径。显式 `display:"primary"` 仍保留
/// 单图兼容行为,避免旧脚本突然被 manifest bundle 破坏。
pub fn execute_screenshot_request(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
) -> io::Result<ControlExecutionOutcome> {
    match request.display {
        ScreenshotDisplaySelector::All => {
            execute_composite_screenshot_request_with_capture(request_id, request, || {
                capture_all_display_images()
            })
        }
        ScreenshotDisplaySelector::Primary => {
            execute_primary_screenshot_request_with_capture(request_id, request, || {
                capture_primary_display_image()
            })
        }
    }
}

fn execute_primary_screenshot_request_with_capture<F>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<RgbaImage>,
{
    validate_primary_request(request)?;
    let image = capture()?;
    build_primary_screenshot_outcome(request_id, request, image)
}

fn execute_composite_screenshot_request_with_capture<F>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
{
    validate_composite_request(request)?;
    let displays = capture()?;
    let screenshot_id = current_unix_epoch_millis().to_string();
    build_composite_screenshot_outcome_with_id(request_id, request, displays, &screenshot_id)
}

fn build_primary_screenshot_outcome(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    image: RgbaImage,
) -> io::Result<ControlExecutionOutcome> {
    validate_primary_request(request)?;

    let width = image.width();
    let height = image.height();
    let filename = format!("screenshot-{}.jpg", current_unix_epoch_millis());
    let jpeg_bytes = encode_jpeg(&image, request.quality)?;
    let save_file = SaveFileFrame {
        request_id,
        filename,
        mime: "image/jpeg".to_owned(),
        encoding: "base64".to_owned(),
        data: BASE64_STANDARD.encode(jpeg_bytes),
        quality: Some(request.quality),
        width: Some(width),
        height: Some(height),
    };

    Ok(ControlExecutionOutcome {
        outbound_frames: vec![
            ControlFrame::SaveFile(save_file),
            ControlFrame::ResponseLine(render_primary_screenshot_success_response(request_id)),
        ],
    })
}

fn build_composite_screenshot_outcome_with_id(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
) -> io::Result<ControlExecutionOutcome> {
    validate_composite_request(request)?;

    let bundle = build_screenshot_bundle(displays, screenshot_id)?;
    let image_filename = format!("screenshot-{screenshot_id}-virtual-desktop.jpg");
    let manifest_filename = format!("screenshot-{screenshot_id}-manifest.json");
    let jpeg_bytes = encode_jpeg(&bundle.composite, request.quality)?;

    let image_frame = SaveFileFrame {
        request_id,
        filename: image_filename.clone(),
        mime: "image/jpeg".to_owned(),
        encoding: "base64".to_owned(),
        data: BASE64_STANDARD.encode(jpeg_bytes),
        quality: Some(request.quality),
        width: Some(bundle.image_size.width),
        height: Some(bundle.image_size.height),
    };

    let manifest_frame = SaveFileFrame {
        request_id,
        filename: manifest_filename.clone(),
        mime: "application/json".to_owned(),
        encoding: "base64".to_owned(),
        data: BASE64_STANDARD.encode(&bundle.manifest_json),
        quality: None,
        width: None,
        height: None,
    };

    let response = render_screenshot_bundle_response(
        request_id,
        &image_filename,
        &manifest_filename,
        bundle.display_count,
    )?;

    Ok(ControlExecutionOutcome {
        outbound_frames: vec![
            ControlFrame::SaveFile(image_frame),
            ControlFrame::SaveFile(manifest_frame),
            ControlFrame::ResponseLine(response),
        ],
    })
}

fn validate_primary_request(request: &ScreenshotRequest) -> io::Result<()> {
    if request.display != ScreenshotDisplaySelector::Primary
        || request.layout != ScreenshotLayout::Single
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "primary screenshot 必须使用 display=primary 且 layout=single",
        ));
    }
    validate_coordinate_space(request)
}

fn validate_composite_request(request: &ScreenshotRequest) -> io::Result<()> {
    if request.display != ScreenshotDisplaySelector::All
        || request.layout != ScreenshotLayout::Composite
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "composite screenshot 必须使用 display=all 且 layout=composite",
        ));
    }
    validate_coordinate_space(request)
}

fn validate_coordinate_space(request: &ScreenshotRequest) -> io::Result<()> {
    if request.coordinate_space != ScreenshotCoordinateSpace::OsLogical {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "当前 screenshot 只支持 coordinate_space=os-logical",
        ));
    }
    Ok(())
}

/// 一个显示器的统一截图元数据。
///
/// backend adapter 只负责把不同库暴露的字段收敛到这里。后面的 composite
/// 逻辑只消费 `os_rect` 和 `native_capture_size`,避免在拼图层猜尺寸语义。
#[derive(Debug, Clone)]
struct CapturedDisplayMetadata {
    id: String,
    name: String,
    is_primary: bool,
    backend: ScreenshotBackend,
    os_rect: LogicalRect,
    native_capture_size: Size,
    scale_factor: f32,
    rotation: f32,
}

#[derive(Debug, Clone)]
struct CapturedDisplay {
    metadata: CapturedDisplayMetadata,
    image: RgbaImage,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ScreenshotBackend {
    SckRs,
    Xcap,
    #[cfg(test)]
    Fake,
}

impl ScreenshotBackend {
    fn as_str(self) -> &'static str {
        match self {
            ScreenshotBackend::SckRs => "sck-rs",
            ScreenshotBackend::Xcap => "xcap",
            #[cfg(test)]
            ScreenshotBackend::Fake => "fake",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
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

    fn contains_rect(self, other: LogicalRect) -> bool {
        i64::from(other.x) >= i64::from(self.x)
            && i64::from(other.y) >= i64::from(self.y)
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
struct Size {
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct ScreenshotBundle {
    composite: RgbaImage,
    manifest_json: Vec<u8>,
    image_size: Size,
    display_count: usize,
}

#[derive(Serialize)]
struct ScreenshotManifest {
    schema: &'static str,
    screenshot_id: String,
    layout: &'static str,
    coordinate_space: &'static str,
    image_coordinate_space: &'static str,
    capture_status: &'static str,
    partial: bool,
    backend_policy: &'static str,
    virtual_bounds: LogicalRect,
    image_size: Size,
    display_count: usize,
    transforms: ScreenshotTransforms,
    gaps: Vec<LogicalRect>,
    displays: Vec<DisplayManifest>,
}

#[derive(Serialize)]
struct ScreenshotTransforms {
    image_to_os: &'static str,
    os_to_image: &'static str,
}

#[derive(Serialize)]
struct DisplayManifest {
    id: String,
    name: String,
    is_primary: bool,
    backend: &'static str,
    os_rect: LogicalRect,
    image_rect: LogicalRect,
    native_capture_size: Size,
    scale_factor: f32,
    resize_applied: bool,
    rotation: f32,
}

#[derive(Serialize)]
struct ScreenshotBundleResponse<'a> {
    kind: &'static str,
    layout: &'static str,
    coordinate_space: &'static str,
    image: &'a str,
    manifest: &'a str,
    display_count: usize,
}

fn build_screenshot_bundle(
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
) -> io::Result<ScreenshotBundle> {
    if displays.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "没有可截图的显示器",
        ));
    }

    validate_captured_displays(&displays)?;

    let virtual_bounds =
        build_virtual_bounds(displays.iter().map(|display| display.metadata.os_rect))?;
    let image_size = Size {
        width: virtual_bounds.width,
        height: virtual_bounds.height,
    };
    let mut composite =
        RgbaImage::from_pixel(image_size.width, image_size.height, Rgba([18, 18, 18, 255]));

    let mut display_manifests = Vec::with_capacity(displays.len());
    let display_rects: Vec<LogicalRect> = displays
        .iter()
        .map(|display| display.metadata.os_rect)
        .collect();
    let gaps = compute_gap_rects(virtual_bounds, &display_rects)?;

    for display in displays {
        let image_rect = os_rect_to_image_rect(display.metadata.os_rect, virtual_bounds)?;
        let resized = resize_display_image_to_logical_rect(&display.image, image_rect);
        overlay(
            &mut composite,
            &resized,
            i64::from(image_rect.x),
            i64::from(image_rect.y),
        );

        let resize_applied = display.image.width() != image_rect.width
            || display.image.height() != image_rect.height;

        display_manifests.push(DisplayManifest {
            id: display.metadata.id,
            name: display.metadata.name,
            is_primary: display.metadata.is_primary,
            backend: display.metadata.backend.as_str(),
            os_rect: display.metadata.os_rect,
            image_rect,
            native_capture_size: display.metadata.native_capture_size,
            scale_factor: display.metadata.scale_factor,
            resize_applied,
            rotation: display.metadata.rotation,
        });
    }

    let manifest = ScreenshotManifest {
        schema: "rdog.screenshot.v1",
        screenshot_id: screenshot_id.to_owned(),
        layout: "composite",
        coordinate_space: "os-logical",
        image_coordinate_space: "virtual-logical-pixels",
        capture_status: "complete",
        partial: false,
        backend_policy: backend_policy_for_current_platform(),
        virtual_bounds,
        image_size,
        display_count: display_manifests.len(),
        transforms: ScreenshotTransforms {
            image_to_os: "os_x=image_x+virtual_bounds.x; os_y=image_y+virtual_bounds.y",
            os_to_image: "image_x=os_x-virtual_bounds.x; image_y=os_y-virtual_bounds.y",
        },
        gaps,
        displays: display_manifests,
    };

    let display_count = manifest.display_count;
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|err| io::Error::other(format!("screenshot manifest 序列化失败: {err}")))?;

    Ok(ScreenshotBundle {
        composite,
        manifest_json,
        image_size,
        display_count,
    })
}

fn validate_captured_displays(displays: &[CapturedDisplay]) -> io::Result<()> {
    for display in displays {
        if display.metadata.os_rect.width == 0 || display.metadata.os_rect.height == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "显示器 {} 的 logical rect 尺寸必须大于 0",
                    display.metadata.id
                ),
            ));
        }
        if display.image.width() == 0 || display.image.height() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("显示器 {} 的截图尺寸必须大于 0", display.metadata.id),
            ));
        }
        if display.metadata.rotation.abs() > f32::EPSILON {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "显示器 {} rotation={} 暂不支持",
                    display.metadata.id, display.metadata.rotation
                ),
            ));
        }
    }
    Ok(())
}

fn build_virtual_bounds(rects: impl IntoIterator<Item = LogicalRect>) -> io::Result<LogicalRect> {
    let mut iter = rects.into_iter();
    let first = iter.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "没有可用于计算 virtual bounds 的显示器",
        )
    })?;

    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_right = first.right();
    let mut max_bottom = first.bottom();

    for rect in iter {
        min_x = min_x.min(rect.x);
        min_y = min_y.min(rect.y);
        max_right = max_right.max(rect.right());
        max_bottom = max_bottom.max(rect.bottom());
    }

    let width = u32::try_from(max_right - i64::from(min_x)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "virtual bounds 宽度超出 u32 范围",
        )
    })?;
    let height = u32::try_from(max_bottom - i64::from(min_y)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "virtual bounds 高度超出 u32 范围",
        )
    })?;

    if width == 0 || height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "virtual bounds 尺寸必须大于 0",
        ));
    }

    Ok(LogicalRect {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn os_rect_to_image_rect(
    os_rect: LogicalRect,
    virtual_bounds: LogicalRect,
) -> io::Result<LogicalRect> {
    let x = os_rect
        .x
        .checked_sub(virtual_bounds.x)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "image_rect.x 计算溢出"))?;
    let y = os_rect
        .y
        .checked_sub(virtual_bounds.y)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "image_rect.y 计算溢出"))?;

    Ok(LogicalRect {
        x,
        y,
        width: os_rect.width,
        height: os_rect.height,
    })
}

fn resize_display_image_to_logical_rect(image: &RgbaImage, image_rect: LogicalRect) -> RgbaImage {
    if image.width() == image_rect.width && image.height() == image_rect.height {
        return image.clone();
    }

    resize(
        image,
        image_rect.width,
        image_rect.height,
        FilterType::Triangle,
    )
}

fn compute_gap_rects(
    virtual_bounds: LogicalRect,
    display_rects: &[LogicalRect],
) -> io::Result<Vec<LogicalRect>> {
    let mut x_edges = vec![
        virtual_bounds.x,
        checked_i64_to_i32(virtual_bounds.right())?,
    ];
    let mut y_edges = vec![
        virtual_bounds.y,
        checked_i64_to_i32(virtual_bounds.bottom())?,
    ];

    for rect in display_rects {
        x_edges.push(rect.x);
        x_edges.push(checked_i64_to_i32(rect.right())?);
        y_edges.push(rect.y);
        y_edges.push(checked_i64_to_i32(rect.bottom())?);
    }

    x_edges.sort_unstable();
    x_edges.dedup();
    y_edges.sort_unstable();
    y_edges.dedup();

    let mut gaps = Vec::new();
    for x_pair in x_edges.windows(2) {
        for y_pair in y_edges.windows(2) {
            let x = x_pair[0];
            let y = y_pair[0];
            let width =
                u32::try_from(i64::from(x_pair[1]) - i64::from(x_pair[0])).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "gap 宽度超出 u32 范围")
                })?;
            let height =
                u32::try_from(i64::from(y_pair[1]) - i64::from(y_pair[0])).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "gap 高度超出 u32 范围")
                })?;

            if width == 0 || height == 0 {
                continue;
            }

            let cell = LogicalRect {
                x,
                y,
                width,
                height,
            };
            if !virtual_bounds.contains_rect(cell) {
                continue;
            }
            if !display_rects
                .iter()
                .any(|display| display.contains_rect(cell))
            {
                gaps.push(os_rect_to_image_rect(cell, virtual_bounds)?);
            }
        }
    }

    Ok(gaps)
}

fn checked_i64_to_i32(value: i64) -> io::Result<i32> {
    i32::try_from(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "坐标超出 i32 范围,无法生成稳定 manifest",
        )
    })
}

fn render_primary_screenshot_success_response(request_id: Option<u64>) -> String {
    match request_id {
        Some(request_id) => format!(r#"@response {{"id":{request_id},"value":0}}"#),
        None => "@response 0".to_owned(),
    }
}

fn render_screenshot_bundle_response(
    request_id: Option<u64>,
    image_filename: &str,
    manifest_filename: &str,
    display_count: usize,
) -> io::Result<String> {
    let summary = ScreenshotBundleResponse {
        kind: "screenshot-bundle",
        layout: "composite",
        coordinate_space: "os-logical",
        image: image_filename,
        manifest: manifest_filename,
        display_count,
    };
    let value = serde_json::to_string(&summary)
        .map_err(|err| io::Error::other(format!("screenshot response 序列化失败: {err}")))?;

    Ok(match request_id {
        Some(request_id) => format!(r#"@response {{"id":{request_id},"value":{value}}}"#),
        None => format!("@response {value}"),
    })
}

fn encode_jpeg(image: &RgbaImage, quality: u8) -> io::Result<Vec<u8>> {
    let mut encoded = Vec::new();
    let dynamic = DynamicImage::ImageRgba8(image.clone());
    let mut encoder = JpegEncoder::new_with_quality(&mut encoded, quality);
    encoder
        .encode_image(&dynamic)
        .map_err(|err| io::Error::other(format!("jpeg 编码失败: {err}")))?;
    Ok(encoded)
}

#[cfg(target_os = "macos")]
fn capture_primary_display_image() -> io::Result<RgbaImage> {
    ensure_screen_recording_permission()?;
    capture_primary_with_sck_rs().or_else(|primary_err| {
        if primary_err.kind() == io::ErrorKind::PermissionDenied {
            return Err(primary_err);
        }
        capture_primary_with_xcap().map_err(|fallback_err| {
            io::Error::new(
                classify_capture_error(&primary_err, &fallback_err),
                format!("sck-rs 截图失败: {primary_err}; xcap fallback 也失败: {fallback_err}"),
            )
        })
    })
}

#[cfg(target_os = "macos")]
fn capture_all_display_images() -> io::Result<Vec<CapturedDisplay>> {
    ensure_screen_recording_permission()?;
    capture_all_with_sck_rs().or_else(|primary_err| {
        if primary_err.kind() == io::ErrorKind::PermissionDenied {
            return Err(primary_err);
        }
        capture_all_with_xcap().map_err(|fallback_err| {
            io::Error::new(
                classify_capture_error(&primary_err, &fallback_err),
                format!(
                    "sck-rs 多显示器截图失败: {primary_err}; xcap fallback 也失败: {fallback_err}"
                ),
            )
        })
    })
}

#[cfg(target_os = "macos")]
fn capture_primary_with_sck_rs() -> io::Result<RgbaImage> {
    let monitor = sck_rs::Monitor::primary().map_err(map_capture_error)?;
    monitor.capture_image().map_err(map_capture_error)
}

#[cfg(target_os = "macos")]
fn capture_all_with_sck_rs() -> io::Result<Vec<CapturedDisplay>> {
    let monitors = sck_rs::Monitor::all().map_err(map_capture_error)?;
    let mut displays = Vec::with_capacity(monitors.len());

    for monitor in monitors {
        let image = monitor.capture_image().map_err(map_capture_error)?;
        let metadata = CapturedDisplayMetadata {
            id: monitor.id().to_string(),
            name: monitor.name().to_owned(),
            is_primary: monitor.is_primary(),
            backend: ScreenshotBackend::SckRs,
            os_rect: LogicalRect {
                x: monitor.x(),
                y: monitor.y(),
                width: monitor.logical_width(),
                height: monitor.logical_height(),
            },
            native_capture_size: Size {
                width: image.width(),
                height: image.height(),
            },
            scale_factor: monitor.scale_factor() as f32,
            rotation: 0.0,
        };
        displays.push(CapturedDisplay { metadata, image });
    }

    Ok(displays)
}

#[cfg(target_os = "macos")]
fn capture_primary_with_xcap() -> io::Result<RgbaImage> {
    let monitors = xcap::Monitor::all().map_err(map_capture_error)?;
    let monitor = monitors
        .into_iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "xcap 未找到主显示器"))?;
    monitor.capture_image().map_err(map_capture_error)
}

#[cfg(not(target_os = "macos"))]
fn capture_primary_display_image() -> io::Result<RgbaImage> {
    let monitors = xcap::Monitor::all().map_err(map_capture_error)?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .cloned()
        .or_else(|| monitors.into_iter().next())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "xcap 未找到可截图显示器"))?;
    monitor.capture_image().map_err(map_capture_error)
}

#[cfg(not(target_os = "macos"))]
fn capture_all_display_images() -> io::Result<Vec<CapturedDisplay>> {
    capture_all_with_xcap()
}

fn capture_all_with_xcap() -> io::Result<Vec<CapturedDisplay>> {
    let monitors = xcap::Monitor::all().map_err(map_capture_error)?;
    let mut displays = Vec::with_capacity(monitors.len());

    for monitor in monitors {
        let id = monitor.id().map_err(map_capture_error)?.to_string();
        let name = monitor
            .name()
            .or_else(|_| monitor.friendly_name())
            .unwrap_or_else(|_| format!("Display {id}"));
        let rotation = monitor.rotation().map_err(map_capture_error)?;
        if rotation.abs() > f32::EPSILON {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("显示器 {id} rotation={rotation} 暂不支持"),
            ));
        }
        let image = monitor.capture_image().map_err(map_capture_error)?;
        let metadata = CapturedDisplayMetadata {
            id,
            name,
            is_primary: monitor.is_primary().unwrap_or(false),
            backend: ScreenshotBackend::Xcap,
            os_rect: LogicalRect {
                x: monitor.x().map_err(map_capture_error)?,
                y: monitor.y().map_err(map_capture_error)?,
                width: monitor.width().map_err(map_capture_error)?,
                height: monitor.height().map_err(map_capture_error)?,
            },
            native_capture_size: Size {
                width: image.width(),
                height: image.height(),
            },
            scale_factor: monitor.scale_factor().unwrap_or(1.0).max(1.0),
            rotation,
        };
        displays.push(CapturedDisplay { metadata, image });
    }

    Ok(displays)
}

#[cfg(target_os = "macos")]
fn ensure_screen_recording_permission() -> io::Result<()> {
    if preflight_screen_recording_permission() {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Screen Recording permission denied for rdog process",
    ))
}

#[cfg(target_os = "macos")]
fn preflight_screen_recording_permission() -> bool {
    unsafe { cg_preflight_screen_capture_access() }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    #[link_name = "CGPreflightScreenCaptureAccess"]
    fn cg_preflight_screen_capture_access() -> bool;
}

fn map_capture_error<E: std::fmt::Display>(err: E) -> io::Error {
    let message = err.to_string();
    let lowered = message.to_ascii_lowercase();
    let kind = if lowered.contains("permission")
        || lowered.contains("screen recording")
        || lowered.contains("not authorized")
        || lowered.contains("denied")
    {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, message)
}

#[cfg(target_os = "macos")]
fn classify_capture_error(primary_err: &io::Error, fallback_err: &io::Error) -> io::ErrorKind {
    if primary_err.kind() == io::ErrorKind::PermissionDenied
        || fallback_err.kind() == io::ErrorKind::PermissionDenied
    {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::Other
    }
}

#[cfg(target_os = "macos")]
fn backend_policy_for_current_platform() -> &'static str {
    "sck-rs-then-xcap"
}

#[cfg(not(target_os = "macos"))]
fn backend_policy_for_current_platform() -> &'static str {
    "xcap"
}

fn current_unix_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
