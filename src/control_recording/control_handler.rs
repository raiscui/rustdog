//! line-control `@record-*` 命令到 LifecycleManager / DeliveryManager 的桥接层。
//!
//! 设计目标:
//! - 只暴露 4 个 line-control 入口 (`@record-start` / `@record-status`
//!   / `@record-stop` / `@record-cancel`),不重新实现 lifecycle。
//! - 所有响应走现有 `control_core::render_*_response` helper,
//!   避免再起一套 envelope 编码。
//! - `@record-mark` 在本 ticket 范围外(需要 session wrapper),
//!   暂时返回 not_implemented。

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::control_core::{
    render_protocol_error_response,
    render_structured_success_response,
};
use crate::control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame};

use super::{
    bundle::{write_bundle, Bundle},
    delivery::{DeliveryError, DeliveryFailureReason, DeliveryManager},
    journal::now_unix_ms,
    session::{
        ConnectionId, FailureDetail, FailureReason, LifecycleError, LifecycleManager, Profile,
        SessionPhase, StopTrigger, TerminalSummary,
    },
};

/// Auto-stop timer flag states. Shared between the timer thread and the
/// recording handler via `Arc<AtomicU8>`. See issue #23 acceptance.
pub const AUTO_STOP_PENDING: u8 = 0;
pub const AUTO_STOP_CANCELLED: u8 = 1;
pub const AUTO_STOP_FIRED: u8 = 2;

/// Owner-side handle for the auto-stop timer spawned by `start`. The
/// handler keeps it in `auto_stop_timer`; the timer thread owns the
/// matching `Arc<AtomicU8>` and observes it on every tick to break out
/// of the loop promptly on cancel / manual-stop / drop.
pub struct AutoStopTimer {
    /// Shared cancellation flag (see `AUTO_STOP_*` constants).
    pub flag: Arc<AtomicU8>,
    /// Worker thread handle. `None` after `take_join` has been called.
    pub join: Option<JoinHandle<()>>,
    /// Owner connection recorded at `start` time. Reserved for the
    /// follow-up owner-disconnect detector (issue #23 roadmap item).
    #[allow(dead_code)]
    pub owner: ConnectionId,
    /// Recording id recorded at `start` time so the auto-stop path can
    /// verify the session still matches.
    pub recording_id: String,
    /// Total duration the timer was configured for. Captured so the
    /// `@record-status` handler can compute `remaining_ms` without
    /// keeping a separate record.
    pub duration_ms: u64,
    /// Wall-clock start captured when the timer was spawned. Used
    /// together with `duration_ms` to compute remaining time.
    pub started_at: Instant,
}

impl AutoStopTimer {
    /// Set the flag to `cancelled` and join the worker thread. Idempotent
    /// on the flag (CAS on first win) and idempotent on join (the
    /// `JoinHandle` is moved out before `join` is called). The caller
    /// must drop the returned `JoinHandle` only once.
    pub fn cancel_and_join(&mut self) {
        self.flag.store(AUTO_STOP_CANCELLED, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    /// Compute `remaining_ms` for the auto-stop deadline. Returns 0
    /// once the wall clock has crossed the deadline or the timer has
    /// been fired. Used by the `@record-status` response so the owner
    /// can render a countdown.
    pub fn remaining_ms(&self) -> u64 {
        let elapsed = self.started_at.elapsed().as_millis() as u64;
        self.duration_ms.saturating_sub(elapsed)
    }
}

pub const RECORD_CONTROL_SCHEMA: &str = "rdog.record-control.v1";

/// line-control 一侧的 recording 请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRequest {
    Start { profile: Profile, duration_ms: Option<u64> },
    Status,
    Mark { label: Option<String>, redaction_active: bool },
    Stop,
    Cancel,
}

impl RecordRequest {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Start { .. } => "record-start",
            Self::Status => "record-status",
            Self::Mark { .. } => "record-mark",
            Self::Stop => "record-stop",
            Self::Cancel => "record-cancel",
        }
    }
}

/// 单实例的 recording handler。`LifecycleManager` 持续持有当前 Session,
/// `DeliveryManager` 跟踪每连接 stop 限流,`completed` 缓存已提交
/// Bundle 用于 completed retry readback。
pub struct RecordingHandler {
    lifecycle: LifecycleManager,
    delivery: DeliveryManager,
    journal_dir: PathBuf,
    bundle_dir: PathBuf,
    completed: HashMap<String, Bundle>,
    /// Active auto-stop timer, if any. Spawned by `start` when
    /// `duration_ms` is set; cleared by `stop` / `cancel` / `Drop` after
    /// the worker thread has been joined. Per issue #23.
    auto_stop_timer: Option<AutoStopTimer>,
    /// Handler-side override of the volatile `last_session` summary,
    /// updated with a `StopTrigger` after `complete_current` /
    /// `cancel_current` / `fail_current`. We can't mutate
    /// `LifecycleManager::last_session` directly without changing its
    /// API (frozen per spec) so the handler keeps its own copy and
    /// returns it from `last_session()` / `status()`.
    last_session_override: Option<TerminalSummary>,
}

impl RecordingHandler {
    pub fn new(journal_dir: PathBuf, bundle_dir: PathBuf) -> Self {
        Self {
            lifecycle: LifecycleManager::new(),
            delivery: DeliveryManager::default(),
            journal_dir,
            bundle_dir,
            completed: HashMap::new(),
            auto_stop_timer: None,
            last_session_override: None,
        }
    }

    /// Return the latest recorded session summary if any. Prefers the
    /// handler-side override (which carries a `StopTrigger`); falls back
    /// to the lifecycle manager's volatile summary for backwards
    /// compatibility. Per issue #23.
    pub fn last_session(&self) -> Option<&TerminalSummary> {
        self.last_session_override
            .as_ref()
            .or_else(|| self.lifecycle.last_session())
    }

    pub fn lifecycle(&self) -> &LifecycleManager { &self.lifecycle }
    pub fn lifecycle_mut(&mut self) -> &mut LifecycleManager { &mut self.lifecycle }
    pub fn delivery_mut(&mut self) -> &mut DeliveryManager { &mut self.delivery }
    pub fn completed(&self) -> &HashMap<String, Bundle> { &self.completed }

    /// 把 line-control 命令转换成 `ControlExecutionOutcome`。
    /// 必要时产出 `@savefile` + 终结 `@response`。
    pub fn handle(
        &mut self,
        request_id: Option<u64>,
        connection: ConnectionId,
        request: RecordRequest,
    ) -> ControlExecutionOutcome {
        // Observe the auto-stop timer first so a fired timer is honored
        // by whichever request comes in next. Per issue #23.
        self.check_auto_stop();
        match request {
            RecordRequest::Start { profile, duration_ms } => self.start(request_id, connection, profile, duration_ms),
            RecordRequest::Status => self.status(request_id),
            RecordRequest::Mark { label, redaction_active } => {
                self.mark(request_id, connection, label, redaction_active)
            }
            RecordRequest::Stop => self.stop(request_id, connection),
            RecordRequest::Cancel => self.cancel(request_id, connection),
        }
    }

    fn start(&mut self, request_id: Option<u64>, connection: ConnectionId, profile: Profile, duration_ms: Option<u64>) -> ControlExecutionOutcome {
        // A new session invalidates the previous summary view.
        self.last_session_override = None;
        // If a previous timer is still around (shouldn't happen but be
        // defensive), cancel + join before starting fresh.
        self.cancel_auto_stop_timer();
        if let Some(active) = self.lifecycle.current() {
            return protocol_error(request_id, 4101, json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-start",
                "error_code": "RECORDING_ALREADY_ACTIVE",
                "recording_id": active.recording_id(),
            }));
        }
        // Validate duration_ms before allocating the session record so
        // we fail fast on bad input. 0 is treated as "no duration" per
        // issue #23 ADR 4.
        if let Some(d) = duration_ms {
            if let Some(err_outcome) = validate_duration(d, request_id) {
                return err_outcome;
            }
        }
        let recording_id = generate_recording_id();
        let started_at = now_unix_ms();
        let journal_path = self.journal_dir.join(format!("{recording_id}.journal.jsonl"));
        if let Err(err) = std::fs::create_dir_all(&self.journal_dir) {
            return io_error_outcome(request_id, &err);
        }
        let platform = super::journal::PlatformInfo {
            os: std::env::consts::OS.to_owned(),
            capture_backend: "auto".to_owned(),
        };
        if let Err(err) = self.lifecycle.start(recording_id.clone(), profile, connection, journal_path, platform, started_at) {
            return lifecycle_outcome(request_id, &err);
        }
        // Spawn the auto-stop timer AFTER the session is registered so a
        // timer that fires almost immediately will find a real session
        // to finalize. Skip when duration_ms is None or 0.
        if let Some(d) = duration_ms {
            if d > 0 {
                self.spawn_auto_stop_timer(connection, recording_id.clone(), d);
            }
        }
        let body = json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-start",
            "status": "recording",
            "recording_id": self.lifecycle.current().map(|s| s.recording_id()).unwrap_or(""),
            "profile": profile_name(profile),
            "started_at_unix_ms": self.lifecycle.current().map(|s| s.started_at_unix_ms()).unwrap_or(0),
            "duration_ms": duration_ms,
        });
        success(request_id, &body)
    }

    /// Spawn the auto-stop timer thread. The thread polls the shared
    /// flag every 100 ms and, on timeout, performs the auto-stop path
    /// (same as manual `stop` minus the framework response).
    fn spawn_auto_stop_timer(&mut self, owner: ConnectionId, recording_id: String, duration_ms: u64) {
        let flag = Arc::new(AtomicU8::new(AUTO_STOP_PENDING));
        let flag_clone = Arc::clone(&flag);
        let join = thread::spawn(move || {
            auto_stop_worker(flag_clone, duration_ms);
        });
        self.auto_stop_timer = Some(AutoStopTimer {
            flag,
            join: Some(join),
            owner,
            recording_id,
            duration_ms,
            started_at: Instant::now(),
        });
    }

    fn status(&self, request_id: Option<u64>) -> ControlExecutionOutcome {
        let body = if let Some(session) = self.lifecycle.current() {
            // `remaining_ms` reflects the auto-stop countdown. We
            // populate it next to `duration_ms` so the owner can render
            // a progress bar without a separate query. `None` when no
            // timer is active (manual stop only).
            let (duration_ms, remaining_ms) = match self.auto_stop_timer.as_ref() {
                Some(timer) => (Some(timer.duration_ms), Some(timer.remaining_ms())),
                None => (None, None),
            };
            json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-status",
                "status": "recording",
                "recording_id": session.recording_id(),
                "phase": phase_name(session.phase()),
                "started_at_unix_ms": session.started_at_unix_ms(),
                "event_count": session.event_count(),
                "mark_count": session.mark_count(),
                "gap_count": session.gap_count(),
                "owner_present": true,
                "delivery_status": "pending",
                "duration_ms": duration_ms,
                "remaining_ms": remaining_ms,
            })
        } else if let Some(summary) = self.last_session() {
            json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-status",
                "status": "idle",
                "last_session": {
                    "recording_id": summary.recording_id,
                    "phase": phase_name(summary.phase),
                    "ended_at_unix_ms": summary.ended_at_unix_ms,
                    "event_count": summary.event_count,
                    "mark_count": summary.mark_count,
                    "gap_count": summary.gap_count,
                    "failure": summary.failure,
                    "stop_trigger": summary.stop_trigger.as_ref().map(|t| t.as_str()),
                },
            })
        } else {
            json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-status",
                "status": "idle",
            })
        };
        success(request_id, &body)
    }

    fn stop(&mut self, request_id: Option<u64>, connection: ConnectionId) -> ControlExecutionOutcome {
        // Cancel the auto-stop timer first so the worker thread does not
        // race with the manual stop path. Per issue #23.
        self.cancel_auto_stop_timer();
        if let Err(err) = self.delivery.check_record_stop(connection) {
            if let DeliveryError::Rejected(reason) = err {
                if matches!(reason, DeliveryFailureReason::RateLimited) {
                    return protocol_error(request_id, 4200, json!({
                        "schema": RECORD_CONTROL_SCHEMA,
                        "kind": "record-stop",
                        "error_code": "DELIVERY_RATE_LIMITED",
                    }));
                }
            }
        }
        let journal_path = match self.lifecycle.current_mut() {
            Some(s) if s.owner() == connection => s.begin_finalize(),
            Some(_) => return protocol_error(request_id, 4102, json!({"error_code": "RECORD_NOT_OWNER"})),
            None => return protocol_error(request_id, 4103, json!({"error_code": "RECORD_NO_ACTIVE_SESSION"})),
        };
        let journal_path = match journal_path {
            Ok(p) => p,
            Err(err) => {
                if let Some(session) = self.lifecycle.current_mut() {
                    let _ = session.fail(FailureDetail { reason: FailureReason::FinalizeError, detail: err.to_string() });
                }
                return lifecycle_outcome(request_id, &err);
            }
        };
        let recording_id = self.lifecycle.current().map(|s| s.recording_id().to_owned()).unwrap_or_default();
        let started_at = self.lifecycle.current().map(|s| s.started_at_unix_ms()).unwrap_or(0);
        let flow = placeholder_flow();
        let bundle = match write_bundle(&self.bundle_dir, &recording_id, started_at, &journal_path, &flow, 0) {
            Ok(b) => b,
            Err(err) => {
                if let Some(session) = self.lifecycle.current_mut() {
                    let _ = session.fail(FailureDetail { reason: FailureReason::FinalizeError, detail: err.to_string() });
                }
                return protocol_error(request_id, 4107, json!({
                    "schema": RECORD_CONTROL_SCHEMA,
                    "kind": "record-stop",
                    "error_code": "BUNDLE_COMMIT_FAILED",
                    "detail": err.to_string(),
                }));
            }
        };
        let summary = match self.lifecycle.complete_current() {
            Ok(s) => s.with_trigger(StopTrigger::Manual),
            Err(err) => return lifecycle_outcome(request_id, &err),
        };
        self.last_session_override = Some(summary.clone());
        self.completed.insert(recording_id.clone(), bundle.clone());
        let body = json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-stop",
            "recording_id": recording_id,
            "phase": phase_name(summary.phase),
            "bundle_filename": bundle.path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "bundle_size_bytes": bundle.size_bytes,
            "bundle_sha256": bundle.sha256,
            "delivery_status": "delivered",
            "trigger": summary.stop_trigger.as_ref().map(|t| t.as_str()),
        });
        let frame = savefile_for_bundle(&bundle, request_id);
        let response = render_structured_success_response(request_id, &serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_owned()));
        ControlExecutionOutcome { outbound_frames: vec![ControlFrame::SaveFile(frame), ControlFrame::ResponseLine(response)] }
    }

    fn mark(&mut self, request_id: Option<u64>, connection: ConnectionId, label: Option<String>, redaction_active: bool) -> ControlExecutionOutcome {
        let session = match self.lifecycle.current_mut() {
            Some(s) if s.owner() == connection => s,
            Some(_) => return protocol_error(request_id, 4102, json!({"error_code": "RECORD_NOT_OWNER"})),
            None => return protocol_error(request_id, 4103, json!({"error_code": "RECORD_NO_ACTIVE_SESSION"})),
        };
        let label_value = label.unwrap_or_else(|| "mark".to_owned());
        if let Err(err) = session.mark(label_value.clone(), redaction_active) {
            return lifecycle_outcome(request_id, &err);
        }
        success(request_id, &json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-mark",
            "recording_id": session.recording_id(),
            "label": label_value,
            "redaction_active": redaction_active,
            "mark_count": session.mark_count(),
        }))
    }

    fn cancel(&mut self, request_id: Option<u64>, connection: ConnectionId) -> ControlExecutionOutcome {
        // Cancel the auto-stop timer first so the worker thread does not
        // race with the manual cancel path. Per issue #23.
        self.cancel_auto_stop_timer();
        let recording_id = match self.lifecycle.current() {
            Some(s) if s.owner() == connection => s.recording_id().to_owned(),
            Some(_) => return protocol_error(request_id, 4102, json!({"error_code": "RECORD_NOT_OWNER"})),
            None => return protocol_error(request_id, 4103, json!({"error_code": "RECORD_NO_ACTIVE_SESSION"})),
        };
        match self.lifecycle.cancel_current() {
            Ok(summary) => {
                let summary = summary.with_trigger(StopTrigger::Manual);
                self.last_session_override = Some(summary.clone());
                success(request_id, &json!({
                    "schema": RECORD_CONTROL_SCHEMA,
                    "kind": "record-cancel",
                    "recording_id": recording_id,
                    "phase": "cancelled",
                    "stop_trigger": summary.stop_trigger.as_ref().map(|t| t.as_str()),
                }))
            }
            Err(err) => lifecycle_outcome(request_id, &err),
        }
    }

    /// Cancel the auto-stop timer and join its worker thread, if any.
    /// Safe to call repeatedly.
    fn cancel_auto_stop_timer(&mut self) {
        if let Some(mut timer) = self.auto_stop_timer.take() {
            timer.cancel_and_join();
        }
    }

    /// Detect a fired auto-stop timer and run the auto-stop path
    /// inline. Called at the start of every `handle` method so the
    /// handler observes the timer regardless of which request comes in
    /// next. Per issue #23.
    fn check_auto_stop(&mut self) {
        let recording_id = match self.auto_stop_timer.as_ref() {
            None => return,
            Some(timer) if timer.flag.load(Ordering::Acquire) != AUTO_STOP_FIRED => return,
            Some(timer) => timer.recording_id.clone(),
        };
        self.auto_stop_internal(&recording_id);
        // Clear the timer now that auto-stop has run; the worker thread
        // is already done at this point.
        self.auto_stop_timer = None;
    }

    /// Run the auto-stop stop path inline. Reuses the same steps as
    /// `stop` (begin_finalize → write_bundle → complete_current) but
    /// does not publish a control response. The result is captured in
    /// `last_session_override` with `StopTrigger::AutoDuration`.
    fn auto_stop_internal(&mut self, recording_id: &str) {
        // Guard: owner check + phase check. If the session is no longer
        // in `Recording` for any reason (already stopped/cancelled),
        // the auto-stop is a no-op.
        let session_match = self
            .lifecycle
            .current()
            .map(|s| s.recording_id() == recording_id && s.phase() == SessionPhase::Recording)
            .unwrap_or(false);
        if !session_match {
            return;
        }
        let journal_path = match self.lifecycle.current_mut() {
            Some(s) => s.begin_finalize(),
            None => return,
        };
        let journal_path = match journal_path {
            Ok(p) => p,
            Err(err) => {
                if let Some(session) = self.lifecycle.current_mut() {
                    let _ = session.fail(FailureDetail {
                        reason: FailureReason::FinalizeError,
                        detail: err.to_string(),
                    });
                }
                return;
            }
        };
        let rid = recording_id.to_owned();
        let started_at = self
            .lifecycle
            .current()
            .map(|s| s.started_at_unix_ms())
            .unwrap_or(0);
        let flow = placeholder_flow();
        let bundle = match write_bundle(&self.bundle_dir, &rid, started_at, &journal_path, &flow, 0) {
            Ok(b) => b,
            Err(_) => {
                if let Some(session) = self.lifecycle.current_mut() {
                    let _ = session.fail(FailureDetail {
                        reason: FailureReason::FinalizeError,
                        detail: "bundle commit failed".to_owned(),
                    });
                }
                return;
            }
        };
        let summary = match self.lifecycle.complete_current() {
            Ok(s) => s.with_trigger(StopTrigger::AutoDuration),
            Err(_) => return,
        };
        self.last_session_override = Some(summary);
        self.completed.insert(rid, bundle);
    }
}

impl Drop for RecordingHandler {
    fn drop(&mut self) {
        // Per issue #23: if the handler is dropped while a timer is
        // still running, signal the worker to exit and join before this
        // object's fields go away. The worker only references captured
        // Arcs and an `Arc<Mutex<RecordingHandler>>` via the global
        // slot, but the captured state is consulted only on the auto-stop
        // path so a join before drop is still the safe default.
        self.cancel_auto_stop_timer();
    }
}

fn profile_name(profile: Profile) -> &'static str { match profile { Profile::Semantic => "semantic", Profile::Physical => "physical" } }
fn phase_name(phase: SessionPhase) -> &'static str {
    match phase {
        SessionPhase::Recording => "recording",
        SessionPhase::Finalizing => "finalizing",
        SessionPhase::Completed => "completed",
        SessionPhase::Failed => "failed",
        SessionPhase::Cancelled => "cancelled",
    }
}

fn generate_recording_id() -> String {
    let nanos = now_unix_ms();
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_be_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(20);
    for byte in &digest[..10] { let _ = std::fmt::write(&mut out, format_args!("{byte:02x}")); }
    format!("rec-{out}")
}

fn placeholder_flow() -> Value {
    json!({
        "schema": "rdog.flow.v1",
        "policy": {"best_effort": true},
        "compiler": {"name": "rdog-replay-compiler", "version": "1"},
        "steps": [],
    })
}

fn savefile_for_bundle(bundle: &Bundle, request_id: Option<u64>) -> SaveFileFrame {
    let bytes = std::fs::read(&bundle.path).unwrap_or_default();
    SaveFileFrame {
        request_id,
        filename: bundle.path.file_name().and_then(|n| n.to_str()).unwrap_or("bundle.rdogrec.tar").to_owned(),
        mime: "application/vnd.rdog.recording-bundle".to_owned(),
        encoding: "base64".to_owned(),
        data: BASE64_STANDARD.encode(bytes),
        quality: None,
        width: None,
        height: None,
    }
}

fn success(request_id: Option<u64>, value: &Value) -> ControlExecutionOutcome {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned());
    ControlExecutionOutcome::from_response_line(render_structured_success_response(request_id, &body))
}

fn protocol_error(request_id: Option<u64>, code: i32, value: Value) -> ControlExecutionOutcome {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned());
    ControlExecutionOutcome::from_response_line(render_protocol_error_response(request_id, code, &body))
}

fn lifecycle_outcome(request_id: Option<u64>, err: &LifecycleError) -> ControlExecutionOutcome {
    protocol_error(request_id, 4106, json!({
        "schema": RECORD_CONTROL_SCHEMA,
        "error_code": "RECORD_LIFECYCLE_ERROR",
        "detail": err.to_string(),
    }))
}

fn io_error_outcome(request_id: Option<u64>, err: &std::io::Error) -> ControlExecutionOutcome {
    let body = serde_json::to_string(&json!({
        "schema": RECORD_CONTROL_SCHEMA,
        "error_code": "RECORD_IO_ERROR",
        "detail": err.to_string(),
    })).unwrap_or_else(|_| "{}".to_owned());
    ControlExecutionOutcome::from_response_line(render_protocol_error_response(request_id, 4110, &body))
}

/// Validate that `duration_ms` is within the accepted range. Returning
/// `Some(outcome)` short-circuits `start` with a structured error
/// response; `None` means the value is acceptable. Per issue #23:
/// 4_000_000 ms (66.6 min) maps to code 4120, 50 ms maps to 4121.
fn validate_duration(d: u64, request_id: Option<u64>) -> Option<ControlExecutionOutcome> {
    // Issue #23 acceptance: 50 ms → 4121, 4_000_000 ms → 4120.
    const MIN_MS: u64 = 100;
    const MAX_MS: u64 = 3_600_000;
    if d < MIN_MS {
        return Some(protocol_error(request_id, 4121, json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-start",
            "error_code": "DURATION_TOO_SMALL",
            "min_ms": MIN_MS,
            "got_ms": d,
        })));
    }
    if d > MAX_MS {
        return Some(protocol_error(request_id, 4120, json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-start",
            "error_code": "DURATION_TOO_LARGE",
            "max_ms": MAX_MS,
            "got_ms": d,
        })));
    }
    None
}

/// Auto-stop worker thread. Polls `flag` every 100 ms; when the
/// deadline is reached and the flag is still `PENDING`, atomically
/// transitions it to `FIRED`. The actual `auto_stop` work happens
/// inline in the next `RecordingHandler` call (which already holds
/// the lock), avoiding cross-thread lock contention. Per issue #23.
fn auto_stop_worker(flag: Arc<AtomicU8>, duration_ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    while flag.load(Ordering::Acquire) == AUTO_STOP_PENDING {
        let now = Instant::now();
        if now >= deadline {
            // Best-effort CAS: if a concurrent cancel/stop already set
            // the flag to CANCELLED, this is a no-op and the thread
            // exits without firing the auto-stop.
            let _ = flag.compare_exchange(
                AUTO_STOP_PENDING,
                AUTO_STOP_FIRED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            break;
        }
        // Sleep at most the time remaining, capped at 100 ms so cancel
        // and stop are observed within at most one tick.
        let remaining = deadline.saturating_duration_since(now);
        let sleep = remaining.min(Duration::from_millis(100));
        thread::sleep(sleep);
    }
}

fn not_implemented(request_id: Option<u64>, kind: &str) -> ControlExecutionOutcome {
    protocol_error(request_id, 4109, json!({
        "schema": RECORD_CONTROL_SCHEMA,
        "kind": kind,
        "error_code": "NOT_IMPLEMENTED",
        "detail": "this record control variant is intentionally deferred",
    }))
}


use std::sync::{Mutex, OnceLock};

static RECORDING_HANDLER: OnceLock<Mutex<RecordingHandler>> = OnceLock::new();

/// 全局注册一个 `RecordingHandler`。只能调用一次,daemon 启动时调用。
pub fn install_recording_handler(handler: RecordingHandler) -> Result<(), RecordingHandler> {
    RECORDING_HANDLER.set(Mutex::new(handler)).map_err(|h| h.into_inner().expect("just inserted"))
}

/// 访问已安装的 handler。`None` 表示 daemon 还没初始化 recording 栈。
pub fn with_recording_handler<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut RecordingHandler) -> R,
{
    let lock = RECORDING_HANDLER.get()?;
    let mut guard = lock.lock().ok()?;
    Some(f(&mut guard))
}
