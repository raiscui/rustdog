use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    io::{self, IsTerminal, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use zenoh::Wait;

use super::session_payload::{render_session_close_payload, render_session_open_payload};
use crate::{
    control_frames::{
        ControlFrame, PtyAttachedFrame, PtyReadyFrame, PtyResizeFrame, PtyStdinFrame,
    },
    zenoh_identity::{
        build_session_to_control_key_with_root, build_session_to_daemon_key_with_root,
    },
};

/// 普通 line-control 请求在 session channel 上等待 final `@response` 的上限。
///
/// 这里故意不要复用 3 秒级的 Zenoh request timeout:
/// - request timeout 适合 target resolve / session open 这种网络控制面操作。
/// - `@window-activate`、`@script` 等普通控制指令可能包含真实 OS side-effect。
/// - Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 是一次等待超时 tick,不是 subscriber closed。
const LINE_CONTROL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct ZenohClientSessionBridge {
    session_id: String,
    publisher: zenoh::pubsub::Publisher<'static>,
    subscriber:
        zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
    session: zenoh::Session,
    to_daemon_key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ZenohPtyStdinMode {
    RawTty,
    Pipe,
}

pub(super) fn build_client_session_bridge(
    session: &zenoh::Session,
    keyexpr_root: &str,
    namespace: &str,
    control_key: &str,
    timeout: Duration,
) -> io::Result<ZenohClientSessionBridge> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_key = build_session_to_control_key_with_root(keyexpr_root, namespace, &session_id);
    let subscriber = session
        .declare_subscriber(session_key)
        .wait()
        .map_err(to_io_error)?;
    let to_daemon_key = build_session_to_daemon_key_with_root(keyexpr_root, namespace, &session_id);
    let publisher = session
        .declare_publisher(to_daemon_key.clone())
        .wait()
        .map_err(to_io_error)?;

    ensure_daemon_session_bridge_open(session, control_key, &session_id, timeout)?;

    Ok(ZenohClientSessionBridge {
        session_id,
        publisher,
        subscriber,
        session: session.clone(),
        to_daemon_key,
    })
}

pub(super) fn close_client_session_bridge(
    session_bridge: &mut ZenohClientSessionBridge,
) -> io::Result<()> {
    session_bridge
        .publisher
        .put(render_session_close_payload(&session_bridge.session_id))
        .wait()
        .map_err(to_io_error)
}

fn wait_for_pty_ready_over_session_bridge(
    session_bridge: &mut ZenohClientSessionBridge,
    open_line: String,
) -> io::Result<PtyReadyFrame> {
    session_bridge
        .publisher
        .put(open_line)
        .wait()
        .map_err(to_io_error)?;

    loop {
        let sample = session_bridge
            .subscriber
            .recv_timeout(Duration::from_secs(3600))
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Zenoh PTY subscriber 在 ready 前关闭",
                )
            })?;
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
            ControlFrame::PtyReady(frame) => return Ok(frame),
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY open 返回了普通响应而不是 @pty-ready: {response}"),
                ));
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY open 收到意外 frame: {}", frame.to_wire_message()),
                ));
            }
        }
    }
}

fn wait_for_pty_attached_over_session_bridge(
    session_bridge: &mut ZenohClientSessionBridge,
    attach_line: String,
) -> io::Result<PtyAttachedFrame> {
    session_bridge
        .publisher
        .put(attach_line)
        .wait()
        .map_err(to_io_error)?;

    loop {
        let sample = session_bridge
            .subscriber
            .recv_timeout(Duration::from_secs(3600))
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Zenoh PTY subscriber 在 attach 前关闭",
                )
            })?;
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
            ControlFrame::PtyAttached(frame) => return Ok(frame),
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY attach 返回了普通响应而不是 @pty-attached: {response}"),
                ));
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Zenoh PTY attach 收到意外 frame: {}",
                        frame.to_wire_message()
                    ),
                ));
            }
        }
    }
}

fn run_client_pty_over_session_bridge_threaded_stdin(
    subscriber: &mut zenoh::pubsub::Subscriber<
        zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
    >,
    input_session: zenoh::Session,
    input_key: String,
    ready: PtyReadyFrame,
    fail_on_nonzero_exit: bool,
    stdin_mode: ZenohPtyStdinMode,
) -> io::Result<()> {
    let input_session_id = ready.session_id.clone();
    let stop_input = Arc::new(AtomicBool::new(false));
    let input_stop_signal = Arc::clone(&stop_input);
    let stdin_session = input_session.clone();
    let stdin_key = input_key.clone();
    let mut input_thread = Some(thread::spawn(move || -> io::Result<()> {
        let mut stdin = io::stdin();
        let mut buffer = [0_u8; 4096];
        while !input_stop_signal.load(Ordering::Relaxed) {
            match stdin.read(&mut buffer) {
                Ok(0) if stdin_mode == ZenohPtyStdinMode::RawTty => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Ok(0) => return Ok(()),
                Ok(len) => {
                    log::debug!(
                        "Zenoh PTY client stdin produced bytes: pty_session_id={}, bytes={}",
                        input_session_id,
                        len
                    );
                    let frame = PtyStdinFrame {
                        session_id: input_session_id.clone(),
                        data: BASE64_STANDARD.encode(&buffer[..len]),
                    };
                    stdin_session
                        .put(stdin_key.as_str(), frame.to_wire_message())
                        .wait()
                        .map_err(to_io_error)?;
                    log::debug!(
                        "Zenoh PTY client stdin frame published: pty_session_id={}, bytes={}",
                        input_session_id,
                        len
                    );
                }
                Err(err)
                    if stdin_mode == ZenohPtyStdinMode::RawTty
                        && matches!(
                            err.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        ) =>
                {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }));
    let resize_thread = if stdin_mode == ZenohPtyStdinMode::RawTty {
        Some(spawn_zenoh_pty_resize_thread(
            input_session.clone(),
            input_key.clone(),
            ready.session_id.clone(),
            Arc::clone(&stop_input),
        ))
    } else {
        None
    };
    let mut resize_thread = resize_thread;

    let mut stdout = io::stdout();
    loop {
        match subscriber.recv_timeout(Duration::from_millis(25)) {
            Ok(Some(sample)) => {
                let payload = sample.payload().try_to_string().map_err(to_io_error)?;

                match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
                    ControlFrame::PtyOutput(frame) if frame.session_id == ready.session_id => {
                        let bytes =
                            BASE64_STANDARD
                                .decode(frame.data.as_bytes())
                                .map_err(|err| {
                                    io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!("@pty-output base64 数据无法解码: {err}"),
                                    )
                                })?;
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                    }
                    ControlFrame::PtyExit(frame) if frame.session_id == ready.session_id => {
                        stop_input.store(true, Ordering::Relaxed);
                        join_zenoh_pty_stdin_thread_for_terminal(&mut input_thread, stdin_mode)?;
                        join_zenoh_pty_resize_thread(&mut resize_thread)?;
                        if !fail_on_nonzero_exit || frame.exit_code == 0 {
                            return Ok(());
                        }
                        return Err(io::Error::other(format!(
                            "remote PTY exited with code {}",
                            frame.exit_code
                        )));
                    }
                    ControlFrame::PtyClosed(frame) if frame.session_id == ready.session_id => {
                        stop_input.store(true, Ordering::Relaxed);
                        join_zenoh_pty_stdin_thread_for_terminal(&mut input_thread, stdin_mode)?;
                        join_zenoh_pty_resize_thread(&mut resize_thread)?;
                        return Err(io::Error::other(format!(
                            "remote PTY closed before natural exit: {}",
                            frame.reason
                        )));
                    }
                    _ => {}
                }
            }
            Ok(None) => {}
            Err(err) => {
                return Err(zenoh_pty_subscriber_closed_before_terminal(
                    err,
                    &ready.session_id,
                ));
            }
        }

        join_finished_zenoh_pty_stdin_thread(&mut input_thread)?;
        join_finished_zenoh_pty_resize_thread(&mut resize_thread)?;
    }
}

fn spawn_zenoh_pty_resize_thread(
    session: zenoh::Session,
    input_key: String,
    pty_session_id: String,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut last_size = crate::pty_control::default_terminal_size();
        let (cols, rows) = last_size;
        let initial_frame = PtyResizeFrame {
            session_id: pty_session_id.clone(),
            cols,
            rows,
        };
        session
            .put(input_key.as_str(), initial_frame.to_wire_message())
            .wait()
            .map_err(to_io_error)?;

        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            let size = crate::pty_control::default_terminal_size();
            if size == last_size {
                continue;
            }

            last_size = size;
            let (cols, rows) = size;
            let frame = PtyResizeFrame {
                session_id: pty_session_id.clone(),
                cols,
                rows,
            };
            session
                .put(input_key.as_str(), frame.to_wire_message())
                .wait()
                .map_err(to_io_error)?;
            log::debug!(
                "Zenoh PTY client resize frame published: pty_session_id={}, cols={}, rows={}",
                pty_session_id,
                cols,
                rows
            );
        }

        Ok(())
    })
}

fn join_finished_zenoh_pty_resize_thread(
    resize_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    if resize_thread
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        return join_zenoh_pty_resize_thread(resize_thread);
    }

    Ok(())
}

fn join_zenoh_pty_resize_thread(
    resize_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let Some(handle) = resize_thread.take() else {
        return Ok(());
    };

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("Zenoh PTY resize thread panicked")),
    }
}

fn join_zenoh_pty_stdin_thread_for_terminal(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
    stdin_mode: ZenohPtyStdinMode,
) -> io::Result<()> {
    if stdin_mode == ZenohPtyStdinMode::RawTty {
        return join_zenoh_pty_stdin_thread(input_thread);
    }

    join_finished_zenoh_pty_stdin_thread(input_thread)
}

fn join_finished_zenoh_pty_stdin_thread(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    if input_thread
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        return join_zenoh_pty_stdin_thread(input_thread);
    }

    Ok(())
}

fn join_zenoh_pty_stdin_thread(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let Some(handle) = input_thread.take() else {
        return Ok(());
    };
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("Zenoh PTY stdin thread panicked")),
    }
}

pub(super) fn run_client_pty_over_session_bridge_tty(
    session_bridge: &mut ZenohClientSessionBridge,
    open_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let ready = wait_for_pty_ready_over_session_bridge(session_bridge, open_line)?;
    let _raw_guard = crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?;
    run_client_pty_over_session_bridge_threaded_stdin(
        &mut session_bridge.subscriber,
        session_bridge.session.clone(),
        session_bridge.to_daemon_key.clone(),
        ready,
        fail_on_nonzero_exit,
        ZenohPtyStdinMode::RawTty,
    )
}

pub(super) fn run_client_pty_over_session_bridge_owned(
    mut session_bridge: ZenohClientSessionBridge,
    open_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let ready = wait_for_pty_ready_over_session_bridge(&mut session_bridge, open_line)?;
    let session_id = session_bridge.session_id.clone();
    let raw_guard = if std::io::stdin().is_terminal() {
        crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?
    } else {
        None
    };
    let stdin_mode = if raw_guard.is_some() {
        ZenohPtyStdinMode::RawTty
    } else {
        ZenohPtyStdinMode::Pipe
    };
    let result = {
        let _raw_guard = raw_guard;
        run_client_pty_over_session_bridge_threaded_stdin(
            &mut session_bridge.subscriber,
            session_bridge.session.clone(),
            session_bridge.to_daemon_key.clone(),
            ready,
            fail_on_nonzero_exit,
            stdin_mode,
        )
    };
    let close_result = session_bridge
        .publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .map_err(to_io_error);

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

pub(super) fn run_client_pty_attach_over_session_bridge_owned(
    mut session_bridge: ZenohClientSessionBridge,
    attach_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let attached = wait_for_pty_attached_over_session_bridge(&mut session_bridge, attach_line)?;
    let ready = PtyReadyFrame {
        session_id: attached.session_id,
        cols: attached.cols,
        rows: attached.rows,
    };
    let session_id = session_bridge.session_id.clone();
    let raw_guard = if std::io::stdin().is_terminal() {
        crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?
    } else {
        None
    };
    let stdin_mode = if raw_guard.is_some() {
        ZenohPtyStdinMode::RawTty
    } else {
        ZenohPtyStdinMode::Pipe
    };
    let result = {
        let _raw_guard = raw_guard;
        run_client_pty_over_session_bridge_threaded_stdin(
            &mut session_bridge.subscriber,
            session_bridge.session.clone(),
            session_bridge.to_daemon_key.clone(),
            ready,
            fail_on_nonzero_exit,
            stdin_mode,
        )
    };
    let close_result = session_bridge
        .publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .map_err(to_io_error);

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

fn ensure_daemon_session_bridge_open(
    session: &zenoh::Session,
    control_key: &str,
    session_id: &str,
    timeout: Duration,
) -> io::Result<()> {
    let replies = session
        .get(control_key)
        .payload(render_session_open_payload(session_id))
        .timeout(timeout)
        .wait()
        .map_err(to_io_error)?;

    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            return Ok(());
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("Zenoh session open 超时,未收到 control_key={control_key} 的 ack"),
    ))
}

pub(super) fn execute_remote_request(
    session: &zenoh::Session,
    control_key: &str,
    line: &str,
    timeout: Duration,
    session_bridge: &mut ZenohClientSessionBridge,
) -> io::Result<String> {
    ensure_daemon_session_bridge_open(session, control_key, &session_bridge.session_id, timeout)?;

    session_bridge
        .publisher
        .put(line.to_owned())
        .wait()
        .map_err(to_io_error)?;

    let mut frames = Vec::new();
    let response_deadline = Instant::now() + LINE_CONTROL_RESPONSE_TIMEOUT.max(timeout);
    loop {
        let now = Instant::now();
        if now >= response_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "Zenoh session bridge 在 {}ms 内未收到 final @response",
                    LINE_CONTROL_RESPONSE_TIMEOUT.max(timeout).as_millis()
                ),
            ));
        }

        // ------------------------------------------------------------
        // `recv_timeout()` 返回 `Ok(None)` 表示这轮等待超时。
        // 它不是 subscriber closed,所以这里继续等待直到明确 deadline。
        // 这样慢一点的 GUI side-effect,例如 `@window-activate`,
        // 不会被误报为 “subscriber 在收到结果前关闭”。
        // ------------------------------------------------------------
        let recv_timeout = timeout.min(response_deadline.saturating_duration_since(now));
        let sample = session_bridge
            .subscriber
            .recv_timeout(recv_timeout)
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?;
        let Some(sample) = sample else {
            continue;
        };
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        frames.push(payload.to_string());

        if payload.starts_with("@response ") {
            return Ok(frames.join("\n"));
        }
    }
}

fn zenoh_pty_subscriber_closed_before_terminal(
    err: impl std::fmt::Display,
    session_id: &str,
) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!(
            "Zenoh PTY subscriber 在收到 terminal lifecycle frame 前关闭: session_id={session_id}, error={err}"
        ),
    )
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}
