use super::{
    control_handler::{MarkKind, RecordRequest},
    model::CaptureTarget,
    session::Profile,
};
use serde_json::Value;
use std::io;

fn object(input: &str) -> io::Result<Value> {
    let value: Value = serde_json::from_str(input.trim())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid recording JSON"))?;
    if !value.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "recording payload must be an object",
        ));
    }
    Ok(value)
}
fn only_fields(value: &Value, allowed: &[&str]) -> io::Result<()> {
    if value
        .as_object()
        .is_some_and(|map| map.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown recording field",
        ));
    }
    Ok(())
}
fn optional_string(value: &Value, key: &str) -> io::Result<Option<String>> {
    value
        .get(key)
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid recording field")
                })
        })
        .transpose()
}
fn recording_id(value: &Value) -> io::Result<String> {
    optional_string(value, "recording_id")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "recording_id required"))
}
pub(crate) fn parse_record_start_payload(input: &str) -> io::Result<RecordRequest> {
    let v = object(input)?;
    only_fields(&v, &["profile", "duration_ms", "targets"])?;
    let profile = match optional_string(&v, "profile")?
        .as_deref()
        .unwrap_or("semantic")
    {
        "semantic" => Profile::Semantic,
        "physical" => Profile::Physical,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid profile",
            ))
        }
    };
    let duration_ms = v
        .get("duration_ms")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid duration_ms"))
        })
        .transpose()?;
    let targets: Vec<CaptureTarget> = serde_json::from_value(
        v.get("targets")
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "targets required"))?,
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid targets"))?;
    Ok(RecordRequest::Start {
        profile,
        duration_ms,
        targets,
    })
}
pub(crate) fn parse_record_status_payload(input: &str) -> io::Result<RecordRequest> {
    if input.trim().is_empty() {
        return Ok(RecordRequest::Status { recording_id: None });
    }
    let v = object(input)?;
    only_fields(&v, &["recording_id"])?;
    Ok(RecordRequest::Status {
        recording_id: optional_string(&v, "recording_id")?,
    })
}
pub(crate) fn parse_record_mark_payload(input: &str) -> io::Result<RecordRequest> {
    let v = object(input)?;
    only_fields(&v, &["recording_id", "mark"])?;
    let mark = match optional_string(&v, "mark")?.as_deref() {
        Some("checkpoint") => MarkKind::Checkpoint,
        Some("parameter_boundary") => MarkKind::ParameterBoundary,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid mark")),
    };
    Ok(RecordRequest::Mark {
        recording_id: recording_id(&v)?,
        mark,
    })
}
pub(crate) fn parse_record_stop_payload(input: &str) -> io::Result<RecordRequest> {
    let v = object(input)?;
    only_fields(&v, &["recording_id"])?;
    Ok(RecordRequest::Stop {
        recording_id: recording_id(&v)?,
    })
}
pub(crate) fn parse_record_cancel_payload(input: &str) -> io::Result<RecordRequest> {
    let v = object(input)?;
    only_fields(&v, &["recording_id"])?;
    Ok(RecordRequest::Cancel {
        recording_id: recording_id(&v)?,
    })
}
