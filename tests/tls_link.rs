//! `zenoh-link-tls` 的回归测试 (RAI-524)。
//!
//! 三条都针对「长命链路 / 背压 / 并发」这些首帧压力复现器覆盖不到的形状:
//!
//! 1. `close_drains_accepted_records_under_backpressure` —— 已受理的字节必须在关闭时真的
//!    送到对端, 不能"报成功但字节没出去";
//! 2. `idle_link_should_not_burn_cpu` —— 空闲链路不能空转 (读任务所在线程的 CPU 必须接近 0);
//! 3. `concurrent_writes_should_not_interleave_frames` —— 两路并发写在字节层不得交错。
//!
//! 每条测试上方都写了它对应的负对照 (把修法改回去会让它变红)。

use std::{
    io,
    sync::Arc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::{crypto::ring::default_provider, pki_types::PrivatePkcs8KeyDer, ServerConfig};
use tokio::{
    io::AsyncReadExt,
    net::{TcpSocket, TcpStream},
    time::{sleep, timeout},
};
use tokio_rustls::{server::TlsStream, TlsAcceptor};
use zenoh_link_commons::{LinkManagerUnicastTrait, LinkUnicast};
use zenoh_link_tls::LinkManagerUnicastTls;
use zenoh_protocol::core::EndPoint;

/// 对端的接收缓冲: 故意开小, 让数十 KiB 的写入必然被拆成多次 poll, 背压才暴露得出来。
const SOCKET_BUFFER: u32 = 4096;
const TEST_TIMEOUT: Duration = Duration::from_secs(60);

async fn tls_pair() -> (LinkUnicast, TlsStream<TcpStream>) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(SOCKET_BUFFER).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let endpoint: EndPoint = format!(
        "tls/{}#root_ca_certificate_base64={};so_sndbuf={SOCKET_BUFFER}",
        listener.local_addr().unwrap(),
        STANDARD.encode(cert.pem())
    )
    .parse()
    .unwrap();
    let (sender, _receiver) = flume::bounded(1);
    let manager = LinkManagerUnicastTls::new(sender);
    let (link, peer) = tokio::join!(manager.new_link(endpoint), async {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await.unwrap()
    });
    (link.unwrap(), peer)
}

/// 对端慢读: 每次只取 64 KiB 再歇一会儿, 让发送侧持续处在背压里。
async fn drip_read(peer: &mut TlsStream<TcpStream>, pause: Duration) -> Vec<u8> {
    let mut received = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match peer.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                received.extend_from_slice(&buf[..n]);
                sleep(pause).await;
            }
        }
    }
    received
}

/// 负对照: 把 `close()` 换回"锁内一次 poll_flush + 一次 poll_shutdown, 然后无条件 Ok"
/// (40631a2 的形状)。8 MiB 远超内核发送缓冲 + rustls 缓冲, 对端又慢读, 所以关闭时缓冲里必然
/// 还有已受理的记录; 旧实现把它们丢掉却报成功, 这条测试以 `received < payload.len()` 变红。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_drains_accepted_records_under_backpressure() {
    timeout(TEST_TIMEOUT, async {
        let (link, mut peer) = tls_pair().await;
        let payload = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect::<Vec<_>>();

        let writer = {
            let expected = payload.clone();
            let link = link.clone();
            tokio::spawn(async move {
                link.write_all(&expected).await.unwrap();
                // 写入刚被受理就关闭: 此刻仍有记录留在发送缓冲里没落到 TCP。
                link.close().await
            })
        };
        let received = drip_read(&mut peer, Duration::from_millis(10)).await;
        let closed = writer.await.unwrap();
        eprintln!(
            "close={closed:?}; received={}/{}",
            received.len(),
            payload.len()
        );
        closed.expect("close must not report success while dropping accepted bytes");
        assert_eq!(
            received.len(),
            payload.len(),
            "close dropped accepted bytes before they reached the peer"
        );
        assert!(received == payload, "close changed accepted bytes");
    })
    .await
    .expect("TLS close test exceeded its time limit");
}

#[cfg(target_os = "linux")]
fn clk_tck() -> f64 {
    (unsafe { libc::sysconf(libc::_SC_CLK_TCK) }) as f64
}

/// 进程内每个线程累计消耗的 CPU 时间 (utime + stime, 单位: clock ticks)。
/// 读任务可能被 tokio 换到别的 worker 上, 所以按「所有线程里最大的那一份」来判定。
#[cfg(target_os = "linux")]
fn thread_cpu_ticks_in(task_dir: &str) -> io::Result<Vec<(u64, u64)>> {
    let parse = |field: &str| {
        field
            .parse::<u64>()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    };
    let mut ticks = Vec::new();
    for entry in std::fs::read_dir(task_dir)? {
        let path = entry?.path();
        let stat = std::fs::read_to_string(path.join("stat"))?;
        let fields = stat
            .rsplit_once(')')
            .expect("thread stat has a comm field")
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        let tid = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u64>().ok())
            .expect("task directory name is a tid");
        ticks.push((tid, parse(fields[11])? + parse(fields[12])?));
    }
    Ok(ticks)
}

/// 负对照: 把等待分支里的 `permit.forget()` 改回 `drop(permit)` (许可还回信号量), 等待分支就
/// 永久就绪, 空闲读任务在自己的循环里空转 —— 复核在 `139e19f` 上量到的 daemon 98% 就是这一条。
/// 进程内的裸 link 复现不出来 (要真 daemon 的长命读任务才会走到), 所以这里起一个真 daemon 采样。
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_pty_link_should_not_burn_daemon_cpu() {
    timeout(TEST_TIMEOUT, async {
        let workdir = std::env::temp_dir().join(format!("rdog-tls-idle-{}", std::process::id()));
        let home = workdir.join("home");
        std::fs::create_dir_all(home.join(".rdog")).unwrap();
        std::fs::write(
            home.join(".rdog/auth.toml"),
            "user = \"idle\"\npassword = \"idle-pw\"\n",
        )
        .unwrap();
        let binary = std::env::var("CARGO_BIN_EXE_rdog").unwrap_or_else(|_| "rdog".to_owned());
        let auth = std::process::Command::new(&binary)
            .args(["auth", "tls-init"])
            .env("HOME", &home)
            .output()
            .expect("tls-init should run");
        assert!(auth.status.success(), "tls-init failed: {auth:?}");
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let name = format!("tls-idle-{}.lab", std::process::id());
        let config = workdir.join("daemon.toml");
        std::fs::write(
            &config,
            format!(
                "[observation]\ndurable_enabled = false\n\n[zenoh]\nenabled = true\nmode = \"router\"\n\
                 namespace = \"lab\"\ndaemon_name = \"{name}\"\n\
                 listen_endpoints = [\"tcp/127.0.0.1:{port}\"]\n\n[tls]\nenabled = true\n"
            ),
        )
        .unwrap();
        let log_path = workdir.join("daemon.log");
        let log = std::fs::File::create(&log_path).unwrap();
        let mut daemon = std::process::Command::new(&binary)
            .args(["daemon", "-c", &config.display().to_string()])
            .env("HOME", &home)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("daemon should start");
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            let text = std::fs::read_to_string(&log_path).unwrap_or_default();
            if text.contains("zenoh router daemon ready") {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        let mut holder = std::process::Command::new(&binary)
            .args([
                "control",
                "--transport",
                "zenoh",
                "--target-name",
                &name,
                "--entry-point",
                &format!("tls/127.0.0.1:{port}"),
                "--namespace",
                "lab",
                "--pty",
                "--",
                "sleep",
                "8",
            ])
            .env("HOME", &home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("pty control should start");

        let tid = daemon.id() as u64;
        sleep(Duration::from_millis(1500)).await;
        let idle = Duration::from_millis(2000);
        let before = thread_cpu_ticks_in(&format!("/proc/{tid}/task")).expect("daemon thread stats");
        sleep(idle).await;
        let after = thread_cpu_ticks_in(&format!("/proc/{tid}/task")).expect("daemon thread stats");
        let mut worst = 0f64;
        for (id, used) in &after {
            if let Some((_, start)) = before.iter().find(|(other, _)| other == id) {
                worst = worst.max(used.saturating_sub(*start) as f64 / clk_tck());
            }
        }
        eprintln!("idle daemon cpu: {worst:.2}s over {:.2}s", idle.as_secs_f64());

        let _ = holder.kill();
        let _ = holder.wait();
        let _ = daemon.kill();
        let _ = daemon.wait();
        std::fs::remove_dir_all(&workdir).ok();

        assert!(
            worst < 0.3 * idle.as_secs_f64(),
            "idle TLS pty link burned {worst:.2}s of daemon CPU in {:.2}s",
            idle.as_secs_f64()
        );
    })
    .await
    .expect("TLS idle CPU test exceeded its time limit");
}

/// 负对照: 去掉整调用 `write_gate`, 对端慢读时一次 `write_all` 会被拆成多次 poll, 另一个写入
/// 就插进帧体中间, 长度前缀或帧体齐整性断言变红。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_writes_should_not_interleave_frames() {
    timeout(TEST_TIMEOUT, async {
        let (link, mut peer) = tls_pair().await;
        const FRAME: usize = 1024 * 1024;
        let frame = |byte: u8| {
            let mut buf = Vec::with_capacity(4 + FRAME);
            buf.extend_from_slice(&(FRAME as u32).to_be_bytes());
            buf.extend_from_slice(&vec![byte; FRAME]);
            buf
        };
        let a = frame(0xAA);
        let b = frame(0xBB);

        // 对端慢读, 否则一次 write_all 会一口气被受理, 交错窗口不出现。
        let reader = tokio::spawn(async move { drip_read(&mut peer, Duration::from_millis(5)).await });
        // 两路写入并发进行, 一次 write_all 就是一个整调用。
        let (wa, wb) = tokio::join!(link.write_all(&a), link.write_all(&b));
        wa.unwrap();
        wb.unwrap();
        // 写进 rustls 缓冲但还没落到 TCP 的字节由 close() 负责推完。
        link.close()
            .await
            .expect("close should deliver the accepted bytes");
        let received = reader.await.unwrap();

        let mut frames = Vec::new();
        let mut rest = received.as_slice();
        while !rest.is_empty() {
            assert!(rest.len() >= 4, "stream ends inside a frame length prefix");
            let (len, body) = rest.split_at(4);
            let len = u32::from_be_bytes(len.try_into().unwrap()) as usize;
            assert_eq!(len, FRAME, "frame length prefix was corrupted");
            assert!(body.len() >= len, "stream ends inside a frame body");
            frames.push(&body[..len]);
            rest = &body[len..];
        }
        assert_eq!(frames.len(), 2, "expected exactly two frames");
        let mut seen = [0usize; 2];
        for frame in frames {
            let byte = frame[0];
            let idx = [0xAAu8, 0xBBu8]
                .iter()
                .position(|candidate| *candidate == byte)
                .unwrap_or_else(|| panic!("frame starts with an unexpected byte {byte:#x}"));
            assert!(
                frame.iter().all(|inner| *inner == byte),
                "frame was interleaved with another writer's bytes"
            );
            seen[idx] += 1;
        }
        assert_eq!(seen, [1, 1], "expected exactly one frame from each writer");
    })
    .await
    .expect("TLS interleaving test exceeded its time limit");
}
