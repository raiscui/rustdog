#![cfg(target_os = "macos")]

use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

const LIVE_AX_E2E_ENV: &str = "RDOG_LIVE_AX_E2E";
const LIVE_AX_E2E_BINARY_ENV: &str = "RDOG_LIVE_AX_E2E_BINARY";
const LIVE_AX_E2E_VIA_TERMINAL_ENV: &str = "RDOG_LIVE_AX_E2E_VIA_TERMINAL";

#[derive(Debug)]
struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child guard should own child")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug)]
struct TerminalDaemon {
    port: u16,
    log_path: PathBuf,
}

impl TerminalDaemon {
    fn startup_log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_else(|err| {
            format!(
                "无法读取 Terminal daemon log {}: {err}",
                self.log_path.display()
            )
        })
    }

    fn stop(&self) {
        for pid in listener_pids(self.port) {
            let _ = Command::new("kill").arg(pid).status();
        }
    }
}

impl Drop for TerminalDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug)]
struct LiveAxDaemon {
    port: u16,
    binary: PathBuf,
    _workdir: PathBuf,
    _direct_daemon: Option<ChildGuard>,
    terminal_daemon: Option<TerminalDaemon>,
}

impl LiveAxDaemon {
    fn uses_terminal_host(&self) -> bool {
        self.terminal_daemon.is_some()
    }

    fn stop_terminal(&self) {
        if let Some(terminal_daemon) = self.terminal_daemon.as_ref() {
            terminal_daemon.stop();
        }
    }
}

fn start_live_ax_daemon(name: &str) -> LiveAxDaemon {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let workdir = temp_workdir(name);
    let mut direct_daemon = None::<ChildGuard>;
    let terminal_daemon = if terminal_host_enabled() {
        Some(start_terminal_daemon(&binary, &workdir, port))
    } else {
        direct_daemon = Some(start_direct_daemon(&binary, &workdir, port));
        None
    };

    let port_ready = wait_until_port_is_busy(
        direct_daemon.as_mut().map(ChildGuard::child_mut),
        port,
        Duration::from_secs(8),
    );
    assert!(
        port_ready,
        "daemon control lane never started listening on port {port}\n{}",
        terminal_daemon
            .as_ref()
            .map(TerminalDaemon::startup_log)
            .unwrap_or_else(|| "direct daemon stderr is intentionally suppressed".to_owned())
    );

    LiveAxDaemon {
        port,
        binary,
        _workdir: workdir,
        _direct_daemon: direct_daemon,
        terminal_daemon,
    }
}

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
    if let Some(path) = std::env::var_os(LIVE_AX_E2E_BINARY_ENV) {
        let binary = PathBuf::from(path);
        assert!(
            binary.exists(),
            "{LIVE_AX_E2E_BINARY_ENV} points to a missing binary: {}",
            binary.display()
        );
        return binary;
    }

    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_rdog") {
        return PathBuf::from(path);
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

fn temp_workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-control-ax-e2e-{name}-{}-{}",
        std::process::id(),
        next_free_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

fn is_port_listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn listener_pids(port: u16) -> Vec<String> {
    let output = Command::new("lsof")
        .arg(format!("-tiTCP:{port}"))
        .arg("-sTCP:LISTEN")
        .output()
        .expect("lsof should be available for macOS live AX E2E");

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn wait_until_port_is_busy(child: Option<&mut Child>, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut child = child;

    while Instant::now() < deadline {
        if let Some(child) = child.as_deref_mut() {
            if child.id() > 0
                && child
                    .try_wait()
                    .expect("try_wait should not fail while waiting for daemon")
                    .is_some()
            {
                return false;
            }
        }

        if is_port_listening(port) {
            return true;
        }

        thread::sleep(Duration::from_millis(20));
    }

    false
}

fn live_ax_e2e_enabled() -> bool {
    matches!(
        std::env::var(LIVE_AX_E2E_ENV).ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn terminal_host_enabled() -> bool {
    matches!(
        std::env::var(LIVE_AX_E2E_VIA_TERMINAL_ENV).ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn shell_quote(value: &Path) -> String {
    let value = value.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn start_direct_daemon(binary: &Path, workdir: &Path, port: u16) -> ChildGuard {
    ChildGuard::new(
        Command::new(binary)
            .arg("daemon")
            .env("RDOG_OUTBOUND__ENABLED", "false")
            .env("RDOG_INBOUND__ENABLED", "true")
            .env("RDOG_INBOUND__HOST", "127.0.0.1")
            .env("RDOG_INBOUND__PORT", port.to_string())
            .env("RDOG_INBOUND__SHELL", "/bin/sh")
            .env("RDOG_INBOUND__MODE", "control")
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|err| panic!("rdog daemon should start: {err}")),
    )
}

fn start_terminal_daemon(binary: &Path, workdir: &Path, port: u16) -> TerminalDaemon {
    let script_path = std::env::temp_dir().join(format!("rdog-ax-e2e-{port}.command"));
    let log_path = std::env::temp_dir().join(format!("rdog-ax-e2e-{port}.log"));
    let script = format!(
        "#!/bin/zsh\n\
         cd {}\n\
         export RDOG_OUTBOUND__ENABLED=false\n\
         export RDOG_INBOUND__ENABLED=true\n\
         export RDOG_INBOUND__HOST=127.0.0.1\n\
         export RDOG_INBOUND__PORT={port}\n\
         export RDOG_INBOUND__SHELL=/bin/sh\n\
         export RDOG_INBOUND__MODE=control\n\
         exec {} daemon > {} 2>&1\n",
        shell_quote(workdir),
        shell_quote(binary),
        shell_quote(&log_path)
    );
    fs::write(&script_path, script).expect("terminal daemon script should write");

    let mut perms = fs::metadata(&script_path)
        .expect("terminal daemon script metadata should exist")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    fs::set_permissions(&script_path, perms).expect("terminal daemon script should be executable");

    let status = Command::new("open")
        .args(["-a", "Terminal"])
        .arg(&script_path)
        .status()
        .expect("open -a Terminal should run");
    assert!(
        status.success(),
        "open -a Terminal should accept daemon command script"
    );

    TerminalDaemon { port, log_path }
}

fn run_control_command(binary: &Path, port: u16, line: &str, timeout: Duration) -> Value {
    let mut child = Command::new(binary)
        .args(["control", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("control command should start: {err}"));

    child
        .stdin
        .as_mut()
        .expect("control stdin should exist")
        .write_all(line.as_bytes())
        .expect("control stdin should accept command");
    drop(child.stdin.take());

    let output = wait_with_output_timeout(child, timeout, "rdog control command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "control command should exit successfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    latest_response(&stdout).unwrap_or_else(|| {
        panic!("control output should contain @response\nstdout:\n{stdout}\nstderr:\n{stderr}")
    })
}

fn wait_with_output_timeout(mut child: Child, timeout: Duration, label: &str) -> Output {
    let stdout = child
        .stdout
        .take()
        .expect("child stdout should be piped before waiting");
    let stderr = child
        .stderr
        .take()
        .expect("child stderr should be piped before waiting");

    // AX tree JSON 可能非常大。必须在等待子进程退出的同时读取 pipe,
    // 否则 stdout 填满后 control 子进程会卡住,测试会误判为协议超时。
    let stdout_reader = spawn_pipe_reader(stdout, label, "stdout");
    let stderr_reader = spawn_pipe_reader(stderr, label, "stderr");
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for output")
            .is_some()
        {
            let status = child
                .wait()
                .unwrap_or_else(|err| panic!("{label} status should be collectable: {err}"));
            return Output {
                status,
                stdout: join_pipe_reader(stdout_reader, label, "stdout"),
                stderr: join_pipe_reader(stderr_reader, label, "stderr"),
            };
        }

        thread::sleep(Duration::from_millis(20));
    }

    let _ = child.kill();
    let status = child
        .wait()
        .unwrap_or_else(|err| panic!("{label} status should be collectable after timeout: {err}"));
    let output = Output {
        status,
        stdout: join_pipe_reader(stdout_reader, label, "stdout"),
        stderr: join_pipe_reader(stderr_reader, label, "stderr"),
    };
    panic!(
        "{label} timed out\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn spawn_pipe_reader<R>(
    mut reader: R,
    label: &str,
    stream_name: &'static str,
) -> thread::JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let label = label.to_owned();
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap_or_else(|err| {
            panic!("{label} {stream_name} should be readable until EOF: {err}")
        });
        output
    })
}

fn join_pipe_reader(
    reader: thread::JoinHandle<Vec<u8>>,
    label: &str,
    stream_name: &str,
) -> Vec<u8> {
    reader
        .join()
        .unwrap_or_else(|_| panic!("{label} {stream_name} reader thread should not panic"))
}

fn latest_response(output: &str) -> Option<Value> {
    output.lines().rev().find_map(|line| {
        let response_index = line.find("@response ")?;
        let json_text = &line[response_index + "@response ".len()..];
        serde_json::from_str::<Value>(json_text).ok()
    })
}

fn successful_response_value(response: Value, label: &str, binary: &Path) -> Value {
    if response.get("code").is_some() {
        let code = response["code"].as_i64();
        let error = response["error"].as_str().unwrap_or("<missing error>");

        if code == Some(77) {
            panic!(
                "{label} reached rdog AX backend, but macOS denied Accessibility permission for the actual daemon process.\n\
                 binary: {}\n\
                 response: {}\n\
                 If Terminal is already authorized, rerun with {LIVE_AX_E2E_VIA_TERMINAL_ENV}=1 so Terminal launches the daemon.",
                binary.display(),
                json_excerpt(&response, 2000)
            );
        }

        panic!(
            "{label} returned protocol error code {:?}: {error}\nresponse: {}",
            code,
            json_excerpt(&response, 2000)
        );
    }

    response
        .get("value")
        .cloned()
        .unwrap_or_else(|| panic!("{label} response should contain value: {response}"))
}

fn assert_ax_tree_is_complete(tree: &Value) {
    assert_eq!(tree["kind"].as_str(), Some("ax-tree"));
    assert_eq!(tree["schema"].as_str(), Some("rdog.ax.v1"));
    assert_eq!(tree["platform"].as_str(), Some("macos"));
    assert_eq!(tree["capture_status"].as_str(), Some("complete"));
    assert_eq!(tree["permission_status"].as_str(), Some("granted"));
    assert_eq!(tree["coordinate_space"].as_str(), Some("os-logical"));
    assert!(
        tree["window_count"].as_u64().unwrap_or(0) > 0,
        "@ax-tree should read at least one real window: {}",
        json_excerpt(tree, 2000)
    );
    assert!(
        tree["element_count"].as_u64().unwrap_or(0) > 0,
        "@ax-tree should read at least one real UI element: {}",
        json_excerpt(tree, 2000)
    );
}

fn find_terminal_window<'a>(tree: &'a Value, port: u16) -> Option<&'a Value> {
    let needle = format!("rdog-ax-e2e-{port}.command");
    tree.get("windows")
        .and_then(Value::as_array)?
        .iter()
        .find(|window| {
            is_terminal_process_name(window["process_name"].as_str())
                && window["title"]
                    .as_str()
                    .is_some_and(|title| title.contains(&needle))
        })
}

fn is_terminal_process_name(name: Option<&str>) -> bool {
    matches!(name, Some("Terminal" | "终端"))
}

fn descendants(value: &Value) -> Vec<&Value> {
    let mut result = Vec::new();
    let mut stack = value
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    while let Some(item) = stack.pop() {
        result.push(item);
        if let Some(children) = item.get("children").and_then(Value::as_array) {
            stack.extend(children);
        }
    }

    result
}

fn find_close_button_id(window: &Value) -> Option<String> {
    descendants(window).into_iter().find_map(|element| {
        let is_close = element["role"].as_str() == Some("AXButton")
            && element["subrole"].as_str() == Some("AXCloseButton");
        let has_press = element
            .get("actions")
            .and_then(Value::as_array)
            .is_some_and(|actions| {
                actions
                    .iter()
                    .any(|action| action.as_str() == Some("AXPress"))
            });

        (is_close && has_press)
            .then(|| element["id"].as_str().map(ToOwned::to_owned))
            .flatten()
    })
}

fn find_terminal_termination_sheet(tree: &Value, port: u16) -> Option<(String, String)> {
    let window = find_terminal_window(tree, port)?;
    let text_seen = descendants(window).into_iter().any(|element| {
        element["value"].as_str().is_some_and(|value| {
            value.contains("你想要终止这个窗口中正在运行的进程吗")
                || value.contains("running processes")
        })
    });
    if !text_seen {
        return None;
    }

    let mut cancel_id = None::<String>;
    let mut terminate_id = None::<String>;
    for element in descendants(window) {
        if element["role"].as_str() != Some("AXButton") {
            continue;
        }
        let name = element["name"].as_str().unwrap_or_default();
        let id = element["id"].as_str().unwrap_or_default().to_owned();
        if name == "取消" || name == "Cancel" {
            cancel_id = Some(id);
        } else if name == "终止" || name == "Terminate" {
            terminate_id = Some(id);
        }
    }

    Some((cancel_id?, terminate_id?))
}

fn json_excerpt(value: &Value, max_len: usize) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|err| {
        format!("{{\"json_error\":\"failed to render json excerpt: {err}\"}}")
    });
    if text.len() > max_len {
        text.truncate(max_len);
        text.push_str("\n...<truncated>");
    }
    text
}

fn assert_ax_press_report(report: &Value, expected_target_id: &str) {
    assert_eq!(report["kind"].as_str(), Some("ax"));
    assert_eq!(report["action"].as_str(), Some("press"));
    assert_eq!(report["backend"].as_str(), Some("macos-accessibility"));
    assert_eq!(report["target_id"].as_str(), Some(expected_target_id));
    assert_eq!(report["performed"].as_bool(), Some(true));
    assert_eq!(report["status"].as_str(), Some("ok"));
}

fn value_contains_action(value: &Value, expected_action: &str) -> bool {
    value
        .get("actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| {
            actions
                .iter()
                .any(|action| action.as_str() == Some(expected_action))
        })
}

fn assert_ax_find_close_button_match(find: &Value, title_needle: &str) -> String {
    assert_eq!(find["kind"].as_str(), Some("ax-find"));
    assert_eq!(find["schema"].as_str(), Some("rdog.ax.v1"));
    assert_eq!(find["platform"].as_str(), Some("macos"));
    assert_eq!(find["capture_status"].as_str(), Some("complete"));
    assert_eq!(find["permission_status"].as_str(), Some("granted"));
    assert_eq!(find["coordinate_space"].as_str(), Some("os-logical"));
    assert!(
        find["match_count"].as_u64().unwrap_or(0) > 0,
        "@ax-find should match the Terminal close button: {}",
        json_excerpt(find, 4000)
    );

    let matches = find["matches"]
        .as_array()
        .unwrap_or_else(|| panic!("@ax-find should return matches array: {find}"));
    let close_button = matches
        .iter()
        .find(|item| {
            item["role"].as_str() == Some("AXButton")
                && item["subrole"].as_str() == Some("AXCloseButton")
                && item["window_title"]
                    .as_str()
                    .is_some_and(|title| title.contains(title_needle))
                && value_contains_action(item, "AXPress")
        })
        .unwrap_or_else(|| {
            panic!(
                "@ax-find should return a pressable Terminal close button: {}",
                json_excerpt(find, 4000)
            )
        });

    close_button["id"]
        .as_str()
        .unwrap_or_else(|| panic!("@ax-find match should contain id: {close_button}"))
        .to_owned()
}

fn assert_ax_get_close_button(get: &Value, expected_target_id: &str, title_needle: &str) {
    assert_eq!(get["kind"].as_str(), Some("ax-get"));
    assert_eq!(get["schema"].as_str(), Some("rdog.ax.v1"));
    assert_eq!(get["platform"].as_str(), Some("macos"));
    assert_eq!(get["capture_status"].as_str(), Some("complete"));
    assert_eq!(get["permission_status"].as_str(), Some("granted"));
    assert_eq!(get["coordinate_space"].as_str(), Some("os-logical"));
    assert_eq!(get["target_id"].as_str(), Some(expected_target_id));
    assert_eq!(get["target_type"].as_str(), Some("element"));
    assert!(
        get["window_title"]
            .as_str()
            .is_some_and(|title| title.contains(title_needle)),
        "@ax-get should preserve the Terminal window title: {}",
        json_excerpt(get, 4000)
    );

    let element = get
        .get("element")
        .unwrap_or_else(|| panic!("@ax-get should return an element body: {get}"));
    assert_eq!(element["id"].as_str(), Some(expected_target_id));
    assert_eq!(element["role"].as_str(), Some("AXButton"));
    assert_eq!(element["subrole"].as_str(), Some("AXCloseButton"));
    assert!(
        value_contains_action(element, "AXPress"),
        "@ax-get element should expose AXPress: {}",
        json_excerpt(get, 4000)
    );
}

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_read_real_terminal_window_and_press_real_button() {
    if !live_ax_e2e_enabled() {
        eprintln!(
            "skipping live AX E2E; set {LIVE_AX_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    let daemon = start_live_ax_daemon("terminal-close-button");
    let port = daemon.port;
    let binary = daemon.binary.as_path();

    let tree_response = run_control_command(
        binary,
        port,
        "@ax-tree#100:{scope:\"windows\",depth:6,max_elements:8000,include_values:true}\n",
        Duration::from_secs(30),
    );
    let tree = successful_response_value(tree_response, "@ax-tree", binary);
    assert_ax_tree_is_complete(&tree);

    let close_button_id = if daemon.uses_terminal_host() {
        let window = find_terminal_window(&tree, port).unwrap_or_else(|| {
            panic!(
                "@ax-tree should include the Terminal daemon window for port {port}: {}",
                json_excerpt(&tree, 4000)
            )
        });
        find_close_button_id(window).unwrap_or_else(|| {
            panic!(
                "Terminal daemon window should expose a pressable close button: {}",
                json_excerpt(window, 4000)
            )
        })
    } else {
        let window = tree["windows"]
            .as_array()
            .and_then(|windows| {
                windows
                    .iter()
                    .find(|window| is_terminal_process_name(window["process_name"].as_str()))
            })
            .unwrap_or_else(|| panic!("@ax-tree should include at least one Terminal window"));
        find_close_button_id(window).unwrap_or_else(|| {
            panic!(
                "Terminal window should expose a pressable close button: {}",
                json_excerpt(window, 4000)
            )
        })
    };

    let press_response = run_control_command(
        binary,
        port,
        &format!("@ax-press#101:{{target:{{id:\"{close_button_id}\"}}}}\n"),
        Duration::from_secs(15),
    );
    let press_report = successful_response_value(press_response, "@ax-press", binary);
    assert_ax_press_report(&press_report, &close_button_id);

    if daemon.uses_terminal_host() {
        let sheet_tree = (0..10)
            .find_map(|_| {
                thread::sleep(Duration::from_millis(250));
                let response = run_control_command(
                    binary,
                    port,
                    "@ax-tree#102:{scope:\"windows\",depth:7,max_elements:8000,include_values:true}\n",
                    Duration::from_secs(30),
                );
                let tree = successful_response_value(response, "@ax-tree", binary);
                find_terminal_termination_sheet(&tree, port)
            })
            .unwrap_or_else(|| {
                panic!("AXPress should open Terminal's running-process confirmation sheet")
            });
        eprintln!(
            "live AX E2E observed Terminal confirmation sheet: cancel_id={}, terminate_id={}",
            sheet_tree.0, sheet_tree.1
        );

        let cancel_response = run_control_command(
            binary,
            port,
            &format!("@ax-press#103:{{target:{{id:\"{}\"}}}}\n", sheet_tree.0),
            Duration::from_secs(15),
        );
        let cancel_report = successful_response_value(cancel_response, "@ax-press cancel", binary);
        assert_ax_press_report(&cancel_report, &sheet_tree.0);
        daemon.stop_terminal();
    }
}

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_find_and_get_real_terminal_button() {
    if !live_ax_e2e_enabled() {
        eprintln!(
            "skipping live AX find/get E2E; set {LIVE_AX_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    if !terminal_host_enabled() {
        eprintln!(
            "skipping live AX find/get E2E; set {LIVE_AX_E2E_VIA_TERMINAL_ENV}=1 to expose the test daemon's Terminal window"
        );
        return;
    }

    let daemon = start_live_ax_daemon("terminal-find-get");
    let port = daemon.port;
    let binary = daemon.binary.as_path();
    let title_needle = format!("rdog-ax-e2e-{port}.command");

    let find_response = run_control_command(
        binary,
        port,
        &format!(
            "@ax-find#200:{{window_title_contains:\"{title_needle}\",role:\"AXButton\",subrole:\"AXCloseButton\",action:\"AXPress\",depth:6,max_elements:8000,include_values:false,limit:5}}\n"
        ),
        Duration::from_secs(30),
    );
    let find = successful_response_value(find_response, "@ax-find", binary);
    let close_button_id = assert_ax_find_close_button_match(&find, &title_needle);

    let get_response = run_control_command(
        binary,
        port,
        &format!(
            "@ax-get#201:{{target:{{id:\"{close_button_id}\"}},depth:2,max_elements:8000,include_values:false}}\n"
        ),
        Duration::from_secs(30),
    );
    let get = successful_response_value(get_response, "@ax-get", binary);
    assert_ax_get_close_button(&get, &close_button_id, &title_needle);
    eprintln!("live AX find/get observed Terminal close button: target_id={close_button_id}");

    daemon.stop_terminal();
}
