use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, RgbaImage};
use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_protocol::ScreenshotRequest,
};

pub fn execute_screenshot_request(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
) -> io::Result<ControlExecutionOutcome> {
    execute_screenshot_request_with_capture(request_id, request, capture_primary_display_image)
}

fn execute_screenshot_request_with_capture<F>(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    capture: F,
) -> io::Result<ControlExecutionOutcome>
where
    F: FnOnce() -> io::Result<RgbaImage>,
{
    let image = capture()?;
    build_screenshot_outcome(request_id, request, image)
}

fn build_screenshot_outcome(
    request_id: Option<u64>,
    request: &ScreenshotRequest,
    image: RgbaImage,
) -> io::Result<ControlExecutionOutcome> {
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
            ControlFrame::ResponseLine(render_screenshot_success_response(request_id)),
        ],
    })
}

fn render_screenshot_success_response(request_id: Option<u64>) -> String {
    match request_id {
        Some(request_id) => format!(r#"@response {{"id":{request_id},"value":0}}"#),
        None => "@response 0".to_owned(),
    }
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
    capture_with_sck_rs().or_else(|primary_err| {
        capture_with_xcap().map_err(|fallback_err| {
            io::Error::new(
                classify_capture_error(&primary_err, &fallback_err),
                format!("sck-rs 截图失败: {primary_err}; xcap fallback 也失败: {fallback_err}"),
            )
        })
    })
}

#[cfg(target_os = "macos")]
fn capture_with_sck_rs() -> io::Result<RgbaImage> {
    let monitor = sck_rs::Monitor::primary().map_err(map_capture_error)?;
    monitor.capture_image().map_err(map_capture_error)
}

#[cfg(target_os = "macos")]
fn capture_with_xcap() -> io::Result<RgbaImage> {
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
        .into_iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| {
            xcap::Monitor::all()
                .ok()
                .and_then(|mut monitors| monitors.drain(..).next())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "xcap 未找到可截图显示器"))?;
    monitor.capture_image().map_err(map_capture_error)
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

fn current_unix_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn build_screenshot_outcome_should_emit_savefile_and_final_response() {
        let image = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 255]));
        let request = ScreenshotRequest::default();
        let outcome =
            build_screenshot_outcome(Some(7), &request, image).expect("outcome should build");

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
    fn execute_screenshot_request_with_capture_should_emit_dynamic_frames() {
        let request = ScreenshotRequest { quality: 80 };
        let outcome = execute_screenshot_request_with_capture(Some(11), &request, || {
            Ok(RgbaImage::from_pixel(3, 2, Rgba([0, 255, 0, 255])))
        })
        .expect("captured image should build screenshot outcome");

        assert_eq!(outcome.outbound_frames.len(), 2);

        match &outcome.outbound_frames[0] {
            ControlFrame::SaveFile(frame) => {
                assert_eq!(frame.request_id, Some(11));
                assert_eq!(frame.quality, Some(80));
                assert_eq!(frame.width, Some(3));
                assert_eq!(frame.height, Some(2));
                assert!(frame.filename.starts_with("screenshot-"));
                assert!(frame.filename.ends_with(".jpg"));
                assert!(!frame.data.is_empty());
            }
            other => panic!("expected first frame to be SaveFile, got {other:?}"),
        }

        match &outcome.outbound_frames[1] {
            ControlFrame::ResponseLine(line) => {
                assert_eq!(line, r#"@response {"id":11,"value":0}"#);
            }
            other => panic!("expected second frame to be ResponseLine, got {other:?}"),
        }
    }

    #[test]
    fn execute_screenshot_request_with_capture_should_preserve_permission_denied() {
        let request = ScreenshotRequest::default();
        let err = execute_screenshot_request_with_capture(Some(1), &request, || {
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
    fn real_capture_smoke_should_capture_or_report_permission_denied() {
        let request = ScreenshotRequest::default();

        match execute_screenshot_request(Some(99), &request) {
            Ok(outcome) => {
                assert_eq!(outcome.outbound_frames.len(), 2);

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
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                eprintln!("real screenshot smoke hit permission boundary: {err}");
            }
            Err(err) => panic!("real screenshot smoke failed unexpectedly: {err}"),
        }
    }
}
