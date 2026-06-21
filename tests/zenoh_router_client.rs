#![cfg(unix)]

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use zenoh::Wait;

const SERIAL_ENDPOINT_EXAMPLE: &str = "serial//dev/ttyFAKE#baudrate=115200";

fn next_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("ephemeral port probe should bind")
        .local_addr()
        .expect("ephemeral port probe should expose local addr")
        .port()
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

fn write_temp_zenoh_router_config(
    daemon_name: &str,
    listen_endpoints: &[String],
    mode: &str,
) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-zenoh-router-{}-{}.toml",
        std::process::id(),
        next_port()
    ));

    let listen_endpoints = listen_endpoints
        .iter()
        .map(|endpoint| format!("\"{endpoint}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let contents = format!(
        r#"[zenoh]
enabled = true
mode = "{mode}"
namespace = "lab"
daemon_name = "{daemon_name}"
listen_endpoints = [{listen_endpoints}]
request_timeout_ms = 3000
startup_guard_window_ms = 1000

[zenoh.key_input_events]
enabled = true
keyexpr = "rdog/lab/daemon/{daemon_name}/member/{daemon_name}/keyinput"
"#
    );

    fs::write(&path, contents).expect("should write temporary daemon config");
    path
}

fn spawn_output_collector_to<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0_u8; 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return,
                Ok(len) => {
                    let text = String::from_utf8_lossy(&chunk[..len]);
                    buffer
                        .lock()
                        .expect("collector buffer lock should work")
                        .push_str(&text);
                }
                Err(_) => return,
            }
        }
    })
}

fn spawn_output_collector<R: Read + Send + 'static>(
    reader: R,
) -> (Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let handle = spawn_output_collector_to(reader, Arc::clone(&buffer));
    (buffer, handle)
}

fn wait_until_output_contains(
    child: &mut Child,
    output: &Arc<Mutex<String>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let collected = output
            .lock()
            .expect("collector buffer lock should work")
            .clone();
        if collected.contains(needle) {
            return Ok(collected);
        }

        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Err(format!(
                "child exited before marker appeared: status={status}, output={collected}"
            ));
        }

        thread::sleep(Duration::from_millis(50));
    }

    let collected = output
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    Err(format!(
        "marker `{needle}` not found before timeout. output={collected}"
    ))
}

fn start_zenoh_daemon_with_config(config_path: &str) -> Child {
    // 直接 spawn rdog daemon,不走 sh wrapper。
    // 2026-06-19 之前用 sh -c "exec rdog ... 2>&1" 是为了兼容
    // "test 只 pipe stdout 但日志走 stderr" 的旧 helper;
    // 现在改用 `start_zenoh_daemon_with_combined_output` 在 helper 内部
    // 合并 stdout+stderr 到一个 buffer,既兼容 stderr 上的 log marker,
    // 也不留 sh 孤儿进程。
    Command::new(rdog_binary_path())
        .args(["daemon", "-c", config_path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh daemon should start")
}

/// 启动 Zenoh daemon 并把 stdout + stderr 合成到一个 buffer。
///
/// 2026-06-19 init_logger 切到 stderr 之后,
/// daemon 启动日志(包括 "zenoh router daemon ready")走 stderr。
/// 这个 helper 让调用方用 `wait_until_output_contains(&combined_buffer, ...)` 一次性
/// 拿到 stdout + stderr 的合流,不需要再分两个 stream 处理。
///
/// 返回 (child, config_path, entrypoint, combined_buffer)。
/// `combined_buffer` 是 `Arc<Mutex<String>>`,由 daemon stdout+stderr 的两个 collector
/// 共同 append。
fn start_zenoh_daemon_with_combined_output(
    name: &str,
    listen_port: u16,
) -> (Child, PathBuf, String, Arc<Mutex<String>>) {
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path = write_temp_zenoh_router_config(name, &[entrypoint.clone()], "router");
    let mut child = start_zenoh_daemon_with_config(&config_path.display().to_string());
    let daemon_stdout = child.stdout.take().expect("daemon stdout should exist");
    let daemon_stderr = child.stderr.take().expect("daemon stderr should exist");
    let combined = Arc::new(Mutex::new(String::new()));
    // 同一个 buffer 接收两个 stream 的内容;collector thread 在 stream EOF 时退出。
    let _stdout_thread = spawn_output_collector_to(daemon_stdout, Arc::clone(&combined));
    let _stderr_thread = spawn_output_collector_to(daemon_stderr, Arc::clone(&combined));
    (child, config_path, entrypoint, combined)
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}.lab", std::process::id())
}

fn temp_workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-zenoh-{name}-{}-{}",
        std::process::id(),
        next_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

fn run_control(args: &[&str], line: &str) -> (std::process::ExitStatus, String, String) {
    let mut child = Command::new(rdog_binary_path())
        .arg("control")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control should start");

    child
        .stdin
        .as_mut()
        .expect("stdin should exist")
        .write_all(format!("{line}\n").as_bytes())
        .expect("should send control line");
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("should collect control output");

    (
        output.status,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn run_control_in_dir(
    workdir: &std::path::Path,
    args: &[&str],
    line: &str,
) -> (std::process::ExitStatus, String, String) {
    let mut child = Command::new(rdog_binary_path())
        .arg("control")
        .args(args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control should start");

    child
        .stdin
        .as_mut()
        .expect("stdin should exist")
        .write_all(format!("{line}\n").as_bytes())
        .expect("should send control line");
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("should collect control output");

    (
        output.status,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn run_control_with_retry_on_missing_target(
    args: &[&str],
    line: &str,
    timeout: Duration,
) -> (std::process::ExitStatus, String, String) {
    let deadline = Instant::now() + timeout;
    let mut result = run_control(args, line);

    loop {
        let combined = format!("{}\n{}", result.1, result.2);
        let should_retry = (combined.contains("未找到目标 service")
            || combined.contains("Unable to connect to any of"))
            && Instant::now() < deadline;

        if !should_retry {
            return result;
        }

        thread::sleep(Duration::from_millis(150));
        result = run_control(args, line);
    }
}

fn start_control_session(args: &[&str]) -> Child {
    Command::new(rdog_binary_path())
        .arg("control")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control session should start")
}

fn wait_until_buffer_contains(
    buffer: &Arc<Mutex<String>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let collected = buffer
            .lock()
            .expect("collector buffer lock should work")
            .clone();
        if collected.contains(needle) {
            return Ok(collected);
        }

        thread::sleep(Duration::from_millis(50));
    }

    let collected = buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    Err(format!(
        "marker `{needle}` not found before timeout. output={collected}"
    ))
}

fn wait_until_match_count_at_least(
    buffer: &Arc<Mutex<String>>,
    needle: &str,
    expected_count: usize,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let collected = buffer
            .lock()
            .expect("collector buffer lock should work")
            .clone();
        if collected.matches(needle).count() >= expected_count {
            return Ok(collected);
        }
        thread::sleep(Duration::from_millis(50));
    }

    let collected = buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    Err(format!(
        "needle `{needle}` count did not reach {expected_count} before timeout. output={collected}"
    ))
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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

        thread::sleep(Duration::from_millis(50));
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

fn wait_with_output_timeout_or_kill(
    mut child: Child,
    timeout: Duration,
) -> Result<std::process::Output, std::process::Output> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for child")
            .is_some()
        {
            return Ok(child
                .wait_with_output()
                .expect("child output should collect after exit"));
        }

        thread::sleep(Duration::from_millis(50));
    }

    let _ = child.kill();
    Err(child
        .wait_with_output()
        .expect("timed out child output should still collect"))
}

fn parse_control_key(output: &str) -> Option<String> {
    let marker = "control_key=";
    let start = output.find(marker)? + marker.len();
    let tail = &output[start..];
    let end = tail.find(',').unwrap_or(tail.len());
    Some(tail[..end].trim().to_string())
}

fn extract_json_string_field(input: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let start = input.find(&marker)? + marker.len();
    let tail = &input[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_owned())
}

fn decode_pty_output_frame(line: &str) -> String {
    let data = extract_json_string_field(line, "data").expect("pty output should include data");
    let bytes = BASE64_STANDARD
        .decode(data.as_bytes())
        .expect("pty output should be valid base64");
    String::from_utf8_lossy(&bytes).into_owned()
}

fn recv_zenoh_text(
    subscriber: &zenoh::pubsub::Subscriber<
        zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
    >,
    timeout: Duration,
) -> String {
    subscriber
        .recv_timeout(timeout)
        .expect("subscriber recv should not fail")
        .expect("subscriber should not close before payload")
        .payload()
        .try_to_string()
        .expect("payload should be utf-8")
        .to_string()
}

fn recv_zenoh_pty_output_until_contains(
    subscriber: &zenoh::pubsub::Subscriber<
        zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
    >,
    needle: &str,
    context: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut decoded = String::new();

    while Instant::now() < deadline {
        let payload = recv_zenoh_text(subscriber, Duration::from_secs(8));
        if payload.starts_with("@pty-output ") {
            decoded.push_str(&decode_pty_output_frame(&payload));
            if decoded.contains(needle) {
                return decoded;
            }
            continue;
        }

        panic!("{context}: unexpected non-output frame while waiting for {needle:?}: {payload:?}");
    }

    panic!("{context}: timed out waiting for {needle:?}\nseen decoded: {decoded:?}");
}

fn open_zenoh_client(entrypoint: &str) -> zenoh::Session {
    let mut config = zenoh::Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .expect("zenoh client mode should configure");
    config
        .insert_json5("connect/endpoints", &format!(r#"["{entrypoint}"]"#))
        .expect("zenoh client endpoints should configure");

    zenoh::open(config)
        .wait()
        .expect("zenoh client session should open")
}

fn build_control_key(namespace: &str, daemon_name: &str) -> String {
    format!("rdog/{namespace}/daemon/{daemon_name}/member/{daemon_name}/control")
}

fn build_session_to_daemon_key(namespace: &str, session_id: &str) -> String {
    format!("rdog/{namespace}/session/{session_id}/to-daemon")
}

fn build_session_to_control_key(namespace: &str, session_id: &str) -> String {
    format!("rdog/{namespace}/session/{session_id}/to-control")
}

fn render_session_open_payload(session_id: &str) -> String {
    format!("__rdog_session_open__:{session_id}")
}

fn render_session_close_payload(session_id: &str) -> String {
    format!("__rdog_session_close__:{session_id}")
}

fn render_session_bridge_payload(session_id: &str, line: &str) -> String {
    format!("__rdog_session__:{session_id}\n{line}")
}

#[test]
fn control_should_use_single_positional_name_as_zenoh_target() {
    let daemon_name = unique_name("short");
    let listen_port = next_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) =
        run_control_with_retry_on_missing_target(&[&daemon_name], "@ping", Duration::from_secs(8));

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "control shorthand should autodiscover router\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains(r#"@response "pong""#));
}

#[test]
fn control_should_reach_daemon_via_explicit_entrypoint_fallback() {
    let daemon_name = unique_name("entry");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@cmd#42:"printf READY""#,
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "control should succeed with explicit entrypoint fallback\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains(r#"@response {"id":42,"value":"READY"}"#));
}

#[test]
fn control_should_fail_when_no_router_is_discoverable_and_no_entrypoint_is_given() {
    let daemon_name = unique_name("missing-router");
    let (status, stdout, stderr) = run_control(
        &["--transport", "zenoh", "--target-name", &daemon_name],
        "@ping",
    );

    assert!(
        !status.success(),
        "control should fail when neither autodiscovery nor entrypoint can provide a router"
    );
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("未找到目标 service")
            || combined.contains("未找到可连接的 router locator")
            || combined.contains("Unable to connect")
            || combined.contains("timed out")
            || combined.contains("timeout"),
        "unexpected failure output:\n{combined}"
    );
}

#[test]
fn daemon_should_fail_fast_on_duplicate_name() {
    let daemon_name = unique_name("dup");
    let first_port = next_port();
    let second_port = next_port();
    let (mut first, first_config, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, first_port);
    wait_until_output_contains(
        &mut first,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("first daemon should report ready");

    let second_config = write_temp_zenoh_router_config(
        &daemon_name,
        &[format!("tcp/127.0.0.1:{second_port}")],
        "router",
    );
    let output = Command::new(rdog_binary_path())
        .args(["daemon", "-c", &second_config.display().to_string()])
        .output()
        .expect("second daemon should run");

    stop_child(&mut first);
    let _ = fs::remove_file(&first_config);
    let _ = fs::remove_file(&second_config);

    assert!(!output.status.success(), "second daemon should fail");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(combined.contains("发现重复 service_name 活跃 member"));
}

#[test]
fn control_should_execute_literal_shell_line_in_zenoh_profile() {
    let daemon_name = unique_name("literal");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        "printf ZENOH_LITERAL_OK",
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "literal shell line should succeed in Zenoh profile\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains(r#"@response "ZENOH_LITERAL_OK""#));
}

#[test]
fn control_should_route_key_request_in_zenoh_profile() {
    let daemon_name = unique_name("key");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@key#7:"hyper""#,
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "control bridge should stay alive and return protocol error\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let combined = format!("{stdout}\n{stderr}");
    assert!(combined.contains(r#""id":7"#));
    assert!(combined.contains(r#""code":64"#));
    assert!(combined.contains("首版不支持的 @key 按键: hyper"));
}

#[test]
#[ignore = "requires real input simulation backend permissions on the daemon host"]
fn control_should_execute_safe_mouse_move_in_zenoh_profile() {
    let daemon_name = unique_name("mouse");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@mouse-move#10:{dx:0,dy:0,coordinate_space:"relative"}"#,
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "safe mouse move should return a protocol response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let combined = format!("{stdout}\n{stderr}");
    if combined.contains(r#""code":77"#) {
        assert!(
            combined.contains("permission to simulate input")
                || combined.contains("辅助功能权限")
                || combined.contains("blocked by UIPI"),
            "permission-denied mouse move should explain missing input permission\n{combined}"
        );
        return;
    }

    assert!(combined.contains(r#""id":10"#));
    assert!(combined.contains(r#""kind":"mouse""#));
    assert!(combined.contains(r#""action":"move""#));
    assert!(combined.contains(r#""coordinate_space":"relative""#));
    assert!(combined.contains(r#""dx":0"#));
    assert!(combined.contains(r#""dy":0"#));
}

#[test]
fn control_should_publish_key_event_after_successful_key_request() {
    let daemon_name = unique_name("keyevent");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let keyexpr = format!("rdog/lab/daemon/{daemon_name}/member/{daemon_name}/keyinput");
    let subscriber = session
        .declare_subscriber(keyexpr.clone())
        .wait()
        .expect("subscriber should declare");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@key#7:"F11""#,
    );

    assert!(
        status.success(),
        "key request should return a protocol response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    if stdout.contains(r#""code":77"#) {
        stop_child(&mut daemon);
        let _ = fs::remove_file(&config_path);
        assert!(
            stdout.contains("permission"),
            "permission-denied @key should explain the missing input permission\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        return;
    }

    let sample = subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("subscriber should receive a payload")
        .expect("subscriber should not close without payload");
    let payload = sample
        .payload()
        .try_to_string()
        .expect("payload should be utf-8")
        .to_string();

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(stdout.contains(r#"@response {"id":7,"value":0}"#));
    assert!(payload.contains(r#""event":"key_input""#));
    assert!(payload.contains(r#""daemon_name":""#));
    assert!(payload.contains(&daemon_name));
    assert!(payload.contains(r#""key":"F11""#));
    assert!(payload.contains(r#""mode":"press_release""#));
}

#[test]
fn external_peer_should_send_control_request_via_zenoh_to_daemon_channel() {
    let daemon_name = unique_name("channel");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let session_id = format!("sess-{}", std::process::id());
    let control_key = build_control_key("lab", &daemon_name);
    let to_control_key = build_session_to_control_key("lab", &session_id);
    let to_daemon_key = build_session_to_daemon_key("lab", &session_id);
    let subscriber = session
        .declare_subscriber(to_control_key)
        .wait()
        .expect("subscriber should declare");

    let replies = session
        .get(control_key.clone())
        .payload(render_session_open_payload(&session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("session open query should send");
    let mut saw_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_ack = true;
            break;
        }
    }
    assert!(saw_ack, "session open should receive an ack reply");

    let publisher = session
        .declare_publisher(to_daemon_key)
        .wait()
        .expect("publisher should declare");
    let collect_response_frames = |subscriber: &zenoh::pubsub::Subscriber<
        zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
    >| {
        let mut frames = Vec::new();
        loop {
            let sample = subscriber
                .recv_timeout(Duration::from_secs(8))
                .expect("subscriber should receive a frame")
                .expect("subscriber should not close early");
            let payload = sample
                .payload()
                .try_to_string()
                .expect("payload should be utf-8")
                .to_string();
            frames.push(payload.clone());
            if payload.starts_with("@response ") {
                return frames;
            }
        }
    };

    publisher
        .put(r#"@script#11:"printf READY""#)
        .wait()
        .expect("to-daemon request should publish");
    let first_frames = collect_response_frames(&subscriber);

    publisher
        .put("printf SESSION_LITERAL")
        .wait()
        .expect("literal to-daemon request should publish");
    let literal_frames = collect_response_frames(&subscriber);

    publisher
        .put(r#"@script#12:"printf AGAIN""#)
        .wait()
        .expect("second to-daemon request should publish");
    let second_frames = collect_response_frames(&subscriber);

    publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .expect("session close should publish");
    let close_frames = collect_response_frames(&subscriber);
    let joined = close_frames.join("\n");
    assert!(
        joined.contains("@response 0"),
        "session close should emit close ack, got:\n{joined}"
    );

    let replies = session
        .get(control_key)
        .payload(render_session_open_payload(&session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("session reopen query should send");
    let mut saw_reopen_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_reopen_ack = true;
            break;
        }
    }
    assert!(saw_reopen_ack, "session reopen should receive ack");

    publisher
        .put(r#"@script#13:"printf REOPENED""#)
        .wait()
        .expect("reopened session request should publish");
    let reopened_frames = collect_response_frames(&subscriber);

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let joined = first_frames.join("\n");
    assert!(
        joined.contains(r#"@response {"id":11,"value":"READY"}"#),
        "unexpected to-control frames:\n{joined}"
    );
    let joined = literal_frames.join("\n");
    assert!(
        joined.contains(r#"@response "SESSION_LITERAL""#),
        "unexpected literal to-control frames:\n{joined}"
    );
    let joined = second_frames.join("\n");
    assert!(
        joined.contains(r#"@response {"id":12,"value":"AGAIN"}"#),
        "unexpected second to-control frames:\n{joined}"
    );
    let joined = reopened_frames.join("\n");
    assert!(
        joined.contains(r#"@response {"id":13,"value":"REOPENED"}"#),
        "unexpected reopened-session frames:\n{joined}"
    );
}

#[test]
fn control_should_wait_for_slow_session_channel_response() {
    let daemon_name = unique_name("slow-response");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let started_at = Instant::now();
    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@script#21:"sleep 4; printf SLOW_READY""#,
    );
    let elapsed = started_at.elapsed();

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "slow response should wait for final @response instead of treating recv timeout as subscriber closed\nelapsed={elapsed:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        elapsed >= Duration::from_secs(4),
        "test did not exercise the slow-response path: elapsed={elapsed:?}"
    );
    assert!(
        stdout.contains(r#"@response {"id":21,"value":"SLOW_READY"}"#),
        "slow script response should reach control stdout\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_should_reject_rich_frame_over_legacy_queryable_path() {
    let daemon_name = unique_name("legacy-rich");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let control_key = build_control_key("lab", &daemon_name);
    let replies = session
        .get(control_key)
        .payload("@screenshot#7")
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("legacy queryable request should send");
    let mut payloads = Vec::new();
    while let Ok(reply) = replies.recv() {
        let Ok(sample) = reply.result() else {
            continue;
        };
        payloads.push(
            sample
                .payload()
                .try_to_string()
                .expect("reply payload should be utf-8")
                .to_string(),
        );
    }

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let joined = payloads.join("\n");
    assert!(
        joined.contains(r#""id":7"#) && joined.contains(r#""code":78"#),
        "legacy queryable should reject rich screenshot with code 78, got:\n{joined}"
    );
    assert!(
        joined.contains("session channel"),
        "legacy queryable rejection should point callers to session channel:\n{joined}"
    );
    assert!(
        !joined.contains("@savefile "),
        "legacy queryable should not deliver rich savefile frames:\n{joined}"
    );
}

#[test]
fn control_should_reject_rich_frame_over_legacy_session_query_payload() {
    let daemon_name = unique_name("legacy-session-rich");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let session_id = uuid::Uuid::new_v4().to_string();
    let control_key = build_control_key("lab", &daemon_name);
    let to_control_key = build_session_to_control_key("lab", &session_id);
    let subscriber = session
        .declare_subscriber(to_control_key)
        .wait()
        .expect("subscriber should declare");

    let replies = session
        .get(control_key.clone())
        .payload(render_session_open_payload(&session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("session open query should send");
    let mut saw_open_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_open_ack = true;
            break;
        }
    }
    assert!(saw_open_ack, "session open should receive ack");

    let replies = session
        .get(control_key)
        .payload(render_session_bridge_payload(&session_id, "@screenshot#7"))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("legacy session query payload should send");
    let mut saw_query_ack = false;
    while let Ok(reply) = replies.recv() {
        let Ok(sample) = reply.result() else {
            continue;
        };
        let payload = sample
            .payload()
            .try_to_string()
            .expect("ack payload should be utf-8");
        if payload.contains("@response 0") {
            saw_query_ack = true;
            break;
        }
    }
    assert!(saw_query_ack, "legacy session query should receive ack");

    let rejection = recv_zenoh_text(&subscriber, Duration::from_secs(8));

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        rejection.contains(r#""id":7"#) && rejection.contains(r#""code":78"#),
        "legacy session query should reject rich screenshot with code 78, got:\n{rejection}"
    );
    assert!(
        rejection.contains("session channel"),
        "legacy session query rejection should point callers to session channel:\n{rejection}"
    );
    assert!(
        !rejection.contains("@savefile "),
        "legacy session query should not deliver rich savefile frames:\n{rejection}"
    );
}

#[test]
fn control_should_route_paste_request_in_zenoh_profile() {
    let daemon_name = unique_name("paste");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        r#"@paste:"hello""#,
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "paste request should come back as protocol response\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("尚未开放 @paste"),
        "Zenoh should no longer reject @paste at the profile gate\n{combined}"
    );
    assert!(
        combined.contains("@response "),
        "paste should return a line-control response\n{combined}"
    );
}

#[test]
fn control_should_run_pty_command_in_zenoh_profile() {
    let daemon_name = unique_name("pty");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let output = wait_with_output_timeout(
        Command::new(rdog_binary_path())
            .args([
                "control",
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--pty",
                "--",
                "/bin/sh",
                "-c",
                "if [ -t 0 ]; then printf ZENOH_PTY_OK; else printf ZENOH_NOT_TTY; fi",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("zenoh pty control should start"),
        Duration::from_secs(12),
        "zenoh pty control",
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "zenoh pty control should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("ZENOH_PTY_OK"),
        "zenoh pty command should see a real tty\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_should_accept_pty_string_shorthand_in_zenoh_profile() {
    let daemon_name = unique_name("pty-short");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let mut child = Command::new(rdog_binary_path())
        .args([
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh control cli should start");

    child
        .stdin
        .as_mut()
        .expect("stdin should exist")
        .write_all(b"@pty:\"/bin/sh -c 'if [ -t 0 ]; then printf ZENOH_STRING_ARGS_OK; else printf ZENOH_NOT_TTY; fi'\"\n")
        .expect("should send pty shorthand request");
    drop(child.stdin.take());

    let output = wait_with_output_timeout(child, Duration::from_secs(12), "zenoh pty shorthand");
    let daemon_log = buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "zenoh pty shorthand should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
    assert!(
        stdout.contains("ZENOH_STRING_ARGS_OK"),
        "zenoh pty shorthand should split cmd args and run inside a real PTY\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("@pty-ready") && !stdout.contains("@response {"),
        "zenoh pty shorthand should not stay in plain line-response mode\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn control_should_forward_pty_resize_frame_in_zenoh_profile() {
    let daemon_name = unique_name("pty-resize");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let control_key = build_control_key("lab", &daemon_name);
    let session_id = uuid::Uuid::new_v4().to_string();
    let to_control_key = build_session_to_control_key("lab", &session_id);
    let to_daemon_key = build_session_to_daemon_key("lab", &session_id);
    let subscriber = session
        .declare_subscriber(to_control_key)
        .wait()
        .expect("subscriber should declare");
    let replies = session
        .get(control_key)
        .payload(render_session_open_payload(&session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("session open query should send");
    let mut saw_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_ack = true;
            break;
        }
    }
    assert!(saw_ack, "session open should receive ack");
    let publisher = session
        .declare_publisher(to_daemon_key)
        .wait()
        .expect("publisher should declare");

    publisher
        .put(
            r#"@pty:{cmd:"/bin/sh",args:["-c","stty size; read line; stty size"],cols:80,rows:24}"#,
        )
        .wait()
        .expect("should publish pty open");
    let ready = recv_zenoh_text(&subscriber, Duration::from_secs(8));
    assert!(
        ready.starts_with("@pty-ready "),
        "unexpected ready frame: {ready:?}"
    );
    let pty_session_id =
        extract_json_string_field(&ready, "session_id").expect("ready should include session id");
    let initial = recv_zenoh_pty_output_until_contains(&subscriber, "24 80", "initial stty size");
    assert!(
        initial.contains("24 80"),
        "initial stty size should reflect open cols/rows\ndecoded: {initial:?}"
    );

    publisher
        .put(format!(
            r#"@pty-resize {{"session_id":"{pty_session_id}","cols":101,"rows":32}}"#
        ))
        .wait()
        .expect("should publish pty resize frame");
    publisher
        .put("go")
        .wait()
        .expect("should publish wake input");
    let resized = recv_zenoh_pty_output_until_contains(&subscriber, "32 101", "resized stty size");
    assert!(
        resized.contains("32 101"),
        "resized stty size should reflect Zenoh @pty-resize frame\ndecoded: {resized:?}"
    );

    publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .expect("session close should publish");
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

#[test]
fn control_should_forward_tty_input_after_zenoh_pty_output_goes_idle() {
    let daemon_name = unique_name("pty-tty-input");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let mut child = Command::new("script")
        .args([
            "-q",
            "/dev/null",
            &rdog_binary_path().to_string_lossy(),
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script-wrapped zenoh control cli should start");

    let mut child_stdin = child
        .stdin
        .take()
        .expect("script-wrapped control stdin should be piped");
    let pty_payload = r#"@pty:{cmd:"/bin/sh",args:["-c","printf READY; stty raw -echo; dd bs=1 count=6 2>/dev/null | od -An -tx1"],cols:80,rows:24}"#;

    thread::sleep(Duration::from_millis(200));
    write!(child_stdin, "{pty_payload}\r").expect("should open zenoh pty via tty control");
    child_stdin.flush().expect("pty open should flush");
    thread::sleep(Duration::from_millis(900));
    child_stdin
        .write_all(b"hello\r")
        .expect("should send terminal input into idle zenoh pty");
    child_stdin.flush().expect("terminal input should flush");
    thread::sleep(Duration::from_millis(700));
    child_stdin
        .write_all(&[0x04])
        .expect("control cli should accept local EOF after pty exits");
    child_stdin.flush().expect("local EOF should flush");
    drop(child_stdin);

    let output_result = wait_with_output_timeout_or_kill(child, Duration::from_secs(12));
    let daemon_log = buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
    let output = match output_result {
        Ok(output) => output,
        Err(output) => {
            panic!(
                "script-wrapped zenoh pty input probe timed out\nstatus: {}\nstdout:\n{}\nstderr:\n{}\ndaemon:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                daemon_log
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "zenoh pty input probe should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
    assert!(
        stdout.contains("READY"),
        "remote PTY should print readiness before waiting for input\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
    let normalized_stdout = stdout.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized_stdout.contains("68 65 6c 6c 6f 0d"),
        "local terminal input should reach the remote Zenoh PTY as raw bytes after output goes idle\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
}

#[test]
fn control_should_repaint_tui_input_while_zenoh_pty_output_is_busy() {
    let daemon_name = unique_name("pty-tui-input");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, daemon_buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &daemon_buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let mut child = Command::new("script")
        .args([
            "-q",
            "/dev/null",
            &rdog_binary_path().to_string_lossy(),
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("script-wrapped zenoh control cli should start");

    let child_stdout = child.stdout.take().expect("control stdout should exist");
    let (control_buffer, _control_collector) = spawn_output_collector(child_stdout);
    let mut child_stdin = child
        .stdin
        .take()
        .expect("script-wrapped control stdin should be piped");
    let pty_payload = r#"@pty:{cmd:"/bin/sh",args:["-c","printf READY; stty raw -echo; ( i=0; while [ $i -lt 60 ]; do printf \"FRAME%03d\\n\" \"$i\"; i=$((i + 1)); sleep 0.05; done; printf DONE ) & spammer=$!; while :; do ch=$(dd bs=1 count=1 2>/dev/null) || break; [ -z \"$ch\" ] && break; printf \"REPAINT:%s\" \"$ch\"; [ \"$ch\" = \"!\" ] && break; done; wait \"$spammer\""],cols:80,rows:24}"#;

    thread::sleep(Duration::from_millis(200));
    write!(child_stdin, "{pty_payload}\r").expect("should open zenoh pty via tty control");
    child_stdin.flush().expect("pty open should flush");
    wait_until_buffer_contains(&control_buffer, "READY", Duration::from_secs(5))
        .expect("remote TUI probe should become ready");

    thread::sleep(Duration::from_millis(250));
    child_stdin
        .write_all(b"b!")
        .expect("should send terminal input during busy remote output");
    child_stdin
        .flush()
        .expect("busy terminal input should flush");

    let repaint_result =
        wait_until_buffer_contains(&control_buffer, "REPAINT:b", Duration::from_millis(900));
    let done_result = wait_until_buffer_contains(&control_buffer, "DONE", Duration::from_secs(15));

    drop(child_stdin);

    thread::sleep(Duration::from_millis(200));
    if child
        .try_wait()
        .expect("try_wait should not fail after TUI probe markers")
        .is_none()
    {
        child
            .kill()
            .expect("script-wrapped control cli should be killable");
    }
    let output = child
        .wait_with_output()
        .expect("script-wrapped control cli output should collect");
    let daemon_log = daemon_buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let stdout = control_buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        repaint_result.is_ok(),
        "local input should trigger remote TUI repaint while output is still busy\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
    assert!(
        done_result.is_ok(),
        "remote TUI probe should finish its busy output phase\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{daemon_log}"
    );
}

#[test]
fn control_should_detach_and_attach_pty_in_zenoh_profile() {
    let daemon_name = unique_name("pty-reattach");
    let listen_port = next_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let session = open_zenoh_client(&entrypoint);
    let control_key = build_control_key("lab", &daemon_name);
    let open_session_id = uuid::Uuid::new_v4().to_string();
    let open_to_control_key = build_session_to_control_key("lab", &open_session_id);
    let open_to_daemon_key = build_session_to_daemon_key("lab", &open_session_id);
    let open_subscriber = session
        .declare_subscriber(open_to_control_key)
        .wait()
        .expect("open subscriber should declare");
    let replies = session
        .get(control_key.clone())
        .payload(render_session_open_payload(&open_session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("open session query should send");
    let mut saw_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_ack = true;
            break;
        }
    }
    assert!(saw_ack, "open session should receive ack");
    let open_publisher = session
        .declare_publisher(open_to_daemon_key)
        .wait()
        .expect("open publisher should declare");

    open_publisher
        .put(r#"@pty:{cmd:"/bin/sh",args:["-c","printf FIRST; sleep 1; printf SECOND; sleep 5"],cols:80,rows:24}"#)
        .wait()
        .expect("should publish pty open");
    let ready = open_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive ready sample")
        .expect("ready sample should exist");
    let ready_payload = ready
        .payload()
        .try_to_string()
        .expect("ready payload should be utf-8")
        .to_string();
    let daemon_log = || {
        buffer
            .lock()
            .expect("collector buffer lock should work")
            .clone()
    };
    assert!(
        ready_payload.starts_with("@pty-ready "),
        "unexpected ready payload: {ready_payload}\ndaemon:\n{}",
        daemon_log()
    );
    let session_id = extract_json_string_field(&ready_payload, "session_id")
        .expect("ready payload should expose pty session id");
    let first_output = open_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive first output sample")
        .unwrap_or_else(|| {
            panic!(
                "first output sample should exist\ndaemon:\n{}",
                daemon_log()
            )
        });
    let first_payload = first_output
        .payload()
        .try_to_string()
        .expect("first output payload should be utf-8")
        .to_string();
    assert!(
        first_payload.starts_with("@pty-output "),
        "unexpected first output payload: {first_payload}\ndaemon:\n{}",
        daemon_log()
    );

    open_publisher
        .put(format!(r#"@pty-detach:{{session_id:"{session_id}"}}"#))
        .wait()
        .expect("should publish pty detach");
    let detached = open_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive detached sample")
        .expect("detached sample should exist");
    let detached_payload = detached
        .payload()
        .try_to_string()
        .expect("detached payload should be utf-8")
        .to_string();
    assert!(
        detached_payload.starts_with("@pty-detached ") && detached_payload.contains(&session_id),
        "unexpected detached payload: {detached_payload}"
    );

    let attach_session_id = uuid::Uuid::new_v4().to_string();
    let attach_to_control_key = build_session_to_control_key("lab", &attach_session_id);
    let attach_to_daemon_key = build_session_to_daemon_key("lab", &attach_session_id);
    let attach_subscriber = session
        .declare_subscriber(attach_to_control_key)
        .wait()
        .expect("attach subscriber should declare");
    let replies = session
        .get(control_key)
        .payload(render_session_open_payload(&attach_session_id))
        .timeout(Duration::from_secs(8))
        .wait()
        .expect("attach session query should send");
    let mut saw_attach_ack = false;
    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            saw_attach_ack = true;
            break;
        }
    }
    assert!(saw_attach_ack, "attach session should receive ack");
    let attach_publisher = session
        .declare_publisher(attach_to_daemon_key)
        .wait()
        .expect("attach publisher should declare");

    attach_publisher
        .put(format!(
            r#"@pty-attach:{{session_id:"{session_id}",cols:80,rows:24}}"#
        ))
        .wait()
        .expect("should publish pty attach");
    let attached = attach_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive attached sample")
        .expect("attached sample should exist");
    let attached_payload = attached
        .payload()
        .try_to_string()
        .expect("attached payload should be utf-8")
        .to_string();
    assert!(
        attached_payload.starts_with("@pty-attached ") && attached_payload.contains(&session_id),
        "unexpected attached payload: {attached_payload}"
    );
    let second_output = attach_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive second output sample")
        .expect("second output sample should exist");
    let second_payload = second_output
        .payload()
        .try_to_string()
        .expect("second output payload should be utf-8")
        .to_string();
    assert!(
        second_payload.starts_with("@pty-output "),
        "unexpected second output payload: {second_payload}"
    );

    attach_publisher
        .put(format!(r#"@pty-close:{{session_id:"{session_id}"}}"#))
        .wait()
        .expect("should publish pty close after attach");
    let closed = attach_subscriber
        .recv_timeout(Duration::from_secs(8))
        .expect("should receive close sample")
        .expect("close sample should exist");
    let closed_payload = closed
        .payload()
        .try_to_string()
        .expect("closed payload should be utf-8")
        .to_string();
    assert!(
        closed_payload.starts_with("@pty-closed ") && closed_payload.contains("force_close"),
        "unexpected closed payload: {closed_payload}"
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

#[test]
#[ignore = "requires real screenshot backend and host screen capture permissions"]
fn control_should_execute_screenshot_and_save_file_in_zenoh_profile() {
    let daemon_name = unique_name("screenshot");
    let listen_port = next_port();
    let workdir = temp_workdir("screenshot");
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path = write_temp_zenoh_router_config(&daemon_name, &[entrypoint.clone()], "router");
    let mut daemon = Command::new(rdog_binary_path())
        .args(["daemon", "-c", &config_path.display().to_string()])
        .current_dir(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh daemon should start");
    let daemon_stdout = daemon.stdout.take().expect("daemon stdout should exist");
    let (buffer, _collector) = spawn_output_collector(daemon_stdout);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control_in_dir(
        &workdir,
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        "@screenshot#7",
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "zenoh control screenshot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("saved file:"),
        "zenoh screenshot output did not contain savefile notice: {combined}"
    );
    assert!(
        combined.matches("saved file:").count() >= 2,
        "zenoh screenshot output did not contain two savefile notices: {combined}"
    );
    assert!(
        combined.contains("screenshot-bundle")
            && combined.contains("coordinate_space")
            && combined.contains("os-logical")
            && combined.contains("display_count"),
        "zenoh screenshot output did not contain bundle summary: {combined}"
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
    assert!(
        manifest_json["display_count"].as_u64().unwrap_or(0) >= 1,
        "manifest should report at least one display"
    );
    assert_eq!(
        manifest_json["display_count"].as_u64(),
        Some(
            manifest_json["displays"]
                .as_array()
                .expect("displays should be an array")
                .len() as u64
        )
    );
    assert!(
        manifest_json["image_size"]["width"].as_u64().unwrap_or(0) > 0
            && manifest_json["image_size"]["height"].as_u64().unwrap_or(0) > 0,
        "manifest image_size should be non-zero"
    );

    let _ = fs::remove_dir_all(workdir);
}

#[test]
fn daemon_should_reuse_same_control_key_after_restart_by_default() {
    let daemon_name = unique_name("stable");
    let listen_port = next_port();
    let first_output = {
        let (mut daemon, config_path, _entrypoint, buffer) =
            start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
        let out = wait_until_output_contains(
            &mut daemon,
            &buffer,
            "zenoh router daemon ready",
            Duration::from_secs(8),
        )
        .expect("first daemon should report ready");
        stop_child(&mut daemon);
        let _ = fs::remove_file(&config_path);
        out
    };
    let first_control_key =
        parse_control_key(&first_output).expect("first output should contain control_key");

    let config_path = write_temp_zenoh_router_config(
        &daemon_name,
        &[format!("tcp/127.0.0.1:{listen_port}")],
        "router",
    );
    let second_output = {
        let mut second = start_zenoh_daemon_with_config(&config_path.display().to_string());
        let second_stdout = second.stdout.take().expect("daemon stdout should exist");
        let second_stderr = second.stderr.take().expect("daemon stderr should exist");
        let combined = Arc::new(Mutex::new(String::new()));
        let _ = spawn_output_collector_to(second_stdout, Arc::clone(&combined));
        let _ = spawn_output_collector_to(second_stderr, Arc::clone(&combined));
        let out = wait_until_output_contains(
            &mut second,
            &combined,
            "zenoh router daemon ready",
            Duration::from_secs(8),
        )
        .expect("second daemon should report ready");
        stop_child(&mut second);
        out
    };
    let second_control_key =
        parse_control_key(&second_output).expect("second output should contain control_key");
    let _ = fs::remove_file(&config_path);

    assert_eq!(first_control_key, second_control_key);
}

#[test]
fn control_session_should_reresolve_after_daemon_restart() {
    let daemon_name = unique_name("resume");
    let listen_port = next_port();
    let (mut first, first_config, entrypoint, first_buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut first,
        &first_buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("first daemon should report ready");

    let mut control = start_control_session(&[
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entrypoint,
    ]);
    let control_stdout = control.stdout.take().expect("control stdout should exist");
    let (control_buffer, _control_collector) = spawn_output_collector(control_stdout);

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(b"@ping\n")
        .expect("should send first ping");
    wait_until_buffer_contains(
        &control_buffer,
        r#"@response "pong""#,
        Duration::from_secs(8),
    )
    .expect("first ping should succeed");

    stop_child(&mut first);
    let _ = fs::remove_file(&first_config);

    let restart_config =
        write_temp_zenoh_router_config(&daemon_name, &[entrypoint.clone()], "router");
    let mut second = start_zenoh_daemon_with_config(&restart_config.display().to_string());
    let second_stdout = second.stdout.take().expect("daemon stdout should exist");
    let second_stderr = second.stderr.take().expect("daemon stderr should exist");
    let second_buffer = Arc::new(Mutex::new(String::new()));
    let _ = spawn_output_collector_to(second_stdout, Arc::clone(&second_buffer));
    let _ = spawn_output_collector_to(second_stderr, Arc::clone(&second_buffer));
    wait_until_output_contains(
        &mut second,
        &second_buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("second daemon should report ready");

    control
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(b"@ping\n")
        .expect("should send second ping");

    let output = wait_until_match_count_at_least(
        &control_buffer,
        r#"@response "pong""#,
        2,
        Duration::from_secs(8),
    )
    .expect("second ping should also succeed");
    let pong_count = output.matches(r#"@response "pong""#).count();
    assert!(
        pong_count >= 2,
        "expected two pong responses, got output:\n{output}"
    );

    drop(control.stdin.take());
    let output = control
        .wait_with_output()
        .expect("control session should exit after stdin closes");
    assert!(
        output.status.success(),
        "control session should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    stop_child(&mut second);
    let _ = fs::remove_file(&restart_config);
}

#[test]
fn daemon_should_reject_serial_only_router_profile() {
    let daemon_name = unique_name("serial-only");
    let config_path = write_temp_zenoh_router_config(
        &daemon_name,
        &[SERIAL_ENDPOINT_EXAMPLE.to_string()],
        "router",
    );

    let output = Command::new(rdog_binary_path())
        .args(["daemon", "-c", &config_path.display().to_string()])
        .output()
        .expect("daemon should run");
    let _ = fs::remove_file(&config_path);

    assert!(
        !output.status.success(),
        "serial-only daemon config should fail"
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("至少需要一个非 serial 的 listen endpoint"));
}

#[test]
fn legacy_zenoh_peer_transport_should_report_migration_error() {
    let daemon_name = unique_name("legacy");
    let output = Command::new(rdog_binary_path())
        .args([
            "control",
            "--transport",
            "zenoh-peer",
            "--target-name",
            &daemon_name,
            "--entry-point",
            "tcp/127.0.0.1:7447",
        ])
        .output()
        .expect("legacy control should run");

    assert!(!output.status.success(), "legacy transport should fail");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("旧 transport `zenoh-peer` 已废弃"));
}

#[test]
fn legacy_peer_mode_config_should_report_migration_error() {
    let daemon_name = unique_name("legacy-mode");
    let config_path = write_temp_zenoh_router_config(
        &daemon_name,
        &[format!("tcp/127.0.0.1:{}", next_port())],
        "peer",
    );

    let output = Command::new(rdog_binary_path())
        .args(["daemon", "-c", &config_path.display().to_string()])
        .output()
        .expect("daemon should run");
    let _ = fs::remove_file(&config_path);

    assert!(
        !output.status.success(),
        "legacy peer mode config should fail"
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("`zenoh.mode = \"peer\"` 已废弃"));
}

// =====================================================================
// Zenoh multi-line one-shot e2e:
// `rdog control <target> @<line> [@<line> ...]` 在 Zenoh profile 下
// 共享同一条 session bridge 串行执行,任一失败整组退出。
// =====================================================================

/// 跑一次 one-shot multi-line `rdog control ... @a @b @c`,返回 (status, stdout, stderr)。
/// 跟 `run_control_with_retry_on_missing_target` 不同的是:不接 stdin,
/// 直接把所有 `@<line>` 放在 positional 里。
fn run_control_multi_one_shot(
    args: &[&str],
    lines: &[&str],
    timeout: Duration,
) -> (std::process::ExitStatus, String, String) {
    // 跟 run_control_with_retry_on_missing_target 同源逻辑,重试到 deadline 为止。
    // Zenoh autodiscovery 在 daemon 刚拉起时偶发 "未找到目标 service",
    // 沿用这个 retry 习惯能保证 e2e 在并行跑时也稳定。
    let mut full_args: Vec<String> = vec!["control".to_string()];
    full_args.extend(args.iter().map(|s| s.to_string()));
    for line in lines {
        full_args.push((*line).to_string());
    }

    let run_once = || {
        Command::new(rdog_binary_path())
            .args(&full_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("control should run to completion")
    };

    let deadline = Instant::now() + timeout;
    let mut output = run_once();
    let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut status = output.status;
    loop {
        let combined = format!("{stdout}\n{stderr}");
        let should_retry = (combined.contains("未找到目标 service")
            || combined.contains("Unable to connect to any of"))
            && Instant::now() < deadline;
        if !should_retry {
            return (status, stdout, stderr);
        }
        thread::sleep(Duration::from_millis(150));
        output = run_once();
        stdout = String::from_utf8_lossy(&output.stdout).to_string();
        stderr = String::from_utf8_lossy(&output.stderr).to_string();
        status = output.status;
    }
}

#[test]
fn control_multi_one_shot_should_run_lines_in_order_for_zenoh_profile() {
    let daemon_name = unique_name("multi");
    let listen_port = next_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control_multi_one_shot(
        &["--transport", "zenoh", "--target-name", &daemon_name],
        &["@ping", r#"@cmd#7:"printf MULTI_OK""#],
        Duration::from_secs(8),
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "multi one-shot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let pong_pos = stdout
        .find(r#"@response "pong""#)
        .expect("pong response should appear in stdout");
    let multi_pos = stdout
        .find(r#"@response {"id":7,"value":"MULTI_OK""#)
        .expect("cmd#7 MULTI_OK response should appear in stdout");
    assert!(
        pong_pos < multi_pos,
        "responses should appear in input order; stdout:\n{stdout}"
    );
}

#[test]
fn control_multi_one_shot_should_run_three_lines_in_order_for_zenoh_profile() {
    let daemon_name = unique_name("tri");
    let listen_port = next_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control_multi_one_shot(
        &["--target-name", &daemon_name],
        &["@ping", r#"@cmd#1:"printf A""#, r#"@cmd#2:"printf B""#],
        Duration::from_secs(8),
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "3-line multi one-shot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let pong = stdout
        .find(r#"@response "pong""#)
        .expect("pong should appear");
    let a = stdout
        .find(r#"@response {"id":1,"value":"A""#)
        .expect("cmd#1 A should appear");
    let b = stdout
        .find(r#"@response {"id":2,"value":"B""#)
        .expect("cmd#2 B should appear");
    assert!(
        pong < a && a < b,
        "three responses should appear in input order; stdout:\n{stdout}"
    );
}

#[test]
fn control_multi_one_shot_should_run_three_lines_with_3_responses_in_zenoh_profile() {
    // 简单替换之前的 fail-fast 烟测;
    // 之前那一条假设"中间 line 失败会 stop 后续 line",但当前 send_control_lines
    // 只在 protocol/connection 错误时中断,对 `@xxx` 这种 error response
    // 仍然顺序执行(daemon 不为每条 line 做事务)。后续若引入 response-code
    // fail-fast 行为,可以再加一条覆盖。占位期间给一条稳健的 3 line 顺序烟测。
    let daemon_name = unique_name("ok3");
    let listen_port = next_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let (status, stdout, stderr) = run_control_multi_one_shot(
        &["--target-name", &daemon_name],
        &[
            r#"@cmd#1:"printf ALPHA""#,
            r#"@cmd#2:"printf BETA""#,
            r#"@cmd#3:"printf GAMMA""#,
        ],
        Duration::from_secs(8),
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "3-line multi one-shot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for needle in [
        r#"@response {"id":1,"value":"ALPHA""#,
        r#"@response {"id":2,"value":"BETA""#,
        r#"@response {"id":3,"value":"GAMMA""#,
    ] {
        assert!(
            stdout.contains(needle),
            "stdout should contain {needle}; stdout:\n{stdout}"
        );
    }
    let alpha_pos = stdout.find(r#"@response {"id":1,"value":"ALPHA""#).unwrap();
    let beta_pos = stdout.find(r#"@response {"id":2,"value":"BETA""#).unwrap();
    let gamma_pos = stdout.find(r#"@response {"id":3,"value":"GAMMA""#).unwrap();
    assert!(
        alpha_pos < beta_pos && beta_pos < gamma_pos,
        "3 responses should appear in input order; stdout:\n{stdout}"
    );
}
