use super::cli::run_session;
use crate::control_frames::{ControlFrame, SaveFileFrame};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use std::{io, sync::mpsc};

#[test]
fn idle_status_precedes_start_without_waiting_for_prompt() {
    let (sender, receiver) = mpsc::channel();
    sender.send(Some("status".into())).unwrap();
    sender.send(Some("start --target 2:3".into())).unwrap();
    sender.send(None).unwrap();
    let mut calls = Vec::new();
    let mut exchange = |line: &str| -> io::Result<Vec<ControlFrame>> {
        calls.push(line.to_owned());
        let value = if line.starts_with("@record-status:") {
            serde_json::json!({"schema":"rdog.record-control.v2","kind":"record-status","phase":"idle","cleanup_status":"complete"})
        } else if line.starts_with("@record-start:") {
            serde_json::json!({"schema":"rdog.record-control.v2","kind":"record-start","phase":"recording","recording_id":"rec-1","policy":{"renew_interval_ms":15000}})
        } else {
            serde_json::json!({"schema":"rdog.record-control.v2","phase":"cancelled"})
        };
        Ok(vec![ControlFrame::ResponseLine(format!("@response {value}"))])
    };
    run_session(&mut exchange, receiver, std::path::Path::new("unused")).unwrap();
    assert_eq!(calls[0], "@record-status:{}");
    assert!(calls[1].starts_with("@record-start:"));
    assert!(calls[2].starts_with("@record-cancel:"));
    assert_eq!(calls.len(), 3);
}

#[test]
fn idle_session_renews_and_delivers_auto_sealed_recording_once() {
    let (sender, receiver) = mpsc::channel();
    sender
        .send(Some("start --target 42:0x123 --duration 1s".into()))
        .unwrap();
    let dir = std::env::temp_dir().join(format!("rdog-cli-test-{}", uuid::Uuid::new_v4()));
    let mut calls = Vec::new();
    let mut exchange = |line: &str| -> io::Result<Vec<ControlFrame>> {
        calls.push(line.to_owned());
        if line.starts_with("@record-start:") {
            assert!(line.contains("\"pid\":42"));
            assert!(line.contains("\"hwnd\":291"));
            return Ok(vec![ControlFrame::ResponseLine(
                "@response {\"schema\":\"rdog.record-control.v2\",\"recording_id\":\"rec-1\",\"policy\":{\"renew_interval_ms\":1}}"
                    .into(),
            )]);
        }
        if line.starts_with("@record-status:") {
            return Ok(vec![ControlFrame::ResponseLine(
                "@response {\"schema\":\"rdog.record-control.v2\",\"phase\":\"recorded\",\"recording_id\":\"rec-1\"}".into(),
            )]);
        }
        assert!(line.starts_with("@record-stop:"));
        let bytes = b"journal\n";
        let metadata = serde_json::json!({"schema":"rdog.record-control.v2","kind":"record-stop","phase":"recorded","recording_id":"rec-1","artifact_filename":"rec-1.rdogrec.jsonl","artifact_size_bytes":bytes.len(),"artifact_sha256":format!("{:x}", Sha256::digest(bytes))});
        Ok(vec![
            ControlFrame::SaveFile(SaveFileFrame {
                request_id: None,
                filename: "rec-1.rdogrec.jsonl".into(),
                mime: "application/x-ndjson".into(),
                encoding: "base64".into(),
                data: STANDARD.encode(bytes),
                quality: None,
                width: None,
                height: None,
            }),
            ControlFrame::ResponseLine(format!("@response {metadata}")),
        ])
    };
    run_session(&mut exchange, receiver, &dir).unwrap();
    assert!(calls[1].starts_with("@record-status:"));
    assert_eq!(
        std::fs::read(dir.join("rec-1.rdogrec.jsonl")).unwrap(),
        b"journal\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn eof_cancels_active_recording_without_retry() {
    let (sender, receiver) = mpsc::channel();
    sender.send(Some("start --target 2:3".into())).unwrap();
    sender.send(None).unwrap();
    let mut calls = Vec::new();
    let mut exchange = |line: &str| -> io::Result<Vec<ControlFrame>> {
        calls.push(line.to_owned());
        Ok(vec![ControlFrame::ResponseLine(
            if line.starts_with("@record-start:") {
                "@response {\"schema\":\"rdog.record-control.v2\",\"recording_id\":\"rec-1\",\"policy\":{\"renew_interval_ms\":15000}}"
            } else {
                "@response {\"schema\":\"rdog.record-control.v2\",\"phase\":\"cancelled\"}"
            }
            .into(),
        )])
    };
    run_session(&mut exchange, receiver, std::path::Path::new("unused")).unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].starts_with("@record-cancel:"));
}

#[test]
fn unknown_protocol_versions_and_active_states_fail_closed() {
    for (version, phase) in [(3, "recording"), (2, "future-state")] {
        let (sender, receiver) = mpsc::channel();
        sender.send(Some("start --target 2:3".into())).unwrap();
        sender.send(Some("status".into())).unwrap();
        sender.send(None).unwrap();
        let dir = std::env::temp_dir().join(format!("rdog-cli-test-{}", uuid::Uuid::new_v4()));
        let mut exchange = |line: &str| -> io::Result<Vec<ControlFrame>> {
            let value = if line.starts_with("@record-start:") {
                serde_json::json!({"schema":format!("rdog.record-control.v{version}"),"recording_id":"rec-1","policy":{"renew_interval_ms":15000}})
            } else if line.starts_with("@record-status:") {
                serde_json::json!({"schema":"rdog.record-control.v2","recording_id":"rec-1","phase":phase})
            } else {
                serde_json::json!({"schema":"rdog.record-control.v2","phase":"cancelled"})
            };
            Ok(vec![ControlFrame::ResponseLine(format!(
                "@response {value}"
            ))])
        };
        assert!(run_session(&mut exchange, receiver, &dir).is_err());
        assert!(!dir.exists());
    }
}
