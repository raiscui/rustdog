use super::model::Result;
#[cfg(not(windows))]
use super::model::{CodingError, ErrorCode};
use std::path::PathBuf;

#[cfg(not(windows))]
pub fn run(_session_dir: PathBuf, _instance_id: String) -> Result<()> {
    Err(CodingError::new(
        ErrorCode::UnsupportedPlatform,
        "coding host requires Windows",
    ))
}

#[cfg(windows)]
mod windows {
    use std::fs::{self, OpenOptions};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use super::super::failure::FailureSlot;
    use super::super::ipc::{HostCall, Server};
    use super::super::manager;
    use super::super::model::{
        CodingError, CodingReply, CodingRequest, EngineIdentity, ErrorCode, InteractionAnswer,
        ProcessIdentity, Result,
    };
    use super::super::omp::{Adapter, EngineEvent, NativeAttempt, NativeState};
    use super::super::platform;
    use super::super::process::OwnedOmp;
    use super::super::store::{AbortWork, EngineAction, Store};

    const TICK: Duration = Duration::from_millis(20);
    const START_TIMEOUT: Duration = Duration::from_secs(120);

    enum Work {
        Prompt(String, String),
        Abort(AbortWork),
        Respond(String, String, InteractionAnswer),
        CancelDialog(String, String),
        State(u64),
    }
    enum Done {
        Prompt(String, NativeAttempt),
        Abort(String, Result<()>),
        Respond(String, Result<bool>),
        CancelDialog(String, Result<()>),
        State(u64, Result<NativeState>),
    }
    struct Workers {
        prompt: SyncSender<Work>,
        abort: SyncSender<Work>,
        respond: SyncSender<Work>,
        state: SyncSender<Work>,
        handles: Vec<JoinHandle<()>>,
    }
    impl Workers {
        fn new(
            adapter: &Adapter,
            done: SyncSender<Done>,
            pending: Arc<Mutex<Option<(String, bool)>>>,
        ) -> Result<Self> {
            fn worker(
                adapter: Adapter,
                done: SyncSender<Done>,
                name: &str,
                pending: Arc<Mutex<Option<(String, bool)>>>,
            ) -> Result<(SyncSender<Work>, JoinHandle<()>)> {
                let (tx, rx) = mpsc::sync_channel(1);
                let handle = thread::Builder::new()
                    .name(name.into())
                    .spawn(move || {
                        while let Ok(work) = rx.recv() {
                            let result = match work {
                                Work::Prompt(id, prompt) => {
                                    Done::Prompt(id, adapter.submit(&prompt))
                                }
                                Work::Abort(work) => {
                                    let result = work
                                        .dialogs
                                        .iter()
                                        .try_for_each(|id| adapter.cancel_ui(id))
                                        .and_then(|()| adapter.abort());
                                    Done::Abort(work.task_id, result)
                                }
                                Work::Respond(id, native, answer) => {
                                    let allowed = pending
                                        .lock()
                                        .expect("interaction state lock poisoned")
                                        .as_ref()
                                        .is_some_and(|(current, cancelled)| {
                                            current == &native && !cancelled
                                        });
                                    Done::Respond(
                                        id,
                                        if allowed {
                                            adapter.respond(&native, &answer).map(|()| true)
                                        } else {
                                            Ok(false)
                                        },
                                    )
                                }
                                Work::CancelDialog(id, native) => {
                                    Done::CancelDialog(id, adapter.cancel_ui(&native))
                                }
                                Work::State(generation) => Done::State(generation, adapter.state()),
                            };
                            if done.send(result).is_err() {
                                break;
                            }
                        }
                    })
                    .map_err(CodingError::from)?;
                Ok((tx, handle))
            }
            let (prompt, a) = worker(
                adapter.clone(),
                done.clone(),
                "coding-prompt",
                Arc::clone(&pending),
            )?;
            let (abort, b) = worker(
                adapter.clone(),
                done.clone(),
                "coding-abort",
                Arc::clone(&pending),
            )?;
            let (respond, c) = worker(
                adapter.clone(),
                done.clone(),
                "coding-respond",
                Arc::clone(&pending),
            )?;
            let (state, d) = worker(adapter.clone(), done, "coding-state", pending)?;
            Ok(Self {
                prompt,
                abort,
                respond,
                state,
                handles: vec![a, b, c, d],
            })
        }
        fn join(self) -> Result<()> {
            drop(self.prompt);
            drop(self.abort);
            drop(self.respond);
            drop(self.state);
            let mut failed = false;
            for handle in self.handles {
                failed |= handle.join().is_err();
            }
            if failed {
                Err(CodingError::new(ErrorCode::Engine, "OMP worker panicked"))
            } else {
                Ok(())
            }
        }
    }
    struct Candidate {
        native_id: String,
        status: String,
        agent_invoked: bool,
        settled: bool,
        error: Option<String>,
    }
    struct Runtime {
        adapter: Adapter,
        events: Receiver<EngineEvent>,
        workers: Workers,
        done: Receiver<Done>,
        generation: u64,
        state_busy: bool,
        state_started: Option<Instant>,
        pending_state: Option<(u64, NativeState)>,
        last_barrier: Option<String>,
        startup_identity: Option<ProcessIdentity>,
        state_needed: bool,
        prompt_busy: bool,
        abort_busy: bool,
        respond_busy: bool,
        candidate: Option<Candidate>,
        pending_interaction: Arc<Mutex<Option<(String, bool)>>>,
    }
    impl Runtime {
        fn invalidate(&mut self) {
            self.generation = self.generation.wrapping_add(1);
            self.state_needed = true;
        }
        fn schedule_state(&mut self) -> Result<()> {
            if self.state_needed && !self.state_busy {
                self.workers
                    .state
                    .send(Work::State(self.generation))
                    .map_err(|_| CodingError::new(ErrorCode::Engine, "state worker stopped"))?;
                self.state_needed = false;
                self.state_busy = true;
                self.state_started = Some(Instant::now());
            }
            Ok(())
        }
        fn apply_state(&mut self, store: &mut Store) -> Result<()> {
            let Some((generation, state)) = self.pending_state.take() else {
                return Ok(());
            };
            if self.last_barrier.as_deref() != Some(state.barrier_id.as_str()) {
                self.pending_state = Some((generation, state));
                return Ok(());
            }
            self.last_barrier = None;
            self.state_busy = false;
            self.state_started = None;
            if generation == self.generation {
                if let Some(process) = self.startup_identity.take() {
                    store.engine_ready(EngineIdentity {
                        process,
                        native_session_id: state.native_session_id,
                    })?;
                }
                store.engine_state(
                    state.is_settled && !state.is_streaming,
                    state.has_pending_async_work,
                )?;
            } else {
                self.state_needed = true;
            }
            Ok(())
        }
        fn claim_abort(&mut self, store: &mut Store) -> Result<()> {
            if !self.prompt_busy && !self.abort_busy {
                if let Some(task) = store.claim_abort()? {
                    send_work(&self.workers.abort, Work::Abort(task))?;
                    self.abort_busy = true;
                }
            }
            Ok(())
        }
        fn claim_late_cancel(&mut self, store: &mut Store) -> Result<()> {
            if !self.respond_busy {
                if let Some((id, native)) = store.claim_late_cancel()? {
                    send_work(&self.workers.respond, Work::CancelDialog(id, native))?;
                    self.respond_busy = true;
                }
            }
            Ok(())
        }
        fn result(&mut self, store: &mut Store, task: &str, result: Candidate) -> Result<()> {
            store.native_result(
                task,
                &result.native_id,
                &result.status,
                result.agent_invoked,
                result.settled,
                result.error,
            )?;
            self.invalidate();
            Ok(())
        }
        fn done(&mut self, store: &mut Store, done: Done) -> Result<()> {
            match done {
                Done::Prompt(task, attempt) => {
                    self.prompt_busy = false;
                    match attempt.acknowledgment {
                        Ok(agent_invoked) => {
                            store.native_ack(&task, &attempt.native_id, agent_invoked)?;
                        }
                        Err(error) => {
                            store.unknown(&task, Some(&attempt.native_id), &error.message)?;
                        }
                    }
                    if let Some(candidate) = self.candidate.take() {
                        if candidate.native_id != attempt.native_id {
                            return Err(CodingError::new(
                                ErrorCode::Engine,
                                "native result identity mismatch",
                            ));
                        }
                        self.result(store, &task, candidate)?;
                    }
                }
                Done::Abort(task, result) => {
                    self.abort_busy = false;
                    match result {
                        Ok(()) => {
                            store.abort_ack(&task)?;
                            self.invalidate();
                        }
                        Err(error) => store.unknown(&task, None, &error.message)?,
                    }
                }
                Done::Respond(id, result) => {
                    self.respond_busy = false;
                    let cancelled = self
                        .pending_interaction
                        .lock()
                        .expect("interaction state lock poisoned")
                        .take()
                        .is_some_and(|(_, cancelled)| cancelled);
                    match result {
                        Ok(true) if !cancelled => {
                            store.interaction_sent(&id)?;
                            self.invalidate();
                        }
                        Ok(_) => {}
                        Err(error) => return Err(error),
                    }
                }
                Done::CancelDialog(id, result) => {
                    self.respond_busy = false;
                    result?;
                    store.late_cancel_sent(&id)?;
                    self.invalidate();
                }
                Done::State(generation, result) => {
                    self.pending_state = Some((generation, result?));
                    self.apply_state(store)?;
                }
            }
            Ok(())
        }
        fn event(&mut self, store: &mut Store, event: EngineEvent) -> Result<()> {
            let active = store.state().active_task_id().map(str::to_owned);
            match event {
                EngineEvent::Text(text) => {
                    if let Some(task) = active {
                        store.output(&task, "text", &text)?;
                    }
                }
                EngineEvent::ToolProgress(text) => {
                    if let Some(task) = active {
                        store.output(&task, "tool_progress", &text)?;
                    }
                }
                EngineEvent::Interaction {
                    native_id,
                    kind,
                    title,
                    message,
                    options,
                } => {
                    if let Some(task) = active {
                        store
                            .interaction_open(&task, &native_id, &kind, &title, message, options)?;
                        self.invalidate();
                    }
                }
                EngineEvent::InteractionCancelled { native_id } => {
                    if let Some((current, cancelled)) = self
                        .pending_interaction
                        .lock()
                        .expect("interaction state lock poisoned")
                        .as_mut()
                    {
                        if current == &native_id {
                            *cancelled = true;
                        }
                    }
                    store.interaction_cancel(&native_id)?;
                    self.invalidate();
                }
                EngineEvent::PromptResult {
                    native_id,
                    status,
                    agent_invoked,
                    session_settled,
                    error,
                } => {
                    let task = store
                        .state()
                        .task_id_for_native(&native_id)
                        .map(str::to_owned);
                    let result = Candidate {
                        native_id,
                        status,
                        agent_invoked,
                        settled: session_settled,
                        error,
                    };
                    if let Some(task) = task {
                        self.result(store, &task, result)?;
                    } else if active.is_some() && self.prompt_busy && self.candidate.is_none() {
                        self.candidate = Some(result);
                    } else {
                        return Err(CodingError::new(
                            ErrorCode::Engine,
                            "unmatched native result",
                        ));
                    }
                }
                EngineEvent::Settled => {
                    self.invalidate();
                }
                EngineEvent::Barrier(id) => {
                    self.last_barrier = Some(id);
                    self.apply_state(store)?;
                }
                EngineEvent::Fatal(message) => {
                    return Err(CodingError::new(ErrorCode::Engine, message))
                }
            }
            Ok(())
        }
    }
    fn send_work(sender: &SyncSender<Work>, work: Work) -> Result<()> {
        sender
            .send(work)
            .map_err(|_| CodingError::new(ErrorCode::Engine, "OMP worker stopped"))
    }
    fn reply(
        call: HostCall,
        store: &mut Store,
        runtime: Option<&mut Runtime>,
        dir: &Path,
        max_bytes: u64,
    ) -> Result<Option<Receiver<Result<()>>>> {
        let request = call.envelope.request;
        if matches!(request, CodingRequest::TaskSubmit { .. })
            && runtime
                .as_ref()
                .is_none_or(|r| r.startup_identity.is_some())
        {
            let _ = call.reply.send(CodingReply::failure(CodingError::new(
                ErrorCode::Starting,
                "OMP is starting",
            )));
            return Ok(None);
        }
        if matches!(request, CodingRequest::InteractionRespond { .. })
            && runtime.as_ref().is_some_and(|r| r.respond_busy)
        {
            match Store::query_offline(dir, request, max_bytes) {
                Ok(value) => {
                    let _ = call.reply.send(CodingReply::success(value));
                }
                Err(error) if error.code == ErrorCode::HostUnavailable => {
                    let _ = call.reply.send(CodingReply::failure(CodingError::new(
                        ErrorCode::Busy,
                        "interaction worker busy",
                    )));
                }
                Err(error) => {
                    let fatal = matches!(
                        error.code,
                        ErrorCode::Storage
                            | ErrorCode::QuotaExceeded
                            | ErrorCode::Io
                            | ErrorCode::JournalCorrupt
                            | ErrorCode::JournalIncomplete
                    );
                    let _ = call.reply.send(CodingReply::failure(error.clone()));
                    if fatal {
                        return Err(error);
                    }
                }
            }
            return Ok(None);
        }
        let disposition = match store.request(request) {
            Ok(value) => value,
            Err(error) => {
                let fatal = matches!(
                    error.code,
                    ErrorCode::Storage
                        | ErrorCode::QuotaExceeded
                        | ErrorCode::Io
                        | ErrorCode::JournalCorrupt
                        | ErrorCode::JournalIncomplete
                );
                let _ = call.reply.send(CodingReply::failure(error.clone()));
                return if fatal { Err(error) } else { Ok(None) };
            }
        };
        let close = matches!(disposition.effect, Some(EngineAction::Close));
        if let Some(effect) = disposition.effect {
            match effect {
                EngineAction::Close => {}
                EngineAction::Prompt { task_id, prompt } => {
                    let r = runtime
                        .ok_or_else(|| CodingError::new(ErrorCode::Starting, "OMP is starting"))?;
                    r.invalidate();
                    r.prompt_busy = true;
                    send_work(&r.workers.prompt, Work::Prompt(task_id, prompt))?;
                }
                EngineAction::Respond {
                    interaction_id,
                    native_id,
                    answer,
                } => {
                    let r = runtime
                        .ok_or_else(|| CodingError::new(ErrorCode::Starting, "OMP is starting"))?;
                    r.respond_busy = true;
                    *r.pending_interaction
                        .lock()
                        .expect("interaction state lock poisoned") =
                        Some((native_id.clone(), false));
                    send_work(
                        &r.workers.respond,
                        Work::Respond(interaction_id, native_id, answer),
                    )?;
                }
            }
        }
        let _ = call.reply.send(CodingReply::success(disposition.reply));
        Ok(if close { Some(call.written) } else { None })
    }
    fn startup(
        meta: &super::super::model::SessionMetadata,
        dir: &Path,
    ) -> Result<(
        OwnedOmp,
        std::process::ChildStdout,
        std::process::ChildStdin,
    )> {
        let omp_dir = dir.join("omp");
        platform::secure_directory(&omp_dir)?;
        let stderr = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("omp.stderr.log"))?;
        let mut command = Command::new(&meta.omp_executable);
        command
            .arg("--profile")
            .arg(&meta.profile)
            .arg("--mode")
            .arg("rpc")
            .arg("--cwd")
            .arg(&meta.cwd)
            .arg("--session-dir")
            .arg(&omp_dir);
        for extension in &meta.extensions {
            command.arg("--extension").arg(extension);
        }
        command.current_dir(&meta.cwd).stderr(Stdio::from(stderr));
        OwnedOmp::spawn(command)
    }
    pub fn run(dir: PathBuf, instance: String) -> Result<()> {
        platform::secure_directory(&dir)?;
        let meta = Store::metadata(&dir)?;
        if dir.file_name().and_then(|name| name.to_str()) != Some(meta.session_id.as_str())
            || instance != meta.host_instance_id
        {
            return Err(CodingError::new(
                ErrorCode::HostIdentityMismatch,
                "session or host instance mismatch",
            ));
        }
        let mut store = Store::open(&dir, meta.max_journal_bytes)?;
        if fs::metadata(dir.join("journal.jsonl"))?.len() != 0 {
            return Err(CodingError::new(
                ErrorCode::Conflict,
                "existing session cannot start another OMP",
            ));
        }
        let mut failure_slot = FailureSlot::create(&dir, &meta.host_instance_id)?;
        manager::write_json_atomic(
            &dir.join("host_identity.json"),
            &platform::current_identity()?,
        )?;
        let (calls_tx, calls_rx) = mpsc::sync_channel(32);
        let server = Server::start(&meta, calls_tx)?;
        let (mut process, stdout, stdin) = match startup(&meta, &dir) {
            Ok(process) => process,
            Err(error) => {
                store.failed(&error.message)?;
                return Err(error);
            }
        };
        let identity = match process.identity() {
            Ok(identity) => identity,
            Err(error) => {
                process.shutdown()?;
                store.failed(&error.message)?;
                return Err(error);
            }
        };
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let startup_thread =
            thread::Builder::new()
                .name("coding-startup".into())
                .spawn(move || {
                    let result = Adapter::connect(stdout, stdin);
                    let _ = startup_tx.send(result);
                })?;
        let mut runtime: Option<Runtime> = None;
        let mut startup_thread = Some(startup_thread);
        let started = Instant::now();
        let mut close_ack = None;
        let outcome = (|| -> Result<()> {
            loop {
                if started.elapsed() >= START_TIMEOUT
                    && runtime
                        .as_ref()
                        .is_none_or(|r| r.startup_identity.is_some())
                {
                    return Err(CodingError::new(ErrorCode::Engine, "OMP startup timed out"));
                }
                if runtime.is_none() {
                    match startup_rx.try_recv() {
                        Ok(Ok((adapter, events))) => {
                            let (done_tx, done) = mpsc::sync_channel(8);
                            let pending_interaction = Arc::new(Mutex::new(None));
                            let workers =
                                Workers::new(&adapter, done_tx, Arc::clone(&pending_interaction))?;
                            runtime = Some(Runtime {
                                adapter,
                                events,
                                workers,
                                done,
                                generation: 0,
                                state_busy: false,
                                state_started: None,
                                state_needed: true,
                                pending_state: None,
                                last_barrier: None,
                                startup_identity: Some(identity.clone()),
                                prompt_busy: false,
                                abort_busy: false,
                                respond_busy: false,
                                candidate: None,
                                pending_interaction,
                            });
                            if let Some(thread) = startup_thread.take() {
                                thread.join().map_err(|_| {
                                    CodingError::new(ErrorCode::Engine, "startup worker panicked")
                                })?;
                            }
                        }
                        Ok(Err(error)) => return Err(error),
                        Err(TryRecvError::Disconnected) => {
                            return Err(CodingError::new(
                                ErrorCode::Engine,
                                "startup worker stopped",
                            ))
                        }
                        Err(TryRecvError::Empty) => {}
                    }
                }
                if let Some(r) = runtime.as_mut() {
                    if r.state_started
                        .is_some_and(|started| started.elapsed() >= START_TIMEOUT)
                    {
                        return Err(CodingError::new(
                            ErrorCode::Engine,
                            "OMP state barrier timed out",
                        ));
                    }
                    for _ in 0..32 {
                        match r.events.try_recv() {
                            Ok(event) => r.event(&mut store, event)?,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                return Err(CodingError::new(
                                    ErrorCode::Engine,
                                    "OMP event worker stopped",
                                ))
                            }
                        }
                    }
                    for _ in 0..8 {
                        match r.done.try_recv() {
                            Ok(done) => r.done(&mut store, done)?,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                return Err(CodingError::new(
                                    ErrorCode::Engine,
                                    "OMP workers stopped",
                                ))
                            }
                        }
                    }
                    r.claim_abort(&mut store)?;
                    r.claim_late_cancel(&mut store)?;
                    r.schedule_state()?;
                }
                match calls_rx.recv_timeout(TICK) {
                    Ok(call) => {
                        if let Some(ack) = reply(
                            call,
                            &mut store,
                            runtime.as_mut(),
                            &dir,
                            meta.max_journal_bytes,
                        )? {
                            close_ack = Some(ack);
                            return Ok(());
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(CodingError::new(
                            ErrorCode::HostUnavailable,
                            "IPC server stopped",
                        ))
                    }
                }
                if !process.is_running()? {
                    return Err(CodingError::new(ErrorCode::Engine, "OMP process exited"));
                }
            }
        })();
        let mut failure = outcome.err();
        if let Err(error) = process.shutdown() {
            if let Some(primary) = &mut failure {
                primary.message = format!("{}; OMP shutdown: {error}", primary.message);
            } else {
                failure = Some(error.into());
            }
        }
        drop(process);
        if let Some(r) = runtime {
            r.adapter.disconnect();
            drop(r.done);
            if let Err(error) = r.workers.join() {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        }
        drop(startup_rx);
        if let Some(thread) = startup_thread {
            if thread.join().is_err() && failure.is_none() {
                failure = Some(CodingError::new(
                    ErrorCode::Engine,
                    "startup worker panicked",
                ));
            }
        }
        if failure.is_none() && close_ack.is_some() {
            if let Err(error) = store.closed("closed by request") {
                failure = Some(error);
            }
        }
        if let Some(error) = &failure {
            if !matches!(
                error.code,
                ErrorCode::Storage
                    | ErrorCode::QuotaExceeded
                    | ErrorCode::JournalCorrupt
                    | ErrorCode::JournalIncomplete
            ) || close_ack.is_some()
            {
                if let Err(journal_error) = store.failed(&error.message) {
                    failure = Some(CodingError::new(
                        journal_error.code,
                        format!(
                            "{}; recording failure: {}",
                            error.message, journal_error.message
                        ),
                    ));
                }
            }
        }
        if let Some(error) = &failure {
            if error.code == ErrorCode::Storage {
                if let Err(report_error) = failure_slot.record(&meta.host_instance_id, error) {
                    eprintln!("无法记录 Host 存储错误: {report_error}");
                }
            }
        }
        let delivery = if let Some(ack) = close_ack {
            match ack.recv_timeout(Duration::from_secs(12)) {
                Ok(result) => result,
                Err(error) => Err(CodingError::new(
                    ErrorCode::HostUnavailable,
                    format!("close reply delivery: {error}"),
                )),
            }
        } else {
            Ok(())
        };
        drop(server);
        if let Some(error) = failure {
            Err(error)
        } else {
            delivery
        }
    }
}

#[cfg(windows)]
pub fn run(session_dir: PathBuf, instance_id: String) -> Result<()> {
    windows::run(session_dir, instance_id)
}
