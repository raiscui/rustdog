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
    control_display::{write_response_for_display, ControlResponseDisplay},
    control_frames::{ControlExecutionOutcome, ControlFrame},
    control_transport::{accept_websocket_stream, ControlTransport},
};

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
            if matches!(frame, ControlFrame::PtyExit(_) | ControlFrame::PtyClosed(_)) {
                return Ok(());
            }
            if matches!(frame, ControlFrame::PtyDetached(_)) {
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
    for frame in outcome.outbound_frames {
        write_response_line(writer, &frame.to_wire_message())?;
    }

    Ok(())
}

fn write_outcome_to_transport(
    transport: &mut ControlTransport,
    outcome: ControlExecutionOutcome,
) -> io::Result<()> {
    for frame in outcome.outbound_frames {
        transport.write_message(frame.to_wire_message().as_str())?;
    }

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

        match ControlFrame::parse_inbound_result_message(&message)? {
            ControlFrame::ResponseLine(response) => {
                write_response_for_display(output, &response, display)?;
                return Ok(());
            }
            ControlFrame::SaveFile(frame) => {
                let saved_path = frame.save_to_directory(save_dir)?;
                writeln!(output, "saved file: {}", saved_path.display())?;
                output.flush()?;
            }
            ControlFrame::PtyReady(_)
            | ControlFrame::PtyOutput(_)
            | ControlFrame::PtyExit(_)
            | ControlFrame::PtyClosed(_)
            | ControlFrame::PtyDetached(_)
            | ControlFrame::PtyAttached(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "line-control response 收到了意外 PTY frame",
                ));
            }
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
mod tests {
    use super::*;
    use crate::control_frames::SaveFileFrame;
    use crate::control_protocol::{KeyMode, KeyRequest, PasteRequestKind};
    use crate::{control_actions::ActionExecutionResult, control_protocol::ControlCommand};
    use std::{
        fs,
        net::{Shutdown, TcpListener, TcpStream},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone, Default)]
    struct FakeExecutor {
        commands: Arc<Mutex<Vec<ControlCommand>>>,
    }

    impl ControlActionExecutor for FakeExecutor {
        fn execute(
            &self,
            command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
            self.commands
                .lock()
                .expect("commands lock should work")
                .push(command.clone());

            let stdout = match command {
                ControlCommand::Key(request) => format!("KEY:{}\n", request.key).into_bytes(),
                ControlCommand::Paste(request) => match &request.kind {
                    PasteRequestKind::GlobalHotkey => b"PASTE:global-hotkey\n".to_vec(),
                    PasteRequestKind::LegacyTextInjection(payload) => {
                        format!("PASTE:{payload}\n").into_bytes()
                    }
                },
                ControlCommand::Script(payload) => format!("SCRIPT:{payload}\n").into_bytes(),
                ControlCommand::Ping => b"PONG\n".to_vec(),
                ControlCommand::Screenshot(request) => {
                    format!("SCREENSHOT:{}\n", request.quality).into_bytes()
                }
                ControlCommand::SaveFile(frame) => {
                    format!("SAVEFILE:{}\n", frame.filename).into_bytes()
                }
                ControlCommand::PtyOpen(request) => {
                    format!("PTY_OPEN:{}\n", request.cmd).into_bytes()
                }
                ControlCommand::PtyClose(request) => {
                    format!("PTY_CLOSE:{}\n", request.session_id).into_bytes()
                }
                ControlCommand::PtyDetach(request) => {
                    format!("PTY_DETACH:{}\n", request.session_id).into_bytes()
                }
                ControlCommand::PtyAttach(request) => format!(
                    "PTY_ATTACH:{}:{}x{}\n",
                    request.session_id, request.cols, request.rows
                )
                .into_bytes(),
                ControlCommand::MouseMove(request) => format!(
                    "MOUSE_MOVE:{}:{}\n",
                    request.x.unwrap_or(0),
                    request.y.unwrap_or(0)
                )
                .into_bytes(),
                ControlCommand::MouseButton(request) => {
                    format!("MOUSE_BUTTON:{}\n", request.button.as_protocol_str()).into_bytes()
                }
                ControlCommand::Click(request) => {
                    format!("CLICK:{}:{}\n", request.x, request.y).into_bytes()
                }
                ControlCommand::Drag(request) => {
                    format!("DRAG:{}:{}\n", request.from.x, request.to.x).into_bytes()
                }
                ControlCommand::Wheel(request) => {
                    format!("WHEEL:{}:{}\n", request.delta_x, request.delta_y).into_bytes()
                }
                ControlCommand::AxTree(request) => {
                    format!("AX_TREE:{}:{}\n", request.depth, request.max_elements).into_bytes()
                }
                ControlCommand::AxFind(request) => {
                    format!("AX_FIND:{}\n", request.limit).into_bytes()
                }
                ControlCommand::AxGet(request) => format!(
                    "AX_GET:{}\n",
                    request.target.id.as_deref().unwrap_or("semantic")
                )
                .into_bytes(),
                ControlCommand::AxFocus(request) => format!(
                    "AX_FOCUS:{}\n",
                    request.window_id.as_deref().unwrap_or("target")
                )
                .into_bytes(),
                ControlCommand::AxScroll(request) => format!(
                    "AX_SCROLL:{}:{}\n",
                    request.direction.as_str(),
                    request.pages
                )
                .into_bytes(),
                ControlCommand::AxAction(request) => format!(
                    "AX_ACTION:{}:{}\n",
                    request.action.protocol_str(),
                    request.target.id.as_deref().unwrap_or("semantic")
                )
                .into_bytes(),
                ControlCommand::AxPress(request) => format!(
                    "AX_PRESS:{}\n",
                    request.target.id.as_deref().unwrap_or("semantic")
                )
                .into_bytes(),
                ControlCommand::AxSetValue(request) => format!(
                    "AX_SET_VALUE:{}:{}\n",
                    request.mode.as_str(),
                    request.target.id.as_deref().unwrap_or("semantic")
                )
                .into_bytes(),
                ControlCommand::TypeText(request) => format!(
                    "TYPE_TEXT:{}:{}\n",
                    request.mode.as_str(),
                    request.target.id.as_deref().unwrap_or("semantic")
                )
                .into_bytes(),
                ControlCommand::WindowFind(request) => {
                    format!("WINDOW_FIND:{}\n", request.limit).into_bytes()
                }
                ControlCommand::WindowActivate(request) => format!(
                    "WINDOW_ACTIVATE:{}\n",
                    request
                        .target
                        .window_id
                        .as_deref()
                        .unwrap_or("query-target")
                )
                .into_bytes(),
                ControlCommand::WindowClose(request) => format!(
                    "WINDOW_CLOSE:{}:{}\n",
                    request.strategy.as_str(),
                    request
                        .target
                        .window_id
                        .as_deref()
                        .unwrap_or("query-target")
                )
                .into_bytes(),
            };

            Ok(ActionExecutionResult {
                exit_code: 0,
                stdout,
                stderr: Vec::new(),
                response_value_json: None,
            })
        }
    }

    fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = bind_test_listener();
        let port = listener
            .local_addr()
            .expect("listener should expose local addr")
            .port();

        let client = TcpStream::connect(("127.0.0.1", port))
            .expect("client should connect to test listener");
        let (server, _) = listener.accept().expect("server should accept test client");
        (client, server)
    }

    fn bind_test_listener() -> TcpListener {
        #[cfg(windows)]
        {
            const PROVIDER_INIT_ERROR: i32 = 10106;

            for _ in 0..8 {
                match TcpListener::bind(("127.0.0.1", 0)) {
                    Ok(listener) => return listener,
                    Err(err) if err.raw_os_error() == Some(PROVIDER_INIT_ERROR) => {
                        thread::sleep(std::time::Duration::from_millis(25));
                    }
                    Err(err) => panic!("ephemeral listener should bind: {err:?}"),
                }
            }

            panic!(
                "ephemeral listener should bind: Windows socket provider kept failing with 10106"
            );
        }

        #[cfg(not(windows))]
        {
            TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind")
        }
    }

    #[cfg(unix)]
    fn temp_shell_wrapper(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rdog-shell-wrapper-{name}-{}", std::process::id()));
        fs::write(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"-c\" ]; then\n  printf '%s' \"$2\"\nelse\n  printf 'unexpected-args'\nfi\n",
        )
        .expect("should write wrapper shell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)
                .expect("wrapper metadata should exist")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).expect("should mark wrapper executable");
        }

        path
    }

    fn cleanup_temp_path(path: &Path) {
        let _ = fs::remove_file(path);
    }

    fn temp_directory(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_millis();
        std::env::temp_dir().join(format!("rdog-shell-{name}-{millis}-{}", std::process::id()))
    }

    fn control_test_shell() -> &'static str {
        #[cfg(windows)]
        {
            "powershell.exe"
        }

        #[cfg(not(windows))]
        {
            "/bin/sh"
        }
    }

    fn escaped_literal_shell_case(_name: &str) -> (String, Vec<u8>, String, Option<PathBuf>) {
        #[cfg(windows)]
        {
            (
                "cmd.exe".to_owned(),
                b"@@echo ESCAPED_OK\n".to_vec(),
                "ESCAPED_OK".to_owned(),
                None,
            )
        }

        #[cfg(not(windows))]
        {
            let wrapper = temp_shell_wrapper(_name);
            (
                wrapper.to_string_lossy().to_string(),
                b"@@printf '@%s' ok\n".to_vec(),
                "@printf '@%s' ok".to_owned(),
                Some(wrapper),
            )
        }
    }

    #[test]
    fn control_receiver_should_route_built_in_commands_to_executor() {
        let (mut client, server) = connected_pair();
        let executor = FakeExecutor::default();
        let recorded = Arc::clone(&executor.commands);
        let shell = control_test_shell();

        let worker =
            thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

        client
            .write_all(br#"@key:"F11""#)
            .expect("should write control line");
        client.write_all(b"\n").expect("should finish control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read control response");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish");

        assert!(output.contains(r#"@response "KEY:F11\n""#));
        assert_eq!(
            recorded
                .lock()
                .expect("commands lock should work")
                .as_slice(),
            &[ControlCommand::Key(KeyRequest::legacy(
                "F11",
                200,
                KeyMode::PressRelease,
            ))]
        );
    }

    #[test]
    fn control_receiver_should_wrap_success_response_with_request_id() {
        let (mut client, server) = connected_pair();
        let executor = FakeExecutor::default();
        let shell = control_test_shell();
        let worker =
            thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

        client
            .write_all(br#"@key#42:"F11""#)
            .expect("should write control line with request id");
        client.write_all(b"\n").expect("should finish control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read response with request id");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish");

        assert!(output.contains(r#"@response {"id":42,"value":"KEY:F11\n"}"#));
    }

    #[test]
    fn control_receiver_should_execute_savefile_request_and_report_saved_path() {
        let (mut client, server) = connected_pair();
        let save_dir = temp_directory("receiver-savefile");
        let executor = SystemControlActionExecutor::with_savefile_base_dir(save_dir.clone());
        let shell = control_test_shell();
        let worker =
            thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

        client
            .write_all(
                br#"@savefile#7:{filename:"shot.jpg",mime:"image/jpeg",encoding:"base64",data:"QUJD"}"#,
            )
            .expect("should write savefile request");
        client.write_all(b"\n").expect("should finish control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read savefile response");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish");

        let saved_path = save_dir.join("shot.jpg");
        assert_eq!(
            fs::read(&saved_path).expect("saved file should exist"),
            b"ABC"
        );
        assert!(output.contains(r#""id":7"#));
        assert!(output.contains("saved file:"));

        let _ = fs::remove_dir_all(save_dir);
    }

    #[test]
    fn receive_control_result_frames_should_save_file_before_final_response() {
        let (client, mut server) = connected_pair();
        let save_dir = temp_directory("savefile");
        let save_frame = SaveFileFrame {
            request_id: Some(7),
            filename: "shot.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: "QUJD".to_owned(),
            quality: Some(75),
            width: Some(100),
            height: Some(60),
        };

        let worker = thread::spawn(move || {
            let mut transport = ControlTransport::from_tcp_stream(client)
                .expect("transport should wrap tcp stream");
            let mut output = Vec::new();
            receive_control_result_frames(
                &mut transport,
                &mut output,
                &save_dir,
                ControlResponseDisplay::Protocol,
            )
            .expect("client should consume savefile and final response");
            (
                String::from_utf8(output).expect("output should be utf-8"),
                save_dir,
            )
        });

        write_response_line(&mut server, &save_frame.to_wire_message())
            .expect("savefile should send");
        write_response_line(&mut server, r#"@response {"id":7,"value":0}"#)
            .expect("final response should send");
        server
            .shutdown(Shutdown::Both)
            .expect("server side should close cleanly");

        let (output, saved_dir) = worker.join().expect("worker should not panic");
        let saved_path = saved_dir.join("shot.jpg");

        assert!(output.contains("saved file:"));
        assert!(output.contains(r#"@response {"id":7,"value":0}"#));
        assert_eq!(
            fs::read(&saved_path).expect("saved file should exist"),
            b"ABC"
        );

        let _ = fs::remove_dir_all(saved_dir);
    }

    #[test]
    fn receive_control_result_frames_should_save_multiple_savefiles_before_final_response() {
        let (client, mut server) = connected_pair();
        let save_dir = temp_directory("savefile-bundle");
        let image_frame = SaveFileFrame {
            request_id: Some(7),
            filename: "screenshot-123-virtual-desktop.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: "QUJD".to_owned(),
            quality: Some(75),
            width: Some(100),
            height: Some(60),
        };
        let manifest_frame = SaveFileFrame {
            request_id: Some(7),
            filename: "screenshot-123-manifest.json".to_owned(),
            mime: "application/json".to_owned(),
            encoding: "base64".to_owned(),
            data: "eyJzY2hlbWEiOiJyZG9nLnNjcmVlbnNob3QudjEifQ==".to_owned(),
            quality: None,
            width: None,
            height: None,
        };

        let worker = thread::spawn(move || {
            let mut transport = ControlTransport::from_tcp_stream(client)
                .expect("transport should wrap tcp stream");
            let mut output = Vec::new();
            receive_control_result_frames(
                &mut transport,
                &mut output,
                &save_dir,
                ControlResponseDisplay::Protocol,
            )
            .expect("client should consume screenshot bundle frames");
            (
                String::from_utf8(output).expect("output should be utf-8"),
                save_dir,
            )
        });

        write_response_line(&mut server, &image_frame.to_wire_message())
            .expect("image savefile should send");
        write_response_line(&mut server, &manifest_frame.to_wire_message())
            .expect("manifest savefile should send");
        write_response_line(
            &mut server,
            r#"@response {"id":7,"value":{"kind":"screenshot-bundle","layout":"composite","coordinate_space":"os-logical","image":"screenshot-123-virtual-desktop.jpg","manifest":"screenshot-123-manifest.json","display_count":2}}"#,
        )
        .expect("bundle response should send");
        server
            .shutdown(Shutdown::Both)
            .expect("server side should close cleanly");

        let (output, saved_dir) = worker.join().expect("worker should not panic");

        assert_eq!(output.matches("saved file:").count(), 2);
        assert!(output.contains("screenshot-bundle"));
        assert_eq!(
            fs::read(saved_dir.join("screenshot-123-virtual-desktop.jpg"))
                .expect("image file should exist"),
            b"ABC"
        );
        assert_eq!(
            fs::read_to_string(saved_dir.join("screenshot-123-manifest.json"))
                .expect("manifest file should exist"),
            r#"{"schema":"rdog.screenshot.v1"}"#
        );

        let _ = fs::remove_dir_all(saved_dir);
    }

    #[test]
    fn control_receiver_should_escape_double_at_to_literal_shell_command() {
        let (mut client, server) = connected_pair();
        let executor = FakeExecutor::default();
        let (shell, request_line, expected_fragment, cleanup_path) =
            escaped_literal_shell_case("shell-unit");
        let worker = thread::spawn(move || {
            run_control_receiver_with_executor(server, shell.as_str(), &executor)
        });

        client
            .write_all(&request_line)
            .expect("should write escaped shell line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read shell fallback output");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish");

        if let Some(path) = cleanup_path.as_deref() {
            cleanup_temp_path(path);
        }

        assert!(output.contains(&expected_fragment));
    }

    #[test]
    fn control_receiver_should_report_parse_failure_without_falling_back_to_shell() {
        let (mut client, server) = connected_pair();
        let executor = FakeExecutor::default();
        let shell = control_test_shell();
        let worker =
            thread::spawn(move || run_control_receiver_with_executor(server, shell, &executor));

        client
            .write_all(br#"@script:"printf a\nb""#)
            .expect("should write invalid script payload");
        client
            .write_all(b"\n")
            .expect("should finish invalid control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read parse failure response");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish");

        assert!(output.contains(r#""code":64"#));
        assert!(output.contains("首版不支持多行 payload"));
        assert!(output.contains("@response {"));
    }

    #[test]
    fn control_receiver_should_report_executor_failure_with_return_object() {
        struct AlwaysFailingExecutor;

        impl ControlActionExecutor for AlwaysFailingExecutor {
            fn execute(
                &self,
                _command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "首版不支持的 @key 按键: hyper",
                ))
            }
        }

        let (mut client, server) = connected_pair();
        let shell = control_test_shell();
        let worker = thread::spawn(move || {
            run_control_receiver_with_executor(server, shell, &AlwaysFailingExecutor)
        });

        client
            .write_all(br#"@key:"hyper""#)
            .expect("should write unsupported key request");
        client.write_all(b"\n").expect("should finish control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read executor failure response");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish after reporting failure");

        assert!(output.contains("首版不支持的 @key 按键: hyper"));
        assert!(output.contains(r#""code":64"#));
        assert!(output.contains("@response {"));
    }

    #[test]
    fn control_receiver_should_wrap_executor_failure_with_request_id() {
        struct AlwaysFailingExecutor;

        impl ControlActionExecutor for AlwaysFailingExecutor {
            fn execute(
                &self,
                _command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "首版不支持的 @key 按键: hyper",
                ))
            }
        }

        let (mut client, server) = connected_pair();
        let shell = control_test_shell();
        let worker = thread::spawn(move || {
            run_control_receiver_with_executor(server, shell, &AlwaysFailingExecutor)
        });

        client
            .write_all(br#"@key#42:"hyper""#)
            .expect("should write unsupported key request with id");
        client.write_all(b"\n").expect("should finish control line");
        client
            .shutdown(Shutdown::Write)
            .expect("should close write side");

        let mut output = String::new();
        client
            .read_to_string(&mut output)
            .expect("should read executor failure response with id");

        worker
            .join()
            .expect("worker should not panic")
            .expect("control receiver should finish after reporting failure");

        assert!(output.contains(r#""id":42"#));
        assert!(output.contains(r#""code":64"#));
        assert!(output.contains("首版不支持的 @key 按键: hyper"));
    }
}
