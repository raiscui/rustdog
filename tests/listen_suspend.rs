#![cfg(unix)]

use std::{
    io::Read,
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
            .expect("try_wait should not fail while waiting for listener")
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

fn wait_until_child_exits(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail")
            .is_some()
        {
            return;
        }

        thread::sleep(Duration::from_millis(20));
    }

    let _ = child.kill();
    let _ = child.wait();
    panic!("rdog did not exit after SIGTSTP");
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

#[test]
fn listen_should_release_port_after_sigtstp() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut child = Command::new(binary)
        .args(["listen", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rdog listener should start");

    if !wait_until_port_is_busy(&mut child, port, Duration::from_secs(3)) {
        let _ = child.kill();
        let output = child
            .wait_with_output()
            .expect("should capture child output after timeout");

        panic!(
            "port {port} was not busy before timeout\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let status = Command::new("kill")
        .args(["-TSTP", &child.id().to_string()])
        .status()
        .expect("kill command should run");
    assert!(status.success(), "kill -TSTP should succeed");

    wait_until_child_exits(&mut child, Duration::from_secs(3));

    let rebound = TcpListener::bind(("127.0.0.1", port))
        .expect("listener port should be reusable after SIGTSTP shutdown");
    drop(rebound);
}

#[test]
fn listen_should_not_exit_just_because_connected_stream_is_temporarily_idle() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut child = Command::new(binary)
        .args(["listen", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rdog listener should start");

    if !wait_until_port_is_busy(&mut child, port, Duration::from_secs(3)) {
        let _ = child.kill();
        let output = child
            .wait_with_output()
            .expect("should capture child output after timeout");

        panic!(
            "port {port} was not busy before timeout\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let client = std::net::TcpStream::connect(("127.0.0.1", port))
        .expect("client should connect to listener");

    thread::sleep(Duration::from_millis(300));
    assert_child_still_running(
        &mut child,
        "listener exited while connected stream was temporarily idle",
    );

    drop(client);
    wait_until_child_exits(&mut child, Duration::from_secs(3));
}
