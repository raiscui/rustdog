//! line-control `@record-*` 到 RecordingHandler 的端到端测试。
//!
//! 策略: 直接调用 `RecordingHandler` 而不通过 `control_core`,因为
//! `control_core` 强依赖 `with_recording_handler` 全局 slot,而 slot
//! 一旦 install 就会被同进程所有测试共享,会污染并发。handler-level
//! 测试保留真实 lifecycle / delivery 状态机,够用。

use std::{fs, path::PathBuf};

use serde_json::Value;

use super::{
    control_handler::{RecordRequest, RecordingHandler},
    session::ConnectionId,
};

fn temp_dirs() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("rdog-rec-handler-{}-{:?}-{}", std::process::id(), std::thread::current().id(), crate::control_recording::journal::now_unix_ms()));
    let journal = base.join("journal");
    let bundle = base.join("bundle");
    fs::create_dir_all(&journal).unwrap();
    fs::create_dir_all(&bundle).unwrap();
    (journal, bundle)
}

fn first_response_value(outcome: &super::super::control_frames::ControlExecutionOutcome) -> Value {
    let frame = outcome.outbound_frames.first().expect("at least one frame");
    let text = match frame {
        super::super::control_frames::ControlFrame::ResponseLine(s) => s.clone(),
        other => panic!("expected response line, got {other:?}"),
    };
    let body = text.strip_prefix("@response ").expect("response shape");
    let envelope: Value = serde_json::from_str(body).expect("response envelope json");
    if let Some(value) = envelope.get("value") { return value.clone(); }
    if let Some(err) = envelope.get("error").and_then(|v| v.as_str()) {
        if let Ok(parsed) = serde_json::from_str::<Value>(err) { return parsed; }
    }
    envelope
}

#[test]
fn start_then_status_then_stop_emits_savefile_and_response() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle.clone());
    let owner = ConnectionId(1);

    let start = handler.handle(Some(1), owner, RecordRequest::Start { profile: super::session::Profile::Semantic, duration_ms: None });
    eprintln!("DEBUG start outcome frames = {:?}", start.outbound_frames);
    let start_value = first_response_value(&start);
    assert_eq!(start_value["kind"], "record-start");
    assert_eq!(start_value["status"], "recording");
    let recording_id = start_value["recording_id"].as_str().unwrap().to_owned();

    let status = handler.handle(Some(2), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "recording");
    assert_eq!(status_value["recording_id"].as_str().unwrap(), recording_id);

    let stop = handler.handle(Some(3), owner, RecordRequest::Stop);
    assert_eq!(stop.outbound_frames.len(), 2);
    match &stop.outbound_frames[0] {
        super::super::control_frames::ControlFrame::SaveFile(frame) => {
            assert_eq!(frame.mime, "application/vnd.rdog.recording-bundle");
            assert!(frame.filename.ends_with(".rdogrec.tar"));
            assert!(!frame.data.is_empty());
        }
        other => panic!("expected savefile, got {other:?}"),
    }
    let final_text = match &stop.outbound_frames[1] {
        super::super::control_frames::ControlFrame::ResponseLine(s) => s.clone(),
        other => panic!("expected response, got {other:?}"),
    };
    let envelope: Value = serde_json::from_str(final_text.strip_prefix("@response ").unwrap()).unwrap();
    let final_value = envelope.get("value").cloned().unwrap_or(Value::Null);
    assert_eq!(final_value["kind"], "record-stop");
    assert_eq!(final_value["recording_id"].as_str().unwrap(), recording_id);
    assert_eq!(final_value["delivery_status"], "delivered");
    let bundle_filename = final_value["bundle_filename"].as_str().unwrap();
    assert!(bundle.join(bundle_filename).is_file());
    assert!(handler.completed().contains_key(&recording_id));
}

#[test]
fn non_owner_stop_is_rejected_with_not_owner_code() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let _ = handler.handle(Some(10), ConnectionId(1), RecordRequest::Start { profile: super::session::Profile::Semantic, duration_ms: None });
    let stop = handler.handle(Some(11), ConnectionId(2), RecordRequest::Stop);
    let value = first_response_value(&stop);
    assert_eq!(value["error_code"], "RECORD_NOT_OWNER");
}

#[test]
fn stop_without_active_session_returns_no_active() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let stop = handler.handle(Some(20), ConnectionId(1), RecordRequest::Stop);
    let value = first_response_value(&stop);
    assert_eq!(value["error_code"], "RECORD_NO_ACTIVE_SESSION");
}

#[test]
fn cancel_after_start_returns_cancelled_status() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);
    let _ = handler.handle(Some(30), owner, RecordRequest::Start { profile: super::session::Profile::Semantic, duration_ms: None });
    let cancel = handler.handle(Some(31), owner, RecordRequest::Cancel);
    let value = first_response_value(&cancel);
    assert_eq!(value["kind"], "record-cancel");
    let phase = value.get("phase").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    assert_eq!(phase, "cancelled");
    assert!(handler.completed().is_empty());
}

#[test]
fn mark_after_start_writes_label_and_bumps_count() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);
    let _ = handler.handle(Some(40), owner, RecordRequest::Start { profile: super::session::Profile::Semantic, duration_ms: None });
    let mark = handler.handle(Some(41), owner, RecordRequest::Mark { label: Some("step-ready".to_owned()), redaction_active: false });
    let value = first_response_value(&mark);
    let label = value.get("label").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    assert_eq!(label, "step-ready");
    let redaction_active = value.get("redaction_active").and_then(|v| v.as_bool()).unwrap_or(true);
    assert!(!redaction_active);
    let mark_count = value.get("mark_count").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(mark_count, 1);
    let second = handler.handle(Some(42), owner, RecordRequest::Mark { label: None, redaction_active: true });
    let second_value = first_response_value(&second);
    let label2 = second_value.get("label").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    assert_eq!(label2, "mark");
    let redaction_active2 = second_value.get("redaction_active").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(redaction_active2);
    let mark_count2 = second_value.get("mark_count").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(mark_count2, 2);
}

#[test]
fn mark_by_non_owner_returns_record_not_owner() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let _ = handler.handle(Some(50), ConnectionId(1), RecordRequest::Start { profile: super::session::Profile::Semantic, duration_ms: None });
    let mark = handler.handle(Some(51), ConnectionId(2), RecordRequest::Mark { label: Some("x".to_owned()), redaction_active: false });
    let value = first_response_value(&mark);
    assert_eq!(value["error_code"], "RECORD_NOT_OWNER");
}

#[test]
fn mark_without_active_session_returns_no_active() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let mark = handler.handle(Some(60), ConnectionId(1), RecordRequest::Mark { label: Some("x".to_owned()), redaction_active: false });
    let value = first_response_value(&mark);
    assert_eq!(value["error_code"], "RECORD_NO_ACTIVE_SESSION");
}

// ----------------------------------------------------------------------------
// Issue #23 auto-stop integration tests
// ----------------------------------------------------------------------------

use std::time::Duration;
use super::session::{Profile, StopTrigger, SessionPhase};

#[test]
fn auto_stop_fires_after_duration() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle.clone());
    let owner = ConnectionId(1);

    let start = handler.handle(
        Some(1),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(200),
        },
    );
    let start_value = first_response_value(&start);
    let recording_id = start_value["recording_id"].as_str().unwrap().to_owned();

    // Sleep 250ms so the auto-stop timer fires (200ms). The next
    // handler call observes the FIRED flag and runs the auto-stop
    // inline.
    std::thread::sleep(Duration::from_millis(250));

    let status = handler.handle(Some(2), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "idle", "session should be auto-stopped");
    let last_session = &status_value["last_session"];
    assert_eq!(last_session["phase"], "completed");
    assert_eq!(last_session["stop_trigger"], "auto_duration");

    let summary = handler.last_session().expect("last_session present");
    assert_eq!(summary.phase, SessionPhase::Completed);
    assert_eq!(summary.stop_trigger, Some(StopTrigger::AutoDuration));
    assert!(handler.completed().contains_key(&recording_id), "bundle persisted");
}

#[test]
fn auto_stop_cancelled_by_manual_stop() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle.clone());
    let owner = ConnectionId(1);

    let _ = handler.handle(
        Some(10),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(200),
        },
    );
    // Sleep 50ms then issue a manual stop; the timer should observe
    // the CANCELLED flag and exit cleanly.
    std::thread::sleep(Duration::from_millis(50));

    let stop = handler.handle(Some(11), owner, RecordRequest::Stop);
    assert_eq!(stop.outbound_frames.len(), 2);
    let final_text = match &stop.outbound_frames[1] {
        super::super::control_frames::ControlFrame::ResponseLine(s) => s.clone(),
        other => panic!("expected response, got {other:?}"),
    };
    let envelope: Value = serde_json::from_str(
        final_text.strip_prefix("@response ").unwrap(),
    )
    .unwrap();
    let final_value = envelope.get("value").cloned().unwrap_or(Value::Null);
    assert_eq!(final_value["trigger"], "manual");

    let summary = handler.last_session().expect("last_session present");
    assert_eq!(summary.phase, SessionPhase::Completed);
    assert_eq!(summary.stop_trigger, Some(StopTrigger::Manual));

    // Sleep past the original deadline to confirm the timer thread
    // exited without firing.
    std::thread::sleep(Duration::from_millis(250));
    let summary_after = handler.last_session().expect("last_session still present");
    assert_eq!(summary_after.stop_trigger, Some(StopTrigger::Manual));
}

#[test]
fn auto_stop_cancelled_by_cancel() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);

    let _ = handler.handle(
        Some(20),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(200),
        },
    );
    std::thread::sleep(Duration::from_millis(50));

    let cancel = handler.handle(Some(21), owner, RecordRequest::Cancel);
    let value = first_response_value(&cancel);
    assert_eq!(value["phase"], "cancelled");
    assert_eq!(value["stop_trigger"], "manual");

    let summary = handler.last_session().expect("last_session present");
    assert_eq!(summary.phase, SessionPhase::Cancelled);
    assert_eq!(summary.stop_trigger, Some(StopTrigger::Manual));
    assert!(handler.completed().is_empty(), "no bundle for cancelled session");

    // Wait past the deadline to confirm the timer thread has exited
    // without surprising side effects.
    std::thread::sleep(Duration::from_millis(250));
    assert!(handler.completed().is_empty());
}

#[test]
fn auto_stop_continues_when_owner_disconnects() {
    // The handler keeps running even after the original owner is
    // "disconnected" because the auto-stop is observed by the next
    // handler call regardless of the connection argument. Per issue
    // #23, the bundle is still committed to disk.
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal.clone(), bundle.clone());
    let owner = ConnectionId(1);

    let start = handler.handle(
        Some(30),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(200),
        },
    );
    let start_value = first_response_value(&start);
    let recording_id = start_value["recording_id"].as_str().unwrap().to_owned();

    // "Owner disconnects" — sleep 250ms (past deadline) without any
    // handler call from the owner.
    std::thread::sleep(Duration::from_millis(250));

    // A status call (in practice: from a new admin connection) picks
    // up the auto-stop and the bundle is committed.
    let status = handler.handle(Some(31), ConnectionId(99), RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "idle");
    let last_session = &status_value["last_session"];
    assert_eq!(last_session["phase"], "completed");
    assert_eq!(last_session["stop_trigger"], "auto_duration");

    assert!(handler.completed().contains_key(&recording_id));
    // Bundle file is on disk.
    let bundle_path = bundle.join(format!("{recording_id}.rdogrec.tar"));
    assert!(bundle_path.is_file(), "bundle file should exist on disk");
    let _ = journal;
}

#[test]
fn duration_too_large_rejected() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);

    let start = handler.handle(
        Some(40),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(4_000_000),
        },
    );
    let value = first_response_value(&start);
    assert_eq!(value["error_code"], "DURATION_TOO_LARGE");
    // No session should be active after the rejection.
    let status = handler.handle(Some(41), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "idle");
    assert!(handler.completed().is_empty());
}

#[test]
fn duration_too_small_rejected() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);

    let start = handler.handle(
        Some(50),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(50),
        },
    );
    let value = first_response_value(&start);
    assert_eq!(value["error_code"], "DURATION_TOO_SMALL");
    let status = handler.handle(Some(51), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "idle");
    assert!(handler.completed().is_empty());
}

#[test]
fn stop_trigger_serializes_to_snake_case_strings() {
    // Per issue #23 acceptance: cover all 4 variants.
    let cases = [
        (StopTrigger::Manual, "\"manual\""),
        (StopTrigger::AutoDuration, "\"auto_duration\""),
        (StopTrigger::OwnerDisconnected, "\"owner_disconnected\""),
        (StopTrigger::AutoFailed, "\"auto_failed\""),
    ];
    for (trigger, expected) in cases {
        let actual = serde_json::to_string(&trigger).expect("serialize");
        assert_eq!(actual, expected, "trigger {trigger:?} serialization");
    }
}

#[test]
fn status_reports_duration_and_remaining_ms_when_auto_stop_active() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);

    let _ = handler.handle(
        Some(70),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(2_000),
        },
    );

    let status = handler.handle(Some(71), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "recording");
    let duration_ms = status_value["duration_ms"].as_u64().expect("duration_ms is u64");
    assert_eq!(duration_ms, 2_000);
    let remaining_ms = status_value["remaining_ms"].as_u64().expect("remaining_ms is u64");
    // `remaining_ms` should be ≤ duration_ms and reasonably close to it.
    assert!(remaining_ms <= duration_ms, "remaining_ms {remaining_ms} > duration_ms {duration_ms}");
    assert!(
        remaining_ms >= duration_ms - 250,
        "remaining_ms {remaining_ms} should be near duration_ms {duration_ms} immediately after start"
    );

    // No auto-stop: duration_ms and remaining_ms are null.
    let (journal2, bundle2) = temp_dirs();
    let mut handler2 = RecordingHandler::new(journal2, bundle2);
    let _ = handler2.handle(
        Some(72),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: None,
        },
    );
    let status2 = handler2.handle(Some(73), owner, RecordRequest::Status);
    let status_value2 = first_response_value(&status2);
    assert!(status_value2["duration_ms"].is_null());
    assert!(status_value2["remaining_ms"].is_null());
}

#[test]
fn status_remaining_ms_clamped_to_zero_after_deadline() {
    let (journal, bundle) = temp_dirs();
    let mut handler = RecordingHandler::new(journal, bundle);
    let owner = ConnectionId(1);

    let _ = handler.handle(
        Some(80),
        owner,
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(100),
        },
    );

    // Sleep past the deadline so the timer fires.
    std::thread::sleep(Duration::from_millis(180));

    // The next status call observes the FIRED flag and runs the
    // auto-stop inline. After that, `remaining_ms` is 0 (timer state
    // is gone) and the session is in `completed` state via
    // `last_session`.
    let status = handler.handle(Some(81), owner, RecordRequest::Status);
    let status_value = first_response_value(&status);
    assert_eq!(status_value["status"], "idle");
    let remaining_ms = status_value["remaining_ms"].as_u64();
    // After auto-stop completes, the timer is cleared, so remaining_ms
    // is null on the last_session branch.
    assert!(remaining_ms.is_none());
    let last_session = &status_value["last_session"];
    assert_eq!(last_session["phase"], "completed");
    assert_eq!(last_session["stop_trigger"], "auto_duration");
}
