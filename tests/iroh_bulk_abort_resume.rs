//! RAI-591 的三条行为门: 中止之后能续上, 而且**不打断活着的 lane**。
//!
//! 现场是三个真进程 (rendezvous + daemon + 客户端), 不是替身: 中止的形状是"客户端进程消失,
//! daemon 那边还挂着它那条已准入的 lane"。
//!
//! - 修复 RAI-591 之前: 中止之后的每一次重跑都是 `iroh_path_unknown`, 只能等传输层自己超时
//!   (实测约 40 s) 或重启 daemon。
//! - 顶替规则写宽的代价 (本文件里两条并发的门盯着): 无条件关掉旧连接会把**正在搬的上传**打断,
//!   同一身份的并发传输也一并废掉 —— 这两格修复前是成立的, 不许退。

#[path = "support/rdog_binary.rs"]
mod rdog_binary;

use rdog_binary::rdog_binary_path;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const TARGET: &str = "lab/bulk";

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("free port")
        .local_addr()
        .expect("bound addr")
        .port()
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .expect("free udp port")
        .local_addr()
        .expect("bound udp addr")
        .port()
}

fn write_config(dir: &Path, name: &str, value: &serde_json::Value) -> String {
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec(value).expect("config json")).expect("config file");
    path.display().to_string()
}

fn wait_for(log: &Path, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        if std::fs::read_to_string(log).unwrap_or_default().contains(marker) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "{marker} never appeared in {}: {}",
        log.display(),
        std::fs::read_to_string(log).unwrap_or_default()
    );
}

struct Owned(Child);

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn staged_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir.join(".rdog-partial"))
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.metadata().ok().map(|meta| meta.len()))
                .sum()
        })
        .unwrap_or(0)
}

/// 一套跑起来的 rendezvous + daemon + 客户端 profile。
struct Harness {
    home: PathBuf,
    binary: PathBuf,
    client: String,
    remote: PathBuf,
    payload: Vec<u8>,
    _rendezvous: Owned,
    _daemon: Owned,
    daemon_log: PathBuf,
}

impl Harness {
    fn start(name: &str, size: usize) -> Self {
        let binary = rdog_binary_path();
        let home = std::env::temp_dir().join(format!("rdog-rai591-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("scratch home");

        let rendezvous_key = iroh::SecretKey::generate();
        let daemon_key = iroh::SecretKey::generate();
        let client_key = iroh::SecretKey::generate();
        for (label, key) in [
            ("rendezvous", &rendezvous_key),
            ("daemon", &daemon_key),
            ("client", &client_key),
        ] {
            std::fs::write(home.join(format!("{label}.key")), key.to_bytes()).expect("identity");
        }

        let addr = |port: u16| format!("127.0.0.1:{port}");
        let (directory, http, https, qad) =
            (free_udp_port(), free_tcp_port(), free_tcp_port(), free_udp_port());
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).expect("relay cert");
        let cert_path = home.join("relay.pem");
        let key_path = home.join("relay-key.pem");
        std::fs::write(&cert_path, cert.pem()).expect("cert file");
        std::fs::write(&key_path, signing_key.serialize_pem()).expect("key file");

        let directory_config = serde_json::json!({
            "id": rendezvous_key.public(),
            "addr": addr(directory),
        });
        let relay = serde_json::json!({
            "url": format!("https://{}", addr(https)),
            "qad_port": qad,
            "root_cert": cert_path.display().to_string(),
        });
        let loopback = |port: u16| SocketAddr::from(([127, 0, 0, 1], port)).to_string();

        let rendezvous = write_config(
            &home,
            "rendezvous",
            &serde_json::json!({
                "key_file": home.join("rendezvous.key").display().to_string(),
                "bind": loopback(directory),
                "relay_http": loopback(http),
                "relay_https": loopback(https),
                "qad": loopback(qad),
                "relay_cert": cert_path.display().to_string(),
                "relay_key": key_path.display().to_string(),
                "registrations": { TARGET: daemon_key.public() },
                "lookups": [client_key.public()],
            }),
        );
        let policy = home.join("policy.json");
        std::fs::write(
            &policy,
            serde_json::to_vec(&serde_json::json!({
                "allow": [client_key.public()],
                "revoked": [],
                "terminate": [],
                "parties": [],
            }))
            .expect("policy json"),
        )
        .expect("policy file");
        let daemon = write_config(
            &home,
            "daemon",
            &serde_json::json!({
                "key_file": home.join("daemon.key").display().to_string(),
                "directory": directory_config,
                "relay": relay,
                "name": TARGET,
                "allow": [client_key.public()],
                "policy_file": policy.display().to_string(),
            }),
        );
        let client = write_config(
            &home,
            "client",
            &serde_json::json!({
                "key_file": home.join("client.key").display().to_string(),
                "directory": directory_config,
                "relay": relay,
                "path": "direct",
            }),
        );

        let source = home.join("source.bin");
        let payload: Vec<u8> = (0..size as u32).map(|byte| (byte % 251) as u8).collect();
        std::fs::write(&source, &payload).expect("source bytes");

        let spawn = |name: &str, args: &[&str]| -> (Owned, PathBuf) {
            let log = home.join(format!("{name}.log"));
            let merged = std::fs::File::create(&log).expect("log file");
            let child = Command::new(&binary)
                .args(args)
                .current_dir(&home)
                .env("HOME", &home)
                .env("TMPDIR", &home)
                // debug 是判别流程的一部分: daemon 的 `iroh lane line … verb=@file-chunk`
                // 与 `iroh bulk ack …` 两行只在 debug 级出现, 而"这块到底到没到 / 回执写没写"
                // 正是 RAI-705 唯一能分开 (A)(B) 的证据。
                .env("RDOG_LOG_LEVEL", "debug")
                .stdin(Stdio::null())
                .stdout(merged.try_clone().expect("log clone"))
                .stderr(merged)
                .spawn()
                .expect("spawn rdog");
            (Owned(child), log)
        };

        let (rendezvous_child, rendezvous_log) = spawn("rendezvous", &["rendezvous", "-c", &rendezvous]);
        wait_for(&rendezvous_log, "iroh rendezvous ready");
        let (daemon_child, daemon_log) = spawn("daemon", &["daemon", "--transport", "iroh", "-c", &daemon]);
        wait_for(&daemon_log, "iroh daemon registered");

        Self {
            remote: home.join("landed.bin"),
            home: home.clone(),
            binary,
            client,
            payload,
            _rendezvous: rendezvous_child,
            _daemon: daemon_child,
            daemon_log,
        }
    }

    fn control(&self, extra: Vec<&str>) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .arg("control")
            .arg("--transport")
            .arg("iroh")
            .arg("--iroh-config")
            .arg(&self.client)
            .arg(TARGET)
            .args(extra)
            .current_dir(&self.home)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .stdin(Stdio::null());
        command
    }

    fn put(&self, source: &str) -> Command {
        let mut command = self.control(vec![
            "--put",
            source,
            "--remote-path",
            &self.remote.display().to_string(),
        ]);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    }

    fn source(&self) -> String {
        self.home.join("source.bin").display().to_string()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// 跑到"字节已经在过线、但还没提交"那一刻再杀: 用 daemon 侧的暂存文件判, 比抢进度行稳。
fn kill_mid_upload(harness: &Harness, mut child: Child) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut staged = 0;
    while Instant::now() < deadline {
        staged = staged_bytes(&harness.home);
        if staged > 0 && staged < harness.payload.len() as u64 {
            break;
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = child.kill();
    let _ = child.wait();
    staged
}

#[test]
fn a_put_killed_mid_upload_resumes_on_the_first_retry() {
    let harness = Harness::start("abort", 64 * 1024 * 1024);
    let staged = kill_mid_upload(&harness, harness.put(&harness.source()).spawn().expect("first put"));
    assert!(
        staged > 0 && staged < harness.payload.len() as u64,
        "第一次必须真的传到一半再被打断 (暂存 {staged} / {} 字节)",
        harness.payload.len()
    );
    assert!(!harness.remote.exists(), "提交前被打断就不该有落盘的目标文件");

    // **一条命令**: 中止之后的第一次重跑就要能续上 (内部按可重试码重拨, 不重启 daemon)。
    let retry = harness.put(&harness.source()).output().expect("retry put");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let daemon_tail = std::fs::read_to_string(&harness.daemon_log).unwrap_or_default();
    assert!(
        retry.status.success(),
        "中止之后的第一次重跑必须续上 (exit={:?}); 输出:\n{combined}\ndaemon:\n{daemon_tail}",
        retry.status.code()
    );
    assert!(combined.contains("resume=honored"), "必须真的续上而不是从头来:\n{combined}");
    assert_eq!(std::fs::read(&harness.remote).expect("landed file"), harness.payload);
}

#[test]
fn the_daemon_logs_both_halves_of_the_ack_discriminant() {
    // RAI-705 的判别流程完全靠 daemon 这两行: 客户端只看得见"回执没到", 分不出 daemon 没写、
    // 写了没回、还是首块根本没到达。它们只在 debug 级出现, 所以这里同时钉住"debug 起得来"与
    // "两行确实打出来" —— 否则现场只能靠猜。
    let harness = Harness::start("acklog", 320 * 1024);
    let put = harness.put(&harness.source()).output().expect("put");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&put.stdout),
        String::from_utf8_lossy(&put.stderr)
    );
    assert!(put.status.success(), "单文件上传必须成功:\n{combined}");
    assert_eq!(std::fs::read(&harness.remote).expect("landed file"), harness.payload);

    let log = std::fs::read_to_string(&harness.daemon_log).unwrap_or_default();
    assert!(
        log.contains("verb=@file-chunk"),
        "daemon 必须记录收到的块 (判据的第一半):\n{log}"
    );
    let acks = log.matches("iroh bulk ack verb=@file-chunk").count();
    assert!(
        acks >= 2,
        "320 KiB = 2 块, 每块都要有一条回执已写出的记录 (判据的第二半), 实际 {acks}:\n{log}"
    );
}

#[test]
fn a_ping_during_an_upload_does_not_break_it() {
    let harness = Harness::start("ping", 128 * 1024 * 1024);
    let upload = harness.put(&harness.source()).spawn().expect("upload");

    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && staged_bytes(&harness.home) == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    // 同一条身份的第二条连接: 只读 ping。它自己可能拿不到路 (修复前就是这样), 但它
    // **不许**把正在搬的那条 lane 顶掉。
    let ping = harness.control(vec!["@ping#7"]).output().expect("ping");
    let ping_text = format!(
        "{}{}",
        String::from_utf8_lossy(&ping.stdout),
        String::from_utf8_lossy(&ping.stderr)
    );

    assert!(
        ping.status.success(),
        "只读 ping 自己必须成立 (exit={:?}); 输出:\n{ping_text}",
        ping.status.code()
    );

    let upload = upload.wait_with_output().expect("upload result");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&upload.stdout),
        String::from_utf8_lossy(&upload.stderr)
    );
    let daemon_tail = std::fs::read_to_string(&harness.daemon_log).unwrap_or_default();
    assert!(
        upload.status.success(),
        "传输中打一条只读 ping 不许打断上传 (upload exit={:?}, ping exit={:?});\nupload:\n{combined}\nping:\n{ping_text}\ndaemon:\n{daemon_tail}",
        upload.status.code(),
        ping.status.code()
    );
    assert_eq!(
        std::fs::read(&harness.remote).expect("landed file"),
        harness.payload,
        "上传必须完整落盘 (ping: {} 字节输出, exit={:?})",
        ping_text.len(),
        ping.status.code()
    );
}

#[test]
fn two_uploads_from_one_identity_do_not_kill_each_other() {
    let harness = Harness::start("concurrent", 128 * 1024 * 1024);
    let second_source = harness.home.join("second.bin");
    std::fs::write(&second_source, &harness.payload[..harness.payload.len() / 2]).expect("second");
    let second_remote = harness.home.join("second-landed.bin");

    let first = harness.put(&harness.source()).spawn().expect("first upload");
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && staged_bytes(&harness.home) == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    let second = harness
        .control(vec![
            "--put",
            &second_source.display().to_string(),
            "--remote-path",
            &second_remote.display().to_string(),
        ])
        .output()
        .expect("second upload");

    let first = first.wait_with_output().expect("first result");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let daemon_tail = std::fs::read_to_string(&harness.daemon_log).unwrap_or_default();
    assert!(
        first.status.success(),
        "先开始的那条上传不许被后来的连接打断 (exit={:?}, second exit={:?});\nfirst:\n{combined}\nsecond:\n{}{}\ndaemon:\n{daemon_tail}",
        first.status.code(),
        second.status.code(),
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        std::fs::read(&harness.remote).expect("landed file"),
        harness.payload,
        "先来的上传必须完整落盘"
    );
}
