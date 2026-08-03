#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

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

#[test]
fn control_cli_should_treat_arrow_keys_as_local_cursor_motion_in_tty() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should expose local addr")
        .port();

    // 这里用最小 TCP server 记录 control client 真正发出的第一行。
    // 一旦它看到了首行,就立刻回一条 `@response`,让 CLI 正常收口。
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("server should accept control client");
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .expect("server should clone stream for reading"),
        );
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("server should read first control line");
        stream
            .write_all(b"@response \"TTY_OK\"\n")
            .expect("server should reply to control client");
        stream.flush().expect("server should flush response");
        line.trim_end_matches(['\r', '\n']).to_owned()
    });

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

    // 模拟真实人类编辑:
    // 1. 先敲错成 `@png`
    // 2. 两次左方向键把光标移到 `n` 前
    // 3. 输入 `i`
    // 4. 一次右方向键回到行尾
    // 5. 回车提交
    // 如果 `rdog control` 没有本地 line editor,远端会收到带 `ESC [ D/C` 的脏输入。
    child_stdin
        .write_all(b"@png\x1b[D\x1b[Di\x1b[C\r")
        .expect("should write tty-edited control line");
    child_stdin.flush().expect("script stdin should flush");
    drop(child_stdin);

    let received = server.join().expect("server thread should finish");
    thread::sleep(Duration::from_millis(200));
    if child
        .try_wait()
        .expect("try_wait should not fail after remote response")
        .is_none()
    {
        child
            .kill()
            .expect("script-wrapped control cli should be killable");
    }
    let output = child
        .wait_with_output()
        .expect("script-wrapped control cli output should be collectable");
    let combined = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    assert_eq!(
        received, "@ping",
        "tty arrow-key editing should stay local instead of leaking escape sequences to the remote control line"
    );
    assert!(
        combined.contains("TTY_OK") && !combined.contains("@response \"TTY_OK\""),
        "tty control cli output should render a human-readable response before test cleanup\nstatus: {}\nactual output:\n{combined}",
        output.status,
    );
}
