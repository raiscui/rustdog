use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
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
    let binary = if cfg!(windows) {
        debug_dir.join("rdog.exe")
    } else {
        debug_dir.join("rdog")
    };

    assert!(
        binary.exists(),
        "expected rdog binary at {}",
        binary.display()
    );

    binary
}

fn platform_test_shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else {
        "/bin/sh"
    }
}

fn platform_ready_command() -> &'static str {
    if cfg!(windows) {
        "echo READY"
    } else {
        "printf READY"
    }
}

fn websocket_url(port: u16) -> String {
    format!("ws://127.0.0.1:{port}")
}

fn temp_workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-control-websocket-{name}-{}-{}",
        std::process::id(),
        next_free_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

fn is_port_listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
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

fn wait_with_output_timeout(
    mut child: Child,
    timeout: Duration,
    context: &str,
) -> std::process::Output {
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

#[test]
fn control_cli_should_drive_websocket_daemon_end_to_end() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let url = websocket_url(port);

    let mut daemon = Command::new(&binary)
        .arg("daemon")
        .env("RDOG_DAEMON__RETRY_SECONDS", "1")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", platform_test_shell())
        .env("RDOG_INBOUND__MODE", "control")
        .env("RDOG_INBOUND__TRANSPORT", "websocket")
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

    let mut json_control = Command::new(&binary)
        .args(["control", "--url", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control cli should start");

    let mut json_stdin = json_control
        .stdin
        .take()
        .expect("control stdin should be piped");
    let control_stdout = json_control
        .stdout
        .take()
        .expect("control stdout should be piped");
    let mut control_reader = BufReader::new(control_stdout);

    writeln!(json_stdin, "{}", r#"{"type":"ping"}"#).expect("should write json ping");
    json_stdin.flush().expect("should flush json ping");
    let mut line = String::new();
    control_reader
        .read_line(&mut line)
        .expect("should read json pong");
    assert_eq!(line.trim_end_matches(['\r', '\n']), "{\"type\":\"pong\"}");

    drop(json_stdin);
    let json_output = json_control
        .wait_with_output()
        .expect("json control cli output should be collectable");
    assert!(
        json_output.status.success(),
        "json control cli should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&json_output.stdout),
        String::from_utf8_lossy(&json_output.stderr),
    );

    let mut control = Command::new(&binary)
        .args(["control", "--url", &url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("line-control cli should start");

    let mut control_stdin = control.stdin.take().expect("control stdin should be piped");
    let control_stdout = control
        .stdout
        .take()
        .expect("control stdout should be piped");
    let mut control_reader = BufReader::new(control_stdout);

    line.clear();
    writeln!(control_stdin, "@ping").expect("should write line-control ping");
    control_stdin
        .flush()
        .expect("should flush line-control ping");
    control_reader
        .read_line(&mut line)
        .expect("should read line-control pong");
    assert_eq!(line.trim_end_matches(['\r', '\n']), "@response \"pong\"");

    line.clear();
    writeln!(
        control_stdin,
        "@cmd#7:\"{}\"",
        platform_ready_command()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
    .expect("should write request-id command");
    control_stdin
        .flush()
        .expect("should flush request-id command");
    control_reader
        .read_line(&mut line)
        .expect("should read request-id response");
    assert!(
        line.contains(r#""id":7"#) && line.contains("READY"),
        "unexpected request-id response: {line}",
    );

    drop(control_stdin);

    let output = control
        .wait_with_output()
        .expect("control cli output should be collectable");
    assert!(
        output.status.success(),
        "control cli should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    assert_child_still_running(
        &mut daemon,
        "daemon exited after websocket control cli session completed",
    );

    daemon
        .kill()
        .expect("daemon should stop after websocket integration cleanup");
    let status = daemon.wait().expect("daemon wait should succeed");
    assert!(
        !status.success(),
        "killed daemon process should not report success: {status}",
    );
}

#[test]
#[cfg(unix)]
fn control_cli_should_run_websocket_pty_command_end_to_end() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let url = websocket_url(port);

    let mut daemon = Command::new(&binary)
        .arg("daemon")
        .env("RDOG_DAEMON__RETRY_SECONDS", "1")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", platform_test_shell())
        .env("RDOG_INBOUND__MODE", "control")
        .env("RDOG_INBOUND__TRANSPORT", "websocket")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("websocket daemon should start");

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

    let output = wait_with_output_timeout(
        Command::new(&binary)
            .args([
                "control",
                "--url",
                &url,
                "--pty",
                "--",
                "/bin/sh",
                "-c",
                "if [ -t 0 ]; then printf WS_PTY_OK; else printf WS_NOT_TTY; fi",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("websocket pty control should start"),
        Duration::from_secs(8),
        "websocket pty control",
    );

    daemon
        .kill()
        .expect("daemon should stop after websocket pty test");
    let _ = daemon.wait();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "websocket pty control should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("WS_PTY_OK"),
        "websocket pty command should see a real tty\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
#[ignore = "requires real screenshot backend and host screen capture permissions"]
fn control_cli_should_execute_screenshot_and_save_file_over_websocket() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let url = websocket_url(port);
    let workdir = temp_workdir("screenshot");

    let mut daemon = Command::new(&binary)
        .arg("daemon")
        .env("RDOG_DAEMON__RETRY_SECONDS", "1")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", platform_test_shell())
        .env("RDOG_INBOUND__MODE", "control")
        .env("RDOG_INBOUND__TRANSPORT", "websocket")
        .current_dir(&workdir)
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
        .args(["control", "--url", &url])
        .current_dir(&workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("websocket control cli should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should be piped")
        .write_all(
            br#"@screenshot#7
"#,
        )
        .expect("should write screenshot request");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("websocket control cli output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    assert!(
        output.status.success(),
        "websocket control cli should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        combined.contains("saved file:"),
        "websocket control output did not contain savefile notice: {combined}"
    );
    assert!(
        combined.contains(r#"@response {"id":7,"value":0}"#),
        "websocket control output did not contain final response payload: {combined}"
    );

    let download_dir = workdir.join("rdog_downloads");
    let entries = fs::read_dir(&download_dir)
        .expect("download directory should exist")
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(
        !entries.is_empty(),
        "download directory should contain saved screenshot files"
    );
    let screenshot = entries
        .iter()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jpg"))
        .expect("should save a jpg screenshot file");
    let metadata = fs::metadata(&screenshot).expect("screenshot file metadata should exist");
    assert!(
        metadata.len() > 0,
        "saved screenshot file should not be empty"
    );

    assert_child_still_running(
        &mut daemon,
        "daemon exited after websocket screenshot control cli session completed",
    );

    daemon
        .kill()
        .expect("daemon should stop after websocket screenshot cleanup");
    let status = daemon.wait().expect("daemon wait should succeed");
    assert!(
        !status.success(),
        "killed daemon process should not report success: {status}",
    );
    let _ = fs::remove_dir_all(workdir);
}
