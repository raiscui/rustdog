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
    matches!(probe, Ok(_))
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
    let deadline = Instant::now() + timeout;
    let mut output = String::new();
    let mut buffer = [0_u8; 1024];
    stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    while Instant::now() < deadline {
        match stream.read(&mut buffer) {
            Ok(0) => return output,
            Ok(len) => {
                output.push_str(&String::from_utf8_lossy(&buffer[..len]));
                if output.contains('\n') {
                    return output;
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(err) => panic!("read should not fail: {err}"),
        }
    }
    output
}

/// Spawn a TCP control daemon wired to a unique recording dir.
///
/// Returns the daemon `Child`, the inbound port, and the recording
/// root (so the test can poke at the journal/bundle dirs).
fn spawn_recording_daemon() -> (Child, u16, PathBuf) {
    let port = next_free_port();
    let binary = rdog_binary_path();
    let recording_root = std::env::temp_dir().join(format!(
        "rdog-recording-e2e-{}-{}",
        std::process::id(),
        port
    ));
    fs::create_dir_all(&recording_root).expect("recording root should create");

    let mut daemon = Command::new(binary)
        .arg("daemon")
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
        .expect("daemon should start");

    assert!(
        wait_until_port_is_busy(&mut daemon, port, Duration::from_secs(3)),
        "daemon never started listening on port {port}",
    );

    (daemon, port, recording_root)
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
    // The daemon wraps each response in `@response {...}`. Strip the
    // prefix and return the JSON body so the test can grep for
    // sub-fields.
    line.strip_prefix("@response").map(|s| s.trim().to_owned())
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
    daemon.kill().expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
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
        status_body.contains(r#""status":"idle""#)
            && !status_body.contains(r#""last_session""#),
        "no session should be active after rejected start: {status_body}",
    );

    client.shutdown(std::net::Shutdown::Both).ok();
    daemon.kill().expect("daemon should stop after test cleanup");
    let _ = daemon.wait();
    let _ = fs::remove_dir_all(&recording_root);
}
