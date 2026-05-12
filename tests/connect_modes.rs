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

fn spawn_connect(mode: &str, port: u16) -> Child {
    Command::new(rdog_binary_path())
        .args([
            "connect",
            "--mode",
            mode,
            "-s",
            "/bin/sh",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("connect should start")
}

fn accept_with_timeout(listener: &TcpListener, child: &mut Child, timeout: Duration) -> TcpStream {
    let deadline = Instant::now() + timeout;
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");

    while Instant::now() < deadline {
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
                "connect process exited before controller accepted socket\nstatus: {status}\nstdout:\n{}\nstderr:\n{}",
                stdout,
                stderr,
            );
        }

        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("accepted controller stream should be blocking");
                return stream;
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => panic!("accept should not fail: {err}"),
        }
    }

    panic!("timed out waiting for connect() to dial back");
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

fn send_agent_request(stream: &mut TcpStream, command: &str) {
    writeln!(
        stream,
        "{{\"command\":\"{}\"}}",
        escape_json_string(command)
    )
    .expect("should write json request");
    stream.flush().expect("should flush request");
}

fn read_agent_response(stream: &TcpStream) -> (i32, String, String) {
    let mut reader = BufReader::new(stream.try_clone().expect("stream should clone"));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("should read response line");
    parse_json_agent_response(line.trim_end_matches(['\r', '\n']))
}

#[test]
fn connect_stdio_mode_should_forward_clean_noninteractive_output() {
    let port = next_free_port();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("controller listener should bind");
    let mut child = spawn_connect("stdio", port);
    let mut stream = accept_with_timeout(&listener, &mut child, Duration::from_secs(3));

    stream
        .write_all(b"printf 'STDIO_OK\\n'; exit\n")
        .expect("should send stdio commands");
    stream.flush().expect("should flush stdio commands");

    let output = read_socket_output_until_contains(&mut stream, "STDIO_OK", Duration::from_secs(5))
        .expect("should read stdio shell output");
    assert!(
        output.contains("STDIO_OK"),
        "stdio mode never produced marker output. output so far:\n{output}",
    );

    drop(stream);

    let output = child
        .wait_with_output()
        .expect("stdio child should exit after shell exit");
    assert!(
        output.status.success(),
        "stdio mode should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn connect_agent_mode_should_return_stdout_stderr_and_exit_code_per_command() {
    let port = next_free_port();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("controller listener should bind");
    let mut child = spawn_connect("agent", port);
    let mut stream = accept_with_timeout(&listener, &mut child, Duration::from_secs(3));

    send_agent_request(&mut stream, "printf 'OUT'; printf 'ERR' >&2; exit 7");
    let (exit_code, stdout, stderr) = read_agent_response(&stream);
    assert_eq!(exit_code, 7);
    assert_eq!(stdout, "OUT");
    assert_eq!(stderr, "ERR");

    send_agent_request(&mut stream, "printf 'SECOND'");
    let (exit_code, stdout, stderr) = read_agent_response(&stream);
    assert_eq!(exit_code, 0);
    assert_eq!(stdout, "SECOND");
    assert!(stderr.is_empty(), "unexpected stderr: {:?}", stderr);

    drop(stream);

    let output = child
        .wait_with_output()
        .expect("agent child should exit after controller disconnects");
    assert!(
        output.status.success(),
        "agent mode should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
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
