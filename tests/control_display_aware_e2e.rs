#![cfg(target_os = "macos")]

use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const FIXTURE_SOURCE: &str = "tests/fixtures/macos_display_aware_fixture.swift";
const FIXTURE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const FIXTURE_EXIT_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(20);

// -----------------------------------------------------------------------------
// 子进程生命周期
// -----------------------------------------------------------------------------

struct FixtureGuard {
    child: Option<Child>,
}

impl FixtureGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn terminate(mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pid = child.id().to_string();
        let _ = Command::new("kill").args(["-TERM", &pid]).status();

        let deadline = Instant::now() + FIXTURE_EXIT_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    assert!(
                        status.success(),
                        "display-aware fixture 应该响应 SIGTERM 并干净退出: {status}"
                    );
                    return;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("display-aware fixture 在 SIGTERM 后没有及时退出");
                }
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("读取 display-aware fixture 退出状态失败: {err}");
                }
            }
        }
    }
}

impl Drop for FixtureGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct DaemonGuard {
    child: Child,
    workdir: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.workdir);
    }
}

// -----------------------------------------------------------------------------
// Fixture 编译与启动
// -----------------------------------------------------------------------------

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn compile_fixture() -> PathBuf {
    let output_dir = manifest_dir().join("target/display-aware-e2e");
    fs::create_dir_all(&output_dir).expect("应该能创建 display-aware E2E 输出目录");
    let binary = output_dir.join("macos-display-aware-fixture");
    let source = manifest_dir().join(FIXTURE_SOURCE);

    let output = Command::new("xcrun")
        .args(["swiftc", "-framework", "AppKit"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("应该能调用 xcrun swiftc 编译 display-aware fixture");
    assert!(
        output.status.success(),
        "fixture 编译失败:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn start_fixture(binary: &Path, required_displays: usize) -> (FixtureGuard, Value) {
    let mut child = Command::new(binary)
        .args(["--require-displays", &required_displays.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("应该能启动 display-aware fixture");
    let stdout = child
        .stdout
        .take()
        .expect("fixture stdout 应该已经配置为 pipe");
    let guard = FixtureGuard::new(child);

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });

    let line = receiver
        .recv_timeout(FIXTURE_READY_TIMEOUT)
        .unwrap_or_else(|_| panic!("fixture 在 {FIXTURE_READY_TIMEOUT:?} 内没有输出 ready JSON"))
        .expect("读取 fixture ready JSON 应该成功");
    let value: Value = serde_json::from_str(&line)
        .unwrap_or_else(|err| panic!("fixture ready 输出不是合法 JSON: {err}; line={line:?}"));
    assert_eq!(
        value["status"], "ready",
        "fixture 没有进入 ready 状态: {value}"
    );
    (guard, value)
}

fn rdog_binary_path() -> PathBuf {
    if let Some(path) = std::env::var_os("RDOG_DISPLAY_AWARE_E2E_BINARY") {
        let binary = PathBuf::from(path);
        assert!(binary.exists(), "E2E binary 不存在: {}", binary.display());
        return binary;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_rdog") {
        return PathBuf::from(path);
    }
    let current_exe = std::env::current_exe().expect("应该能读取当前 test binary 路径");
    let debug_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("integration test 应该位于 target/debug/deps");
    let binary = debug_dir.join("rdog");
    assert!(
        binary.exists(),
        "预期 rdog binary 位于 {}",
        binary.display()
    );
    binary
}

fn next_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("应该能分配临时 TCP 端口");
    listener.local_addr().expect("listener 应该有地址").port()
}

fn start_daemon(binary: &Path) -> (DaemonGuard, u16) {
    let port = next_free_port();
    let workdir = std::env::temp_dir().join(format!(
        "rdog-display-aware-daemon-{}-{port}",
        std::process::id()
    ));
    fs::create_dir_all(&workdir).expect("应该能创建 daemon 临时目录");
    let child = Command::new(binary)
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
        .expect("应该能启动 rdog test daemon");
    let mut guard = DaemonGuard { child, workdir };
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if guard
            .child
            .try_wait()
            .expect("读取 daemon 状态不应该失败")
            .is_some()
        {
            panic!("rdog test daemon 在监听前提前退出");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return (guard, port);
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("rdog test daemon 在 8 秒内没有监听端口 {port}");
}

fn run_control_command(binary: &Path, port: u16, line: &str) -> Value {
    let output = run_control_command_output(binary, port, line, None);
    latest_response_value(&String::from_utf8_lossy(&output.stdout))
}

fn run_control_command_output(
    binary: &Path,
    port: u16,
    line: &str,
    current_dir: Option<&Path>,
) -> Output {
    let label = format!("rdog control line={line}");
    let mut command = Command::new(binary);
    command.args(["control", "127.0.0.1", &port.to_string(), line]);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map(|child| wait_with_output_timeout(child, CONTROL_TIMEOUT, &label))
        .expect("应该能启动 rdog control");
    assert!(
        output.status.success(),
        "rdog control 失败:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn wait_with_output_timeout(mut child: Child, timeout: Duration, label: &str) -> Output {
    let stdout = child.stdout.take().expect("child stdout 应该已经 pipe");
    let stderr = child.stderr.take().expect("child stderr 应该已经 pipe");
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        BufReader::new(stdout)
            .read_to_end(&mut bytes)
            .expect("应该能读取 child stdout");
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        BufReader::new(stderr)
            .read_to_end(&mut bytes)
            .expect("应该能读取 child stderr");
        bytes
    });
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("读取 child 状态不应该失败")
            .is_some()
        {
            let status = child.wait().expect("应该能回收 child");
            return Output {
                status,
                stdout: stdout_reader.join().expect("stdout reader 不应该 panic"),
                stderr: stderr_reader.join().expect("stderr reader 不应该 panic"),
            };
        }
        thread::sleep(Duration::from_millis(20));
    }

    let _ = child.kill();
    let status = child.wait().expect("timeout 后应该能回收 child");
    let output = Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader 不应该 panic"),
        stderr: stderr_reader.join().expect("stderr reader 不应该 panic"),
    };
    panic!(
        "{label} timeout:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn latest_response_value(stdout: &str) -> Value {
    let response = stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("@response "))
        .unwrap_or_else(|| panic!("control stdout 缺少 @response: {stdout}"));
    let envelope: Value =
        serde_json::from_str(response).expect("@response payload 应该是合法 JSON");
    assert!(
        envelope.get("code").is_none(),
        "control 返回结构化错误: {envelope}"
    );
    envelope.get("value").cloned().unwrap_or(envelope)
}

fn find_fixture_window(binary: &Path, port: u16, title: &str, request_id: u64) -> Value {
    // -------------------------------------------------------------------------
    // AXWindows 的数组顺序没有创建顺序契约。测试必须通过 fixture 的唯一标题
    // 解析真实 window identity,不能根据 ready payload 的数组下标猜 AX window index。
    // -------------------------------------------------------------------------
    let payload = serde_json::json!({
        "title": title,
        "limit": 5,
        "include_state": true,
        "include_recipes": false,
    });
    let response = run_control_command(
        binary,
        port,
        &format!("@window-find#{request_id}:{payload}"),
    );
    assert_eq!(
        response["match_count"], 1,
        "fixture title必须唯一定位一个窗口: title={title}; response={response}"
    );

    let observation_id = response["observation"]["observation_id"]
        .as_str()
        .expect("window-find 应该返回 observation_id");
    assert!(
        !observation_id.is_empty(),
        "window-find observation_id 不能为空"
    );

    let window = response["matches"]
        .as_array()
        .and_then(|matches| matches.first())
        .expect("window-find 应该返回唯一 match");
    assert!(
        window["window_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "fixture window应该返回合法window_id: {window}"
    );
    assert!(
        window["ref"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "fixture window应该返回observation ref: {window}"
    );
    assert!(
        window["rect"].is_object(),
        "fixture window应该返回rect: {window}"
    );
    response
}

fn fixture_window_point(window_find: &Value) -> (i32, i32) {
    let rect = &window_find["matches"][0]["rect"];
    let x = rect["x"].as_i64().expect("window-find rect.x应该是整数") as i32 + 10;
    let y = rect["y"].as_i64().expect("window-find rect.y应该是整数") as i32 + 10;
    (x, y)
}

// -----------------------------------------------------------------------------
// Live fixture contract
// -----------------------------------------------------------------------------

#[test]
#[ignore = "requires a visible macOS desktop with at least two active displays"]
fn macos_fixture_should_create_one_deterministic_window_per_display() {
    let binary = compile_fixture();
    let (fixture, ready) = start_fixture(&binary, 2);

    let displays = ready["displays"]
        .as_array()
        .expect("fixture ready 应该包含 displays array");
    let windows = ready["windows"]
        .as_array()
        .expect("fixture ready 应该包含 windows array");

    assert!(displays.len() >= 2, "双屏 E2E 必须存在至少两块 display");
    assert_eq!(windows.len(), 2, "fixture 应该创建两个窗口");
    assert_eq!(windows[0]["title"], "rdog-display-aware-d1");
    assert_eq!(windows[0]["display_index"], 1);
    assert_eq!(windows[1]["title"], "rdog-display-aware-d2");
    assert_eq!(windows[1]["display_index"], 2);
    assert_eq!(
        windows[0]["button_accessibility_id"],
        "display-aware-button-1"
    );
    assert_eq!(
        windows[1]["button_accessibility_id"],
        "display-aware-button-2"
    );

    fixture.terminate();
}

#[test]
#[ignore = "requires a visible dual-display macOS desktop and Accessibility permission"]
fn daemon_should_guard_and_verify_activation_on_second_display() {
    let fixture_binary = compile_fixture();
    let (fixture, _ready) = start_fixture(&fixture_binary, 2);
    let rdog_binary = rdog_binary_path();
    let (_daemon, port) = start_daemon(&rdog_binary);

    let ping = run_control_command(&rdog_binary, port, "@ping#1");
    assert_eq!(ping, "pong", "control lane 必须先通过 liveness 验证");

    let second_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d2", 2);
    let window_id = second_window["matches"][0]["window_id"]
        .as_str()
        .expect("第二屏fixture window应该返回window_id");
    let (guard_x, guard_y) = fixture_window_point(&second_window);

    let activate_payload = serde_json::json!({
        "window_id": window_id,
        "guard": {"display": {"contains_point": {"x": guard_x, "y": guard_y}}},
        "verify": {"focused": true, "timeout_ms": 3000, "poll_interval_ms": 75},
    });
    let activate = run_control_command(
        &rdog_binary,
        port,
        &format!("@window-activate#3:{activate_payload}"),
    );
    assert_eq!(
        activate["status"], "ok",
        "activate 必须完成焦点验证: {activate}"
    );
    assert_eq!(activate["verify"]["status"], "passed");
    assert_eq!(activate["verify"]["focused"], true);
    assert_eq!(activate["verify"]["frontmost"], true);
    assert!(activate["guard"]["resolved"]["display_id"].is_string());

    let reobserve = run_control_command(
        &rdog_binary,
        port,
        r#"@window-find#4:{title:"rdog-display-aware-d2",limit:5,include_state:true,include_recipes:false}"#,
    );
    assert_eq!(reobserve["matches"][0]["app"]["frontmost"], true);
    assert_eq!(reobserve["matches"][0]["state"]["interactable"], true);

    fixture.terminate();
}

#[test]
#[ignore = "requires a visible dual-display macOS desktop and Accessibility permission"]
fn daemon_should_reject_display_guard_before_activation_side_effects() {
    let fixture_binary = compile_fixture();
    let (fixture, _ready) = start_fixture(&fixture_binary, 2);
    let rdog_binary = rdog_binary_path();
    let (_daemon, port) = start_daemon(&rdog_binary);

    let first_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d1", 1);
    let second_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d2", 2);
    let second_window_id = second_window["matches"][0]["window_id"]
        .as_str()
        .expect("第二屏fixture window应该返回window_id");

    // -------------------------------------------------------------------------
    // 目标窗口位于第二块display,guard用第一块display的窗口identity解析。
    // 响应必须在任何 activate/raise step 之前结束。
    // -------------------------------------------------------------------------
    let first_window_id = first_window["matches"][0]["window_id"]
        .as_str()
        .expect("第一屏fixture window应该返回window_id");
    let activate_payload = serde_json::json!({
        "window_id": second_window_id,
        "guard": {"display": {"window_id": first_window_id}},
        "verify": {"focused": true, "timeout_ms": 1000, "poll_interval_ms": 50},
    });
    let activate = run_control_command(
        &rdog_binary,
        port,
        &format!("@window-activate#3:{activate_payload}"),
    );

    assert_eq!(activate["status"], "failed");
    assert_eq!(activate["error_code"], "WINDOW_ACTIVATE_GUARD_FAILED");
    assert_eq!(activate["failed_step"], "guard_display");
    assert_eq!(activate["guard"]["before_rect_intersects"], false);
    let steps = activate["steps"]
        .as_array()
        .expect("activate response 应该包含 steps array");
    assert_eq!(steps.len(), 1, "guard 失败后不能继续执行激活 steps");
    assert_eq!(steps[0]["step"], "guard_display");
    assert!(
        steps
            .iter()
            .all(|step| step["step"] != "activate_app" && step["step"] != "raise_window"),
        "guard 失败后不能报告任何 activation side effect: {activate}"
    );

    fixture.terminate();
}

#[test]
#[ignore = "requires a visible dual-display macOS desktop and Accessibility permission"]
fn daemon_should_capture_ax_from_only_the_target_window() {
    let fixture_binary = compile_fixture();
    let (fixture, _ready) = start_fixture(&fixture_binary, 2);
    let rdog_binary = rdog_binary_path();
    let (_daemon, port) = start_daemon(&rdog_binary);

    let second_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d2", 1);
    let second_window_id = second_window["matches"][0]["window_id"]
        .as_str()
        .expect("第二屏fixture window应该返回window_id");
    let (second_x, second_y) = fixture_window_point(&second_window);

    let targeted_payload = serde_json::json!({
        "window": {"window_id": second_window_id},
        "role": "AXButton",
        "name_contains": "increment-display-",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "limit": 10,
        "scope": {"display": {"contains_point": {"x": second_x, "y": second_y}}},
    });
    let targeted = run_control_command(
        &rdog_binary,
        port,
        &format!("@ax-find#2:{targeted_payload}"),
    );
    assert_eq!(
        targeted["match_count"], 1,
        "targeted AX 只能看到目标窗口的 fixture button: {targeted}"
    );
    assert_eq!(targeted["returned_count"], 1);
    assert_eq!(targeted["matches"][0]["window_id"], second_window_id);
    assert_eq!(targeted["matches"][0]["name"], "increment-display-2");
    assert_eq!(targeted["display_scope"]["matched_before_filter"], 1);
    assert_eq!(targeted["display_scope"]["matched_after_filter"], 1);

    let observation_id = second_window["observation"]["observation_id"]
        .as_str()
        .expect("window-find 应该返回 observation_id");
    let window_ref = second_window["matches"][0]["ref"]
        .as_str()
        .expect("window-find match 应该返回 ref");
    let ref_payload = serde_json::json!({
        "window": {"ref": window_ref, "observation_id": observation_id},
        "role": "AXButton",
        "name_contains": "increment-display-",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "limit": 10,
    });
    let targeted_by_ref =
        run_control_command(&rdog_binary, port, &format!("@ax-find#3:{ref_payload}"));
    assert_eq!(targeted_by_ref["match_count"], 1);
    assert_eq!(targeted_by_ref["matches"][0]["window_id"], second_window_id);
    assert_eq!(targeted_by_ref["matches"][0]["name"], "increment-display-2");

    // -------------------------------------------------------------------------
    // 同一 target window 配错误 display scope 时,window filter 必须在 query 前清空输入。
    // 如果仍对完整 snapshot 查询,这里会错误返回第二屏按钮。
    // -------------------------------------------------------------------------
    let first_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d1", 4);
    let first_window_id = first_window["matches"][0]["window_id"]
        .as_str()
        .expect("第一屏fixture window应该返回window_id");
    let mismatched_payload = serde_json::json!({
        "window": {"window_id": second_window_id},
        "role": "AXButton",
        "name_contains": "increment-display-",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "limit": 10,
        "scope": {"display": {"window_id": first_window_id}},
    });
    let mismatched = run_control_command(
        &rdog_binary,
        port,
        &format!("@ax-find#5:{mismatched_payload}"),
    );
    assert_eq!(mismatched["match_count"], 0);
    assert_eq!(mismatched["returned_count"], 0);
    assert_eq!(mismatched["display_scope"]["matched_before_filter"], 1);
    assert_eq!(mismatched["display_scope"]["matched_after_filter"], 0);

    fixture.terminate();
}

#[test]
#[ignore = "requires a visible dual-display macOS desktop, Accessibility, and Screen Recording permission"]
fn daemon_should_emit_single_display_visual_artifacts_for_scoped_observe() {
    let fixture_binary = compile_fixture();
    let (fixture, _ready) = start_fixture(&fixture_binary, 2);
    let rdog_binary = rdog_binary_path();
    let (daemon, port) = start_daemon(&rdog_binary);

    let second_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d2", 1);
    let (second_x, second_y) = fixture_window_point(&second_window);
    let observe_payload = serde_json::json!({
        "mode": "hybrid",
        "scope": {"display": {"contains_point": {"x": second_x, "y": second_y}}},
        "include_screenshot": true,
        "include_ax": true,
        "ax_required": true,
        "include_windows": false,
        "include_manifest": true,
        "include_refs": false,
        "include_selectors": false,
    });
    let output = run_control_command_output(
        &rdog_binary,
        port,
        &format!("@observe#2:{observe_payload}"),
        Some(&daemon.workdir),
    );
    let observe = latest_response_value(&String::from_utf8_lossy(&output.stdout));

    assert_eq!(
        observe["status"], "complete",
        "scoped observe 失败: {observe}"
    );
    assert_eq!(observe["visual"]["scope_applied"], true);
    assert_eq!(observe["visual"]["layout"], "single-display");
    assert_eq!(observe["visual"]["display_count"], 1);
    let resolved_display_id = observe["visual"]["resolved_display_id"]
        .as_str()
        .expect("visual 应该返回 resolved_display_id");

    let download_dir = daemon.workdir.join("rdog_downloads");
    let files = fs::read_dir(&download_dir)
        .expect("scoped observe 应该创建下载目录")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let manifest_path = files
        .iter()
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .expect("scoped observe 应该保存 manifest");
    let image_path = files
        .iter()
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jpg"))
        .expect("scoped observe 应该保存 JPEG");
    let manifest: Value = serde_json::from_slice(
        &fs::read(manifest_path).expect("应该能读取 scoped screenshot manifest"),
    )
    .expect("scoped screenshot manifest 应该是合法 JSON");

    assert_eq!(manifest["layout"], "single-display");
    assert_eq!(manifest["display_count"], 1);
    assert_eq!(manifest["displays"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["displays"][0]["display_id"], resolved_display_id);
    assert_eq!(manifest["displays"][0]["image_rect"]["x"], 0);
    assert_eq!(manifest["displays"][0]["image_rect"]["y"], 0);
    assert_eq!(
        manifest["displays"][0]["os_rect"],
        observe["scope"]["display"]["resolved"]["os_rect"]
    );

    let scoped_rect = &observe["scope"]["display"]["resolved"]["os_rect"];
    let ax_windows = observe["accessibility"]["windows"]
        .as_array()
        .expect("hybrid scoped observe应该返回AX windows");
    assert!(!ax_windows.is_empty(), "fixture AX windows不应该为空");
    assert!(
        ax_windows
            .iter()
            .all(|window| json_rects_intersect(&window["rect"], scoped_rect)),
        "scoped AX lane不能泄漏其他display窗口: {ax_windows:?}"
    );

    let display_name = manifest["displays"][0]["name"]
        .as_str()
        .expect("manifest display应该返回name");
    let name_scope_payload = serde_json::json!({
        "title": "rdog-display-aware-d2",
        "scope": {"display": {"name_contains": display_name}},
        "limit": 5,
        "include_state": false,
        "include_recipes": false,
    });
    let name_scoped_window = run_control_command(
        &rdog_binary,
        port,
        &format!("@window-find#3:{name_scope_payload}"),
    );
    assert_eq!(name_scoped_window["match_count"], 1);
    assert_eq!(
        name_scoped_window["display_scope"]["resolved"]["display_id"],
        resolved_display_id
    );

    let image = image::open(image_path).expect("scoped screenshot JPEG 应该可以解码");
    assert_eq!(
        image.width(),
        manifest["image_size"]["width"].as_u64().unwrap() as u32
    );
    assert_eq!(
        image.height(),
        manifest["image_size"]["height"].as_u64().unwrap() as u32
    );

    fixture.terminate();
}

fn json_rects_intersect(left: &Value, right: &Value) -> bool {
    let bounds = |rect: &Value| {
        let x = rect["x"].as_i64().expect("rect.x应该是整数");
        let y = rect["y"].as_i64().expect("rect.y应该是整数");
        let width = rect["width"].as_u64().expect("rect.width应该是正整数") as i64;
        let height = rect["height"].as_u64().expect("rect.height应该是正整数") as i64;
        (x, y, x + width, y + height)
    };
    let (left_x, left_y, left_right, left_bottom) = bounds(left);
    let (right_x, right_y, right_right, right_bottom) = bounds(right);
    left_x < right_right && left_right > right_x && left_y < right_bottom && left_bottom > right_y
}

#[test]
#[ignore = "requires a live macOS Screen Recording permission"]
fn daemon_should_keep_primary_single_display_screenshot_compatibility() {
    let rdog_binary = rdog_binary_path();
    let (daemon, port) = start_daemon(&rdog_binary);
    let output = run_control_command_output(
        &rdog_binary,
        port,
        r#"@screenshot#1:{display:"primary",layout:"single",coordinate_space:"os-logical"}"#,
        Some(&daemon.workdir),
    );
    let response = latest_response_value(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(
        response, 0,
        "primary single screenshot 应该成功: {response}"
    );

    let download_dir = daemon.workdir.join("rdog_downloads");
    let files = fs::read_dir(&download_dir)
        .expect("primary screenshot 应该创建下载目录")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let jpeg_files = files
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jpg"))
        .collect::<Vec<_>>();
    assert_eq!(jpeg_files.len(), 1, "primary single路径只能保存1张JPEG");
    assert!(
        files
            .iter()
            .all(|path| path.extension().and_then(|ext| ext.to_str()) != Some("json")),
        "primary single兼容路径不应该生成composite manifest"
    );

    let image = image::open(jpeg_files[0]).expect("primary single JPEG应该可以解码");
    assert!(image.width() > 0 && image.height() > 0);
}

#[test]
#[ignore = "requires a visible dual-display macOS desktop and Accessibility permission"]
fn daemon_should_verify_ax_action_with_fresh_targeted_evidence() {
    let fixture_binary = compile_fixture();
    let (fixture, _ready) = start_fixture(&fixture_binary, 2);
    let rdog_binary = rdog_binary_path();
    let (_daemon, port) = start_daemon(&rdog_binary);

    let second_window = find_fixture_window(&rdog_binary, port, "rdog-display-aware-d2", 1);
    let second_window_id = second_window["matches"][0]["window_id"]
        .as_str()
        .expect("第二屏fixture window应该返回window_id");
    let (second_x, second_y) = fixture_window_point(&second_window);

    let activate_payload = serde_json::json!({
        "window_id": second_window_id,
        "guard": {"display": {"contains_point": {"x": second_x, "y": second_y}}},
        "verify": {"focused": true, "timeout_ms": 3000, "poll_interval_ms": 75},
    });
    let activate = run_control_command(
        &rdog_binary,
        port,
        &format!("@window-activate#2:{activate_payload}"),
    );
    assert_eq!(
        activate["status"], "ok",
        "动作前 focus precondition失败: {activate}"
    );
    assert_eq!(activate["verify"]["status"], "passed");

    let before_payload = serde_json::json!({
        "window": {"window_id": second_window_id},
        "role": "AXStaticText",
        "value": "count:0",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "include_values": true,
        "limit": 10,
    });
    let before = run_control_command(&rdog_binary, port, &format!("@ax-find#3:{before_payload}"));
    assert_eq!(
        before["match_count"], 1,
        "动作前必须观察到 count:0: {before}"
    );

    let button_payload = serde_json::json!({
        "window": {"window_id": second_window_id},
        "role": "AXButton",
        "name": "increment-display-2",
        "action": "AXPress",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "limit": 5,
    });
    let button = run_control_command(&rdog_binary, port, &format!("@ax-find#4:{button_payload}"));
    assert_eq!(
        button["match_count"], 1,
        "必须唯一定位第二屏 button: {button}"
    );
    let observation_id = button["observation"]["observation_id"]
        .as_str()
        .expect("button find 应该返回 observation_id");
    let button_ref = button["matches"][0]["ref"]
        .as_str()
        .expect("button match 应该返回 ref");
    let action_payload = serde_json::json!({
        "target": {"ref": button_ref, "observation_id": observation_id},
        "action": "AXPress",
    });
    let action = run_control_command(
        &rdog_binary,
        port,
        &format!("@ax-action#5:{action_payload}"),
    );
    assert_eq!(action["status"], "ok", "AXPress 应该成功提交: {action}");
    assert_eq!(action["performed"], true);

    let after_payload = serde_json::json!({
        "window": {"window_id": second_window_id},
        "role": "AXStaticText",
        "value": "count:1",
        "mode": "full",
        "depth": 6,
        "max_elements": 200,
        "include_values": true,
        "limit": 10,
    });
    let after = run_control_command(&rdog_binary, port, &format!("@ax-find#6:{after_payload}"));
    assert_eq!(
        after["match_count"], 1,
        "fresh evidence必须观察到 count:1: {after}"
    );
    assert_eq!(after["matches"][0]["window_id"], second_window_id);
    assert_eq!(after["matches"][0]["value"], "count:1");

    fixture.terminate();
}
