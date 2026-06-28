use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{
    codecs::jpeg::JpegEncoder,
    imageops::{overlay, resize, FilterType},
    DynamicImage, Rgba, RgbaImage,
};
use serde::Serialize;
use std::{
    io,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    control_ax::{capture_default_ax_snapshot, current_ax_platform, AxSnapshot, AxTreeRequest},
    control_display_scope::{DisplayRect, DisplaySummary, DISPLAY_ID_STABILITY_SESSION},
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
    }
}

/// 生成 composite screenshot 的文件 frame 和轻量摘要。
///
/// `@observe` 复用这个入口,避免反解析 `@screenshot` 的 response 文本。
/// 这里只返回 `@savefile` frames,最终 `@response` 由调用方自己组织。
pub fn execute_screenshot_bundle_request(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
) -> io::Result<(Vec<ControlFrame>, ScreenshotBundleSummary)> {
    execute_screenshot_bundle_request_with_freshness(
        request_id,
        request,
        reject_stale_composite_capture,
    )
}

fn execute_screenshot_bundle_request_with_freshness<F>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    freshness_check: F,
) -> io::Result<(Vec<ControlFrame>, ScreenshotBundleSummary)>
where
    F: FnOnce(&[CapturedDisplay]) -> io::Result<()>,
{
    validate_composite_request(request)?;
    let displays = capture_all_display_images()?;
    freshness_check(&displays)?;
    let screenshot_id = current_unix_epoch_millis().to_string();
    let accessibility = build_accessibility_manifest(request, capture_default_ax_snapshot)?;
    build_composite_screenshot_parts_with_id_and_ax(
        request_id,
        request,
        displays,
        &screenshot_id,
        accessibility,
    )
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

    let bundle = build_screenshot_bundle_with_ax(displays, screenshot_id, accessibility)?;
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

    let summary = ScreenshotBundleSummary {
        kind: "screenshot-bundle",
        layout: "composite",
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
    #[serde(skip_serializing_if = "Option::is_none")]
    accessibility: Option<AxSnapshot>,
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

fn build_screenshot_bundle_with_ax(
    displays: Vec<CapturedDisplay>,
    screenshot_id: &str,
    accessibility: Option<AxSnapshot>,
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

pub fn current_display_summaries() -> io::Result<Vec<DisplaySummary>> {
    let displays = capture_all_display_images()?;
    validate_captured_displays(&displays)?;
    let virtual_bounds =
        build_virtual_bounds(displays.iter().map(|display| display.metadata.os_rect))?;
    displays
        .into_iter()
        .map(|display| {
            let image_rect = os_rect_to_image_rect(display.metadata.os_rect, virtual_bounds)?;
            Ok(display_summary_from_metadata(&display.metadata, image_rect))
        })
        .collect()
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
fn reset_screenshot_freshness_cache_for_tests() {
    let cache = LAST_COMPOSITE_FINGERPRINT.get_or_init(|| Mutex::new(None));
    if let Ok(mut last) = cache.lock() {
        *last = None;
    }
}

#[cfg(test)]
mod tests;
