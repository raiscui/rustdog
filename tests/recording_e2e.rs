#![cfg(not(windows))]

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(20);

struct TestDaemon {
    child: Child,
    root: PathBuf,
    port: u16,
}

impl TestDaemon {
    fn start() -> Self {
        let root =
            std::env::temp_dir().join(format!("rdog-recording-e2e-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("create isolated recording test HOME");
        let port = TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve loopback port")
            .local_addr()
            .expect("read reserved port")
            .port();
        let stderr = File::create(root.join("daemon.stderr")).expect("create daemon stderr log");
        let mut command = Command::new(env!("CARGO_BIN_EXE_rdog"));
        isolate(&mut command, &root);
        let child = command
            .arg("daemon")
            .env("RDOG_ZENOH__ENABLED", "false")
            .env("RDOG_OBSERVATION__DURABLE_ENABLED", "false")
            .env("RDOG_OUTBOUND__ENABLED", "false")
            .env("RDOG_INBOUND__ENABLED", "true")
            .env("RDOG_INBOUND__HOST", "127.0.0.1")
            .env("RDOG_INBOUND__PORT", port.to_string())
            .env("RDOG_INBOUND__SHELL", "/bin/sh")
            .env("RDOG_INBOUND__MODE", "control")
            .env("RDOG_RECORDING_DIR", root.join("recordings"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("spawn isolated recording daemon");
        let mut daemon = Self { child, root, port };
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = daemon.child.try_wait().expect("check daemon startup") {
                panic!(
                    "isolated daemon exited during startup: {status}; stderr:\n{}",
                    daemon.stderr()
                );
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "isolated daemon did not listen on {port}; stderr:\n{}",
                daemon.stderr()
            );
            thread::sleep(Duration::from_millis(20));
        }
        daemon
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.root.join("daemon.stderr"))
            .unwrap_or_else(|e| format!("cannot read daemon stderr: {e}"))
    }

    fn connect(&self) -> BufReader<TcpStream> {
        let stream = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|e| panic!("connect to test daemon: {e}; stderr:\n{}", self.stderr()));
        stream
            .set_read_timeout(Some(TIMEOUT))
            .expect("set response timeout");
        stream
            .set_write_timeout(Some(TIMEOUT))
            .expect("set request timeout");
        BufReader::new(stream)
    }

    fn exchange(&self, stream: &mut BufReader<TcpStream>, request: &str) -> Value {
        writeln!(stream.get_mut(), "{request}")
            .unwrap_or_else(|e| panic!("send {request}: {e}; daemon stderr:\n{}", self.stderr()));
        let mut line = String::new();
        let bytes = stream.read_line(&mut line).unwrap_or_else(|e| {
            panic!(
                "read response to {request}: {e}; daemon stderr:\n{}",
                self.stderr()
            )
        });
        assert!(
            bytes > 0 && line.ends_with('\n'),
            "incomplete response to {request}: {line:?}; daemon stderr:\n{}",
            self.stderr()
        );
        let body = line.strip_prefix("@response ").unwrap_or_else(|| {
            panic!(
                "unexpected response to {request}: {line:?}; daemon stderr:\n{}",
                self.stderr()
            )
        });
        serde_json::from_str(body).unwrap_or_else(|e| {
            panic!(
                "invalid response to {request}: {e}: {line:?}; daemon stderr:\n{}",
                self.stderr()
            )
        })
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::thread::panicking() {
            eprintln!("test daemon stderr:\n{}", self.stderr());
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn isolate(command: &mut Command, home: &std::path::Path) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RDOG_") {
            command.env_remove(key);
        }
    }
    command.env("HOME", home).current_dir(home);
}

struct TestChild(Child);
impl Drop for TestChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn unsupported_capture_rejects_start_and_preserves_control_connection() {
    let daemon = TestDaemon::start();
    let mut owner = daemon.connect();
    let mut other = daemon.connect();
    let start = daemon.exchange(
        &mut owner,
        r#"@record-start#41:{"profile":"semantic","targets":[{"pid":1,"hwnd":1}]}"#,
    );
    assert_eq!(start["id"], 41, "start response id: {start}");
    assert_eq!(
        start["error_code"], "RECORD_UNAVAILABLE",
        "unsupported native capture must fail: {start}"
    );
    let journal = daemon.root.join("recordings").join("journal");
    if journal.exists() {
        assert_eq!(
            fs::read_dir(&journal)
                .expect("read isolated journal directory")
                .count(),
            0,
            "failed capture must not create a journal in {}",
            journal.display()
        );
    }
    let status = daemon.exchange(&mut owner, r#"@record-status#42:{}"#);
    assert_eq!(status["id"], 42, "status response id: {status}");
    assert_eq!(
        status["value"]["schema"], "rdog.record-control.v2",
        "status schema: {status}"
    );
    assert_eq!(
        status["value"]["phase"], "idle",
        "status after rejected start: {status}"
    );
    let stop = daemon.exchange(
        &mut other,
        r#"@record-stop#43:{"recording_id":"not-owned"}"#,
    );
    assert_eq!(stop["id"], 43, "stop response id: {stop}");
    assert_eq!(
        stop["error_code"], "RECORD_NOT_OWNER",
        "non-owner stop must be rejected: {stop}"
    );
    let ping = daemon.exchange(&mut other, "@ping#44");
    assert_eq!(ping["id"], 44, "ping response id: {ping}");
    assert_eq!(
        ping["value"]["pong"],
        json!(true),
        "ping after rejected stop: {ping}"
    );
}

#[test]
fn record_cli_status_then_eof_exits_against_real_daemon() {
    let daemon = TestDaemon::start();
    let stdout_path = daemon.root.join("cli.stdout");
    let stderr_path = daemon.root.join("cli.stderr");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rdog"));
    isolate(&mut command, &daemon.root);
    let child = command
        .args(["record", "--transport", "tcp", "--artifacts-dir"])
        .arg(daemon.root.join("downloads"))
        .arg("127.0.0.1")
        .arg(daemon.port.to_string())
        .stdin(Stdio::piped())
        .stdout(File::create(&stdout_path).expect("create CLI stdout"))
        .stderr(File::create(&stderr_path).expect("create CLI stderr"))
        .spawn()
        .expect("spawn persistent record CLI");
    let mut child = TestChild(child);
    let mut stdin = child.0.stdin.take().expect("record CLI stdin");
    writeln!(stdin, "status").expect("write status to record CLI");
    drop(stdin);
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("check record CLI exit") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "record CLI did not exit after EOF; CLI stderr:\n{}; daemon stderr:\n{}",
            fs::read_to_string(&stderr_path).unwrap_or_default(),
            daemon.stderr()
        );
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = fs::read_to_string(stdout_path).expect("read record CLI stdout");
    let stderr = fs::read_to_string(stderr_path).expect("read record CLI stderr");
    assert!(
        status.success(),
        "record CLI failed ({status}); stdout: {stdout}; stderr: {stderr}; daemon stderr:\n{}",
        daemon.stderr()
    );
    let value: Value = stdout.lines().find_map(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(|| panic!("record CLI did not print a JSON status; stdout: {stdout}; stderr: {stderr}; daemon stderr:\n{}", daemon.stderr()));
    assert_eq!(
        value["schema"], "rdog.record-control.v2",
        "record CLI status: {value}"
    );
    assert_eq!(value["phase"], "idle", "record CLI status: {value}");
}
