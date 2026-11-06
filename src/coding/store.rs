use super::model::{
    CodingError, CodingRequest, EngineIdentity, ErrorCode, InteractionAnswer, InteractionView,
    Result, SessionMetadata,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions, TryLockError},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

fn error(code: ErrorCode, message: impl Into<String>) -> CodingError {
    CodingError::new(code, message)
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
#[derive(Debug)]
pub struct Disposition {
    pub reply: Value,
    pub effect: Option<EngineAction>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct AbortWork {
    pub task_id: String,
    pub dialogs: Vec<String>,
}
#[derive(Debug)]
pub enum EngineAction {
    Prompt {
        task_id: String,
        prompt: String,
    },
    Respond {
        interaction_id: String,
        native_id: String,
        answer: InteractionAnswer,
    },
    Close,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Decision {
    digest: String,
    reply: Value,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Cancellation {
    None,
    Requested,
    Dispatched,
    Acknowledged,
    Superseded,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    task_id: String,
    status: String,
    native_id: Option<String>,
    agent_invoked: Option<bool>,
    result_status: Option<String>,
    result_error: Option<String>,
    settled: bool,
    cancellation: Cancellation,
    prompt_uncertainty: Option<String>,
    abort_uncertainty: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Interaction {
    view: InteractionView,
    native_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Decision {
        request_id: String,
        digest: String,
        reply: Value,
    },
    TaskStarted {
        task_id: String,
    },
    CancelRequested {
        task_id: String,
    },
    AbortDispatched {
        task_id: String,
        interaction_ids: Vec<String>,
    },
    CloseRequested,
    EngineReady {
        identity: EngineIdentity,
    },
    Ack {
        task_id: String,
        native_id: String,
        agent_invoked: Option<bool>,
    },
    AbortAcknowledged {
        task_id: String,
    },
    Result {
        task_id: String,
        native_id: String,
        status: String,
        agent_invoked: bool,
        settled: bool,
        error: Option<String>,
    },
    Output {
        task_id: String,
        stream: String,
        text: String,
    },
    InteractionOpened {
        interaction_id: String,
        task_id: String,
        native_id: String,
        kind: String,
        title: String,
        message: Option<String>,
        options: Vec<String>,
    },
    InteractionDecision {
        interaction_id: String,
    },
    InteractionSent {
        interaction_id: String,
    },
    LateCancelDispatched {
        interaction_id: String,
    },
    LateCancelSent {
        interaction_id: String,
    },
    InteractionCancelled {
        native_id: String,
    },
    EngineState {
        settled: bool,
        pending: bool,
    },
    Unknown {
        task_id: String,
        native_id: Option<String>,
        message: String,
    },
    Closed {
        reason: String,
    },
    Failed {
        message: String,
    },
}
#[derive(Debug, Serialize, Deserialize)]
struct Record {
    seq: u64,
    events: Vec<Event>,
}
#[derive(Debug)]
pub struct SessionState {
    meta: SessionMetadata,
    tasks: BTreeMap<String, Task>,
    interactions: BTreeMap<String, Interaction>,
    decisions: BTreeMap<String, Decision>,
    active: Option<String>,
    closed: bool,
    closing: bool,
    engine: Option<EngineIdentity>,
    failure: Option<String>,
    seq: u64,
}
impl SessionState {
    fn new(meta: SessionMetadata) -> Self {
        Self {
            meta,
            tasks: BTreeMap::new(),
            interactions: BTreeMap::new(),
            decisions: BTreeMap::new(),
            active: None,
            closed: false,
            closing: false,
            engine: None,
            failure: None,
            seq: 0,
        }
    }
    pub fn active_task_id(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn task_id_for_native(&self, native_id: &str) -> Option<&str> {
        self.tasks
            .values()
            .find(|t| t.native_id.as_deref() == Some(native_id))
            .map(|t| t.task_id.as_str())
    }
    pub fn session_json(&self) -> Value {
        json!({"session_id":self.meta.session_id,"host_instance_id":self.meta.host_instance_id,"status":if self.closed {"closed"} else if self.failure.is_some() {"failed"} else if self.closing {"closing"} else if self.engine.is_none() {"starting"} else {"ready"},"active_task_id":self.active,"engine":self.engine,"error":self.failure,"created_at_ms":self.meta.created_at_ms,"cwd":self.meta.cwd,"profile":self.meta.profile})
    }
    pub fn task_json(&self, task_id: &str) -> Result<Value> {
        let t = self
            .tasks
            .get(task_id)
            .ok_or_else(|| error(ErrorCode::NotFound, "task not found"))?;
        Ok(
            json!({"task_id":t.task_id,"status":t.status,"native_id":t.native_id,"agent_invoked":t.agent_invoked,"result_status":t.result_status,"error":t.result_error,"settled":t.settled,"uncertainty":t.abort_uncertainty.as_ref().or(t.prompt_uncertainty.as_ref())}),
        )
    }
    pub fn interactions_json(&self) -> Value {
        json!({"interactions":self.interactions.values().map(|i|&i.view).collect::<Vec<_>>()})
    }
    fn apply(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Decision {
                request_id,
                digest,
                reply,
            } => {
                self.decisions
                    .insert(request_id, Decision { digest, reply });
            }
            Event::TaskStarted { task_id } => {
                self.tasks.insert(
                    task_id.clone(),
                    Task {
                        task_id: task_id.clone(),
                        status: "accepted".into(),
                        native_id: None,
                        agent_invoked: None,
                        result_status: None,
                        result_error: None,
                        settled: false,
                        cancellation: Cancellation::None,
                        prompt_uncertainty: None,
                        abort_uncertainty: None,
                    },
                );
                self.active = Some(task_id);
            }
            Event::CancelRequested { task_id } => {
                let t = self.task_mut(&task_id)?;
                t.cancellation = Cancellation::Requested;
                t.status = "cancelling".into();
            }
            Event::AbortDispatched {
                task_id,
                interaction_ids,
            } => {
                self.task_mut(&task_id)?.cancellation = Cancellation::Dispatched;
                for interaction_id in interaction_ids {
                    self.interaction_mut(&interaction_id)?.view.state = "cancelling".into();
                }
            }
            Event::CloseRequested => self.closing = true,
            Event::AbortAcknowledged { task_id } => {
                let t = self.task_mut(&task_id)?;
                t.cancellation = Cancellation::Acknowledged;
                t.abort_uncertainty = None;
                t.settled = false;
                for interaction in self
                    .interactions
                    .values_mut()
                    .filter(|i| i.view.task_id == task_id && i.view.state == "cancelling")
                {
                    interaction.view.state = "cancelled".into();
                }
            }
            Event::EngineReady { identity } => self.engine = Some(identity),
            Event::Ack {
                task_id,
                native_id,
                agent_invoked,
            } => {
                let t = self.task_mut(&task_id)?;
                if let Some(existing) = &t.native_id {
                    if existing != &native_id {
                        return Err(error(ErrorCode::JournalCorrupt, "native id mismatch"));
                    }
                }
                t.native_id = Some(native_id);
                if agent_invoked.is_some() && t.agent_invoked.is_none() {
                    t.agent_invoked = agent_invoked;
                }
                if t.status == "accepted" {
                    t.status = "running".into();
                }
            }
            Event::Result {
                task_id,
                native_id,
                status,
                agent_invoked,
                settled,
                error: result_error,
            } => {
                let t = self.task_mut(&task_id)?;
                if let Some(existing) = &t.native_id {
                    if existing != &native_id {
                        return Err(error(ErrorCode::JournalCorrupt, "native id mismatch"));
                    }
                }
                t.native_id = Some(native_id);
                t.agent_invoked = Some(agent_invoked);
                t.result_status = Some(status.clone());
                t.status = status;
                t.result_error = result_error;
                t.settled = settled;
                if t.cancellation == Cancellation::Requested {
                    t.cancellation = Cancellation::Superseded;
                }
                t.prompt_uncertainty = None;
            }
            Event::Output { .. } => {}
            Event::InteractionOpened {
                interaction_id,
                task_id,
                native_id,
                kind,
                title,
                message,
                options,
            } => {
                self.interactions.insert(
                    interaction_id.clone(),
                    Interaction {
                        view: InteractionView {
                            interaction_id,
                            task_id,
                            kind,
                            title,
                            message,
                            options,
                            state: "pending".into(),
                        },
                        native_id,
                    },
                );
            }
            Event::InteractionDecision { interaction_id } => {
                self.interaction_mut(&interaction_id)?.view.state = "decided".into();
            }
            Event::InteractionSent { interaction_id } => {
                let i = self.interaction_mut(&interaction_id)?;
                if i.view.state == "decided" {
                    i.view.state = "sent".into();
                }
            }
            Event::LateCancelDispatched { interaction_id } => {
                self.interaction_mut(&interaction_id)?.view.state = "cancelling_late".into();
            }
            Event::LateCancelSent { interaction_id } => {
                let interaction = self.interaction_mut(&interaction_id)?;
                if interaction.view.state == "cancelling_late" {
                    interaction.view.state = "cancel_waiting_fence".into();
                }
            }
            Event::InteractionCancelled { native_id } => {
                for i in self.interactions.values_mut().filter(|i| {
                    i.native_id == native_id
                        && matches!(
                            i.view.state.as_str(),
                            "pending"
                                | "decided"
                                | "cancelling"
                                | "cancelling_late"
                                | "cancel_waiting_fence"
                        )
                }) {
                    i.view.state = "cancelled".into();
                }
            }
            Event::EngineState { settled, pending } => {
                for interaction in self
                    .interactions
                    .values_mut()
                    .filter(|i| i.view.state == "cancel_waiting_fence")
                {
                    interaction.view.state = "cancelled".into();
                }
                if let Some(task_id) = self.active.clone() {
                    let unresolved_interaction = self.interactions.values().any(|i| {
                        i.view.task_id == task_id
                            && matches!(
                                i.view.state.as_str(),
                                "pending"
                                    | "decided"
                                    | "cancelling"
                                    | "cancelling_late"
                                    | "cancel_waiting_fence"
                            )
                    });
                    let t = self.task_mut(&task_id)?;
                    t.settled = settled && !pending;
                    if t.settled
                        && !unresolved_interaction
                        && !matches!(
                            t.cancellation,
                            Cancellation::Requested | Cancellation::Dispatched
                        )
                        && (t.result_status.is_some()
                            || t.cancellation == Cancellation::Acknowledged)
                    {
                        if t.result_status.is_none() {
                            t.status = "cancelled".into();
                        }
                        self.active = None;
                    }
                }
            }
            Event::Unknown {
                task_id,
                native_id,
                message,
            } => {
                let t = self.task_mut(&task_id)?;
                if let Some(native_id) = native_id {
                    if t.native_id
                        .as_ref()
                        .is_some_and(|existing| existing != &native_id)
                    {
                        return Err(error(ErrorCode::JournalCorrupt, "native id mismatch"));
                    }
                    t.native_id = Some(native_id);
                    t.prompt_uncertainty = Some(message);
                } else {
                    t.abort_uncertainty = Some(message);
                }
                if t.result_status.is_none() {
                    t.status = "unknown".into();
                }
            }
            Event::Closed { reason: _ } => {
                self.closed = true;
                self.closing = false;
                self.active = None;
            }
            Event::Failed { message } => self.failure = Some(message),
        }
        Ok(())
    }
    fn task_mut(&mut self, id: &str) -> Result<&mut Task> {
        self.tasks
            .get_mut(id)
            .ok_or_else(|| error(ErrorCode::JournalCorrupt, "missing task"))
    }
    fn interaction_mut(&mut self, id: &str) -> Result<&mut Interaction> {
        self.interactions
            .get_mut(id)
            .ok_or_else(|| error(ErrorCode::JournalCorrupt, "missing interaction"))
    }
}
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    journal: File,
    _lease: File,
    max_bytes: u64,
    state: SessionState,
    poisoned: bool,
}
impl Store {
    #[cfg(test)]
    pub fn create(root: &Path, meta: SessionMetadata, max_bytes: u64) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("manifest.json"))?;
        serde_json::to_writer(&mut f, &meta)?;
        f.sync_all()?;
        Self::open(root, max_bytes)
    }
    pub fn metadata(root: &Path) -> Result<SessionMetadata> {
        Ok(serde_json::from_reader(File::open(
            root.join("manifest.json"),
        )?)?)
    }
    pub fn open(root: &Path, max_bytes: u64) -> Result<Self> {
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("writer.lock"))?;
        match lease.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(error(ErrorCode::Busy, "session writer already active"))
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        };
        let meta = Self::metadata(root)?;
        let journal = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(root.join("journal.jsonl"))?;
        let state = Self::replay(&journal, meta, max_bytes)?;
        Ok(Self {
            root: root.to_owned(),
            journal,
            _lease: lease,
            max_bytes,
            state,
            poisoned: false,
        })
    }
    pub fn read(root: &Path, max_bytes: u64) -> Result<SessionState> {
        let meta = Self::metadata(root)?;
        match File::open(root.join("journal.jsonl")) {
            Ok(f) => Self::replay(&f, meta, max_bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SessionState::new(meta)),
            Err(e) => Err(e.into()),
        }
    }
    fn replay(file: &File, meta: SessionMetadata, max: u64) -> Result<SessionState> {
        if file.metadata()?.len() > max {
            return Err(error(ErrorCode::QuotaExceeded, "journal exceeds quota"));
        }
        let mut state = SessionState::new(meta);
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                return Err(error(
                    ErrorCode::JournalIncomplete,
                    "incomplete journal record",
                ));
            }
            let rec: Record = serde_json::from_slice(&line)
                .map_err(|e| error(ErrorCode::JournalCorrupt, e.to_string()))?;
            if rec.seq != state.seq + 1 || rec.events.is_empty() {
                return Err(error(ErrorCode::JournalCorrupt, "invalid journal sequence"));
            }
            for e in rec.events {
                state.apply(e)?;
            }
            state.seq = rec.seq;
        }
        Ok(state)
    }
    pub fn state(&self) -> &SessionState {
        &self.state
    }
    fn append(&mut self, events: Vec<Event>) -> Result<()> {
        if self.poisoned {
            return Err(error(
                ErrorCode::Storage,
                "journal writer has an uncertain outcome",
            ));
        }
        let rec = Record {
            seq: self.state.seq + 1,
            events,
        };
        let mut bytes = serde_json::to_vec(&rec)?;
        bytes.push(b'\n');
        if self
            .journal
            .metadata()?
            .len()
            .saturating_add(bytes.len() as u64)
            > self.max_bytes
        {
            return Err(error(ErrorCode::QuotaExceeded, "journal quota exceeded"));
        }
        if let Err(e) = self
            .journal
            .write_all(&bytes)
            .and_then(|_| self.journal.sync_all())
        {
            self.poisoned = true;
            return Err(error(ErrorCode::Storage, e.to_string()));
        }
        for e in rec.events {
            if let Err(e) = self.state.apply(e) {
                self.poisoned = true;
                return Err(e);
            }
        }
        self.state.seq = rec.seq;
        Ok(())
    }
    pub fn request(&mut self, request: CodingRequest) -> Result<Disposition> {
        request.validate()?;
        if request.session_id() != Some(self.state.meta.session_id.as_str()) {
            return Err(error(ErrorCode::NotFound, "session not found"));
        }
        match &request {
            CodingRequest::SessionStatus { .. } => {
                return Ok(Disposition {
                    reply: self.state.session_json(),
                    effect: None,
                })
            }
            CodingRequest::TaskStatus { task_id, .. } => {
                return Ok(Disposition {
                    reply: self.state.task_json(task_id)?,
                    effect: None,
                })
            }
            CodingRequest::TaskOutput {
                task_id,
                cursor,
                limit,
                ..
            } => {
                return Ok(Disposition {
                    reply: self.output_page(task_id, cursor.as_deref(), *limit)?,
                    effect: None,
                })
            }
            CodingRequest::InteractionList { .. } => {
                return Ok(Disposition {
                    reply: self.state.interactions_json(),
                    effect: None,
                })
            }
            _ => {}
        }
        let (request_id, hash) = match &request {
            CodingRequest::TaskSubmit { request_id, .. }
            | CodingRequest::TaskCancel { request_id, .. }
            | CodingRequest::SessionClose { request_id, .. }
            | CodingRequest::InteractionRespond { request_id, .. } => {
                (request_id.clone(), digest(&request)?)
            }
            _ => {
                return Err(error(
                    ErrorCode::InvalidRequest,
                    "request is not session scoped",
                ))
            }
        };
        if let Some(prior) = self.state.decisions.get(&request_id) {
            if prior.digest != hash {
                return Err(error(
                    ErrorCode::Conflict,
                    "request id reused with different payload",
                ));
            }
            return Ok(Disposition {
                reply: prior.reply.clone(),
                effect: None,
            });
        }
        if self.state.closed || self.state.closing {
            return Err(error(ErrorCode::SessionClosed, "session is closed"));
        }
        let (reply, effect, mut events) = match request {
            CodingRequest::TaskSubmit { prompt, .. } => {
                if self.state.active.is_some() {
                    return Err(error(ErrorCode::Busy, "task already active"));
                }
                let task_id = id();
                (
                    json!({"task_id":task_id,"status":"accepted"}),
                    Some(EngineAction::Prompt {
                        task_id: task_id.clone(),
                        prompt,
                    }),
                    vec![Event::TaskStarted { task_id }],
                )
            }
            CodingRequest::TaskCancel { task_id, .. } => {
                let task = self
                    .state
                    .tasks
                    .get(&task_id)
                    .ok_or_else(|| error(ErrorCode::NotFound, "task not found"))?;
                let requested = self.state.active.as_deref() == Some(task_id.as_str())
                    && task.result_status.is_none()
                    && task.cancellation == Cancellation::None;
                let reply = json!({"task_id":task_id,"status":if requested {"cancelling"} else {&task.status}});
                let events = if requested {
                    vec![Event::CancelRequested { task_id }]
                } else {
                    vec![]
                };
                (reply, None, events)
            }
            CodingRequest::SessionClose { .. } => (
                json!({"status":"closing"}),
                Some(EngineAction::Close),
                vec![Event::CloseRequested],
            ),
            CodingRequest::InteractionRespond {
                interaction_id,
                answer,
                ..
            } => {
                let interaction = self
                    .state
                    .interactions
                    .get(&interaction_id)
                    .ok_or_else(|| error(ErrorCode::NotFound, "interaction not found"))?;
                if interaction.view.state != "pending" {
                    return Err(error(
                        ErrorCode::InteractionClosed,
                        "interaction already decided",
                    ));
                }
                let matches = matches!(
                    (&*interaction.view.kind, &answer),
                    ("confirm", InteractionAnswer::Confirm { .. })
                        | ("select", InteractionAnswer::Select { .. })
                        | ("input", InteractionAnswer::Input { .. })
                        | ("editor", InteractionAnswer::Editor { .. })
                );
                if !matches {
                    return Err(error(ErrorCode::InvalidRequest, "answer kind mismatch"));
                }
                if let InteractionAnswer::Select { value: Some(value) } = &answer {
                    if !interaction.view.options.contains(value) {
                        return Err(error(ErrorCode::InvalidRequest, "choice is not an option"));
                    }
                }
                let native_id = interaction.native_id.clone();
                (
                    json!({"interaction_id":interaction_id,"status":"decided"}),
                    Some(EngineAction::Respond {
                        interaction_id: interaction_id.clone(),
                        native_id,
                        answer,
                    }),
                    vec![Event::InteractionDecision { interaction_id }],
                )
            }
            _ => return Err(error(ErrorCode::InvalidRequest, "unsupported request")),
        };
        events.insert(
            0,
            Event::Decision {
                request_id,
                digest: hash,
                reply: reply.clone(),
            },
        );
        self.append(events)?;
        Ok(Disposition { reply, effect })
    }
    pub fn query_offline(root: &Path, request: CodingRequest, max_bytes: u64) -> Result<Value> {
        request.validate()?;
        let state = match Self::read(root, max_bytes) {
            Ok(state) => state,
            Err(mut journal_error)
                if matches!(
                    journal_error.code,
                    ErrorCode::JournalIncomplete | ErrorCode::JournalCorrupt
                ) && matches!(
                    &request,
                    CodingRequest::SessionStatus { .. } | CodingRequest::TaskStatus { .. }
                ) =>
            {
                let meta = Self::metadata(root)?;
                if request.session_id() != Some(meta.session_id.as_str()) {
                    return Err(error(ErrorCode::NotFound, "session not found"));
                }
                match super::failure::FailureSlot::read(root, &meta.host_instance_id) {
                    Ok(Some(host_error)) => {
                        journal_error.message = format!(
                            "{}; host {:?}: {}",
                            journal_error.message, host_error.code, host_error.message
                        );
                    }
                    Ok(None) => {}
                    Err(evidence_error) => {
                        journal_error.message = format!(
                            "{}; host failure evidence unavailable: {}",
                            journal_error.message, evidence_error.message
                        );
                    }
                }
                return Err(journal_error);
            }
            Err(journal_error) => return Err(journal_error),
        };
        if request.session_id() != Some(state.meta.session_id.as_str()) {
            return Err(error(ErrorCode::NotFound, "session not found"));
        }
        let with_host_failure = |mut result: Value| -> Result<Value> {
            if let Some(failure) =
                super::failure::FailureSlot::read(root, &state.meta.host_instance_id)?
            {
                result["host_error"] = serde_json::to_value(failure)?;
            }
            Ok(result)
        };
        match &request {
            CodingRequest::SessionStatus { .. } => with_host_failure(state.session_json()),
            CodingRequest::TaskStatus { task_id, .. } => with_host_failure(state.task_json(task_id)?),
            CodingRequest::InteractionList { .. } => Ok(state.interactions_json()),
            CodingRequest::TaskOutput {
                task_id,
                cursor,
                limit,
                ..
            } => Self::page(root, &state, task_id, cursor.as_deref(), *limit, max_bytes),
            CodingRequest::TaskSubmit { request_id, .. }
            | CodingRequest::TaskCancel { request_id, .. }
            | CodingRequest::SessionClose { request_id, .. }
            | CodingRequest::InteractionRespond { request_id, .. } => {
                if let Some(prior) = state.decisions.get(request_id) {
                    if prior.digest != digest(&request)? {
                        return Err(error(
                            ErrorCode::Conflict,
                            "request id reused with different payload",
                        ));
                    }
                    Ok(prior.reply.clone())
                } else if state.closed {
                    Err(error(ErrorCode::SessionClosed, "session closed"))
                } else {
                    Err(error(
                        ErrorCode::HostUnavailable,
                        "session host unavailable",
                    ))
                }
            }
            _ => Err(error(ErrorCode::InvalidRequest, "unsupported request")),
        }
    }
    fn output_page(
        &self,
        task_id: &str,
        cursor: Option<&str>,
        limit: Option<u16>,
    ) -> Result<Value> {
        Self::page(
            &self.root,
            &self.state,
            task_id,
            cursor,
            limit,
            self.max_bytes,
        )
    }
    fn page(
        root: &Path,
        state: &SessionState,
        task_id: &str,
        cursor: Option<&str>,
        limit: Option<u16>,
        max: u64,
    ) -> Result<Value> {
        if !state.tasks.contains_key(task_id) {
            return Err(error(ErrorCode::NotFound, "task not found"));
        }
        let mut after = 0;
        if let Some(cursor) = cursor {
            let bytes = URL_SAFE_NO_PAD
                .decode(cursor)
                .map_err(|_| error(ErrorCode::CursorInvalid, "invalid cursor"))?;
            let (session, epoch, seq): (String, String, u64) = serde_json::from_slice(&bytes)
                .map_err(|_| error(ErrorCode::CursorInvalid, "invalid cursor"))?;
            if session != state.meta.session_id {
                return Err(error(ErrorCode::CursorInvalid, "cursor session mismatch"));
            }
            if epoch != state.meta.cursor_epoch {
                return Err(error(ErrorCode::CursorExpired, "cursor epoch expired"));
            }
            if seq > state.seq {
                return Err(error(ErrorCode::CursorInvalid, "cursor ahead of journal"));
            }
            after = seq;
        }
        let file = File::open(root.join("journal.jsonl"))?;
        if file.metadata()?.len() > max {
            return Err(error(ErrorCode::QuotaExceeded, "journal exceeds quota"));
        }
        let mut lines = BufReader::new(file);
        let mut line = Vec::new();
        let mut entries = Vec::new();
        let mut encoded_bytes = 2;
        let mut next = after;
        'records: while {
            line.clear();
            lines.read_until(b'\n', &mut line)? != 0
        } {
            if line.last() != Some(&b'\n') {
                return Err(error(
                    ErrorCode::JournalIncomplete,
                    "incomplete journal record",
                ));
            }
            let rec: Record = serde_json::from_slice(&line)
                .map_err(|e| error(ErrorCode::JournalCorrupt, e.to_string()))?;
            if rec.seq <= after {
                continue;
            }
            let before = entries.len();
            for event in rec.events {
                if let Event::Output {
                    task_id: id,
                    stream,
                    text,
                } = event
                {
                    if id == task_id {
                        if text.len() > 1024 {
                            return Err(error(
                                ErrorCode::JournalCorrupt,
                                "oversized output record",
                            ));
                        }
                        let entry = json!({"seq":rec.seq,"stream":stream,"text":text});
                        let size =
                            serde_json::to_vec(&entry)?.len() + usize::from(!entries.is_empty());
                        if entries.len() == usize::from(limit.unwrap_or(32))
                            || encoded_bytes + size > 15 * 1024
                        {
                            entries.truncate(before);
                            break 'records;
                        }
                        encoded_bytes += size;
                        entries.push(entry);
                    }
                }
            }
            next = rec.seq;
        }
        let cursor = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(
            state.meta.session_id.as_str(),
            state.meta.cursor_epoch.as_str(),
            next,
        ))?);
        Ok(json!({"task_id":task_id,"entries":entries,"next_cursor":cursor}))
    }
    #[cfg(windows)]
    pub fn engine_ready(&mut self, identity: EngineIdentity) -> Result<()> {
        self.append(vec![Event::EngineReady { identity }])
    }
    #[cfg(windows)]
    pub fn native_ack(
        &mut self,
        task_id: &str,
        native_id: &str,
        agent_invoked: Option<bool>,
    ) -> Result<()> {
        self.ensure_native(task_id, native_id)?;
        self.append(vec![Event::Ack {
            task_id: task_id.into(),
            native_id: native_id.into(),
            agent_invoked,
        }])
    }
    pub fn claim_abort(&mut self) -> Result<Option<AbortWork>> {
        let Some(task_id) = self
            .state
            .active
            .as_ref()
            .filter(|task_id| self.state.tasks[*task_id].cancellation == Cancellation::Requested)
            .cloned()
        else {
            return Ok(None);
        };
        let (dialogs, interaction_ids) = self
            .state
            .interactions
            .values()
            .filter(|i| i.view.task_id == task_id && i.view.state == "pending")
            .map(|i| (i.native_id.clone(), i.view.interaction_id.clone()))
            .unzip();
        self.append(vec![Event::AbortDispatched {
            task_id: task_id.clone(),
            interaction_ids,
        }])?;
        Ok(Some(AbortWork { task_id, dialogs }))
    }
    pub fn claim_late_cancel(&mut self) -> Result<Option<(String, String)>> {
        let Some(task_id) = self.state.active.as_ref() else {
            return Ok(None);
        };
        if !matches!(
            self.state.tasks[task_id].cancellation,
            Cancellation::Requested | Cancellation::Dispatched | Cancellation::Acknowledged
        ) {
            return Ok(None);
        }
        let Some((interaction_id, native_id)) = self
            .state
            .interactions
            .values()
            .find(|i| &i.view.task_id == task_id && i.view.state == "pending")
            .map(|i| (i.view.interaction_id.clone(), i.native_id.clone()))
        else {
            return Ok(None);
        };
        self.append(vec![Event::LateCancelDispatched {
            interaction_id: interaction_id.clone(),
        }])?;
        Ok(Some((interaction_id, native_id)))
    }
    pub fn late_cancel_sent(&mut self, interaction_id: &str) -> Result<()> {
        self.append(vec![Event::LateCancelSent {
            interaction_id: interaction_id.into(),
        }])
    }
    pub fn abort_ack(&mut self, task_id: &str) -> Result<()> {
        self.ensure_task(task_id)?;
        self.append(vec![Event::AbortAcknowledged {
            task_id: task_id.into(),
        }])
    }
    pub fn native_result(
        &mut self,
        task_id: &str,
        native_id: &str,
        status: &str,
        agent_invoked: bool,
        settled: bool,
        error: Option<String>,
    ) -> Result<()> {
        self.ensure_native(task_id, native_id)?;
        self.append(vec![Event::Result {
            task_id: task_id.into(),
            native_id: native_id.into(),
            status: status.into(),
            agent_invoked,
            settled,
            error,
        }])
    }
    pub fn output(&mut self, task_id: &str, stream: &str, text: &str) -> Result<()> {
        self.ensure_task(task_id)?;
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + 1024).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            self.append(vec![Event::Output {
                task_id: task_id.into(),
                stream: stream.into(),
                text: text[start..end].into(),
            }])?;
            start = end;
        }
        Ok(())
    }
    pub fn interaction_open(
        &mut self,
        task_id: &str,
        native_id: &str,
        kind: &str,
        title: &str,
        message: Option<String>,
        options: Vec<String>,
    ) -> Result<()> {
        self.ensure_task(task_id)?;
        self.append(vec![Event::InteractionOpened {
            interaction_id: id(),
            task_id: task_id.into(),
            native_id: native_id.into(),
            kind: kind.into(),
            title: title.into(),
            message,
            options,
        }])
    }
    pub fn interaction_cancel(&mut self, native_id: &str) -> Result<()> {
        self.append(vec![Event::InteractionCancelled {
            native_id: native_id.into(),
        }])
    }
    pub fn interaction_sent(&mut self, interaction_id: &str) -> Result<()> {
        if !self.state.interactions.contains_key(interaction_id) {
            return Err(error(ErrorCode::NotFound, "interaction not found"));
        }
        self.append(vec![Event::InteractionSent {
            interaction_id: interaction_id.into(),
        }])
    }
    pub fn engine_state(&mut self, is_settled: bool, has_pending_async_work: bool) -> Result<()> {
        self.append(vec![Event::EngineState {
            settled: is_settled,
            pending: has_pending_async_work,
        }])
    }
    pub fn unknown(&mut self, task_id: &str, native_id: Option<&str>, message: &str) -> Result<()> {
        if let Some(native_id) = native_id {
            self.ensure_native(task_id, native_id)?;
        } else {
            self.ensure_task(task_id)?;
        }
        self.append(vec![Event::Unknown {
            task_id: task_id.into(),
            native_id: native_id.map(str::to_owned),
            message: message.into(),
        }])
    }
    pub fn closed(&mut self, reason: &str) -> Result<()> {
        self.append(vec![Event::Closed {
            reason: reason.into(),
        }])
    }
    #[cfg(windows)]
    pub fn failed(&mut self, message: &str) -> Result<()> {
        self.append(vec![Event::Failed {
            message: message.into(),
        }])
    }
    fn ensure_task(&self, task_id: &str) -> Result<()> {
        if self.state.tasks.contains_key(task_id) {
            Ok(())
        } else {
            Err(error(ErrorCode::NotFound, "task not found"))
        }
    }
    fn ensure_native(&self, task_id: &str, native_id: &str) -> Result<()> {
        let task = self
            .state
            .tasks
            .get(task_id)
            .ok_or_else(|| error(ErrorCode::NotFound, "task not found"))?;
        if task
            .native_id
            .as_deref()
            .is_some_and(|existing| existing != native_id)
        {
            return Err(error(ErrorCode::Conflict, "native id mismatch"));
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
