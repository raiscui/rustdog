//! `with_bulk_lane_at` 的执行模型回归测试 (真机 panic: "Cannot start a runtime from within a runtime")。
//!
//! 回环 TCP 替身 (`bulk_tests`) 照不到这一格: 它根本不经过 `IrohLineTransport`, 也就不会有
//! "同步接口背后 `handle.block_on`" 这件事。所以这里的替身是**真 quinn 流、真 loopback 端点对**,
//! 客户端走的正是产品那条路: `with_bulk_lane_at` → `runtime().block_on` → 握手 → body →
//! `IrohBulkLane::request` → `IrohLineTransport::write_message` → `handle.block_on`。
//!
//! 负对照: 把 `with_bulk_lane_at` 里的握手与 body 挪回 async 块 (即 panic 之前的形状), 这条用例
//! 会 panic 在 "Cannot start a runtime from within a runtime"。

use super::*;
use iroh::endpoint::Connection;
use std::io::Write as _;

// 必须与产品客户端连的 ALPN 一致: `with_bulk_lane_at` 用 `CONTROL_ALPN` 发起连接。
const TEST_ALPN: &[u8] = super::CONTROL_ALPN;

/// 一个够用的"准入 lane 对端": 读一行握手, 回 `admitted`, 之后每条 `@ping` 回一行 pong 的 `@response`。
async fn serve_admitted_fixture(conn: Connection) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;
    let handshake = read_line(&mut recv).await?;
    assert!(handshake.contains("\"op\":\"admit\""), "{handshake}");
    send.write_all(b"{\"status\":\"admitted\"}\n").await?;
    loop {
        let Ok(line) = read_line(&mut recv).await else {
            break;
        };
        if line.starts_with("@ping") {
            send.write_all(b"@response {\"pong\":true}\n").await?;
        }
    }
    let _ = send.finish();
    Ok(())
}

/// 客户端的 256 KiB 块 → base64 之后约 341 KiB 一行。daemon 的读循环必须能收下并回一行。
async fn serve_echo_fixture(conn: Connection) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;
    let handshake = read_line(&mut recv).await?;
    assert!(handshake.contains("\"op\":\"admit\""), "{handshake}");
    send.write_all(b"{\"status\":\"admitted\"}\n").await?;
    loop {
        let Ok(line) = read_line(&mut recv).await else {
            break;
        };
        if line.starts_with("@ping") {
            send.write_all(b"@response {\"pong\":true}\n").await?;
        } else {
            // 回一行带长度的回执: 只有整行都被 daemon 侧读进来才会到达这里。
            send.write_all(format!("@response {{\"len\":{}}}\n", line.len()).as_bytes())
                .await?;
        }
    }
    let _ = send.finish();
    Ok(())
}

async fn read_line(recv: &mut RecvStream) -> Result<String> {
    let mut buffer = Vec::new();
    loop {
        match recv.read_chunk(64 * 1024).await? {
            None => break,
            Some(bytes) => {
                buffer.extend_from_slice(&bytes);
                if buffer.contains(&b'\n') {
                    break;
                }
            }
        }
    }
    let end = buffer.iter().position(|byte| *byte == b'\n').unwrap_or(buffer.len());
    Ok(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

/// 读走握手与下一条请求、**永不回执**, 并让连接保持开着。
///
/// "回执没来"与"连接断了"在客户端是两条完全不同的结论 (前者是本端不知道对端做了什么), 而这两条
/// 只能靠"读超时那一刻连接还开着"分开。这个对端把前者造出来: 请求确实被读走了, 就是没有答案。
async fn serve_silent_fixture(conn: Connection) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await?;
    let handshake = read_line(&mut recv).await?;
    assert!(handshake.contains("\"op\":\"admit\""), "{handshake}");
    send.write_all(b"{\"status\":\"admitted\"}\n").await?;
    let _ = read_line(&mut recv).await;
    // 保持连接直到对端自己关: 客户端读超时时 `close_reason()` 必须是 `None`。
    let _ = timeout(CONNECT, conn.closed()).await;
    Ok(())
}

/// 写一份 32 字节的 identity 文件与一个最小 `ClientConfig`: `path=direct`, 直连 loopback 端点,
/// 因此不需要 relay / 目录的任何证书。
fn client_config_for(key_file: &Path) -> ClientConfig {
    let mut file = File::create(key_file).expect("identity file");
    file.write_all(&[7u8; 32]).expect("identity bytes");
    ClientConfig {
        key_file: key_file.to_path_buf(),
        directory: Directory {
            id: SecretKey::generate().public(),
            addr: None,
            via_relay: false,
            relay: None,
        },
        relay: Relay {
            url: "https://relay.invalid:443".parse().expect("relay url"),
            qad_port: 1,
            root_cert: key_file.to_path_buf(),
        },
        path: Route::Direct,
        routable: false,
        bind: Some("127.0.0.1:0".parse().expect("bind")),
        lease_ttl_ms: None,
        frame_timeout_ms: None,
    }
}

#[test]
fn a_ping_over_the_real_line_transport_survives_the_single_runtime() {
    // 这条用例覆盖的正是真机上 panic 的那一格: 同步 `IrohBulkLane` 被一次 `runtime().block_on`
    // 包着, 而它每次读写都是 `handle.block_on`。修好之前, 握手那次写就会 panic。
    let dir = std::env::temp_dir().join(format!("rdog-lane-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let cfg = client_config_for(&dir.join("client.key"));

    let rt = runtime().expect("runtime");
    let server_key = SecretKey::generate();
    let (server, addr) = rt.block_on(async {
        let server = endpoint(server_key, Some("127.0.0.1:0".parse().unwrap()), TEST_ALPN, None)
            .await
            .expect("fixture endpoint");
        let addr = server.addr();
        (server, addr)
    });
    let acceptor = {
        let server = server.clone();
        rt.spawn(async move {
            if let Ok(Some(incoming)) = timeout(CONNECT, server.accept()).await {
                if let Ok(conn) = incoming.await {
                    let _ = serve_admitted_fixture(conn).await;
                }
            }
        })
    };

    let reply = with_bulk_lane_at(cfg, addr, |lane| lane.request("@ping", CONNECT))
        .map_err(|err| err.to_string())
        .expect("the lane handshake and one request must run without nesting block_on")
        .expect("@ping must be answered");
    assert!(reply.contains("\"pong\":true"), "{reply}");

    rt.block_on(async {
        let _ = timeout(CONNECT, server.close()).await;
    });
    acceptor.abort();
    rt.shutdown_background();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bulk_sized_chunk_line_is_read_and_answered() {
    // 真机现场: 握手通过、`@put-file` 也回了, 但第一块 (256 KiB 载荷 → base64 后约 341 KiB 一行)
    // 等不到回执。这里用同尺寸的一行直接压同一条传输路径, 把"帧太大 / 行读取卡住"与"别的状态"
    // 分开。
    let dir = std::env::temp_dir().join(format!("rdog-lane-bigline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let cfg = client_config_for(&dir.join("client.key"));

    let rt = runtime().expect("runtime");
    let server_key = SecretKey::generate();
    let (server, addr) = rt.block_on(async {
        let server = endpoint(server_key, Some("127.0.0.1:0".parse().unwrap()), TEST_ALPN, None)
            .await
            .expect("fixture endpoint");
        let addr = server.addr();
        (server, addr)
    });
    let acceptor = {
        let server = server.clone();
        rt.spawn(async move {
            if let Ok(Some(incoming)) = timeout(CONNECT, server.accept()).await {
                if let Ok(conn) = incoming.await {
                    let _ = serve_echo_fixture(conn).await;
                }
            }
        })
    };

    let payload = "A".repeat(256 * 1024);
    let line = format!("@file-chunk:{{\"transfer_id\":\"t\",\"offset\":0,\"data\":\"{payload}\"}}");
    let expected = line.len();
    let reply = with_bulk_lane_at(cfg, addr, move |lane| lane.request(&line, CONNECT))
        .map_err(|err| err.to_string())
        .expect("the lane must hand a bulk-sized line through")
        .expect("a bulk-sized line must be answered");
    assert!(
        reply.contains(&format!("\"len\":{expected}")),
        "回执里的长度必须与整行一致: {reply}"
    );

    rt.block_on(async {
        let _ = timeout(CONNECT, server.close()).await;
    });
    acceptor.abort();
    rt.shutdown_background();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_read_timeout_on_a_live_connection_maps_to_the_ack_unanswered_code() {
    // RAI-705: "回执没来"与"连接断了"必须给出两个不同的结论 —— 前者是本端不知道对端做了什么
    // (chunk 可能已落盘, `@file-end` 甚至可能已提交), 后者才是"对端没了"。这条映射靠"超时那一
    // 刻连接还开着"成立, 所以对端读走请求、永不回执、连接保持打开: 结果与链路快慢无关。
    let dir = std::env::temp_dir().join(format!("rdog-lane-acktimeout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let cfg = client_config_for(&dir.join("client.key"));

    let rt = runtime().expect("runtime");
    let server_key = SecretKey::generate();
    let (server, addr) = rt.block_on(async {
        let server = endpoint(server_key, Some("127.0.0.1:0".parse().unwrap()), TEST_ALPN, None)
            .await
            .expect("fixture endpoint");
        let addr = server.addr();
        (server, addr)
    });
    let acceptor = {
        let server = server.clone();
        rt.spawn(async move {
            if let Ok(Some(incoming)) = timeout(CONNECT, server.accept()).await {
                if let Ok(conn) = incoming.await {
                    let _ = serve_silent_fixture(conn).await;
                }
            }
        })
    };

    let err = with_bulk_lane_at(cfg, addr, |lane| {
        lane.request(
            "@file-chunk:{\"transfer_id\":\"t\",\"offset\":0,\"data\":\"\"}",
            Duration::from_millis(300),
        )
    })
    .map_err(|err| err.to_string())
    .expect("the lane handshake must succeed")
    .expect_err("对端不回执, 这次请求必须超时");
    assert!(err.contains(BULK_CHUNK_ACK_UNANSWERED), "{err}");
    assert!(
        err.contains("对端连接仍开着"),
        "连接没断时必须如实说, 否则会把人指向网络: {err}"
    );

    rt.block_on(async {
        let _ = timeout(CONNECT, server.close()).await;
    });
    acceptor.abort();
    rt.shutdown_background();
    let _ = std::fs::remove_dir_all(&dir);
}

/// 下载方向的真流回归: 客户端 `run_get` 跑在一条真 quinn 流上, 对端按 daemon 的形状回
/// `@get-file` / `@file-read` (末帧带 `evidence`)。
///
/// 真机 `--get` 出现过"握手成功但一块回执都没有、被外层 timeout 杀掉"的现场; 这条用例覆盖同一
/// 传输路径, 用来把"客户端读循环 / 帧尺寸"与"网络慢"分开。
#[test]
fn a_download_runs_over_the_real_line_transport() {
    use crate::transfer_cli::{iroh_exchange, run_get, BulkPathRecorder};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!("rdog-lane-get-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let cfg = client_config_for(&dir.join("client.key"));

    let payload: Vec<u8> = (0..300_000u32).map(|byte| (byte % 251) as u8).collect();
    let sha = format!("{:x}", Sha256::digest(&payload));

    let rt = runtime().expect("runtime");
    let server_key = SecretKey::generate();
    let (server, addr) = rt.block_on(async {
        let server = endpoint(server_key, Some("127.0.0.1:0".parse().unwrap()), TEST_ALPN, None)
            .await
            .expect("fixture endpoint");
        let addr = server.addr();
        (server, addr)
    });
    let served = payload.clone();
    let served_sha = sha.clone();
    let acceptor = {
        let server = server.clone();
        rt.spawn(async move {
            let Ok(Some(incoming)) = timeout(CONNECT, server.accept()).await else {
                return;
            };
            let Ok(conn) = incoming.await else { return };
            let Ok((mut send, mut recv)) = conn.accept_bi().await else {
                return;
            };
            let _ = read_line(&mut recv).await;
            let _ = send.write_all(b"{\"status\":\"admitted\"}\n").await;
            while let Ok(line) = read_line(&mut recv).await {
                let reply = if line.starts_with("@get-file") {
                    format!(
                        "@response {{\"kind\":\"file-transfer\",\"state\":\"reading\",\"size\":{},\"sha256\":\"{served_sha}\"}}\n",
                        served.len()
                    )
                } else if line.starts_with("@file-read") {
                    let request: serde_json::Value = serde_json::from_str(
                        line.split_once(':').map(|(_, payload)| payload).unwrap_or("{}"),
                    )
                    .unwrap_or_default();
                    let offset = request["offset"].as_u64().unwrap_or(0) as usize;
                    let length = request["length"].as_u64().unwrap_or(0) as usize;
                    let end = (offset + length).min(served.len());
                    let block = &served[offset.min(served.len())..end];
                    let eof = end >= served.len();
                    let evidence = if eof {
                        // 与 daemon 半快照同尺寸: 末帧因此最容易撞上响应行预算。
                        ", \"evidence\":{\"schema\":\"rdog.bulk-path.v1\",\"attempt_id\":\"a\",\"transfer_id\":\"t\",\"direction\":\"get\",\"path_aware\":true,\"bulk_path_pin\":false,\"bulk_path_checked_per_chunk\":true,\"path_class\":\"direct\",\"direct_addr\":null,\"bulk_bytes\":300000,\"wire_bytes\":400000,\"chunks\":9,\"concurrency_suspected\":null,\"endpoint\":\"daemon\",\"relay_tx_before\":null,\"relay_tx_after\":null,\"relay_rx_before\":null,\"relay_rx_after\":null,\"relay_sample_available\":false,\"aborted\":false,\"verdict\":\"relay_sample_unavailable\"}"
                    } else {
                        ""
                    };
                    let line = format!(
                        "@response {{\"kind\":\"file-transfer\",\"offset\":{offset},\"length\":{},\"data\":\"{}\",\"eof\":{eof}{evidence}}}",
                        block.len(),
                        BASE64.encode(block)
                    );
                    // daemon 的响应行有统一上限 (`crate::control_frames::CONTROL_RESPONSE_MAX_BYTES`), 超了会被包成
                    // `output_budget_exceeded` 或截断 —— 末帧还带着 `evidence`, 最容易撞上这条。
                    assert!(
                        line.len() < crate::control_frames::CONTROL_RESPONSE_MAX_BYTES,
                        "回执行 {} 字节, 撞上响应预算 {}",
                        line.len(),
                        crate::control_frames::CONTROL_RESPONSE_MAX_BYTES
                    );
                    format!("{line}\n")
                } else {
                    "@response {\"pong\":true}\n".to_string()
                };
                if send.write_all(reply.as_bytes()).await.is_err() {
                    break;
                }
                if line.starts_with("@file-read") && reply.contains("\"eof\":true") {
                    // 与 daemon 的收尾同形: 先把写半边 finish 掉, 再等对端关连接 —— 直接丢连接
                    // 会把末帧连同回执一起丢在地上 (客户端就报 connection lost)。
                    let _ = send.finish();
                    let _ = timeout(CONNECT, conn.closed()).await;
                    return;
                }
            }
        })
    };

    let target = dir.join("landed.bin");
    let target_for_lane = target.clone();
    let receipt = with_bulk_lane_at(cfg, addr, move |lane| {
        let exchange_peer = iroh_exchange(lane, 5_000);
        let mut recorder = BulkPathRecorder::new(
            "lane-get-test".to_string(),
            crate::bulk_path::PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
            target_for_lane.with_file_name("bulk-path.json").into(),
        );
        run_get(&exchange_peer, &mut recorder, "/remote/file.bin", &target_for_lane)
    })
    .map_err(|err| err.to_string())
    .expect("the lane ran")
    .expect("the download must finish over the real transport");

    assert_eq!(receipt.bytes, payload.len() as u64);
    assert_eq!(std::fs::read(&target).expect("landed file"), payload);

    rt.block_on(async {
        let _ = timeout(CONNECT, server.close()).await;
    });
    acceptor.abort();
    rt.shutdown_background();
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// RAI-591: lane 被中止之后的第二次尝试
// ---------------------------------------------------------------------------

/// 真 daemon 那一段: 真 `serve_admitted` → `serve_lane` → `serve_bulk_request` → 会话表,
/// 只把传输换成 loopback 的 quinn 流。客户端中途消失时留在它这里的会话就是真现场。
fn serve_real_daemon_lane(
    rt: &Runtime,
    server: &Endpoint,
    admission: Arc<Mutex<admission::Admission>>,
) -> tokio::task::JoinHandle<()> {
    let server = server.clone();
    rt.spawn(async move {
        let Ok(Some(incoming)) = timeout(CONNECT, server.accept()).await else {
            return;
        };
        let Ok(conn) = incoming.await else { return };
        let Ok((send, read)) = conn.accept_bi().await else {
            return;
        };
        let transport = ControlTransport::Iroh(IrohLineTransport::new(send, read, CONNECT));
        let probe = conn.clone();
        let peer = conn.remote_id();
        let _ = tokio::task::spawn_blocking(move || {
            let sampler = PathSampler::Lane {
                conn: &probe,
                routable: false,
            };
            let mut gate = BulkPathGate::new(ADMITTED_LANE_PATH_CAPABILITY);
            serve_admitted(transport, peer, "loopback", admission, &sampler, &mut gate)
        })
        .await;
    })
}

/// 真准入状态: 策略文件里放行这条 loopback 客户端 (identity 固定成 `[7u8; 32]` 那份)。
fn loopback_admission(dir: &Path, target: EndpointId) -> Arc<Mutex<admission::Admission>> {
    let client = SecretKey::from_bytes(&[7u8; 32]).public();
    let policy = dir.join("policy.json");
    // 直接写策略 JSON: `save_policy` 的持久化会 fsync 父目录, Windows 上打开目录当文件会失败
    // (仓库里其它用它的 fixture 都按 linux 分流), 而这里要的只是"策略文件存在且能读"。
    let body = admission::Policy {
        allow: std::collections::BTreeSet::from([client]),
        ..admission::Policy::default()
    };
    std::fs::write(&policy, serde_json::to_vec(&body).expect("policy json"))
        .expect("policy file");
    Arc::new(Mutex::new(admission::Admission::new(
        policy,
        target,
    )))
}

#[test]
fn a_lane_aborted_mid_upload_still_resumes_on_the_next_attempt() {
    use crate::transfer_cli::{iroh_exchange, run_put, BulkPathRecorder, TransferIdStore};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!("rdog-lane-abort-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let key_file = dir.join("client.key");
    let local = dir.join("source.bin");
    let remote = dir.join("landed.bin");
    let payload: Vec<u8> = (0..400_000u32).map(|byte| (byte % 251) as u8).collect();
    std::fs::write(&local, &payload).expect("source bytes");
    let sha = format!("{:x}", Sha256::digest(&payload));
    let transfer_id = "rai591-abort".to_string();

    let rt = runtime().expect("runtime");
    let server_key = SecretKey::generate();
    let (server, addr) = rt.block_on(async {
        let server = endpoint(server_key.clone(), Some("127.0.0.1:0".parse().unwrap()), TEST_ALPN, None)
            .await
            .expect("fixture endpoint");
        let addr = server.addr();
        (server, addr)
    });
    let admission = loopback_admission(&dir, server_key.public());

    // 第一次尝试: open + 一块, 然后客户端直接消失 (没有 `@file-end`, 也没有 `@file-cancel`)。
    let first_serve = serve_real_daemon_lane(&rt, &server, Arc::clone(&admission));
    let first_chunk = &payload[..100_000];
    let open = serde_json::json!({
        "transfer_id": transfer_id,
        "attempt_id": "attempt-1",
        "path": remote.display().to_string(),
        "size": payload.len(),
        "sha256": sha,
        "resume": false,
    });
    let first = with_bulk_lane_at(client_config_for(&key_file), addr.clone(), {
        let open = format!("@put-file:{open}");
        let chunk = format!(
            "@file-chunk:{}",
            serde_json::json!({
                "transfer_id": transfer_id,
                "offset": 0,
                "data": BASE64.encode(first_chunk),
            })
        );
        move |lane| {
            let opened = lane.request(&open, CONNECT).expect("open must be answered");
            assert!(opened.contains("\"written\""), "{opened}");
            let accepted = lane.request(&chunk, CONNECT).expect("chunk must be answered");
            assert!(accepted.contains("\"written\""), "{accepted}");
        }
    });
    first.expect("the aborted attempt must at least run");
    first_serve.abort();

    // 会话与半成品此刻都留在 daemon 侧: 客户端没来得及 cancel, 这正是"被中止的 lane"。
    // 端点本身不重启 —— 判据要求的是"不重启 daemon 也能恢复", 这里连进程都没换。
    let staged: Vec<u64> = std::fs::read_dir(dir.join(".rdog-partial"))
        .expect("the daemon keeps the partial under .rdog-partial")
        .map(|entry| entry.expect("partial entry").metadata().expect("metadata").len())
        .collect();
    assert_eq!(
        staged,
        vec![100_000],
        "第一次尝试必须留下 100000 字节的可续半成品"
    );

    // 第二次尝试: 同一条 (目标, 本地, 远端) 复用同一个 transfer_id。
    let ids = TransferIdStore::at(dir.join("ids"));
    ids.seed("loopback", &local, &remote.display().to_string(), &transfer_id);
    let second_serve = serve_real_daemon_lane(&rt, &server, Arc::clone(&admission));
    let remote_text = remote.display().to_string();
    let evidence_out = dir.join("bulk-path.json");
    let receipt = with_bulk_lane_at(client_config_for(&key_file), addr.clone(), {
        let ids = ids;
        let local = local.clone();
        move |lane| {
            let exchange_peer = iroh_exchange(lane, 20_000);
            let mut recorder = BulkPathRecorder::new(
                "lane-abort-retry".to_string(),
                crate::bulk_path::PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
                Some(evidence_out),
            );
            run_put(
                &exchange_peer,
                &mut recorder,
                &ids,
                "loopback",
                &local,
                &remote_text,
                false,
            )
        }
    });
    second_serve.abort();

    let receipt = receipt
        .map_err(|err| err.to_string())
        .expect("the resumed attempt must run")
        .expect("the resumed attempt must commit");
    assert_eq!(receipt.transfer_id, transfer_id);
    assert_eq!(
        receipt.resume_honored,
        Some(true),
        "被打断的 lane 之后重跑必须真的续上 (收到 {receipt:?})"
    );
    assert_eq!(std::fs::read(&remote).expect("landed file"), payload);

    crate::control_file_transfer::forget_sessions_for_tests(&[&transfer_id]);
    rt.block_on(async {
        let _ = timeout(CONNECT, server.close()).await;
    });
    rt.shutdown_background();
    let _ = std::fs::remove_dir_all(&dir);
}
