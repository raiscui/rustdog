use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub struct ControlSession {
    child: Child,
    stdin: ChildStdin,
    output_rx: Receiver<String>,
    output: String,
}

impl ControlSession {
    pub fn spawn(binary: &Path, workdir: &Path, port: u16) -> Self {
        let mut child = Command::new(binary)
            .args(["control", "127.0.0.1", &port.to_string()])
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("control session should start");
        let stdin = child.stdin.take().expect("control stdin should exist");
        let stdout = child.stdout.take().expect("control stdout should exist");
        let stderr = child.stderr.take().expect("control stderr should exist");
        let (output_tx, output_rx) = mpsc::channel();

        spawn_output_reader(stdout, output_tx.clone());
        spawn_output_reader(stderr, output_tx);

        Self {
            child,
            stdin,
            output_rx,
            output: String::new(),
        }
    }

    pub fn send(&mut self, script: &str) {
        self.stdin
            .write_all(script.as_bytes())
            .expect("should send control script to persistent session");
        self.stdin
            .flush()
            .expect("control stdin should flush after script");
    }

    pub fn wait_for_all(&mut self, label: &str, needles: &[&str], timeout: Duration) -> String {
        let start_len = self.output.len();
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if needles
                .iter()
                .all(|needle| self.output[start_len..].contains(needle))
            {
                return self.output[start_len..].to_owned();
            }

            if self
                .child
                .try_wait()
                .expect("control try_wait should not fail")
                .is_some()
            {
                panic!(
                    "control session exited while waiting for {label}\n{}",
                    self.output
                );
            }

            if let Ok(line) = self.output_rx.recv_timeout(Duration::from_millis(20)) {
                self.output.push_str(&line);
                self.output.push('\n');
            }
        }

        panic!(
            "timed out while waiting for {label}; missing one of {needles:?}\n{}",
            self.output
        );
    }
}

impl Drop for ControlSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct DaemonGuard {
    child: Child,
}

impl DaemonGuard {
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        stop_child(&mut self.child);
    }
}

pub fn start_daemon(binary: &Path, workdir: &Path, port: u16) -> DaemonGuard {
    DaemonGuard {
        child: Command::new(binary)
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
            .expect("daemon should start"),
    }
}

pub fn next_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should expose local addr")
        .port();
    drop(listener);
    port
}

pub fn rdog_binary_path() -> PathBuf {
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

pub fn temp_workdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rdog-control-mouse-e2e-{name}-{}-{}",
        std::process::id(),
        next_free_port()
    ));
    fs::create_dir_all(&path).expect("temp workdir should create");
    path
}

pub fn wait_until_port_is_busy(child: &mut Child, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("try_wait should not fail while waiting for process")
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

fn spawn_output_reader<R>(reader: R, sender: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = line.unwrap_or_else(|err| format!("control output read error: {err}"));
            if sender.send(line).is_err() {
                break;
            }
        }
    });
}

fn is_port_listening(port: u16) -> bool {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .expect("lsof should be available for macOS integration tests");

    output.status.success() && !output.stdout.is_empty()
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
