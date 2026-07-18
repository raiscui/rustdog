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

use super::RDOG_NAMESPACE;

pub(super) fn next_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("ephemeral port probe should bind")
        .local_addr()
        .expect("ephemeral port probe should expose local addr")
        .port()
}

pub(super) fn rdog_binary_path() -> PathBuf {
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
    write_temp_zenoh_router_config_for_namespace(
        daemon_name,
        RDOG_NAMESPACE,
        listen_endpoints,
        mode,
        unixpipe_enabled,
        false,
        None,
    )
}

fn write_temp_zenoh_router_config_for_namespace(
    daemon_name: &str,
    namespace: &str,
    listen_endpoints: &[String],
    mode: &str,
    unixpipe_enabled: bool,
    local_default: bool,
    unixpipe_socket_path: Option<&Path>,
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
        let socket_path = unixpipe_socket_path
            .map(|path| format!("socket_path = \"{}\"\n", path.display()))
            .unwrap_or_default();
        format!(
            "\n[zenoh.unixpipe]\nenabled = true\nlocal_default = {local_default}\n{socket_path}"
        )
    } else {
        "\n[zenoh.unixpipe]\nenabled = false\n".to_string()
    };

    let contents = format!(
        r#"[zenoh]
enabled = true
mode = "{mode}"
namespace = "{namespace}"
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

/// 每个 e2e 用例独占的 XDG state home。
pub(super) struct TestStateHome {
    path: PathBuf,
}

impl TestStateHome {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            path: create_state_home(prefix),
        }
    }

    pub(super) fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TestStateHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 持有 daemon 子进程和本轮全部临时资源,panic 时也会自动收口。
pub(super) struct TestDaemon {
    child: Child,
    config_path: PathBuf,
    output: Arc<Mutex<String>>,
    output_threads: Vec<thread::JoinHandle<()>>,
    owned_state_home: Option<PathBuf>,
}

impl TestDaemon {
    pub(super) fn output(&self) -> &Arc<Mutex<String>> {
        &self.output
    }

    /// 模拟daemon崩溃:Unix上的 `Child::kill` 发送SIGKILL,不会运行子进程Drop.
    pub(super) fn kill_abruptly(&mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        for thread in self.output_threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_file(&self.config_path);
        if let Some(state_home) = self.owned_state_home.as_ref() {
            let _ = fs::remove_dir_all(state_home);
        }
    }
}

fn create_state_home(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-{prefix}-state-{}-{}",
        std::process::id(),
        next_port()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("state home should be created");
    path
}

fn spawn_zenoh_daemon(
    config_path: PathBuf,
    xdg_state_home: &Path,
    owned_state_home: Option<PathBuf>,
) -> TestDaemon {
    let mut child = Command::new(rdog_binary_path())
        .args(["daemon", "-c", config_path.display().to_string().as_str()])
        .env("XDG_STATE_HOME", xdg_state_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zenoh daemon should start");

    let daemon_stdout = child.stdout.take().expect("daemon stdout should exist");
    let daemon_stderr = child.stderr.take().expect("daemon stderr should exist");
    let output = Arc::new(Mutex::new(String::new()));
    let output_threads = vec![
        spawn_output_collector(daemon_stdout, Arc::clone(&output)),
        spawn_output_collector(daemon_stderr, Arc::clone(&output)),
    ];
    TestDaemon {
        child,
        config_path,
        output,
        output_threads,
        owned_state_home,
    }
}

/// 启动 Zenoh daemon 并把 stdout + stderr 合成到一个 buffer。
pub(super) fn start_zenoh_daemon_with_combined_output(
    name: &str,
    listen_port: u16,
    unixpipe_enabled: bool,
) -> TestDaemon {
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path =
        write_temp_zenoh_router_config(name, &[entrypoint.clone()], "router", unixpipe_enabled);
    let state_home = create_state_home("unixpipe-daemon");
    spawn_zenoh_daemon(config_path, &state_home, Some(state_home.clone()))
}

pub(super) fn start_zenoh_daemon_with_namespace_and_local_default(
    name: &str,
    namespace: &str,
    listen_port: u16,
    unixpipe_enabled: bool,
    local_default: bool,
    xdg_state_home: &PathBuf,
    unixpipe_socket_path: Option<&Path>,
) -> TestDaemon {
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path = write_temp_zenoh_router_config_for_namespace(
        name,
        namespace,
        &[entrypoint.clone()],
        "router",
        unixpipe_enabled,
        local_default,
        unixpipe_socket_path,
    );
    spawn_zenoh_daemon(config_path, xdg_state_home, None)
}

pub(super) fn wait_for_marker(
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

/// 在限定时间内等待子进程退出,避免重复 daemon 异常路径让测试永久阻塞。
pub(super) fn wait_for_child_exit(
    daemon: &mut TestDaemon,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match daemon.child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    None
}

/// 跑 `rdog control <target> @ping` 并返回 (exit_status, stdout, stderr)。
pub(super) fn run_control_ping(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    run_control_with_args_and_env(args, None)
}

pub(super) fn run_control_with_args_and_env(
    args: &[&str],
    xdg_state_home: Option<&Path>,
) -> (std::process::ExitStatus, String, String) {
    let mut command = Command::new(rdog_binary_path());
    command
        .arg("control")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(state_home) = xdg_state_home {
        command.env("XDG_STATE_HOME", state_home);
    }
    let mut child = command.spawn().expect("control should start");

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
