#![cfg(unix)]

//! E2E smoke for the recording auto-stop pipeline, runnable via
//! `cargo test --test recording_e2e`. The daemon is spawned in TCP
//! inbound `control` mode, the test sends `@record-start` with a
//! short `--duration`, sleeps past the deadline, then asks for
//! `@record-status` and asserts the auto-stop path committed a
//! bundle and stamped `stop_trigger == "auto_duration"` on the
//! summary. This is the candidate follow-up to the manual smoke
//! documented in `specs/rdog-acceptance-matrix.md`.

use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct RecordingDaemon {
    child: Child,
    _test_lock: fs::File,
    recording_root: PathBuf,
}

impl Drop for RecordingDaemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = fs::remove_dir_all(&self.recording_root);
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
    let probe = TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(50),
    );
    probe.is_ok()
}

fn wait_until_port_is_busy(child: &mut Child, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for daemon")
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

fn read_response_line(stream: &mut TcpStream, timeout: Duration) -> String {
    // Read everything the daemon has sent until the read deadline
    // expires. The daemon emits savefile frames (`@savefile ...`)
    // before the `@response ...` envelope on `@record-stop`, so we
    // can't bound the read by a single `@response` marker because the
    // response body itself may also be split across multiple TCP
    // packets. Reading until quiet is the simplest robust strategy.
    let deadline = Instant::now() + timeout;
    let mut output = String::new();
    let mut buffer = [0_u8; 4096];
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    while Instant::now() < deadline {
        match stream.read(&mut buffer) {
            Ok(0) => return output,
            Ok(len) => {
                output.push_str(&String::from_utf8_lossy(&buffer[..len]));
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // "读直到安静"只对已经见过 @response 的流成立: savefile 多帧
                // 之后的安静可以收工。慢 runner 上 daemon 处理 @record-start
                // (semantic profile 启动) 可能超过 200ms, 响应前的安静只是
                // 中间状态 — 此时返回会让上层拿到空串 unwrap None 稳定炸
                // (CI 环境决定性, 本地快机器察觉不到)。没见到 @response
                // 就继续等到 deadline。
                if output.contains("@response ") {
                    return output;
                }
                continue;
            }
            Err(err) => panic!("read should not fail: {err}"),
        }
    }
    output
}

fn spawn_recording_child(
    binary: &std::path::Path,
    port: u16,
    recording_root: &std::path::Path,
) -> Child {
    match Command::new(binary)
        .arg("daemon")
        .env("RDOG_ZENOH__ENABLED", "false")
        .env("RDOG_OBSERVATION__DURABLE_ENABLED", "false")
        .env("RDOG_OUTBOUND__ENABLED", "false")
        .env("RDOG_INBOUND__ENABLED", "true")
        .env("RDOG_INBOUND__HOST", "127.0.0.1")
        .env("RDOG_INBOUND__PORT", port.to_string())
        .env("RDOG_INBOUND__SHELL", "/bin/sh")
        .env("RDOG_INBOUND__MODE", "control")
        .env("RDOG_RECORDING_DIR", &recording_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(recording_root);
            panic!("daemon should start: {error}");
        }
    }
}

/// Spawn a TCP control daemon wired to a unique recording dir.
///
/// Returns the daemon guard, the inbound port, and the recording root.
fn spawn_recording_daemon() -> (RecordingDaemon, u16, PathBuf) {
    // macOS recording 使用 host-global capture 能力,不同测试进程不能并行占用。
    // 文件锁同时覆盖 cargo test 的线程和 nextest 的独立测试进程。
    let test_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(std::env::temp_dir().join("rdog-recording-e2e.lock"))
        .expect("recording E2E lock should open");
    test_lock.lock().expect("recording E2E lock should acquire");

    let port = next_free_port();
    let recording_root = std::env::temp_dir().join(format!(
        "rdog-recording-e2e-{}-{}",
        std::process::id(),
        port
    ));
    fs::create_dir_all(&recording_root).expect("recording root should create");
    let child = spawn_recording_child(&rdog_binary_path(), port, &recording_root);

    let mut daemon = RecordingDaemon {
        child,
        _test_lock: test_lock,
        recording_root: recording_root.clone(),
    };

    assert!(
        wait_until_port_is_busy(&mut daemon.child, port, Duration::from_secs(3)),
        "daemon never started listening on port {port}",
    );
    assert!(
        wait_until_daemon_answers(port, Duration::from_secs(20)),
        "daemon never answered a @ping probe on port {port}",
    );

    (daemon, port, recording_root)
}

/// 端口监听 != 控制面就绪: 负载 CI 上 daemon 首个业务往返可能超过单次
/// 5s 静默读窗口 (2026-08-28 CI 实证 start 响应 5s 未达, unwrap None)。
/// 用幂等的 @ping 探活, 应答过一次 @response 才放行测试, 消除所有
/// 测试的首轮往返竞态。
fn wait_until_daemon_answers(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut probe) = TcpStream::connect(("127.0.0.1", port)) {
            let response = send_line_and_read_response(&mut probe, "@ping");
            drop(probe);
            if parse_response_value(&response).is_some() {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn recording_daemon_spawn_failure_should_remove_temporary_root() {
    let port = next_free_port();
    let recording_root = std::env::temp_dir().join(format!(
        "rdog-recording-e2e-spawn-failure-{}-{port}",
        std::process::id()
    ));
    fs::create_dir_all(&recording_root).expect("recording root should create");

    let result = std::panic::catch_unwind(|| {
        spawn_recording_child(
            std::path::Path::new("/definitely/missing/rdog"),
            port,
            &recording_root,
        );
    });

    assert!(result.is_err(), "missing daemon binary should panic");
    assert!(
        !recording_root.exists(),
        "spawn failure should remove temporary recording root"
    );
}

fn send_line_and_read_response(stream: &mut TcpStream, line: &str) -> String {
    stream
        .write_all(line.as_bytes())
        .expect("should write line");
    stream.write_all(b"\n").expect("should write newline");
    stream.flush().expect("should flush");
    read_response_line(stream, Duration::from_secs(5))
}

fn parse_response_value(line: &str) -> Option<String> {
    // The daemon may emit a `@savefile` frame before the `@response`
    // envelope. Find the `@response` marker and return the JSON body.
    let idx = line.find("@response")?;
    let after = line[idx + "@response".len()..].trim();
    Some(after.to_owned())
}

#[test]
fn recording_auto_stop_pipeline_commits_bundle_and_marks_auto_duration() {
    let (mut daemon, port, recording_root) = spawn_recording_daemon();

    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("client should connect");

    // 1. Start a 200 ms auto-stop recording.
    let start_line = r#"@record-start:{"profile":"semantic","duration_ms":200}"#;
    let start_resp = send_line_and_read_response(&mut client, start_line);
    assert!(
        parse_response_value(&start_resp).is_some(),
        "start response should be @response JSON, got: {start_resp}",
    );
    let start_body = parse_response_value(&start_resp).unwrap();
    assert!(
        start_body.contains(r#""kind":"record-start""#),
        "start response should include record-start: {start_body}",
    );
    assert!(
        start_body.contains(r#""duration_ms":200"#),
        "start response should echo duration_ms: {start_body}",
    );

    // 2. Sleep past the deadline so the auto-stop fires.
    thread::sleep(Duration::from_millis(300));

    // 3. Probe status — the next handler call observes the FIRED flag
    //    and runs the auto-stop inline.
    let status_line = r#"@record-status"#;
    let status_resp = send_line_and_read_response(&mut client, status_line);
    let status_body = parse_response_value(&status_resp)
        .unwrap_or_else(|| panic!("status response should be @response JSON, got: {status_resp}"));
    assert!(
        status_body.contains(r#""status":"idle""#),
        "status should report idle after auto-stop: {status_body}",
    );
    assert!(
        status_body.contains(r#""last_session""#),
        "status should include last_session: {status_body}",
    );
    assert!(
        status_body.contains(r#""phase":"completed""#),
        "last_session.phase should be completed: {status_body}",
    );
    assert!(
        status_body.contains(r#""stop_trigger":"auto_duration""#),
        "last_session.stop_trigger should be auto_duration: {status_body}",
    );

    // 4. Bundle on disk — the auto-stop path committed a tar under
    //    `bundle/rec-*.rdogrec.tar`.
    let bundle_dir = recording_root.join("bundle");
    let mut bundle_files: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .expect("bundle dir should exist")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|s| s.to_str()) == Some("tar")
                && p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with("rec-"))
                    .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        bundle_files.len(),
        1,
        "expected exactly one recording bundle, got {bundle_files:?}",
    );
    let bundle_path = bundle_files.pop().unwrap();
    let metadata = fs::metadata(&bundle_path).expect("bundle file should be readable");
    assert!(
        metadata.len() > 0,
        "bundle file should be non-empty: {}",
        bundle_path.display()
    );

    // 5. Cleanup.
    client.shutdown(std::net::Shutdown::Both).ok();
    daemon
        .child
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.child.wait();
    let _ = fs::remove_dir_all(&recording_root);
}

#[test]
fn recording_duration_too_small_returns_4121_without_starting_session() {
    let (mut daemon, port, recording_root) = spawn_recording_daemon();
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("client should connect");

    // 50 ms is below the 100 ms minimum, so the daemon must reject
    // the start. No session should be created.
    let start_line = r#"@record-start:{"profile":"semantic","duration_ms":50}"#;
    let start_resp = send_line_and_read_response(&mut client, start_line);
    let body = parse_response_value(&start_resp)
        .unwrap_or_else(|| panic!("response should be @response JSON, got: {start_resp}"));
    assert!(
        body.contains(r#""error_code":"DURATION_TOO_SMALL""#),
        "start should reject too-small duration: {body}",
    );

    // Status should still be idle and no session should have been
    // committed.
    let status_resp = send_line_and_read_response(&mut client, r#"@record-status"#);
    let status_body = parse_response_value(&status_resp).unwrap();
    assert!(
        status_body.contains(r#""status":"idle""#) && !status_body.contains(r#""last_session""#),
        "no session should be active after rejected start: {status_body}",
    );

    client.shutdown(std::net::Shutdown::Both).ok();
    daemon
        .child
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.child.wait();
    let _ = fs::remove_dir_all(&recording_root);
}

#[test]
fn recording_manual_cancel_before_deadline_leaves_no_bundle() {
    let (mut daemon, port, recording_root) = spawn_recording_daemon();
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("client should connect");

    // 1. Start a 1s auto-stop recording.
    let start_resp = send_line_and_read_response(
        &mut client,
        r#"@record-start:{"profile":"semantic","duration_ms":1000}"#,
    );
    assert!(
        parse_response_value(&start_resp)
            .unwrap()
            .contains(r#""duration_ms":1000"#),
        "start should echo duration_ms: {start_resp}",
    );

    // 2. Cancel well before the deadline; the auto-stop timer must
    //    observe the manual cancel and exit without committing a bundle.
    thread::sleep(Duration::from_millis(120));
    let cancel_resp = send_line_and_read_response(&mut client, r#"@record-cancel"#);
    let cancel_body = parse_response_value(&cancel_resp).unwrap();
    assert!(
        cancel_body.contains(r#""phase":"cancelled""#),
        "cancel should report phase=cancelled: {cancel_body}",
    );

    // 3. Snapshot the bundle directory before the original deadline
    //    so we can confirm nothing lands after the cancel.
    let bundle_dir = recording_root.join("bundle");
    let bundles_after_cancel: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .map(|iter| {
            iter.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s == "tar")
                        .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();

    // 4. Wait past the deadline and probe status again — the session
    //    should still be in cancelled state with no new bundle.
    thread::sleep(Duration::from_millis(1_100));
    let status_resp = send_line_and_read_response(&mut client, r#"@record-status"#);
    let status_body = parse_response_value(&status_resp).unwrap();
    assert!(
        status_body.contains(r#""status":"idle""#),
        "session should be idle after cancel: {status_body}",
    );
    assert!(
        status_body.contains(r#""phase":"cancelled""#),
        "last_session.phase should remain cancelled: {status_body}",
    );
    assert!(
        status_body.contains(r#""stop_trigger":"manual""#),
        "last_session.stop_trigger should be manual: {status_body}",
    );

    let bundles_after_deadline: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .map(|iter| {
            iter.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s == "tar")
                        .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();

    assert_eq!(
        bundles_after_cancel.len(),
        bundles_after_deadline.len(),
        "no new bundle should land after manual cancel. before: {bundles_after_cancel:?}, after: {bundles_after_deadline:?}",
    );
    assert!(
        bundles_after_deadline.is_empty(),
        "manual cancel should not commit a bundle, got: {bundles_after_deadline:?}",
    );

    client.shutdown(std::net::Shutdown::Both).ok();
    daemon
        .child
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.child.wait();
    let _ = fs::remove_dir_all(&recording_root);
}

#[test]
fn recording_manual_stop_before_deadline_yields_manual_trigger_and_bundle() {
    let (mut daemon, port, recording_root) = spawn_recording_daemon();
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("client should connect");

    // 1. Start a 1s auto-stop recording, then stop it manually after
    //    120ms. The auto-stop timer must release cleanly and the
    //    bundle must be tagged trigger=manual.
    let start_resp = send_line_and_read_response(
        &mut client,
        r#"@record-start:{"profile":"semantic","duration_ms":1000}"#,
    );
    assert!(
        parse_response_value(&start_resp)
            .unwrap()
            .contains(r#""duration_ms":1000"#),
        "start should echo duration_ms: {start_resp}",
    );

    thread::sleep(Duration::from_millis(120));
    let stop_resp = send_line_and_read_response(&mut client, r#"@record-stop"#);
    let stop_body = parse_response_value(&stop_resp)
        .unwrap_or_else(|| panic!("stop response should be @response JSON, got: {stop_resp:?}"));
    assert!(
        stop_body.contains(r#""trigger":"manual""#),
        "manual stop should report trigger=manual (body len={})",
        stop_body.len(),
    );

    // 2. The stop response is wrapped in a savefile frame; the body
    //    mentions the bundle filename. We don't unwrap the savefile
    //    here — the bundle file is the source of truth.
    let bundle_dir = recording_root.join("bundle");
    let mut bundles: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .map(|iter| {
            iter.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s == "tar")
                        .unwrap_or(false)
                        && e.path()
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.starts_with("rec-"))
                            .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        bundles.len(),
        1,
        "manual stop should commit exactly one bundle, got {bundles:?}",
    );
    let bundle_path = bundles.pop().unwrap();
    assert!(
        fs::metadata(&bundle_path).unwrap().len() > 0,
        "bundle file should be non-empty: {}",
        bundle_path.display()
    );

    // 3. Sleep past the original deadline and confirm the timer did
    //    not fire a second auto-stop that would re-commit anything.
    thread::sleep(Duration::from_millis(1_100));
    let bundles_after: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .map(|iter| {
            iter.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s == "tar")
                        .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        bundles_after.len(),
        1,
        "manual stop should not let auto-stop fire a second bundle, got: {bundles_after:?}",
    );

    // 4. last_session should still report trigger=manual after the
    //    deadline so the owner cannot be surprised by a follow-up
    //    auto-stop.
    let status_resp = send_line_and_read_response(&mut client, r#"@record-status"#);
    let status_body = parse_response_value(&status_resp).unwrap();
    assert!(
        status_body.contains(r#""stop_trigger":"manual""#),
        "last_session.stop_trigger should remain manual: {status_body}",
    );

    client.shutdown(std::net::Shutdown::Both).ok();
    daemon
        .child
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.child.wait();
    let _ = fs::remove_dir_all(&recording_root);
}

#[test]
fn recording_auto_stop_survives_owner_disconnect_and_reconnect() {
    let (mut daemon, port, recording_root) = spawn_recording_daemon();

    // 1. Open a connection, start a 200ms auto-stop, then close the
    //    socket without sending stop. This simulates the owner
    //    connection dropping.
    let mut start_client =
        TcpStream::connect(("127.0.0.1", port)).expect("first client should connect");
    let start_resp = send_line_and_read_response(
        &mut start_client,
        r#"@record-start:{"profile":"semantic","duration_ms":200}"#,
    );
    assert!(
        parse_response_value(&start_resp)
            .unwrap()
            .contains(r#""duration_ms":200"#),
        "start should echo duration_ms: {start_resp}",
    );
    drop(start_client);

    // 2+3. 轮询探测 (每轮新连接 = 模拟不同 owner 重连): daemon 侧 auto-stop
    // 的 FIRED 置位由下一个 handler 调用捡起, 固定 sleep 一次性探测在负载
    // CI 上会早于置位 (与 handler 层测试同构的竞态, 2026-08-28 CI 实证)。
    // 轮询到 idle 为止, 上限 10s; 解析失败也重试 (连接/时序抖动)。
    let probe_deadline = std::time::Instant::now() + Duration::from_secs(10);
    #[allow(unused_assignments)] // 循环出口由 break 前赋值, 初始值仅为类型锚
    let mut status_body = String::new();
    loop {
        let mut probe_client =
            TcpStream::connect(("127.0.0.1", port)).expect("probe client should connect");
        let status_resp = send_line_and_read_response(&mut probe_client, r#"@record-status"#);
        drop(probe_client);
        if let Some(body) = parse_response_value(&status_resp) {
            if body.contains(r#""status":"idle""#) {
                status_body = body;
                break;
            }
        }
        assert!(
            std::time::Instant::now() < probe_deadline,
            "auto-stop 未在 10s 内被探测到: last response: {status_resp}"
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        status_body.contains(r#""status":"idle""#),
        "session should be idle after auto-stop: {status_body}",
    );
    assert!(
        status_body.contains(r#""phase":"completed""#),
        "last_session.phase should be completed: {status_body}",
    );
    assert!(
        status_body.contains(r#""stop_trigger":"auto_duration""#),
        "last_session.stop_trigger should be auto_duration: {status_body}",
    );
    // 清理段需要 probe_client 句柄, 重连一次供 shutdown 收口。
    let probe_client =
        TcpStream::connect(("127.0.0.1", port)).expect("probe client should reconnect for cleanup");

    // 4. Bundle on disk confirms the auto-stop path actually ran.
    let bundle_dir = recording_root.join("bundle");
    let mut bundles: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .map(|iter| {
            iter.filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s == "tar")
                        .unwrap_or(false)
                        && e.path()
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.starts_with("rec-"))
                            .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        bundles.len(),
        1,
        "auto-stop should commit exactly one bundle after owner disconnect, got: {bundles:?}",
    );
    let bundle_path = bundles.pop().unwrap();
    assert!(
        fs::metadata(&bundle_path).unwrap().len() > 0,
        "bundle file should be non-empty: {}",
        bundle_path.display()
    );

    probe_client.shutdown(std::net::Shutdown::Both).ok();
    daemon
        .child
        .kill()
        .expect("daemon should stop after test cleanup");
    let _ = daemon.child.wait();
    let _ = fs::remove_dir_all(&recording_root);
}
