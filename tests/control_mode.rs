#![cfg(unix)]

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

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
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .expect("lsof should be available for unix integration tests");

    output.status.success() && !output.stdout.is_empty()
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

fn assert_child_still_running(child: &mut Child, context: &str) {
    if let Some(status) = child.try_wait().expect("try_wait should not fail") {
        let mut stdout = String::new();
        let mut stderr = String::new();

        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .expect("should read child stdout");
        }
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)
                .expect("should read child stderr");
        }

        panic!(
            "{context}\nstatus: {status}\nstdout:\n{}\nstderr:\n{}",
            stdout, stderr,
        );
    }
}

fn send_agent_request(stream: &mut impl Write, command: &str) {
    send_json_line(
        stream,
        &format!("{{\"command\":\"{}\"}}", escape_json_string(command)),
    );
}

fn send_json_line(stream: &mut impl Write, line: &str) {
    writeln!(stream, "{line}").expect("should write json line");
    stream.flush().expect("should flush request");
}

fn read_agent_response(reader: &mut impl BufRead) -> (i32, String, String) {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("should read response line");
    parse_json_agent_response(line.trim_end_matches(['\r', '\n']))
}

fn read_line(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).expect("should read line");
    line
}

fn read_socket_output_until_contains(
    stream: &mut TcpStream,
    needle: &str,
    timeout: Duration,
) -> io::Result<String> {
    let deadline = Instant::now() + timeout;
    let mut output = String::new();
    let mut buffer = [0_u8; 1024];

    stream.set_read_timeout(Some(Duration::from_millis(100)))?;

    while Instant::now() < deadline {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(output),
            Ok(len) => {
                output.push_str(&String::from_utf8_lossy(&buffer[..len]));
                if output.contains(needle) {
                    return Ok(output);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => return Err(err),
        }
    }

    Ok(output)
}

#[test]
fn control_mode_should_bridge_agent_protocol_to_daemon_inbound() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
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
        .expect("daemon should start");

    if !wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(6)) {
        let _ = daemon.kill();
        let output = daemon
            .wait_with_output()
            .expect("should capture daemon output after timeout");

        panic!(
            "port {port} was not busy before timeout\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control should start");

    let mut control_stdin = control.stdin.take().expect("control stdin should be piped");
    let control_stdout = control
        .stdout
        .take()
        .expect("control stdout should be piped");
    let mut control_reader = BufReader::new(control_stdout);

    // 控制连接先建立,但暂时不发任何消息。
    // 这里故意等超过旧的自动回退窗口,验证 daemon inbound 不会因为“长时间没首包”
    // 就把连接误判成普通交互 shell。
    thread::sleep(Duration::from_millis(400));

    send_json_line(&mut control_stdin, "{\"type\":\"ping\"}");
    let pong = read_line(&mut control_reader);
    assert_eq!(pong.trim_end_matches(['\r', '\n']), "{\"type\":\"pong\"}");

    send_agent_request(
        &mut control_stdin,
        "printf 'CONTROL_OK'; printf 'CONTROL_ERR' >&2; exit 6",
    );
    let (exit_code, stdout, stderr) = read_agent_response(&mut control_reader);
    assert_eq!(exit_code, 6);
    assert_eq!(stdout, "CONTROL_OK");
    assert_eq!(stderr, "CONTROL_ERR");

    send_agent_request(&mut control_stdin, "printf 'SECOND_OK'");
    let (exit_code, stdout, stderr) = read_agent_response(&mut control_reader);
    assert_eq!(exit_code, 0);
    assert_eq!(stdout, "SECOND_OK");
    assert!(stderr.is_empty(), "unexpected stderr: {:?}", stderr);

    drop(control_stdin);

    let output = control
        .wait_with_output()
        .expect("control should exit after stdin closes");
    assert!(
        output.status.success(),
        "control mode should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    assert_child_still_running(
        &mut daemon,
        "daemon exited after agent-controlled session completed",
    );

    let mut simple_client =
        TcpStream::connect(("127.0.0.1", port)).expect("simple client should connect to daemon");
    // 简单命令模式也支持最小心跳。
    simple_client
        .write_all(b"@ping\n")
        .expect("should send simple ping request");
    simple_client
        .flush()
        .expect("simple client should flush ping request");
    let output = read_socket_output_until_contains(
        &mut simple_client,
        r#"@response "pong""#,
        Duration::from_secs(5),
    )
    .expect("should read simple ping response");
    assert!(
        output.contains(r#"@response "pong""#),
        "simple command ping did not receive pong. output so far:\n{output}",
    );

    simple_client
        .write_all(b"printf 'AT_OK'")
        .expect("should send simple command request");
    simple_client
        .write_all(b"\n")
        .expect("should terminate simple command line");
    simple_client
        .flush()
        .expect("simple client should flush command");

    let output = read_socket_output_until_contains(
        &mut simple_client,
        r#"@response "AT_OK""#,
        Duration::from_secs(5),
    )
    .expect("should read simple command response");
    assert!(
        output.contains(r#"@response "AT_OK""#),
        "simple command response did not contain request/response payload. output so far:\n{output}",
    );
    drop(simple_client);

    let mut plain_client =
        TcpStream::connect(("127.0.0.1", port)).expect("plain client should connect to daemon");
    plain_client
        .write_all(b"printf 'PLAIN_OK\\n'; exit\n")
        .expect("should send plain shell command");
    plain_client
        .flush()
        .expect("plain client should flush command");

    let output =
        read_socket_output_until_contains(&mut plain_client, "PLAIN_OK", Duration::from_secs(5))
            .expect("should read plain shell output");
    assert!(
        output.contains("PLAIN_OK"),
        "plain tcp session did not receive marker output. output so far:\n{output}",
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let status = daemon.wait().expect("daemon wait should succeed");
    assert!(
        !status.success(),
        "killed daemon process should not report success: {status}",
    );
}

fn parse_json_agent_response(input: &str) -> (i32, String, String) {
    let exit_code = parse_json_field(input, "exit_code")
        .parse::<i32>()
        .expect("exit_code should parse");
    let stdout = parse_json_field(input, "stdout");
    let stderr = parse_json_field(input, "stderr");
    (exit_code, stdout, stderr)
}

fn parse_json_field(input: &str, key: &str) -> String {
    let pattern = format!("\"{key}\":");
    let start = input
        .find(&pattern)
        .unwrap_or_else(|| panic!("missing json field `{key}` in `{input}`"))
        + pattern.len();
    let value = &input[start..];

    if key == "exit_code" {
        let end = value.find([',', '}']).unwrap_or(value.len());
        return value[..end].trim().to_owned();
    }

    let value = value.trim_start();
    assert!(
        value.starts_with('"'),
        "json string field `{key}` should start with quote in `{input}`"
    );
    let bytes = value.as_bytes();
    let mut escaped = false;
    let mut result = String::new();

    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if escaped {
            match byte {
                b'"' => result.push('"'),
                b'\\' => result.push('\\'),
                b'n' => result.push('\n'),
                b'r' => result.push('\r'),
                b't' => result.push('\t'),
                other => panic!("unsupported escape `{}` in `{input}`", other as char),
            }
            escaped = false;
            continue;
        }

        match byte {
            b'\\' => escaped = true,
            b'"' => {
                assert!(
                    value[index + 1..].trim_start().starts_with([',', '}']),
                    "json field `{key}` has invalid trailer in `{input}`"
                );
                return result;
            }
            byte => result.push(byte as char),
        }
    }

    panic!("unterminated json string field `{key}` in `{input}`");
}

fn escape_json_string(input: &str) -> String {
    let mut escaped = String::new();

    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch => escaped.push(ch),
        }
    }

    escaped
}
