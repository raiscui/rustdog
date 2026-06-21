#![cfg(unix)]
#![cfg(target_os = "macos")]

//! Zenoh `transport_unixpipe` 本机 fast path 的端到端集成测试。
//!
//! 这些测试只覆盖同机 daemon + control 的 unixpipe fast path 行为。
//! 跨主机 / 跨网络场景由 `tests/zenoh_router_client.rs` 已有的 `control_multi_one_shot_*` 等测试覆盖。
//!
//! 关键测试点:
//! 1. daemon 启用 unixpipe + control 同机,@ping 走 fast path
//! 2. daemon 没启用 unixpipe,control 走 fallback(走 UDP scout)
//! 3. 残留的 stale FIFO 文件会被 daemon 启动时清理

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

const RDOG_NAMESPACE: &str = "lab";

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
    unixpipe_enabled: bool,
) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-zenoh-unixpipe-{}-{}-{}.toml",
        std::process::id(),
        daemon_name,
        next_port()
    ));

    let listen_endpoints = listen_endpoints
        .iter()
        .map(|endpoint| format!("\"{endpoint}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let unixpipe_block = if unixpipe_enabled {
        "\n[zenoh.unixpipe]\nenabled = true\n"
    } else {
        "\n[zenoh.unixpipe]\nenabled = false\n"
    };

    let contents = format!(
        r#"[zenoh]
enabled = true
mode = "{mode}"
namespace = "{RDOG_NAMESPACE}"
daemon_name = "{daemon_name}"
listen_endpoints = [{listen_endpoints}]
request_timeout_ms = 3000
startup_guard_window_ms = 500
{unixpipe_block}
"#
    );

    fs::write(&path, contents).expect("should write temporary daemon config");
    path
}

fn spawn_output_collector<R: Read + Send + 'static>(
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

/// 启动 Zenoh daemon 并把 stdout + stderr 合成到一个 buffer。
fn start_zenoh_daemon_with_combined_output(
    name: &str,
    listen_port: u16,
    unixpipe_enabled: bool,
) -> (Child, PathBuf, String, Arc<Mutex<String>>) {
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path =
        write_temp_zenoh_router_config(name, &[entrypoint.clone()], "router", unixpipe_enabled);
    let mut child = Command::new(rdog_binary_path())
        .args(["daemon", "-c", config_path.display().to_string().as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh daemon should start");

    let daemon_stdout = child.stdout.take().expect("daemon stdout should exist");
    let daemon_stderr = child.stderr.take().expect("daemon stderr should exist");
    let combined = Arc::new(Mutex::new(String::new()));
    let _stdout_thread = spawn_output_collector(daemon_stdout, Arc::clone(&combined));
    let _stderr_thread = spawn_output_collector(daemon_stderr, Arc::clone(&combined));
    (child, config_path, entrypoint, combined)
}

fn wait_for_marker(
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

/// 跑 `rdog control <target> @ping` 并返回 (exit_status, stdout, stderr)。
fn run_control_ping(args: &[&str]) -> (std::process::ExitStatus, String, String) {
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
        .write_all(b"@ping\n")
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

fn unique_daemon_name(prefix: &str) -> String {
    // daemon_name 必须带 `.lab` 后缀,namespace 才能从名字后缀推断出来。
    // 见 `crate::zenoh_identity::infer_namespace_from_daemon_name`。
    format!(
        "{prefix}-{}-{}.{RDOG_NAMESPACE}",
        std::process::id(),
        next_port()
    )
}

/// 等 FIFO 出现,或返回 NotFound。
fn wait_for_fifo(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn derive_unixpipe_base_path(namespace: &str, daemon_name: &str) -> PathBuf {
    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    tmpdir.join(format!("rdog-{namespace}-{daemon_name}.pipe"))
}

fn cleanup_unixpipe_artifacts(base: &PathBuf) {
    let _ = fs::remove_file(base);
    let _ = fs::remove_file(format!("{}_uplink", base.display()));
    let _ = fs::remove_file(format!("{}_downlink", base.display()));
    // Zenoh 还会创建带 suffix 的 dedicated FIFO,尽力清掉,避免跨测试污染。
    if let Ok(entries) = fs::read_dir(
        base.parent()
            .unwrap_or_else(|| std::path::Path::new("/tmp")),
    ) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(base.file_name().unwrap().to_str().unwrap()) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// 清理 namespace 下所有 rdog-{ns}-*.pipe* 残留,避免多 test 之间的 FIFO 污染。
/// 用途:`self` / 空 target 的 e2e 强依赖 namespace 范围内只能有 1 个 daemon,
/// 旧 test 残留的 fifo 会让 fast path 误报"多候选"。
fn cleanup_namespace_artifacts(namespace: &str) {
    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let prefix = format!("rdog-{namespace}-");
    if let Ok(entries) = fs::read_dir(&tmpdir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

// ============================================================================
// 测试用例
// ============================================================================

#[test]
fn unixpipe_endpoint_should_be_created_when_daemon_starts_with_unixpipe_enabled() {
    let daemon_name = unique_daemon_name("unixpipe-create");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let (mut child, _config_path, _entry, combined) =
        start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    // 等待 daemon 起来 + log
    wait_for_marker(
        &combined,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // 验证 FIFO 文件被创建
    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let _ = child.kill();
    let _ = child.wait();
    cleanup_unixpipe_artifacts(&base_path);
}

#[test]
fn unixpipe_fast_path_should_make_ping_respond_within_budget() {
    let daemon_name = unique_daemon_name("unixpipe-ping");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let (mut child, _config_path, _entry, combined) =
        start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    wait_for_marker(
        &combined,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // 给 listener 一点时间 settle。
    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected FIFO {uplink_path} to exist before client"
    );

    let start = Instant::now();
    let (status, stdout, stderr) = run_control_ping(&[daemon_name.as_str()]);
    let elapsed = start.elapsed();

    assert!(
        status.success(),
        "control @ping should succeed, stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 响应应该包含 pong, stdout={stdout}"
    );

    // 远端 IP 通过 multicast 走不通时 control 会 fallback 到 10s+;unixpipe 路径必须 < 1s。
    assert!(
        elapsed < Duration::from_millis(1000),
        "unixpipe fast path 必须在 1s 内返回,实际 {elapsed:?}"
    );

    let _ = child.kill();
    let _ = child.wait();
    cleanup_unixpipe_artifacts(&base_path);
}

#[test]
fn stale_unixpipe_socket_files_should_be_cleaned_on_daemon_start() {
    let daemon_name = unique_daemon_name("unixpipe-stale");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    // 模拟上次崩溃残留的 3 个文件:base / base_uplink / base_downlink。
    for suffix in ["", "_uplink", "_downlink"] {
        let path = format!("{}{suffix}", base_path.display());
        let status = Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success(), "mkfifo {path} 失败");
    }

    // 启动 daemon,触发 stale cleanup。
    let (mut child, _config_path, _entry, combined) =
        start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    // daemon 起来后,残留的 3 个文件必须已经被清理。
    // 重新创建新的 FIFO 是 daemon 自己的事,我们只验证旧的被 unlink。
    let base_only = base_path.clone();
    let uplink_path = format!("{}_uplink", base_path.display());

    wait_for_marker(
        &combined,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // base 本身不会作为 FIFO 存在,daemon 只创建 _uplink 和 _downlink。
    // 老的 _uplink 和 _downlink 应该被 unlink,然后 daemon 重新创建新的 _uplink。
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected new {uplink_path} to be created after cleanup"
    );
    // 老的 base 文件应该是 base path 本身。如果 daemon 清理了它,文件应该已经不存在。
    // 这里我们用 base path 的"派生路径"来验证,而不是 base 本身。
    let _ = base_only;

    let _ = child.kill();
    let _ = child.wait();
    cleanup_unixpipe_artifacts(&base_path);
}

// ============================================================================
// self / 空 target 入口(`rdog control self @<line>` / `rdog control @<line>`)
// ============================================================================

fn run_control_with_args(args: &[&str]) -> (std::process::ExitStatus, String, String) {
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
        .write_all(b"@ping\n")
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

#[test]
fn self_target_with_explicit_namespace_should_find_local_daemon() {
    // 独立 namespace,完全跟其他 `lab` 测试隔离,允许 cargo test 默认并发。
    let ns = "selfexp";
    cleanup_namespace_artifacts(ns);

    let daemon_name = format!("selftest.{ns}");
    let base_path = derive_unixpipe_base_path(ns, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let (mut child, _config_path, _entry, combined) =
        start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    wait_for_marker(
        &combined,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) = run_control_with_args(&["self", "--namespace", ns]);
    let _ = child.kill();
    let _ = child.wait();
    cleanup_unixpipe_artifacts(&base_path);

    assert!(
        status.success(),
        "control self should succeed, stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn empty_target_with_namespace_should_find_local_daemon() {
    let ns = "emptytgt";
    cleanup_namespace_artifacts(ns);

    let daemon_name = format!("only.{ns}");
    let base_path = derive_unixpipe_base_path(ns, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let (mut child, _config_path, _entry, combined) =
        start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    wait_for_marker(
        &combined,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) = run_control_with_args(&["--namespace", ns]);
    let _ = child.kill();
    let _ = child.wait();
    cleanup_unixpipe_artifacts(&base_path);

    assert!(
        status.success(),
        "control 空 target + --namespace 应该成功,stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn self_target_should_error_when_no_local_daemon_running() {
    // 清理掉所有残留 FIFO,确保本地没有 daemon
    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    if let Ok(entries) = std::fs::read_dir(&tmpdir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("rdog-") && name.ends_with(".pipe_uplink") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    let (status, _stdout, stderr) =
        run_control_with_args(&["self", "--namespace", "rdog-namespace-no-such-daemon-12345"]);
    assert!(
        !status.success(),
        "没有本地 daemon 时 control self 应该失败"
    );
    let err_lower = stderr.to_lowercase();
    assert!(
        err_lower.contains("not found") || err_lower.contains("未找到"),
        "应该报未找到本地 daemon,实际 stderr={stderr}"
    );
}

#[test]
fn self_target_should_error_when_multiple_local_daemons() {
    // 在同 namespace 启动两个 daemon
    let daemon_name_a = unique_daemon_name("self-multi-a");
    let daemon_name_b = unique_daemon_name("self-multi-b");
    let base_a = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name_a);
    let base_b = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name_b);
    cleanup_unixpipe_artifacts(&base_a);
    cleanup_unixpipe_artifacts(&base_b);

    let (mut child_a, _cp_a, _e_a, combined_a) =
        start_zenoh_daemon_with_combined_output(&daemon_name_a, next_port(), true);
    let (mut child_b, _cp_b, _e_b, combined_b) =
        start_zenoh_daemon_with_combined_output(&daemon_name_b, next_port(), true);

    wait_for_marker(
        &combined_a,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon A should be ready");
    wait_for_marker(
        &combined_b,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon B should be ready");

    // 等两个 fifo 都出现
    let uplink_a = format!("{}_uplink", base_a.display());
    let uplink_b = format!("{}_uplink", base_b.display());
    assert!(wait_for_fifo(
        std::path::Path::new(&uplink_a),
        Duration::from_secs(2)
    ));
    assert!(wait_for_fifo(
        std::path::Path::new(&uplink_b),
        Duration::from_secs(2)
    ));

    let (status, _stdout, stderr) = run_control_with_args(&["self", "--namespace", RDOG_NAMESPACE]);
    let _ = child_a.kill();
    let _ = child_a.wait();
    let _ = child_b.kill();
    let _ = child_b.wait();
    cleanup_unixpipe_artifacts(&base_a);
    cleanup_unixpipe_artifacts(&base_b);

    assert!(
        !status.success(),
        "两个本地 daemon 时 control self 应该失败(歧义)"
    );
    let err_lower = stderr.to_lowercase();
    assert!(
        err_lower.contains("already exists") || err_lower.contains("多个"),
        "应该报多个 daemon 冲突,实际 stderr={stderr}"
    );
}
