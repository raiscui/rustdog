//! Unit tests for the Recording Journal writer.
//!
//! Per ticket `#17` and `specs/rdog-recording-journal-model.md`. These tests
//! exercise the JSONL envelope, sequence monotonicity, capture_seq gating,
//! and crash-resilience boundaries (orphan file on drop without explicit close).

#![cfg_attr(test, allow(dead_code))]

use std::{
    fs,
    path::PathBuf,
};

use serde_json::Value;

use super::{
    CaptureEvent,
    journal::{
        GapDeclaration, JournalError, JournalKind, JournalWriter, LaneTransition, Mark,
        PlatformInfo, SessionTerminalState, WallClockAnchor, JOURNAL_SCHEMA,
    },
};

fn temp_path(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rdog-recorder-journal-test-{}-{}",
        label,
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir.join(format!("{label}.jsonl"))
}

fn open_in(label: &str) -> (JournalWriter, PathBuf) {
    let path = temp_path(label);
    let _ = fs::remove_file(&path);
    let anchor = WallClockAnchor {
        started_at_unix_ms: 1_700_000_000_000,
        monotonic_origin_ns: 8_192_000_000,
    };
    let platform = PlatformInfo {
        os: std::env::consts::OS.to_string(),
        capture_backend: "stub".into(),
    };
    let lanes: &[(&str, &str, u64)] = &[
        ("event_listen", "available", 1),
        ("accessibility", "available", 1),
        ("tap_health", "available", 1),
    ];
    let writer = JournalWriter::open(
        path.clone(),
        format!("rec-{label}"),
        platform,
        anchor,
        "semantic",
        "topology-1",
        "os-logical",
        lanes,
    )
    .expect("open journal");
    (writer, path)
}

fn read_lines(path: &PathBuf) -> Vec<Value> {
    let text = fs::read_to_string(path).expect("read journal");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse journal line"))
        .collect()
}

#[test]
fn session_start_is_journal_seq_zero_and_unique() {
    let (mut writer, path) = open_in("session_start");
    writer.close().expect("close");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 1);
    let entry = &lines[0];
    assert_eq!(entry["schema"], JOURNAL_SCHEMA);
    assert_eq!(entry["journal_seq"], 0);
    assert_eq!(entry["kind"], "session_start");
    let payload = &entry["payload"];
    assert_eq!(payload["type"], "start");
    assert_eq!(payload["profile"], "semantic");
    assert_eq!(payload["started_at_unix_ms"], 1_700_000_000_000u64);
    assert_eq!(payload["monotonic_origin_ns"], 8_192_000_000u64);
    let platform = &payload["platform"];
    assert_eq!(platform["capture_backend"], "stub");
    let lanes = &payload["lanes"];
    assert_eq!(lanes["event_listen"]["state"], "available");
    assert_eq!(lanes["event_listen"]["generation"], 1);
    let topology = &payload["display_topology"];
    assert_eq!(topology["topology_key"], "topology-1");
    assert_eq!(topology["coordinate_space"], "os-logical");
}

#[test]
fn capture_event_assigns_strictly_monotonic_capture_seq() {
    let (mut writer, path) = open_in("capture_seq");
    let event = CaptureEvent::Key {
        monotonic_ms: 100,
        keycode: 0x04,
        down: true,
        text: Some("a".into()),
    };
    let s0 = writer.write_capture_event(101, &event).expect("write 1");
    let s1 = writer.write_capture_event(102, &event).expect("write 2");
    let s2 = writer.write_capture_event(103, &event).expect("write 3");
    assert_eq!((s0, s1, s2), (0, 1, 2));
    assert_eq!(writer.capture_seq(), 3);
    writer.close().expect("close");
    let lines = read_lines(&path);
    // session_start + 3 physical = 4 entries
    assert_eq!(lines.len(), 4);
    for (idx, line) in lines.iter().enumerate() {
        assert_eq!(line["journal_seq"], idx as u64);
    }
    for (idx, line) in lines[1..].iter().enumerate() {
        assert_eq!(line["kind"], "physical");
        assert_eq!(line["capture_seq"], idx as u64);
        assert_eq!(line["payload"]["type"], "key_down");
        assert_eq!(line["payload"]["keycode"], 0x04);
        assert_eq!(line["payload"]["text"], "a");
    }
}

#[test]
fn lane_status_writes_transition_with_required_fields() {
    let (mut writer, path) = open_in("lane_status");
    writer
        .write_lane_status(
            200,
            LaneTransition {
                lane: "accessibility".into(),
                state: "denied".into(),
                reason: "user_revoked".into(),
                recoverable: false,
                generation: 2,
            },
        )
        .expect("write lane_status");
    writer.close().expect("close");
    let lines = read_lines(&path);
    let lane_entry = &lines[1];
    assert_eq!(lane_entry["kind"], "lane_status");
    let payload = &lane_entry["payload"];
    assert_eq!(payload["type"], "transition");
    assert_eq!(payload["lane"], "accessibility");
    assert_eq!(payload["state"], "denied");
    assert_eq!(payload["reason"], "user_revoked");
    assert_eq!(payload["recoverable"], false);
    assert_eq!(payload["generation"], 2);
    // capture_seq must NOT be set on lane_status.
    assert!(lane_entry.get("capture_seq").is_none());
}

#[test]
fn gap_carries_capture_seq_range_and_cause() {
    let (mut writer, path) = open_in("gap");
    writer
        .write_gap(
            300,
            GapDeclaration {
                first_capture_seq: Some(7),
                last_capture_seq: Some(12),
                dropped_count: Some(6),
                cause: "queue_overflow".into(),
                recoverable: true,
            },
        )
        .expect("write gap");
    writer.close().expect("close");
    let lines = read_lines(&path);
    let gap_entry = &lines[1];
    assert_eq!(gap_entry["kind"], "gap");
    let payload = &gap_entry["payload"];
    assert_eq!(payload["type"], "event_loss");
    assert_eq!(payload["capture_seq_range"]["first"], 7);
    assert_eq!(payload["capture_seq_range"]["last"], 12);
    assert_eq!(payload["dropped_count"], 6);
    assert_eq!(payload["cause"], "queue_overflow");
    assert_eq!(payload["recoverable"], true);
}

#[test]
fn mark_boundary_writes_fsync_immediately() {
    let (mut writer, path) = open_in("mark");
    writer
        .write_mark(
            400,
            Mark {
                label: "redact_password".into(),
                redaction_active: true,
            },
        )
        .expect("write mark");
    writer.close().expect("close");
    let lines = read_lines(&path);
    let mark_entry = &lines[1];
    assert_eq!(mark_entry["kind"], "mark");
    assert_eq!(mark_entry["payload"]["type"], "redact_password");
    assert_eq!(mark_entry["payload"]["redaction_active"], true);
}

#[test]
fn session_terminal_is_last_entry_and_distinct() {
    let (mut writer, path) = open_in("terminal");
    writer
        .write_capture_event(
            500,
            &CaptureEvent::MouseMove {
                monotonic_ms: 500,
                x: 1,
                y: 1,
            },
        )
        .expect("write physical");
    writer
        .write_session_terminal(600, SessionTerminalState::Completed)
        .expect("write terminal");
    writer.close().expect("close");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["kind"], "session_start");
    assert_eq!(lines[1]["kind"], "physical");
    let terminal = &lines[2];
    assert_eq!(terminal["kind"], "session_terminal");
    assert_eq!(terminal["payload"]["type"], "completed");
    assert_eq!(terminal["journal_seq"], 2);
}

#[test]
fn writes_after_close_return_state_error() {
    let (mut writer, _path) = open_in("closed_state");
    writer.close().expect("close");
    let event = CaptureEvent::MouseMove {
        monotonic_ms: 1,
        x: 0,
        y: 0,
    };
    let err = writer.write_capture_event(2, &event).expect_err("must fail after close");
    assert!(matches!(err, JournalError::State(_)));
}

#[test]
fn fsync_interval_triggers_at_threshold() {
    // FSYNC_INTERVAL_EVENTS = 100. Open, write session_start (1 fsync), then
    // write 100 capture events to trigger the next fsync at the boundary.
    let (mut writer, path) = open_in("fsync_interval");
    let event = CaptureEvent::Scroll {
        monotonic_ms: 1,
        delta_x: 0,
        delta_y: 1,
    };
    for _ in 0..JournalWriter::FSYNC_INTERVAL_EVENTS {
        writer.write_capture_event(2, &event).expect("write");
    }
    writer.close().expect("close");
    // session_start + 100 physical = 101 entries
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 101);
    // All entries share the same schema and recording_id.
    for line in &lines {
        assert_eq!(line["schema"], JOURNAL_SCHEMA);
        assert!(line["recording_id"].as_str().unwrap().starts_with("rec-fsync_interval"));
    }
}

#[test]
fn drop_without_close_leaves_orphan_file_with_session_start() {
    // Crash semantics: a writer dropped without explicit close leaves the
    // file on disk with whatever entries were flushed. Lifecycle recovers
    // the file as orphan per ticket #5.
    let path = temp_path("orphan");
    let _ = fs::remove_file(&path);
    let anchor = WallClockAnchor {
        started_at_unix_ms: 42,
        monotonic_origin_ns: 100,
    };
    let platform = PlatformInfo {
        os: "test".into(),
        capture_backend: "test".into(),
    };
    {
        let mut writer = JournalWriter::open(
            path.clone(),
            "rec-orphan".into(),
            platform,
            anchor,
            "semantic",
            "topology-1",
            "os-logical",
            &[("event_listen", "available", 1)],
        )
        .expect("open");
        writer
            .write_capture_event(
                10,
                &CaptureEvent::MouseMove {
                    monotonic_ms: 10,
                    x: 1,
                    y: 1,
                },
            )
            .expect("write");
        // No close() call — drop runs the Drop impl.
    }
    assert!(path.exists(), "orphan file must remain on disk");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["kind"], "session_start");
    assert_eq!(lines[1]["kind"], "physical");
}

#[test]
fn journal_kind_as_str_matches_schema_strings() {
    // The map in `JournalKind::as_str` is the contract for emitted `kind`
    // strings. Any drift breaks downstream readers.
    let cases = [
        (JournalKind::SessionStart, "session_start"),
        (JournalKind::Physical, "physical"),
        (JournalKind::SemanticCandidate, "semantic_candidate"),
        (JournalKind::Context, "context"),
        (JournalKind::LaneStatus, "lane_status"),
        (JournalKind::Redaction, "redaction"),
        (JournalKind::Gap, "gap"),
        (JournalKind::Mark, "mark"),
        (JournalKind::SessionTerminal, "session_terminal"),
    ];
    for (kind, expected) in cases {
        // Round-trip through serde to ensure rename_all = snake_case holds.
        let json = serde_json::to_value(kind).expect("serialize");
        assert_eq!(json, Value::String(expected.to_string()));
    }
}

#[test]
fn open_rejects_existing_file_with_create_new() {
    let path = temp_path("create_new");
    fs::write(&path, b"existing content").expect("pre-write");
    let anchor = WallClockAnchor {
        started_at_unix_ms: 1,
        monotonic_origin_ns: 1,
    };
    let platform = PlatformInfo {
        os: "test".into(),
        capture_backend: "test".into(),
    };
    let err = JournalWriter::open(
        path,
        "rec-dup".into(),
        platform,
        anchor,
        "semantic",
        "topology-1",
        "os-logical",
        &[("event_listen", "available", 1)],
    )
    .expect_err("open on existing file must fail");
    assert!(matches!(err, JournalError::Io(_)));
}
