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
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
    let (output_buffer, output_thread) = spawn_output_collector(listener_stdout);

    assert!(
        wait_until_output_contains(
            &output_buffer,
            "Connection Received",
            Duration::from_secs(5)
        ),
        "listener never reported Connection Received. output so far:\n{}",
        output_buffer.lock().expect("buffer lock should work"),
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
        wait_until_output_contains(
            &output_buffer,
            r#"@response "LISTEN_OK""#,
            Duration::from_secs(5)
        ),
        "listener never printed control return payload. output so far:\n{}",
        output_buffer.lock().expect("buffer lock should work"),
    );

    drop(listener.stdin.take());

    let listener_status = listener
        .wait()
        .expect("listener should exit after stdin EOF");
    let connector_status = connector
        .wait()
        .expect("connect control receiver should exit after socket close");
    output_thread
        .join()
        .expect("output collection should not panic");
    let output = output_buffer
        .lock()
        .expect("buffer lock should work")
        .clone();

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
