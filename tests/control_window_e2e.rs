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

const LIVE_WINDOW_E2E_ENV: &str = "RDOG_LIVE_WINDOW_E2E";
const LIVE_WINDOW_E2E_BINARY_ENV: &str = "RDOG_LIVE_WINDOW_E2E_BINARY";
const LIVE_WINDOW_E2E_VIA_TERMINAL_ENV: &str = "RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL";
const LIVE_AX_E2E_BINARY_ENV: &str = "RDOG_LIVE_AX_E2E_BINARY";
const LIVE_AX_E2E_VIA_TERMINAL_ENV: &str = "RDOG_LIVE_AX_E2E_VIA_TERMINAL";
const TERMINAL_WINDOW_MARKER_PREFIX: &str = "rdog-window-e2e-";

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
        close_terminal_windows_matching(TERMINAL_WINDOW_MARKER_PREFIX);
    }
}

impl Drop for TerminalDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug)]
struct LiveWindowDaemon {
    port: u16,
    binary: PathBuf,
    _workdir: PathBuf,
    _direct_daemon: Option<ChildGuard>,
    terminal_daemon: Option<TerminalDaemon>,
}

impl LiveWindowDaemon {
    fn uses_terminal_host(&self) -> bool {
        self.terminal_daemon.is_some()
    }
}

#[derive(Debug)]
struct TextEditOccluder {
    file_path: PathBuf,
    title_needle: String,
}

impl TextEditOccluder {
    fn create(name: &str) -> Self {
        let file_path = std::env::temp_dir().join(format!(
            "rdog-window-occluder-{name}-{}-{}.txt",
            std::process::id(),
            next_free_port()
        ));
        fs::write(&file_path, b"rdog window occluder\n")
            .expect("TextEdit occluder file should write");

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
            .expect("TextEdit occluder file name should be UTF-8")
            .to_owned();

        Self {
            file_path,
            title_needle,
        }
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

impl Drop for TextEditOccluder {
    fn drop(&mut self) {
        self.close_without_saving();
        let _ = fs::remove_file(&self.file_path);
    }
}

#[derive(Debug)]
struct FinderFixture {
    dir_path: PathBuf,
    title_needle: String,
}

impl FinderFixture {
    fn create(name: &str) -> Self {
        let dir_path = std::env::temp_dir().join(format!(
            "rdog-window-e2e-{name}-{}-{}",
            std::process::id(),
            next_free_port()
        ));
        fs::create_dir_all(&dir_path).expect("Finder fixture directory should create");

        let status = Command::new("open")
            .arg("-a")
            .arg("Finder")
            .arg(&dir_path)
            .status()
            .expect("open -a Finder should run");
        assert!(status.success(), "open -a Finder should succeed");
        run_applescript("tell application \"Finder\" to activate");

        let title_needle = dir_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture directory name should be UTF-8")
            .to_owned();

        Self {
            dir_path,
            title_needle,
        }
    }

    fn close_window(&self) {
        let title = apple_script_string(&self.title_needle);
        let script = format!(
            "tell application \"Finder\"\n\
             repeat with w in windows\n\
               try\n\
                 if (name of w) contains \"{title}\" then\n\
                   close w\n\
                 end if\n\
               end try\n\
             end repeat\n\
             end tell"
        );
        let _ = run_applescript(&script);
    }
}

impl Drop for FinderFixture {
    fn drop(&mut self) {
        self.close_window();
        let _ = fs::remove_dir_all(&self.dir_path);
    }
}

fn finder_query(title_needle: &str) -> String {
    format!(
        "{{app:\"Finder\",title_contains:\"{}\",limit:5,include_state:true,include_recipes:true}}",
        title_needle.replace('\"', "\\\"")
    )
}

fn finder_window_find(binary: &Path, port: u16, title_needle: &str) -> Value {
    let response = run_control_command(
        binary,
        port,
        &format!("@window-find#100:{}\n", finder_query(title_needle)),
        Duration::from_secs(20),
    );
    successful_response_value(response, "@window-find", binary)
}

fn wait_for_finder_window(
    binary: &Path,
    port: u16,
    title_needle: &str,
    timeout: Duration,
) -> Value {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let find = finder_window_find(binary, port, title_needle);
        if find["match_count"].as_u64().unwrap_or(0) > 0 {
            return find;
        }
        thread::sleep(Duration::from_millis(250));
    }

    panic!("Finder window query never matched title needle: {title_needle}");
}

fn wait_for_finder_window_state(
    binary: &Path,
    port: u16,
    title_needle: &str,
    predicate_label: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);

    while Instant::now() < deadline {
        let find = wait_for_finder_window(binary, port, title_needle, Duration::from_secs(2));
        let candidate = first_match(&find);
        if predicate(candidate) {
            return find;
        }
        thread::sleep(Duration::from_millis(250));
    }

    let find = finder_window_find(binary, port, title_needle);
    panic!(
        "Finder window never reached expected state `{predicate_label}`: {}",
        json_excerpt(&find, 4000)
    );
}

fn wait_for_no_finder_window(
    binary: &Path,
    port: u16,
    title_needle: &str,
    timeout: Duration,
) -> Value {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let find = finder_window_find(binary, port, title_needle);
        if find["match_count"].as_u64().unwrap_or(0) == 0 {
            return find;
        }
        thread::sleep(Duration::from_millis(250));
    }

    let find = finder_window_find(binary, port, title_needle);
    panic!(
        "Finder window should be gone after close, but query still matched: {}",
        json_excerpt(&find, 4000)
    );
}

fn start_live_window_daemon(name: &str) -> LiveWindowDaemon {
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

    LiveWindowDaemon {
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
    for key in [LIVE_WINDOW_E2E_BINARY_ENV, LIVE_AX_E2E_BINARY_ENV] {
        if let Some(path) = std::env::var_os(key) {
            let binary = PathBuf::from(path);
            assert!(
                binary.exists(),
                "{key} points to a missing binary: {}",
                binary.display()
            );
            return binary;
        }
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
        "rdog-control-window-e2e-{name}-{}-{}",
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
        .expect("lsof should be available for macOS live window E2E");

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

fn live_window_e2e_enabled() -> bool {
    matches!(
        std::env::var(LIVE_WINDOW_E2E_ENV).ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn terminal_host_enabled() -> bool {
    [
        LIVE_WINDOW_E2E_VIA_TERMINAL_ENV,
        LIVE_AX_E2E_VIA_TERMINAL_ENV,
    ]
    .into_iter()
    .find_map(|key| std::env::var(key).ok())
    .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
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
    close_terminal_windows_matching(TERMINAL_WINDOW_MARKER_PREFIX);

    let script_path = std::env::temp_dir().join(format!("rdog-window-e2e-{port}.command"));
    let log_path = std::env::temp_dir().join(format!("rdog-window-e2e-{port}.log"));
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
                "{label} reached rdog window backend, but macOS denied Accessibility permission for the actual daemon process.\n\
                 binary: {}\n\
                 response: {}\n\
                 If Terminal is already authorized, rerun with {LIVE_WINDOW_E2E_VIA_TERMINAL_ENV}=1 so Terminal launches the daemon.",
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

fn apple_script_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\"', "\\\"")
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

fn close_terminal_windows_matching(marker_contains: &str) {
    let marker = apple_script_string(marker_contains);
    let script = format!(
        "tell application \"Terminal\"\n\
         try\n\
           close (every window whose name contains \"{marker}\")\n\
         end try\n\
         end tell"
    );
    let _ = try_run_applescript(&script);
}

fn first_match(find: &Value) -> &Value {
    assert_eq!(find["kind"].as_str(), Some("window-find"));
    assert_eq!(find["schema"].as_str(), Some("rdog.window.v1"));
    assert_eq!(find["platform"].as_str(), Some("macos"));
    assert_eq!(find["status"].as_str(), Some("complete"));

    find["matches"]
        .as_array()
        .and_then(|matches| matches.first())
        .unwrap_or_else(|| panic!("window-find should contain at least one match: {find}"))
}

fn rect_field_i64(candidate: &Value, key: &str) -> i64 {
    candidate["rect"][key]
        .as_i64()
        .unwrap_or_else(|| panic!("window candidate should contain rect.{key}: {candidate}"))
}

fn window_id(candidate: &Value) -> String {
    candidate["window_id"]
        .as_str()
        .unwrap_or_else(|| panic!("window candidate should contain window_id: {candidate}"))
        .to_owned()
}

fn place_textedit_occluder_over_finder(occluder: &TextEditOccluder, finder_candidate: &Value) {
    let finder_x = rect_field_i64(finder_candidate, "x");
    let finder_y = rect_field_i64(finder_candidate, "y");
    let finder_width = rect_field_i64(finder_candidate, "width");
    let finder_height = rect_field_i64(finder_candidate, "height");

    let occluder_width = (finder_width / 2).clamp(260, 640);
    let occluder_height = (finder_height / 2).clamp(180, 420);
    let right = finder_x + occluder_width;
    let bottom = finder_y + occluder_height;
    let title = apple_script_string(&occluder.title_needle);

    run_applescript(&format!(
        "tell application \"TextEdit\"\n\
         activate\n\
         repeat with w in windows\n\
           try\n\
             if (name of w) contains \"{title}\" then\n\
               set bounds of w to {{{finder_x}, {finder_y}, {right}, {bottom}}}\n\
               exit repeat\n\
             end if\n\
           end try\n\
         end repeat\n\
         end tell"
    ));
}

fn assert_bool_field(value: &Value, path: &[&str], expected: bool, label: &str) {
    let mut cursor = value;
    for segment in path {
        cursor = cursor
            .get(*segment)
            .unwrap_or_else(|| panic!("{label} missing field `{segment}`: {value}"));
    }
    assert_eq!(
        cursor.as_bool(),
        Some(expected),
        "{label} expected {} to be {expected}: {}",
        path.join("."),
        json_excerpt(value, 4000)
    );
}

fn assert_activate_report(
    report: &Value,
    expected_window_id: &str,
    required_steps: &[&str],
    expected_status: &str,
) {
    assert_eq!(report["kind"].as_str(), Some("window-action"));
    assert_eq!(report["schema"].as_str(), Some("rdog.window.v1"));
    assert_eq!(report["platform"].as_str(), Some("macos"));
    assert_eq!(report["action"].as_str(), Some("activate"));
    assert_eq!(
        report["status"].as_str(),
        Some(expected_status),
        "unexpected activate report: {}",
        json_excerpt(report, 4000)
    );
    assert_eq!(report["window_id"].as_str(), Some(expected_window_id));

    let steps = report["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("activate report should contain steps: {report}"));

    for required_step in required_steps {
        assert!(
            steps
                .iter()
                .any(|step| step["step"].as_str() == Some(required_step)),
            "activate report should include step `{required_step}`: {}",
            json_excerpt(report, 4000)
        );
    }
}

fn activate_window(binary: &Path, port: u16, window_id: &str) -> Value {
    let response = run_control_command(
        binary,
        port,
        &format!("@window-activate#101:{{window_id:\"{window_id}\"}}\n"),
        Duration::from_secs(20),
    );
    successful_response_value(response, "@window-activate", binary)
}

fn close_window(binary: &Path, port: u16, window_id: &str) -> Value {
    let response = run_control_command(
        binary,
        port,
        &format!("@window-close#102:{{window_id:\"{window_id}\"}}\n"),
        Duration::from_secs(20),
    );
    successful_response_value(response, "@window-close", binary)
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

#[test]
#[ignore = "requires a visible macOS desktop and Accessibility permission for the actual daemon host"]
fn daemon_control_lane_should_find_activate_and_close_real_hidden_minimized_occluded_window() {
    if !live_window_e2e_enabled() {
        eprintln!(
            "skipping live window E2E; set {LIVE_WINDOW_E2E_ENV}=1 to run against the real macOS desktop"
        );
        return;
    }

    let daemon = start_live_window_daemon("finder-window-state");
    let binary = daemon.binary.as_path();
    let port = daemon.port;

    if !daemon.uses_terminal_host() {
        eprintln!(
            "running live window E2E without Terminal host. If Accessibility is only granted to Terminal, rerun with {LIVE_WINDOW_E2E_VIA_TERMINAL_ENV}=1."
        );
    }

    let fixture = FinderFixture::create("states");
    let title = fixture.title_needle.clone();
    let initial_find = wait_for_finder_window_state(
        binary,
        port,
        &title,
        "baseline current-space interactable",
        |candidate| {
            candidate["state"]["current_space"].as_bool() == Some(true)
                && candidate["state"]["interactable"].as_bool() == Some(true)
        },
    );
    let initial_candidate = first_match(&initial_find);

    let occluder = TextEditOccluder::create("finder-occluder");
    place_textedit_occluder_over_finder(&occluder, initial_candidate);
    let occluded_find =
        wait_for_finder_window_state(binary, port, &title, "occluded", |candidate| {
            candidate["state"]["occluded"].as_bool() == Some(true)
                && candidate["state"]["interactable"].as_bool() == Some(false)
                && candidate["state"]["current_space"].as_bool() == Some(true)
        });
    let occluded_candidate = first_match(&occluded_find);
    let occluded_window_id = window_id(occluded_candidate);
    assert_bool_field(
        occluded_candidate,
        &["state", "occluded"],
        true,
        "occluded candidate",
    );
    assert_bool_field(
        occluded_candidate,
        &["state", "current_space"],
        true,
        "occluded candidate",
    );
    assert_bool_field(
        occluded_candidate,
        &["state", "interactable"],
        false,
        "occluded candidate",
    );

    let occluded_activate = activate_window(binary, port, &occluded_window_id);
    assert_activate_report(
        &occluded_activate,
        &occluded_window_id,
        &["activate_app", "raise_window"],
        "ok",
    );
    let occluded_recovered = wait_for_finder_window_state(
        binary,
        port,
        &title,
        "interactable after occluded activate",
        |candidate| {
            candidate["state"]["occluded"].as_bool() == Some(false)
                && candidate["state"]["interactable"].as_bool() == Some(true)
        },
    );
    let occluded_recovered_candidate = first_match(&occluded_recovered);
    assert_bool_field(
        occluded_recovered_candidate,
        &["state", "interactable"],
        true,
        "recovered occluded candidate",
    );
    drop(occluder);

    let title_script = apple_script_string(&title);
    run_applescript(&format!(
        "tell application \"System Events\"\n\
         tell process \"Finder\"\n\
           set value of attribute \"AXMinimized\" of (first window whose name contains \"{title_script}\") to true\n\
         end tell\n\
         end tell"
    ));
    let minimized_find =
        wait_for_finder_window_state(binary, port, &title, "minimized", |candidate| {
            candidate["state"]["minimized"].as_bool() == Some(true)
                && candidate["state"]["interactable"].as_bool() == Some(false)
        });
    let minimized_candidate = first_match(&minimized_find);
    let minimized_window_id = window_id(minimized_candidate);
    assert_bool_field(
        minimized_candidate,
        &["state", "minimized"],
        true,
        "minimized candidate",
    );

    let minimized_activate = activate_window(binary, port, &minimized_window_id);
    assert_activate_report(
        &minimized_activate,
        &minimized_window_id,
        &["unminimize_window", "activate_app", "raise_window"],
        "ok",
    );
    let minimized_recovered = wait_for_finder_window_state(
        binary,
        port,
        &title,
        "interactable after minimize activate",
        |candidate| {
            candidate["state"]["minimized"].as_bool() == Some(false)
                && candidate["state"]["interactable"].as_bool() == Some(true)
        },
    );
    let minimized_recovered_candidate = first_match(&minimized_recovered);
    assert_bool_field(
        minimized_recovered_candidate,
        &["state", "interactable"],
        true,
        "recovered minimized candidate",
    );

    run_applescript(
        "tell application \"System Events\"\n\
         tell process \"Finder\"\n\
           set visible to false\n\
         end tell\n\
         end tell",
    );
    let hidden_find = wait_for_finder_window_state(binary, port, &title, "hidden", |candidate| {
        candidate["state"]["app_hidden"].as_bool() == Some(true)
            && candidate["state"]["interactable"].as_bool() == Some(false)
    });
    let hidden_candidate = first_match(&hidden_find);
    let hidden_window_id = window_id(hidden_candidate);
    assert_bool_field(
        hidden_candidate,
        &["state", "app_hidden"],
        true,
        "hidden candidate",
    );

    let hidden_activate = activate_window(binary, port, &hidden_window_id);
    assert_activate_report(
        &hidden_activate,
        &hidden_window_id,
        &["unhide_app", "activate_app", "raise_window"],
        "ok",
    );
    let hidden_recovered = wait_for_finder_window_state(
        binary,
        port,
        &title,
        "interactable after hidden activate",
        |candidate| {
            candidate["state"]["app_hidden"].as_bool() == Some(false)
                && candidate["state"]["interactable"].as_bool() == Some(true)
        },
    );
    let hidden_recovered_candidate = first_match(&hidden_recovered);
    let close_window_id = window_id(hidden_recovered_candidate);
    assert_bool_field(
        hidden_recovered_candidate,
        &["state", "interactable"],
        true,
        "recovered hidden candidate",
    );

    let close_report = close_window(binary, port, &close_window_id);
    assert_eq!(close_report["kind"].as_str(), Some("window-action"));
    assert_eq!(close_report["action"].as_str(), Some("close"));
    assert_eq!(close_report["status"].as_str(), Some("ok"));
    assert_eq!(close_report["strategy"].as_str(), Some("graceful"));
    assert_eq!(
        close_report["window_id"].as_str(),
        Some(close_window_id.as_str())
    );

    let closed_find = wait_for_no_finder_window(binary, port, &title, Duration::from_secs(15));
    assert_eq!(closed_find["match_count"].as_u64(), Some(0));
}
