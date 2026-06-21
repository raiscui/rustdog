use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use clap::ValueEnum;
use std::{
    char,
    io::{self, stdout, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::{self, JoinHandle},
};

use crate::{
    control_actions::{
        build_shell_command, shell_program_name, ControlActionExecutor, SystemControlActionExecutor,
    },
    control_client_input::ControlStdinAction,
    control_core::parse_and_execute_control_line,
    control_display::ControlResponseDisplay,
    control_frames::ControlExecutionOutcome,
    control_session::{
        route_line_control_result_message, ControlPeerLifecycleDecision, ControlPeerSession,
    },
    control_transport::{accept_websocket_stream, ControlTransport},
};

#[cfg(test)]
use crate::control_session::LineWriteFrameSink;

const CONTROL_DETECT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

enum JsonAgentRequest {
    Command(String),
    Ping,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ControlSessionMode {
    Disconnected,
    JsonAgent,
    LineControl,
}

/// shell 会话的 I/O 语义。
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ShellMode {
    /// 面向人类交互的终端模式。
    Interactive,
    /// 纯 stdio 字节流模式,适合普通程序桥接。
    Stdio,
    /// JSON 风格的命令请求/响应模式。
    Agent,
    /// 显式控制协议模式,按行解释 `@...` / `@@...`。
    Control,
}

/// 连接到远端并把本地 shell 挂到该 socket 上。
pub fn connect_and_run_shell(
    host: &str,
    port: u16,
    shell: &str,
    mode: ShellMode,
) -> io::Result<()> {
    let stream = TcpStream::connect((host, port))?;
    run_shell_over_stream(stream, shell, mode)
}

/// 以本地 stdio 作为控制通道,把请求直接转发到远端 daemon inbound。
pub fn control_remote(host: &str, port: u16) -> io::Result<()> {
    let transport = ControlTransport::connect_tcp(host, port)?;
    control_transport_stream(transport)
}

/// 以 TCP control lane 打开远端 PTY 会话。
pub fn control_remote_pty(host: &str, port: u16, argv: &[String]) -> io::Result<()> {
    let (cols, rows) = crate::pty_control::default_terminal_size();
    let open_line = crate::pty_control::render_pty_open_line(argv, cols, rows)?;
    let mut transport = ControlTransport::connect_tcp(host, port)?;
    crate::pty_control::run_pty_client_transport(&mut transport, open_line, true)
}

pub fn control_remote_attach(
    host: &str,
    port: u16,
    session_id: &str,
    cols: u16,
    rows: u16,
) -> io::Result<()> {
    let attach_line = crate::pty_control::render_pty_attach_line(session_id, cols, rows)?;
    let mut transport = ControlTransport::connect_tcp(host, port)?;
    crate::pty_control::run_pty_attach_client_transport(&mut transport, attach_line, true)
}

/// 以 websocket client 形式把本地 stdio 挂到远端 control lane。
pub fn control_remote_url(url: &str) -> io::Result<()> {
    let transport = ControlTransport::connect_websocket(url)?;
    control_transport_stream(transport)
}

/// 以 websocket control lane 打开远端 PTY 会话。
pub fn control_remote_url_pty(url: &str, argv: &[String]) -> io::Result<()> {
    let (cols, rows) = crate::pty_control::default_terminal_size();
    let open_line = crate::pty_control::render_pty_open_line(argv, cols, rows)?;
    let mut transport = ControlTransport::connect_websocket(url)?;
    crate::pty_control::run_pty_client_transport(&mut transport, open_line, true)
}

pub fn control_remote_url_attach(
    url: &str,
    session_id: &str,
    cols: u16,
    rows: u16,
) -> io::Result<()> {
    let attach_line = crate::pty_control::render_pty_attach_line(session_id, cols, rows)?;
    let mut transport = ControlTransport::connect_websocket(url)?;
    crate::pty_control::run_pty_attach_client_transport(&mut transport, attach_line, true)
}

/// 以 Zenoh router/client control profile 把本地 stdio 挂到远端 control lane。
///
/// 默认走 Zenoh router scouting / autodiscovery。
/// `--entry-point` 只作为无法自动发现时的显式 fallback。
pub fn control_remote_zenoh(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
) -> io::Result<()> {
    let namespace =
        crate::zenoh_identity::resolve_namespace(namespace.as_deref(), target_name.as_deref())?;

    if let Some(target_name) = target_name.as_deref() {
        crate::zenoh_identity::validate_daemon_name(target_name)?;
    }

    if entry_point.iter().any(|item| item.trim().is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--entry-point 不能包含空字符串",
        ));
    }

    crate::zenoh_control::run_client_control(namespace.to_owned(), target_name, entry_point, 3_000)
}

pub fn control_remote_zenoh_pty(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    argv: &[String],
) -> io::Result<()> {
    let namespace =
        crate::zenoh_identity::resolve_namespace(namespace.as_deref(), target_name.as_deref())?;

    if let Some(target_name) = target_name.as_deref() {
        crate::zenoh_identity::validate_daemon_name(target_name)?;
    }

    let (cols, rows) = crate::pty_control::default_terminal_size();
    let open_line = crate::pty_control::render_pty_open_line(argv, cols, rows)?;
    crate::zenoh_control::run_client_pty_control(
        namespace.to_owned(),
        target_name,
        entry_point,
        3_000,
        open_line,
    )
}

pub fn control_remote_zenoh_attach(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    session_id: &str,
    cols: u16,
    rows: u16,
) -> io::Result<()> {
    let namespace =
        crate::zenoh_identity::resolve_namespace(namespace.as_deref(), target_name.as_deref())?;

    if let Some(target_name) = target_name.as_deref() {
        crate::zenoh_identity::validate_daemon_name(target_name)?;
    }

    let attach_line = crate::pty_control::render_pty_attach_line(session_id, cols, rows)?;
    crate::zenoh_control::run_client_pty_attach(
        namespace.to_owned(),
        target_name,
        entry_point,
        3_000,
        attach_line,
    )
}

/// 把已经建立好的 socket 挂到本地 shell 上。
pub fn run_shell_over_stream(stream: TcpStream, shell: &str, mode: ShellMode) -> io::Result<()> {
    match mode {
        ShellMode::Interactive => run_interactive_shell_over_stream(stream, shell),
        ShellMode::Stdio => run_stdio_shell_over_stream(stream, shell),
        ShellMode::Agent => run_agent_shell_over_stream(stream, shell),
        ShellMode::Control => run_control_receiver_over_stream(stream, shell),
    }
}

fn control_transport_stream(mut transport: ControlTransport) -> io::Result<()> {
    let display = ControlResponseDisplay::from_stdio();
    let mut output = stdout().lock();
    let save_dir = default_savefile_directory()?;

    loop {
        let mut pty_open_line = None::<String>;

        crate::control_client_input::for_each_control_stdin_line(|message| {
            if message.is_empty() {
                return Ok(ControlStdinAction::Continue);
            }

            if crate::pty_control::parse_pty_open_request(&message)?.is_some() {
                pty_open_line = Some(message);
                return Ok(ControlStdinAction::Break);
            }

            transport.write_message(&message)?;
            if is_json_agent_client_request(&message) {
                receive_single_raw_control_message(&mut transport, &mut output)?;
            } else {
                receive_control_result_frames(&mut transport, &mut output, &save_dir, display)?;
            }

            Ok(ControlStdinAction::Continue)
        })?;

        let Some(open_line) = pty_open_line else {
            break;
        };

        crate::pty_control::run_pty_client_transport(&mut transport, open_line, false)?;
    }

    transport.close()?;
    Ok(())
}

fn is_json_agent_client_request(message: &str) -> bool {
    message.trim_start().starts_with('{')
}

fn receive_single_raw_control_message<W: Write>(
    transport: &mut ControlTransport,
    output: &mut W,
) -> io::Result<()> {
    if let Some(message) = transport.read_message()? {
        writeln!(output, "{message}")?;
        output.flush()?;
    }

    Ok(())
}

/// 复用一条已建好的 ControlTransport 串行执行一组 line-control 请求。
///
/// 这是 `rdog control <target> @<line> [@<line> ...]` 多 line 入口的核心:
/// 一次性发一组 `@<line>`,每条都走完整的 frame 收口循环
/// (能正确处理 `@savefile` 多 frame 场景,如 `@screenshot`),
/// 共享同一条 TCP / WebSocket 连接,任一行失败整组退出。
///
/// 不适用 `--pty-close` / `--pty-detach` 这种简单 PTY 帧——那种场景
/// 继续走 `send_single_control_line_*` 单帧 exchange 路径。
pub fn run_line_control_lines(
    transport: &mut ControlTransport,
    lines: &[String],
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let display = ControlResponseDisplay::from_stdio();
    let mut output = stdout().lock();
    let save_dir = default_savefile_directory()?;

    for (idx, line) in lines.iter().enumerate() {
        transport.write_message(line)?;
        // 当前 one-shot 入口都只发 line-control 帧,不走 JSON agent。
        // 但保持 `control_transport_stream` 同源逻辑,避免出现新行为漂移。
        if line.trim_start().starts_with('{') {
            receive_single_raw_control_message(transport, &mut output)?;
        } else {
            match receive_control_result_frames(transport, &mut output, &save_dir, display) {
                Ok(()) => {}
                Err(err) => {
                    log::warn!(
                        "control multi-line request failed at line index {idx} (line={line}): {err}"
                    );
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

fn run_interactive_shell_over_stream(stream: TcpStream, shell: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        return crate::unixshell::shell_from_stream(stream, shell);
    }

    #[cfg(windows)]
    {
        return crate::winshell::shell_from_stream(stream, shell);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (stream, shell);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "当前平台不支持交互 shell 模式",
        ))
    }
}

fn run_stdio_shell_over_stream(stream: TcpStream, shell: &str) -> io::Result<()> {
    let mut child = build_stdio_shell_command(shell)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stdin"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stdout"))?;
    let child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stderr"))?;

    let socket_to_child = pipe_thread(stream.try_clone()?, child_stdin, None);
    let stdout_to_socket = pipe_thread(
        child_stdout,
        stream.try_clone()?,
        Some("stdio shell stdout closed"),
    );
    let stderr_to_socket = pipe_thread(child_stderr, stream, Some("stdio shell stderr closed"));

    let status = child.wait()?;

    join_io_thread(stdout_to_socket)?;
    join_io_thread(stderr_to_socket)?;

    if socket_to_child.is_finished() {
        join_io_thread(socket_to_child)?;
    }

    log::warn!("Stdio shell exited with status {status}");

    Ok(())
}

fn run_agent_shell_over_stream(stream: TcpStream, shell: &str) -> io::Result<()> {
    let mut transport = ControlTransport::from_tcp_stream(stream)?;
    let Some(first_message) = transport.read_message()? else {
        return Ok(());
    };
    run_agent_shell_over_transport(transport, shell, first_message)
}

pub fn run_control_receiver_over_stream(stream: TcpStream, shell: &str) -> io::Result<()> {
    match detect_control_session_mode(&stream)? {
        ControlSessionMode::Disconnected => Ok(()),
        ControlSessionMode::JsonAgent => run_agent_shell_over_stream(stream, shell),
        ControlSessionMode::LineControl => {
            let executor = SystemControlActionExecutor::default();
            let transport = ControlTransport::from_tcp_stream(stream)?;
            run_control_receiver_with_transport(transport, shell, &executor)
        }
    }
}

pub fn run_control_receiver_over_websocket_stream(
    stream: TcpStream,
    shell: &str,
) -> io::Result<()> {
    let transport = accept_websocket_stream(stream)?;
    let executor = SystemControlActionExecutor::default();
    run_control_receiver_with_transport(transport, shell, &executor)
}

fn run_control_receiver_with_transport<E: ControlActionExecutor>(
    mut transport: ControlTransport,
    shell: &str,
    executor: &E,
) -> io::Result<()> {
    let Some(first_message): Option<String> = transport.read_message()? else {
        return Ok(());
    };

    match detect_transport_session_mode(&first_message) {
        ControlSessionMode::JsonAgent => {
            run_agent_shell_over_transport(transport, shell, first_message)
        }
        ControlSessionMode::LineControl => {
            if let Some(request) = crate::pty_control::parse_pty_open_request(&first_message)? {
                run_pty_receiver_with_transport(&mut transport, request)?;
                return run_control_receiver_messages(transport, shell, executor, None);
            }
            run_control_receiver_messages(transport, shell, executor, Some(first_message))
        }
        ControlSessionMode::Disconnected => Ok(()),
    }
}

#[cfg(test)]
fn run_control_receiver_with_executor<E: ControlActionExecutor>(
    stream: TcpStream,
    shell: &str,
    executor: &E,
) -> io::Result<()> {
    let reader_stream = stream.try_clone()?;
    let mut reader = std::io::BufReader::new(reader_stream);
    let mut writer = stream;

    while let Some(line) = read_control_request_line(&mut reader)? {
        let outcome = parse_and_execute_control_line(&line, shell, executor);
        if outcome.outbound_frames.is_empty() {
            continue;
        }

        write_outcome_to_line_writer(&mut writer, outcome)?;
    }

    Ok(())
}

fn run_agent_shell_over_transport(
    mut transport: ControlTransport,
    shell: &str,
    first_message: String,
) -> io::Result<()> {
    let mut pending = Some(first_message);

    loop {
        let message = match pending.take() {
            Some(message) => message,
            None => match transport.read_message()? {
                Some(message) => message,
                None => return Ok(()),
            },
        };

        match parse_json_request_object(&message)? {
            JsonAgentRequest::Command(command) => {
                let output = build_shell_command(shell, &command).output()?;
                transport.write_message(
                    format_json_agent_response(
                        output.status.code().unwrap_or(-1),
                        &output.stdout,
                        &output.stderr,
                    )
                    .as_str(),
                )?;
            }
            JsonAgentRequest::Ping => transport.write_message("{\"type\":\"pong\"}")?,
        }
    }
}

fn run_control_receiver_messages<E: ControlActionExecutor>(
    mut transport: ControlTransport,
    shell: &str,
    executor: &E,
    first_message: Option<String>,
) -> io::Result<()> {
    let mut pending = first_message;

    loop {
        let line = match pending.take() {
            Some(line) => line,
            None => match transport.read_message()? {
                Some(line) => line,
                None => return Ok(()),
            },
        };

        if let Some(request) = crate::pty_control::parse_pty_open_request(&line)? {
            run_pty_receiver_with_transport(&mut transport, request)?;
            continue;
        }

        if let Some(request) = crate::pty_control::parse_pty_attach_request(&line)? {
            run_pty_attach_receiver_with_transport(&mut transport, request)?;
            continue;
        }

        let outcome = parse_and_execute_control_line(&line, shell, executor);
        if outcome.outbound_frames.is_empty() {
            continue;
        }

        write_outcome_to_transport(&mut transport, outcome)?;
    }
}

fn run_pty_receiver_with_transport(
    transport: &mut ControlTransport,
    request: crate::control_protocol::PtyOpenRequest,
) -> io::Result<()> {
    let mut session = crate::pty_control::open_attached_pty_session(request)?;
    bridge_attached_pty_session(transport, &mut session)
}

fn run_pty_attach_receiver_with_transport(
    transport: &mut ControlTransport,
    request: crate::control_protocol::PtyAttachRequest,
) -> io::Result<()> {
    let Some(mut session) = crate::pty_control::attach_active_pty_session(request)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "PTY attach 目标 session 不存在",
        ));
    };
    bridge_attached_pty_session(transport, &mut session)
}

fn bridge_attached_pty_session(
    transport: &mut ControlTransport,
    session: &mut crate::pty_control::AttachedPtySession,
) -> io::Result<()> {
    transport.set_read_timeout(Some(std::time::Duration::from_millis(25)))?;
    let result = bridge_attached_pty_session_inner(transport, session);
    let restore_result = transport.set_read_timeout(None);
    match (result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

fn bridge_attached_pty_session_inner(
    transport: &mut ControlTransport,
    session: &mut crate::pty_control::AttachedPtySession,
) -> io::Result<()> {
    loop {
        while let Ok(frame) = session.try_recv_frame() {
            transport.write_message(frame.to_wire_message().as_str())?;
            if matches!(
                ControlPeerSession::lifecycle_decision_for_frame(&frame),
                ControlPeerLifecycleDecision::TerminalComplete { .. }
                    | ControlPeerLifecycleDecision::Detached { .. }
            ) {
                return Ok(());
            }
        }

        match transport.read_message() {
            Ok(Some(message)) => {
                if let Some(frame) =
                    crate::control_frames::PtyStdinFrame::parse_wire_message(&message)?
                {
                    if frame.session_id == session.session_id() {
                        let bytes =
                            BASE64_STANDARD
                                .decode(frame.data.as_bytes())
                                .map_err(|err| {
                                    io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!("@pty-stdin base64 数据无法解码: {err}"),
                                    )
                                })?;
                        session.send_stdin_bytes(bytes)?;
                        continue;
                    }
                }

                if let Some(frame) =
                    crate::control_frames::PtyResizeFrame::parse_wire_message(&message)?
                {
                    if frame.session_id == session.session_id() {
                        session.resize(frame.cols, frame.rows)?;
                        continue;
                    }
                }

                if crate::pty_control::should_close_pty_session(&message, session.session_id())? {
                    session.close("force_close")?;
                    continue;
                }

                if matches!(
                    crate::control_protocol::parse_control_line(&message),
                    Ok(crate::control_protocol::ControlParseResult::Control(
                        crate::control_protocol::ControlRequest {
                            command: crate::control_protocol::ControlCommand::PtyDetach(_),
                            ..
                        }
                    ))
                ) {
                    session.detach("owner_detach")?;
                    continue;
                }

                session.send_stdin_bytes({
                    let mut bytes = message.into_bytes();
                    bytes.push(b'\n');
                    bytes
                })?;
            }
            Ok(None) => {
                session.close("control_disconnect")?;
                return Ok(());
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
fn write_outcome_to_line_writer<W: Write>(
    writer: &mut W,
    outcome: ControlExecutionOutcome,
) -> io::Result<()> {
    let mut session = ControlPeerSession::new("line-writer-test");
    let mut sink = LineWriteFrameSink::new(writer);
    session.dispatch_outcome(outcome, &mut sink)?;
    Ok(())
}

fn write_outcome_to_transport(
    transport: &mut ControlTransport,
    outcome: ControlExecutionOutcome,
) -> io::Result<()> {
    let mut session = ControlPeerSession::new("socket-control");
    session.dispatch_outcome(outcome, transport)?;
    Ok(())
}

fn default_savefile_directory() -> io::Result<PathBuf> {
    Ok(std::env::current_dir()?.join("rdog_downloads"))
}

fn receive_control_result_frames<W: Write>(
    transport: &mut ControlTransport,
    output: &mut W,
    save_dir: &Path,
    display: ControlResponseDisplay,
) -> io::Result<()> {
    loop {
        let Some(message) = transport.read_message()? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control connection 在收到结果前就关闭了",
            ));
        };

        route_line_control_result_message(&message, output, display, |frame| {
            frame.save_to_directory(save_dir)
        })?;

        if message.trim_start().starts_with("@response ") {
            return Ok(());
        }
    }
}

fn pipe_thread<R, W>(
    mut reader: R,
    mut writer: W,
    eof_message: Option<&'static str>,
) -> JoinHandle<io::Result<()>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0_u8; 1024];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    if let Some(message) = eof_message {
                        log::warn!("{message}");
                    }
                    return Ok(());
                }
                Ok(len) => writer.write_all(&buffer[..len])?,
                Err(err) => return Err(err),
            }

            writer.flush()?;
        }
    })
}

fn join_io_thread(handle: JoinHandle<io::Result<()>>) -> io::Result<()> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("shell bridge thread panicked")),
    }
}

fn build_stdio_shell_command(shell: &str) -> Command {
    let mut command = Command::new(shell);

    match shell_program_name(shell).as_deref() {
        Some("bash") => {
            // 非交互模式下禁掉 profile,避免提示符和用户本地定制噪音污染程序输出。
            command.args(["--noprofile", "--norc"]);
        }
        Some("zsh") => {
            // `-f` 会跳过 zshrc,保持字节流更干净。
            command.arg("-f");
        }
        Some("pwsh") | Some("pwsh.exe") | Some("powershell") | Some("powershell.exe") => {
            command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"]);
        }
        Some("cmd") | Some("cmd.exe") => {
            command.args(["/Q", "/D"]);
        }
        _ => {}
    }

    command
}
fn detect_control_session_mode(stream: &TcpStream) -> io::Result<ControlSessionMode> {
    let previous_timeout = stream.read_timeout()?;
    stream.set_read_timeout(Some(CONTROL_DETECT_POLL_INTERVAL))?;

    let mut buffer = [0_u8; 64];

    let result = loop {
        match stream.peek(&mut buffer) {
            Ok(0) => break ControlSessionMode::Disconnected,
            Ok(len) => {
                let bytes = &buffer[..len];
                if let Some(prefix) = bytes
                    .iter()
                    .copied()
                    .find(|byte| !byte.is_ascii_whitespace())
                {
                    match prefix {
                        b'{' => break ControlSessionMode::JsonAgent,
                        _ => break ControlSessionMode::LineControl,
                    }
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => {
                stream.set_read_timeout(previous_timeout)?;
                return Err(err);
            }
        }
    };

    stream.set_read_timeout(previous_timeout)?;
    Ok(result)
}

fn detect_transport_session_mode(message: &str) -> ControlSessionMode {
    match message.bytes().find(|byte| !byte.is_ascii_whitespace()) {
        Some(b'{') => ControlSessionMode::JsonAgent,
        Some(_) => ControlSessionMode::LineControl,
        None => ControlSessionMode::Disconnected,
    }
}

#[cfg(test)]
fn read_control_request_line<R: std::io::BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;

        if bytes_read == 0 {
            return Ok(None);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }

        return Ok(Some(line.to_owned()));
    }
}

fn format_json_agent_response(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!(
        "{{\"exit_code\":{exit_code},\"stdout\":\"{}\",\"stderr\":\"{}\"}}",
        escape_json_string(&stdout),
        escape_json_string(&stderr)
    )
}

#[cfg(test)]
fn write_response_line(writer: &mut impl Write, response_line: &str) -> io::Result<()> {
    writeln!(writer, "{response_line}")?;
    writer.flush()?;
    Ok(())
}

fn parse_json_request_object(input: &str) -> io::Result<JsonAgentRequest> {
    let bytes = input.as_bytes();
    let mut index = 0;

    skip_json_whitespace(bytes, &mut index);
    expect_json_byte(bytes, &mut index, b'{')?;

    let mut command = None::<String>;
    let mut request_type = None::<String>;

    loop {
        skip_json_whitespace(bytes, &mut index);

        if consume_json_byte(bytes, &mut index, b'}') {
            break;
        }

        let key = parse_json_string(bytes, &mut index)?;
        skip_json_whitespace(bytes, &mut index);
        expect_json_byte(bytes, &mut index, b':')?;
        skip_json_whitespace(bytes, &mut index);
        let value = parse_json_string(bytes, &mut index)?;

        match key.as_str() {
            "command" | "cmd" => command = Some(value),
            "type" | "op" => request_type = Some(value),
            _ => {}
        }

        skip_json_whitespace(bytes, &mut index);
        if consume_json_byte(bytes, &mut index, b',') {
            continue;
        }
        if consume_json_byte(bytes, &mut index, b'}') {
            break;
        }

        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid json command object: {input}"),
        ));
    }

    skip_json_whitespace(bytes, &mut index);
    if index != bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected trailing bytes in json command: {input}"),
        ));
    }

    if matches!(
        request_type.as_deref(),
        Some("ping") | Some("heartbeat") | Some("health")
    ) {
        return Ok(JsonAgentRequest::Ping);
    }

    command.map(JsonAgentRequest::Command).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "json command object must contain `command` / `cmd`, or use `type: ping`",
        )
    })
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

fn skip_json_whitespace(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && bytes[*index].is_ascii_whitespace() {
        *index += 1;
    }
}

fn consume_json_byte(bytes: &[u8], index: &mut usize, expected: u8) -> bool {
    if *index < bytes.len() && bytes[*index] == expected {
        *index += 1;
        return true;
    }
    false
}

fn expect_json_byte(bytes: &[u8], index: &mut usize, expected: u8) -> io::Result<()> {
    if consume_json_byte(bytes, index, expected) {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("expected byte `{}` in json input", expected as char),
    ))
}

fn parse_json_string(bytes: &[u8], index: &mut usize) -> io::Result<String> {
    expect_json_byte(bytes, index, b'"')?;
    let mut result = Vec::new();

    while *index < bytes.len() {
        let byte = bytes[*index];
        *index += 1;

        match byte {
            b'"' => {
                return String::from_utf8(result).map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("json string is not valid utf-8: {err}"),
                    )
                });
            }
            b'\\' => {
                let escaped = *bytes.get(*index).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unterminated escape in json string",
                    )
                })?;
                *index += 1;

                match escaped {
                    b'"' => result.push(b'"'),
                    b'\\' => result.push(b'\\'),
                    b'/' => result.push(b'/'),
                    b'b' => result.push(0x08),
                    b'f' => result.push(0x0c),
                    b'n' => result.push(b'\n'),
                    b'r' => result.push(b'\r'),
                    b't' => result.push(b'\t'),
                    b'u' => {
                        let end = *index + 4;
                        let digits = bytes.get(*index..end).ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "incomplete unicode escape in json string",
                            )
                        })?;
                        let digits = std::str::from_utf8(digits).map_err(|err| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("unicode escape is not utf-8: {err}"),
                            )
                        })?;
                        let codepoint = u16::from_str_radix(digits, 16).map_err(|err| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("invalid unicode escape `{digits}`: {err}"),
                            )
                        })?;
                        let ch = char::from_u32(codepoint as u32).ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("invalid unicode codepoint: {codepoint}"),
                            )
                        })?;
                        let mut encoded = [0_u8; 4];
                        result.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
                        *index = end;
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("unsupported escape sequence: \\{}", escaped as char),
                        ));
                    }
                }
            }
            byte => result.push(byte),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unterminated json string",
    ))
}

#[cfg(test)]
mod tests;
