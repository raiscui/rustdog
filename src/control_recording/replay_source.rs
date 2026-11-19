use std::{fs::File, io::{self, Read}, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{model::CapturedInput, session::JOURNAL_MAX_BYTES};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    recording_id: String,
    journal_seq: u64,
    kind: String,
    monotonic_ns: u64,
    #[serde(default)]
    capture_seq: Option<u64>,
    #[serde(default)]
    captured_monotonic_ns: Option<u64>,
    payload: Payload,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Payload {
    Start(Start),
    Capture(CapturedInput),
    Marker(Marker),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[serde(rename = "type")]
    kind: String,
    profile: String,
    started_at_unix_ms: u64,
    platform: Platform,
    coordinate_space: String,
    lanes: Lanes,
    coverage: String,
    text_mode: String,
    shortcut_mode: String,
    replay_status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Platform {
    os: String,
    capture_backend: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lanes {
    physical: String,
    window: String,
    semantic: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize)]
pub(crate) struct ReplaySource {
    pub sha256: String,
    pub recording_id: String,
    pub facts: Vec<ReplayFact>,
    pub segments: Vec<super::replay_normalize::Segment>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ReplayFact {
    Capture { journal_seq: u64, monotonic_ns: u64, capture_seq: u64, captured_monotonic_ns: u64, input: CapturedInput },
    Mark { journal_seq: u64, monotonic_ns: u64, mark: String },
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "无效或不支持的录制文件")
}

pub(crate) fn read_source(path: &Path) -> io::Result<ReplaySource> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(JOURNAL_MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > JOURNAL_MAX_BYTES || !bytes.ends_with(b"\n") {
        return Err(invalid());
    }
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let mut recording_id = None;
    let mut facts = Vec::new();
    let mut previous_time = 0;
    let mut next_capture = 0;
    let mut terminal = false;
    for (index, line) in bytes[..bytes.len() - 1].split(|b| *b == b'\n').enumerate() {
        let entry: Envelope = serde_json::from_slice(line).map_err(|_| invalid())?;
        if entry.schema != super::journal::JOURNAL_SCHEMA
            || entry.recording_id.is_empty()
            || entry.journal_seq != index as u64
            || entry.monotonic_ns < previous_time
            || terminal
        {
            return Err(invalid());
        }
        previous_time = entry.monotonic_ns;
        match &recording_id {
            None => recording_id = Some(entry.recording_id.clone()),
            Some(id) if id != &entry.recording_id => return Err(invalid()),
            _ => {}
        }
        match entry.kind.as_str() {
            "session_start" if index == 0 => {
                let Payload::Start(start) = entry.payload else { return Err(invalid()); };
                if start.kind != "start" || !matches!(start.profile.as_str(), "semantic" | "physical")
                    || start.platform.os != "windows" || start.platform.capture_backend != "windows_helper"
                    || start.coordinate_space != "native_physical_screen" || start.coverage != "best_effort"
                    || start.text_mode != super::TEXT_MODE || start.shortcut_mode != "unsupported_without_evidence"
                    || start.replay_status != "not_compiled" || start.started_at_unix_ms == 0
                    || start.lanes.physical != "available" || start.lanes.window != "available"
                    || !matches!(start.lanes.semantic.as_str(), "available" | "disabled")
                    || entry.capture_seq.is_some() || entry.captured_monotonic_ns.is_some()
                { return Err(invalid()); }
            }
            "physical" | "context" | "semantic" | "redaction" if index > 0 => {
                let capture_seq = entry.capture_seq.ok_or_else(invalid)?;
                let captured_monotonic_ns = entry.captured_monotonic_ns.ok_or_else(invalid)?;
                if capture_seq != next_capture { return Err(invalid()); }
                let Payload::Capture(input) = entry.payload else { return Err(invalid()); };
                let expected = match &input {
                    CapturedInput::MouseDown { .. } | CapturedInput::MouseUp { .. }
                    | CapturedInput::MouseMove { .. } | CapturedInput::Scroll { .. } => "physical",
                    CapturedInput::DisplayContext { .. } | CapturedInput::WindowContext { .. } => "context",
                    CapturedInput::Redaction { .. } => "redaction",
                    CapturedInput::SemanticCandidate { .. } | CapturedInput::SemanticMissing { .. } => "semantic",
                };
                if entry.kind != expected { return Err(invalid()); }
                facts.push(ReplayFact::Capture { journal_seq: entry.journal_seq, monotonic_ns: entry.monotonic_ns, capture_seq, captured_monotonic_ns, input });
                next_capture += 1;
            }
            "mark" if index > 0 => {
                let Payload::Marker(mark) = entry.payload else { return Err(invalid()); };
                if !matches!(mark.kind.as_str(), "checkpoint" | "parameter_boundary")
                    || entry.capture_seq.is_some() || entry.captured_monotonic_ns.is_some() { return Err(invalid()); }
                facts.push(ReplayFact::Mark { journal_seq: entry.journal_seq, monotonic_ns: entry.monotonic_ns, mark: mark.kind });
            }
            "session_terminal" if index > 0 => {
                let Payload::Marker(marker) = entry.payload else { return Err(invalid()); };
                if marker.kind != "recorded" || entry.capture_seq.is_some() || entry.captured_monotonic_ns.is_some() { return Err(invalid()); }
                terminal = true;
            }
            _ => return Err(invalid()),
        }
    }
    if !terminal { return Err(invalid()); }
    let segments = super::replay_normalize::normalize(&facts);
    Ok(ReplaySource { sha256, recording_id: recording_id.ok_or_else(invalid)?, facts, segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_recording::{journal::JournalWriter, session::Profile, CaptureEvent, CapturedInput};

    #[test]
    fn reads_sealed_journal_and_rejects_unknown_sensitive_fields() {
        let dir = std::env::temp_dir().join(format!("rdog-replay-source-{}", uuid::Uuid::new_v4()));
        let mut writer = JournalWriter::open(&dir, "rec-source", Profile::Physical, 1, 4096).unwrap();
        writer.event(&CaptureEvent {
            sequence: 0,
            monotonic_ns: 1,
            input: CapturedInput::Redaction { window: None, segment: 1, active: true },
        }).unwrap();
        let sealed = writer.seal().unwrap();
        let source = read_source(&sealed.path).unwrap();
        assert_eq!(source.sha256, sealed.sha256);
        assert_eq!(source.recording_id, "rec-source");
        assert!(matches!(&source.facts[..], [ReplayFact::Capture { journal_seq: 1, monotonic_ns: _, capture_seq: 0, captured_monotonic_ns: 1, input: CapturedInput::Redaction { window: None, segment: 1, active: true } }]));
        let original = std::fs::read_to_string(&sealed.path).unwrap();
        let modified = original.replace("\"segment\":1", "\"segment\":1,\"text\":\"secret-canary\"");
        assert_ne!(original, modified);
        std::fs::write(&sealed.path, modified).unwrap();
        let error = read_source(&sealed.path).err().unwrap().to_string();
        assert!(!error.contains("secret-canary"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
