#![cfg(unix)]

use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

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

fn next_free_port() -> u16 {
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should expose local addr")
        .port();
    drop(listener);
    port
}

fn is_port_listening(port: u16) -> bool {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .expect("lsof should be available for unix integration tests");

    output.status.success() && !output.stdout.is_empty()
}

fn wait_until_port_is_busy(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if is_port_listening(port) {
            return;
        }

        thread::sleep(Duration::from_millis(20));
    }

    panic!("port {port} was not busy before timeout");
}

fn spawn_output_collector(
    mut reader: impl Read + Send + 'static,
) -> (Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buffer);
    let handle = thread::spawn(move || {
        let mut local = [0_u8; 1024];

        loop {
            match reader.read(&mut local) {
                Ok(0) => return,
                Ok(len) => {
                    let chunk = String::from_utf8_lossy(&local[..len]);
                    shared
                        .lock()
                        .expect("buffer lock should work")
                        .push_str(&chunk);
                }
                Err(_) => return,
            }
        }
    });

    (buffer, handle)
}

fn wait_until_output_contains(
    buffer: &Arc<Mutex<String>>,
    needle: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if buffer
            .lock()
            .expect("buffer lock should work")
            .contains(needle)
        {
            return true;
        }

        thread::sleep(Duration::from_millis(20));
    }

    false
}

#[test]
fn reverse_shell_should_run_with_tty_semantics() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut listener = Command::new(&binary)
        .args(["listen", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("listener should start");

    wait_until_port_is_busy(port, Duration::from_secs(3));

    let listener_stdout = listener
        .stdout
        .take()
        .expect("listener stdout pipe should exist");
    let (output_buffer, output_thread) = spawn_output_collector(listener_stdout);

    let mut connector = Command::new(&binary)
        .args(["connect", "-s", "/bin/bash", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("connector should start");

    assert!(
        wait_until_output_contains(
            &output_buffer,
            "Connection Received",
            Duration::from_secs(5)
        ),
        "listener never reported Connection Received. output so far:\n{}",
        output_buffer.lock().expect("buffer lock should work"),
    );

    let command = b"python3 -c 'import os; print(\"TTY=%d%d%d\" % (os.isatty(0), os.isatty(1), os.isatty(2)))'\n";
    listener
        .stdin
        .as_mut()
        .expect("listener stdin pipe should exist")
        .write_all(command)
        .expect("should send tty probe command");
    listener
        .stdin
        .as_mut()
        .expect("listener stdin pipe should exist")
        .write_all(b"exit\n")
        .expect("should send exit command");
    listener
        .stdin
        .as_mut()
        .expect("listener stdin pipe should exist")
        .flush()
        .expect("listener stdin should flush");

    assert!(
        wait_until_output_contains(&output_buffer, "TTY=111", Duration::from_secs(5)),
        "shell never reported TTY=111. output so far:\n{}",
        output_buffer.lock().expect("buffer lock should work"),
    );

    let listener_status = listener.wait().expect("listener should exit");
    let connector_status = connector.wait().expect("connector should exit");
    output_thread
        .join()
        .expect("output collector should finish");

    assert!(
        listener_status.success(),
        "listener should exit successfully, got {listener_status}",
    );
    assert!(
        connector_status.success(),
        "connector should exit successfully, got {connector_status}",
    );
}
