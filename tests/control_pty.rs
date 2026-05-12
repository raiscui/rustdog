#![cfg(unix)]

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

fn next_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should expose local addr")
        .port();
    drop(listener);
    port
}

fn rdog_binary_path() -> PathBuf {
    if let Some(binary) = std::env::var_os("CARGO_BIN_EXE_rdog") {
        let binary = PathBuf::from(binary);
        assert!(
            binary.exists(),
            "expected rdog binary from CARGO_BIN_EXE_rdog at {}",
            binary.display()
        );
        return binary;
    }

    let current_exe = std::env::current_exe().expect("current test binary path should exist");
    let debug_dir = current_exe
        .parent()
        .expect("test binary should have parent directory")
        .parent()
        .expect("test binary should live under target/debug/deps");
    let binary = debug_dir.join("rdog");

    assert!(
        binary.exists(),
        "expected rdog binary at {}",
        binary.display()
    );

    binary
}

fn is_port_listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn wait_until_port_is_busy(child: &mut Child, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for daemon")
            .is_some()
        {
            return false;
        }

        if is_port_listening(port) {
            return true;
        }

        thread::sleep(Duration::from_millis(20));
    }

    false
}

fn wait_with_output_timeout(mut child: Child, timeout: Duration, context: &str) -> Output {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for child")
            .is_some()
        {
            return child
                .wait_with_output()
                .expect("child output should collect after exit");
        }

        thread::sleep(Duration::from_millis(20));
    }

    let _ = child.kill();
    let output = child
        .wait_with_output()
        .expect("timed out child output should still collect");
    panic!(
        "{context} timed out\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn spawn_control_daemon(port: u16) -> Child {
    Command::new(rdog_binary_path())
        .arg("daemon")
        .env("RDOG_DAEMON__RETRY_SECONDS", "1")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", "/bin/sh")
        .env("RDOG_INBOUND__MODE", "control")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("daemon should start")
}

fn spawn_control_daemon_with_output(port: u16) -> Child {
    Command::new(rdog_binary_path())
        .arg("daemon")
        .env("RDOG_DAEMON__RETRY_SECONDS", "1")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", "/bin/sh")
        .env("RDOG_INBOUND__MODE", "control")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon should start with output capture")
}

fn spawn_output_collector(
    mut reader: impl Read + Send + 'static,
) -> (Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buffer);
    let handle = thread::spawn(move || {
        let mut chunk = [0_u8; 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return,
                Ok(len) => {
                    let text = String::from_utf8_lossy(&chunk[..len]);
                    shared
                        .lock()
                        .expect("collector buffer lock should work")
                        .push_str(&text);
                }
                Err(_) => return,
            }
        }
    });

    (buffer, handle)
}

fn wait_until_buffer_contains(
    buffer: &Arc<Mutex<String>>,
    needle: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let snapshot = buffer
            .lock()
            .expect("collector buffer lock should work")
            .clone();
        if snapshot.contains(needle) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }

    false
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn extract_json_string_field(input: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let start = input.find(&marker)? + marker.len();
    let tail = &input[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_owned())
}

fn decode_pty_output_frame(line: &str) -> String {
    let data = extract_json_string_field(line, "data").expect("pty output should include data");
    let bytes = BASE64_STANDARD
        .decode(data.as_bytes())
        .expect("pty output should be valid base64");
    String::from_utf8_lossy(&bytes).into_owned()
}

fn read_until_pty_output_contains(
    reader: &mut BufReader<TcpStream>,
    needle: &str,
    context: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut decoded = String::new();

    while Instant::now() < deadline {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("raw pty reader should read a frame");
        assert!(
            !line.is_empty(),
            "{context}: PTY stream ended before expected output\nseen decoded: {decoded:?}"
        );

        if line.starts_with("@pty-output ") {
            decoded.push_str(&decode_pty_output_frame(&line));
            if decoded.contains(needle) {
                return decoded;
            }
            continue;
        }

        panic!("{context}: unexpected non-output frame while waiting for {needle:?}: {line:?}");
    }

    panic!("{context}: timed out waiting for {needle:?}\nseen decoded: {decoded:?}");
}

#[test]
fn control_pty_cli_should_spawn_remote_real_tty() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let output = wait_with_output_timeout(
        Command::new(rdog_binary_path())
            .args([
                "control",
                "127.0.0.1",
                &port.to_string(),
                "--pty",
                "--",
                "/bin/sh",
                "-c",
                "if [ -t 0 ]; then printf PTY_OK; else printf NOT_TTY; fi",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pty control cli should start"),
        Duration::from_secs(8),
        "pty tty probe",
    );

    stop_child(&mut daemon);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pty control cli should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("PTY_OK"),
        "remote command should see a real tty\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_pty_string_shorthand_should_switch_cli_into_pty_mode() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let mut child = Command::new(rdog_binary_path())
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control cli should start");

    child
        .stdin
        .as_mut()
        .expect("stdin should exist")
        .write_all(b"@pty:\"/usr/bin/tty\"\n")
        .expect("should send pty shorthand request");
    drop(child.stdin.take());

    let output = wait_with_output_timeout(child, Duration::from_secs(8), "pty shorthand smoke");
    stop_child(&mut daemon);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pty shorthand control cli should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("/dev/"),
        "pty shorthand should switch into PTY mode and print the allocated tty path\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("@pty-ready") && !stdout.contains("@response {"),
        "pty shorthand should not stay in plain line-response mode\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_pty_string_shorthand_should_forward_enter_as_carriage_return_in_tty() {
    let helper_path = std::env::temp_dir().join(format!(
        "rdog-pty-enter-byte-{}-{}.sh",
        std::process::id(),
        next_free_port()
    ));
    fs::write(
        &helper_path,
        "#!/bin/sh\nstty raw -echo\ndd bs=1 count=1 2>/dev/null | od -An -tx1\n",
    )
    .expect("raw byte helper should be written");
    let mut permissions = fs::metadata(&helper_path)
        .expect("raw byte helper metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&helper_path, permissions).expect("raw byte helper should be executable");

    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let binary = rdog_binary_path();
    let mut child = Command::new("script")
        .args([
            "-q",
            "/dev/null",
            &binary.to_string_lossy(),
            "control",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script-wrapped control cli should start");

    let mut child_stdin = child
        .stdin
        .take()
        .expect("script-wrapped control stdin should be piped");

    thread::sleep(Duration::from_millis(200));
    write!(child_stdin, "@pty:\"{}\"\r", helper_path.to_string_lossy())
        .expect("should open pty via string shorthand");
    child_stdin.flush().expect("pty open should flush");
    thread::sleep(Duration::from_millis(700));
    child_stdin
        .write_all(b"\r")
        .expect("should send one terminal Enter key into pty");
    child_stdin.flush().expect("terminal Enter should flush");
    thread::sleep(Duration::from_millis(400));
    child_stdin
        .write_all(&[0x04])
        .expect("control cli should accept local EOF after the PTY helper exits");
    child_stdin.flush().expect("local EOF should flush");
    drop(child_stdin);

    let output = wait_with_output_timeout(
        child,
        Duration::from_secs(8),
        "script-wrapped pty enter byte probe",
    );
    stop_child(&mut daemon);
    let _ = fs::remove_file(&helper_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pty enter byte probe should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("0d"),
        "terminal Enter should reach the remote raw PTY as carriage return 0d, not as newline 0a\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_pty_ctrl_c_should_exit_remote_program_and_return_to_control_prompt() {
    let helper_path = std::env::temp_dir().join(format!(
        "rdog-pty-ctrl-c-{}-{}.sh",
        std::process::id(),
        next_free_port()
    ));
    fs::write(
        &helper_path,
        "#!/bin/sh\ntrap 'printf REMOTE_INT; exit 130' INT\nprintf REMOTE_READY\nwhile true; do sleep 1; done\n",
    )
    .expect("ctrl-c helper should be written");
    let mut permissions = fs::metadata(&helper_path)
        .expect("ctrl-c helper metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&helper_path, permissions).expect("ctrl-c helper should be executable");

    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let binary = rdog_binary_path();
    let mut child = Command::new("script")
        .args([
            "-q",
            "/dev/null",
            &binary.to_string_lossy(),
            "control",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script-wrapped control cli should start");

    let child_stdout = child
        .stdout
        .take()
        .expect("script-wrapped control stdout should be piped");
    let (stdout_buffer, _stdout_collector) = spawn_output_collector(child_stdout);

    let mut child_stdin = child
        .stdin
        .take()
        .expect("script-wrapped control stdin should be piped");

    thread::sleep(Duration::from_millis(200));
    write!(child_stdin, "@pty:\"{}\"\r", helper_path.to_string_lossy())
        .expect("should open pty via string shorthand");
    child_stdin.flush().expect("pty open should flush");
    assert!(
        wait_until_buffer_contains(&stdout_buffer, "REMOTE_READY", Duration::from_secs(3)),
        "remote PTY helper should print readiness before Ctrl-C"
    );
    child_stdin
        .write_all(&[0x03])
        .expect("should send Ctrl-C into remote PTY");
    child_stdin.flush().expect("Ctrl-C should flush");
    thread::sleep(Duration::from_millis(900));
    child_stdin
        .write_all(b"@ping\r")
        .expect("control cli should still accept input after remote PTY exits");
    child_stdin.flush().expect("post-pty ping should flush");
    thread::sleep(Duration::from_millis(400));
    child_stdin
        .write_all(&[0x04])
        .expect("control cli should accept local EOF after returning to line-control mode");
    child_stdin.flush().expect("local EOF should flush");
    drop(child_stdin);

    let output = wait_with_output_timeout(
        child,
        Duration::from_secs(8),
        "script-wrapped pty ctrl-c resume probe",
    );
    stop_child(&mut daemon);
    let _ = fs::remove_file(&helper_path);

    let stdout = {
        let mut stdout = stdout_buffer
            .lock()
            .expect("collector buffer lock should work")
            .clone();
        stdout.push_str(&String::from_utf8_lossy(&output.stdout));
        stdout
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pty ctrl-c resume probe should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("REMOTE_INT"),
        "Ctrl-C should reach and terminate only the remote PTY program\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "rdog control should return to line-control mode after remote PTY exits\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_pty_should_treat_control_words_as_remote_input() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let mut child = Command::new(rdog_binary_path())
        .args([
            "control",
            "127.0.0.1",
            &port.to_string(),
            "--pty",
            "--",
            "/bin/sh",
            "-c",
            r#"IFS= read -r line; printf 'GOT:%s' "$line""#,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("pty control cli should start");

    child
        .stdin
        .as_mut()
        .expect("stdin should exist")
        .write_all(b"@script:\"printf BAD\"\n")
        .expect("should send control-looking text into pty");
    drop(child.stdin.take());

    let output = wait_with_output_timeout(child, Duration::from_secs(8), "pty transparent input");
    stop_child(&mut daemon);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pty transparent input command should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(r#"GOT:@script:"printf BAD""#),
        "control-looking text should be delivered to the remote pty, not executed locally\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_pty_resize_frame_should_update_remote_winsize() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("raw pty client should connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .expect("read timeout should configure");
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .expect("stream should clone for raw pty reader"),
    );

    writeln!(
        stream,
        r#"@pty:{{cmd:"/bin/sh",args:["-c","stty size; read line; stty size"],cols:80,rows:24}}"#
    )
    .expect("raw pty open should write");
    stream.flush().expect("raw pty open should flush");

    let mut ready = String::new();
    reader
        .read_line(&mut ready)
        .expect("should receive pty ready");
    assert!(
        ready.starts_with("@pty-ready "),
        "unexpected ready frame: {ready:?}"
    );
    let session_id = extract_json_string_field(&ready, "session_id")
        .expect("pty ready should include session id");

    let first_text = read_until_pty_output_contains(&mut reader, "24 80", "initial stty size");
    assert!(
        first_text.contains("24 80"),
        "initial stty size should reflect open cols/rows\ndecoded: {first_text:?}"
    );

    writeln!(
        stream,
        r#"@pty-resize {{"session_id":"{session_id}","cols":100,"rows":31}}"#
    )
    .expect("resize frame should write");
    writeln!(stream, "go").expect("stdin wake line should write");
    stream.flush().expect("resize and wake should flush");

    let resized_text = read_until_pty_output_contains(&mut reader, "31 100", "resized stty size");
    assert!(
        resized_text.contains("31 100"),
        "resized stty size should reflect @pty-resize frame\ndecoded: {resized_text:?}"
    );

    drop(reader);
    drop(stream);
    stop_child(&mut daemon);
}

#[test]
fn control_pty_close_should_kill_active_session_by_id() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon(port);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("raw pty client should connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .expect("read timeout should configure");
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .expect("stream should clone for raw pty reader"),
    );

    writeln!(
        stream,
        r#"@pty:{{cmd:"/bin/sh",args:["-c","sleep 30"],cols:80,rows:24}}"#
    )
    .expect("raw pty open should write");
    stream.flush().expect("raw pty open should flush");

    let mut ready = String::new();
    reader
        .read_line(&mut ready)
        .expect("should receive pty ready");
    assert!(
        ready.starts_with("@pty-ready "),
        "unexpected ready frame: {ready:?}"
    );
    let session_id = extract_json_string_field(&ready, "session_id")
        .expect("pty ready should include session id");

    let close_output = wait_with_output_timeout(
        Command::new(rdog_binary_path())
            .args([
                "control",
                "127.0.0.1",
                &port.to_string(),
                "--pty-close",
                &session_id,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pty-close cli should start"),
        Duration::from_secs(8),
        "pty close cli",
    );
    let close_stdout = String::from_utf8_lossy(&close_output.stdout);
    let close_stderr = String::from_utf8_lossy(&close_output.stderr);
    assert!(
        close_output.status.success() && close_stdout.contains("@response 0"),
        "pty-close should acknowledge active session close\nstdout:\n{close_stdout}\nstderr:\n{close_stderr}"
    );

    let mut exit = String::new();
    reader
        .read_line(&mut exit)
        .expect("original pty client should receive pty closed after close");
    assert!(
        exit.starts_with("@pty-closed ") && exit.contains(&session_id),
        "unexpected pty closed frame after close: {exit:?}"
    );

    drop(reader);
    drop(stream);
    stop_child(&mut daemon);
}

#[test]
fn control_pty_detach_should_allow_later_attach() {
    let port = next_free_port();
    let mut daemon = spawn_control_daemon_with_output(port);
    let daemon_stdout = daemon.stdout.take().expect("daemon stdout should exist");
    let daemon_stderr = daemon.stderr.take().expect("daemon stderr should exist");
    let (daemon_stdout_buffer, _daemon_stdout_collector) = spawn_output_collector(daemon_stdout);
    let (daemon_stderr_buffer, _daemon_stderr_collector) = spawn_output_collector(daemon_stderr);
    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(4)),
        "daemon control lane never started listening on port {port}",
    );

    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("raw pty client should connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .expect("read timeout should configure");
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .expect("stream should clone for raw pty reader"),
    );

    writeln!(
        stream,
        r#"@pty:{{cmd:"/bin/sh",args:["-c","printf FIRST; sleep 1; printf SECOND; sleep 5"],cols:80,rows:24}}"#
    )
    .expect("raw pty open should write");
    stream.flush().expect("raw pty open should flush");

    let mut ready = String::new();
    reader
        .read_line(&mut ready)
        .expect("should receive pty ready");
    let session_id = extract_json_string_field(&ready, "session_id")
        .expect("pty ready should include session id");

    let mut first_output = String::new();
    reader
        .read_line(&mut first_output)
        .expect("should receive first pty output");
    assert!(
        first_output.starts_with("@pty-output "),
        "unexpected first pty output: {first_output:?}"
    );

    let detach_output = wait_with_output_timeout(
        Command::new(rdog_binary_path())
            .args([
                "control",
                "127.0.0.1",
                &port.to_string(),
                "--pty-detach",
                &session_id,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pty-detach cli should start"),
        Duration::from_secs(8),
        "pty detach cli",
    );
    assert!(
        detach_output.status.success(),
        "pty-detach should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&detach_output.stdout),
        String::from_utf8_lossy(&detach_output.stderr)
    );

    let mut detached = String::new();
    reader
        .read_line(&mut detached)
        .expect("original pty client should receive pty detached frame");
    assert!(
        detached.starts_with("@pty-detached ") && detached.contains(&session_id),
        "unexpected pty detached frame: {detached:?}"
    );

    drop(reader);
    drop(stream);

    let attach_child = Command::new(rdog_binary_path())
        .args([
            "control",
            "127.0.0.1",
            &port.to_string(),
            "--pty-attach",
            &session_id,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("pty-attach cli should start");
    thread::sleep(Duration::from_millis(2200));
    let close_after_attach_output = wait_with_output_timeout(
        Command::new(rdog_binary_path())
            .args([
                "control",
                "127.0.0.1",
                &port.to_string(),
                "--pty-close",
                &session_id,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pty-close after attach cli should start"),
        Duration::from_secs(8),
        "pty close after attach cli",
    );
    let close_after_attach_stdout = String::from_utf8_lossy(&close_after_attach_output.stdout);
    let close_after_attach_stderr = String::from_utf8_lossy(&close_after_attach_output.stderr);
    assert!(
        close_after_attach_output.status.success() && close_after_attach_stdout.contains("@response 0"),
        "pty-close after attach should succeed\nstdout:\n{close_after_attach_stdout}\nstderr:\n{close_after_attach_stderr}"
    );
    let attach_output =
        wait_with_output_timeout(attach_child, Duration::from_secs(8), "pty attach cli");
    let attach_stdout = String::from_utf8_lossy(&attach_output.stdout);
    let attach_stderr = String::from_utf8_lossy(&attach_output.stderr);
    let daemon_stdout = daemon_stdout_buffer
        .lock()
        .expect("daemon stdout buffer lock should work")
        .clone();
    let daemon_stderr = daemon_stderr_buffer
        .lock()
        .expect("daemon stderr buffer lock should work")
        .clone();
    assert!(
        attach_stdout.contains("SECOND"),
        "reattached client should continue receiving future PTY output after attach\nstdout:\n{attach_stdout}\nstderr:\n{attach_stderr}\ndaemon-stdout:\n{daemon_stdout}\ndaemon-stderr:\n{daemon_stderr}"
    );
    assert!(
        attach_stdout.contains("remote PTY closed before natural exit: force_close"),
        "reattached client should observe force-close terminal semantics after out-of-band close\nstdout:\n{attach_stdout}\nstderr:\n{attach_stderr}\ndaemon-stdout:\n{daemon_stdout}\ndaemon-stderr:\n{daemon_stderr}"
    );

    stop_child(&mut daemon);
}
