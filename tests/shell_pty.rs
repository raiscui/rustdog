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

#[test]
fn reverse_shell_should_run_with_tty_semantics() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut listener = Command::new(&binary)
        .args(["listen", &port.to_string()])
        .stdin(Stdio::piped())
        // 2026-06-19 init_logger 切到 stderr 之后,
        // listener 把 "Connection Received" log 走 stderr 而不是 stdout。
        // 同时 stdout 还会收 listener 把远端 shell 输出 pipe_thread 出来的字节。
        // 两个 stream 都要 pipe,合流检查。
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("listener should start");

    wait_until_port_is_busy(port, Duration::from_secs(3));

    let listener_stdout = listener
        .stdout
        .take()
        .expect("listener stdout pipe should exist");
    let listener_stderr = listener
        .stderr
        .take()
        .expect("listener stderr pipe should exist");
    let (stdout_buffer, stdout_thread) = spawn_output_collector(listener_stdout);
    let (stderr_buffer, stderr_thread) = spawn_output_collector(listener_stderr);
    // 2026-06-19 init_logger 切到 stderr 之后,"Connection Received" log 走 stderr,
    // 但 TTY=111 这种 shell 输出走 stdout (经 pipe_thread 转发)。
    // 合流两个 buffer 才能完整断言。
    let combined_output = || {
        format!(
            "{}{}",
            stdout_buffer.lock().expect("stdout lock").clone(),
            stderr_buffer.lock().expect("stderr lock").clone()
        )
    };

    let mut connector = Command::new(&binary)
        .args(["connect", "-s", "/bin/bash", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("connector should start");

    // 2026-06-19 init_logger 切到 stderr 之后,"Connection Received" 走 stderr。
    // 这里用循环 polling 等到出现,而不是单次 check;
    // connector 启动 → listener accept → log::info! → stderr 收集,
    // 全链路可能要 100ms ~ 1s。
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut got = false;
        while std::time::Instant::now() < deadline {
            if combined_output().contains("Connection Received") {
                got = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            got,
            "listener never reported Connection Received. output so far:\n{}",
            combined_output(),
        );
    }

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

    let combined_output_for_tty = || combined_output();
    let has_tty = {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            if combined_output().contains("TTY=111") {
                found = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        found
    };
    assert!(
        has_tty,
        "shell never reported TTY=111. output so far:\n{}",
        combined_output(),
    );

    let listener_status = listener.wait().expect("listener should exit");
    let connector_status = connector.wait().expect("connector should exit");
    stdout_thread
        .join()
        .expect("stdout collector should finish");
    stderr_thread
        .join()
        .expect("stderr collector should finish");
    let _ = combined_output_for_tty; // 抑制 unused 警告

    assert!(
        listener_status.success(),
        "listener should exit successfully, got {listener_status}",
    );
    assert!(
        connector_status.success(),
        "connector should exit successfully, got {connector_status}",
    );
}
