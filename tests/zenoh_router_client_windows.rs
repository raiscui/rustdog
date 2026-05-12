#![cfg(windows)]

use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    let binary = debug_dir.join("rdog.exe");

    assert!(
        binary.exists(),
        "expected rdog binary at {}",
        binary.display()
    );

    binary
}

fn spawn_output_collector(
    mut reader: impl Read + Send + 'static,
) -> (Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buffer);
    let handle = thread::spawn(move || {
        let mut chunk = [0_u8; 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return,
                Ok(len) => {
                    let text = String::from_utf8_lossy(&chunk[..len]);
                    shared
                        .lock()
                        .expect("collector buffer lock should work")
                        .push_str(&text);
                }
                Err(_) => return,
            }
        }
    });

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
    Command::new(rdog_binary_path())
        .args(["daemon", "-c", config_path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh daemon with config should start")
}

fn run_control(args: &[&str], line: &str) -> (ExitStatus, String, String) {
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

fn run_control_until_success(
    args: &[&str],
    line: &str,
    expected_fragment: &str,
    timeout: Duration,
) -> Result<(String, String), String> {
    let deadline = Instant::now() + timeout;
    let mut last_output = String::new();

    while Instant::now() < deadline {
        let (status, stdout, stderr) = run_control(args, line);
        let combined = format!("{stdout}\n{stderr}");
        if status.success() && combined.contains(expected_fragment) {
            return Ok((stdout, stderr));
        }

        last_output = format!("status={status}\nstdout:\n{stdout}\nstderr:\n{stderr}");
        thread::sleep(Duration::from_millis(250));
    }

    Err(format!(
        "control did not produce expected fragment `{expected_fragment}` before timeout\n{last_output}"
    ))
}

fn unique_name(prefix: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    format!("{prefix}-{}-{stamp}.lab", std::process::id())
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn write_temp_zenoh_daemon_config(daemon_name: &str, listen_port: u16) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-win-zenoh-{}-{listen_port}.toml",
        std::process::id()
    ));

    let contents = format!(
        r#"[zenoh]
enabled = true
mode = "router"
namespace = "lab"
daemon_name = "{daemon_name}"
listen_endpoints = ["tcp/127.0.0.1:{listen_port}"]
request_timeout_ms = 3000
startup_guard_window_ms = 1000
"#
    );

    fs::write(&path, contents).expect("should write temporary daemon config");
    path
}

#[test]
fn windows_zenoh_router_client_smoke_should_ping_and_run_request_id_command() {
    // 这条 smoke 专门补 Windows 覆盖:
    // 真实拉起 embedded router daemon,再通过 control client 走 @ping 和 @cmd#id。
    let daemon_name = unique_name("win-smoke");
    let listen_port = 17000 + (std::process::id() % 1000) as u16;
    let config_path = write_temp_zenoh_daemon_config(&daemon_name, listen_port);
    let config_path_string = config_path.display().to_string();
    let entry_point = format!("tcp/127.0.0.1:{listen_port}");
    let mut daemon = start_zenoh_daemon_with_config(&config_path_string);
    let daemon_stdout = daemon.stdout.take().expect("daemon stdout should exist");
    let (buffer, _collector) = spawn_output_collector(daemon_stdout);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let control_args = [
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entry_point,
    ];

    let (ping_stdout, ping_stderr) = run_control_until_success(
        &control_args,
        "@ping",
        r#"@response "pong""#,
        Duration::from_secs(12),
    )
    .expect("ping should eventually succeed on Windows smoke path");

    let (cmd_stdout, cmd_stderr) = run_control_until_success(
        &control_args,
        r#"@cmd#7:"echo READY""#,
        "READY",
        Duration::from_secs(12),
    )
    .expect("request-id command should eventually succeed on Windows smoke path");

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        ping_stdout.contains(r#"@response "pong""#),
        "unexpected ping stdout:\n{ping_stdout}\nstderr:\n{ping_stderr}"
    );
    assert!(
        cmd_stdout.contains(r#""id":7"#),
        "unexpected cmd stdout:\n{cmd_stdout}\nstderr:\n{cmd_stderr}"
    );
    assert!(
        cmd_stdout.contains("READY"),
        "unexpected cmd stdout:\n{cmd_stdout}\nstderr:\n{cmd_stderr}"
    );
}

#[test]
fn windows_zenoh_router_client_should_reach_daemon_via_fixed_listen_endpoint() {
    // ------------------------------------------------------------
    // 这条测试覆盖 Windows 上更稳定的 deterministic join:
    // daemon 使用固定 `listen_endpoints`, control 通过 `--entry-point`
    // 以 client 形态接入 embedded router。
    // ------------------------------------------------------------
    let daemon_name = unique_name("win-entry");
    let listen_port = 17000 + (std::process::id() % 1000) as u16;
    let config_path = write_temp_zenoh_daemon_config(&daemon_name, listen_port);
    let config_path_string = config_path.display().to_string();
    let entry_point = format!("tcp/127.0.0.1:{listen_port}");
    let mut daemon = start_zenoh_daemon_with_config(&config_path_string);
    let daemon_stdout = daemon.stdout.take().expect("daemon stdout should exist");
    let (buffer, _collector) = spawn_output_collector(daemon_stdout);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let control_args = [
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entry_point,
    ];

    let (ping_stdout, ping_stderr) = run_control_until_success(
        &control_args,
        "@ping",
        r#"@response "pong""#,
        Duration::from_secs(12),
    )
    .expect("ping should succeed via fixed entry-point on Windows");

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        ping_stdout.contains(r#"@response "pong""#),
        "unexpected ping stdout:\n{ping_stdout}\nstderr:\n{ping_stderr}"
    );
}
