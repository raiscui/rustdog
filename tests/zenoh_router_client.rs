#![cfg(unix)]

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use zenoh::Wait;

const SERIAL_ENDPOINT_EXAMPLE: &str = "serial//dev/ttyFAKE#baudrate=115200";

#[path = "support/tty_script.rs"]
mod tty_script;

#[path = "support/child_guard.rs"]
mod child_guard;

use child_guard::ChildGuard;

#[path = "support/test_ports.rs"]
mod test_ports;

#[path = "support/propagation.rs"]
mod propagation;

use test_ports::allocate_tcp_port;

/// 等到 `declaring` 上的订阅对 router 可见再继续 (RAI-580)。
///
/// 声明传播是异步的: `declare_subscriber().wait()` 只保证本地注册, 不等 router 确认。所以
/// 「声明后立刻触发对端 pub」在跨进程 e2e 里只能靠猜时长 (sleep) 或干脆不猜。这里改成可观测条件:
/// 另开一个 session 去 query 这条链路的 liveliness 探针, router 答得出就说明它已经处理过这份声明。
fn await_subscriber_visible(declaring: &zenoh::Session, entrypoint: &str, keyexpr: &str) {
    let witness = open_authenticated_test_session(entrypoint);
    propagation::await_subscriber_visible(declaring, &witness, keyexpr)
        .unwrap_or_else(|err| panic!("{err}"));
}

/// 认证的测试 zenoh session (issue #82): 读 ~/.rdog/auth.toml 注入 usrpwd
/// 凭证 — daemon 默认开启认证, 裸 session 的 pub 会被拒。

fn open_authenticated_test_session(entrypoint: &str) -> zenoh::Session {
    let mut config = zenoh::Config::default();
    config
        .insert_json5("connect/endpoints", &format!("[\"{entrypoint}\"]"))
        .expect("zenoh config connect");
    if let Some((user, password)) = test_zenoh_credentials() {
        config
            .insert_json5("transport/auth/usrpwd/user", &format!("\"{user}\""))
            .expect("auth user");
        config
            .insert_json5("transport/auth/usrpwd/password", &format!("\"{password}\""))
            .expect("auth password");
    }
    zenoh::open(config)
        .wait()
        .expect("authenticated test session")
}

/// 测试进程的凭证: RDOG_AUTH env 成对或 ~/.rdog/auth.toml。
fn test_zenoh_credentials() -> Option<(String, String)> {
    if let (Ok(user), Ok(password)) = (
        std::env::var("RDOG_AUTH_USER"),
        std::env::var("RDOG_AUTH_PASSWORD"),
    ) {
        return Some((user, password));
    }
    // 凭证必须来自隔离 HOME (与 daemon 子进程同一份 — 真实 HOME 的凭证
    // 与隔离 daemon 不匹配会被 usrpwd 全拒)
    let path = test_isolated_home().join(".rdog/auth.toml");
    let content = std::fs::read_to_string(path).ok()?;
    let mut user = None;
    let mut password = None;
    for line in content.lines() {
        if let Some((k, v)) = line.trim().split_once('=') {
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "user" => user = Some(v.to_owned()),
                "password" => password = Some(v.to_owned()),
                _ => {}
            }
        }
    }
    Some((user?, password?))
}

/// 测试隔离 HOME (进程级一次): daemon/control 子进程的 HOME 都指向它,
/// 凭证 (~/.rdog/auth.toml) 与 TLS 材料在测试目录内自持,
/// 不再触碰真实用户目录。auth_wrong 等 env 覆盖用例优先于文件。
/// 隔离 HOME 的根目录: 优先 `/tmp`。
///
/// macOS runner 的 `$TMPDIR` 是 `/var/folders/q2/<32 位随机>/T` (约 50 字符), 在它下面
/// 建 `.rdog/run` 后, zenoh unixpipe 的 pipe 名会超出 95 字节上限, daemon 直接拒启动:
/// `本机 fast path 运行目录太长,压缩后的 pipe 名仍超出 95 字节上限 ... 请把 XDG_RUNTIME_DIR
/// 指向更短的目录` (2026-11-26 macOS runner 实测, 3 条 agent_* 用例因此假失败)。
/// `/tmp` 在 macOS 上是 `/private/tmp` 的短路径, 足够容纳 pipe 名。
fn short_temp_root() -> std::path::PathBuf {
    let short = std::path::PathBuf::from("/tmp");
    if short.is_dir() {
        short
    } else {
        std::env::temp_dir()
    }
}

fn test_isolated_home() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = short_temp_root().join(format!("rdog-e2e-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test home should create");
        dir
    })
    .clone()
}

/// 隔离 HOME 下预生成认证凭证 (幂等): 需要 usrpwd 的测试先调用。
fn ensure_test_home_credentials() {
    // 手写测试凭证 (与 auth_credentials 的自控格式一致), client/daemon 共享;
    // 不走 daemon 启动路径生成 — 太重。
    let auth_dir = test_isolated_home().join(".rdog");
    std::fs::create_dir_all(&auth_dir).expect("auth dir should create");
    let auth_file = auth_dir.join("auth.toml");
    if !auth_file.exists() {
        std::fs::write(
            &auth_file,
            "# rdog 测试凭证 (隔离 HOME 自持)\nuser = \"rdog-e2e-test\"\npassword = \"e2e-test-password\"\n",
        )
        .expect("test credentials should write");
    }
}

#[path = "support/rdog_binary.rs"]
mod rdog_binary;

use rdog_binary::rdog_binary_path;
fn write_temp_zenoh_router_config(
    daemon_name: &str,
    listen_endpoints: &[String],
    mode: &str,
) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-zenoh-router-{}-{}.toml",
        std::process::id(),
        allocate_tcp_port()
    ));

    let listen_endpoints = listen_endpoints
        .iter()
        .map(|endpoint| format!("\"{endpoint}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let contents = format!(
        r#"[observation]
durable_enabled = false

[zenoh]
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

/// 失败自证: 断言失败时把 daemon 的 stdout+stderr 一起打进 panic 文案。
/// 没有这一行, TLS/连接类失败在 CI 上只剩空 stdout, 下一次还是只能猜 (issue #163 验收 3)。
fn daemon_output(buffer: &Arc<Mutex<String>>) -> String {
    buffer
        .lock()
        .map(|collected| collected.clone())
        .unwrap_or_default()
}

/// marker 命中**不等于整行到齐**。
///
/// 收集器按 1024 字节块把 daemon 输出追加进 buffer, 而 `log::info!` 的一行可能跨块:
/// 只看 `contains(needle)` 会在半行处返回, 读这一行后半段 (`control_key=…`) 的调用方
/// 就拿到 `None` —— RAI-582 实测 `daemon_should_reuse_same_control_key_after_restart_by_default`
/// 1/20 红在这个形状上。判据因此是「marker 之后还出现了换行」。
fn marker_line_complete(collected: &str, needle: &str) -> bool {
    let Some(position) = collected.find(needle) else {
        return false;
    };
    collected[position + needle.len()..].contains('\n')
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
        if marker_line_complete(&collected, needle) {
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
        .env("HOME", test_isolated_home())
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
    ensure_test_home_credentials();
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path =
        write_temp_zenoh_router_config(name, std::slice::from_ref(&entrypoint), "router");
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
        allocate_tcp_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

fn run_control(args: &[&str], line: &str) -> (std::process::ExitStatus, String, String) {
    let mut child = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
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
        .env("HOME", test_isolated_home())
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
        .env("HOME", test_isolated_home())
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
    // 认证注入 (issue #82): daemon 默认开启 usrpwd
    if let Some((user, password)) = test_zenoh_credentials() {
        config
            .insert_json5("transport/auth/usrpwd/user", &format!("\"{user}\""))
            .expect("auth user");
        config
            .insert_json5("transport/auth/usrpwd/password", &format!("\"{password}\""))
            .expect("auth password");
    }
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
    let listen_port = allocate_tcp_port();
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
    assert!(stdout.contains(r#""pong":true"#));
}

#[test]
fn control_should_reach_daemon_via_explicit_entrypoint_fallback() {
    let daemon_name = unique_name("entry");
    let listen_port = allocate_tcp_port();
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
    let first_port = allocate_tcp_port();
    let second_port = allocate_tcp_port();
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
        .env("HOME", test_isolated_home())
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
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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

// `@key` 的真实注入走 enigo: linux 后端必须先连上 X11 / Wayland display, 否则
// `Enigo::new` 直接报 `no connection could be established` (code 70) —— 与 control
// client 到 daemon 的连接无关 (RAI-464 归因订正)。CI 的 ubuntu job 在 xvfb-run 下
// 执行 integration target, 所以这条不再是平台级阻塞; ubuntu 上该 target 仍因
// zenoh-link-tls 的首帧握手竞态整体豁免 (issue #163), 与本用例无关。
#[test]
fn control_should_publish_key_event_after_successful_key_request() {
    let daemon_name = unique_name("keyevent");
    let listen_port = allocate_tcp_port();
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
    await_subscriber_visible(&session, &entrypoint, &keyexpr);

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
    let listen_port = allocate_tcp_port();
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
        .declare_subscriber(to_control_key.clone())
        .wait()
        .expect("subscriber should declare");
    await_subscriber_visible(&session, &entrypoint, &to_control_key);

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
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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
        .declare_subscriber(to_control_key.clone())
        .wait()
        .expect("subscriber should declare");
    await_subscriber_visible(&session, &entrypoint, &to_control_key);

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
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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
            .env("HOME", test_isolated_home())
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
    let listen_port = allocate_tcp_port();
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
        .env("HOME", test_isolated_home())
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
    let listen_port = allocate_tcp_port();
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
        .declare_subscriber(to_control_key.clone())
        .wait()
        .expect("subscriber should declare");
    await_subscriber_visible(&session, &entrypoint, &to_control_key);
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
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let mut child = tty_script::scripted(
        &rdog_binary_path(),
        &[
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
    )
    .env("HOME", test_isolated_home())
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
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, daemon_buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &daemon_buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let mut child = tty_script::scripted(
        &rdog_binary_path(),
        &[
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
    )
    .env("HOME", test_isolated_home())
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

/// 等到一条**控制帧** (以 `prefix` 开头), 期间到达的 `@pty-output` 入队而不是丢弃。
///
/// PTY 输出被切在哪个 chunk 由内核与调度决定, 与控制帧之间**没有严格交替的保证** ——
/// 子进程在 close 前打出的一个换行 (`data:"DQo="`) 就足以让「下一条一定是 `@pty-closed`」
/// 的断言偶发失败 (RAI-582 实测 1–2/20)。跳过输出帧不削弱断言: 被测性质仍然是
/// 「close 得到 `@pty-closed` 且带 force_close」, 只是不再替内核规定交付顺序。
///
/// 跳过的帧**必须留在 `buffered_output` 里**: 下游 (`second_output`) 要的正是下一条
/// `@pty-output`, 丢弃等于把失败从 attached 断言挪到「拿不到 second output」上 —— 断言没变弱,
/// 但诊断变差了 (复核 observation 2)。
fn wait_for_pty_control_frame(
    prefix: &str,
    daemon_log: &dyn Fn() -> String,
    buffered_output: &mut VecDeque<String>,
    mut next_frame: impl FnMut(Duration) -> Option<String>,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut skipped_output = 0_usize;
    while Instant::now() < deadline {
        let Some(payload) = next_frame(Duration::from_millis(500)) else {
            continue;
        };
        if payload.starts_with("@pty-output ") {
            skipped_output += 1;
            buffered_output.push_back(payload);
            continue;
        }
        assert!(
            payload.starts_with(prefix),
            "unexpected pty frame (期待 {}, 期间暂存 {skipped_output} 条 @pty-output): {payload}\ndaemon:\n{}",
            prefix.trim_end(),
            daemon_log()
        );
        return payload;
    }
    panic!(
        "等待 {} 超时 (期间暂存 {skipped_output} 条 @pty-output)\ndaemon:\n{}",
        prefix.trim_end(),
        daemon_log()
    );
}

/// 取下一条 `@pty-output`: 先取等控制帧时暂存下来的, 再向订阅者要新的。
fn next_pty_output(
    buffered_output: &mut VecDeque<String>,
    daemon_log: &dyn Fn() -> String,
    mut next_frame: impl FnMut(Duration) -> Option<String>,
) -> String {
    if let Some(payload) = buffered_output.pop_front() {
        return payload;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Some(payload) = next_frame(Duration::from_millis(500)) {
            assert!(
                payload.starts_with("@pty-output "),
                "unexpected pty frame (期待 @pty-output): {payload}\ndaemon:\n{}",
                daemon_log()
            );
            return payload;
        }
    }
    panic!("等待 @pty-output 超时\ndaemon:\n{}", daemon_log());
}

/// RAI-582 复核 observation 2 的守卫: 等控制帧时跳过的 `@pty-output` 必须能被下游读到。
#[test]
fn pty_control_frame_wait_must_not_swallow_output_frames() {
    let daemon_log = || String::new();
    let mut buffered: VecDeque<String> = VecDeque::new();
    let mut frames = [
        r#"@pty-output {"encoding":"base64","data":"U0VDT05E"}"#.to_string(),
        r#"@pty-attached {"session_id":"s-1"}"#.to_string(),
    ]
    .into_iter();

    let attached = wait_for_pty_control_frame("@pty-attached ", &daemon_log, &mut buffered, |_| {
        frames.next()
    });
    assert!(attached.starts_with("@pty-attached "), "{attached}");
    assert_eq!(
        buffered.len(),
        1,
        "被跳过的输出帧必须留在缓冲里, 不能被吃掉"
    );

    let output = next_pty_output(&mut buffered, &daemon_log, |_| None);
    assert!(output.starts_with("@pty-output "), "{output}");
    assert!(output.contains("U0VDT05E"), "下游仍要能读到那一帧: {output}");
    assert!(buffered.is_empty(), "取过之后不该留着");
}

#[test]
fn control_should_detach_and_attach_pty_in_zenoh_profile() {
    let daemon_name = unique_name("pty-reattach");
    let listen_port = allocate_tcp_port();
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
        .declare_subscriber(open_to_control_key.clone())
        .wait()
        .expect("open subscriber should declare");
    await_subscriber_visible(&session, &entrypoint, &open_to_control_key);
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
        .declare_subscriber(attach_to_control_key.clone())
        .wait()
        .expect("attach subscriber should declare");
    await_subscriber_visible(&session, &entrypoint, &attach_to_control_key);
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
    let mut buffered_output: VecDeque<String> = VecDeque::new();
    let attached_payload =
        wait_for_pty_control_frame("@pty-attached ", &daemon_log, &mut buffered_output, |timeout| {
        attach_subscriber
            .recv_timeout(timeout)
            .ok()
            .flatten()
            .and_then(|sample| sample.payload().try_to_string().ok().map(|text| text.into_owned()))
    });
    assert!(
        attached_payload.contains(&session_id),
        "unexpected attached payload: {attached_payload}"
    );
    let second_payload = next_pty_output(&mut buffered_output, &daemon_log, |timeout| {
        attach_subscriber
            .recv_timeout(timeout)
            .ok()
            .flatten()
            .and_then(|sample| sample.payload().try_to_string().ok().map(|text| text.into_owned()))
    });
    assert!(
        second_payload.starts_with("@pty-output "),
        "unexpected second output payload: {second_payload}"
    );

    attach_publisher
        .put(format!(r#"@pty-close:{{session_id:"{session_id}"}}"#))
        .wait()
        .expect("should publish pty close after attach");
    let closed_payload =
        wait_for_pty_control_frame("@pty-closed ", &daemon_log, &mut buffered_output, |timeout| {
        attach_subscriber
            .recv_timeout(timeout)
            .ok()
            .flatten()
            .and_then(|sample| sample.payload().try_to_string().ok().map(|text| text.into_owned()))
    });
    assert!(
        closed_payload.contains("force_close"),
        "unexpected closed payload: {closed_payload}"
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

#[test]
#[ignore = "requires real screenshot backend and host screen capture permissions"]
fn control_should_execute_screenshot_and_save_file_in_zenoh_profile() {
    let daemon_name = unique_name("screenshot");
    let listen_port = allocate_tcp_port();
    let workdir = temp_workdir("screenshot");
    let entrypoint = format!("tcp/127.0.0.1:{listen_port}");
    let config_path =
        write_temp_zenoh_router_config(&daemon_name, std::slice::from_ref(&entrypoint), "router");
    let mut daemon = ChildGuard::new(
        Command::new(rdog_binary_path())
            .env("HOME", test_isolated_home())
            .args(["daemon", "-c", &config_path.display().to_string()])
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("zenoh daemon should start"),
    );
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

/// RAI-582 负对照: marker 落在 1024 字节块边界上时, 旧判据 (`contains(needle)`) 会
/// 在半行处返回, 而 `parse_control_key` 读的正是这一行的后半段。
///
/// 这不是构造出来的巧合: 收集器就是按 1024 字节块追加的, marker 落在哪里由 daemon
/// 已打印多少字节决定 —— 实测在整 target 并发下命中过 1/20。
#[test]
fn sentinel_wait_must_wait_for_the_whole_line() {
    let marker = "zenoh router daemon ready";
    // 首块正好 1024 字节, 且断在 `control_key=` 之前 —— 与 :2034 那次红的快照同形。
    let head = "zenoh router daemon ready: namespace=lab, ";
    let cut = "contro";
    let first_chunk = format!("{}{}{}", "r".repeat(1024 - head.len() - cut.len()), head, cut);
    assert_eq!(first_chunk.len(), 1024);
    assert!(
        first_chunk.contains(marker),
        "旧判据 (`contains(needle)`) 在这半行上就会返回"
    );
    assert!(
        !marker_line_complete(&first_chunk, marker),
        "换行还没到, 整行不算到齐"
    );
    assert!(
        parse_control_key(&first_chunk).is_none(),
        "半行里没有 `control_key=` —— 这正是 `first output should contain control_key` 的 panic"
    );

    let whole_line = format!("{first_chunk}l_key=rdog/lab/stable\n");
    assert!(marker_line_complete(&whole_line, marker), "换行到了才算整行");
    assert_eq!(
        parse_control_key(&whole_line).as_deref(),
        Some("rdog/lab/stable"),
        "整行能读出 control_key"
    );
}

#[test]
fn daemon_should_reuse_same_control_key_after_restart_by_default() {
    let daemon_name = unique_name("stable");
    let listen_port = allocate_tcp_port();
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
    let listen_port = allocate_tcp_port();
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
    wait_until_buffer_contains(&control_buffer, r#""pong":true"#, Duration::from_secs(8))
        .expect("first ping should succeed");

    stop_child(&mut first);
    let _ = fs::remove_file(&first_config);

    let restart_config =
        write_temp_zenoh_router_config(&daemon_name, std::slice::from_ref(&entrypoint), "router");
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
        r#""pong":true"#,
        2,
        Duration::from_secs(8),
    )
    .expect("second ping should also succeed");
    let pong_count = output.matches(r#""pong":true"#).count();
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
        .env("HOME", test_isolated_home())
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
        .env("HOME", test_isolated_home())
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
        &[format!("tcp/127.0.0.1:{}", allocate_tcp_port())],
        "peer",
    );

    let output = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
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

    // 每次尝试都带**真实预算**: `Command::output()` 没有超时, 客户端一旦挂住
    // (daemon 卡死/应答不到) 就会把整条用例拖到 job 级超时, 日志只剩
    // "has been running for over N seconds" —— 那是 RAI-579 记录的第二种形状。
    // 预算到点就杀掉并在 stderr 留下可分辨的标记, 让失败自证是「客户端被预算杀了」
    // 而不是「daemon 回了空结果」。
    const KILLED_MARKER: &str = "<control 超时被杀>";
    let run_once = |budget: Duration| {
        let mut child = Command::new(rdog_binary_path())
            .env("HOME", test_isolated_home())
            .args(&full_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("control should spawn");
        let attempt_deadline = Instant::now() + budget;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() >= attempt_deadline => {
                    let _ = child.kill();
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(err) => panic!("control 子进程状态读取失败: {err}"),
            }
        }
        let output = child
            .wait_with_output()
            .expect("control should run to completion");
        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() && Instant::now() >= attempt_deadline {
            stderr.push_str(KILLED_MARKER);
        }
        (output.status, stdout, stderr)
    };

    let deadline = Instant::now() + timeout;
    let mut output = run_once(timeout);
    let mut stdout = output.1.clone();
    let mut stderr = output.2.clone();
    let mut status = output.0;
    loop {
        let combined = format!("{stdout}\n{stderr}");
        let missing_target = combined.contains("未找到目标 service")
            || combined.contains("Unable to connect to any of");
        if !missing_target {
            return (status, stdout, stderr);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // 重试窗口用尽后**再给一次足够长的预算**, 让客户端把错误说完:
            // 否则最后一次尝试只剩窗口残留, 客户端会先被预算杀掉, stderr 只剩
            // `<control 超时被杀>`, 它自己那句「第几条失败」永远说不出口
            // (macOS 的 discovery 比 Linux 慢, CI 实测踩到)。
            const FINAL_ATTEMPT_BUDGET: Duration = Duration::from_secs(20);
            return run_once(FINAL_ATTEMPT_BUDGET);
        }
        thread::sleep(Duration::from_millis(150));
        output = run_once(remaining);
        stdout = output.1.clone();
        stderr = output.2.clone();
        status = output.0;
    }
}

/// RAI-579 负对照: 客户端挂住时, helper 必须按预算收手并在 stderr 点名。
///
/// 形状是确定性的: 冻住 daemon (`kill -STOP`) 之后 TCP 仍能连上、zenoh 应答永远不到 ——
/// 客户端因此会一直等。修前 helper 用 `Command::output()`, 没有超时也没有 kill,
/// 这条用例会挂到 job 级超时 (本机实测 >20s 不返回); 修后它在预算附近杀掉客户端并留下
/// `<control 超时被杀>`。
#[cfg(unix)]
#[test]
fn multi_one_shot_helper_must_not_hang_past_its_budget() {
    let daemon_name = unique_name("multi-hang");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let daemon_pid = daemon.id().to_string();
    let signal = |name: &str| {
        let _ = Command::new("kill")
            .args([name, daemon_pid.as_str()])
            .status();
    };
    signal("-STOP");

    let budget = Duration::from_secs(2);
    let started = Instant::now();
    let (status, stdout, stderr) = run_control_multi_one_shot(
        &["--transport", "zenoh", "--target-name", &daemon_name],
        &["@ping"],
        budget,
    );
    let elapsed = started.elapsed();

    // 无论断言结果如何都要把 daemon 放开, 否则 `stop_child` 收不掉它。
    signal("-CONT");
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        !status.success(),
        "冻住的 daemon 不应成功; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("<control 超时被杀>"),
        "失败文本必须点名「客户端被预算杀掉」, 否则读日志的人分不出是挂住还是空应答; stderr:\n{stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "helper 必须在预算附近收手, 实测 {elapsed:?}"
    );
}

// 说明 (RAI-579): 「多行 one-shot 失败时 CLI 打出第几条 / 共几条 / 已收到几帧」这条
// 只保留**纯单测** (src/control_invocation.rs 的 `multi_one_shot_failure_message_*`)。
// 曾经想用「目标 daemon 不存在」做端到端, 但它依赖 discovery 耗时: macOS 上 3s 窗口内
// 客户端还没开口就被预算杀掉, 只剩 `<control 超时被杀>`; 把窗口抬到 20s+20s 收尾, 就变成
// 一个 40s 的用例, 不值得。端到端读数改为人工一次性取证 (见账本/PR)。

#[test]
fn control_multi_one_shot_should_run_lines_in_order_for_zenoh_profile() {
    let daemon_name = unique_name("multi");
    let listen_port = allocate_tcp_port();
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
        .find(r#""pong":true"#)
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
    let listen_port = allocate_tcp_port();
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
    let pong = stdout.find(r#""pong":true"#).expect("pong should appear");
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
    let listen_port = allocate_tcp_port();
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

// ============================================================================
// Task/Spawn Phase 2 (spec §6.4): session channel 进度帧 e2e
//
// one-shot client 收到 spawn 的 @response 即返回, 进度帧由后续命令行的
// frames 前缀带回 (client session bridge 把非 response 帧 push 进下一条
// 命令的返回)。帧自带 task id, stdout 断言无需预知 id。
// ============================================================================

/// 从输出提取第一个 task id (`"task":"t-xxxx"` 形状)。
fn extract_task_id_from_output(stdout: &str) -> String {
    let marker = "\"task\":\"";
    let start = stdout
        .find(marker)
        .expect("output should contain task field")
        + marker.len();
    let rest = &stdout[start..];
    let end = rest.find('"').expect("task id should be quoted");
    rest[..end].to_owned()
}

#[test]
fn task_spawn_progress_frames_should_reach_control_client() {
    let daemon_name = unique_name("task-frames");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    // 行1 spawn 毫秒级任务; 行2/行3 的往返窗口带回 started + completed 帧,
    // 同时验证帧推送不打断同 session 后续命令 (@ping 正常 pong)
    let (status, stdout, stderr) = run_control_multi_one_shot(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ],
        // 帧到达的时序契约: started 在 spawn 后首个 25ms drain 周期发出,
        // completed 在 waiter 50ms 收割后入 pending。client 三行在本机 zenoh
        // 下 ~50ms 就跑完并发 session close, 会早于 echo 任务的收割点 —
        // 所以中间插一行 daemon 侧 @wait 400ms: bridge 处理完 wait 返回后、
        // 行3 请求前必然经过 drain 轮, completed 帧此时随行3 的 frames 回带。
        &[
            "@spawn:echo TASK_E2E_DONE",
            "@ping#401",
            "@wait#402:{duration_ms:400}",
            "@ping#403",
        ],
        Duration::from_secs(20),
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "multi one-shot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // client 侧断言: started 帧到达 (覆盖 client 白名单/exchange/打印全链路,
    // completed 走同一链路)。one-shot 多行行间隙毫秒级, client 可能先于终态帧
    // 退出, completed 的到达以 daemon 侧日志为准 (下方 buffer 断言)。
    assert!(
        stdout.contains("@task-started {\"task\":\"t-"),
        "started frame should reach client stdout\nstdout:\n{stdout}"
    );
    // 帧推送没有阻塞同 session 的后续命令
    assert!(
        stdout.contains("pong"),
        "@ping should still answer while frames stream\nstdout:\n{stdout}"
    );

    // daemon 侧断言: started 与 completed 两帧都被推到 session channel。
    // buffer 是 daemon stdout/stderr 合流 (String), bridge drain 有 INFO 日志。
    let daemon_log = buffer
        .lock()
        .expect("collector buffer lock should work")
        .clone();
    for needle in ["frame=@task-started", "frame=@task-completed"] {
        assert!(
            daemon_log.contains(needle),
            "daemon log should contain {needle}\ndaemon_log:\n{daemon_log}"
        );
    }
}

#[test]
fn task_spawn_cancel_should_work_across_client_processes() {
    let daemon_name = unique_name("task-cancel");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let args: &[&str] = &[
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entrypoint,
    ];

    // 进程1: spawn 长任务并提取 task id (registry 是 daemon 进程级, 跨 client 存活)
    let (status, stdout, stderr) =
        run_control_multi_one_shot(args, &["@spawn:sleep 30"], Duration::from_secs(20));
    assert!(
        status.success() && stdout.contains("\"state\":\"running\""),
        "spawn should answer running\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let task_id = extract_task_id_from_output(&stdout);
    assert!(task_id.starts_with("t-"));

    // 进程2: 另一个 session 发起 cancel — registry 唯一真相源, 跨 session 生效。
    // (canceled 帧推给进程1 的 lane, 进程1 已退出故帧滞留; 帧本身在单测
    //  canceled_task_terminal_frame_should_reuse_task_failed_with_flag 验收)
    let cancel_line = format!("@task-cancel:{task_id}");
    let (status, stdout, stderr) =
        run_control_multi_one_shot(args, &[&cancel_line], Duration::from_secs(20));

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    assert!(
        status.success(),
        "cancel one-shot should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("\"state\":\"canceled\""),
        "cancel response should report canceled\nstdout:\n{stdout}"
    );
}

// ============================================================================
// agent messaging Phase 3 (issue #73): mailbox 投递 -> 补拉 -> ack 全链
//
// 投递方: 测试进程自己的 zenoh session (dev-dep, peer 连 daemon entry-point)。
// 这模拟了真实的跨主机委派: 委派方 pub 一条 rdog.agentmsg.v1 envelope 到
// 目标 agent 的 inbox keyexpr。
// ============================================================================

#[test]
fn agent_mailbox_should_accept_delivery_and_support_pull_and_ack() {
    let daemon_name = unique_name("agent-mb");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("helper-{}.lab", std::process::id());
    let args: &[&str] = &[
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entrypoint,
        "--namespace",
        "lab",
    ];

    // 1. 注册 agent (mailbox 开始缓存它)
    let (status, stdout, stderr) = run_control_multi_one_shot(
        args,
        &[&format!("@agent-register:{agent_name}")],
        Duration::from_secs(20),
    );
    assert!(
        status.success() && stdout.contains("\"registered\":true"),
        "register should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // 2. 测试进程投递一条 task 消息到 inbox keyexpr。
    // session 显式 connect daemon entry-point: 只有连上 daemon 的 router,
    // pub 才能路由到 daemon 的通配 inbox sub (不依赖 multicast scout)。
    let inbox_key = format!("rdog/lab/agent/{agent_name}/inbox");
    let sent_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let message = format!(
        "{{\"v\":1,\"id\":\"e2e-mailbox-1\",\"from\":\"orchestrator-x.lab\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"打开计算器并算 1+1\",\"sent_at\":{sent_at_ms}}}"
    );
    {
        let zenoh_session = open_authenticated_test_session(&entrypoint);
        let publisher = zenoh_session
            .declare_publisher(inbox_key.clone())
            .wait()
            .expect("declare inbox publisher");
        publisher
            .put(message.as_str())
            .wait()
            .expect("deliver message");
        // publisher 声明保持到 put 完成; session drop 前让路由 flush
        thread::sleep(Duration::from_millis(100));
    }

    // 3. 补拉: @agent-inbox 应看到这条消息 (异步投递, 带重试窗口)
    let mut saw_pending = false;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(150));
        let (status, stdout, stderr) = run_control_multi_one_shot(
            args,
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        assert!(status.success(), "inbox query should succeed\n{stderr}");
        if stdout.contains("\"kind\":\"task\"") && stdout.contains("orchestrator-x.lab") {
            saw_pending = true;
            // 4. ack: 从 stdout 提取消息 id
            let id_marker = "\\\"id\\\":\\\"";
            if let Some(start) = stdout.find(id_marker) {
                let rest = &stdout[start + id_marker.len()..];
                if let Some(end) = rest.find('"') {
                    let message_id = &rest[..end];
                    let (status, stdout, stderr) = run_control_multi_one_shot(
                        args,
                        &[&format!("@agent-ack:{agent_name}:{message_id}")],
                        Duration::from_secs(10),
                    );
                    assert!(
                        status.success() && stdout.contains("\"acked\":true"),
                        "ack should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
                    );
                    // 5. ack 后 pending 为空
                    let (status, stdout, _) = run_control_multi_one_shot(
                        args,
                        &[&format!("@agent-inbox:{agent_name}")],
                        Duration::from_secs(10),
                    );
                    assert!(status.success());
                    assert!(
                        !stdout.contains("\"kind\":\"task\""),
                        "ack 后 pending 应为空\nstdout:\n{stdout}"
                    );
                    break;
                }
            }
            break;
        }
    }
    assert!(saw_pending, "投递的消息应在 mailbox 补拉中可见");

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

// ============================================================================
// agent runtime e2e (issue #74): rdog agent 子进程起停 + task 消息往返
// ============================================================================

#[test]
fn agent_runtime_should_answer_task_message_via_echo_decision() {
    let daemon_name = unique_name("agent-rt");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("rt-helper-{}.lab", std::process::id());
    let mut agent = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
        .args([
            "agent",
            "--name",
            &agent_name,
            "--namespace",
            "lab",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rdog agent should start");
    let agent_out = agent.stdout.take().expect("agent stdout");
    let agent_err = agent.stderr.take().expect("agent stderr");
    let agent_log = Arc::new(Mutex::new(String::new()));
    let _ = spawn_output_collector_to(agent_out, Arc::clone(&agent_log));
    let _ = spawn_output_collector_to(agent_err, Arc::clone(&agent_log));

    let args: &[&str] = &[
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entrypoint,
        "--namespace",
        "lab",
    ];

    // 就绪等待: agent 完成 mailbox 注册
    let mut registered = false;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(250));
        let (status, stdout, _) = run_control_multi_one_shot(
            args,
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success() && stdout.contains("\"registered\":true") {
            registered = true;
            break;
        }
    }
    assert!(registered, "agent 应完成 mailbox 注册");

    // 投递 task 并等 echo 回复。结构契约: reply_sub 与 task publisher 在
    // 同一个 session, session 必须活到等待结束 (drop 后 sub 失效)。
    // 声明传播窗口: sub/publisher 声明后 sleep 200ms 再 pub, 等待期间
    // 持续 recv (流量本身也促进路由收敛)。
    let sent_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let sender = format!("orch-{}.lab", std::process::id());
    let task_message = format!(
        "{{\"v\":1,\"id\":\"rt-e2e-1\",\"from\":\"{sender}\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"HELLO_AGENT\",\"sent_at\":{sent_at_ms}}}"
    );
    let mut saw_reply = false;
    {
        // 单一 session: reply_sub 先声明并留 1s 传播窗口 (zenoh 声明传播是
        // 异步的, sub 未传播时 agent 的 reply pub 无匹配被丢), 再投 task。
        // session 必须活到等待结束 (drop 后 sub 失效)。
        let zenoh_session = open_authenticated_test_session(&entrypoint);
        let reply_key = format!("rdog/lab/agent/{sender}/inbox");
        let reply_sub = zenoh_session
            .declare_subscriber(reply_key.clone())
            .wait()
            .expect("reply sub");
        let publisher = zenoh_session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("task publisher");
        await_subscriber_visible(&zenoh_session, &entrypoint, &reply_key);
        publisher
            .put(task_message.as_str())
            .wait()
            .expect("task delivery");

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(sample)) = reply_sub.recv_timeout(Duration::from_millis(300)) {
                if let Ok(payload) = sample.payload().try_to_string() {
                    if payload.contains("\"kind\":\"reply\"")
                        && payload.contains("echo:HELLO_AGENT")
                    {
                        saw_reply = true;
                        break;
                    }
                }
            }
        }
    }

    // 诊断: daemon mailbox 是否见过 task (区分 pub 未达 vs agent 未收)
    let mut mailbox_saw_task = false;
    for _ in 0..10 {
        let (status, stdout, _) = run_control_multi_one_shot(
            args,
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success() && stdout.contains("HELLO_AGENT") {
            mailbox_saw_task = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }

    assert!(
        saw_reply,
        "agent 应以 echo 回复 task; mailbox_saw_task={mailbox_saw_task}; agent_log:\n{}",
        agent_log.lock().unwrap()
    );

    agent.kill().expect("agent should stop");
    let _ = agent.wait();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

#[test]
fn agent_card_should_be_published_and_queryable_with_version_updates() {
    let daemon_name = unique_name("agent-card");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("card-helper-{}.lab", std::process::id());
    let mut agent = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
        .args([
            "agent",
            "--name",
            &agent_name,
            "--namespace",
            "lab",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("rdog agent should start");

    let args: &[&str] = &[
        "--transport",
        "zenoh",
        "--target-name",
        &daemon_name,
        "--entry-point",
        &entrypoint,
        "--namespace",
        "lab",
    ];

    // 就绪等待: 卡片可查且是 echo 决策的 v1
    let mut saw_card = false;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(250));
        let (status, stdout, _) = run_control_multi_one_shot(
            args,
            &[&format!("@agent-card:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success()
            && stdout.contains("\"version\":1")
            && stdout.contains("\"decision\":\"echo\"")
        {
            saw_card = true;
            break;
        }
    }
    assert!(saw_card, "agent 启动后 @agent-card 应命中 echo v1 卡片");

    // 外部重 pub v2 卡片 (模拟 agent 侧能力更新): daemon 缓存应反映最新
    {
        let zenoh_session = open_authenticated_test_session(&entrypoint);
        let publisher = zenoh_session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/card"))
            .wait()
            .expect("card publisher");
        publisher
            .put(format!(
                "{{\"agent\":\"{agent_name}\",\"version\":2,\"card\":{{\"decision\":\"echo\",\"capabilities\":[\"task-reply\",\"beta\"]}}}}"
            ))
            .wait()
            .expect("card v2 publish");
        thread::sleep(Duration::from_millis(300));
    }
    let mut saw_v2 = false;
    for _ in 0..20 {
        let (status, stdout, _) = run_control_multi_one_shot(
            args,
            &[&format!("@agent-card:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success() && stdout.contains("\"version\":2") {
            saw_v2 = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(saw_v2, "卡片更新后查询应反映 v2");

    agent.kill().expect("agent should stop");
    let _ = agent.wait();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

// ============================================================================
// agent messaging Phase 3 收口 e2e (issue #76):
// 1) 先投递后起 agent (mailbox 补拉恢复, 防丢语义)
// 2) 重复投递 id 去重
// ============================================================================

#[test]
fn agent_message_delivered_before_agent_start_should_be_recovered_via_mailbox() {
    let daemon_name = unique_name("agent-pre");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("pre-helper-{}.lab", std::process::id());
    let sender = format!("pre-orch-{}.lab", std::process::id());
    let sent_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let task_message = format!(
        "{{\"v\":1,\"id\":\"pre-e2e-1\",\"from\":\"{sender}\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"PRE_START_TASK\",\"sent_at\":{sent_at_ms}}}"
    );

    // 1. agent 未起时投递 (mailbox 通配缓存, 防丢语义)
    {
        let zenoh_session = open_authenticated_test_session(&entrypoint);
        let publisher = zenoh_session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("pre-start publisher");
        publisher
            .put(task_message.as_str())
            .wait()
            .expect("pre-start delivery");
        thread::sleep(Duration::from_millis(300));
    }

    // 1.5 投递后 mailbox 应立即可见 (缓存发生在 agent 起之前 — 防丢语义断言)
    {
        let mut cached = false;
        for _ in 0..20 {
            let (status, stdout, _) = run_control_multi_one_shot(
                &[
                    "--transport",
                    "zenoh",
                    "--target-name",
                    &daemon_name,
                    "--entry-point",
                    &entrypoint,
                    "--namespace",
                    "lab",
                ],
                &[&format!("@agent-inbox:{agent_name}")],
                Duration::from_secs(10),
            );
            if status.success() && stdout.contains("PRE_START_TASK") {
                cached = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        assert!(
            cached,
            "投递后 mailbox 应缓存 pre-start 消息 (pub 无缓存, 丢了补拉不回来); daemon_log:\n{}",
            daemon_output(&buffer)
        );
    }

    // 2. 先建 reply 订阅 (agent 首次补拉在启动后立即触发, 回复可能早于
    //    测试侧订阅声明 — pub 无订阅者即丢, 必须先 sub 后起 agent)
    let reply_session = open_authenticated_test_session(&entrypoint);
    let reply_key = format!("rdog/lab/agent/{sender}/inbox");
    let reply_sub = reply_session
        .declare_subscriber(reply_key.clone())
        .wait()
        .expect("reply sub");
    await_subscriber_visible(&reply_session, &entrypoint, &reply_key);

    // 3. 起 agent: 补拉路径应拿到 pre-start 消息并回复
    let mut agent = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
        .args([
            "agent",
            "--name",
            &agent_name,
            "--namespace",
            "lab",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rdog agent should start");
    let agent_stderr = agent.stderr.take().expect("agent stderr");
    let agent_log = Arc::new(Mutex::new(String::new()));
    let _ = spawn_output_collector_to(agent_stderr, Arc::clone(&agent_log));

    let mut saw_reply = false;
    {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(sample)) = reply_sub.recv_timeout(Duration::from_millis(300)) {
                if let Ok(payload) = sample.payload().try_to_string() {
                    if payload.contains("\"kind\":\"reply\"")
                        && payload.contains("echo:PRE_START_TASK")
                    {
                        saw_reply = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(
        saw_reply,
        "agent 起后应经 mailbox 补拉恢复 pre-start 消息并回复; agent_log:\n{}",
        agent_log.lock().unwrap()
    );

    agent.kill().expect("agent should stop");
    let _ = agent.wait();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

/// RAI-576: ready sentinel 必须晚于 mailbox 通配 sub 的声明。
///
/// 这是这组改动**确定性**的负对照: 把 ready 那行挪回声明之前, 本用例必红; 而出事的
/// 那两条 mailbox 用例只会「有时」红, 拿它们当回归证据等于靠运气 (本机 0/50 复现)。
///
/// 断言的是日志顺序, 不是实现细节: `ready` 是 e2e 与运维共用的「可以投递」契约,
/// 它的定义就是「订阅已声明」—— 契约写在
/// `docs/solutions/conventions/daemon-log-sentinel-e2e-contract.md`。
#[test]
fn daemon_ready_should_be_logged_after_mailbox_subscriptions() {
    let daemon_name = unique_name("ready-order");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, _entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let log = daemon_output(&buffer);
    let position = |needle: &str| {
        log.find(needle)
            .unwrap_or_else(|| panic!("daemon 日志缺少 `{needle}`; daemon_log:\n{log}"))
    };
    let card = position("agent card 通配 sub 已声明");
    let inbox = position("agent inbox 通配 sub 已声明");
    let ready = position("zenoh router daemon ready");
    assert!(
        card < ready && inbox < ready,
        "ready 必须晚于两条通配 sub 声明 (card={card}, inbox={inbox}, ready={ready}); daemon_log:\n{log}"
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

/// RAI-576: 「ready」必须意味着「现在投递不会丢」。
///
/// 这条用例只等 ready 日志, 之后既不 sleep 也不重试投递 —— 与出事的那两条同形
/// (`flow`/`dedup` 都是声明完立刻 put)。pub 无缓存, 所以丢了就是丢了, 补拉只能
/// 看到丢了的结果。
#[test]
fn mailbox_should_accept_a_publish_immediately_after_ready() {
    let daemon_name = unique_name("agent-ready");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("ready-{}.lab", std::process::id());
    let message = format!(
        "{{\"v\":1,\"id\":\"ready-e2e-1\",\"from\":\"ready-orch.lab\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"READY_TASK\",\"sent_at\":{}}}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    {
        let session = open_authenticated_test_session(&entrypoint);
        let publisher = session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("publisher");
        publisher
            .put(message.as_str())
            .wait()
            .expect("delivery right after ready");
    }

    let mut cached = false;
    for _ in 0..20 {
        let (status, stdout, _) = run_control_multi_one_shot(
            &[
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--namespace",
                "lab",
            ],
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success() && stdout.contains("READY_TASK") {
            cached = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(
        cached,
        "ready 之后立刻投递的消息应进 mailbox (pub 无缓存, 丢了补拉不回来); daemon_log:\n{}",
        daemon_output(&buffer)
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

#[test]
fn agent_mailbox_should_deduplicate_repeated_delivery_by_id() {
    let daemon_name = unique_name("agent-dedup");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("dd-helper-{}.lab", std::process::id());
    let sent_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let message = format!(
        "{{\"v\":1,\"id\":\"dd-e2e-fixed-id\",\"from\":\"dd-orch.lab\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"ONCE\",\"sent_at\":{sent_at_ms}}}"
    );

    // 同 id 投递两次
    {
        let zenoh_session = open_authenticated_test_session(&entrypoint);
        let publisher = zenoh_session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("publisher");
        publisher
            .put(message.as_str())
            .wait()
            .expect("first delivery");
        thread::sleep(Duration::from_millis(150));
        publisher
            .put(message.as_str())
            .wait()
            .expect("duplicate delivery");
        thread::sleep(Duration::from_millis(300));
    }

    let mut saw_single = false;
    let mut last_occurrences = 0;
    let mut last_duplicate = false;
    for _ in 0..20 {
        let (status, stdout, _) = run_control_multi_one_shot(
            &[
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--namespace",
                "lab",
            ],
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        if status.success() && stdout.contains("dd-e2e-fixed-id") {
            let occurrences = stdout.matches("dd-e2e-fixed-id").count();
            let duplicate_flag = stdout.contains("\"duplicate\":1");
            last_occurrences = occurrences;
            last_duplicate = duplicate_flag;
            saw_single = occurrences == 1 && duplicate_flag;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(
        saw_single,
        "重复投递应只留 1 条且 duplicate 计数为 1 (最后读数: id 出现 {last_occurrences} 次, \
         duplicate 标记={last_duplicate}); daemon_log:\n{}",
        daemon_output(&buffer)
    );

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}

// ============================================================================
// auth e2e (issue #82): 错误凭证被拒 — daemon 开启 usrpwd 后,
// 带错密码的 client 连不上 (反向验收)
// ============================================================================

#[test]
fn auth_wrong_credentials_should_be_rejected_by_daemon() {
    let daemon_name = unique_name("auth-reject");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "auth enabled (usrpwd)",
        Duration::from_secs(8),
    )
    .expect("daemon should report auth enabled");

    // 错误凭证 client: 显式 env 覆盖成假值, 连接应失败
    let child = Command::new(rdog_binary_path())
        .env("RDOG_AUTH_USER", "rdog-intruder")
        .env("RDOG_AUTH_PASSWORD", "wrong-password")
        .args([
            "control",
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
            "--namespace",
            "lab",
            "@ping",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("control should start");
    let output = child.wait_with_output().expect("control should finish");

    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("pong"),
        "错误凭证不应收到 pong\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

// ============================================================================
// tls e2e (issue #88/#89, spec: specs/rdog-tls-plan.md):
// [tls] enabled daemon (tcp->tls + 材料注入) + CA client 连通 + 错 CA 拒绝
// ============================================================================

fn write_temp_zenoh_tls_config(daemon_name: &str, listen_port: u16) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-zenoh-tls-{}-{}.toml",
        std::process::id(),
        listen_port
    ));
    std::fs::write(
        &path,
        format!(
            r#"[observation]
durable_enabled = false

[zenoh]
enabled = true
mode = "router"
namespace = "lab"
daemon_name = "{daemon_name}"
listen_endpoints = ["tcp/127.0.0.1:{listen_port}"]

[tls]
enabled = true
"#
        ),
    )
    .expect("tls config should write");
    path
}

#[test]
fn tls_enabled_daemon_should_serve_ca_client_and_reject_wrong_ca() {
    ensure_test_home_credentials();
    // 前置: 隔离 HOME 内的 TLS 材料 (幂等; 测试目录自持)
    let user_dir = dirs_home_rdkg();
    assert!(
        std::process::Command::new(rdog_binary_path())
            .env("HOME", test_isolated_home())
            .args(["auth", "tls-init"])
            .status()
            .expect("tls-init should run")
            .success(),
        "rdog auth tls-init 应成功 (隔离 HOME)"
    );
    assert!(
        test_isolated_home().join(".rdog/tls/ca.pem").exists(),
        "TLS 材料应在隔离 HOME 内"
    );

    let daemon_name = unique_name("tls-e2e");
    let listen_port = allocate_tcp_port();
    let config_path = write_temp_zenoh_tls_config(&daemon_name, listen_port);
    let entrypoint = format!("tls/127.0.0.1:{listen_port}");
    let _ = user_dir;

    let mut daemon = ChildGuard::new(
        Command::new(rdog_binary_path())
            .env("HOME", test_isolated_home())
            .args(["daemon", "-c", config_path.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("tls daemon should start"),
    );
    let daemon_out = daemon.stdout.take().expect("daemon stdout");
    let daemon_err = daemon.stderr.take().expect("daemon stderr");
    let buffer = Arc::new(Mutex::new(String::new()));
    let _ = spawn_output_collector_to(daemon_out, Arc::clone(&buffer));
    let _ = spawn_output_collector_to(daemon_err, Arc::clone(&buffer));
    wait_until_output_contains(&mut daemon, &buffer, "tls enabled", Duration::from_secs(8))
        .expect("daemon should report tls enabled");
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // 场景 1: 正确材料 client (usrpwd + TLS 双层) 连通
    let (status, stdout, stderr) = run_control_multi_one_shot(
        &[
            "--transport",
            "zenoh",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
            "--namespace",
            "lab",
        ],
        &["@ping#901"],
        Duration::from_secs(20),
    );
    assert!(
        status.success(),
        "CA client 应经 tls 连通\nstdout:\n{stdout}\nstderr:\n{stderr}\ndaemon:\n{}",
        daemon_output(&buffer)
    );
    let response = stdout
        .lines()
        .find_map(|line| line.strip_prefix("@response "))
        .expect("TLS ping should return a protocol response");
    let response: serde_json::Value =
        serde_json::from_str(response).expect("TLS ping response should be JSON");
    assert_eq!(response["id"], 901, "TLS ping should retain the request id");
    assert_eq!(
        response["value"]["pong"], true,
        "TLS ping should report liveness"
    );

    // 场景 2: 错 CA (HOME 指向独立材料目录) — TLS 握手层拒绝 (BadSignature)
    let fake_home = std::env::temp_dir().join(format!("rdog-tls-fakehome-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fake_home);
    std::fs::create_dir_all(fake_home.join(".rdog")).unwrap();
    assert!(
        std::process::Command::new(rdog_binary_path())
            .env("HOME", &fake_home)
            .args(["auth", "tls-init"])
            .status()
            .expect("fake tls-init should run")
            .success(),
        "fake HOME 的独立 CA 应生成"
    );
    fs::copy(
        test_isolated_home().join(".rdog/auth.toml"),
        fake_home.join(".rdog/auth.toml"),
    )
    .expect("both clients should use the same test credentials");
    let run_fake_client = || {
        let child = Command::new(rdog_binary_path())
            .env("HOME", &fake_home)
            .args([
                "control",
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--namespace",
                "lab",
                "@ping#902",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("fake-home control should start");
        wait_with_output_timeout(child, Duration::from_secs(30), "fake-home TLS client")
    };
    let wrong = run_fake_client();
    let wrong_stdout = String::from_utf8_lossy(&wrong.stdout);
    let wrong_stderr = String::from_utf8_lossy(&wrong.stderr);
    assert!(
        !wrong.status.success() && !wrong_stdout.contains(r#""pong":true"#),
        "错 CA 不应连通\nstdout:\n{wrong_stdout}\nstderr:\n{wrong_stderr}\ndaemon:\n{}",
        daemon_output(&buffer)
    );

    fs::copy(
        test_isolated_home().join(".rdog/tls/ca.pem"),
        fake_home.join(".rdog/tls/ca.pem"),
    )
    .expect("correct CA should replace the wrong CA");
    let corrected = run_fake_client();
    let corrected_stdout = String::from_utf8_lossy(&corrected.stdout);
    let corrected_stderr = String::from_utf8_lossy(&corrected.stderr);
    assert!(
        corrected.status.success() && corrected_stdout.contains(r#""pong":true"#),
        "相同客户端换成正确 CA 后应连通\nstdout:\n{corrected_stdout}\nstderr:\n{corrected_stderr}\n\
         daemon:\n{}",
        daemon_output(&buffer)
    );

    stop_child(&mut daemon);
    let _ = std::fs::remove_file(&config_path);
    let _ = std::fs::remove_dir_all(&fake_home);
}

#[test]
fn mtls_daemon_should_require_client_certificate_and_accept_configured_client() {
    ensure_test_home_credentials();
    let home = test_isolated_home();
    assert!(
        Command::new(rdog_binary_path())
            .env("HOME", &home)
            .args(["auth", "tls-init"])
            .status()
            .expect("tls-init should run")
            .success(),
        "隔离 HOME 的证书应生成"
    );

    let workdir = PathBuf::from(format!(
        "/tmp/rdog-mtls-{}-{}",
        std::process::id(),
        allocate_tcp_port()
    ));
    fs::create_dir_all(&workdir).expect("mTLS workdir should create");
    let daemon_tmp = workdir.join("daemon-tmp");
    let client_tmp = workdir.join("client-tmp");
    let ca_only_home = workdir.join("ca-only");
    fs::create_dir_all(&daemon_tmp).expect("daemon tmp should create");
    fs::create_dir_all(&client_tmp).expect("client tmp should create");
    fs::create_dir_all(ca_only_home.join(".rdog/tls")).expect("CA-only home should create");
    fs::copy(
        home.join(".rdog/auth.toml"),
        ca_only_home.join(".rdog/auth.toml"),
    )
    .expect("CA-only client should use the same credentials");
    fs::copy(
        home.join(".rdog/tls/ca.pem"),
        ca_only_home.join(".rdog/tls/ca.pem"),
    )
    .expect("CA-only client should trust the server");

    let daemon_name = unique_name("mtls-e2e");
    let port = allocate_tcp_port();
    let config_path = write_temp_zenoh_tls_config(&daemon_name, port);
    fs::OpenOptions::new()
        .append(true)
        .open(&config_path)
        .expect("TLS config should open")
        .write_all(b"enable_mtls = true\n")
        .expect("mTLS setting should write");
    let entrypoint = format!("tls/127.0.0.1:{port}");
    let mut daemon = ChildGuard::new(
        Command::new(rdog_binary_path())
            .env("HOME", &home)
            .env("TMPDIR", &daemon_tmp)
            .args(["daemon", "-c", config_path.to_str().expect("config path")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("mTLS daemon should start"),
    );
    let buffer = Arc::new(Mutex::new(String::new()));
    let _ = spawn_output_collector_to(
        daemon.stdout.take().expect("daemon stdout"),
        Arc::clone(&buffer),
    );
    let _ = spawn_output_collector_to(
        daemon.stderr.take().expect("daemon stderr"),
        Arc::clone(&buffer),
    );
    let ready = wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    );
    let run_client = |client_home: &std::path::Path| {
        let child = Command::new(rdog_binary_path())
            .env("HOME", client_home)
            .env("TMPDIR", &client_tmp)
            .args([
                "control",
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--namespace",
                "lab",
                "@ping",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("mTLS client should start");
        wait_with_output_timeout_or_kill(child, Duration::from_secs(30))
    };
    let (accepted, rejected) = if ready.is_ok() {
        (Some(run_client(&home)), Some(run_client(&ca_only_home)))
    } else {
        (None, None)
    };
    stop_child(&mut daemon);
    fs::remove_file(&config_path).expect("mTLS config should be removed");
    fs::remove_dir_all(&workdir).expect("mTLS workdir should be removed");

    ready.expect("mTLS daemon should become ready");
    let accepted = accepted
        .expect("client should run")
        .expect("client should finish");
    assert!(
        accepted.status.success()
            && String::from_utf8_lossy(&accepted.stdout).contains(r#""pong":true"#),
        "持有客户端证书时应连接成功: {}\ndaemon:\n{}",
        String::from_utf8_lossy(&accepted.stderr),
        daemon_output(&buffer)
    );
    let rejected = rejected
        .expect("CA-only client should run")
        .expect("client should finish");
    assert!(
        !rejected.status.success()
            && !String::from_utf8_lossy(&rejected.stdout).contains(r#""pong":true"#),
        "仅持有 CA 证书时不应连接到 mTLS 服务\ndaemon:\n{}",
        daemon_output(&buffer)
    );
}

/// 测试辅助: home 目录 (仅用于 tls-init 前置的存在性说明)。
fn dirs_home_rdkg() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

// ============================================================================
// 诊断对拍 (issue: 测试 session pub 在 agent 存在时不到达 daemon):
// 同款投递, 唯一变量 = agent 进程是否在跑
// ============================================================================

#[test]
fn diag_pub_reaches_mailbox_with_and_without_agent() {
    let daemon_name = unique_name("diag-pub");
    let listen_port = allocate_tcp_port();
    let (mut daemon, config_path, entrypoint, buffer) =
        start_zenoh_daemon_with_combined_output(&daemon_name, listen_port);
    wait_until_output_contains(
        &mut daemon,
        &buffer,
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should report ready");

    let agent_name = format!("diag-h-{}.lab", std::process::id());
    let sent_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let message_for = |tag: &str| {
        format!(
            "{{\"v\":1,\"id\":\"diag-{tag}\",\"from\":\"orch.lab\",\"to\":\"{agent_name}\",\"kind\":\"task\",\"payload\":\"DIAG_{tag}\",\"sent_at\":{sent_at_ms}}}"
        )
    };
    let query_inbox = |needle: &str| -> bool {
        for _ in 0..15 {
            thread::sleep(Duration::from_millis(200));
            let (status, stdout, _) = run_control_multi_one_shot(
                &[
                    "--transport",
                    "zenoh",
                    "--target-name",
                    &daemon_name,
                    "--entry-point",
                    &entrypoint,
                    "--namespace",
                    "lab",
                ],
                &[&format!("@agent-inbox:{agent_name}")],
                Duration::from_secs(10),
            );
            if status.success() && stdout.contains(needle) {
                return true;
            }
        }
        false
    };

    // 连接健康探测: authenticated session 直接 query daemon control key。
    // zenoh::open 即使 connect 失败也返回 session (异步重连), pub 会静默
    // 排队 — 必须用 query 证明链路活着, 否则后续断言全是伪命题。
    {
        let session = open_authenticated_test_session(&entrypoint);
        let replies = session
            .get(format!(
                "rdog/lab/daemon/{daemon_name}/member/{daemon_name}/control"
            ))
            .payload("@ping")
            .timeout(Duration::from_secs(5))
            .wait()
            .expect("probe query should dispatch");
        let mut probe_ok = false;
        if let Ok(reply) = replies.recv() {
            if let Ok(sample) = reply.result() {
                if let Ok(payload) = sample.payload().try_to_string() {
                    probe_ok = payload.contains("pong");
                }
            }
        }
        assert!(probe_ok, "探测: authenticated session 的 query 应通");
    }

    // 阶段 1: 无 agent, 纯 pub session 投递 -> mailbox 应缓存
    {
        let session = open_authenticated_test_session(&entrypoint);
        let publisher = session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("publisher");
        publisher
            .put(message_for("noagent").as_str())
            .wait()
            .expect("pub noagent");
        thread::sleep(Duration::from_millis(200));
    }
    assert!(
        query_inbox("DIAG_noagent"),
        "阶段1: 无 agent 时 pub 应达 mailbox; daemon_log:\n{}",
        buffer.lock().unwrap()
    );

    // 阶段 2: 起 agent (消费会 ack; 观察点改为 agent 处理后的 pending 空 +
    // 新投递能否再达 — 若 agent 存在阻断 pub, 新消息不会出现也不会被处理)
    let mut agent = Command::new(rdog_binary_path())
        .env("HOME", test_isolated_home())
        .args([
            "agent",
            "--name",
            &agent_name,
            "--namespace",
            "lab",
            "--target-name",
            &daemon_name,
            "--entry-point",
            &entrypoint,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("agent should start");
    // 等 agent 注册
    let mut registered = false;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(250));
        let (status, _stdout, _) = run_control_multi_one_shot(
            &[
                "--transport",
                "zenoh",
                "--target-name",
                &daemon_name,
                "--entry-point",
                &entrypoint,
                "--namespace",
                "lab",
            ],
            &[&format!("@agent-inbox:{agent_name}")],
            Duration::from_secs(10),
        );
        // 注册后 noagent 消息会被 agent 消费, registered 状态依旧可查
        if status.success() {
            registered = true;
            break;
        }
    }
    assert!(registered, "agent 应注册 (control 查询仍通)");

    // 阶段 3: agent 在跑, 同款纯 pub 投递
    {
        let session = open_authenticated_test_session(&entrypoint);
        let publisher = session
            .declare_publisher(format!("rdog/lab/agent/{agent_name}/inbox"))
            .wait()
            .expect("publisher");
        thread::sleep(Duration::from_millis(200));
        publisher
            .put(message_for("withagent").as_str())
            .wait()
            .expect("pub withagent");
    }
    // agent 若收到会处理+ack (pending 空); 阻断则 pending 也空。
    // 用 daemon 侧证据区分: 等 2s 让 agent 消费, 然后查 reply 是否到了
    // orchestrator inbox (经 sub 收不到 — 用 daemon 日志? card 缓存?
    // 最简: orch.lab 的 reply 需要有人 sub; 这里用 mailbox of orch?
    // daemon 通配 sub 只入 mailbox… orch 也会被缓存! 查 orch mailbox!)
    thread::sleep(Duration::from_secs(2));
    let reply_cached = {
        let mut hit = false;
        for _ in 0..15 {
            let (status, stdout, _) = run_control_multi_one_shot(
                &[
                    "--transport",
                    "zenoh",
                    "--target-name",
                    &daemon_name,
                    "--entry-point",
                    &entrypoint,
                    "--namespace",
                    "lab",
                ],
                &["@agent-inbox:orch.lab"],
                Duration::from_secs(10),
            );
            if status.success() && stdout.contains("echo:DIAG_withagent") {
                hit = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        hit
    };
    assert!(
        reply_cached,
        "agent 在跑时投递应被处理, reply 应达 orch mailbox (daemon 侧证据)"
    );

    agent.kill().expect("agent should stop");
    let _ = agent.wait();
    stop_child(&mut daemon);
    let _ = fs::remove_file(&config_path);
}
