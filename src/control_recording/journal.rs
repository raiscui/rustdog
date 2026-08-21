//! Recording Journal writer for `rdog.recording.v1`.
//!
//! Per ticket `#17` and `specs/rdog-recording-journal-model.md`. Writes an
//! append-only JSONL stream. `journal_seq` is strictly monotonic across the
//! session (first entry is `session_start` at seq 0). `capture_seq` is
//! assigned to physical events only.
//!
//! ponytail: minimum-viable writer. No background thread, no schema
//! validator beyond the Rust type system, no fsync daemon. fsync happens
//! at session boundary, mark boundary, and every
//! [`FSYNC_INTERVAL_EVENTS`](Self::FSYNC_INTERVAL_EVENTS) entries.

#![cfg_attr(test, allow(dead_code))]

use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::control_recording::CaptureEvent;

/// `rdog.recording.v1` envelope schema identifier.
pub const JOURNAL_SCHEMA: &str = "rdog.recording.v1";

/// Event kinds emitted by the writer. Mirrors `specs/rdog-recording-journal-model.md` §Event families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalKind {
    SessionStart,
    Physical,
    SemanticCandidate,
    Context,
    LaneStatus,
    Redaction,
    Gap,
    Mark,
    SessionTerminal,
}

impl JournalKind {
    fn as_str(&self) -> &'static str {
        match self {
            JournalKind::SessionStart => "session_start",
            JournalKind::Physical => "physical",
            JournalKind::SemanticCandidate => "semantic_candidate",
            JournalKind::Context => "context",
            JournalKind::LaneStatus => "lane_status",
            JournalKind::Redaction => "redaction",
            JournalKind::Gap => "gap",
            JournalKind::Mark => "mark",
            JournalKind::SessionTerminal => "session_terminal",
        }
    }
}

/// Wall-clock anchor captured at session start (Unix epoch ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallClockAnchor {
    /// Unix epoch ms.
    pub started_at_unix_ms: u64,
    /// Monotonic ns at session start.
    pub monotonic_origin_ns: u64,
}

/// Lane transition report. Mirrors `lane_status` payload schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaneTransition {
    /// Lane name.
    pub lane: String,
    /// New state.
    pub state: String,
    /// Reason for the transition.
    pub reason: String,
    /// Whether the lane can recover automatically.
    pub recoverable: bool,
    /// Monotonic generation counter for the lane.
    pub generation: u64,
}

/// Gap declaration emitted when capture drops events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GapDeclaration {
    /// First capture_seq lost (or `None` if unknown).
    pub first_capture_seq: Option<u64>,
    /// Last capture_seq lost (or `None`).
    pub last_capture_seq: Option<u64>,
    /// Dropped count (or `None`).
    pub dropped_count: Option<u64>,
    /// Cause code.
    pub cause: String,
    /// Whether the backend can recover.
    pub recoverable: bool,
}

/// User / system annotation marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mark {
    /// Free-form label.
    pub label: String,
    /// Whether the mark opens a redaction interval.
    pub redaction_active: bool,
}

/// Session terminal event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTerminalState {
    Completed,
    Failed,
    Cancelled,
}

impl SessionTerminalState {
    fn as_str(&self) -> &'static str {
        match self {
            SessionTerminalState::Completed => "completed",
            SessionTerminalState::Failed => "failed",
            SessionTerminalState::Cancelled => "cancelled",
        }
    }
}

/// Platform descriptor for session_start payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlatformInfo {
    /// OS identifier.
    pub os: String,
    /// Backend identifier.
    pub capture_backend: String,
}

/// Errors raised by the journal writer.
#[derive(Debug)]
pub enum JournalError {
    /// I/O failure during write / fsync / open.
    Io(io::Error),
    /// The writer was used after `close` or before `open`.
    State(&'static str),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalError::Io(err) => write!(f, "journal io: {err}"),
            JournalError::State(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<io::Error> for JournalError {
    fn from(err: io::Error) -> Self {
        JournalError::Io(err)
    }
}

/// Recording Journal writer.
///
/// Owns a `BufWriter<File>` and assigns sequence numbers. Closes cleanly via
/// [`close`](Self::close). Crash leaves the partial file on disk (orphaned)
/// per ticket `#5` lifecycle semantics.
#[derive(Debug)]
pub struct JournalWriter {
    writer: Option<BufWriter<File>>,
    path: PathBuf,
    recording_id: String,
    journal_seq: u64,
    capture_seq: u64,
    monotonic_origin_ns: u64,
    closed: bool,
    events_since_fsync: u64,
}

impl JournalWriter {
    /// fsync cadence — every N entries.
    pub const FSYNC_INTERVAL_EVENTS: u64 = 100;

    /// Open a Journal file and write the mandatory `session_start` entry.
    ///
    /// `parent_dir` is created if missing. Existing files are NOT truncated —
    /// the caller must pick a unique `recording_id` filename.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        path: PathBuf,
        recording_id: String,
        platform: PlatformInfo,
        anchor: WallClockAnchor,
        profile: &str,
        display_topology_key: &str,
        coordinate_space: &str,
        lane_generations: &[(&str, &str, u64)], // (lane, state, generation)
    ) -> Result<Self, JournalError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let mut writer = BufWriter::new(file);

        let lanes_json: Value = {
            let mut map = serde_json::Map::new();
            for (lane, state, generation) in lane_generations {
                map.insert(
                    (*lane).to_string(),
                    json!({"state": *state, "generation": generation}),
                );
            }
            Value::Object(map)
        };
        let payload = json!({
            "type": "start",
            "profile": profile,
            "started_at_unix_ms": anchor.started_at_unix_ms,
            "monotonic_origin_ns": anchor.monotonic_origin_ns,
            "platform": {
                "os": platform.os,
                "capture_backend": platform.capture_backend,
            },
            "lanes": lanes_json,
            "display_topology": {
                "topology_key": display_topology_key,
                "coordinate_space": coordinate_space,
            },
        });
        let envelope = json!({
            "schema": JOURNAL_SCHEMA,
            "recording_id": recording_id,
            "journal_seq": 0,
            "kind": JournalKind::SessionStart.as_str(),
            "monotonic_ns": anchor.monotonic_origin_ns,
            "payload": payload,
        });
        let line = canonical_json_line(&envelope)?;
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
        file_fsync(writer.get_ref())?;

        Ok(Self {
            writer: Some(writer),
            path,
            recording_id,
            journal_seq: 1, // session_start consumed seq 0; next is 1.
            capture_seq: 0,
            monotonic_origin_ns: anchor.monotonic_origin_ns,
            closed: false,
            events_since_fsync: 0,
        })
    }

    /// Map a `CaptureEvent` from the capture backend to a `physical`
    /// journal entry. Returns the assigned `capture_seq`.
    pub fn write_capture_event(
        &mut self,
        monotonic_ns: u64,
        event: &CaptureEvent,
    ) -> Result<u64, JournalError> {
        let payload = physical_payload(event);
        let capture_seq = self.capture_seq;
        let envelope = self.envelope(
            JournalKind::Physical,
            monotonic_ns,
            Some(capture_seq),
            payload,
        )?;
        self.append_line(envelope)?;
        self.capture_seq += 1;
        Ok(capture_seq)
    }

    /// Emit a `lane_status` transition entry.
    pub fn write_lane_status(
        &mut self,
        monotonic_ns: u64,
        transition: LaneTransition,
    ) -> Result<(), JournalError> {
        let payload = json!({
            "type": "transition",
            "lane": transition.lane,
            "state": transition.state,
            "reason": transition.reason,
            "recoverable": transition.recoverable,
            "generation": transition.generation,
        });
        let envelope = self.envelope(JournalKind::LaneStatus, monotonic_ns, None, payload)?;
        self.append_line(envelope)
    }

    /// Emit a `gap` declaration.
    pub fn write_gap(
        &mut self,
        monotonic_ns: u64,
        gap: GapDeclaration,
    ) -> Result<(), JournalError> {
        let payload = json!({
            "type": "event_loss",
            "capture_seq_range": {
                "first": gap.first_capture_seq,
                "last": gap.last_capture_seq,
            },
            "dropped_count": gap.dropped_count,
            "cause": gap.cause,
            "recoverable": gap.recoverable,
        });
        let envelope = self.envelope(JournalKind::Gap, monotonic_ns, None, payload)?;
        self.append_line(envelope)
    }

    /// Emit a `mark` annotation.
    pub fn write_mark(&mut self, monotonic_ns: u64, mark: Mark) -> Result<(), JournalError> {
        let payload = json!({
            "type": mark.label,
            "redaction_active": mark.redaction_active,
        });
        let envelope = self.envelope(JournalKind::Mark, monotonic_ns, None, payload)?;
        self.append_line(envelope)?;
        self.fsync_now()?;
        Ok(())
    }

    /// Emit the mandatory `session_terminal` entry. Must be the final entry.
    pub fn write_session_terminal(
        &mut self,
        monotonic_ns: u64,
        terminal: SessionTerminalState,
    ) -> Result<(), JournalError> {
        let payload = json!({ "type": terminal.as_str() });
        let envelope = self.envelope(JournalKind::SessionTerminal, monotonic_ns, None, payload)?;
        self.append_line(envelope)
    }

    /// Close the journal: flush, fsync, drop the writer. Idempotent.
    pub fn close(&mut self) -> Result<(), JournalError> {
        if self.closed {
            return Ok(());
        }
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
            file_fsync(writer.get_ref())?;
        }
        self.closed = true;
        Ok(())
    }

    /// Number of entries written so far (excluding `session_start`).
    pub fn journal_seq(&self) -> u64 {
        self.journal_seq
    }

    /// Number of physical capture events written so far.
    pub fn capture_seq(&self) -> u64 {
        self.capture_seq
    }

    /// Path the writer is flushing to. Useful for crash-orphan cleanup.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether `close` has run.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    // -----------------------------------------------------------------------
    // private helpers
    // -----------------------------------------------------------------------

    fn envelope(
        &self,
        kind: JournalKind,
        monotonic_ns: u64,
        capture_seq: Option<u64>,
        payload: Value,
    ) -> Result<Value, JournalError> {
        if self.closed {
            return Err(JournalError::State("journal closed"));
        }
        let mut env = serde_json::Map::new();
        env.insert("schema".into(), Value::String(JOURNAL_SCHEMA.into()));
        env.insert(
            "recording_id".into(),
            Value::String(self.recording_id.clone()),
        );
        env.insert("journal_seq".into(), json!(self.journal_seq));
        env.insert("kind".into(), Value::String(kind.as_str().into()));
        env.insert("monotonic_ns".into(), json!(monotonic_ns));
        if let Some(seq) = capture_seq {
            env.insert("capture_seq".into(), json!(seq));
        }
        env.insert("payload".into(), payload);
        Ok(Value::Object(env))
    }

    fn append_line(&mut self, envelope: Value) -> Result<(), JournalError> {
        let writer = self
            .writer
            .as_mut()
            .ok_or(JournalError::State("journal not open"))?;
        let line = canonical_json_line(&envelope)?;
        writer.write_all(line.as_bytes())?;
        self.journal_seq += 1;
        self.events_since_fsync += 1;
        if self.events_since_fsync >= Self::FSYNC_INTERVAL_EVENTS {
            writer.flush()?;
            file_fsync(writer.get_ref())?;
            self.events_since_fsync = 0;
        }
        Ok(())
    }

    fn fsync_now(&mut self) -> Result<(), JournalError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.flush()?;
            file_fsync(writer.get_ref())?;
            self.events_since_fsync = 0;
        }
        Ok(())
    }
}

impl Drop for JournalWriter {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Serialize a JSON value to a single canonical JSONL line: trailing newline.
fn canonical_json_line(value: &Value) -> io::Result<String> {
    let mut buf = serde_json::to_vec(value).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("serialize journal line: {err}"),
        )
    })?;
    buf.push(b'\n');
    String::from_utf8(buf).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("non-utf8 journal line: {err}"),
        )
    })
}

/// fsync the underlying file. `sync_all` is correct on every platform
/// (slightly slower than `fdatasync`); the spec does not require finer
/// granularity.
fn file_fsync(file: &File) -> io::Result<()> {
    file.sync_all()
}

/// Convert a `CaptureEvent` into its physical JSONL payload.
fn physical_payload(event: &CaptureEvent) -> Value {
    match event {
        CaptureEvent::Key {
            monotonic_ms: _,
            keycode,
            down,
            text,
        } => json!({
            "type": if *down { "key_down" } else { "key_up" },
            "keycode": keycode,
            "text": text,
        }),
        CaptureEvent::MouseButton {
            monotonic_ms: _,
            x,
            y,
            button,
            down,
        } => json!({
            "type": if *down { "mouse_down" } else { "mouse_up" },
            "x": x,
            "y": y,
            "button": button,
        }),
        CaptureEvent::MouseMove {
            monotonic_ms: _,
            x,
            y,
        } => json!({
            "type": "mouse_move",
            "x": x,
            "y": y,
        }),
        CaptureEvent::Scroll {
            monotonic_ms: _,
            delta_x,
            delta_y,
        } => json!({
            "type": "scroll",
            "delta_x": delta_x,
            "delta_y": delta_y,
        }),
        CaptureEvent::AxSnapshot {
            monotonic_ms: _,
            focused,
        } => json!({
            "type": "ax_snapshot",
            "focused": focused,
        }),
        CaptureEvent::WorkspaceFocus {
            monotonic_ms: _,
            bundle_id,
        } => json!({
            "type": "workspace_focus",
            "bundle_id": bundle_id,
        }),
        CaptureEvent::WindowGeometry {
            monotonic_ms: _,
            bundle_id,
            title,
            x,
            y,
            width,
            height,
        } => json!({
            "type": "window_geometry",
            "bundle_id": bundle_id,
            "title": title,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }),
        CaptureEvent::LaneStatus {
            monotonic_ms: _,
            lane,
            status,
        } => json!({
            "type": "lane_status_inline",
            "lane": lane,
            "status": status,
        }),
    }
}

/// Capture epoch ms — daemon monotonic clock anchor helper for callers.
pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
