#![cfg(unix)]

use std::{
    io::{self, Read, Write},
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
fn daemon_inbound_should_keep_idle_session_alive_until_client_sends_data() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(binary)
        .arg("daemon")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", "/bin/bash")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon should start");

    if !wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)) {
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

    let mut client =
        TcpStream::connect(("127.0.0.1", port)).expect("client should connect to daemon");

    // 先制造一段“连接建立了,但远端还没发任何数据”的空窗期。
    // 这正是本次回归原来会被误判成 `WouldBlock` 错误的时间段。
    thread::sleep(Duration::from_millis(300));
    assert_child_still_running(
        &mut daemon,
        "daemon exited while inbound session was only temporarily idle",
    );

    client
        .write_all(b"echo DAEMON_IDLE_OK\nexit\n")
        .expect("should send commands after idle period");
    client.flush().expect("client should flush commands");

    let output =
        read_socket_output_until_contains(&mut client, "DAEMON_IDLE_OK", Duration::from_secs(5))
            .expect("should read shell output from daemon inbound session");

    assert!(
        output.contains("DAEMON_IDLE_OK"),
        "shell output never contained marker after idle period. output so far:\n{output}",
    );

    assert_child_still_running(
        &mut daemon,
        "daemon exited after inbound session completed successfully",
    );

    let second_client =
        TcpStream::connect(("127.0.0.1", port)).expect("daemon should keep listening");
    drop(second_client);

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let status = daemon.wait().expect("daemon wait should succeed");
    assert!(
        !status.success(),
        "killed daemon process should not report success: {status}",
    );
}
