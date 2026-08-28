use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{
    codecs::jpeg::JpegEncoder,
    imageops::{crop_imm, overlay, resize, FilterType},
    DynamicImage, Rgba, RgbaImage,
};
use serde::Serialize;
use std::{
    io,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use crate::{
    ax_query::{capture_default_ax_snapshot, current_ax_platform},
    control_ax::{resolve_current_ax_target_rect, AxSnapshot, AxTarget, AxTreeRequest},
    control_display_scope::{
        resolve_display_scope, resolve_observation_window_ref, DisplayRect, DisplayScope,
        DisplayScopeResolution, DisplaySelector, DisplaySummary, DISPLAY_ID_STABILITY_SESSION,
    },
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_protocol::{
        ScreenshotCoordinateSpace, ScreenshotDisplaySelector, ScreenshotLayout, ScreenshotRequest,
        ScreenshotTarget, ScreenshotWindowTarget,
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
    match request.target {
        ScreenshotTarget::Display => match request.display {
            ScreenshotDisplaySelector::All => {
                execute_composite_screenshot_request_with_capture_ax_and_freshness(
                    request_id,
                    request,
                    || capture_all_display_images(),
                    |ax_request| capture_default_ax_snapshot(ax_request),
                    reject_stale_composite_capture,
                )
            }
            ScreenshotDisplaySelector::Primary => {
                execute_primary_screenshot_request_with_capture(request_id, request, || {
                    capture_primary_display_image()
                })
            }
        },
        ScreenshotTarget::Window => execute_window_screenshot_request(request_id, request),
    }
}

/// 窗口截图使用当前 screenshot backend 的真实桌面像素。
///
/// 这不是 ScreenCaptureKit 的独立 window surface capture。macOS 不允许我们把
/// 被其他窗口遮挡的像素伪造成目标窗口内容,所以 manifest 必须明确标记来源。
fn execute_window_screenshot_request(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
) -> io::Result<ControlExecutionOutcome> {
    let window_id = resolve_screenshot_window_id(request.window.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "窗口截图缺少 window target")
    })?)?;
    let resolved = resolve_current_ax_target_rect(&AxTarget {
        id: Some(window_id.clone()),
        ..AxTarget::default()
    })?;
    if resolved.target_type != "window" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "窗口截图 target 必须是 window id,当前是 {}",
                resolved.target_type
            ),
        ));
    }
    let rect = resolved.rect.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "窗口截图目标没有可用的 AX rect,无法裁剪显示器图像",
        )
    })?;

    execute_window_screenshot_request_with_capture(
        request_id,
        request,
        window_id,
        LogicalRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        },
        capture_all_display_images,
        reject_stale_composite_capture,
    )
}

fn resolve_screenshot_window_id(target: &ScreenshotWindowTarget) -> io::Result<String> {
    if let Some(window_id) = target.window_id.as_ref() {
        return Ok(window_id.clone());
    }
    let observation_id = target.observation_id.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "窗口截图 ref 缺少 observation_id",
        )
    })?;
    let ref_id = target.ref_id.as_deref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "窗口截图缺少 window_id 或 ref")
    })?;
    Ok(resolve_observation_window_ref(observation_id, ref_id)?.window_id)
}

fn execute_window_screenshot_request_with_capture<C, S>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    window_id: String,
    requested_rect: LogicalRect,
    capture: C,
    freshness_check: S,
) -> io::Result<ControlExecutionOutcome>
where
    C: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
    S: FnOnce(&[CapturedDisplay]) -> io::Result<()>,
{
    validate_window_request(request)?;
    let displays = capture()?;
    freshness_check(&displays)?;
    build_window_screenshot_outcome_with_id(
        request_id,
        request,
        displays,
        &window_id,
        requested_rect,
        &current_unix_epoch_millis().to_string(),
    )
}

/// 生成 screenshot bundle 的文件 frame 和轻量摘要。
///
/// `@observe` 复用这个入口,避免反解析 `@screenshot` 的 response 文本。
/// resolved display id 存在时,从同一次 capture 中选择单块 display。
/// 这里只返回 `@savefile` frames,最终 `@response` 由调用方自己组织。
pub struct ScreenshotBundleExecutionResult {
    pub frames: Vec<ControlFrame>,
    pub summary: ScreenshotBundleSummary,
    pub display_scope_resolution: Option<DisplayScopeResolution>,
}

pub fn execute_screenshot_bundle_request<W>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    display_scope: Option<&DisplayScope>,
    window_rect_for_selector: W,
) -> io::Result<ScreenshotBundleExecutionResult>
where
    W: FnMut(&DisplaySelector) -> io::Result<Option<DisplayRect>>,
{
    execute_screenshot_bundle_request_with(
        ScreenshotBundleExecution {
            request_id,
            request,
            display_scope,
        },
        capture_all_display_images,
        window_rect_for_selector,
        reject_stale_composite_capture,
    )
}

struct ScreenshotBundleExecution<'a> {
    request_id: Option<u64>,
    request: &'a ScreenshotRequest,
    display_scope: Option<&'a DisplayScope>,
}

fn execute_screenshot_bundle_request_with<C, W, S>(
    execution: ScreenshotBundleExecution<'_>,
    capture: C,
    mut window_rect_for_selector: W,
    freshness_check: S,
) -> io::Result<ScreenshotBundleExecutionResult>
where
    C: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
    W: FnMut(&DisplaySelector) -> io::Result<Option<DisplayRect>>,
    S: FnOnce(&[CapturedDisplay]) -> io::Result<()>,
{
    validate_composite_request(execution.request)?;
    let displays = capture()?;
    freshness_check(&displays)?;
    let display_scope_resolution = execution
        .display_scope
        .map(|scope| {
            let summaries = display_summaries_from_captured(&displays)?;
            resolve_display_scope(scope, &summaries, &mut window_rect_for_selector)
        })
        .transpose()?;
    let screenshot_id = current_unix_epoch_millis().to_string();
    let accessibility =
        build_accessibility_manifest(execution.request, capture_default_ax_snapshot)?;
    let (displays, layout) = if let Some(resolution) = display_scope_resolution.as_ref() {
        (
            vec![select_captured_display(
                displays,
                &resolution.resolved.display_id,
            )?],
            ScreenshotBundleLayout::SingleDisplay,
        )
    } else {
        (displays, ScreenshotBundleLayout::Composite)
    };
    let (frames, summary) = build_screenshot_parts_with_id_and_ax(
        execution.request_id,
        execution.request,
        displays,
        &screenshot_id,
        accessibility,
        layout,
    )?;
    Ok(ScreenshotBundleExecutionResult {
        frames,
        summary,
        display_scope_resolution,
    })
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

#[cfg(test)]
fn execute_composite_screenshot_request_with_capture<F>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
{
    execute_composite_screenshot_request_with_capture_ax_and_freshness(
        request_id,
        request,
        capture,
        |ax_request| capture_default_ax_snapshot(ax_request),
        |_| Ok(()),
    )
}

#[cfg(test)]
fn execute_composite_screenshot_request_with_capture_and_ax<F, A>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
    capture_ax: A,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
    A: FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
{
    execute_composite_screenshot_request_with_capture_ax_and_freshness(
        request_id,
        request,
        capture,
        capture_ax,
        |_| Ok(()),
    )
}

fn execute_composite_screenshot_request_with_capture_ax_and_freshness<F, A, S>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
    capture_ax: A,
    freshness_check: S,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<Vec<CapturedDisplay>>,
    A: FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
    S: FnOnce(&[CapturedDisplay]) -> io::Result<()>,
{
    validate_composite_request(request)?;
    let displays = capture()?;
    freshness_check(&displays)?;
    let screenshot_id = current_unix_epoch_millis().to_string();
    let accessibility = build_accessibility_manifest(request, capture_ax)?;
    let (mut frames, summary) = build_composite_screenshot_parts_with_id_and_ax(
        request_id,
        request,
        displays,
        &screenshot_id,
        accessibility,
    )?;
    frames.push(ControlFrame::ResponseLine(
        render_screenshot_bundle_response(request_id, &summary)?,
    ));
    Ok(ControlExecutionOutcome {
        outbound_frames: frames,
    })
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

fn build_window_screenshot_outcome_with_id(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    window_id: &str,
    requested_os_rect: LogicalRect,
    screenshot_id: &str,
) -> io::Result<ControlExecutionOutcome> {
    validate_window_request(request)?;
    validate_captured_displays(&displays)?;
    let virtual_bounds =
        build_virtual_bounds(displays.iter().map(|display| display.metadata.os_rect))?;
    let captured_os_rect = requested_os_rect
        .intersection(virtual_bounds)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "窗口截图目标不在当前虚拟桌面范围内",
            )
        })?;

    // 先复用唯一的 composite builder 统一所有 display scale / rotation 处理。
    // 裁剪只在该逻辑坐标已经归一化之后发生,不再另起一套像素换算公式。
    let composite = build_screenshot_bundle_with_ax_and_layout(
        displays,
        screenshot_id,
        None,
        ScreenshotBundleLayout::Composite,
    )?
    .composite;
    let image_rect = os_rect_to_image_rect(captured_os_rect, virtual_bounds)?;
    let image = crop_imm(
        &composite,
        u32::try_from(image_rect.x)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "窗口截图 image x 不应为负"))?,
        u32::try_from(image_rect.y)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "窗口截图 image y 不应为负"))?,
        image_rect.width,
        image_rect.height,
    )
    .to_image();
    let image_filename = format!("screenshot-{screenshot_id}-window.jpg");
    let manifest_filename = format!("screenshot-{screenshot_id}-window-manifest.json");
    let manifest = WindowScreenshotManifest {
        schema: "rdog.screenshot.window.v1",
        screenshot_id,
        coordinate_space: "os-logical",
        image_coordinate_space: "window-logical-pixels",
        capture_status: "complete",
        source: "display-composite-crop",
        visibility: "screen-composited",
        window: WindowScreenshotTargetManifest {
            window_id,
            requested_os_rect,
            captured_os_rect,
            clipped: requested_os_rect != captured_os_rect,
        },
        image_size: Size {
            width: image.width(),
            height: image.height(),
        },
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|err| io::Error::other(format!("窗口截图 manifest 序列化失败: {err}")))?;
    let response_value = serde_json::json!({
        "kind": "window-screenshot",
        "coordinate_space": "os-logical",
        "source": "display-composite-crop",
        "visibility": "screen-composited",
        "window_id": window_id,
        "image": image_filename,
        "manifest": manifest_filename,
        "clipped": requested_os_rect != captured_os_rect,
    });
    let response = match request_id {
        Some(request_id) => format!("@response {{\"id\":{request_id},\"value\":{response_value}}}"),
        None => format!("@response {response_value}"),
    };

    Ok(ControlExecutionOutcome {
        outbound_frames: vec![
            ControlFrame::SaveFile(SaveFileFrame {
                request_id,
                filename: image_filename,
                mime: "image/jpeg".to_owned(),
                encoding: "base64".to_owned(),
                data: BASE64_STANDARD.encode(encode_jpeg(&image, request.quality)?),
                quality: Some(request.quality),
                width: Some(image.width()),
                height: Some(image.height()),
            }),
            ControlFrame::SaveFile(SaveFileFrame {
                request_id,
                filename: manifest_filename,
                mime: "application/json".to_owned(),
                encoding: "base64".to_owned(),
                data: BASE64_STANDARD.encode(manifest_json),
                quality: None,
                width: None,
                height: None,
            }),
            ControlFrame::ResponseLine(response),
        ],
    })
}

#[cfg(test)]
fn build_composite_screenshot_outcome_with_id(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
) -> io::Result<ControlExecutionOutcome> {
    build_composite_screenshot_outcome_with_id_and_ax(
        request_id,
        request,
        displays,
        screenshot_id,
        None,
    )
}

#[cfg(test)]
fn build_composite_screenshot_outcome_with_id_and_ax(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
) -> io::Result<ControlExecutionOutcome> {
    let (mut frames, summary) = build_composite_screenshot_parts_with_id_and_ax(
        request_id,
        request,
        displays,
        screenshot_id,
        accessibility,
    )?;
    frames.push(ControlFrame::ResponseLine(
        render_screenshot_bundle_response(request_id, &summary)?,
    ));
    Ok(ControlExecutionOutcome {
        outbound_frames: frames,
    })
}

fn build_composite_screenshot_parts_with_id_and_ax(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
) -> io::Result<(Vec<ControlFrame>, ScreenshotBundleSummary)> {
    validate_composite_request(request)?;

    build_screenshot_parts_with_id_and_ax(
        request_id,
        request,
        displays,
        screenshot_id,
        accessibility,
        ScreenshotBundleLayout::Composite,
    )
}

fn build_screenshot_parts_with_id_and_ax(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
    layout: ScreenshotBundleLayout,
) -> io::Result<(Vec<ControlFrame>, ScreenshotBundleSummary)> {
    validate_coordinate_space(request)?;

    let bundle =
        build_screenshot_bundle_with_ax_and_layout(displays, screenshot_id, accessibility, layout)?;
    let image_filename = match layout {
        ScreenshotBundleLayout::Composite => {
            format!("screenshot-{screenshot_id}-virtual-desktop.jpg")
        }
        ScreenshotBundleLayout::SingleDisplay => {
            format!("screenshot-{screenshot_id}-display.jpg")
        }
    };
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

    let summary = ScreenshotBundleSummary {
        kind: "screenshot-bundle",
        layout: layout.as_str(),
        coordinate_space: "os-logical",
        image: image_filename,
        manifest: manifest_filename,
        display_count: bundle.display_count,
    };

    Ok((
        vec![
            ControlFrame::SaveFile(image_frame),
            ControlFrame::SaveFile(manifest_frame),
        ],
        summary,
    ))
}

fn build_accessibility_manifest<A>(
    request: &ScreenshotRequest,
    capture_ax: A,
) -> io::Result<Option<AxSnapshot>>
where
    A: FnOnce(&AxTreeRequest) -> io::Result<AxSnapshot>,
{
    if !request.include_ax {
        return Ok(None);
    }

    let ax_request = AxTreeRequest {
        depth: request.ax_depth,
        max_elements: request.ax_max_elements,
        include_values: request.ax_include_values,
        ..AxTreeRequest::default()
    };

    match capture_ax(&ax_request) {
        Ok(snapshot) => Ok(Some(snapshot.with_observation("@screenshot include_ax")?)),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && !request.ax_required => {
            Ok(Some(
                AxSnapshot::permission_denied(current_ax_platform())
                    .with_observation("@screenshot include_ax")?,
            ))
        }
        Err(err) if err.kind() == io::ErrorKind::Unsupported && !request.ax_required => Ok(Some(
            AxSnapshot::unsupported().with_observation("@screenshot include_ax")?,
        )),
        Err(err) => Err(err),
    }
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

fn validate_window_request(request: &ScreenshotRequest) -> io::Result<()> {
    if !matches!(request.target, ScreenshotTarget::Window) || request.window.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "窗口截图需要 target=window 和 window target",
        ));
    }
    if request.include_ax {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "窗口截图暂不支持 include_ax;请分别执行 @ax-tree 或 @ax-get 获取结构化 AX 证据",
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompositeCaptureFingerprint {
    /// 抓帧时刻,用于 stale guard 的 TTL 早退。
    ///
    /// daemon 长跑场景下 `LAST_COMPOSITE_FINGERPRINT` 里存的是 N 小时前的
    /// fingerprint,如果继续跟当前帧做严格比对,会把"用户视角的第一次请求"
    /// 当成 stale 拒掉。带 `captured_at` 之后,gap 超过 cache TTL 就视为
    /// 缓存陈旧,直接放行,不再误判。
    captured_at: Instant,
    display_count: usize,
    display_fingerprints: Vec<DisplayCaptureFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayCaptureFingerprint {
    id: String,
    backend: &'static str,
    os_rect: LogicalRect,
    native_capture_size: Size,
    pixel_hash: u64,
}

static LAST_COMPOSITE_FINGERPRINT: OnceLock<Mutex<Option<CompositeCaptureFingerprint>>> =
    OnceLock::new();

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

    fn intersection(self, other: LogicalRect) -> Option<LogicalRect> {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            return None;
        }
        Some(LogicalRect {
            x: i32::try_from(left).ok()?,
            y: i32::try_from(top).ok()?,
            width: u32::try_from(right - left).ok()?,
            height: u32::try_from(bottom - top).ok()?,
        })
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ScreenshotBundleLayout {
    Composite,
    SingleDisplay,
}

impl ScreenshotBundleLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::Composite => "composite",
            Self::SingleDisplay => "single-display",
        }
    }

    fn image_coordinate_space(self) -> &'static str {
        match self {
            Self::Composite => "virtual-logical-pixels",
            Self::SingleDisplay => "display-logical-pixels",
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    accessibility: Option<AxSnapshot>,
}

#[derive(Serialize)]
struct WindowScreenshotManifest<'a> {
    schema: &'static str,
    screenshot_id: &'a str,
    coordinate_space: &'static str,
    image_coordinate_space: &'static str,
    capture_status: &'static str,
    /// 说明图像来自可见 desktop composite,不是不可遮挡的原生 window surface。
    source: &'static str,
    visibility: &'static str,
    window: WindowScreenshotTargetManifest<'a>,
    image_size: Size,
}

#[derive(Serialize)]
struct WindowScreenshotTargetManifest<'a> {
    window_id: &'a str,
    requested_os_rect: LogicalRect,
    captured_os_rect: LogicalRect,
    clipped: bool,
}

#[derive(Serialize)]
struct ScreenshotTransforms {
    image_to_os: &'static str,
    os_to_image: &'static str,
}

#[derive(Serialize)]
struct DisplayManifest {
    id: String,
    display_id: String,
    display_id_stability: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable_key: Option<String>,
    name: String,
    is_primary: bool,
    primary: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotBundleSummary {
    pub kind: &'static str,
    pub layout: &'static str,
    pub coordinate_space: &'static str,
    pub image: String,
    pub manifest: String,
    pub display_count: usize,
}

#[cfg(test)]
fn build_screenshot_bundle(
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
) -> io::Result<ScreenshotBundle> {
    build_screenshot_bundle_with_ax(displays, screenshot_id, None)
}

#[cfg(test)]
fn build_scoped_screenshot_bundle(
    displays: Vec<CapturedDisplay>,
    display_id: &str,
    screenshot_id: &str,
) -> io::Result<ScreenshotBundle> {
    let display = select_captured_display(displays, display_id)?;
    build_screenshot_bundle_with_ax_and_layout(
        vec![display],
        screenshot_id,
        None,
        ScreenshotBundleLayout::SingleDisplay,
    )
}

#[cfg(test)]
fn build_screenshot_bundle_with_ax(
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
) -> io::Result<ScreenshotBundle> {
    build_screenshot_bundle_with_ax_and_layout(
        displays,
        screenshot_id,
        accessibility,
        ScreenshotBundleLayout::Composite,
    )
}

fn build_screenshot_bundle_with_ax_and_layout(
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
    layout: ScreenshotBundleLayout,
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
            id: display.metadata.id.clone(),
            display_id: display.metadata.id.clone(),
            display_id_stability: DISPLAY_ID_STABILITY_SESSION,
            stable_key: Some(format!(
                "{}:{}",
                display.metadata.backend.as_str(),
                display.metadata.id
            )),
            name: display.metadata.name,
            is_primary: display.metadata.is_primary,
            primary: display.metadata.is_primary,
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
        layout: layout.as_str(),
        coordinate_space: "os-logical",
        image_coordinate_space: layout.image_coordinate_space(),
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
        accessibility,
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

fn select_captured_display(
    displays: Vec<CapturedDisplay>,
    display_id: &str,
) -> io::Result<CapturedDisplay> {
    displays
        .into_iter()
        .find(|display| display.metadata.id == display_id)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("截图结果中不存在 resolved display id: {display_id}"),
            )
        })
}

pub fn current_display_summaries() -> io::Result<Vec<DisplaySummary>> {
    let displays = enumerate_display_metadata()?;
    display_summaries_from_metadata(&displays)
}

fn display_summaries_from_captured(
    displays: &[CapturedDisplay],
) -> io::Result<Vec<DisplaySummary>> {
    validate_captured_displays(displays)?;
    let metadata = displays
        .iter()
        .map(|display| display.metadata.clone())
        .collect::<Vec<_>>();
    display_summaries_from_metadata(&metadata)
}

fn display_summaries_from_metadata(
    displays: &[CapturedDisplayMetadata],
) -> io::Result<Vec<DisplaySummary>> {
    let virtual_bounds = build_virtual_bounds(displays.iter().map(|display| display.os_rect))?;
    displays
        .iter()
        .map(|metadata| {
            let image_rect = os_rect_to_image_rect(metadata.os_rect, virtual_bounds)?;
            Ok(display_summary_from_metadata(metadata, image_rect))
        })
        .collect()
}

/// 枚举 display catalog,但不抓取任何屏幕像素。
///
/// display scope/guard 只需要 id、名称和全局 rect。它们不能隐式要求
/// Screen Recording 权限,更不能为了校验一个坐标启动 ScreenCaptureKit。
fn enumerate_display_metadata() -> io::Result<Vec<CapturedDisplayMetadata>> {
    let monitors = xcap::Monitor::all().map_err(map_capture_error)?;
    let mut displays = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        let id = monitor.id().map_err(map_capture_error)?.to_string();
        // macOS xcap 的 `name()` 可能只是 "Display #<model>"。
        // selector需要用户可见名称,因此先取friendly name,失败后才回退generic name。
        let name = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| format!("Display {id}"));
        let width = monitor.width().map_err(map_capture_error)?;
        let height = monitor.height().map_err(map_capture_error)?;
        let rotation = monitor.rotation().map_err(map_capture_error)?;
        if rotation.abs() > f32::EPSILON {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("显示器 {id} rotation={rotation} 暂不支持"),
            ));
        }
        displays.push(CapturedDisplayMetadata {
            id,
            name,
            is_primary: monitor.is_primary().unwrap_or(false),
            backend: ScreenshotBackend::Xcap,
            os_rect: LogicalRect {
                x: monitor.x().map_err(map_capture_error)?,
                y: monitor.y().map_err(map_capture_error)?,
                width,
                height,
            },
            // catalog 不消费 native size。保留逻辑尺寸,避免为获得像素尺寸触发 capture。
            native_capture_size: Size { width, height },
            scale_factor: monitor.scale_factor().unwrap_or(1.0).max(1.0),
            rotation,
        });
    }
    if displays.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "没有可用 display metadata",
        ));
    }
    Ok(displays)
}

fn display_summary_from_metadata(
    metadata: &CapturedDisplayMetadata,
    image_rect: LogicalRect,
) -> DisplaySummary {
    DisplaySummary {
        display_id: metadata.id.clone(),
        stable_key: Some(format!("{}:{}", metadata.backend.as_str(), metadata.id)),
        primary: metadata.is_primary,
        name: metadata.name.clone(),
        os_rect: DisplayRect {
            x: metadata.os_rect.x,
            y: metadata.os_rect.y,
            width: metadata.os_rect.width,
            height: metadata.os_rect.height,
        },
        image_rect: DisplayRect {
            x: image_rect.x,
            y: image_rect.y,
            width: image_rect.width,
            height: image_rect.height,
        },
        scale_factor: metadata.scale_factor,
        rotation: metadata.rotation,
        display_id_stability: DISPLAY_ID_STABILITY_SESSION,
    }
}

fn reject_stale_composite_capture(displays: &[CapturedDisplay]) -> io::Result<()> {
    let fingerprint = composite_capture_fingerprint(displays);
    let cache = LAST_COMPOSITE_FINGERPRINT.get_or_init(|| Mutex::new(None));
    let mut last = cache
        .lock()
        .map_err(|_| io::Error::other("screenshot freshness cache lock poisoned"))?;

    reject_stale_composite_fingerprint(fingerprint, &mut last)
}

fn reject_stale_composite_fingerprint(
    fingerprint: CompositeCaptureFingerprint,
    last: &mut Option<CompositeCaptureFingerprint>,
) -> io::Result<()> {
    // stale guard 的 cache TTL:超过这个间隔就视为缓存陈旧,直接放行。
    //
    // daemon 长跑(N 小时~N 天)期间,`LAST_COMPOSITE_FINGERPRINT` 里存的是
    // 上一次请求的 fingerprint。SCK 抓帧 + WindowServer 没标 dirty 时
    // composite hash 可能跨请求不变,如果严格比对,会把"用户视角的第一次
    // 请求"误判成 stale 而拒掉。带时间窗口后,长间隔请求一律放行,
    // 短间隔(用户连续多次 observe)撞 hash 才走真正的 stale 检测。
    const CACHE_TTL: Duration = Duration::from_secs(30);

    if let Some(prev) = last.take() {
        // 取出旧值后再判断,避免 borrow 和赋值冲突。
        let gap = fingerprint
            .captured_at
            .saturating_duration_since(prev.captured_at);

        // 无论后续走哪条分支,`last` 都要更新成最新的 fingerprint,
        // 提前把当前 fingerprint 放回去。
        *last = Some(fingerprint);

        // 长间隔 → 缓存已陈旧,直接放行(用户视角的"第一次请求"场景)
        if gap >= CACHE_TTL {
            return Ok(());
        }

        // 短间隔 + 不同 hash → 屏确实变了,放行
        if prev.display_fingerprints != last.as_ref().expect("just set above").display_fingerprints
        {
            return Ok(());
        }

        // 短间隔 + 同 hash → 才是真正可疑的 stale
        let payload = stale_screenshot_error_payload(last.as_ref().expect("just set above"))?;
        return Err(io::Error::other(payload));
    }

    *last = Some(fingerprint);
    Ok(())
}

fn composite_capture_fingerprint(displays: &[CapturedDisplay]) -> CompositeCaptureFingerprint {
    CompositeCaptureFingerprint {
        captured_at: Instant::now(),
        display_count: displays.len(),
        display_fingerprints: displays.iter().map(display_capture_fingerprint).collect(),
    }
}

fn display_capture_fingerprint(display: &CapturedDisplay) -> DisplayCaptureFingerprint {
    DisplayCaptureFingerprint {
        id: display.metadata.id.clone(),
        backend: display.metadata.backend.as_str(),
        os_rect: display.metadata.os_rect,
        native_capture_size: display.metadata.native_capture_size,
        pixel_hash: rgba_image_hash(&display.image),
    }
}

fn rgba_image_hash(image: &RgbaImage) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    update_fnv1a_u32(&mut hash, image.width());
    update_fnv1a_u32(&mut hash, image.height());
    update_fnv1a_bytes(&mut hash, image.as_raw());
    hash
}

fn update_fnv1a_u32(hash: &mut u64, value: u32) {
    update_fnv1a_bytes(hash, &value.to_le_bytes());
}

fn update_fnv1a_bytes(hash: &mut u64, bytes: &[u8]) {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn stale_screenshot_error_payload(fingerprint: &CompositeCaptureFingerprint) -> io::Result<String> {
    #[derive(Serialize)]
    struct StaleScreenshotError<'a> {
        kind: &'static str,
        error_code: &'static str,
        error: &'static str,
        guard_policy: &'static str,
        backend_policy: &'static str,
        display_count: usize,
        displays: Vec<StaleDisplayReport<'a>>,
        recovery_hint: &'static str,
    }

    #[derive(Serialize)]
    struct StaleDisplayReport<'a> {
        id: &'a str,
        backend: &'a str,
        os_rect: LogicalRect,
        native_capture_size: Size,
        pixel_hash: String,
    }

    let displays = fingerprint
        .display_fingerprints
        .iter()
        .map(|display| StaleDisplayReport {
            id: &display.id,
            backend: display.backend,
            os_rect: display.os_rect,
            native_capture_size: display.native_capture_size,
            pixel_hash: format!("{:016x}", display.pixel_hash),
        })
        .collect();

    let report = StaleScreenshotError {
        kind: "screenshot-stale-frame",
        error_code: "SCREENSHOT_STALE_FRAME",
        error: "连续两次 composite screenshot 捕获到完全相同的显示器布局和像素指纹,疑似截图后端返回旧帧",
        guard_policy: "reject-consecutive-identical-composite-fingerprint",
        backend_policy: backend_policy_for_current_platform(),
        display_count: fingerprint.display_count,
        displays,
        recovery_hint: "保留现场后检查截图后端状态; 可重启 daemon 验证是否由 long-running capture backend stale 引起",
    };

    serde_json::to_string(&report)
        .map_err(|err| io::Error::other(format!("stale screenshot error 序列化失败: {err}")))
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
    summary: &ScreenshotBundleSummary,
) -> io::Result<String> {
    let summary = ScreenshotBundleResponse {
        kind: summary.kind,
        layout: summary.layout,
        coordinate_space: summary.coordinate_space,
        image: &summary.image,
        manifest: &summary.manifest,
        display_count: summary.display_count,
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
    const CAPTURE_KIND: &str = "primary";
    ensure_screen_recording_permission(CAPTURE_KIND)?;
    capture_with_sck_fallback(
        CAPTURE_KIND,
        "截图",
        || {
            capture_with_timeout(
                CAPTURE_KIND,
                "sck-rs",
                &SCK_CAPTURE_IN_FLIGHT,
                MACOS_CAPTURE_TIMEOUT,
                capture_primary_with_sck_rs,
            )
        },
        || {
            capture_with_timeout(
                CAPTURE_KIND,
                "xcap",
                &XCAP_CAPTURE_IN_FLIGHT,
                MACOS_CAPTURE_TIMEOUT,
                capture_primary_with_xcap,
            )
        },
    )
}

#[cfg(target_os = "macos")]
fn capture_all_display_images() -> io::Result<Vec<CapturedDisplay>> {
    const CAPTURE_KIND: &str = "all";
    ensure_screen_recording_permission(CAPTURE_KIND)?;
    capture_with_sck_fallback(
        CAPTURE_KIND,
        "多显示器截图",
        || {
            capture_with_timeout(
                CAPTURE_KIND,
                "sck-rs",
                &SCK_CAPTURE_IN_FLIGHT,
                MACOS_CAPTURE_TIMEOUT,
                capture_all_with_sck_rs,
            )
        },
        || {
            capture_with_timeout(
                CAPTURE_KIND,
                "xcap",
                &XCAP_CAPTURE_IN_FLIGHT,
                MACOS_CAPTURE_TIMEOUT,
                capture_all_with_xcap,
            )
        },
    )
}

/// 统一 macOS 的 SCK -> xcap 策略,让 primary 与 all-display 的 fallback 和日志字段一致。
#[cfg(target_os = "macos")]
fn capture_with_sck_fallback<T>(
    capture_kind: &'static str,
    capture_label: &'static str,
    primary_capture: impl FnOnce() -> io::Result<T>,
    fallback_capture: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let primary_started = Instant::now();
    match primary_capture() {
        Ok(result) => Ok(result),
        Err(primary_err) if primary_err.kind() == io::ErrorKind::PermissionDenied => {
            trace_capture_permission_denied(capture_kind, "sck-rs", &primary_err);
            Err(primary_err)
        }
        Err(primary_err) => {
            tracing::info!(
                target: "rustdog.screenshot",
                event_name = "screenshot_capture_fallback",
                capture_kind,
                failed_backend = "sck-rs",
                fallback_backend = "xcap",
                primary_error_kind = ?primary_err.kind(),
                primary_elapsed_ms = primary_started.elapsed().as_millis() as u64,
                primary_error = %primary_err,
                "native screenshot capture falling back to xcap"
            );

            let fallback_started = Instant::now();
            match fallback_capture() {
                Ok(result) => Ok(result),
                Err(fallback_err) => {
                    let final_error_kind = classify_capture_error(&primary_err, &fallback_err);
                    if final_error_kind == io::ErrorKind::PermissionDenied {
                        trace_capture_permission_denied(capture_kind, "xcap", &fallback_err);
                    } else {
                        tracing::error!(
                            target: "rustdog.screenshot",
                            event_name = "screenshot_capture_failed",
                            capture_kind,
                            primary_backend = "sck-rs",
                            fallback_backend = "xcap",
                            primary_error_kind = ?primary_err.kind(),
                            fallback_error_kind = ?fallback_err.kind(),
                            final_error_kind = ?final_error_kind,
                            fallback_elapsed_ms = fallback_started.elapsed().as_millis() as u64,
                            primary_error = %primary_err,
                            fallback_error = %fallback_err,
                            "all native screenshot backends failed"
                        );
                    }

                    Err(io::Error::new(
                        final_error_kind,
                        format!(
                            "sck-rs {capture_label}失败: {primary_err}; xcap fallback 也失败: {fallback_err}"
                        ),
                    ))
                }
            }
        }
    }
}

/// macOS 原生 capture 没有可用的取消句柄,因此必须在 daemon 外层设定返回期限。
#[cfg(target_os = "macos")]
const MACOS_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// 每个 backend 最多允许一个未返回的 native capture worker。
///
/// `capture_image` 超时后无法安全地强制终止 Objective-C 调用。保留这个标记,
/// 让后续请求立刻走下一个 backend,而不是为同一个卡住的 backend 持续创建线程。
#[cfg(target_os = "macos")]
static SCK_CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static XCAP_CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 在独立线程中执行可能阻塞的 native capture,并将控制面等待时间限制为指定期限。
///
/// ponytail: 原生 API 没有取消接口,超时 worker 可能存活到 API 自行返回;
/// 每个 backend 的 atomic gate 把上限固定为一个 worker,未来依赖支持取消后可替换为 handle cancel。
#[cfg(target_os = "macos")]
fn capture_with_timeout<T>(
    capture_kind: &'static str,
    backend: &'static str,
    in_flight: &'static AtomicBool,
    timeout: Duration,
    capture: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T>
where
    T: Send + 'static,
{
    if in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        let error = io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{backend} screenshot capture 的上一轮仍未返回"),
        );
        trace_capture_timeout(capture_kind, backend, timeout, "previous_capture_in_flight");
        return Err(error);
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name(format!("rdog-{backend}-capture"))
        .spawn(move || {
            let result = capture();
            in_flight.store(false, Ordering::Release);
            let _ = sender.send(result);
        })
        .map_err(|err| {
            in_flight.store(false, Ordering::Release);
            io::Error::other(format!("无法启动 {backend} screenshot worker: {err}"))
        })?;

    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(err) => {
            let (message, reason) = match err {
                mpsc::RecvTimeoutError::Timeout => (
                    format!(
                        "{backend} screenshot capture 超时: {} ms",
                        timeout.as_millis()
                    ),
                    "deadline_exceeded",
                ),
                mpsc::RecvTimeoutError::Disconnected => (
                    format!("{backend} screenshot worker 在返回结果前断开"),
                    "worker_disconnected",
                ),
            };
            trace_capture_timeout(capture_kind, backend, timeout, reason);
            Err(io::Error::new(io::ErrorKind::TimedOut, message))
        }
    }
}

/// 记录 native backend 没有在控制面等待期限内返回的事实,包括无法新建 worker 的 gate 命中。
#[cfg(target_os = "macos")]
fn trace_capture_timeout(
    capture_kind: &'static str,
    backend: &'static str,
    timeout: Duration,
    timeout_reason: &'static str,
) {
    tracing::warn!(
        target: "rustdog.screenshot",
        event_name = "screenshot_capture_timeout",
        capture_kind,
        backend,
        timeout_ms = timeout.as_millis() as u64,
        timeout_reason,
        "native screenshot capture timed out"
    );
}

/// 权限拒绝是终态类别。单独记录它,避免被泛化为普通 backend 失败而触发无意义重试。
#[cfg(target_os = "macos")]
fn trace_capture_permission_denied(
    capture_kind: &'static str,
    source: &'static str,
    error: &io::Error,
) {
    tracing::warn!(
        target: "rustdog.screenshot",
        event_name = "screenshot_capture_permission_denied",
        capture_kind,
        source,
        error_kind = ?error.kind(),
        error = %error,
        "macOS Screen Recording permission denied"
    );
}

#[cfg(target_os = "macos")]
fn capture_primary_with_sck_rs() -> io::Result<RgbaImage> {
    let monitor = sck_rs::Monitor::primary().map_err(map_capture_error)?;
    monitor.capture_image().map_err(map_capture_error)
}

#[cfg(target_os = "macos")]
fn capture_all_with_sck_rs() -> io::Result<Vec<CapturedDisplay>> {
    let monitors = sck_rs::Monitor::all().map_err(map_capture_error)?;
    // SCK 的 `name()` 在部分机器上只返回 "Display N"。
    // 用相同display id关联metadata catalog的friendly name,保持scope与manifest一致。
    let display_catalog = enumerate_display_metadata().unwrap_or_default();
    let mut displays = Vec::with_capacity(monitors.len());

    for monitor in monitors {
        let id = monitor.id().to_string();
        let image = monitor.capture_image().map_err(map_capture_error)?;
        let metadata = CapturedDisplayMetadata {
            name: display_catalog
                .iter()
                .find(|display| display.id == id)
                .map(|display| display.name.clone())
                .unwrap_or_else(|| monitor.name().to_owned()),
            id,
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
            .friendly_name()
            .or_else(|_| monitor.name())
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
fn ensure_screen_recording_permission(capture_kind: &'static str) -> io::Result<()> {
    if preflight_screen_recording_permission() {
        return Ok(());
    }

    let error = io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Screen Recording permission denied for rdog process",
    );
    trace_capture_permission_denied(capture_kind, "preflight", &error);
    Err(error)
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
fn reset_screenshot_freshness_cache_for_tests() {
    let cache = LAST_COMPOSITE_FINGERPRINT.get_or_init(|| Mutex::new(None));
    if let Ok(mut last) = cache.lock() {
        *last = None;
    }
}

#[cfg(test)]
mod tests;
