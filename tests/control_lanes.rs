#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
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
            .expect("try_wait should not fail while waiting for process")
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

fn key_request_response_is_acceptable(stdout: &str) -> bool {
    if stdout.contains(r#"@response {"id":7,"value":0}"#) {
        return true;
    }

    // 这条集成测试真正要锁的是:
    // - `@key` 对象 payload 被正确路由
    // - request id 在响应中被保留
    // 真实输入模拟是否被 macOS / Windows 权限放行,属于机器环境前提,不应把
    // “本机已授权”误写成协议层唯一成功条件。
    stdout.contains(r#"@response {"id":7,"code":77,"error":"#)
        && (stdout.contains("permission to simulate input")
            || stdout.contains("辅助功能权限")
            || stdout.contains("blocked by UIPI"))
}

fn temp_shell_wrapper(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-control-wrapper-{name}-{}",
        std::process::id()
    ));
    fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = \"-c\" ]; then\n  printf '%s' \"$2\"\nelse\n  printf 'unexpected-args'\nfi\n",
    )
    .expect("should write wrapper shell");

    let mut perms = fs::metadata(&path)
        .expect("wrapper metadata should exist")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    fs::set_permissions(&path, perms).expect("should mark wrapper executable");

    path
}

fn cleanup_temp_path(path: &Path) {
    let _ = fs::remove_file(path);
}

fn temp_workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-control-lanes-{name}-{}-{}",
        std::process::id(),
        next_free_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

fn temp_cat_shell(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rdog-cat-shell-{name}-{}", std::process::id()));
    fs::write(&path, "#!/bin/sh\ncat\n").expect("should write cat shell");

    let mut perms = fs::metadata(&path)
        .expect("cat shell metadata should exist")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    fs::set_permissions(&path, perms).expect("should mark cat shell executable");

    path
}

#[test]
fn daemon_control_lane_should_execute_script_via_rdog_control() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control sender should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(
            br#"@script:"printf READY"
"#,
        )
        .expect("should send control script");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("control sender output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control sender should exit successfully"
    );
    assert!(
        stdout.contains(r#"@response "READY""#),
        "stdout did not contain request/response payload: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn daemon_control_lane_should_roundtrip_request_id_in_response() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control sender should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(
            br#"@cmd#42:"printf READY"
"#,
        )
        .expect("should send control cmd with request id");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("control sender output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control sender should exit successfully"
    );
    assert!(
        stdout.contains(r#"@response {"id":42,"value":"READY"}"#),
        "stdout did not contain request-id response payload: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn daemon_control_lane_should_accept_key_object_payload_with_request_id() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control sender should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(
            br#"@key#7:{key:"F11",hold_ms:200,mode:"press_release"}
"#,
        )
        .expect("should send key object payload with request id");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("control sender output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control sender should exit successfully"
    );
    assert!(
        key_request_response_is_acceptable(&stdout),
        "stdout did not contain key object response payload: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
#[ignore = "requires real screenshot backend and host screen capture permissions"]
fn daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let workdir = temp_workdir("screenshot-smoke");
    let mut daemon = Command::new(&binary)
        .arg("daemon")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", "/bin/sh")
        .env("RDOG_INBOUND__MODE", "control")
        .current_dir(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .current_dir(&workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control sender should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(
            br#"@screenshot#7
"#,
        )
        .expect("should send screenshot request");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("control sender output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    assert!(
        output.status.success(),
        "control sender should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        combined.contains("saved file:"),
        "control output did not contain savefile notice: {combined}"
    );
    assert!(
        combined.matches("saved file:").count() >= 2,
        "control output did not contain two savefile notices: {combined}"
    );
    assert!(
        combined.contains("screenshot-bundle") && combined.contains("os-logical"),
        "control output did not contain screenshot bundle summary: {combined}"
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
    let manifest = entries
        .iter()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("should save a json screenshot manifest");
    let manifest_text = fs::read_to_string(&manifest).expect("manifest should be readable");
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest should be json");
    assert_eq!(manifest_json["schema"], "rdog.screenshot.v1");
    assert_eq!(manifest_json["layout"], "composite");
    assert_eq!(manifest_json["coordinate_space"], "os-logical");
    assert_eq!(
        manifest_json["display_count"].as_u64(),
        Some(
            manifest_json["displays"]
                .as_array()
                .expect("displays should be an array")
                .len() as u64
        )
    );

    daemon
        .kill()
        .expect("daemon should stop after screenshot smoke");
    let _ = daemon.wait();
    let _ = fs::remove_dir_all(workdir);
}

#[test]
fn daemon_control_lane_should_report_invalid_key_name_to_client() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let mut control = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control sender should start");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(
            br#"@key:"hyper"
"#,
        )
        .expect("should send unsupported key request");
    drop(control.stdin.take());

    let output = control
        .wait_with_output()
        .expect("control sender output should be collectable");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control sender should still exit successfully after remote protocol error",
    );
    assert!(
        stdout.contains("首版不支持的 @key 按键: hyper"),
        "stdout did not contain invalid key error: {stdout}",
    );
    assert!(
        stdout.contains(r#""code":64"#),
        "stdout did not contain invalid key error code: {stdout}",
    );
    assert!(
        stdout.contains("@response {"),
        "stdout did not contain invalid key return envelope: {stdout}",
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn daemon_outbound_control_lane_should_execute_remote_script() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("control server should bind");

    let mut daemon = Command::new(&binary)
        .arg("daemon")
        .env("RDOG_OUTBOUND__ENABLED", "true")
        .env("RDOG_OUTBOUND__HOST", "127.0.0.1")
        .env("RDOG_OUTBOUND__PORT", port.to_string())
        .env("RDOG_OUTBOUND__SHELL", "/bin/sh")
        .env("RDOG_OUTBOUND__MODE", "control")
        .env("RDOG_INBOUND__ENABLED", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("daemon outbound control lane should start");

    let (mut server_stream, _) = listener
        .accept()
        .expect("control server should accept daemon outbound");
    server_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("server stream timeout should set");

    server_stream
        .write_all(
            br#"@script:"printf OUTBOUND_OK"
"#,
        )
        .expect("should send outbound control script");
    server_stream
        .shutdown(std::net::Shutdown::Write)
        .expect("should close write half");

    let mut output = String::new();
    server_stream
        .read_to_string(&mut output)
        .expect("should read outbound control response");

    daemon
        .kill()
        .expect("daemon should stop after outbound control verification");
    let _ = daemon.wait();

    assert!(
        output.contains(r#"@response "OUTBOUND_OK""#),
        "daemon outbound control lane did not return script payload: {output}",
    );
}

#[test]
fn connect_control_mode_should_execute_escaped_line_as_literal_shell_text() {
    let port = next_free_port();
    let wrapper = temp_shell_wrapper("connect");
    let binary = rdog_binary_path();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("test listener should bind");

    let mut connector = Command::new(&binary)
        .args([
            "connect",
            "-s",
            wrapper.to_str().expect("wrapper path should be utf-8"),
            "--mode",
            "control",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("connect control receiver should start");

    let (mut server_stream, _) = listener.accept().expect("listener should accept connector");
    server_stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("server stream timeout should set");

    server_stream
        .write_all(b"@@echo hi\n")
        .expect("should send escaped control line");
    server_stream
        .shutdown(std::net::Shutdown::Write)
        .expect("should close write half");

    let mut output = String::new();
    server_stream
        .read_to_string(&mut output)
        .expect("should read control receiver output");

    let status = connector
        .wait()
        .expect("connect control receiver should exit after EOF");

    cleanup_temp_path(&wrapper);

    assert!(
        status.success(),
        "connect control receiver should exit successfully"
    );
    assert!(
        output.contains(r#"@response "@echo hi""#),
        "escaped line did not return shell fallback payload intact: {output}",
    );
}

#[test]
fn connect_interactive_mode_should_not_interpret_control_lines() {
    let port = next_free_port();
    let cat_shell = temp_cat_shell("interactive-negative");
    let binary = rdog_binary_path();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("negative listener should bind");

    let mut connector = Command::new(&binary)
        .args([
            "connect",
            "-s",
            cat_shell.to_str().expect("cat shell path should be utf-8"),
            "--mode",
            "interactive",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("interactive connector should start");

    let (mut server_stream, _) = listener.accept().expect("listener should accept connector");
    server_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("server stream timeout should set");

    server_stream
        .write_all(
            br#"@script:"printf SHOULD_NOT_RUN"
"#,
        )
        .expect("should send control-looking line");
    thread::sleep(Duration::from_millis(300));
    server_stream
        .shutdown(std::net::Shutdown::Write)
        .expect("should close write half");

    let mut output = String::new();
    server_stream
        .read_to_string(&mut output)
        .expect("should read interactive shell output");
    let status = connector
        .wait()
        .expect("interactive connector should exit after EOF");

    cleanup_temp_path(&cat_shell);

    assert!(
        status.success(),
        "interactive connector should exit successfully"
    );
    assert!(
        output.contains(r#"@script:"printf SHOULD_NOT_RUN""#),
        "interactive mode unexpectedly reinterpreted control-looking line: {output}",
    );
    assert!(
        !output.contains("@response"),
        "interactive mode should not emit control protocol return markers: {output}",
    );
}

#[test]
fn listen_local_interactive_should_reach_connect_control_lane() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut listener = Command::new(&binary)
        .args(["listen", "--local-interactive", &port.to_string()])
        .stdin(Stdio::piped())
        // 两个 stream 都要 pipe:
        // - stdout 收 `pipe_thread(stream → stdout())` 转发的 `@response` 帧
        // - stderr 收 init_logger 的 `info:` / `warn:` 等日志(包括 "Connection Received")
        //   (2026-06-19 init_logger 切到 stderr 之前这些日志是走 stdout 的)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("listener should start");

    assert!(
        wait_until_port_is_busy(&mut listener, port, Duration::from_secs(3)),
        "listener local-interactive never started on port {port}",
    );

    let mut connector = Command::new(&binary)
        .args([
            "connect",
            "-s",
            "/bin/sh",
            "--mode",
            "control",
            "127.0.0.1",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("connect control receiver should start");

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
    // listener 把连接返回的 `@response` 帧从 socket 转发到 stdout
    // (见 listener/mod.rs::Mode::LocalInteractive 里的 pipe_thread),
    // 而 init_logger 的 "Connection Received" 走 stderr。
    // 合并两个 buffer 方便后续断言。
    let combined_output = move || {
        format!(
            "{}{}",
            stdout_buffer.lock().expect("stdout lock").clone(),
            stderr_buffer.lock().expect("stderr lock").clone()
        )
    };
    fn wait_until_combined_contains<F>(combined: &F, needle: &str, timeout: Duration) -> bool
    where
        F: Fn() -> String,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if combined().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    assert!(
        wait_until_combined_contains(
            &combined_output,
            "Connection Received",
            Duration::from_secs(5)
        ),
        "listener never reported Connection Received. output so far:\n{}",
        combined_output(),
    );

    listener
        .stdin
        .as_mut()
        .expect("listener stdin should exist")
        .write_all(
            br#"@script:"printf LISTEN_OK"
"#,
        )
        .expect("should send control script via local interactive lane");
    listener
        .stdin
        .as_mut()
        .expect("listener stdin should exist")
        .flush()
        .expect("listener stdin should flush");

    assert!(
        wait_until_combined_contains(
            &combined_output,
            r#"@response "LISTEN_OK""#,
            Duration::from_secs(5),
        ),
        "listener never printed control return payload. output so far:\n{}",
        combined_output(),
    );

    drop(listener.stdin.take());

    let listener_status = listener
        .wait()
        .expect("listener should exit after stdin EOF");
    let connector_status = connector
        .wait()
        .expect("connect control receiver should exit after socket close");
    stdout_thread
        .join()
        .expect("stdout collection should not panic");
    stderr_thread
        .join()
        .expect("stderr collection should not panic");
    let output = combined_output();

    assert!(
        listener_status.success(),
        "listener should exit successfully"
    );
    assert!(
        connector_status.success(),
        "connector should exit successfully"
    );
    assert!(
        output.contains(r#"@response "LISTEN_OK""#),
        "listener output never contained control return payload: {output}",
    );
}

// =====================================================================
// one-shot CLI 入口 e2e: `rdog control <target> @<line>` 这种无状态形式
// 替代 `printf '@<line>\n' | rdog control <target>` 的常见组合。
// =====================================================================

#[test]
fn control_one_shot_should_send_ping_and_exit_for_tcp_lane() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    // 关键变化: `rdog control 127.0.0.1 <port> @ping` 一行结束,
    // 不再需要 `printf '@ping\n' | rdog control ...`。
    let output = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string(), "@ping"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control one-shot should exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(r#"@response "pong""#),
        "stdout did not contain @response pong: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn control_one_shot_should_send_ping_with_request_id() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let output = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string(), "@ping#7"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control one-shot should exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(r#"@response {"id":7,"value":"pong"}"#),
        "stdout did not contain id=7 pong response: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn control_one_shot_should_reject_at_line_without_target() {
    // 没有 target 但给了 one-shot line,应该报错并退出非 0。
    // 注: rdog 的错误日志当前默认走 stdout,所以既要检查 exit code
    // 也要在 stdout 里确认错误文案,不要假设错误一定在 stderr。
    let binary = rdog_binary_path();
    let output = Command::new(&binary)
        .args(["control", "@ping"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "control one-shot without target should fail"
    );
    assert!(
        combined.contains("one-shot line 需要 control 目标"),
        "stdout+stderr should explain missing target, got: {combined}"
    );
}

#[test]
fn control_one_shot_should_accept_two_at_lines_and_run_in_order_for_tcp_lane() {
    // 2 个 `@` line 必须被 clap `num_args = 0..=32` 接受,
    // 走和 N=1 / N>1 一样的 `send_control_lines_tcp` → `run_line_control_lines` 路径,
    // 共享同一条 TCP 连接,顺序串行执行。这条用例锁住 one-shot N=1 / N>1 已经统一
    // 走 `send_control_lines_*` 这条契约,不让 spec / test 再退回老设计。
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    // 一次发 2 条 line,共享同一条 TCP 连接,顺序串行
    let output = Command::new(&binary)
        .args([
            "control",
            "127.0.0.1",
            &port.to_string(),
            "@ping",
            "@capabilities#1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control 2 `@` lines should be accepted and exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 两条响应都在,且顺序为 1) ping 2) capabilities#1
    let pong_pos = stdout
        .find(r#"@response "pong""#)
        .expect("pong should appear");
    let caps_pos = stdout
        .find(r#"@response {"id":1,"value":{"capabilities""#)
        .expect("capabilities id=1 should appear");
    assert!(
        pong_pos < caps_pos,
        "responses should appear in input order; stdout={stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn control_multi_one_shot_should_run_lines_in_order_for_tcp_lane() {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    // 一次发 3 条 line,共享同一条 TCP 连接,顺序串行
    let output = Command::new(&binary)
        .args([
            "control",
            "127.0.0.1",
            &port.to_string(),
            "@ping",
            "@capabilities#1",
            r#"@cmd#7:"printf READY""#,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control multi one-shot should exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 三条响应都在,且顺序为 1) ping 2) capabilities 3) cmd#7 READY
    let pong_pos = stdout
        .find(r#"@response "pong""#)
        .expect("pong should appear");
    let caps_pos = stdout
        .find(r#"@response {"id":1,"value":{"capabilities""#)
        .expect("capabilities id=1 should appear");
    let ready_pos = stdout
        .find(r#"@response {"id":7,"value":"READY""#)
        .expect("cmd#7 READY should appear");
    assert!(
        pong_pos < caps_pos && caps_pos < ready_pos,
        "responses should appear in input order; stdout={stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn control_multi_one_shot_should_run_with_one_line_for_tcp_lane() {
    // 1 line 的多 line 形式应该等价于单 line one-shot
    let port = next_free_port();
    let binary = rdog_binary_path();
    let mut daemon = Command::new(&binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon control lane never started listening on port {port}",
    );

    let output = Command::new(&binary)
        .args(["control", "127.0.0.1", &port.to_string(), "@ping"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "control 1-line multi-form should exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(r#"@response "pong""#),
        "stdout did not contain pong: {stdout}"
    );

    daemon
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
}

#[test]
fn control_multi_one_shot_should_reject_at_line_in_middle_of_host() {
    // 中间夹 `@` 应当被 main.rs 拒绝
    let binary = rdog_binary_path();
    let output = Command::new(&binary)
        .args([
            "control",
            "127.0.0.1",
            "5555",
            "@ping",
            "extra",
            "@capabilities",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("control sender should run to completion");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "control with @ in middle of host should fail"
    );
    assert!(
        combined.contains("前面位置参数不应以 `@` 开头") || combined.contains("前面位置参数不应以"),
        "stderr+stdout should explain trailing-only @ rule, got: {combined}"
    );
}
