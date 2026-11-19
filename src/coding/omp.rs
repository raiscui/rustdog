use std::num::NonZeroUsize;
use std::process::{ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use omp_rpc::{
    AbortCommand, AssistantMessageEvent, CancelUiResponse, Client, ClientOptions,
    ConfirmUiResponse, Event, ExtensionUiRequest, ExtensionUiResponse, GetStateCommand, LitTrue,
    PromptCommand, RpcAgentEvent, RpcInbound, RpcNotification, ValueUiResponse,
};

use super::model::{CodingError, ErrorCode, InteractionAnswer, Result};

#[derive(Debug)]
pub struct NativeAttempt {
    pub native_id: String,
    pub acknowledgment: Result<Option<bool>>,
}

#[derive(Debug)]
pub struct NativeState {
    pub barrier_id: String,
    pub native_session_id: String,
    pub is_streaming: bool,
    pub is_settled: bool,
    pub has_pending_async_work: bool,
}

#[derive(Debug)]
pub enum EngineEvent {
    Text(String),
    ToolProgress(String),
    PromptResult {
        native_id: String,
        status: String,
        agent_invoked: bool,
        session_settled: bool,
        error: Option<String>,
    },
    Interaction {
        native_id: String,
        kind: String,
        title: String,
        message: Option<String>,
        options: Vec<String>,
    },
    InteractionCancelled {
        native_id: String,
    },
    Settled,
    Barrier(String),
    Fatal(String),
}

#[derive(Clone)]
pub struct Adapter {
    inner: Arc<AdapterInner>,
}

struct AdapterInner {
    client: Client,
    closed: Arc<AtomicBool>,
    normalizer: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for AdapterInner {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl AdapterInner {
    fn disconnect(&self) {
        self.closed.store(true, Ordering::Release);
        self.client.disconnect();
        if let Some(thread) = self
            .normalizer
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = thread.join();
        }
    }
}

fn engine_error(error: impl std::fmt::Display) -> CodingError {
    CodingError::new(ErrorCode::Engine, error.to_string())
}

impl Adapter {
    pub fn connect(
        stdout: ChildStdout,
        stdin: ChildStdin,
    ) -> Result<(Self, Receiver<EngineEvent>)> {
        let options = ClientOptions {
            event_capacity: NonZeroUsize::new(32).expect("事件容量非零"),
            ..ClientOptions::default()
        };
        let (client, incoming) = Client::from_io(stdout, stdin, options).map_err(engine_error)?;
        let closed = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(32);
        let thread_closed = Arc::clone(&closed);
        let normalizer = thread::Builder::new()
            .name("coding-omp-events".into())
            .spawn(move || {
                let closed = thread_closed;
                loop {
                    if closed.load(Ordering::Acquire) {
                        return;
                    }
                    match incoming.recv_timeout(Duration::from_millis(50)) {
                        Ok(event) => {
                            if let Some(event) = normalize(event) {
                                if !send_event(&sender, &closed, event) {
                                    return;
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                if !closed.load(Ordering::Acquire) {
                    send_event(
                        &sender,
                        &closed,
                        EngineEvent::Fatal("OMP transport closed".into()),
                    );
                }
            })
            .map_err(engine_error)?;
        let adapter = Self {
            inner: Arc::new(AdapterInner {
                client,
                closed,
                normalizer: Mutex::new(Some(normalizer)),
            }),
        };
        Ok((adapter, receiver))
    }

    pub fn submit(&self, prompt: &str) -> NativeAttempt {
        let outcome = self.inner.client.call_correlated(&PromptCommand {
            message: prompt.to_owned(),
            images: None,
            streaming_behavior: None,
        });
        NativeAttempt {
            native_id: outcome.request_id,
            acknowledgment: outcome
                .response
                .map(|ack| ack.agent_invoked)
                .map_err(engine_error),
        }
    }

    pub fn state(&self) -> Result<NativeState> {
        let outcome = self
            .inner
            .client
            .call_correlated_with_barrier(&GetStateCommand {});
        let state = outcome.response.map_err(engine_error)?;
        Ok(NativeState {
            barrier_id: outcome.request_id,
            native_session_id: state.session_id,
            is_streaming: state.is_streaming,
            is_settled: state.is_settled,
            has_pending_async_work: state.has_pending_async_work,
        })
    }

    pub fn abort(&self) -> Result<()> {
        self.inner
            .client
            .call(&AbortCommand {})
            .map_err(engine_error)
    }

    pub fn cancel_ui(&self, native_id: &str) -> Result<()> {
        self.inner
            .client
            .send(&RpcInbound::ExtensionUiResponse(
                ExtensionUiResponse::CancelUiResponse(CancelUiResponse {
                    id: native_id.to_owned(),
                    cancelled: LitTrue,
                    timed_out: None,
                }),
            ))
            .map_err(engine_error)
    }

    pub fn respond(&self, native_id: &str, answer: &InteractionAnswer) -> Result<()> {
        let response = match answer {
            InteractionAnswer::Confirm { confirmed } => {
                ExtensionUiResponse::ConfirmUiResponse(ConfirmUiResponse {
                    id: native_id.to_owned(),
                    confirmed: *confirmed,
                })
            }
            InteractionAnswer::Select { value }
            | InteractionAnswer::Input { value }
            | InteractionAnswer::Editor { value } => match value {
                Some(value) => ExtensionUiResponse::ValueUiResponse(ValueUiResponse {
                    id: native_id.to_owned(),
                    value: value.clone(),
                }),
                None => return self.cancel_ui(native_id),
            },
        };
        self.inner
            .client
            .send(&RpcInbound::ExtensionUiResponse(response))
            .map_err(engine_error)
    }

    pub fn disconnect(&self) {
        self.inner.disconnect();
    }
}

fn send_event(
    sender: &SyncSender<EngineEvent>,
    closed: &AtomicBool,
    mut event: EngineEvent,
) -> bool {
    loop {
        if closed.load(Ordering::Acquire) {
            return false;
        }
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(pending)) => {
                event = pending;
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn normalize(event: Event) -> Option<EngineEvent> {
    match event {
        Event::Undecodable { error, .. } => Some(EngineEvent::Fatal(format!(
            "OMP frame decode failed: {error}"
        ))),
        Event::ResponseBarrier(id) => Some(EngineEvent::Barrier(id)),
        Event::UnmatchedResponse(_) => None,
        Event::Notification(notification) => match notification {
            RpcNotification::PromptResult(result) => match result.id {
                Some(native_id) => Some(EngineEvent::PromptResult {
                    native_id,
                    status: result.status.as_str().to_owned(),
                    agent_invoked: result.agent_invoked,
                    session_settled: result.session_settled,
                    error: result.error.map(|error| error.message),
                }),
                None => Some(EngineEvent::Fatal(
                    "OMP prompt result has no request id".into(),
                )),
            },
            RpcNotification::SessionSettled(_) => Some(EngineEvent::Settled),
            RpcNotification::RpcFrameError(error) => Some(EngineEvent::Fatal(error.error)),
            RpcNotification::ExtensionUiRequest(request) => match request {
                ExtensionUiRequest::Select(request) => Some(EngineEvent::Interaction {
                    native_id: request.id,
                    kind: "select".into(),
                    title: request.title,
                    message: None,
                    options: request.options,
                }),
                ExtensionUiRequest::Confirm(request) => Some(EngineEvent::Interaction {
                    native_id: request.id,
                    kind: "confirm".into(),
                    title: request.title,
                    message: Some(request.message),
                    options: Vec::new(),
                }),
                ExtensionUiRequest::Input(request) => Some(EngineEvent::Interaction {
                    native_id: request.id,
                    kind: "input".into(),
                    title: request.title,
                    message: request.placeholder,
                    options: Vec::new(),
                }),
                ExtensionUiRequest::Editor(request) => Some(EngineEvent::Interaction {
                    native_id: request.id,
                    kind: "editor".into(),
                    title: request.title,
                    message: request.prefill,
                    options: Vec::new(),
                }),
                ExtensionUiRequest::Cancel(request) => Some(EngineEvent::InteractionCancelled {
                    native_id: request.target_id,
                }),
                _ => None,
            },
            RpcNotification::RpcAgentEvent(event) => match event {
                RpcAgentEvent::MessageUpdate(update) => match update.assistant_message_event {
                    AssistantMessageEvent::TextDelta(delta) => delta.delta.map(EngineEvent::Text),
                    _ => None,
                },
                RpcAgentEvent::ToolExecutionStart(start) => {
                    Some(EngineEvent::ToolProgress(bounded(start.tool_name)))
                }
                RpcAgentEvent::ToolExecutionEnd(end) => {
                    Some(EngineEvent::ToolProgress(bounded(end.tool_name)))
                }
                _ => None,
            },
            _ => None,
        },
    }
}

fn bounded(mut text: String) -> String {
    if text.len() > 1024 {
        let mut end = 1024;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
