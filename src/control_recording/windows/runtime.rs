use super::super::{
    BoundedQueue, CaptureEvent, CaptureSpec, CaptureStop, CapturedInput, RecorderCapture,
    RecorderError,
};
use super::{
    hooks::{self, HookCommand, HookRuntime},
    uia,
    wire::{self, Command as Request, Reply},
};
use std::{
    collections::VecDeque,
    io,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::POINT,
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::CREATE_NO_WINDOW,
    },
};

const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const REAP_TIMEOUT: Duration = Duration::from_secs(3);
const SOFT_TIMEOUT: Duration = Duration::from_millis(250);
const HARD_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct ReaderState {
    ready: bool,
    frozen: Option<Option<u64>>,
    last: Option<u64>,
    fault: Option<String>,
    eof: bool,
    exit_success: Option<bool>,
}
fn frozen_result(state: &ReaderState) -> Result<CaptureStop, RecorderError> {
    if let Some(message) = &state.fault {
        return Err(error(message));
    }
    match (state.frozen, state.eof, state.exit_success) {
        (Some(last_sequence), true, Some(true)) => Ok(CaptureStop { last_sequence }),
        _ => Err(error("helper freeze, EOF or successful exit not confirmed")),
    }
}
fn wait_for_stop(
    queue: &BoundedQueue,
    state: &Mutex<ReaderState>,
    drain: &mut dyn FnMut(&BoundedQueue) -> io::Result<()>,
    mut reap: impl FnMut() -> bool,
    timeout: Duration,
) -> Result<CaptureStop, RecorderError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        drain(queue).map_err(error)?;
        let outcome = {
            let s = state.lock().expect("reader state poisoned");
            (s.frozen, s.eof, s.fault.clone())
        };
        if let Some(fault) = outcome.2 {
            return Err(error(fault));
        }
        if matches!((outcome.0, outcome.1), (Some(_), true)) && reap() {
            drain(queue).map_err(error)?;
            return frozen_result(&state.lock().expect("reader state poisoned"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(error("helper freeze or exit timeout"))
}
struct OwnedChild {
    child: Child,
    job: OwnedHandle,
    input: Option<ChildStdin>,
    reader: Option<JoinHandle<()>>,
    state: Arc<Mutex<ReaderState>>,
}
pub struct WindowsCapture {
    queue: Arc<BoundedQueue>,
    owned: Option<OwnedChild>,
    anchor: u64,
}

fn error(message: impl ToString) -> RecorderError {
    RecorderError::Backend(message.to_string())
}
fn job() -> io::Result<OwnedHandle> {
    // SAFETY: 空安全属性和名称创建匿名且不可继承的 Job.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Win32 返回的句柄由本对象独占, 只释放一次.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: Job 句柄有效, limits 的 ABI 和大小与 API 要求一致.
    if unsafe {
        SetInformationJobObject(
            handle.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}
fn reader(mut output: impl io::Read, queue: Arc<BoundedQueue>, state: Arc<Mutex<ReaderState>>) {
    loop {
        let reply = wire::read_frame::<Reply>(&mut output);
        let mut s = state.lock().expect("reader state poisoned");
        match reply {
            Ok(Some(Reply::Ready)) if !s.ready && s.last.is_none() && s.frozen.is_none() => {
                s.ready = true
            }
            Ok(Some(Reply::Event(event))) if s.ready && s.frozen.is_none() => {
                let expected = s.last.map_or(Some(0), |v| v.checked_add(1));
                let sequence = event.sequence;
                if Some(sequence) != expected || !queue.push(event) {
                    s.fault = Some("event sequence or queue overflow".into());
                    break;
                }
                s.last = Some(sequence);
            }
            Ok(Some(Reply::Frozen { last_sequence }))
                if s.ready && s.frozen.is_none() && last_sequence == s.last =>
            {
                s.frozen = Some(last_sequence)
            }
            Ok(Some(Reply::Error(message))) => {
                s.fault = Some(message);
                break;
            }
            Ok(None) => {
                s.eof = true;
                if s.frozen.is_none() {
                    s.fault = Some("unexpected helper EOF".into());
                }
                break;
            }
            Ok(Some(_)) => {
                s.fault = Some("invalid helper message order or frozen boundary".into());
                break;
            }
            Err(e) => {
                s.fault = Some(format!("helper frame: {e}"));
                break;
            }
        }
    }
}
impl WindowsCapture {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(BoundedQueue::new()),
            owned: None,
            anchor: 0,
        }
    }
    fn terminate(&mut self) {
        if let Some(owned) = &mut self.owned {
            owned.input.take();
            // SAFETY: 本对象持有 Job 句柄, 调用期间不会释放.
            unsafe {
                TerminateJobObject(owned.job.as_raw_handle(), 1);
            }
            let _ = owned.child.kill();
        }
    }
    fn reap(&mut self) -> bool {
        let Some(owned) = &mut self.owned else {
            return true;
        };
        let status = match owned.child.try_wait() {
            Ok(Some(status)) => status,
            _ => return false,
        };
        owned
            .state
            .lock()
            .expect("reader state poisoned")
            .exit_success = Some(status.success());
        if !owned.reader.as_ref().is_none_or(JoinHandle::is_finished) {
            return false;
        }
        if let Some(reader) = owned.reader.take() {
            let _ = reader.join();
        }
        self.owned = None;
        true
    }
    fn wait_reap(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.reap() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.reap()
    }
    fn fail_start(&mut self, message: impl ToString) -> RecorderError {
        self.terminate();
        self.wait_reap(REAP_TIMEOUT);
        error(message)
    }
}
impl RecorderCapture for WindowsCapture {
    fn start(&mut self, spec: &CaptureSpec) -> Result<(), RecorderError> {
        if self.owned.is_some() {
            return Err(error("previous helper has not been reaped"));
        }
        self.queue = Arc::new(BoundedQueue::new());
        let job = job().map_err(error)?;
        let mut child = Command::new(std::env::current_exe().map_err(error)?)
            .arg("__recording-helper")
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(error)?;
        // SAFETY: 两个句柄有效, 子进程收到配置前不会安装 hook.
        let assignment_error =
            if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0
            {
                Some(io::Error::last_os_error())
            } else {
                None
            };
        let input = child.stdin.take();
        let output = child.stdout.take().expect("piped stdout exists");
        let state = Arc::new(Mutex::new(ReaderState::default()));
        let reader_state = Arc::clone(&state);
        let queue = Arc::clone(&self.queue);
        let reader = thread::spawn(move || reader(output, queue, reader_state));
        self.owned = Some(OwnedChild {
            child,
            job,
            input,
            reader: Some(reader),
            state,
        });
        if let Some(e) = assignment_error {
            return Err(self.fail_start(format!("assign helper to job: {e}")));
        }
        self.anchor = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let input = self
            .owned
            .as_mut()
            .and_then(|o| o.input.as_mut())
            .expect("piped stdin exists");
        if let Err(e) = wire::write_frame(input, &Request::Configure(spec.clone())) {
            return Err(self.fail_start(e));
        }
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            let outcome = {
                let s = self
                    .owned
                    .as_ref()
                    .unwrap()
                    .state
                    .lock()
                    .expect("reader state poisoned");
                if let Some(e) = &s.fault {
                    Some(Err(e.clone()))
                } else if s.ready {
                    Some(Ok(()))
                } else {
                    None
                }
            };
            match outcome {
                Some(Ok(())) => return Ok(()),
                Some(Err(e)) => return Err(self.fail_start(e)),
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
        Err(self.fail_start("helper readiness timeout"))
    }
    fn stop(
        &mut self,
        drain: &mut dyn FnMut(&BoundedQueue) -> io::Result<()>,
    ) -> Result<CaptureStop, RecorderError> {
        if self.owned.is_none() {
            return Err(error("helper not running"));
        }
        if let Err(e) = drain(&self.queue) {
            self.terminate();
            self.wait_reap(REAP_TIMEOUT);
            return Err(error(e));
        }
        let owned = self.owned.as_mut().expect("helper is running");
        let sent = owned
            .input
            .take()
            .ok_or_else(|| error("helper input closed"))
            .and_then(|mut input| wire::write_frame(&mut input, &Request::Freeze).map_err(error));
        let state = Arc::clone(&owned.state);
        let queue = Arc::clone(&self.queue);
        let result =
            sent.and_then(|()| wait_for_stop(&queue, &state, drain, || self.reap(), STOP_TIMEOUT));
        if result.is_err() {
            self.terminate();
            self.wait_reap(REAP_TIMEOUT);
        }
        result
    }
    fn parameter_boundary(&mut self) -> Result<(), RecorderError> {
        let input = self
            .owned
            .as_mut()
            .and_then(|owned| owned.input.as_mut())
            .ok_or_else(|| error("helper input closed"))?;
        wire::write_frame(input, &Request::ParameterBoundary).map_err(error)
    }
    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }
    fn wall_clock_anchor_ms(&self) -> u64 {
        self.anchor
    }
    fn is_healthy(&self) -> bool {
        self.owned.as_ref().is_some_and(|o| {
            let s = o.state.lock().expect("reader state poisoned");
            s.ready && s.fault.is_none() && !s.eof && !self.queue.overflowed()
        })
    }
    fn cleanup_pending(&mut self) -> bool {
        !self.reap()
    }
}
impl Drop for WindowsCapture {
    fn drop(&mut self) {
        if self.owned.is_some() {
            self.terminate();
            self.wait_reap(REAP_TIMEOUT);
        }
    }
}

struct Query {
    sequence: u64,
    evidence: hooks::IdentityEvidence,
    x: i32,
    y: i32,
    queued: Instant,
    dispatched: Option<Instant>,
    missing: bool,
}
struct Answer {
    sequence: u64,
    result: Result<Option<uia::Candidate>, &'static str>,
    observed_ns: u64,
}
fn emit_at(
    out: &mut impl io::Write,
    seq: &mut u64,
    monotonic_ns: u64,
    input: CapturedInput,
) -> Result<(), String> {
    let sequence = *seq;
    *seq = seq.checked_add(1).ok_or("sequence exhausted")?;
    wire::write_frame(
        out,
        &Reply::Event(CaptureEvent {
            sequence,
            monotonic_ns,
            input,
        }),
    )
    .map_err(|e| e.to_string())
}
fn emit(
    out: &mut impl io::Write,
    seq: &mut u64,
    origin: Instant,
    input: CapturedInput,
) -> Result<(), String> {
    emit_at(
        out,
        seq,
        origin.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        input,
    )
}
fn read_commands(
    mut input: impl io::Read,
    commands: mpsc::SyncSender<io::Result<Option<Request>>>,
) {
    loop {
        let command = wire::read_frame::<Request>(&mut input);
        let done = !matches!(command, Ok(Some(Request::ParameterBoundary)));
        if commands.send(command).is_err() || done {
            break;
        }
    }
}
fn helper() -> Result<(), String> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let spec = match wire::read_frame::<Request>(&mut input).map_err(|e| e.to_string())? {
        Some(Request::Configure(spec)) => spec,
        _ => return Err("expected configuration".into()),
    };
    drop(input);
    // SAFETY: DPI context 仅修改本进程, 这些调用不解引用用户指针.
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            DPI_AWARENESS_PER_MONITOR_AWARE,
        };
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext())
            != DPI_AWARENESS_PER_MONITOR_AWARE
        {
            return Err("per-monitor DPI awareness unavailable".into());
        }
    }
    let origin = Instant::now();
    let (events_tx, events) = mpsc::sync_channel(1024);
    let (hook_tx, hook_rx) = mpsc::sync_channel(1);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let fault = Arc::new(AtomicBool::new(false));
    let hook_fault = Arc::clone(&fault);
    let hook_spec = spec.clone();
    let hook = thread::spawn(move || {
        hooks::run(HookRuntime {
            spec: hook_spec,
            events: events_tx,
            commands: hook_rx,
            ready: ready_tx,
            fault: hook_fault,
            origin,
        })
    });
    let (query_tx, query_rx) = mpsc::sync_channel::<(u64, hooks::IdentityEvidence, i32, i32)>(1024);
    let (answer_tx, answer_rx) = mpsc::channel::<Answer>();
    let (uia_ready_tx, uia_ready_rx) = mpsc::sync_channel(1);
    let semantic = spec.semantic;
    let uia_thread = thread::spawn(move || {
        if !semantic {
            let _ = uia_ready_tx.send(Ok(()));
            return;
        }
        let automation = match uia::Automation::new() {
            Ok(a) => {
                let _ = uia_ready_tx.send(Ok(()));
                a
            }
            Err(e) => {
                let _ = uia_ready_tx.send(Err(e));
                return;
            }
        };
        while let Ok((sequence, evidence, x, y)) = query_rx.recv() {
            let result = if hooks::valid_identity(&evidence) {
                automation.candidate(evidence.window.hwnd as _, POINT { x, y })
            } else {
                Ok(None)
            };
            let observed_ns = origin.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            if answer_tx
                .send(Answer {
                    sequence,
                    result,
                    observed_ns,
                })
                .is_err()
            {
                break;
            }
        }
    });
    ready_rx.recv().map_err(|_| "hook startup failed")??;
    uia_ready_rx
        .recv()
        .map_err(|_| "UIA startup failed")?
        .map_err(str::to_owned)?;
    let mut out = io::stdout().lock();
    wire::write_frame(&mut out, &Reply::Ready).map_err(|e| e.to_string())?;
    let (command_tx, command_rx) = mpsc::sync_channel(1);
    thread::spawn(move || read_commands(io::stdin().lock(), command_tx));
    let mut seq = 0u64;
    let mut pending = VecDeque::<Query>::new();
    let mut freezing = false;
    let mut hooks_done = false;
    loop {
        if !freezing {
            match command_rx.try_recv() {
                Ok(Ok(Some(Request::Freeze))) => {
                    freezing = true;
                    hook_tx
                        .send(HookCommand::Freeze)
                        .map_err(|_| "hook command failed")?;
                }
                Ok(Ok(Some(Request::ParameterBoundary))) => hook_tx
                    .send(HookCommand::ParameterBoundary)
                    .map_err(|_| "hook command failed")?,
                Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("invalid command or parent EOF".into())
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        loop {
            match events.try_recv() {
                Ok(event) => {
                    let query = match &event.input {
                        CapturedInput::MouseDown { x, y, .. } if spec.semantic => Some((
                            event.evidence.ok_or("mouse identity evidence missing")?,
                            *x,
                            *y,
                        )),
                        _ => None,
                    };
                    emit_at(&mut out, &mut seq, event.at_ns, event.input)?;
                    if let Some((evidence, x, y)) = query {
                        let for_sequence = seq - 1;
                        if freezing || !hooks::valid_identity(&evidence) {
                            emit(
                                &mut out,
                                &mut seq,
                                origin,
                                CapturedInput::SemanticMissing { for_sequence },
                            )?;
                        } else {
                            if pending.len() >= 1024 {
                                return Err("UIA pending queue overflow".into());
                            }
                            pending.push_back(Query {
                                sequence: for_sequence,
                                evidence,
                                x,
                                y,
                                queued: Instant::now(),
                                dispatched: None,
                                missing: false,
                            });
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    hooks_done = true;
                    break;
                }
            }
        }
        if fault.load(Ordering::Acquire) {
            return Err("hook queue overflow".into());
        }
        for query in &mut pending {
            if (freezing || query.queued.elapsed() >= SOFT_TIMEOUT) && !query.missing {
                emit(
                    &mut out,
                    &mut seq,
                    origin,
                    CapturedInput::SemanticMissing {
                        for_sequence: query.sequence,
                    },
                )?;
                query.missing = true;
            }
            if query
                .dispatched
                .is_some_and(|started| started.elapsed() >= HARD_TIMEOUT)
            {
                return Err("UIA provider hard timeout".into());
            }
        }
        pending.retain(|query| !query.missing || query.dispatched.is_some());
        while let Ok(answer) = answer_rx.try_recv() {
            let Some(query) = pending.pop_front() else {
                return Err("unexpected UIA answer".into());
            };
            if answer.sequence != query.sequence {
                return Err("UIA answer order".into());
            }
            if !query.missing {
                let input = match answer.result {
                    Ok(Some(candidate)) if hooks::valid_identity(&query.evidence) => {
                        CapturedInput::SemanticCandidate {
                            for_sequence: query.sequence,
                            window: query.evidence.window,
                            observed_ns: answer.observed_ns,
                            control_type: candidate.control_type,
                            bounds: candidate.bounds,
                        }
                    }
                    _ => CapturedInput::SemanticMissing {
                        for_sequence: query.sequence,
                    },
                };
                emit(&mut out, &mut seq, origin, input)?;
            }
        }
        if let Some(query) = pending.front_mut() {
            if query.dispatched.is_none() && !hooks::valid_identity(&query.evidence) {
                emit(
                    &mut out,
                    &mut seq,
                    origin,
                    CapturedInput::SemanticMissing {
                        for_sequence: query.sequence,
                    },
                )?;
                pending.pop_front();
            } else if query.dispatched.is_none() {
                query_tx
                    .try_send((query.sequence, query.evidence.clone(), query.x, query.y))
                    .map_err(|_| "UIA queue failed")?;
                query.dispatched = Some(Instant::now());
            }
        }
        if freezing && hooks_done && pending.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    drop(query_tx);
    uia_thread.join().map_err(|_| "UIA thread panicked")?;
    hook.join().map_err(|_| "hook thread panicked")??;
    wire::write_frame(
        &mut out,
        &Reply::Frozen {
            last_sequence: seq.checked_sub(1),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn run_helper() -> Result<(), String> {
    let result = helper();
    if let Err(message) = &result {
        let _ = wire::write_frame(&mut io::stdout().lock(), &Reply::Error(message.clone()));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run_reader(
        replies: Vec<Reply>,
        capacity: usize,
    ) -> (Arc<BoundedQueue>, Arc<Mutex<ReaderState>>) {
        let mut bytes = Vec::new();
        for reply in replies {
            wire::write_frame(&mut bytes, &reply).unwrap();
        }
        let queue = Arc::new(BoundedQueue::with_capacity(capacity));
        let state = Arc::new(Mutex::new(ReaderState::default()));
        reader(&bytes[..], Arc::clone(&queue), Arc::clone(&state));
        (queue, state)
    }
    fn event(sequence: u64) -> Reply {
        Reply::Event(CaptureEvent {
            sequence,
            monotonic_ns: 1,
            input: CapturedInput::SemanticMissing { for_sequence: 1 },
        })
    }
    #[test]
    fn valid_freeze_requires_matching_sequence_and_eof() {
        let (queue, state) = run_reader(
            vec![
                Reply::Ready,
                event(0),
                Reply::Frozen {
                    last_sequence: Some(0),
                },
            ],
            1,
        );
        let state = state.lock().unwrap();
        assert!(state.fault.is_none());
        assert!(state.eof);
        assert_eq!(state.frozen, Some(Some(0)));
        let mut observed = Vec::new();
        queue.drain_all(|event| observed.push(event.sequence));
        assert_eq!(observed, vec![0]);
    }
    #[test]
    fn sequence_gap_and_mismatched_freeze_fail() {
        let (_, state) = run_reader(vec![Reply::Ready, event(2)], 2);
        assert!(state.lock().unwrap().fault.is_some());
        let (_, state) = run_reader(
            vec![
                Reply::Ready,
                event(0),
                Reply::Frozen {
                    last_sequence: None,
                },
            ],
            2,
        );
        assert!(state.lock().unwrap().fault.is_some());
    }
    #[test]
    fn eof_before_freeze_and_queue_overflow_fail() {
        let (_, state) = run_reader(vec![Reply::Ready], 2);
        assert!(state.lock().unwrap().fault.is_some());
        let (queue, state) = run_reader(vec![Reply::Ready, event(0), event(1)], 1);
        assert!(state.lock().unwrap().fault.is_some());
        assert!(queue.overflowed());
    }
    #[test]
    fn command_reader_keeps_boundaries_and_reports_eof() {
        let mut bytes = Vec::new();
        wire::write_frame(&mut bytes, &Request::ParameterBoundary).unwrap();
        wire::write_frame(&mut bytes, &Request::ParameterBoundary).unwrap();
        let (tx, rx) = mpsc::sync_channel(3);
        read_commands(&bytes[..], tx);
        assert!(matches!(
            rx.recv().unwrap().unwrap(),
            Some(Request::ParameterBoundary)
        ));
        assert!(matches!(
            rx.recv().unwrap().unwrap(),
            Some(Request::ParameterBoundary)
        ));
        assert!(rx.recv().unwrap().unwrap().is_none());
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    #[test]
    fn stop_rejects_unknown_or_failed_child_exit() {
        let mut state = ReaderState {
            frozen: Some(Some(1)),
            eof: true,
            ..ReaderState::default()
        };
        assert!(frozen_result(&state).is_err());
        state.exit_success = Some(false);
        assert!(frozen_result(&state).is_err());
        state.exit_success = Some(true);
        assert_eq!(frozen_result(&state).unwrap().last_sequence, Some(1));
        state.fault = Some("reader failed".into());
        assert!(frozen_result(&state).is_err());
    }
    fn queued_event(sequence: u64) -> CaptureEvent {
        CaptureEvent {
            sequence,
            monotonic_ns: 1,
            input: CapturedInput::SemanticMissing { for_sequence: 0 },
        }
    }
    #[test]
    fn stop_drains_while_waiting_and_after_reap() {
        let queue = BoundedQueue::with_capacity(1);
        let state = Mutex::new(ReaderState::default());
        assert!(queue.push(queued_event(0)));
        let mut seen = Vec::new();
        let mut calls = 0;
        let mut drain = |queue: &BoundedQueue| {
            queue.drain_all(|event| seen.push(event.sequence));
            calls += 1;
            if calls == 1 {
                assert!(queue.push(queued_event(1)));
            }
            if calls == 2 {
                let mut state = state.lock().unwrap();
                state.frozen = Some(Some(2));
                state.eof = true;
            }
            Ok(())
        };
        let stop = wait_for_stop(
            &queue,
            &state,
            &mut drain,
            || {
                assert!(queue.push(queued_event(2)));
                state.lock().unwrap().exit_success = Some(true);
                true
            },
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(stop.last_sequence, Some(2));
        assert_eq!(seen, vec![0, 1, 2]);
        assert!(!queue.overflowed());
    }
    #[test]
    fn stop_propagates_drain_failure_before_reap() {
        let queue = BoundedQueue::with_capacity(1);
        let state = Mutex::new(ReaderState {
            frozen: Some(None),
            eof: true,
            ..ReaderState::default()
        });
        let mut reaped = false;
        let result = wait_for_stop(
            &queue,
            &state,
            &mut |_| Err(io::Error::other("disk failure")),
            || {
                reaped = true;
                true
            },
            Duration::from_secs(1),
        );
        assert!(result.is_err());
        assert!(!reaped);
    }
}
