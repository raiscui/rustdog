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

/// 写一份 32 字节的 identity 文件与一个最小 `ClientConfig`: `path=direct`, 直连 loopback 端点,
/// 因此不需要 relay / directory 的任何证书。
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
