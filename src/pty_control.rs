use crate::{
    control_frames::{
        ControlFrame, PtyAttachedFrame, PtyClosedFrame, PtyDetachedFrame, PtyExitFrame,
        PtyOutputFrame, PtyReadyFrame, PtyResizeFrame, PtyStdinFrame,
    },
    control_protocol::{
        parse_control_line, ControlCommand, ControlParseResult, PtyAttachRequest, PtyOpenRequest,
    },
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
enum PtyTerminalOutcome {
    ProcessExit { exit_code: i32 },
    Closed { reason: String },
}

#[derive(Debug)]
pub enum PtyRuntimeCommand {
    Stdin(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    Close {
        reason: String,
    },
    Detach {
        reason: String,
    },
    Attach {
        control_session_id: String,
        cols: u16,
        rows: u16,
        sender: mpsc::Sender<ControlFrame>,
        response: mpsc::Sender<io::Result<()>>,
    },
}

pub struct AttachedPtySession {
    session_id: String,
    input_tx: mpsc::Sender<PtyRuntimeCommand>,
    output_rx: mpsc::Receiver<ControlFrame>,
}

impl AttachedPtySession {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn try_recv_frame(&mut self) -> Result<ControlFrame, mpsc::TryRecvError> {
        self.output_rx.try_recv()
    }

    pub fn send_stdin_bytes(&self, bytes: Vec<u8>) -> io::Result<()> {
        self.input_tx
            .send(PtyRuntimeCommand::Stdin(bytes))
            .map_err(|_| io::Error::other("PTY runtime command channel closed"))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.input_tx
            .send(PtyRuntimeCommand::Resize { cols, rows })
            .map_err(|_| io::Error::other("PTY runtime command channel closed"))
    }

    pub fn close(&self, reason: &str) -> io::Result<()> {
        self.input_tx
            .send(PtyRuntimeCommand::Close {
                reason: reason.to_owned(),
            })
            .map_err(|_| io::Error::other("PTY runtime command channel closed"))
    }

    pub fn detach(&self, reason: &str) -> io::Result<()> {
        self.input_tx
            .send(PtyRuntimeCommand::Detach {
                reason: reason.to_owned(),
            })
            .map_err(|_| io::Error::other("PTY runtime command channel closed"))
    }
}

#[cfg(unix)]
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};

#[cfg(unix)]
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        raw::{c_int, c_ulong},
    },
    sync::OnceLock,
};

#[cfg(unix)]
use termios::{
    tcsetattr, Termios, BRKINT, CS8, CSIZE, ECHO, ECHONL, ICANON, ICRNL, IEXTEN, IGNBRK, IGNCR,
    INLCR, INPCK, ISIG, ISTRIP, IXON, OPOST, PARENB, PARMRK, TCSANOW, VMIN, VTIME,
};

/// 可被 PTY runtime 线程安全复用的 frame 发送器。
#[allow(dead_code)]
pub trait PtyFrameSender: Clone + Send + 'static {
    fn send_frame(&self, frame: &ControlFrame) -> io::Result<()>;
}

#[cfg(unix)]
type PtyCloseHandle = Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>;

#[cfg(unix)]
#[derive(Clone)]
struct ActivePtySession {
    close_handle: PtyCloseHandle,
    terminal_reason: Arc<Mutex<Option<String>>>,
    input_tx: mpsc::Sender<PtyRuntimeCommand>,
    attached_control_session_id: Arc<Mutex<Option<String>>>,
}

#[cfg(unix)]
static ACTIVE_PTY_SESSIONS: OnceLock<Mutex<HashMap<String, ActivePtySession>>> = OnceLock::new();

/// 从 line-control 文本中提取 `@pty` 打开请求。
///
/// 这个 helper 是 TCP / WebSocket / Zenoh 的共享入口。
/// 它只识别 `@pty`,不会把 `@pty-close` 误判成打开请求。
pub fn parse_pty_open_request(line: &str) -> io::Result<Option<PtyOpenRequest>> {
    let trimmed = line.trim_start();
    if !is_pty_open_control_line(trimmed) {
        return Ok(None);
    }

    match parse_control_line(line) {
        Ok(ControlParseResult::Control(crate::control_protocol::ControlRequest {
            command: ControlCommand::PtyOpen(request),
            ..
        })) => Ok(Some(request)),
        Ok(_) => Ok(None),
        Err(err) => Err(err),
    }
}

fn is_pty_open_control_line(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("@pty") else {
        return false;
    };

    matches!(
        rest.as_bytes().first().copied(),
        None | Some(b':') | Some(b'#') | Some(b' ') | Some(b'\t')
    )
}

pub fn parse_pty_attach_request(line: &str) -> io::Result<Option<PtyAttachRequest>> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("@pty-attach") {
        return Ok(None);
    }

    match parse_control_line(line) {
        Ok(ControlParseResult::Control(crate::control_protocol::ControlRequest {
            command: ControlCommand::PtyAttach(request),
            ..
        })) => Ok(Some(request)),
        Ok(_) => Ok(None),
        Err(err) => Err(err),
    }
}

/// 尝试按 session id 关闭一个活动 PTY。
///
/// 返回 `Ok(false)` 表示没有找到这个 session。
#[cfg(unix)]
pub fn close_active_pty_session(session_id: &str) -> io::Result<bool> {
    close_active_pty_session_with_reason(session_id, "force_close")
}

#[cfg(unix)]
fn close_active_pty_session_with_reason(session_id: &str, reason: &str) -> io::Result<bool> {
    let session = {
        let sessions = active_pty_sessions()
            .lock()
            .map_err(|_| io::Error::other("PTY session registry lock poisoned"))?;
        sessions.get(session_id).cloned()
    };

    let Some(session) = session else {
        return Ok(false);
    };

    if let Ok(mut terminal_reason) = session.terminal_reason.lock() {
        *terminal_reason = Some(reason.to_owned());
    }

    let mut killer = session
        .close_handle
        .lock()
        .map_err(|_| io::Error::other("PTY close handle lock poisoned"))?;
    killer.kill().map_err(to_io_error)?;
    Ok(true)
}

#[cfg(unix)]
pub fn detach_active_pty_session(session_id: &str) -> io::Result<bool> {
    let session = {
        let sessions = active_pty_sessions()
            .lock()
            .map_err(|_| io::Error::other("PTY session registry lock poisoned"))?;
        sessions.get(session_id).cloned()
    };

    let Some(session) = session else {
        return Ok(false);
    };

    let is_attached = session
        .attached_control_session_id
        .lock()
        .map_err(|_| io::Error::other("PTY attached state lock poisoned"))?
        .is_some();
    if !is_attached {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("PTY session is not currently attached: {session_id}"),
        ));
    }
    session
        .input_tx
        .send(PtyRuntimeCommand::Detach {
            reason: "owner_detach".to_owned(),
        })
        .map_err(|_| io::Error::other("PTY detach command channel closed"))?;
    Ok(true)
}

#[cfg(unix)]
pub fn attach_active_pty_session(
    request: PtyAttachRequest,
) -> io::Result<Option<AttachedPtySession>> {
    let session = {
        let sessions = active_pty_sessions()
            .lock()
            .map_err(|_| io::Error::other("PTY session registry lock poisoned"))?;
        sessions.get(&request.session_id).cloned()
    };

    let Some(session) = session else {
        return Ok(None);
    };

    let was_detached = session
        .attached_control_session_id
        .lock()
        .map_err(|_| io::Error::other("PTY attached state lock poisoned"))?
        .is_none();
    if !was_detached {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("PTY session is already attached: {}", request.session_id),
        ));
    }

    let control_session_id = uuid::Uuid::new_v4().to_string();
    let (frame_tx, frame_rx) = mpsc::channel::<ControlFrame>();
    let (response_tx, response_rx) = mpsc::channel::<io::Result<()>>();
    session
        .input_tx
        .send(PtyRuntimeCommand::Attach {
            control_session_id,
            cols: request.cols,
            rows: request.rows,
            sender: frame_tx,
            response: response_tx,
        })
        .map_err(|_| io::Error::other("PTY attach command channel closed"))?;
    response_rx
        .recv()
        .map_err(|_| io::Error::other("PTY attach response channel closed"))??;

    Ok(Some(AttachedPtySession {
        session_id: request.session_id,
        input_tx: session.input_tx,
        output_rx: frame_rx,
    }))
}

#[cfg(not(unix))]
pub fn attach_active_pty_session(
    _request: PtyAttachRequest,
) -> io::Result<Option<AttachedPtySession>> {
    Ok(None)
}

#[cfg(not(unix))]
pub fn close_active_pty_session(_session_id: &str) -> io::Result<bool> {
    Ok(false)
}

#[cfg(not(unix))]
pub fn detach_active_pty_session(_session_id: &str) -> io::Result<bool> {
    Ok(false)
}

/// 把 CLI argv 渲染成 `@pty` control line。
pub fn render_pty_open_line(argv: &[String], cols: u16, rows: u16) -> io::Result<String> {
    let cmd = argv.first().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--pty` 需要在 `--` 后提供要运行的远端命令",
        )
    })?;

    if cmd.is_empty() || argv.iter().any(|item| item.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--pty` 命令和参数不能为空字符串",
        ));
    }

    let args = argv
        .iter()
        .skip(1)
        .map(|item| format!("\"{}\"", escape_json_string(item)))
        .collect::<Vec<_>>()
        .join(",");

    Ok(format!(
        "@pty:{{cmd:\"{}\",args:[{}],cols:{cols},rows:{rows}}}",
        escape_json_string(cmd),
        args
    ))
}

pub fn render_pty_close_line(session_id: &str) -> io::Result<String> {
    if session_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--pty-close` 的 session id 不能为空",
        ));
    }

    Ok(format!(
        "@pty-close:{{session_id:\"{}\"}}",
        escape_json_string(session_id)
    ))
}

pub fn render_pty_detach_line(session_id: &str) -> io::Result<String> {
    if session_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--pty-detach` 的 session id 不能为空",
        ));
    }

    Ok(format!(
        "@pty-detach:{{session_id:\"{}\"}}",
        escape_json_string(session_id)
    ))
}

pub fn render_pty_attach_line(session_id: &str, cols: u16, rows: u16) -> io::Result<String> {
    if session_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "`--pty-attach` 的 session id 不能为空",
        ));
    }

    Ok(format!(
        "@pty-attach:{{session_id:\"{}\",cols:{cols},rows:{rows}}}",
        escape_json_string(session_id)
    ))
}

/// control client 侧的 PTY 会话。
pub fn run_pty_client_transport(
    transport: &mut crate::control_transport::ControlTransport,
    open_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    transport.write_message(&open_line)?;

    let ready = loop {
        let Some(message) = transport.read_message()? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control connection 在 PTY ready 前关闭",
            ));
        };

        match ControlFrame::parse_inbound_result_message(&message)? {
            ControlFrame::PtyReady(frame) => break frame,
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("PTY open 返回了普通响应而不是 @pty-ready: {response}"),
                ));
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("PTY open 收到意外 frame: {}", frame.to_wire_message()),
                ));
            }
        }
    };

    let raw_guard = LocalRawTerminalGuard::enter_if_tty()?;
    if raw_guard.is_some() {
        let resize_writer = transport.try_clone_writer()?;
        return run_pty_client_tty_loop(
            transport,
            ready,
            fail_on_nonzero_exit,
            raw_guard,
            Some(resize_writer),
        );
    }

    run_pty_client_threaded_stdin_loop(transport, ready, fail_on_nonzero_exit)
}

pub fn run_pty_attach_client_transport(
    transport: &mut crate::control_transport::ControlTransport,
    attach_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    transport.write_message(&attach_line)?;

    let attached = loop {
        let Some(message) = transport.read_message()? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control connection 在 PTY attach 前关闭",
            ));
        };

        match ControlFrame::parse_inbound_result_message(&message)? {
            ControlFrame::PtyAttached(frame) => break frame,
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("PTY attach 返回了普通响应而不是 @pty-attached: {response}"),
                ));
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("PTY attach 收到意外 frame: {}", frame.to_wire_message()),
                ));
            }
        }
    };

    let ready = PtyReadyFrame {
        session_id: attached.session_id,
        cols: attached.cols,
        rows: attached.rows,
    };
    let raw_guard = LocalRawTerminalGuard::enter_if_tty()?;
    if raw_guard.is_some() {
        let resize_writer = transport.try_clone_writer()?;
        return run_pty_client_tty_loop(
            transport,
            ready,
            fail_on_nonzero_exit,
            raw_guard,
            Some(resize_writer),
        );
    }
    run_pty_client_threaded_stdin_loop(transport, ready, fail_on_nonzero_exit)
}

#[cfg(unix)]
pub fn open_attached_pty_session(request: PtyOpenRequest) -> io::Result<AttachedPtySession> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: request.rows,
            cols: request.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(to_io_error)?;

    let mut command = CommandBuilder::new(&request.cmd);
    for arg in &request.args {
        command.arg(arg);
    }

    let mut child = pair.slave.spawn_command(command).map_err(to_io_error)?;
    drop(pair.slave);

    let mut pty_writer = pair.master.take_writer().map_err(to_io_error)?;
    let mut pty_reader = pair.master.try_clone_reader().map_err(to_io_error)?;
    let close_handle = Arc::new(Mutex::new(child.clone_killer()));
    let terminal_reason = Arc::new(Mutex::new(None::<String>));
    let attached_control_session_id = Arc::new(Mutex::new(Some(session_id.clone())));
    let attached_frame_sender = Arc::new(Mutex::new(None::<mpsc::Sender<ControlFrame>>));
    let (input_tx, input_rx) = mpsc::channel::<PtyRuntimeCommand>();
    let (frame_tx, frame_rx) = mpsc::channel::<ControlFrame>();
    if let Ok(mut sender_slot) = attached_frame_sender.lock() {
        *sender_slot = Some(frame_tx.clone());
    }

    register_active_pty_session(
        &session_id,
        ActivePtySession {
            close_handle: Arc::clone(&close_handle),
            terminal_reason: Arc::clone(&terminal_reason),
            input_tx: input_tx.clone(),
            attached_control_session_id: Arc::clone(&attached_control_session_id),
        },
    )?;

    frame_tx
        .send(ControlFrame::PtyReady(PtyReadyFrame {
            session_id: session_id.clone(),
            cols: request.cols,
            rows: request.rows,
        }))
        .map_err(|_| io::Error::other("PTY frame channel closed before ready"))?;

    let output_sender = Arc::clone(&attached_frame_sender);
    let (output_done_tx, output_done_rx) = mpsc::channel::<()>();
    let output_session_id = session_id.clone();
    let output_thread = thread::spawn(move || -> io::Result<()> {
        let mut buffer = [0_u8; 8192];
        loop {
            match pty_reader.read(&mut buffer) {
                Ok(0) => {
                    let _ = output_done_tx.send(());
                    return Ok(());
                }
                Ok(len) => {
                    log::debug!(
                        "PTY output produced: session_id={}, bytes={}",
                        output_session_id,
                        len
                    );
                    let data = BASE64_STANDARD.encode(&buffer[..len]);
                    let frame = ControlFrame::PtyOutput(PtyOutputFrame {
                        session_id: output_session_id.clone(),
                        data,
                    });
                    if let Ok(sender_slot) = output_sender.lock() {
                        if let Some(sender) = sender_slot.as_ref() {
                            if sender.send(frame).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Err(err) => return Err(err),
            }
        }
    });

    let exit_session_id = session_id.clone();
    let (exit_tx, exit_rx) = mpsc::channel::<i32>();
    let wait_thread = thread::spawn(move || -> io::Result<()> {
        let status = child.wait().map_err(to_io_error)?;
        unregister_active_pty_session(&exit_session_id);
        let _ = exit_tx.send(status.exit_code() as i32);
        Ok(())
    });

    let runtime_session_id = session_id.clone();
    thread::spawn(move || {
        let mut terminal_outcome = None::<PtyTerminalOutcome>;

        loop {
            match exit_rx.try_recv() {
                Ok(exit_code) => {
                    drop(pty_writer);
                    let _ = output_done_rx.recv_timeout(Duration::from_millis(150));
                    let forced_reason = terminal_reason
                        .lock()
                        .ok()
                        .and_then(|reason| reason.clone());
                    let outcome = terminal_outcome
                        .clone()
                        .or_else(|| {
                            forced_reason.map(|reason| PtyTerminalOutcome::Closed { reason })
                        })
                        .unwrap_or(PtyTerminalOutcome::ProcessExit { exit_code });
                    if let Ok(sender_slot) = attached_frame_sender.lock() {
                        if let Some(sender) = sender_slot.as_ref() {
                            let _ = send_terminal_outcome_to_channel(
                                sender,
                                &runtime_session_id,
                                outcome.clone(),
                            );
                        } else {
                            let _ = send_terminal_outcome_to_channel(
                                &frame_tx,
                                &runtime_session_id,
                                outcome.clone(),
                            );
                        }
                    }
                    let _ = wait_thread.join();
                    let _ = output_thread.join();
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return,
            }

            match input_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(PtyRuntimeCommand::Stdin(bytes)) => {
                    log::debug!(
                        "PTY stdin received: session_id={}, bytes={}",
                        runtime_session_id,
                        bytes.len()
                    );
                    let _ = pty_writer.write_all(&bytes);
                    let _ = pty_writer.flush();
                }
                Ok(PtyRuntimeCommand::Resize { cols, rows }) => {
                    log::debug!(
                        "PTY resize received: session_id={}, cols={}, rows={}",
                        runtime_session_id,
                        cols,
                        rows
                    );
                    let _ = pair.master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                Ok(PtyRuntimeCommand::Close { reason }) => {
                    terminal_outcome = Some(PtyTerminalOutcome::Closed { reason });
                    let _ = kill_pty_child(&close_handle);
                }
                Ok(PtyRuntimeCommand::Detach { reason }) => {
                    if let Ok(mut attached) = attached_control_session_id.lock() {
                        *attached = None;
                    }
                    if let Ok(mut sender_slot) = attached_frame_sender.lock() {
                        if let Some(sender) = sender_slot.as_ref() {
                            let _ = sender.send(ControlFrame::PtyDetached(PtyDetachedFrame {
                                session_id: runtime_session_id.clone(),
                                reason,
                                detached_at: current_utc_timestamp_string(),
                            }));
                        }
                        *sender_slot = None;
                    }
                }
                Ok(PtyRuntimeCommand::Attach {
                    control_session_id,
                    cols,
                    rows,
                    sender,
                    response,
                }) => {
                    log::info!(
                        "PTY attach requested: session_id={}, control_session_id={}, cols={}, rows={}",
                        runtime_session_id, control_session_id, cols, rows
                    );
                    let resize_result = pair
                        .master
                        .resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(to_io_error);
                    if let Ok(mut attached) = attached_control_session_id.lock() {
                        *attached = Some(control_session_id.clone());
                    }
                    if let Ok(mut sender_slot) = attached_frame_sender.lock() {
                        *sender_slot = Some(sender.clone());
                    }
                    let attached_frame = ControlFrame::PtyAttached(PtyAttachedFrame {
                        session_id: runtime_session_id.clone(),
                        control_session_id,
                        cols,
                        rows,
                        attached_at: current_utc_timestamp_string(),
                    });
                    let attached_frame_clone = attached_frame.clone();
                    let send_result = resize_result.and_then(|_| {
                        sender
                            .send(attached_frame)
                            .map_err(|_| io::Error::other("PTY attach frame channel closed"))
                    });
                    let attach_succeeded = send_result.is_ok();
                    let _ = response.send(send_result);
                    if attach_succeeded {
                        let _ = frame_tx.send(attached_frame_clone);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if terminal_outcome.is_none() {
                        terminal_outcome = Some(PtyTerminalOutcome::Closed {
                            reason: "transport_lost".to_owned(),
                        });
                    }
                    let _ = kill_pty_child(&close_handle);
                }
            }
        }
    });

    Ok(AttachedPtySession {
        session_id,
        input_tx,
        output_rx: frame_rx,
    })
}

fn run_pty_client_threaded_stdin_loop(
    transport: &mut crate::control_transport::ControlTransport,
    ready: PtyReadyFrame,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let mut input_writer = transport.try_clone_writer()?;
    let input_session_id = ready.session_id.clone();
    let input_thread = thread::spawn(move || -> io::Result<()> {
        let mut stdin = io::stdin();
        let mut buffer = [0_u8; 4096];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(len) => {
                    let frame = PtyStdinFrame {
                        session_id: input_session_id.clone(),
                        data: BASE64_STANDARD.encode(&buffer[..len]),
                    };
                    input_writer.write_message(&frame.to_wire_message())?;
                }
                Err(err) => return Err(err),
            }
        }
    });

    let mut stdout = io::stdout();
    loop {
        let Some(message) = transport.read_message()? else {
            return Ok(());
        };

        match ControlFrame::parse_inbound_result_message(&message)? {
            ControlFrame::PtyOutput(frame) if frame.session_id == ready.session_id => {
                let bytes = BASE64_STANDARD
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
                if input_thread.is_finished() {
                    let _ = input_thread.join();
                }
                if fail_on_nonzero_exit && frame.exit_code != 0 {
                    return Err(io::Error::other(format!(
                        "remote PTY exited with code {}",
                        frame.exit_code
                    )));
                }
                return Ok(());
            }
            ControlFrame::PtyClosed(frame) if frame.session_id == ready.session_id => {
                if input_thread.is_finished() {
                    let _ = input_thread.join();
                }
                return Err(io::Error::other(format!(
                    "remote PTY closed before natural exit: {}",
                    frame.reason
                )));
            }
            _ => {}
        }
    }
}

fn run_pty_client_tty_loop(
    transport: &mut crate::control_transport::ControlTransport,
    ready: PtyReadyFrame,
    fail_on_nonzero_exit: bool,
    _raw_guard: Option<LocalRawTerminalGuard>,
    resize_writer: Option<crate::control_transport::ControlTransportWriter>,
) -> io::Result<()> {
    let resize_stop = Arc::new(AtomicBool::new(false));
    let mut resize_thread = match resize_writer {
        Some(writer) => Some(spawn_resize_publisher(
            writer,
            ready.session_id.clone(),
            Arc::clone(&resize_stop),
        )),
        None => None,
    };

    transport.set_read_timeout(Some(Duration::from_millis(25)))?;
    let result = run_pty_client_tty_loop_inner(transport, ready, fail_on_nonzero_exit);
    let restore_result = transport.set_read_timeout(None);
    resize_stop.store(true, Ordering::Relaxed);
    let resize_result = join_resize_publisher(&mut resize_thread);

    match (result, restore_result, resize_result) {
        (Ok(()), Ok(()), Ok(())) => Ok(()),
        (Err(err), _, _) => Err(err),
        (Ok(()), Err(err), _) => Err(err),
        (Ok(()), Ok(()), Err(err)) => Err(err),
    }
}

fn run_pty_client_tty_loop_inner(
    transport: &mut crate::control_transport::ControlTransport,
    ready: PtyReadyFrame,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buffer = [0_u8; 4096];

    loop {
        match transport.read_message() {
            Ok(Some(message)) => match ControlFrame::parse_inbound_result_message(&message)? {
                ControlFrame::PtyOutput(frame) if frame.session_id == ready.session_id => {
                    let bytes = BASE64_STANDARD
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
                    if fail_on_nonzero_exit && frame.exit_code != 0 {
                        return Err(io::Error::other(format!(
                            "remote PTY exited with code {}",
                            frame.exit_code
                        )));
                    }
                    return Ok(());
                }
                ControlFrame::PtyClosed(frame) if frame.session_id == ready.session_id => {
                    return Err(io::Error::other(format!(
                        "remote PTY closed before natural exit: {}",
                        frame.reason
                    )));
                }
                _ => {}
            },
            Ok(None) => return Ok(()),
            Err(err) if is_timeout_like(&err) => {}
            Err(err) => return Err(err),
        }

        match stdin.read(&mut buffer) {
            Ok(0) => {}
            Ok(len) => {
                let frame = PtyStdinFrame {
                    session_id: ready.session_id.clone(),
                    data: BASE64_STANDARD.encode(&buffer[..len]),
                };
                transport.write_message(&frame.to_wire_message())?;
            }
            Err(err) if is_timeout_like(&err) => {}
            Err(err) => return Err(err),
        }
    }
}

pub fn render_pty_resize_line(session_id: &str, cols: u16, rows: u16) -> io::Result<String> {
    if session_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PTY resize 的 session id 不能为空",
        ));
    }

    if cols == 0 || rows == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PTY resize 的 cols / rows 必须大于 0",
        ));
    }

    Ok(PtyResizeFrame {
        session_id: session_id.to_owned(),
        cols,
        rows,
    }
    .to_wire_message())
}

fn spawn_resize_publisher(
    mut writer: crate::control_transport::ControlTransportWriter,
    session_id: String,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut last_size = default_terminal_size();
        let (cols, rows) = last_size;
        writer.write_message(&render_pty_resize_line(&session_id, cols, rows)?)?;

        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            let size = default_terminal_size();
            if size == last_size {
                continue;
            }

            last_size = size;
            let (cols, rows) = size;
            writer.write_message(&render_pty_resize_line(&session_id, cols, rows)?)?;
        }

        Ok(())
    })
}

fn join_resize_publisher(
    resize_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let Some(handle) = resize_thread.take() else {
        return Ok(());
    };

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("PTY resize publisher thread panicked")),
    }
}

#[cfg(unix)]
pub fn default_terminal_size() -> (u16, u16) {
    current_terminal_size().unwrap_or((80, 24))
}

#[cfg(unix)]
pub fn current_terminal_size() -> Option<(u16, u16)> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    terminal_size_from_fd(tty.as_raw_fd())
}

#[cfg(unix)]
fn terminal_size_from_fd(fd: c_int) -> Option<(u16, u16)> {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const TIOCGWINSZ_IOCTL: c_ulong = 0x40087468;
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    const TIOCGWINSZ_IOCTL: c_ulong = 0x5413;

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    let mut winsize = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    // `ioctl(TIOCGWINSZ)` 只读取当前终端尺寸。
    // 失败或返回 0 尺寸时继续走保守 fallback,不影响非 TTY / CI 场景。
    let result = unsafe { ioctl(fd, TIOCGWINSZ_IOCTL, &mut winsize) };
    if result != 0 || winsize.ws_col == 0 || winsize.ws_row == 0 {
        return None;
    }

    Some((winsize.ws_col, winsize.ws_row))
}

#[cfg(not(unix))]
pub fn default_terminal_size() -> (u16, u16) {
    (80, 24)
}

#[cfg(unix)]
pub struct LocalRawTerminalGuard {
    tty: File,
    original: Termios,
}

#[cfg(unix)]
impl LocalRawTerminalGuard {
    pub fn enter_if_tty() -> io::Result<Option<Self>> {
        if !std::io::IsTerminal::is_terminal(&io::stdin())
            || !std::io::IsTerminal::is_terminal(&io::stdout())
        {
            return Ok(None);
        }

        let tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let fd = tty.as_raw_fd();
        let original = Termios::from_fd(fd)?;
        let mut raw = original;
        // 这里要接近 `cfmakeraw`,而不是只关 canonical。
        // 否则 macOS 终端会继续把 Enter 的 `\r` 翻译成 `\n`,
        // `codex` 这类远端 TUI 就可能把回车当成无效输入。
        raw.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON | INPCK);
        raw.c_oflag &= !OPOST;
        raw.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
        raw.c_cflag &= !(CSIZE | PARENB);
        raw.c_cflag |= CS8;
        raw.c_cc[VMIN] = 0;
        raw.c_cc[VTIME] = 1;
        tcsetattr(fd, TCSANOW, &raw)?;
        Ok(Some(Self { tty, original }))
    }
}

#[cfg(unix)]
impl Drop for LocalRawTerminalGuard {
    fn drop(&mut self) {
        let _ = tcsetattr(self.tty.as_raw_fd(), TCSANOW, &self.original);
    }
}

#[cfg(not(unix))]
pub struct LocalRawTerminalGuard;

#[cfg(not(unix))]
impl LocalRawTerminalGuard {
    pub fn enter_if_tty() -> io::Result<Option<Self>> {
        Ok(None)
    }
}

/// 启动远端 PTY,并用传入的收发函数完成 frame bridge。
#[cfg(unix)]
#[allow(dead_code)]
pub fn run_pty_server_loop<R, S>(
    request: PtyOpenRequest,
    mut recv_message: R,
    sender: S,
) -> io::Result<()>
where
    R: FnMut() -> io::Result<Option<String>>,
    S: PtyFrameSender,
{
    let session_id = uuid::Uuid::new_v4().to_string();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: request.rows,
            cols: request.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(to_io_error)?;

    let mut command = CommandBuilder::new(&request.cmd);
    for arg in &request.args {
        command.arg(arg);
    }

    let mut child = pair.slave.spawn_command(command).map_err(to_io_error)?;
    drop(pair.slave);

    let mut pty_writer = pair.master.take_writer().map_err(to_io_error)?;
    let mut pty_reader = pair.master.try_clone_reader().map_err(to_io_error)?;
    let close_handle = Arc::new(Mutex::new(child.clone_killer()));
    let terminal_reason = Arc::new(Mutex::new(None::<String>));
    let (placeholder_tx, _placeholder_rx) = mpsc::channel::<PtyRuntimeCommand>();
    register_active_pty_session(
        &session_id,
        ActivePtySession {
            close_handle: Arc::clone(&close_handle),
            terminal_reason: Arc::clone(&terminal_reason),
            input_tx: placeholder_tx,
            attached_control_session_id: Arc::new(Mutex::new(Some(session_id.clone()))),
        },
    )?;

    sender.send_frame(&ControlFrame::PtyReady(PtyReadyFrame {
        session_id: session_id.clone(),
        cols: request.cols,
        rows: request.rows,
    }))?;

    let output_sender = sender.clone();
    let output_session_id = session_id.clone();
    let output_thread = thread::spawn(move || -> io::Result<()> {
        let mut buffer = [0_u8; 8192];
        loop {
            match pty_reader.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(len) => {
                    let data = BASE64_STANDARD.encode(&buffer[..len]);
                    output_sender.send_frame(&ControlFrame::PtyOutput(PtyOutputFrame {
                        session_id: output_session_id.clone(),
                        data,
                    }))?;
                }
                Err(err) => return Err(err),
            }
        }
    });

    let exit_session_id = session_id.clone();
    let (exit_tx, exit_rx) = mpsc::channel::<i32>();
    let wait_thread = thread::spawn(move || -> io::Result<()> {
        let status = child.wait().map_err(to_io_error)?;
        unregister_active_pty_session(&exit_session_id);
        let _ = exit_tx.send(status.exit_code() as i32);
        Ok(())
    });
    let mut terminal_outcome = None::<PtyTerminalOutcome>;

    loop {
        match exit_rx.try_recv() {
            Ok(exit_code) => {
                drop(pty_writer);
                let _ = output_thread.join();
                let forced_reason = terminal_reason
                    .lock()
                    .ok()
                    .and_then(|reason| reason.clone());
                let outcome = terminal_outcome
                    .clone()
                    .or_else(|| forced_reason.map(|reason| PtyTerminalOutcome::Closed { reason }))
                    .unwrap_or(PtyTerminalOutcome::ProcessExit { exit_code });
                send_terminal_outcome(&sender, &session_id, outcome)?;
                let _ = wait_thread.join();
                return Ok(());
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }

        let message = match recv_message() {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(err) if is_timeout_like(&err) => continue,
            Err(err) => return Err(err),
        };

        if let Some(frame) = PtyStdinFrame::parse_wire_message(&message)? {
            if frame.session_id == session_id {
                let bytes = BASE64_STANDARD
                    .decode(frame.data.as_bytes())
                    .map_err(|err| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("@pty-stdin base64 数据无法解码: {err}"),
                        )
                    })?;
                pty_writer.write_all(&bytes)?;
                pty_writer.flush()?;
            }
            continue;
        }

        if let Some(frame) = PtyResizeFrame::parse_wire_message(&message)? {
            if frame.session_id == session_id {
                pair.master
                    .resize(PtySize {
                        rows: frame.rows,
                        cols: frame.cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(to_io_error)?;
            }
            continue;
        }

        if should_close_pty_session(&message, &session_id)? {
            terminal_outcome = Some(PtyTerminalOutcome::Closed {
                reason: "force_close".to_owned(),
            });
            let _ = kill_pty_child(&close_handle);
            continue;
        }

        // PTY streaming 期间不再解析普通 control line。
        // 非 PTY frame 的输入按字面内容写入远端 PTY,保证 `@script` 不会被误执行。
        pty_writer.write_all(message.as_bytes())?;
        pty_writer.write_all(b"\n")?;
        pty_writer.flush()?;
    }

    drop(pty_writer);
    if terminal_outcome.is_none() {
        match exit_rx.recv_timeout(Duration::from_millis(150)) {
            Ok(exit_code) => {
                terminal_outcome = Some(PtyTerminalOutcome::ProcessExit { exit_code });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                terminal_outcome = Some(PtyTerminalOutcome::Closed {
                    reason: "transport_lost".to_owned(),
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                terminal_outcome = Some(PtyTerminalOutcome::Closed {
                    reason: "transport_lost".to_owned(),
                });
            }
        }
    }
    let _ = kill_pty_child(&close_handle);
    unregister_active_pty_session(&session_id);
    let _ = wait_thread.join();
    let _ = output_thread.join();
    send_terminal_outcome(
        &sender,
        &session_id,
        terminal_outcome.unwrap_or(PtyTerminalOutcome::Closed {
            reason: "transport_lost".to_owned(),
        }),
    )?;
    Ok(())
}

#[cfg(not(unix))]
pub fn run_pty_server_loop<R, S>(
    _request: PtyOpenRequest,
    _recv_message: R,
    _sender: S,
) -> io::Result<()>
where
    R: FnMut() -> io::Result<Option<String>>,
    S: PtyFrameSender,
{
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前平台暂不支持 PTY control session",
    ))
}

#[cfg(unix)]
fn active_pty_sessions() -> &'static Mutex<HashMap<String, ActivePtySession>> {
    ACTIVE_PTY_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(unix)]
fn register_active_pty_session(session_id: &str, session: ActivePtySession) -> io::Result<()> {
    let mut sessions = active_pty_sessions()
        .lock()
        .map_err(|_| io::Error::other("PTY session registry lock poisoned"))?;
    sessions.insert(session_id.to_owned(), session);
    Ok(())
}

#[cfg(unix)]
fn unregister_active_pty_session(session_id: &str) {
    let Some(sessions) = ACTIVE_PTY_SESSIONS.get() else {
        return;
    };
    if let Ok(mut sessions) = sessions.lock() {
        sessions.remove(session_id);
    }
}

#[cfg(unix)]
fn kill_pty_child(handle: &PtyCloseHandle) -> io::Result<()> {
    let mut killer = handle
        .lock()
        .map_err(|_| io::Error::other("PTY close handle lock poisoned"))?;
    killer.kill().map_err(to_io_error)
}

pub fn should_close_pty_session(message: &str, session_id: &str) -> io::Result<bool> {
    match parse_control_line(message) {
        Ok(ControlParseResult::Control(crate::control_protocol::ControlRequest {
            command: ControlCommand::PtyClose(request),
            ..
        })) => Ok(request.session_id == session_id),
        _ => Ok(false),
    }
}

fn current_utc_timestamp_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

#[allow(dead_code)]
fn send_terminal_outcome<S: PtyFrameSender>(
    sender: &S,
    session_id: &str,
    outcome: PtyTerminalOutcome,
) -> io::Result<()> {
    match outcome {
        PtyTerminalOutcome::ProcessExit { exit_code } => {
            log::info!(
                "PTY terminal outcome: session_id={session_id}, frame=@pty-exit, exit_code={exit_code}"
            );
            sender.send_frame(&ControlFrame::PtyExit(PtyExitFrame {
                session_id: session_id.to_owned(),
                exit_code,
                reason: "process_exit".to_owned(),
                ended_at: current_utc_timestamp_string(),
            }))?;
            log::info!("PTY terminal frame sent: session_id={session_id}, frame=@pty-exit");
            Ok(())
        }
        PtyTerminalOutcome::Closed { reason } => {
            log::info!(
                "PTY terminal outcome: session_id={session_id}, frame=@pty-closed, reason={reason}"
            );
            sender.send_frame(&ControlFrame::PtyClosed(PtyClosedFrame {
                session_id: session_id.to_owned(),
                reason,
                ended_at: current_utc_timestamp_string(),
            }))?;
            log::info!("PTY terminal frame sent: session_id={session_id}, frame=@pty-closed");
            Ok(())
        }
    }
}

fn send_terminal_outcome_to_channel(
    sender: &mpsc::Sender<ControlFrame>,
    session_id: &str,
    outcome: PtyTerminalOutcome,
) -> io::Result<()> {
    match outcome {
        PtyTerminalOutcome::ProcessExit { exit_code } => sender
            .send(ControlFrame::PtyExit(PtyExitFrame {
                session_id: session_id.to_owned(),
                exit_code,
                reason: "process_exit".to_owned(),
                ended_at: current_utc_timestamp_string(),
            }))
            .map_err(|_| io::Error::other("PTY frame channel closed before @pty-exit")),
        PtyTerminalOutcome::Closed { reason } => sender
            .send(ControlFrame::PtyClosed(PtyClosedFrame {
                session_id: session_id.to_owned(),
                reason,
                ended_at: current_utc_timestamp_string(),
            }))
            .map_err(|_| io::Error::other("PTY frame channel closed before @pty-closed")),
    }
}

fn is_timeout_like(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn escape_json_string(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_protocol::{ControlRequest, PtyCloseRequest};

    #[test]
    fn render_pty_open_line_should_roundtrip_through_protocol_parser() {
        let argv = vec![
            "codex".to_owned(),
            "--profile".to_owned(),
            "fast profile".to_owned(),
        ];

        let line = render_pty_open_line(&argv, 120, 40).expect("open line should render from argv");
        let parsed = parse_pty_open_request(&line)
            .expect("rendered open line should parse")
            .expect("rendered line should be a pty open request");

        assert_eq!(
            parsed,
            PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec!["--profile".to_owned(), "fast profile".to_owned()],
                cols: 120,
                rows: 40,
            }
        );
    }

    #[test]
    fn parse_pty_open_request_should_support_string_shorthand() {
        let parsed = parse_pty_open_request(r#"@pty:"codex""#)
            .expect("string shorthand should parse")
            .expect("string shorthand should be a pty open request");

        assert_eq!(
            parsed,
            PtyOpenRequest {
                cmd: "codex".to_owned(),
                args: vec![],
                cols: 80,
                rows: 24,
            }
        );
    }

    #[test]
    fn parse_pty_open_request_should_not_claim_pty_stream_frames() {
        let frame = PtyStdinFrame {
            session_id: "session-1".to_owned(),
            data: "QUJD".to_owned(),
        };

        assert!(
            parse_pty_open_request(&frame.to_wire_message())
                .expect("stdin frame should not be parsed as a malformed pty open request")
                .is_none(),
            "@pty-stdin 是 PTY 数据帧,不能被 @pty open helper 误拦截"
        );
    }

    #[test]
    fn render_pty_close_line_should_roundtrip_through_protocol_parser() {
        let line = render_pty_close_line("session-1").expect("close line should render");

        assert_eq!(
            parse_control_line(&line).expect("close line should parse"),
            ControlParseResult::Control(ControlRequest {
                request_id: None,
                command: ControlCommand::PtyClose(PtyCloseRequest {
                    session_id: "session-1".to_owned(),
                }),
            })
        );
    }

    #[test]
    fn close_active_pty_session_should_report_unknown_session() {
        let closed =
            close_active_pty_session("missing-session").expect("unknown close should not fail");
        assert!(
            !closed,
            "unknown session close should report false instead of pretending success"
        );
    }
}
