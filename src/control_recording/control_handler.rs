use super::{
    journal::{now_unix_ms, SealedRecording, StagingCleanupError},
    session::{ConnectionId, Profile, RecordingPolicy, Session},
    CaptureSpec, CaptureTarget, RecorderCapture,
};
use crate::{
    control_core::{render_protocol_error_response, render_structured_success_response},
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_line_read::MAX_CONTROL_MESSAGE_BYTES,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

pub const RECORD_CONTROL_SCHEMA: &str = "rdog.record-control.v2";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Checkpoint,
    ParameterBoundary,
}
#[derive(Clone, PartialEq, Eq)]
pub enum RecordRequest {
    Start {
        profile: Profile,
        duration_ms: Option<u64>,
        targets: Vec<CaptureTarget>,
    },
    Status {
        recording_id: Option<String>,
    },
    Mark {
        recording_id: String,
        mark: MarkKind,
    },
    Stop {
        recording_id: String,
    },
    Cancel {
        recording_id: String,
    },
}
impl RecordRequest {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Start { .. } => "record-start",
            Self::Status { .. } => "record-status",
            Self::Mark { .. } => "record-mark",
            Self::Stop { .. } => "record-stop",
            Self::Cancel { .. } => "record-cancel",
        }
    }
}
impl std::fmt::Debug for RecordRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status { recording_id, .. } => f
                .debug_struct("Status")
                .field("recording_id", recording_id)
                .finish_non_exhaustive(),
            _ => f.write_str(self.kind()),
        }
    }
}

type Factory = Box<dyn Fn() -> Box<dyn RecorderCapture> + Send>;
enum Command {
    Request(
        Option<u64>,
        ConnectionId,
        RecordRequest,
        mpsc::SyncSender<ControlExecutionOutcome>,
    ),
    AnonymousStatus(Option<u64>, mpsc::SyncSender<ControlExecutionOutcome>),
    OwnerClosed(ConnectionId),
}
#[derive(Clone)]
pub struct RecordingHandler {
    sender: mpsc::Sender<Command>,
}
impl RecordingHandler {
    pub fn new(journal_dir: PathBuf) -> Self {
        Self::with_factory(journal_dir, Box::new(super::platform_capture))
    }
    fn with_factory(dir: PathBuf, factory: Factory) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            Worker {
                dir,
                factory,
                active: None,
                terminal: None,
                pending_cleanup: None,
            }
            .run(receiver)
        });
        Self { sender }
    }
    #[cfg(test)]
    pub(super) fn with_test_factory(dir: PathBuf, factory: Factory) -> Self {
        Self::with_factory(dir, factory)
    }
    pub fn handle(
        &mut self,
        id: Option<u64>,
        owner: ConnectionId,
        request: RecordRequest,
    ) -> ControlExecutionOutcome {
        let (send, recv) = mpsc::sync_channel(1);
        if self
            .sender
            .send(Command::Request(id, owner, request, send))
            .is_err()
        {
            return error(id, "RECORD_UNAVAILABLE");
        }
        recv.recv()
            .unwrap_or_else(|_| error(id, "RECORD_UNAVAILABLE"))
    }
    pub(crate) fn anonymous_status(&self, id: Option<u64>) -> ControlExecutionOutcome {
        let (send, recv) = mpsc::sync_channel(1);
        if self.sender.send(Command::AnonymousStatus(id, send)).is_err() {
            return error(id, "RECORD_UNAVAILABLE");
        }
        recv.recv().unwrap_or_else(|_| error(id, "RECORD_UNAVAILABLE"))
    }
    #[cfg(test)]
    pub(super) fn close_owner_for_test(&self, owner: ConnectionId) {
        let _ = self.sender.send(Command::OwnerClosed(owner));
    }
}
struct Terminal {
    id: String,
    owner: ConnectionId,
    phase: &'static str,
    artifact: Option<SealedRecording>,
    attempted: Option<bool>,
    cleanup_residual: Option<PathBuf>,
}
struct Worker {
    dir: PathBuf,
    factory: Factory,
    active: Option<Session>,
    terminal: Option<Terminal>,
    pending_cleanup: Option<Box<dyn RecorderCapture>>,
}
impl Worker {
    fn run(mut self, receiver: mpsc::Receiver<Command>) {
        loop {
            self.tick();
            let command = if self.active.is_none() && self.pending_cleanup.is_none() {
                receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            } else {
                receiver.recv_timeout(Duration::from_millis(25))
            };
            match command {
                Ok(Command::Request(id, owner, request, reply)) => {
                    let _ = reply.send(self.handle(id, owner, request));
                }
                Ok(Command::AnonymousStatus(id, reply)) => {
                    let phase = if self.active.is_some() { "recording" } else { "idle" };
                    let _ = reply.send(success(id, json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-status","phase":phase})));
                }
                Ok(Command::OwnerClosed(owner)) => self.owner_closed(owner),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(session) = self.active.take() {
                        if let Err((_, capture)) = session.cancel() {
                            self.retain_cleanup(capture);
                        }
                    }
                    if self.pending_cleanup.is_none() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }
    fn reap_capture(&mut self) {
        if self
            .pending_cleanup
            .as_mut()
            .is_some_and(|capture| !capture.cleanup_pending())
        {
            self.pending_cleanup = None;
        }
    }
    fn tick(&mut self) {
        self.reap_capture();
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.drain().is_err() {
            self.fail_active();
            return;
        }
        if Instant::now() >= active.lease_deadline {
            self.fail_active();
            return;
        }
        if Instant::now() >= active.deadline || active.nearing_limit() {
            self.finish_active();
        }
    }
    fn retain_cleanup(&mut self, mut capture: Box<dyn RecorderCapture>) {
        if capture.cleanup_pending() {
            self.pending_cleanup = Some(capture);
        }
    }
    fn fail_active(&mut self) {
        if let Some(session) = self.active.take() {
            let id = session.id.clone();
            let owner = session.owner;
            let cleanup_residual = match session.cancel() {
                Ok(()) => None,
                Err((err, capture)) => {
                    let residual = cleanup_path(&err);
                    self.retain_cleanup(capture);
                    residual
                }
            };
            self.terminal = Some(Terminal {
                id,
                owner,
                phase: "failed",
                artifact: None,
                attempted: None,
                cleanup_residual,
            });
        }
    }
    fn finish_active(&mut self) {
        if let Some(session) = self.active.take() {
            let id = session.id.clone();
            let owner = session.owner;
            let result = session.seal();
            self.terminal = Some(match result {
                Ok(artifact) => Terminal {
                    id,
                    owner,
                    phase: "recorded",
                    cleanup_residual: artifact.cleanup_residual.clone(),
                    artifact: Some(artifact),
                    attempted: None,
                },
                Err((err, capture)) => {
                    let cleanup_residual = cleanup_path(&err);
                    self.retain_cleanup(capture);
                    Terminal {
                        id,
                        owner,
                        phase: "failed",
                        artifact: None,
                        attempted: None,
                        cleanup_residual,
                    }
                }
            });
        }
    }
    fn owner_closed(&mut self, owner: ConnectionId) {
        if self.active.as_ref().is_some_and(|s| s.owner == owner) {
            self.fail_active();
        }
        if self.terminal.as_ref().is_some_and(|t| t.owner == owner) {
            self.terminal = None;
        }
    }
    fn handle(
        &mut self,
        id: Option<u64>,
        owner: ConnectionId,
        request: RecordRequest,
    ) -> ControlExecutionOutcome {
        match request {
            RecordRequest::Start {
                profile,
                duration_ms,
                targets,
            } => {
                if owner.0 == 0 {
                    return error(id, "RECORD_NOT_OWNER");
                }
                if self.active.is_some() {
                    return error(id, "RECORD_BUSY");
                }
                self.reap_capture();
                if self.pending_cleanup.is_some() {
                    return error(id, "RECORD_CLEANUP_PENDING");
                }
                if self
                    .terminal
                    .as_ref()
                    .is_some_and(|t| t.artifact.is_some() && t.attempted.is_none())
                {
                    return error(id, "RESULT_PENDING");
                }
                let Some(policy) = RecordingPolicy::new(duration_ms) else {
                    return error(id, "RECORD_INVALID_DURATION");
                };
                if targets.is_empty() || targets.iter().any(|t| t.pid == 0 || t.hwnd == 0) {
                    return error(id, "RECORD_INVALID_TARGET");
                }
                let recording_id = new_id();
                let spec = CaptureSpec {
                    targets,
                    semantic: profile == Profile::Semantic,
                };
                match Session::start(
                    recording_id.clone(),
                    owner,
                    profile,
                    spec,
                    policy,
                    &self.dir,
                    (self.factory)(),
                ) {
                    Ok(session) => {
                        self.active = Some(session);
                        self.terminal = None;
                        success(
                            id,
                            json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-start","phase":"recording","recording_id":recording_id,"profile":profile,"coverage":"best_effort","replay_status":"not_compiled","text_mode":super::TEXT_MODE,"shortcut_mode":"unsupported_without_evidence","policy":policy}),
                        )
                    }
                    Err((err, capture)) => {
                        let residual = cleanup_path(&err);
                        self.retain_cleanup(capture);
                        cleanup_outcome(id, residual.as_ref(), "RECORD_UNAVAILABLE")
                    }
                }
            }
            RecordRequest::Status { recording_id } => {
                if let Some(session) = self.active.as_mut() {
                    if owner == session.owner && recording_id.as_deref() == Some(&session.id) {
                        session.renew();
                    }
                    if owner == session.owner {
                        return success(
                            id,
                            json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-status","phase":"recording","recording_id":session.id}),
                        );
                    }
                    return success(
                        id,
                        json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-status","phase":"recording"}),
                    );
                }
                if let Some(t) = self.terminal.as_ref() {
                    if owner == t.owner {
                        return success(
                            id,
                            json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-status","phase":t.phase,"recording_id":t.id,"cleanup_status":cleanup_status(self.pending_cleanup.is_some(), t.cleanup_residual.as_ref()),"cleanup_residual_path":t.cleanup_residual}),
                        );
                    }
                }
                success(
                    id,
                    json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-status","phase":"idle","cleanup_status":cleanup_status(self.pending_cleanup.is_some(), None)}),
                )
            }
            RecordRequest::Mark { recording_id, mark } => {
                if !self.authorized(owner, &recording_id) {
                    return error(id, "RECORD_NOT_OWNER");
                }
                let Some(session) = self.active.as_mut() else {
                    return error(id, "RECORD_NO_ACTIVE_SESSION");
                };
                let label = match mark {
                    MarkKind::Checkpoint => "checkpoint",
                    MarkKind::ParameterBoundary => "parameter_boundary",
                };
                if session.mark(label).is_err() {
                    self.fail_active();
                    return cleanup_outcome(
                        id,
                        self.terminal
                            .as_ref()
                            .and_then(|t| t.cleanup_residual.as_ref()),
                        "RECORD_IO_ERROR",
                    );
                }
                success(
                    id,
                    json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-mark","recording_id":recording_id,"mark":label}),
                )
            }
            RecordRequest::Cancel { recording_id } => {
                if !self.authorized(owner, &recording_id) {
                    return error(id, "RECORD_NOT_OWNER");
                }
                if let Some(session) = self.active.take() {
                    let result = session.cancel();
                    let cleanup_residual =
                        result.as_ref().err().and_then(|(err, _)| cleanup_path(err));
                    self.terminal = Some(Terminal {
                        id: recording_id.clone(),
                        owner,
                        phase: if result.is_ok() {
                            "cancelled"
                        } else {
                            "failed"
                        },
                        artifact: None,
                        attempted: None,
                        cleanup_residual,
                    });
                    if let Err((_, capture)) = result {
                        self.retain_cleanup(capture);
                        return cleanup_outcome(
                            id,
                            self.terminal
                                .as_ref()
                                .and_then(|t| t.cleanup_residual.as_ref()),
                            "RECORD_IO_ERROR",
                        );
                    }
                }
                success(
                    id,
                    json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-cancel","recording_id":recording_id,"phase":self.terminal.as_ref().map(|t| t.phase)}),
                )
            }
            RecordRequest::Stop { recording_id } => {
                if !self.authorized(owner, &recording_id) {
                    return error(id, "RECORD_NOT_OWNER");
                }
                if self.active.is_some() {
                    self.finish_active();
                }
                let Some(t) = self.terminal.as_mut() else {
                    return error(id, "RECORD_NO_ACTIVE_SESSION");
                };
                if t.phase != "recorded" {
                    return cleanup_outcome(id, t.cleanup_residual.as_ref(), "RECORD_FAILED");
                }
                let Some(artifact) = t.artifact.as_ref() else {
                    return error(id, "RECORD_FAILED");
                };
                let metadata = json!({"schema":RECORD_CONTROL_SCHEMA,"kind":"record-stop","phase":"recorded","recording_id":t.id,"artifact_filename":artifact.filename,"artifact_size_bytes":artifact.size_bytes,"artifact_sha256":artifact.sha256,"replay_status":"not_compiled","delivery_status":"sent_unconfirmed","cleanup_status":cleanup_status(self.pending_cleanup.is_some(), t.cleanup_residual.as_ref()),"cleanup_residual_path":t.cleanup_residual});
                if let Some(sent) = t.attempted {
                    if !sent {
                        return error(id, "RECORD_DELIVERY_FAILED");
                    }
                    let mut summary = metadata;
                    summary["kind"] = json!("record-stop-summary");
                    return success(id, summary);
                }
                t.attempted = Some(false);
                let Ok(bytes) = std::fs::read(&artifact.path) else {
                    return error(id, "RECORD_IO_ERROR");
                };
                if bytes.len() as u64 != artifact.size_bytes
                    || format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256
                {
                    return error(id, "RECORD_ARTIFACT_MISMATCH");
                }
                let frame = SaveFileFrame {
                    request_id: id,
                    filename: artifact.filename.clone(),
                    mime: "application/x-ndjson".into(),
                    encoding: "base64".into(),
                    data: BASE64.encode(bytes),
                    quality: None,
                    width: None,
                    height: None,
                };
                if frame.to_wire_message().len() > MAX_CONTROL_MESSAGE_BYTES {
                    return error(id, "RECORD_ARTIFACT_TOO_LARGE");
                }
                t.attempted = Some(true);
                let mut outcome = success(id, metadata);
                outcome
                    .outbound_frames
                    .insert(0, ControlFrame::SaveFile(frame));
                outcome
            }
        }
    }
    fn authorized(&self, owner: ConnectionId, id: &str) -> bool {
        owner.0 != 0
            && (self
                .active
                .as_ref()
                .is_some_and(|s| s.owner == owner && s.id == id)
                || self
                    .terminal
                    .as_ref()
                    .is_some_and(|t| t.owner == owner && t.id == id))
    }
}
fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "rec-{:x}-{:x}",
        now_unix_ms(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn success(id: Option<u64>, value: Value) -> ControlExecutionOutcome {
    ControlExecutionOutcome::from_response_line(render_structured_success_response(
        id,
        &value.to_string(),
    ))
}
fn error(id: Option<u64>, code: &str) -> ControlExecutionOutcome {
    ControlExecutionOutcome::from_response_line(render_protocol_error_response(
        id,
        4106,
        &json!({"schema":RECORD_CONTROL_SCHEMA,"error_code":code}).to_string(),
    ))
}
fn cleanup_path(err: &std::io::Error) -> Option<PathBuf> {
    err.get_ref()?
        .downcast_ref::<StagingCleanupError>()
        .map(|failure| failure.path.clone())
}
fn cleanup_outcome(
    id: Option<u64>,
    path: Option<&PathBuf>,
    fallback: &str,
) -> ControlExecutionOutcome {
    match path {
        Some(path) => ControlExecutionOutcome::from_response_line(render_protocol_error_response(id, 4106, &json!({"schema":RECORD_CONTROL_SCHEMA,"error_code":"RECORD_STAGING_CLEANUP_FAILED","cleanup_residual_path":path}).to_string())),
        None => error(id, fallback),
    }
}
static RECORDING_HANDLER: OnceLock<RecordingHandler> = OnceLock::new();
pub fn install_recording_handler(handler: RecordingHandler) -> Result<(), RecordingHandler> {
    RECORDING_HANDLER.set(handler)
}
pub fn with_recording_handler<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut RecordingHandler) -> R,
{
    let mut handler = RECORDING_HANDLER.get()?.clone();
    Some(f(&mut handler))
}
pub fn recording_owner_closed(owner: ConnectionId) {
    if let Some(handler) = RECORDING_HANDLER.get() {
        let _ = handler.sender.send(Command::OwnerClosed(owner));
    }
}

fn cleanup_status(capture_pending: bool, residual: Option<&PathBuf>) -> &'static str {
    match (capture_pending, residual.is_some()) {
        (true, _) => "pending",
        (false, true) => "failed",
        (false, false) => "complete",
    }
}
