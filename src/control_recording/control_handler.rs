//! line-control `@record-*` 命令到 LifecycleManager / DeliveryManager 的桥接层。
//!
//! 设计目标:
//! - 只暴露 4 个 line-control 入口 (`@record-start` / `@record-status`
//!   / `@record-stop` / `@record-cancel`),不重新实现 lifecycle。
//! - 所有响应走现有 `control_core::render_*_response` helper,
//!   避免再起一套 envelope 编码。
//! - `@record-mark` 在本 ticket 范围外(需要 session wrapper),
//!   暂时返回 not_implemented。

use std::{collections::HashMap, path::PathBuf};

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
        SessionPhase,
    },
};

pub const RECORD_CONTROL_SCHEMA: &str = "rdog.record-control.v1";

/// line-control 一侧的 recording 请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRequest {
    Start { profile: Profile },
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
}

impl RecordingHandler {
    pub fn new(journal_dir: PathBuf, bundle_dir: PathBuf) -> Self {
        Self {
            lifecycle: LifecycleManager::new(),
            delivery: DeliveryManager::default(),
            journal_dir,
            bundle_dir,
            completed: HashMap::new(),
        }
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
        match request {
            RecordRequest::Start { profile } => self.start(request_id, connection, profile),
            RecordRequest::Status => self.status(request_id),
            RecordRequest::Mark { label, redaction_active } => {
                self.mark(request_id, connection, label, redaction_active)
            }
            RecordRequest::Stop => self.stop(request_id, connection),
            RecordRequest::Cancel => self.cancel(request_id, connection),
        }
    }

    fn start(&mut self, request_id: Option<u64>, connection: ConnectionId, profile: Profile) -> ControlExecutionOutcome {
        if let Some(active) = self.lifecycle.current() {
            return protocol_error(request_id, 4101, json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-start",
                "error_code": "RECORDING_ALREADY_ACTIVE",
                "recording_id": active.recording_id(),
            }));
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
        if let Err(err) = self.lifecycle.start(recording_id, profile, connection, journal_path, platform, started_at) {
            return lifecycle_outcome(request_id, &err);
        }
        let body = json!({
            "schema": RECORD_CONTROL_SCHEMA,
            "kind": "record-start",
            "status": "recording",
            "recording_id": self.lifecycle.current().map(|s| s.recording_id()).unwrap_or(""),
            "profile": profile_name(profile),
            "started_at_unix_ms": self.lifecycle.current().map(|s| s.started_at_unix_ms()).unwrap_or(0),
        });
        success(request_id, &body)
    }

    fn status(&self, request_id: Option<u64>) -> ControlExecutionOutcome {
        let body = if let Some(session) = self.lifecycle.current() {
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
            })
        } else if let Some(summary) = self.lifecycle.last_session() {
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
            Ok(s) => s,
            Err(err) => return lifecycle_outcome(request_id, &err),
        };
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
        let recording_id = match self.lifecycle.current() {
            Some(s) if s.owner() == connection => s.recording_id().to_owned(),
            Some(_) => return protocol_error(request_id, 4102, json!({"error_code": "RECORD_NOT_OWNER"})),
            None => return protocol_error(request_id, 4103, json!({"error_code": "RECORD_NO_ACTIVE_SESSION"})),
        };
        match self.lifecycle.cancel_current() {
            Ok(_) => success(request_id, &json!({
                "schema": RECORD_CONTROL_SCHEMA,
                "kind": "record-cancel",
                "recording_id": recording_id,
                "phase": "cancelled",
            })),
            Err(err) => lifecycle_outcome(request_id, &err),
        }
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
