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
struct TextEditDocumentFixture {
    file_path: PathBuf,
    title_needle: String,
}

impl TextEditDocumentFixture {
    fn create(name: &str) -> Self {
        Self::create_with_contents(name, "")
    }

    fn create_with_contents(name: &str, contents: &str) -> Self {
        let file_path = std::env::temp_dir().join(format!(
            "rdog-ax-textedit-{name}-{}-{}.txt",
            std::process::id(),
            next_free_port()
        ));
        fs::write(&file_path, contents).expect("TextEdit fixture file should write");

        let status = Command::new("open")
            .arg("-a")
            .arg("TextEdit")
            .arg(&file_path)
            .status()
            .expect("open -a TextEdit should run");
        assert!(status.success(), "open -a TextEdit should succeed");
        run_applescript("tell application \"TextEdit\" to activate");

        let title_needle = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("TextEdit fixture file name should be UTF-8")
            .to_owned();

        Self {
            file_path,
            title_needle,
        }
    }

    fn set_bounds(&self, x: i64, y: i64, width: i64, height: i64) {
        let right = x + width;
        let bottom = y + height;
        let title = apple_script_string(&self.title_needle);
        run_applescript(&format!(
            "tell application \"TextEdit\"\n\
             activate\n\
             repeat with w in windows\n\
               try\n\
                 if (name of w) contains \"{title}\" then\n\
                   set bounds of w to {{{x}, {y}, {right}, {bottom}}}\n\
                   exit repeat\n\
                 end if\n\
               end try\n\
             end repeat\n\
             end tell"
        ));
    }

    fn hide_app(&self) {
        run_applescript(
            "tell application \"System Events\"\n\
             tell process \"TextEdit\"\n\
               set visible to false\n\
             end tell\n\
             end tell",
        );
    }

    fn close_without_saving(&self) {
        let title = apple_script_string(&self.title_needle);
        let script = format!(
            "tell application \"TextEdit\"\n\
             repeat with d in documents\n\
               try\n\
                 if (name of d) contains \"{title}\" then\n\
                   close d saving no\n\
                 end if\n\
               end try\n\
             end repeat\n\
             end tell"
        );
        let _ = try_run_applescript(&script);
    }
}

impl Drop for TextEditDocumentFixture {
    fn drop(&mut self) {
        self.close_without_saving();
        let _ = fs::remove_file(&self.file_path);
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

fn run_applescript(script: &str) -> String {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .unwrap_or_else(|err| panic!("osascript should run: {err}"));
    assert!(
        output.status.success(),
        "osascript should succeed\nscript:\n{script}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn try_run_applescript(script: &str) -> Option<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn apple_script_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
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

fn assert_ax_focus_report(report: &Value, expected_window_id: &str, activated: bool) {
    assert_eq!(report["kind"].as_str(), Some("ax-focus"));
    assert_eq!(report["backend"].as_str(), Some("macos-accessibility"));
    assert_eq!(report["window_id"].as_str(), Some(expected_window_id));
    assert_eq!(report["activated"].as_bool(), Some(activated));
    assert_eq!(report["performed"].as_bool(), Some(true));
    assert_eq!(report["status"].as_str(), Some("ok"));
}

fn assert_type_text_targeted_keyboard_report(report: &Value, expected_target_id: &str) {
    assert_eq!(report["kind"].as_str(), Some("type-text"));
    assert_eq!(
        report["backend"].as_str(),
        Some("macos-cg-event-post-to-pid")
    );
    assert_eq!(report["target_id"].as_str(), Some(expected_target_id));
    assert_eq!(report["mode"].as_str(), Some("targeted-keyboard"));
    assert_eq!(report["delivered_via"].as_str(), Some("targeted-keyboard"));
    assert_eq!(report["performed"].as_bool(), Some(true));
    assert_eq!(report["status"].as_str(), Some("ok"));
    assert_eq!(report["used_clipboard"].as_bool(), Some(false));
}

fn assert_key_delivery_report(
    report: &Value,
    expected_delivery: &str,
    expected_key: &str,
    expected_pid: i32,
    expected_window_id: Option<&str>,
) {
    assert_eq!(report["kind"].as_str(), Some("key"));
    assert_eq!(
        report["backend"].as_str(),
        Some("macos-cg-event-post-to-pid")
    );
    assert_eq!(report["delivery"].as_str(), Some(expected_delivery));
    assert_eq!(report["key"].as_str(), Some(expected_key));
    assert_eq!(report["performed"].as_bool(), Some(true));
    assert_eq!(report["status"].as_str(), Some("ok"));
    assert_eq!(report["target_pid"].as_i64(), Some(i64::from(expected_pid)));
    match expected_window_id {
        Some(window_id) => assert_eq!(report["window_id"].as_str(), Some(window_id)),
        None => assert!(report.get("window_id").is_none()),
    }
}

fn assert_ax_scroll_report(report: &Value, expected_target_id: &str, expected_pages: u16) {
    assert_eq!(report["kind"].as_str(), Some("ax-scroll"));
    assert_eq!(report["backend"].as_str(), Some("macos-accessibility"));
    assert_eq!(report["target_id"].as_str(), Some(expected_target_id));
    assert_eq!(report["direction"].as_str(), Some("down"));
    assert_eq!(report["pages"].as_u64(), Some(u64::from(expected_pages)));
    assert_eq!(report["delivered_via"].as_str(), Some("ax-scrollbar-value"));
    assert_eq!(report["performed"].as_bool(), Some(true));
    assert_eq!(report["status"].as_str(), Some("ok"));
}

fn wait_for_textedit_window_id(
    binary: &Path,
    port: u16,
    title_needle: &str,
    label: &str,
) -> String {
    let mut last_find = None::<Value>;
    for _ in 0..20 {
        let response = run_control_command(
            binary,
            port,
            &format!(
                "@window-find#302:{{app:\"TextEdit\",title_contains:\"{title_needle}\",limit:5,include_state:true,include_recipes:false}}\n"
            ),
            Duration::from_secs(30),
        );
        let find = successful_response_value(response, label, binary);
        last_find = Some(find.clone());
        if let Some(first) = find["matches"]
            .as_array()
            .and_then(|matches| matches.first())
        {
            return first["window_id"]
                .as_str()
                .unwrap_or_else(|| panic!("window-find match should contain window_id: {first}"))
                .to_owned();
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "Timed out waiting for TextEdit window_id: {title_needle}\nlast @window-find response:\n{}",
        last_find
            .as_ref()
            .map(|value| json_excerpt(value, 4000))
            .unwrap_or_else(|| "<no @window-find response captured>".to_owned())
    );
}

fn wait_for_textedit_editor_match(
    binary: &Path,
    port: u16,
    window_id: &str,
    label: &str,
) -> (String, String, i32) {
    let mut last_get = None::<Value>;
    for _ in 0..20 {
        let response = run_control_command(
            binary,
            port,
            &format!(
                "@ax-get#300:{{target:{{id:\"{window_id}\"}},depth:6,max_elements:2000,include_values:false}}\n"
            ),
            Duration::from_secs(30),
        );
        let get = successful_response_value(response, label, binary);
        last_get = Some(get.clone());
        if let Some(window) = get.get("window") {
            if let Some(editor) = descendants(window).into_iter().find(|element| {
                matches!(
                    element["role"].as_str(),
                    Some("AXTextArea") | Some("AXTextField")
                )
            }) {
                let target_id = editor["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("TextEdit editor node should contain id: {editor}"))
                    .to_owned();
                let restored_window_id = get["window_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("ax-get should contain window_id: {get}"))
                    .to_owned();
                let pid = get["pid"]
                    .as_i64()
                    .unwrap_or_else(|| panic!("ax-get should contain pid: {get}"))
                    as i32;
                return (target_id, restored_window_id, pid);
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "Timed out waiting for TextEdit editor target in window: {window_id}\nlast @ax-get response:\n{}",
        last_get
            .as_ref()
            .map(|value| json_excerpt(value, 4000))
            .unwrap_or_else(|| "<no @ax-get response captured>".to_owned())
    );
}

fn wait_for_textedit_value(
    binary: &Path,
    port: u16,
    target_id: &str,
    expected_text: &str,
) -> Value {
    let mut last_get = None::<Value>;
    for _ in 0..20 {
        let response = run_control_command(
            binary,
            port,
            &format!(
                "@ax-get#301:{{target:{{id:\"{target_id}\"}},depth:2,max_elements:400,include_values:true}}\n"
            ),
            Duration::from_secs(30),
        );
        let get = successful_response_value(response, "@ax-get text value", binary);
        last_get = Some(get.clone());
        let value_text = get["element"]["value"].as_str().unwrap_or_default();
        if value_text.contains(expected_text) {
            return get;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "Timed out waiting for TextEdit value to contain expected text: {expected_text}\nlast @ax-get response:\n{}",
        last_get
            .as_ref()
            .map(|value| json_excerpt(value, 4000))
            .unwrap_or_else(|| "<no @ax-get response captured>".to_owned())
    );
}

fn wait_for_textedit_value_exact(
    binary: &Path,
    port: u16,
    target_id: &str,
    expected_text: &str,
) -> Value {
    let mut last_get = None::<Value>;
    for _ in 0..20 {
        let response = run_control_command(
            binary,
            port,
            &format!(
                "@ax-get#304:{{target:{{id:\"{target_id}\"}},depth:2,max_elements:400,include_values:true}}\n"
            ),
            Duration::from_secs(30),
        );
        let get = successful_response_value(response, "@ax-get exact text value", binary);
        last_get = Some(get.clone());
        let value_text = get["element"]["value"].as_str().unwrap_or_default();
        if value_text == expected_text {
            return get;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "Timed out waiting for TextEdit value to equal expected text: {expected_text}\nlast @ax-get response:\n{}",
        last_get
            .as_ref()
            .map(|value| json_excerpt(value, 4000))
            .unwrap_or_else(|| "<no @ax-get response captured>".to_owned())
    );
}

fn read_textedit_window_tree(binary: &Path, port: u16, window_id: &str, label: &str) -> Value {
    let response = run_control_command(
        binary,
        port,
        &format!(
            "@ax-get#320:{{target:{{id:\"{window_id}\"}},depth:8,max_elements:3000,include_values:false}}\n"
        ),
        Duration::from_secs(30),
    );
    successful_response_value(response, label, binary)
}

fn find_first_textedit_editor_target(window_tree: &Value) -> Option<String> {
    let window = window_tree.get("window")?;
    descendants(window).into_iter().find_map(|element| {
        matches!(
            element["role"].as_str(),
            Some("AXTextArea") | Some("AXTextField")
        )
        .then(|| element["id"].as_str().map(ToOwned::to_owned))
        .flatten()
    })
}

fn find_first_scrollbar_target_id(window_tree: &Value) -> Option<String> {
    let window = window_tree.get("window")?;
    descendants(window).into_iter().find_map(|element| {
        (element["role"].as_str() == Some("AXScrollBar"))
            .then(|| element["id"].as_str().map(ToOwned::to_owned))
            .flatten()
    })
}

fn read_ax_element(binary: &Path, port: u16, target_id: &str, label: &str) -> Value {
    let response = run_control_command(
        binary,
        port,
        &format!(
            "@ax-get#321:{{target:{{id:\"{target_id}\"}},depth:1,max_elements:32,include_values:true}}\n"
        ),
        Duration::from_secs(15),
    );
    successful_response_value(response, label, binary)
}

fn read_scrollbar_state(binary: &Path, port: u16, scrollbar_target_id: &str) -> Value {
    read_ax_element(binary, port, scrollbar_target_id, "@ax-get scrollbar value")
}

fn extract_scrollbar_position(element_get: &Value) -> Option<f64> {
    element_get["element"]["children"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|child| {
            if child["role"].as_str() != Some("AXValueIndicator") {
                return None;
            }
            child["rect"]["y"].as_f64()
        })
        .or_else(|| extract_numeric_value(element_get))
}

fn extract_numeric_value(element_get: &Value) -> Option<f64> {
    element_get["element"]["value"]
        .as_str()
        .and_then(|raw| raw.parse::<f64>().ok())
}

fn wait_for_scrollbar_value_change(
    binary: &Path,
    port: u16,
    scrollbar_target_id: &str,
    old_value: f64,
) -> Value {
    let mut last_tree = None::<Value>;
    for _ in 0..20 {
        let tree = read_scrollbar_state(binary, port, scrollbar_target_id);
        last_tree = Some(tree.clone());
        if let Some(new_value) = extract_scrollbar_position(&tree) {
            if (new_value - old_value).abs() > 0.0001 {
                return tree;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!(
        "Timed out waiting for AX scroll bar position/value to change from {old_value}\nlast @ax-get response:\n{}",
        last_tree
            .as_ref()
            .map(|value| json_excerpt(value, 4000))
            .unwrap_or_else(|| "<no @ax-get response captured>".to_owned())
    );
}

fn assert_textedit_is_frontmost() {
    let frontmost = run_applescript(
        "tell application \"System Events\"\n\
         tell process \"TextEdit\"\n\
           return frontmost as string\n\
         end tell\n\
         end tell",
    );
    assert_eq!(
        frontmost, "true",
        "TextEdit should become frontmost after @ax-focus"
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

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_focus_hidden_textedit_and_type_without_mouse() {
    if !live_ax_e2e_enabled() {
        eprintln!(
            "skipping live AX focus/type E2E; set {LIVE_AX_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    let daemon = start_live_ax_daemon("textedit-focus-type");
    let binary = daemon.binary.as_path();
    let port = daemon.port;
    let fixture = TextEditDocumentFixture::create("focus-type");

    let initial_window_id =
        wait_for_textedit_window_id(binary, port, &fixture.title_needle, "@window-find initial");
    fixture.hide_app();

    let focus_response = run_control_command(
        binary,
        port,
        &format!("@ax-focus#310:{{window_id:\"{initial_window_id}\",activate:true}}\n"),
        Duration::from_secs(15),
    );
    let focus_report = successful_response_value(focus_response, "@ax-focus", binary);
    assert_ax_focus_report(&focus_report, &initial_window_id, true);
    assert_textedit_is_frontmost();

    let (editor_target_id, restored_window_id, target_pid) =
        wait_for_textedit_editor_match(binary, port, &initial_window_id, "@ax-get restored editor");
    assert_eq!(
        restored_window_id, initial_window_id,
        "TextEdit window_id should stay stable across hide/focus recovery"
    );

    let type_response = run_control_command(
        binary,
        port,
        &format!(
            "@type-text#311:{{target:{{id:\"{editor_target_id}\"}},text:\"AX TARGETED TEXT\",mode:\"targeted-keyboard\"}}\n"
        ),
        Duration::from_secs(15),
    );
    let type_report =
        successful_response_value(type_response, "@type-text targeted-keyboard", binary);
    assert_type_text_targeted_keyboard_report(&type_report, &editor_target_id);

    let get = wait_for_textedit_value(binary, port, &editor_target_id, "AX TARGETED TEXT");
    assert_eq!(get["kind"].as_str(), Some("ax-get"));
    assert_eq!(get["target_id"].as_str(), Some(editor_target_id.as_str()));
    assert_eq!(get["window_id"].as_str(), Some(restored_window_id.as_str()));
    assert_eq!(get["pid"].as_i64(), Some(i64::from(target_pid)));
    assert!(
        get["element"]["value"]
            .as_str()
            .is_some_and(|value| value.contains("AX TARGETED TEXT")),
        "TextEdit AX value should contain typed text: {}",
        json_excerpt(&get, 4000)
    );

    eprintln!(
        "live AX focus/type E2E observed TextEdit target: window_id={}, target_id={}, pid={}",
        restored_window_id, editor_target_id, target_pid
    );

    daemon.stop_terminal();
}

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_deliver_pid_and_window_targeted_hotkeys_to_real_textedit() {
    if !live_ax_e2e_enabled() {
        eprintln!(
            "skipping live targeted-hotkey E2E; set {LIVE_AX_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    let daemon = start_live_ax_daemon("textedit-targeted-key");
    let binary = daemon.binary.as_path();
    let port = daemon.port;
    let fixture = TextEditDocumentFixture::create("targeted-key");

    let window_id =
        wait_for_textedit_window_id(binary, port, &fixture.title_needle, "@window-find key");
    let (editor_target_id, restored_window_id, target_pid) =
        wait_for_textedit_editor_match(binary, port, &window_id, "@ax-get key editor");
    assert_eq!(restored_window_id, window_id);

    let focus_response = run_control_command(
        binary,
        port,
        &format!("@ax-focus#329:{{target:{{id:\"{editor_target_id}\"}}}}\n"),
        Duration::from_secs(15),
    );
    let focus_report = successful_response_value(focus_response, "@ax-focus key target", binary);
    assert_eq!(focus_report["kind"].as_str(), Some("ax-focus"));
    assert_eq!(
        focus_report["target_id"].as_str(),
        Some(editor_target_id.as_str())
    );
    assert_eq!(focus_report["performed"].as_bool(), Some(true));

    let seed_text_response = run_control_command(
        binary,
        port,
        &format!(
            "@type-text#328:{{target:{{id:\"{editor_target_id}\"}},text:\"AB\",mode:\"targeted-keyboard\"}}\n"
        ),
        Duration::from_secs(15),
    );
    let seed_text_report =
        successful_response_value(seed_text_response, "@type-text hotkey seed", binary);
    assert_type_text_targeted_keyboard_report(&seed_text_report, &editor_target_id);

    let seeded_get = wait_for_textedit_value_exact(binary, port, &editor_target_id, "AB");
    assert!(
        seeded_get["element"]["value"].as_str() == Some("AB"),
        "TextEdit should contain hotkey seed text before backspace delivery: {}",
        json_excerpt(&seeded_get, 4000)
    );

    let pid_key_response = run_control_command(
        binary,
        port,
        &format!("@key#330:{{key:\"Backspace\",delivery:\"pid-targeted\",pid:{target_pid}}}\n"),
        Duration::from_secs(15),
    );
    let pid_key_report = successful_response_value(pid_key_response, "@key pid-targeted", binary);
    assert_key_delivery_report(
        &pid_key_report,
        "pid-targeted",
        "Backspace",
        target_pid,
        None,
    );

    let first_get = wait_for_textedit_value_exact(binary, port, &editor_target_id, "A");
    assert!(
        first_get["element"]["value"].as_str() == Some("A"),
        "TextEdit should reflect pid-targeted backspace delivery: {}",
        json_excerpt(&first_get, 4000)
    );

    let window_key_response = run_control_command(
        binary,
        port,
        &format!(
            "@key#331:{{key:\"Backspace\",delivery:\"window-targeted\",window_id:\"{window_id}\"}}\n"
        ),
        Duration::from_secs(15),
    );
    let window_key_report =
        successful_response_value(window_key_response, "@key window-targeted", binary);
    assert_key_delivery_report(
        &window_key_report,
        "window-targeted",
        "Backspace",
        target_pid,
        Some(&window_id),
    );

    let second_get = wait_for_textedit_value_exact(binary, port, &editor_target_id, "");
    assert!(
        second_get["element"]["value"]
            .as_str()
            .is_some_and(str::is_empty),
        "TextEdit value should become empty after pid/window targeted backspace delivery: {}",
        json_excerpt(&second_get, 4000)
    );

    eprintln!(
        "live targeted-hotkey E2E observed TextEdit target: window_id={}, target_id={}, pid={}",
        window_id, editor_target_id, target_pid
    );

    daemon.stop_terminal();
}

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_scroll_real_textedit_without_mouse() {
    if !live_ax_e2e_enabled() {
        eprintln!(
            "skipping live AX scroll E2E; set {LIVE_AX_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    let long_text = (0..400)
        .map(|index| format!("scroll line {index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let daemon = start_live_ax_daemon("textedit-scroll");
    let binary = daemon.binary.as_path();
    let port = daemon.port;
    let fixture = TextEditDocumentFixture::create_with_contents("scroll", &long_text);
    fixture.set_bounds(80, 80, 480, 260);

    let window_id =
        wait_for_textedit_window_id(binary, port, &fixture.title_needle, "@window-find scroll");
    let window_tree = read_textedit_window_tree(binary, port, &window_id, "@ax-get scroll initial");
    let editor_target_id = find_first_textedit_editor_target(&window_tree).unwrap_or_else(|| {
        panic!(
            "scroll E2E should find a TextEdit editor target in window tree: {}",
            json_excerpt(&window_tree, 4000)
        )
    });
    let scrollbar_target_id = find_first_scrollbar_target_id(&window_tree).unwrap_or_else(|| {
        panic!(
            "scroll E2E should find an AXScrollBar target before scrolling: {}",
            json_excerpt(&window_tree, 4000)
        )
    });
    let initial_scrollbar_get = read_scrollbar_state(binary, port, &scrollbar_target_id);
    let initial_scrollbar =
        extract_scrollbar_position(&initial_scrollbar_get).unwrap_or_else(|| {
            panic!(
                "scroll E2E should read AXScrollBar position/value before scrolling: {}",
                json_excerpt(&initial_scrollbar_get, 4000)
            )
        });

    let scroll_response = run_control_command(
        binary,
        port,
        &format!(
            "@ax-scroll#340:{{target:{{id:\"{editor_target_id}\"}},direction:\"down\",pages:2}}\n"
        ),
        Duration::from_secs(15),
    );
    let scroll_report = successful_response_value(scroll_response, "@ax-scroll", binary);
    assert_ax_scroll_report(&scroll_report, &editor_target_id, 2);

    let scrolled_tree =
        wait_for_scrollbar_value_change(binary, port, &scrollbar_target_id, initial_scrollbar);
    let scrolled_value = extract_scrollbar_position(&scrolled_tree).unwrap_or_else(|| {
        panic!(
            "scroll E2E should still expose AXScrollBar position/value after scrolling: {}",
            json_excerpt(&scrolled_tree, 4000)
        )
    });
    assert!(
        scrolled_value > initial_scrollbar,
        "AX scroll should increase TextEdit scroll bar value: before={initial_scrollbar}, after={scrolled_value}\n{}",
        json_excerpt(&scrolled_tree, 4000)
    );

    eprintln!(
        "live AX scroll E2E observed TextEdit scroll bar change: window_id={}, target_id={}, before={}, after={}",
        window_id, editor_target_id, initial_scrollbar, scrolled_value
    );

    daemon.stop_terminal();
}
