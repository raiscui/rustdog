//! wayfinder #124 原型 spike —— 验证 iroh 作为 rdog 跨网 lane 的四个关键断言。
//!
//! 用法(三个角色可以同进程,也可以分进程):
//!
//! ```text
//! iroh-spike relay  --http-bind 127.0.0.1:0
//! iroh-spike node   --relay <url>
//! iroh-spike client --relay <url> --target <addr-json> [--relay-only] [--bytes N]
//! ```
//!
//! 线程模型刻意对齐 rdog 的现状:**daemon 是线程式的,`src/` 里没有 async runtime**。
//! 所以这里用一个独立 tokio 运行时线程承载 async 侧,同步侧(行桥)跑在普通 std 线程上。
//! 这既是 spike 的真实性要求,也直接量出了"接 iroh 要改多少"。

use std::{net::SocketAddr, str::FromStr, sync::mpsc as std_mpsc, thread};

use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, RelayMode, RelayUrl,
    endpoint::presets,
};
use iroh_relay::RelayMap;
use serde::Serialize;
use tokio::runtime::Builder;

mod line_bridge;
mod protocol;

use line_bridge::LineBridge;
use protocol::{ALPN, MAX_CONTROL_MESSAGE_BYTES};

/// relay 模式输出。
#[derive(Serialize)]
struct RelayOut {
    relay: String,
}

/// node 模式输出 —— **这就是 rdog 目录服务要下发的内容的形状**:
/// `namespace/daemon_name → (EndpointId, RelayUrl)`。
#[derive(Serialize)]
struct NodeOut {
    id: String,
    relay: String,
    addr: EndpointAddr,
}

fn main() -> Result<()> {
    // 日志走 stderr —— stdout 专留给机器可读的 JSON 输出(否则 tracing 会污染它)。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("help");
    let opts = Opts::parse(&args[2..]);

    // 一个 tokio 运行时线程 + 主线程做驱动。rdog 的 daemon 将来需要同样的形状。
    let rt = Builder::new_multi_thread().enable_all().build()?;
    let handle = rt.handle().clone();
    thread::spawn(move || {
        rt.block_on(async {
            std::future::pending::<()>().await;
        });
    });

    match mode {
        "relay" => {
            handle.block_on(run_relay(&opts))?;
        }
        "node" => {
            handle.block_on(run_node(&opts))?;
        }
        "client" => {
            handle.block_on(run_client(&opts))?;
        }
        other => {
            eprintln!("未知模式 {other};用法见文件头注释");
            std::process::exit(2);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 命令行

#[derive(Default)]
struct Opts {
    http_bind: String,
    relay: String,
    target: String,
    relay_only: bool,
    bytes: usize,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        let mut o = Opts {
            http_bind: "127.0.0.1:0".to_string(),
            bytes: MAX_CONTROL_MESSAGE_BYTES,
            ..Default::default()
        };
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--http-bind" => o.http_bind = args.get(i + 1).cloned().unwrap_or_default(),
                "--relay" => o.relay = args.get(i + 1).cloned().unwrap_or_default(),
                "--target" => o.target = args.get(i + 1).cloned().unwrap_or_default(),
                "--bytes" => {
                    o.bytes = args
                        .get(i + 1)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(MAX_CONTROL_MESSAGE_BYTES)
                }
                "--relay-only" => o.relay_only = true,
                _ => {}
            }
            i += 2;
        }
        o
    }
}

// ---------------------------------------------------------------- 角色

/// **云端角色**:在同一个二进制里内嵌 iroh-relay server。
///
/// 这是 #110 的直接验证点:不需要 coturn、不需要 TURN 进程、不需要 OpenSSL。
async fn run_relay(opts: &Opts) -> Result<()> {
    let addr: SocketAddr = opts.http_bind.parse().context("解析 --http-bind")?;
    // `ServerConfig` 是 `#[non_exhaustive]`,外部 crate 不能用结构体字面量 ——
    // 只能 default 出来再逐字段赋值。
    let mut config = iroh_relay::server::ServerConfig::default();
    config.relay = Some(iroh_relay::server::RelayConfig::new(addr));
    let server = iroh_relay::server::Server::spawn(config)
        .await
        .context("启动内嵌 iroh-relay server")?;
    // `http_url()` 在 `test-utils` feature 后面,稳定接口是 `http_addr()`。
    let bound = server
        .http_addr()
        .context("relay server 未提供 http addr")?;
    let url = RelayUrl::from_str(&format!("http://{bound}")).context("构造 relay url")?;
    println!(
        "{}",
        serde_json::to_string(&RelayOut {
            relay: url.to_string()
        })?
    );
    use std::io::Write;
    std::io::stdout().flush()?;
    tracing::info!(relay = %url, "内嵌 relay server 已启动");
    std::future::pending::<()>().await;
    Ok(())
}

/// **daemon 角色**:绑定 endpoint,接受连接,用 rdog 的行协议应答。
async fn run_node(opts: &Opts) -> Result<()> {
    let relay_url = parse_relay(&opts.relay)?;
    let endpoint = build_endpoint(relay_url.clone(), false).await?;

    // 目录服务下发的内容形状:EndpointId + RelayUrl。
    let addr = EndpointAddr::from_parts(
        endpoint.id(),
        [iroh::TransportAddr::Relay(relay_url.clone())],
    );
    println!(
        "{}",
        serde_json::to_string(&NodeOut {
            id: endpoint.id().to_string(),
            relay: relay_url.to_string(),
            addr,
        })?
    );
    use std::io::Write;
    std::io::stdout().flush()?;

    // 每个连接一个线程。
    //
    // ⚠️ 第一版把同步侧写成**串行**循环(一个 worker 依次服务多条连接),
    // 结果实测里第二条连接被第一条堵死 —— 客户端没有干净关闭时,
    // 服务端的 `read_message()` 迟迟等不到 EOF,后续连接全部排队。
    // → rdog 的 daemon 必须能**并发**服务多个控制会话,这里如实记下来。
    loop {
        let incoming = endpoint.accept().await.context("accept 失败")?;
        let conn = incoming.await.context("握手失败")?;
        tracing::info!(remote = %conn.remote_id(), "已建立连接");
        let (send, recv) = conn.accept_bi().await.context("accept_bi 失败")?;
        let bridge = LineBridge::new(recv, send);
        thread::spawn(move || {
            let mut bridge = bridge;
            if let Err(err) = serve_lines(&mut bridge) {
                tracing::warn!(%err, "连接处理结束");
            }
        });
    }
    Ok(())
}

/// **控制端角色**:连过去,发一行,读一行。
async fn run_client(opts: &Opts) -> Result<()> {
    let relay_url = parse_relay(&opts.relay)?;
    let target: EndpointAddr =
        serde_json::from_str(&opts.target).context("解析 --target(EndpointAddr JSON)")?;

    let endpoint = build_endpoint(relay_url, opts.relay_only).await?;
    tracing::info!(
        target = %target.id.fmt_short(),
        relay_only = opts.relay_only,
        "开始连接"
    );

    let conn = endpoint
        .connect(target, ALPN)
        .await
        .context("connect 失败")?;
    tracing::info!(remote = %conn.remote_id(), "连接建立");

    let (send, recv) = conn.open_bi().await.context("open_bi 失败")?;
    let mut bridge = LineBridge::new(recv, send);

    // 同步侧必须在非 async 上下文用 blocking_recv —— 丢到独立线程。
    let (tx, rx) = std_mpsc::channel::<Option<String>>();
    let req_bytes = opts.bytes;
    thread::spawn(move || {
        let req = protocol::big_line(req_bytes);
        let t0 = std::time::Instant::now();
        let send_res = bridge.write_message(&req);
        let resp = bridge.read_message();
        let elapsed = t0.elapsed();
        let resp_len = resp
            .as_ref()
            .ok()
            .and_then(|o| o.as_ref())
            .map(|s| s.len())
            .unwrap_or(0);
        let resp_head = resp
            .as_ref()
            .ok()
            .and_then(|o| o.as_ref())
            .map(|s| s.chars().take(48).collect::<String>())
            .unwrap_or_default();
        let _ = tx.send(Some(format!(
            "elapsed_ms={} req_len={} send={:?} resp_len={} resp_head={:?}",
            elapsed.as_millis(),
            req.len(),
            send_res.is_ok(),
            resp_len,
            resp_head,
        )));
    });

    match rx.recv() {
        Ok(Some(line)) => println!("{line}"),
        _ => eprintln!("客户端未拿到响应"),
    }
    Ok(())
}

// ---------------------------------------------------------------- 共用

fn parse_relay(s: &str) -> Result<RelayUrl> {
    RelayUrl::from_str(s).with_context(|| format!("解析 relay url {s}"))
}

/// 建一个只连指定 relay 的 endpoint。
///
/// `relay_only = true` 时用 `clear_ip_transports()` **移除所有 IP 传输**,
/// 端点因此没有直连路径,只能走 relay —— 这是稳定公开 API,用来强制测 relay 路径。
async fn build_endpoint(relay_url: RelayUrl, relay_only: bool) -> Result<Endpoint> {
    let relay_cfg = iroh_relay::RelayConfig::new(relay_url, None);
    let relay_map = RelayMap::from_iter([relay_cfg]);
    let mut builder = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Custom(relay_map))
        // ALPN 必须两端都注册:connect 侧传 ALPN,accept 侧要声明支持它,
        // 否则握手报 "peer doesn't support any known protocol"。
        .alpns(vec![ALPN.to_vec()]);
    if relay_only {
        builder = builder.clear_ip_transports();
    }
    let endpoint = builder.bind().await.context("bind endpoint 失败")?;
    Ok(endpoint)
}

/// **rdog 行协议的服务端循环**:读一行,回一行(等长)。
fn serve_lines(bridge: &mut LineBridge) -> Result<()> {
    while let Some(line) = bridge.read_message()? {
        let len = line.len();
        // 回一个等长的 `@response {ok} len=<n>` 行 —— 顺带测大响应方向。
        let head = format!("@response {{ok}} len={len} ");
        let mut resp = String::with_capacity(len);
        resp.push_str(&head);
        resp.extend(std::iter::repeat_n('y', len.saturating_sub(head.len())));
        bridge.write_message(&resp)?;
    }
    Ok(())
}
