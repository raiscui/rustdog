use super::{
    control_handler::{MarkKind, RecordRequest, RecordingHandler},
    session::{ConnectionId, Profile},
    BoundedQueue, CaptureEvent, CaptureSpec, CaptureStop, CaptureTarget, CapturedInput,
    RecorderCapture, RecorderError,
};
use crate::control_frames::{ControlExecutionOutcome, ControlFrame};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

struct Fake {
    queue: Arc<BoundedQueue>,
    healthy: Arc<AtomicBool>,
    tail: bool,
}
impl RecorderCapture for Fake {
    fn start(&mut self, _: &CaptureSpec) -> Result<(), RecorderError> {
        Ok(())
    }
    fn stop(
        &mut self,
        drain: &mut dyn FnMut(&BoundedQueue) -> std::io::Result<()>,
    ) -> Result<CaptureStop, RecorderError> {
        drain(&self.queue).map_err(|error| RecorderError::Backend(error.to_string()))?;
        if self.tail {
            self.queue.push(event(1));
            drain(&self.queue).map_err(|error| RecorderError::Backend(error.to_string()))?;
            Ok(CaptureStop {
                last_sequence: Some(1),
            })
        } else {
            Ok(CaptureStop {
                last_sequence: None,
            })
        }
    }
    fn parameter_boundary(&mut self) -> Result<(), RecorderError> {
        Ok(())
    }
    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }
    fn wall_clock_anchor_ms(&self) -> u64 {
        1
    }
    fn cleanup_pending(&mut self) -> bool {
        false
    }
    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }
}
struct PendingCapture {
    queue: BoundedQueue,
    pending: Arc<AtomicBool>,
}
impl RecorderCapture for PendingCapture {
    fn start(&mut self, _: &CaptureSpec) -> Result<(), RecorderError> {
        Ok(())
    }
    fn stop(
        &mut self,
        _: &mut dyn FnMut(&BoundedQueue) -> std::io::Result<()>,
    ) -> Result<CaptureStop, RecorderError> {
        Err(RecorderError::Backend("not reaped".into()))
    }
    fn parameter_boundary(&mut self) -> Result<(), RecorderError> {
        Ok(())
    }
    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }
    fn wall_clock_anchor_ms(&self) -> u64 {
        1
    }
    fn is_healthy(&self) -> bool {
        true
    }
    fn cleanup_pending(&mut self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }
}
fn event(sequence: u64) -> CaptureEvent {
    CaptureEvent {
        sequence,
        monotonic_ns: sequence,
        input: CapturedInput::SemanticMissing { for_sequence: 0 },
    }
}
fn dir() -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "rdog-rec-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}
fn fixture(tail: bool, capacity: usize) -> (RecordingHandler, Arc<BoundedQueue>, PathBuf) {
    let path = dir();
    let queue = Arc::new(BoundedQueue::with_capacity(capacity));
    let healthy = Arc::new(AtomicBool::new(true));
    let q = queue.clone();
    let handler = RecordingHandler::with_test_factory(
        path.clone(),
        Box::new(move || {
            Box::new(Fake {
                queue: q.clone(),
                healthy: healthy.clone(),
                tail,
            })
        }),
    );
    (handler, queue, path)
}
fn value(outcome: &ControlExecutionOutcome) -> Value {
    let line = outcome
        .outbound_frames
        .iter()
        .find_map(|frame| {
            if let ControlFrame::ResponseLine(s) = frame {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    let envelope: Value = serde_json::from_str(line.strip_prefix("@response ").unwrap()).unwrap();
    if let Some(v) = envelope.get("value") {
        return v.clone();
    }
    envelope
        .get("error")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(envelope)
}
fn start(handler: &mut RecordingHandler, duration_ms: u64) -> String {
    let out = handler.handle(
        Some(1),
        ConnectionId(1),
        RecordRequest::Start {
            profile: Profile::Semantic,
            duration_ms: Some(duration_ms),
            targets: vec![CaptureTarget { pid: 1, hwnd: 1 }],
        },
    );
    value(&out)["recording_id"].as_str().unwrap().to_owned()
}
fn status(handler: &mut RecordingHandler, id: &str) -> Value {
    value(&handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Status {
            recording_id: Some(id.into()),
        },
    ))
}
#[test]
fn stop_drains_tail_and_exports_exact_sealed_bytes_once() {
    let (mut handler, queue, _) = fixture(true, 8);
    let id = start(&mut handler, 10_000);
    assert!(queue.push(event(0)));
    let out = handler.handle(
        Some(3),
        ConnectionId(1),
        RecordRequest::Stop {
            recording_id: id.clone(),
        },
    );
    let ControlFrame::SaveFile(file) = &out.outbound_frames[0] else {
        panic!("missing artifact")
    };
    let bytes = BASE64.decode(&file.data).unwrap();
    let meta = value(&out);
    assert_eq!(
        meta["artifact_size_bytes"].as_u64().unwrap(),
        bytes.len() as u64
    );
    assert_eq!(
        meta["artifact_sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    let lines: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter_map(|v| v["capture_seq"].as_u64())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(lines.last().unwrap()["payload"]["type"], "recorded");
    let again = handler.handle(
        Some(4),
        ConnectionId(1),
        RecordRequest::Stop { recording_id: id },
    );
    assert_eq!(again.outbound_frames.len(), 1);
}
#[test]
fn nonowner_and_anonymous_cannot_mark_stop_or_cancel() {
    let (mut handler, _, _) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    for owner in [ConnectionId(0), ConnectionId(2)] {
        for request in [
            RecordRequest::Mark {
                recording_id: id.clone(),
                mark: MarkKind::Checkpoint,
            },
            RecordRequest::Stop {
                recording_id: id.clone(),
            },
            RecordRequest::Cancel {
                recording_id: id.clone(),
            },
        ] {
            assert_eq!(
                value(&handler.handle(Some(1), owner, request))["error_code"],
                "RECORD_NOT_OWNER"
            );
        }
    }
    assert_eq!(status(&mut handler, &id)["phase"], "recording");
}
#[test]
fn cancel_removes_staging_and_does_not_export() {
    let (mut handler, _, path) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    let out = handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Cancel {
            recording_id: id.clone(),
        },
    );
    assert_eq!(value(&out)["phase"], "cancelled");
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
    assert_eq!(status(&mut handler, &id)["phase"], "cancelled");
}
#[test]
fn timer_seals_without_polling_and_then_delivers() {
    let (mut handler, _, path) = fixture(false, 8);
    let id = start(&mut handler, 40);
    std::thread::sleep(Duration::from_millis(200));
    assert!(path.join(format!("{id}.rdogrec.jsonl")).exists());
    assert_eq!(status(&mut handler, &id)["phase"], "recorded");
    assert_eq!(
        handler
            .handle(
                Some(3),
                ConnectionId(1),
                RecordRequest::Stop { recording_id: id }
            )
            .outbound_frames
            .len(),
        2
    );
}
#[test]
fn overflow_fails_instead_of_sealing() {
    let (mut handler, queue, _) = fixture(false, 0);
    let id = start(&mut handler, 10_000);
    assert!(!queue.push(event(0)));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(status(&mut handler, &id)["phase"], "failed");
}
#[test]
fn queue_overflow_remains_sticky_after_drain() {
    let queue = BoundedQueue::with_capacity(1);
    assert!(queue.push(event(0)));
    assert!(!queue.push(event(1)));
    assert_eq!(queue.drain_all(|_| {}), 1);
    assert!(queue.overflowed());
}
#[test]
fn sequence_gap_fails_the_recording() {
    let (mut handler, queue, path) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    assert!(queue.push(event(1)));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(status(&mut handler, &id)["phase"], "failed");
    assert!(!path.join(format!("{id}.rdogrec.jsonl")).exists());
}
#[test]
fn anonymous_status_does_not_reveal_recording_identity() {
    let (mut handler, _, _) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    let out = handler.handle(
        Some(2),
        ConnectionId(0),
        RecordRequest::Status {
            recording_id: Some(id),
        },
    );
    let response = value(&out);
    assert_eq!(response["phase"], "recording");
    assert!(response.get("recording_id").is_none());
}
#[test]
fn missing_staging_file_during_seal_is_failure() {
    let (mut handler, _, path) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    std::fs::remove_file(path.join(format!(".{id}.staging"))).unwrap();
    let out = handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Stop {
            recording_id: id.clone(),
        },
    );
    assert_eq!(value(&out)["error_code"], "RECORD_FAILED");
    assert!(!path.join(format!("{id}.rdogrec.jsonl")).exists());
}
#[cfg(unix)]
#[test]
fn cancel_reports_staging_cleanup_failure_to_owner() {
    let (mut handler, _, path) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    let staging = path.join(format!(".{id}.staging"));
    let moved = path.join("held-open");
    std::fs::rename(&staging, &moved).unwrap();
    std::fs::create_dir(&staging).unwrap();
    let out = handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Cancel {
            recording_id: id.clone(),
        },
    );
    let response = value(&out);
    assert_eq!(response["error_code"], "RECORD_STAGING_CLEANUP_FAILED");
    assert_eq!(response["cleanup_residual_path"], staging.to_str().unwrap());
    assert!(staging.is_dir());
    assert_eq!(status(&mut handler, &id)["phase"], "failed");
    let anonymous = handler.handle(
        Some(3),
        ConnectionId(0),
        RecordRequest::Status { recording_id: None },
    );
    assert!(value(&anonymous).get("cleanup_residual_path").is_none());
}
#[test]
fn owner_close_discards_unsealed_capture() {
    let (mut handler, _, path) = fixture(false, 8);
    let id = start(&mut handler, 10_000);
    handler.close_owner_for_test(ConnectionId(1));
    assert_eq!(status(&mut handler, &id)["phase"], "idle");
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
}
#[test]
fn failed_stop_blocks_new_start_until_capture_is_reaped() {
    let pending = Arc::new(AtomicBool::new(true));
    let shared = pending.clone();
    let mut handler = RecordingHandler::with_test_factory(
        dir(),
        Box::new(move || {
            Box::new(PendingCapture {
                queue: BoundedQueue::new(),
                pending: shared.clone(),
            })
        }),
    );
    let id = start(&mut handler, 10_000);
    let stop = handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Stop { recording_id: id.clone() },
    );
    assert_eq!(value(&stop)["error_code"], "RECORD_FAILED");
    assert_eq!(status(&mut handler, &id)["cleanup_status"], "pending");
    let request = RecordRequest::Start {
        profile: Profile::Physical,
        duration_ms: None,
        targets: vec![CaptureTarget { pid: 1, hwnd: 1 }],
    };
    assert_eq!(
        value(&handler.handle(Some(3), ConnectionId(1), request.clone()))["error_code"],
        "RECORD_CLEANUP_PENDING"
    );
    pending.store(false, Ordering::SeqCst);
    assert_eq!(
        value(&handler.handle(Some(4), ConnectionId(1), request))["phase"],
        "recording"
    );
}
#[cfg(not(windows))]
#[test]
fn unsupported_platform_does_not_start() {
    let mut handler = RecordingHandler::new(dir());
    let out = handler.handle(
        Some(1),
        ConnectionId(1),
        RecordRequest::Start {
            profile: Profile::Physical,
            duration_ms: None,
            targets: vec![CaptureTarget { pid: 1, hwnd: 1 }],
        },
    );
    assert_eq!(value(&out)["error_code"], "RECORD_UNAVAILABLE");
}

struct StopBurst {
    queue: BoundedQueue,
}
impl RecorderCapture for StopBurst {
    fn start(&mut self, _: &CaptureSpec) -> Result<(), RecorderError> {
        Ok(())
    }
    fn stop(
        &mut self,
        drain: &mut dyn FnMut(&BoundedQueue) -> std::io::Result<()>,
    ) -> Result<CaptureStop, RecorderError> {
        for sequence in 0..64 {
            drain(&self.queue).map_err(|error| RecorderError::Backend(error.to_string()))?;
            if !self.queue.push(event(sequence)) {
                return Err(RecorderError::Backend("tail overflow".into()));
            }
        }
        drain(&self.queue).map_err(|error| RecorderError::Backend(error.to_string()))?;
        Ok(CaptureStop {
            last_sequence: Some(63),
        })
    }
    fn parameter_boundary(&mut self) -> Result<(), RecorderError> {
        Ok(())
    }
    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }
    fn wall_clock_anchor_ms(&self) -> u64 {
        1
    }
    fn cleanup_pending(&mut self) -> bool {
        false
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

#[test]
fn stop_consumes_more_inflight_events_than_queue_capacity() {
    let path = dir();
    let mut handler = RecordingHandler::with_test_factory(
        path.clone(),
        Box::new(|| {
            Box::new(StopBurst {
                queue: BoundedQueue::with_capacity(1),
            })
        }),
    );
    let id = start(&mut handler, 10_000);
    let out = handler.handle(
        Some(2),
        ConnectionId(1),
        RecordRequest::Stop { recording_id: id },
    );
    let ControlFrame::SaveFile(file) = &out.outbound_frames[0] else {
        panic!("停止期间没有完整排空");
    };
    let bytes = BASE64.decode(&file.data).unwrap();
    let lines: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines
            .iter()
            .filter_map(|line| line["capture_seq"].as_u64())
            .collect::<Vec<_>>(),
        (0..64).collect::<Vec<_>>()
    );
    assert_eq!(lines.last().unwrap()["payload"]["type"], "recorded");
    std::fs::remove_dir_all(path).unwrap();
}
