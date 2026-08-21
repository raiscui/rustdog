//! Recording Session lifecycle state machine.
//!
//! Per ticket `#18` and `specs/rdog-recording-session-lifecycle.md`.
//! Defines the 5-state lifecycle: `recording` / `finalizing` / `completed` /
//! `failed` / `cancelled`, the single-active slot, crash recovery, and the
//! lane health transitions.
//!
//! ponytail: minimum-viable state machine. The control-plane protocol
//! frame parser (`@record-start` etc.) lands in a follow-up ticket; this
//! module exposes a typed API the protocol layer calls into.

#![cfg_attr(test, allow(dead_code))]

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};

use super::{
    journal::{
        JournalError, JournalWriter, LaneTransition, Mark, PlatformInfo, SessionTerminalState,
        WallClockAnchor, JOURNAL_SCHEMA,
    },
    platform_capture, CaptureEvent, RecorderCapture, RecorderError, ShutdownSignal,
};

/// Recording profile (`semantic` / `physical`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Semantic,
    Physical,
}

impl Profile {
    fn as_str(&self) -> &'static str {
        match self {
            Profile::Semantic => "semantic",
            Profile::Physical => "physical",
        }
    }

    /// Required lanes for this profile. Source: spec §Permission and lane health.
    fn required_lanes(&self) -> &'static [&'static str] {
        match self {
            Profile::Semantic => &["event_listen", "accessibility", "tap_health"],
            Profile::Physical => &["event_listen", "tap_health"],
        }
    }
}

/// Session phase. `Idle` is the manager phase; only one of
/// `Recording` / `Finalizing` / `Completed` / `Failed` / `Cancelled` can be
/// active at a time per manager (single-active slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Recording,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

impl SessionPhase {
    fn as_str(&self) -> &'static str {
        match self {
            SessionPhase::Recording => "recording",
            SessionPhase::Finalizing => "finalizing",
            SessionPhase::Completed => "completed",
            SessionPhase::Failed => "failed",
            SessionPhase::Cancelled => "cancelled",
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionPhase::Completed | SessionPhase::Failed | SessionPhase::Cancelled
        )
    }
}

/// Lane health snapshot. `Unchecked` is the initial value before the
/// permission / health gate runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneState {
    Available,
    Unavailable,
    Denied,
    Disabled,
}

/// Lane record. `generation` increments on every transition so consumers
/// can dedupe `lane_status` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaneRecord {
    pub state: LaneState,
    pub generation: u64,
}

/// One transition trigger that ended a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    RequiredLaneFailure,
    OwnerDisconnected,
    OwnerCancelled,
    FinalizeError,
    PermissionRevoked,
}

/// Stop trigger that ended a session. Exposed via `TerminalSummary` and
/// the `@record-status` / `@record-stop` responses so callers can
/// distinguish manual stops from auto-stop and other daemon-side
/// triggers. Per issue #23 acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopTrigger {
    /// Owner issued `@record-stop` or `@record-cancel` explicitly.
    Manual,
    /// `RecordingHandler` auto-stop timer fired after `--duration`.
    AutoDuration,
    /// Owner WebSocket / TCP connection dropped; recording continues to
    /// the next event-boundary then ends.
    OwnerDisconnected,
    /// Lifecycle transitioned to `Failed`; trigger kind is the failure
    /// reason itself, but this label keeps the summary uniform.
    AutoFailed,
}

impl StopTrigger {
    /// Stable snake-case name used by the JSON wire protocol.
    pub fn as_str(&self) -> &'static str {
        match self {
            StopTrigger::Manual => "manual",
            StopTrigger::AutoDuration => "auto_duration",
            StopTrigger::OwnerDisconnected => "owner_disconnected",
            StopTrigger::AutoFailed => "auto_failed",
        }
    }
}

/// Failure detail recorded when a session transitions to `Failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureDetail {
    pub reason: FailureReason,
    pub detail: String,
}

/// Volatile terminal summary retained after a session ends, until the next
/// `start` or daemon restart. Per spec §`@record-status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalSummary {
    pub recording_id: String,
    pub phase: SessionPhase,
    pub ended_at_unix_ms: u64,
    pub event_count: u64,
    pub mark_count: u64,
    pub gap_count: u64,
    pub failure: Option<FailureDetail>,
    /// What triggered the session to end. `None` only for summaries
    /// produced before this field existed; new code should always set
    /// one via `with_trigger`. Per issue #23.
    pub stop_trigger: Option<StopTrigger>,
}

impl TerminalSummary {
    /// Builder-style setter for `stop_trigger`. Returns the modified
    /// summary so callers can chain. The `RecordingHandler` calls this
    /// after `LifecycleManager::complete_current` / `cancel_current` /
    /// `fail_current` because the manager-side API does not accept a
    /// trigger (kept stable per spec).
    pub fn with_trigger(mut self, trigger: StopTrigger) -> Self {
        self.stop_trigger = Some(trigger);
        self
    }
}

/// Owner connection identifier. Production code maps to the line-control
/// connection identity; the prototype uses an opaque u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub u64);

/// Crate error surface for the lifecycle.
#[derive(Debug)]
pub enum LifecycleError {
    /// A session is already active on the manager.
    AlreadyActive { recording_id: String },
    /// No active session when one was expected.
    NoActiveSession,
    /// The transition is not allowed from the current phase.
    InvalidTransition {
        from: SessionPhase,
        to: &'static str,
    },
    /// Recorder capture backend reported a failure.
    Capture(RecorderError),
    /// Journal writer failure.
    Journal(JournalError),
    /// Permission missing.
    PermissionMissing(&'static str),
    /// Required lane unhealthy.
    LaneUnavailable(&'static str),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleError::AlreadyActive { recording_id } => {
                write!(f, "recording already active: {recording_id}")
            }
            LifecycleError::NoActiveSession => f.write_str("no active session"),
            LifecycleError::InvalidTransition { from, to } => {
                write!(f, "invalid transition from {} to {to}", from.as_str())
            }
            LifecycleError::Capture(err) => write!(f, "capture: {err}"),
            LifecycleError::Journal(err) => write!(f, "journal: {err}"),
            LifecycleError::PermissionMissing(name) => write!(f, "permission missing: {name}"),
            LifecycleError::LaneUnavailable(name) => write!(f, "lane unavailable: {name}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

impl From<RecorderError> for LifecycleError {
    fn from(err: RecorderError) -> Self {
        LifecycleError::Capture(err)
    }
}

impl From<JournalError> for LifecycleError {
    fn from(err: JournalError) -> Self {
        LifecycleError::Journal(err)
    }
}

/// Monotonic clock source. In production the daemon's monotonic clock
/// feeds this; in tests we use a manual counter so transition ordering
/// is reproducible.
#[derive(Debug, Default)]
pub struct MonotonicClock {
    counter: AtomicU64,
}

impl MonotonicClock {
    /// New clock starting at 0.
    pub fn new() -> Self {
        Self::default()
    }
    /// Advance and return the new monotonic ms.
    pub fn tick(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
    /// Read the current value without advancing.
    pub fn now(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }
}

/// Recording Session — owns the Journal writer, the capture backend, and
/// the per-session lane health record.
pub struct Session {
    recording_id: String,
    profile: Profile,
    state: SessionPhase,
    started_at_unix_ms: u64,
    monotonic_origin_ns: u64,
    owner: ConnectionId,
    journal: Option<JournalWriter>,
    capture: Option<Box<dyn RecorderCapture>>,
    lanes: BTreeMap<String, LaneRecord>,
    event_count: u64,
    mark_count: u64,
    gap_count: u64,
    journal_path: PathBuf,
    clock: Arc<MonotonicClock>,
    failure_detail: Option<FailureDetail>,
    /// Best-effort reference to the capture shutdown signal so the manager
    /// can ask the capture backend to stop without owning its thread.
    shutdown: Arc<ShutdownSignal>,
}

impl Session {
    /// Open a new `Session` in the `Recording` phase. The Journal
    /// `session_start` entry is written before this function returns. The
    /// capture backend is started but the worker loop lives in the
    /// capture backend itself.
    pub fn start(
        recording_id: String,
        profile: Profile,
        owner: ConnectionId,
        journal_path: PathBuf,
        platform: PlatformInfo,
        started_at_unix_ms: u64,
        monotonic_origin_ns: u64,
        clock: Arc<MonotonicClock>,
    ) -> Result<Self, LifecycleError> {
        // Lane preflight — every required lane starts as Available; the
        // capture backend's start() runs the real TCC check. We assume
        // "available" here; a follow-up ticket wires the explicit
        // permission-prompt path (spec §`@record-start` step 3).
        let mut lanes = BTreeMap::new();
        for lane in profile.required_lanes() {
            lanes.insert(
                (*lane).to_string(),
                LaneRecord {
                    state: LaneState::Available,
                    generation: 0,
                },
            );
        }

        let anchor = WallClockAnchor {
            started_at_unix_ms,
            monotonic_origin_ns,
        };
        let lane_refs: Vec<(&str, &str, u64)> = lanes
            .iter()
            .map(|(name, rec)| (name.as_str(), lane_state_str(rec.state), rec.generation))
            .collect();
        let mut journal = JournalWriter::open(
            journal_path.clone(),
            recording_id.clone(),
            platform,
            anchor,
            profile.as_str(),
            "topology-default",
            "os-logical",
            &lane_refs,
        )?;

        let mut capture = platform_capture();
        capture.start()?;
        let shutdown = capture_shutdown_signal(&capture);
        let _ = journal_seq_after_open(&mut journal);

        Ok(Self {
            recording_id,
            profile,
            state: SessionPhase::Recording,
            started_at_unix_ms,
            monotonic_origin_ns,
            owner,
            journal: Some(journal),
            capture: Some(capture),
            lanes,
            event_count: 0,
            mark_count: 0,
            gap_count: 0,
            journal_path,
            clock,
            failure_detail: None,
            shutdown,
        })
    }

    /// `recording_id` accessor.
    pub fn recording_id(&self) -> &str {
        &self.recording_id
    }

    /// Current phase.
    pub fn phase(&self) -> SessionPhase {
        self.state
    }

    /// Owner connection that started this session.
    pub fn owner(&self) -> ConnectionId {
        self.owner
    }

    /// Wall-clock anchor.
    pub fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }

    /// Monotonic clock anchor (ns).
    pub fn monotonic_origin_ns(&self) -> u64 {
        self.monotonic_origin_ns
    }

    /// Capture event count.
    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Mark count.
    pub fn mark_count(&self) -> u64 {
        self.mark_count
    }

    /// Gap count.
    pub fn gap_count(&self) -> u64 {
        self.gap_count
    }

    /// Lane state snapshot for status / capability reports.
    pub fn lane(&self, name: &str) -> Option<LaneRecord> {
        self.lanes.get(name).cloned()
    }

    /// Path to the active Journal file. Useful for crash recovery.
    pub fn journal_path(&self) -> &std::path::Path {
        &self.journal_path
    }

    /// Open the shared capture shutdown signal so the manager can signal
    /// the capture worker to stop without owning its join handle.
    pub fn shutdown_signal(&self) -> Arc<ShutdownSignal> {
        Arc::clone(&self.shutdown)
    }

    /// Whether this session is currently in the given phase.
    pub fn is_phase(&self, phase: SessionPhase) -> bool {
        self.state == phase
    }

    /// Attempt to claim ownership of a session whose recorded owner
    /// connection has been lost. Returns `true` if the new connection
    /// became the owner. Per spec §Ownership and disconnects: a session
    /// may only be adopted by a new connection if the original has not
    /// responded within the configured grace window — that timer lives
    /// in the manager; this method is the actual transfer.
    pub fn try_adopt(&mut self, new_owner: ConnectionId) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        self.owner = new_owner;
        true
    }

    /// Mark event. Triggers an immediate fsync on the Journal.
    pub fn mark(&mut self, label: String, redaction_active: bool) -> Result<(), LifecycleError> {
        self.require_phase(SessionPhase::Recording, "mark")?;
        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        journal.write_mark(
            monotonic_ns,
            Mark {
                label,
                redaction_active,
            },
        )?;
        self.mark_count += 1;
        Ok(())
    }

    /// Record a physical capture event. Returns the assigned `capture_seq`.
    pub fn record_event(&mut self, event: &CaptureEvent) -> Result<u64, LifecycleError> {
        self.require_phase(SessionPhase::Recording, "record_event")?;
        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        let seq = journal.write_capture_event(monotonic_ns, event)?;
        self.event_count += 1;
        Ok(seq)
    }

    /// Lane state transition. Increments `generation` and emits a
    /// `lane_status` journal entry. If the lane is required by the
    /// profile and the new state is not `Available`, the session moves
    /// to `Failed` and the journal terminal entry is written.
    pub fn update_lane(
        &mut self,
        name: &str,
        new_state: LaneState,
        reason: String,
    ) -> Result<(), LifecycleError> {
        if self.state.is_terminal() {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: "update_lane",
            });
        }
        let rec = self.lanes.entry(name.to_string()).or_insert(LaneRecord {
            state: LaneState::Available,
            generation: 0,
        });
        rec.generation += 1;
        rec.state = new_state;

        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        journal.write_lane_status(
            monotonic_ns,
            LaneTransition {
                lane: name.to_string(),
                state: lane_state_str(new_state).to_string(),
                reason: reason.clone(),
                recoverable: new_state == LaneState::Available,
                generation: rec.generation,
            },
        )?;

        // Required lane failure → fail closed.
        if self.profile.required_lanes().contains(&name) && new_state != LaneState::Available {
            self.fail(FailureDetail {
                reason: FailureReason::RequiredLaneFailure,
                detail: format!("lane {name} -> {}", lane_state_str(new_state)),
            })?;
        }
        Ok(())
    }

    /// Gap declaration. Records a `gap` journal entry and updates the
    /// counter; does not change phase.
    pub fn record_gap(
        &mut self,
        first: Option<u64>,
        last: Option<u64>,
        dropped: Option<u64>,
        cause: String,
        recoverable: bool,
    ) -> Result<(), LifecycleError> {
        self.require_phase(SessionPhase::Recording, "record_gap")?;
        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        journal.write_gap(
            monotonic_ns,
            super::journal::GapDeclaration {
                first_capture_seq: first,
                last_capture_seq: last,
                dropped_count: dropped,
                cause,
                recoverable,
            },
        )?;
        self.gap_count += 1;
        Ok(())
    }

    /// Move the session to `Finalizing`. The capture backend is asked to
    /// stop. The Journal remains open for the `session_terminal` entry
    /// that the bundle commit (ticket `#19`) writes after the atomic
    /// rename. Returns the (still-open) journal path so the manager can
    /// hand it to the bundle writer.
    pub fn begin_finalize(&mut self) -> Result<PathBuf, LifecycleError> {
        self.require_phase(SessionPhase::Recording, "begin_finalize")?;
        if let Some(capture) = self.capture.as_mut() {
            capture.stop()?;
        }
        self.shutdown.trigger();
        self.state = SessionPhase::Finalizing;
        Ok(self.journal_path.clone())
    }

    /// Mark the session as `Completed` after the bundle commit succeeded.
    /// The journal's `session_terminal` entry is written here.
    pub fn complete(&mut self) -> Result<(), LifecycleError> {
        self.require_phase(SessionPhase::Finalizing, "complete")?;
        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        journal.write_session_terminal(monotonic_ns, SessionTerminalState::Completed)?;
        if let Some(mut j) = self.journal.take() {
            j.close()?;
        }
        self.state = SessionPhase::Completed;
        Ok(())
    }

    /// Mark the session as `Cancelled`. Used by `@record-cancel` from
    /// `Recording` and by `@record-stop` when the owner aborts before
    /// finalize. The journal `session_terminal` entry is written and the
    /// journal is closed. The bundle is never written for a cancelled
    /// session; the orphan journal is the audit trail.
    pub fn cancel(&mut self) -> Result<(), LifecycleError> {
        if self.state != SessionPhase::Recording {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: "cancel",
            });
        }
        if let Some(capture) = self.capture.as_mut() {
            capture.stop()?;
        }
        self.shutdown.trigger();
        let monotonic_ns = self.clock.tick();
        let journal = self
            .journal
            .as_mut()
            .ok_or(LifecycleError::NoActiveSession)?;
        journal.write_session_terminal(monotonic_ns, SessionTerminalState::Cancelled)?;
        if let Some(mut j) = self.journal.take() {
            j.close()?;
        }
        self.state = SessionPhase::Cancelled;
        Ok(())
    }

    /// Move the session to `Failed`. Writes the `session_terminal` entry
    /// and stops the capture backend. After this, the session is terminal
    /// and cannot be resumed.
    pub fn fail(&mut self, detail: FailureDetail) -> Result<(), LifecycleError> {
        // Allow fail() from any non-terminal state. Spec §Crash paths
        // distinguish: recording → failed (orphan journal retained),
        // finalizing → failed (staging cleared, orphan retained), and
        // completed → still completed (retried by completed retry, not
        // failed). The caller is expected to be in `Recording` or
        // `Finalizing` only.
        if self.state == SessionPhase::Completed {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: "fail",
            });
        }
        if let Some(capture) = self.capture.as_mut() {
            let _ = capture.stop();
        }
        self.shutdown.trigger();
        let monotonic_ns = self.clock.tick();
        if let Some(journal) = self.journal.as_mut() {
            let _ = journal.write_session_terminal(monotonic_ns, SessionTerminalState::Failed);
        }
        if let Some(mut j) = self.journal.take() {
            let _ = j.close();
        }
        self.failure_detail = Some(detail);
        self.state = SessionPhase::Failed;
        Ok(())
    }

    /// Read the failure detail recorded by `fail`. `None` if the session
    /// is not in `Failed`.
    pub fn failure_detail(&self) -> Option<&FailureDetail> {
        self.failure_detail.as_ref()
    }

    /// Drain the capture backend queue into the journal. Returns the
    /// number of events flushed. Callers iterate this in a loop until the
    /// queue is empty (typically once at `begin_finalize` boundary).
    pub fn drain_capture(&mut self) -> Result<usize, LifecycleError> {
        // Move events out of the capture queue first; the closure inside
        // `drain_all` would otherwise borrow `self` while the journal
        // path inside `record_event` mutably borrows it.
        let mut events: Vec<super::CaptureEvent> = Vec::new();
        if let Some(capture) = self.capture.as_mut() {
            capture.queue().drain_all(|event| events.push(event));
        }
        let mut count = 0;
        for event in &events {
            if self.record_event(event).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Build a `TerminalSummary` capturing the session's final state. The
    /// session is consumed because once terminal the live state is no
    /// longer needed.
    pub fn into_summary(self) -> TerminalSummary {
        let failure = self.failure_detail;
        TerminalSummary {
            recording_id: self.recording_id,
            phase: self.state,
            ended_at_unix_ms: journal::now_unix_ms(),
            event_count: self.event_count,
            mark_count: self.mark_count,
            gap_count: self.gap_count,
            failure,
            stop_trigger: None,
        }
    }

    fn require_phase(
        &self,
        expected: SessionPhase,
        op: &'static str,
    ) -> Result<(), LifecycleError> {
        if self.state != expected {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: op,
            });
        }
        Ok(())
    }
}

#[allow(unused)]
fn lane_state_str(state: LaneState) -> &'static str {
    match state {
        LaneState::Available => "available",
        LaneState::Unavailable => "unavailable",
        LaneState::Denied => "denied",
        LaneState::Disabled => "disabled",
    }
}

#[allow(unused)]
fn capture_shutdown_signal(capture: &Box<dyn RecorderCapture>) -> Arc<ShutdownSignal> {
    // The capture backend owns its own ShutdownSignal; the stub exposes
    // the same surface through a static helper. We construct a fresh
    // signal here so the manager can broadcast to the capture loop.
    let _ = capture; // silence unused on stub-only impls
    ShutdownSignal::shared()
}

#[allow(unused)]
fn journal_seq_after_open(_j: &mut JournalWriter) -> u64 {
    // Reserved for follow-up: return the seq cursor after session_start.
    1
}

/// Manager: single-active slot plus the volatile `last_session` summary.
pub struct LifecycleManager {
    current: Option<Session>,
    last_session: Option<TerminalSummary>,
    clock: Arc<MonotonicClock>,
}

impl LifecycleManager {
    /// New empty manager.
    pub fn new() -> Self {
        Self {
            current: None,
            last_session: None,
            clock: Arc::new(MonotonicClock::new()),
        }
    }

    /// Inject a custom clock (used by tests for deterministic ordering).
    pub fn with_clock(mut self, clock: Arc<MonotonicClock>) -> Self {
        self.clock = clock;
        self
    }

    /// Borrow the shared monotonic clock.
    pub fn clock(&self) -> Arc<MonotonicClock> {
        Arc::clone(&self.clock)
    }

    /// Try to start a session. Returns the new session on success.
    pub fn start(
        &mut self,
        recording_id: String,
        profile: Profile,
        owner: ConnectionId,
        journal_path: PathBuf,
        platform: PlatformInfo,
        started_at_unix_ms: u64,
    ) -> Result<&mut Session, LifecycleError> {
        if let Some(active) = self.current.as_ref() {
            return Err(LifecycleError::AlreadyActive {
                recording_id: active.recording_id.clone(),
            });
        }
        let session = Session::start(
            recording_id,
            profile,
            owner,
            journal_path,
            platform,
            started_at_unix_ms,
            self.clock.now(),
            Arc::clone(&self.clock),
        )?;
        self.current = Some(session);
        Ok(self.current.as_mut().expect("just inserted"))
    }

    /// Current session, if any.
    pub fn current(&self) -> Option<&Session> {
        self.current.as_ref()
    }

    /// Mutable current session, if any.
    pub fn current_mut(&mut self) -> Option<&mut Session> {
        self.current.as_mut()
    }

    /// Consume the current session by completing the lifecycle. The
    /// manager is left with `last_session` populated and `current` empty.
    pub fn complete_current(&mut self) -> Result<TerminalSummary, LifecycleError> {
        let mut session = self.current.take().ok_or(LifecycleError::NoActiveSession)?;
        session.complete()?;
        let summary = session.into_summary();
        self.last_session = Some(summary.clone());
        Ok(summary)
    }

    /// Cancel the current session.
    pub fn cancel_current(&mut self) -> Result<TerminalSummary, LifecycleError> {
        let mut session = self.current.take().ok_or(LifecycleError::NoActiveSession)?;
        session.cancel()?;
        let summary = session.into_summary();
        self.last_session = Some(summary.clone());
        Ok(summary)
    }

    /// Fail the current session with detail.
    pub fn fail_current(
        &mut self,
        detail: FailureDetail,
    ) -> Result<TerminalSummary, LifecycleError> {
        let mut session = self.current.take().ok_or(LifecycleError::NoActiveSession)?;
        session.fail(detail)?;
        let summary = session.into_summary();
        self.last_session = Some(summary.clone());
        Ok(summary)
    }

    /// Volatile summary of the most recent terminal session. Cleared
    /// when the next session starts.
    pub fn last_session(&self) -> Option<&TerminalSummary> {
        self.last_session.as_ref()
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Placeholder for follow-up: crash recovery detects a partial Journal on
/// startup and routes it to the lifecycle ticket. The current commit
/// focuses on the in-memory state machine; `Session::recover_from_journal`
/// lands with ticket `#18` follow-up that reads the journal header.
#[derive(Debug)]
pub struct CrashRecovery;

impl CrashRecovery {
    /// New crash recovery helper.
    pub fn new() -> Self {
        Self
    }

    /// Decide what to do with an orphan journal on startup. Returns
    /// `None` if the journal is empty / not present.
    pub fn classify(_path: &std::path::Path) -> Option<CrashClassification> {
        // ponytail: a real implementation reads the first line of the
        // journal file, looks for `session_start`, scans for
        // `session_terminal`, and decides recording/finalizing/completed.
        // The current commit defers this to a follow-up that pairs with
        // the bundle writer ticket so we do not duplicate the
        // classification logic.
        None
    }
}

impl Default for CrashRecovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Classification of an orphan journal found on disk at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashClassification {
    /// Journal has `session_start` but no `session_terminal` and no
    /// `bundle_committed` evidence — recorder crashed during recording
    /// or finalize. The journal is retained as orphan.
    Incomplete { recording_id: String },
    /// Journal has both `session_start` and `session_terminal` and a
    /// committed bundle reference. The completed session can be retried
    /// via `@record-stop` to re-deliver the same bundle.
    Completed { recording_id: String },
}

/// Re-export so the integration ticket can use `journal::JournalKind` from
/// `control_recording::session` without a second import path.
#[allow(unused)]
mod journal {
    pub use super::super::journal::now_unix_ms;
    pub use super::super::journal::*;
}

/// Public schema id re-export so the protocol layer can reference the
/// canonical string.
pub const RECORD_CONTROL_SCHEMA: &str = "rdog.record-control.v1";

// Ensure the constants are visible at the module level for tests /
// downstream consumers.
#[doc(hidden)]
pub const _JOURNAL_SCHEMA_REF: &str = JOURNAL_SCHEMA;
