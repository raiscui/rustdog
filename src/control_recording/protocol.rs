//! `@record-start` / `@record-status` / `@record-mark` / `@record-stop` /
//! `@record-cancel` 的协议层解析。
//!
//! 协议层只产出 `RecordRequest`,真正 lifecycle/delivery 由
//! `control_handler::RecordingHandler` 负责。

use std::io;

use serde_json::Value;

use crate::control_recording::session::Profile;
use crate::control_protocol::object_inner;

use super::control_handler::RecordRequest;

fn invalid_data(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// `@record-start:{...}` 或裸 `@record-start`。
pub(crate) fn parse_record_start_payload(input: &str) -> io::Result<RecordRequest> {
    let profile = if input.trim().is_empty() {
        Profile::Semantic
    } else {
        let inner = object_inner(input, "@record-start")?;
        let value: Value = serde_json::from_str(&inner).map_err(|err| invalid_data(format!("@record-start payload 不是 JSON: {err}")))?;
        match value.get("profile").and_then(|v| v.as_str()).unwrap_or("semantic") {
            "semantic" => Profile::Semantic,
            "physical" => Profile::Physical,
            other => return Err(invalid_data(format!("@record-start.profile 不支持: {other}"))),
        }
    };
    Ok(RecordRequest::Start { profile })
}

/// `@record-status` 不接受 payload。
pub(crate) fn parse_record_status_payload(input: &str) -> io::Result<RecordRequest> {
    if !input.trim().is_empty() {
        return Err(invalid_data("@record-status 不接受 payload"));
    }
    Ok(RecordRequest::Status)
}

/// `@record-mark:{...}` 或裸 `@record-mark`。
pub(crate) fn parse_record_mark_payload(input: &str) -> io::Result<RecordRequest> {
    if input.trim().is_empty() {
        return Ok(RecordRequest::Mark { label: None, redaction_active: false });
    }
    let inner = object_inner(input, "@record-mark")?;
    let value: Value = serde_json::from_str(&inner).map_err(|err| invalid_data(format!("@record-mark payload 不是 JSON: {err}")))?;
    let label = value.get("label").and_then(|v| v.as_str()).map(|s| s.to_owned());
Ok(RecordRequest::Mark { label, redaction_active: value.get("redaction_active").and_then(|v| v.as_bool()).unwrap_or(false) })
}

/// `@record-stop:{...}` 或裸 `@record-stop`。
pub(crate) fn parse_record_stop_payload(input: &str) -> io::Result<RecordRequest> {
    if !input.trim().is_empty() {
        let _ = object_inner(input, "@record-stop")?;
    }
    Ok(RecordRequest::Stop)
}

/// `@record-cancel:{...}` 或裸 `@record-cancel`。
pub(crate) fn parse_record_cancel_payload(input: &str) -> io::Result<RecordRequest> {
    if !input.trim().is_empty() {
        let _ = object_inner(input, "@record-cancel")?;
    }
    Ok(RecordRequest::Cancel)
}
