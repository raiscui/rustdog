use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use iroh::{
    endpoint::{presets, Connection, ConnectionError, PortmapperConfig, RecvStream, SendStream},
    Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr,
};
use iroh_relay::{server, tls::CaTlsConfig, RelayConfig, RelayMap, RelayQuicConfig};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    runtime::{Builder, Handle, Runtime},
    task::JoinSet,
    time::timeout,
};

use crate::{
    bulk_path::{BulkDecision, PathCapability, PathClass},
    control_core::execute_ping,
    control_protocol::{parse_control_line, ControlCommand, ControlParseResult, ControlRequest},
    control_session::ControlPeerSession,
    control_transport::ControlTransport,
};

pub(crate) mod admission;

#[cfg(all(test, target_os = "linux"))]
mod fixtures;

#[cfg(all(test, target_os = "linux"))]
mod lifecycle_tests;

#[cfg(test)]
mod profile_tests;

#[cfg(test)]
mod registry_tests;

#[cfg(test)]
mod frame_tests;

#[cfg(test)]
mod bulk_tests;

#[cfg(test)]
mod readonly_tests {
    #[test]
    fn default_iroh_gate_admits_only_ping() {
        assert!(super::readonly_request("@ping").is_ok());
        for line in ["@window-find", "@capabilities", "ls -la"] {
            let err = super::readonly_request(line).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{line}");
            assert_eq!(err.to_string(), "iroh_readonly_ping_required", "{line}");
        }
        let open = crate::pty_control::render_pty_open_line(&["python3".into()], 80, 24).unwrap();
        let err = super::readonly_request(&open).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(err.to_string(), "iroh_readonly_ping_required");
    }

    #[test]
    fn default_iroh_gate_refuses_bulk_with_the_path_pin_code() {
        // 默认 lane 没有租约与主体判定 (裁定 F), 所以 bulk 只走准入 lane。这里的拒码是
        // **能力事实**: 换一条承载 bulk 的通道, 而不是"这条策略不许"。
        for line in [
            r#"@put-file:{"transfer_id":"t","path":"/tmp/a","size":1,"resume":false}"#,
            r#"@get-file:{"transfer_id":"t","path":"/tmp/a"}"#,
            r#"@file-chunk:{"transfer_id":"t","offset":0,"data":""}"#,
            r#"@file-end:{"transfer_id":"t","sha256":""}"#,
            r#"@file-read:{"transfer_id":"t","offset":0,"length":1}"#,
            r#"@file-cancel:{"transfer_id":"t"}"#,
            "@file-list",
        ] {
            let err = super::readonly_request(line).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{line}");
            assert_eq!(
                err.to_string(),
                crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE,
                "{line}"
            );
        }
    }

    #[test]
    fn the_default_lane_declares_that_it_neither_pins_nor_checks() {
        // 拒绝码必须由声明查表得出, 不是手写字符串: 把声明改成"逐 chunk 检查", 这条
        // lane 就会开始按路径类别判定 —— 那是另一件事, 得有租约面才谈得上 (裁定 F)。
        let declared = super::DEFAULT_LANE_PATH_CAPABILITY;
        assert!(declared.path_aware);
        assert!(!declared.bulk_path_pin);
        assert!(!declared.bulk_path_checked_per_chunk);
        assert_eq!(
            super::default_lane_bulk_refusal(),
            crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE
        );
    }

    #[test]
    fn one_shot_entry_still_refuses_bulk() {
        // `run_control` 发一行、只认 pong: 一次 bulk 尝试要 open / chunk / end 多行,
        // 这条入口给不了, 所以它连 `@put-file` 都不收。
        let err = super::ping_request(
            r#"@put-file:{"transfer_id":"t","path":"/tmp/a","size":1,"resume":false}"#,
        )
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(err.to_string(), "iroh_readonly_ping_required");
        assert!(super::ping_request("@ping").is_ok());
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
/// The CA roots and relay mode an endpoint dials its relay with.
type RelayTransport = (CaTlsConfig, RelayMode);
const DIRECTORY_ALPN: &[u8] = b"rdog/iroh-directory/1";
const CONTROL_ALPN: &[u8] = b"rdog/iroh-readonly/1";
const IO: Duration = Duration::from_secs(3);
const CONNECT: Duration = Duration::from_secs(7);
// A relay candidate brings its home connection up first, so it needs a budget
// that covers that hop as well as the connection made through it.
const RELAY_CONNECT: Duration = Duration::from_secs(20);
// The directory hop sits on the lease's critical path: a daemon registers and
// then renews every `RENEW`, so a directory call that ran longer than
// `LEASE - RENEW` would let the lease expire while it was still in flight and
// deny every peer in the meantime. It is deliberately shorter than `RELAY_CONNECT`.
const DIRECTORY_CONNECT: Duration = Duration::from_secs(8);
const LEASE: Duration = Duration::from_secs(15);
const RENEW: Duration = Duration::from_secs(5);
const MAX_DIRECTORY: usize = 2048;
const MAX_SESSIONS: usize = 16;
const DENIED: &str = "iroh_target_denied";
const DENIED_CODE: u32 = 43;
/// This endpoint carries a directory *client*: a peer asking it for a lookup is
/// refused with its own code, not a target denial it would misread.
const UNSERVED: &str = "iroh_directory_alpn_unserved";
const UNSERVED_CODE: u32 = 44;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RendezvousConfig {
    key_file: PathBuf,
    bind: SocketAddr,
    relay_http: SocketAddr,
    relay_https: SocketAddr,
    qad: SocketAddr,
    relay_cert: PathBuf,
    relay_key: PathBuf,
    registrations: HashMap<String, EndpointId>,
    lookups: Vec<EndpointId>,
    #[serde(default)]
    routable: bool,
    /// Address handed to peers in the relay URL when it differs from the bound
    /// one, which is the case whenever the public address sits on a NAT device
    /// (a 1:1 NAT EIP cannot be bound locally at all).
    #[serde(default)]
    advertise: Option<std::net::IpAddr>,
    /// Publish the directory endpoint on this relay, so a peer that cannot reach
    /// `bind` still has an address to dial it through.
    #[serde(default)]
    directory_relay: Option<DirectoryRelay>,
    /// The relay a registration must carry when its data path uses a relay other
    /// than the one this rendezvous advertises — a daemon behind NAT whose lane
    /// runs over a public relay, for instance. Omitted, the advertised relay is
    /// the one registration has to match.
    #[serde(default)]
    registry_relay: Option<RelayUrl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    id: EndpointId,
    #[serde(default)]
    addr: Option<SocketAddr>,
    /// Reach the directory through `relay` instead of dialling `addr`. A host
    /// behind NAT has no inbound address to publish, so the relay is the only
    /// address a peer can hand out for it.
    #[serde(default)]
    via_relay: bool,
    #[serde(default)]
    relay: Option<DirectoryRelay>,
}

/// The relay that carries directory traffic. `root_cert` names the CA chain that
/// verifies it and is required: nothing here falls back to public WebPKI, so a
/// public relay needs its root bundle handed in as `root_cert`.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct DirectoryRelay {
    url: RelayUrl,
    /// The CA chain that verifies this relay. It is required, not optional: naming
    /// the anchor is what lets a shared endpoint accept public WebPKI roots
    /// alongside the lane's pinned chain, so trust is never widened by merely
    /// flipping `via_relay` on.
    root_cert: PathBuf,
}

/// Where a directory is dialled, and the relay that carries the dial when the
/// directory has no address of its own.
#[derive(Clone)]
struct DirectoryDial {
    addr: EndpointAddr,
    relay: Option<DirectoryRelay>,
}

impl Directory {
    fn dial(&self, routable: bool) -> Result<DirectoryDial> {
        match (self.via_relay, &self.relay, self.addr) {
            (true, Some(_), Some(_)) => Err(
                "iroh directory profile names both `addr` and `via_relay`; dial one of them".into(),
            ),
            (true, Some(relay), None) => Ok(DirectoryDial {
                addr: EndpointAddr::from_parts(self.id, [TransportAddr::Relay(relay.url.clone())]),
                relay: Some(DirectoryRelay {
                    url: relay.url.clone(),
                    root_cert: relay.root_cert.clone(),
                }),
            }),
            (true, None, _) => Err("iroh directory `via_relay` requires a `relay`".into()),
            (false, Some(_), _) => {
                Err("iroh directory `relay` is only read with `via_relay`".into())
            }
            (false, None, Some(addr)) => Ok(DirectoryDial {
                addr: EndpointAddr::from_parts(
                    self.id,
                    [TransportAddr::Ip(network(addr, routable)?)],
                ),
                relay: None,
            }),
            (false, None, None) => {
                Err("iroh directory profile requires `addr`, or `via_relay` with `relay`".into())
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relay {
    url: RelayUrl,
    qad_port: u16,
    root_cert: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DaemonConfig {
    key_file: PathBuf,
    directory: Directory,
    relay: Relay,
    name: String,
    allow: Vec<EndpointId>,
    #[serde(default)]
    routable: bool,
    #[serde(default)]
    bind: Option<SocketAddr>,
    #[serde(default)]
    policy_file: Option<PathBuf>,
    #[serde(default)]
    frame_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientConfig {
    key_file: PathBuf,
    directory: Directory,
    relay: Relay,
    path: Route,
    #[serde(default)]
    routable: bool,
    #[serde(default)]
    bind: Option<SocketAddr>,
    #[serde(default)]
    lease_ttl_ms: Option<u64>,
    #[serde(default)]
    frame_timeout_ms: Option<u64>,
}

impl DaemonConfig {
    fn frame_timeout(&self) -> Duration {
        self.frame_timeout_ms.map_or(IO, Duration::from_millis)
    }

    fn check_frame_budget(&self) -> Result<()> {
        frame_budget(self.frame_timeout_ms).map(|_| ())
    }
}

impl ClientConfig {
    fn frame_timeout(&self) -> Duration {
        self.frame_timeout_ms.map_or(IO, Duration::from_millis)
    }

    fn check_frame_budget(&self) -> Result<()> {
        frame_budget(self.frame_timeout_ms).map(|_| ())
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Route {
    Direct,
    Relay,
    /// Try a direct path first and fall back to the configured relay; the path
    /// that actually carried the session is what the CLI reports.
    Auto,
}

impl Route {
    fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
            Self::Auto => "auto",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
enum DirectoryRequest {
    Register { name: String, addr: EndpointAddr },
    Lookup { name: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "status", rename_all = "snake_case")]
enum DirectoryReply {
    Registered,
    Found { addr: EndpointAddr },
    Rejected { reason: String },
}

fn profile<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("iroh profile exceeds 64 KiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid iroh JSON profile".into())
}

fn identity(path: &Path) -> Result<SecretKey> {
    let mut bytes = Vec::new();
    File::open(path)?.take(33).read_to_end(&mut bytes)?;
    let fixed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "invalid iroh identity material")?;
    Ok(SecretKey::from_bytes(&fixed))
}

fn loopback(addr: SocketAddr) -> Result<SocketAddr> {
    if !addr.ip().is_loopback() || !addr.is_ipv4() {
        return Err("P1 iroh requires IPv4 loopback".into());
    }
    Ok(addr)
}

fn network(addr: SocketAddr, routable: bool) -> Result<SocketAddr> {
    if !routable {
        return loopback(addr);
    }
    let unusable = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast(),
        std::net::IpAddr::V6(ip) => ip.is_unspecified() || ip.is_multicast(),
    };
    if unusable {
        return Err("iroh routable profile requires an explicit address".into());
    }
    Ok(addr)
}

/// A relay URL plus its QAD port. The explicit-port rule is not a security property:
/// it exists because a self-hosted relay listens on a random port, so the profile
/// has to say which one. A public relay listens on the default 443, and `RelayUrl`
/// normalizes that away, so demanding an explicit port there would refuse the only
/// spelling a standard public relay can have. HTTPS and a non-zero QAD port stay
/// required in both profiles, and the loopback profile keeps the explicit port.
fn relay_allowed(url: &RelayUrl, qad_port: u16, routable: bool) -> Result<()> {
    let https = url.scheme() == "https" && qad_port != 0;
    if !routable {
        if !https || url.port().is_none() || url.host_str() != Some("127.0.0.1") {
            return Err("P1 iroh requires an explicit HTTPS loopback relay and QAD port".into());
        }
        return Ok(());
    }
    if !https || url.host_str().is_none() {
        return Err(
            "iroh routable profile requires an HTTPS relay host and a non-zero QAD port".into(),
        );
    }
    Ok(())
}

/// A candidate a remote peer can dial. Routable profiles never publish loopback:
/// a peer dialling `127.0.0.1` only reaches itself, so the attempt is wasted and
/// the failure it produces misleads whoever reads the log.
fn candidate_ok(addr: SocketAddr, routable: bool) -> Result<SocketAddr> {
    let addr = network(addr, routable)?;
    if routable && addr.ip().is_loopback() {
        return Err("iroh routable profile never publishes a loopback candidate".into());
    }
    Ok(addr)
}

/// Binds may name the wildcard address under a routable profile: the socket is
/// this machine's own and is never published, so it can follow whatever address
/// the machine currently has. Published addresses still go through `network`,
/// which keeps a registration's candidates real.
fn bindable(addr: SocketAddr, routable: bool) -> Result<SocketAddr> {
    if routable && addr.ip().is_unspecified() {
        return Ok(addr);
    }
    network(addr, routable)
}

/// Resolve a profile's `bind`. Under a routable profile an omitted bind — or the
/// explicit `0.0.0.0:0` — means the wildcard socket, so a machine that changes
/// networks does not need a new profile. A wildcard with a fixed port stays
/// refused instead of quietly dropping the port, and the loopback profile keeps
/// its P1 loopback socket.
fn bind_addr(bind: Option<SocketAddr>, routable: bool) -> Result<SocketAddr> {
    let wildcard = bind.map_or(true, |addr| addr.ip().is_unspecified() && addr.port() == 0);
    if routable && wildcard {
        return Ok(SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            0,
        ));
    }
    network(
        bind.unwrap_or_else(|| "127.0.0.1:0".parse().unwrap()),
        routable,
    )
}

fn name_valid(name: &str) -> bool {
    let Some((namespace, daemon)) = name.split_once('/') else {
        return false;
    };
    [namespace, daemon].into_iter().all(|part| {
        (1..=64).contains(&part.len())
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    })
}

/// 未开准入策略的默认 lane 的请求门禁: 只收 `@ping`。
///
/// bulk 只走**准入** lane (daemon profile 给了 `policy_file`) —— 那条路径上有租约与主体
/// 判定, 不带策略的入口没有。所以这里对 bulk 回的是**能力码**(由本 lane 的能力声明查表
/// 得出, 见 `default_lane_bulk_refusal`), 而不是通用的"只许 ping": 前者告诉调用方换一条
/// 承载 bulk 的通道, 后者只说这条不许。
fn readonly_request(line: &str) -> io::Result<ControlRequest> {
    match parse_control_line(line)? {
        ControlParseResult::Control(request) if matches!(request.command, ControlCommand::Ping) => {
            Ok(request)
        }
        ControlParseResult::Control(request)
            if crate::control_core::is_file_transfer_command(&request.command) =>
        {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                default_lane_bulk_refusal(),
            ))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "iroh_readonly_ping_required",
        )),
    }
}

/// 一次性入口 (`run_control`, 客户端发一行、只认 `pong`) 的请求门禁。
///
/// 这条入口只发一行, 而一次 bulk 尝试是 open / chunk / end 多行, 所以它连 `@put-file`
/// 都不收。它和 `readonly_request` 的区别不是能力, 是"这条入口只问一次"。
fn ping_request(line: &str) -> io::Result<ControlRequest> {
    match parse_control_line(line)? {
        ControlParseResult::Control(request) if matches!(request.command, ControlCommand::Ping) => {
            Ok(request)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "iroh_readonly_ping_required",
        )),
    }
}

/// 默认 lane 的能力声明: 有 relay 跳, **既钉不住也不逐 chunk 检查** ⇒ 不承载 bulk。
///
/// 这是适配器声明, 不是策略开关: 不带 `policy_file` 的入口没有租约与主体判定, 它就没有
/// 让 bulk 字节上线的授权面。
const DEFAULT_LANE_PATH_CAPABILITY: PathCapability = PathCapability::PATH_AWARE_UNPINNED;

/// 准入 lane 的能力声明: 有 relay 跳, 传输层**钉不住**, 应用层在 open 与每个 chunk 边界
/// 各检查一次 (`BulkPathGate`)。
///
/// 刻意**不是** `PATH_AWARE_PINNED`: `conn.paths()` 是事后快照, 它禁不了 QUIC 的路径迁移,
/// 也看不见排队/重传里改走 relay 的字节 (§3.5 的两条 pin 义务)。把逐 chunk 检查登记成 pin,
/// 就是拿"每个边界上都直连"去顶替"边界之间不会改路"。真做传输层 pin 另开票。
const ADMITTED_LANE_PATH_CAPABILITY: PathCapability =
    PathCapability::PATH_AWARE_CHECKED_PER_CHUNK;

/// 默认 lane 拒绝 bulk 的码: 查自己声明的能力, 不手写字符串。
fn default_lane_bulk_refusal() -> &'static str {
    match crate::bulk_path::decide(DEFAULT_LANE_PATH_CAPABILITY, PathClass::Unknown, false) {
        BulkDecision::Refused(code) => code,
        // 走到这里说明判定表把"既不钉也不查"改成了放行 —— 但这条 lane 仍然不承载 bulk:
        // 拒绝依据是**授权面**(没有租约与主体判定), 不是判定表结果。今天两者恰好同码。
        BulkDecision::Allowed => crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE,
    }
}

/// 读路径的有界重试: 握手刚完成时路径可能还没就绪, **一次**读不出不等于"问不到类别"。
///
/// 与客户端 `await_route` 同形 (20 × 50ms); 重试穷尽才 fall back `Unknown` (fail-closed)。
const PATH_SAMPLE_ATTEMPTS: usize = 20;
const PATH_SAMPLE_STEP: Duration = Duration::from_millis(50);

/// 门禁读路径的采样面: 生产上读这条 iroh 连接**此刻**选中的路径。
///
/// `Script` 只给测试用 —— 回环拓扑上 `route` 只会给出 `direct`, 另外三种读数
/// (relay / 读不出 / 先 direct 后掉 relay) 在回环里造不出来, 而它们正是门禁的
/// 主体。把读数做成可按顺序钉住的, 而不是把判定逻辑抄第二份进测试。
enum PathSampler<'a> {
    Lane { conn: &'a Connection, routable: bool },
    #[cfg(test)]
    Script {
        readings: Mutex<Vec<PathClass>>,
        /// 每次 `relay_read()` 取一个读数; `None` = 这一读"读不到" (用来钉住"未采样不是零")。
        relay: Mutex<Vec<Option<RelayReading>>>,
    },
}

/// 本端 iroh 连接上**被归类为 relay** 的那些 path 的累计收发字节 (§4.1/§4.2)。
///
/// `relay_paths` 是采样时在场的 relay path 标识。括号采样用它做一件必须做的事: `PathList` 只保留
/// **未关闭**的 path, 一次在 `before` 时在场、`after` 前关掉的 relay path, 它的字节已经从快照里
/// 消失 —— 那种读数是"少算了", 与前一次读数相减会得到一个假的零。所以在场集合对不上时整份读数作废
/// (`None`), 不是拿剩下的 path 凑一个数。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RelayReading {
    tx: u64,
    rx: u64,
    /// 在场 relay path 的对端地址。用地址而不是 `PathId`, 因为地址是快照里能印出来给人看的、
    /// 也是测试能构造的那个 (PathId 的构造函数不在公开 API 里)。
    relay_paths: Vec<String>,
}

impl PathSampler<'_> {
    fn sample(&self) -> PathClass {
        match self {
            Self::Lane { conn, routable } => {
                for attempt in 0..PATH_SAMPLE_ATTEMPTS {
                    if let Ok(observed) = route(conn, *routable) {
                        match observed {
                            Route::Direct => return PathClass::Direct,
                            Route::Relay => return PathClass::Relay,
                            // `route` 从不返回 auto; 若哪天返回, 按读不出处理。
                            Route::Auto => break,
                        }
                    }
                    if attempt + 1 < PATH_SAMPLE_ATTEMPTS {
                        std::thread::sleep(PATH_SAMPLE_STEP);
                    }
                }
                PathClass::Unknown
            }
            // 按顺序取读数; 用完最后一个就一直用它 (单读数的用例写一个元素即可)。
            #[cfg(test)]
            Self::Script { readings, .. } => {
                let mut readings = readings.lock().expect("path readings poisoned");
                if readings.len() > 1 {
                    readings.remove(0)
                } else {
                    readings[0]
                }
            }
        }
    }

    /// 本端 relay 累计字节的**一次**读数 (§4.2 的括号端点)。
    ///
    /// `None` 是"读不到", 不是 0: 生产侧在读不到或在场集合对不上时给 `None` (见 `RelayReading`),
    /// 测试侧靠 `Script` 显式钉 `None`。四个计数最终是 `Option<u64>`, 所以这条区分能一路带到
    /// 快照与 JSON —— 中间任何一层把它当 0 处理, `relay_bytes_zero` 就会在"没接仪表"时成立。
    fn relay_read(&self) -> Option<RelayReading> {
        match self {
            Self::Lane { conn, .. } => {
                // `PathList` 是调用时刻的快照; 枚举本身不会失败, 失败的是"读不到任何 path"
                // (连接刚关/还没选路), 那种情况没有可用的字节读数。
                let paths = conn.paths();
                let mut reading = RelayReading {
                    tx: 0,
                    rx: 0,
                    relay_paths: Vec::new(),
                };
                let mut seen_any = false;
                for path in paths.iter() {
                    seen_any = true;
                    if !path.is_relay() {
                        continue;
                    }
                    let stats = path.stats();
                    reading.tx += stats.udp_tx.bytes;
                    reading.rx += stats.udp_rx.bytes;
                    reading.relay_paths.push(path.remote_addr().to_string());
                }
                // 一条 path 都没有: 连接此刻什么都测不到, 不是"relay 上没有字节"。
                seen_any.then_some(reading)
            }
            #[cfg(test)]
            Self::Script { relay, .. } => {
                let mut readings = relay.lock().expect("relay readings poisoned");
                if readings.is_empty() {
                    return None;
                }
                if readings.len() > 1 {
                    readings.remove(0)
                } else {
                    readings[0].clone()
                }
            }
        }
    }

    /// 此刻被选中的那条 path 的对端地址 —— 只在它是直连 (IP) 时给出。
    ///
    /// 证据里的 `direct_addr` 只回答"当时直连到哪个地址", 不参与任何判定; 读不到就是 `None`,
    /// 不影响 `path_class` (那一格由门禁的采样决定)。
    fn selected_direct_addr(&self) -> Option<String> {
        match self {
            Self::Lane { conn, .. } => conn
                .paths()
                .iter()
                .find(|path| path.is_selected())
                .map(|path| path.remote_addr().clone())
                .filter(|address| matches!(address, TransportAddr::Ip(_)))
                .map(|address| address.to_string()),
            #[cfg(test)]
            Self::Script { .. } => None,
        }
    }
}

#[cfg(test)]
impl PathSampler<'static> {
    /// 同时钉住路径读数与 relay 读数: 括号的两端各取走一个 `relay` 元素。
    fn script_with_relay(readings: Vec<PathClass>, relay: Vec<Option<RelayReading>>) -> Self {
        Self::Script {
            readings: Mutex::new(readings),
            relay: Mutex::new(relay),
        }
    }
}

#[cfg(test)]
impl RelayReading {
    /// 一次 relay 累计读数 (不含具体 relay path 地址): 括号相减的用例只需要这两个数。
    fn at(tx: u64, rx: u64) -> Self {
        Self {
            tx,
            rx,
            relay_paths: Vec::new(),
        }
    }
}

/// 这条请求是本次尝试的开头, 还是它的后续一步。
#[derive(Clone, Copy, PartialEq, Eq)]
enum BulkStep {
    Open,
    Continue,
}

/// bulk 的逐 chunk 检查门禁 (§3.1): open 与每个 chunk 边界各采样、各判一次。
///
/// `attempts` 记的是**每条在途传输是否曾观测到 direct**, 不是"当前是否 direct" ——
/// 只有前者能把"中途掉 relay" (`iroh_direct_path_lost`) 与"从来没有直连"
/// (`iroh_direct_path_unavailable`) 分成两个码。按键而不是单槽, 是因为同一条连接上
/// 可以交错两条传输 (A 的 chunk、B 的 chunk), 单槽会让 B 的 open 把 A 的"曾直连"清掉。
///
/// **这层不做传输层 pin** (`bulk_path_pin = false`): 边界之间的排队与重传它看不到,
/// 也禁不了 QUIC 的路径迁移 (§3.5)。
struct BulkPathGate {
    capability: PathCapability,
    attempts: HashMap<String, bool>,
}

impl BulkPathGate {
    fn new(capability: PathCapability) -> Self {
        Self {
            capability,
            attempts: HashMap::new(),
        }
    }

    /// 一次采样 → 一次判定。
    ///
    /// open **无条件**把这条传输复位: 同 id 的幂等重试是**新的一次尝试**, 继承上一次的
    /// "曾直连"会把 `unavailable` 误判成 `lost`。判定为拒时把条目移掉 —— 一次失败的尝试
    /// 不该留下可供下一次继承的状态。
    fn admit(&mut self, step: BulkStep, attempt: Option<&str>, observed: PathClass) -> BulkDecision {
        let mut seen = match (step, attempt) {
            (BulkStep::Open, Some(id)) => {
                self.attempts.insert(id.to_owned(), false);
                false
            }
            (_, Some(id)) => *self.attempts.entry(id.to_owned()).or_insert(false),
            // 没有 transfer_id 就分不清是哪一次尝试: 按"未观测到直连"判 (fail-closed),
            // 执行器随后按它自己的规则拒这条载荷。
            (_, None) => false,
        };
        if observed == PathClass::Direct {
            seen = true;
        }
        if let Some(id) = attempt {
            self.attempts.insert(id.to_owned(), seen);
        }
        let decision = crate::bulk_path::decide(self.capability, observed, seen);
        if decision != BulkDecision::Allowed {
            if let Some(id) = attempt {
                self.attempts.remove(id);
            }
        }
        decision
    }
}

/// 一条 bulk 请求过不过路径门禁; `Some(code)` = 拒, 码取自判定表。
///
/// `@file-list` / `@file-cancel` 不搬字节, 因此不过路径门禁: 拒一条 cancel 只会
/// 把半成品留在盘上, 换不来任何"字节没走 relay"的保证。
///
/// 注意门禁的粒度: 上传方向的 chunk 载荷内联在请求行里, daemon 读到 `transfer_id` 之前
/// 那一 chunk 的字节**已经过线** (at most one chunk in flight)。"上传方向线上一字节为零"
/// 只能由发送端的 pre-send 门禁保证, 不在这一层。
fn gate_bulk(
    gate: &mut BulkPathGate,
    sampler: &PathSampler,
    command: &ControlCommand,
) -> GateVerdict {
    use crate::control_core::FileTransferVerb;

    let (step, transfer_id) = match crate::control_core::file_transfer_verb(command) {
        Some(FileTransferVerb::PutFile(payload))
        | Some(FileTransferVerb::GetFile(payload)) => {
            (BulkStep::Open, payload_transfer_id(payload))
        }
        Some(
            FileTransferVerb::Chunk(payload)
            | FileTransferVerb::End(payload)
            | FileTransferVerb::Read(payload),
        ) => (BulkStep::Continue, payload_transfer_id(payload)),
        // 这两个动词不搬字节 (也不进括号), `None` 的动词同理由执行器兜。
        _ => return GateVerdict::Ungated,
    };
    let observed = sampler.sample();
    let decision = gate.admit(step, transfer_id.as_deref(), observed);
    GateVerdict::Gated {
        observed,
        refusal: match decision {
            BulkDecision::Allowed => None,
            BulkDecision::Refused(code) => Some(code),
        },
    }
}

/// 门禁对这条请求的结论, 连同它读到的那次采样。
///
/// 采样要**一次**、两处用: 门禁拿它判, 证据 (daemon 半快照) 拿它记 `path_class`。各读一次
/// 会留下"拒了但同时记成 direct"这种自相矛盾的可能。
enum GateVerdict {
    Gated {
        observed: PathClass,
        refusal: Option<&'static str>,
    },
    /// 不搬字节 / 不是 bulk 动词: 既不过门禁, 也不进证据。
    Ungated,
}

/// 载荷里的 `transfer_id` —— 一次尝试的身份。
///
/// 载荷本身不合法时读不到, 于是 attempt 为 `None`: 门禁照做, 执行器随后按它自己的规则
/// 拒这条请求。这里不替执行器判载荷合法性。
fn payload_transfer_id(payload: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()?
        .get("transfer_id")?
        .as_str()
        .map(str::to_owned)
}

/// daemon 一侧一次 bulk 尝试的括号采样与字节记账 (§4.2/§4.3)。
///
/// 括号的两端是这条连接上的 relay 累计量: open **执行前**取 `before`, 终态请求**执行后**取
/// `after`。中间每一步只累加"被 daemon 确认过"的字节 —— 记账在发送之前会把从未落盘的字节
/// 算进去, 两端 join 就对不上账 (§4.3)。
struct AttemptEvidence {
    /// 两端快照对齐用的身份 (§4.3): 客户端在 open 载荷里交来的那个, 缺省时由 daemon 派生。
    attempt_id: String,
    direction: &'static str,
    relay_before: Option<RelayReading>,
    /// 每次边界采样后的类别, **最后一次赢了**: 门禁只在全 direct 时放行, 所以能走到终态的
    /// 尝试这一格就是 `direct`; 中途掉 relay 的那次会被拒, 快照按 `aborted` 记。
    path_class: PathClass,
    direct_addr: Option<String>,
    bulk_bytes: u64,
    wire_bytes: u64,
    chunks: u64,
}

/// 本端 (daemon) 的 bulk 证据表, 键是 `transfer_id`。
///
/// 与 `BulkPathGate` 分开: 门禁的问题是"这条请求能不能过", 证据的问题是"这次尝试搬了多少字节、
/// 两端对不对得上"。合成一个结构会让"拒了"同时意味着"证据没了" —— 而拒绝本身正是最该留证据的时刻。
struct DaemonPathEvidence {
    capability: PathCapability,
    attempts: HashMap<String, AttemptEvidence>,
    /// 每个 `transfer_id` 被 open 过几次 —— 只用来派生缺省的 `attempt_id` (§2.2)。
    opens: HashMap<String, u64>,
}

impl DaemonPathEvidence {
    fn new(capability: PathCapability) -> Self {
        Self {
            capability,
            attempts: HashMap::new(),
            opens: HashMap::new(),
        }
    }

    /// open 执行**前**的基线。同 id 重开是一次新尝试, 旧基线必须被覆盖 (§4.2)。
    fn begin(
        &mut self,
        transfer_id: &str,
        attempt_id: Option<&str>,
        direction: &'static str,
        sampler: &PathSampler,
    ) {
        let seq = self.opens.entry(transfer_id.to_owned()).or_insert(0);
        *seq += 1;
        let attempt_id = attempt_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{transfer_id}#{seq}"));
        self.attempts.insert(
            transfer_id.to_owned(),
            AttemptEvidence {
                attempt_id,
                direction,
                relay_before: sampler.relay_read(),
                path_class: PathClass::Unknown,
                direct_addr: None,
                bulk_bytes: 0,
                wire_bytes: 0,
                chunks: 0,
            },
        );
    }

    /// 记下这一次边界的采样结果。
    fn observe(&mut self, transfer_id: &str, observed: PathClass, sampler: &PathSampler) {
        let Some(attempt) = self.attempts.get_mut(transfer_id) else {
            return;
        };
        attempt.path_class = observed;
        if observed == PathClass::Direct && attempt.direct_addr.is_none() {
            attempt.direct_addr = sampler.selected_direct_addr();
        }
    }

    /// 记一块**已被 daemon 确认**的字节。
    fn note_confirmed(&mut self, transfer_id: &str, bulk: u64, wire: u64) {
        if let Some(attempt) = self.attempts.get_mut(transfer_id) {
            attempt.bulk_bytes += bulk;
            attempt.wire_bytes += wire;
            attempt.chunks += 1;
        }
    }

    /// 终态: 取 `after`、给这份尝试收尾, 返回要挂进响应信的 `evidence` 子对象。
    ///
    /// `aborted` 只由"这条尝试没有成功走完"决定, 不由调用方感觉决定。
    fn finalize(
        &mut self,
        transfer_id: &str,
        sampler: &PathSampler,
        aborted: bool,
    ) -> Option<serde_json::Value> {
        let attempt = self.attempts.remove(transfer_id)?;
        let after = sampler.relay_read();
        // 在 `before` 时在场的 relay path, `after` 时必须还在: 少了一条就说明它中途关了,
        // 它的关闭字节不在快照里 —— 相减会得到一个假的零。宁可交白卷 (§4.3 的 `null`)。
        let readable = match (&attempt.relay_before, &after) {
            (Some(before), Some(after)) => before
                .relay_paths
                .iter()
                .all(|id| after.relay_paths.contains(id)),
            _ => false,
        };
        let (tx_before, tx_after, rx_before, rx_after) = if readable {
            match (&attempt.relay_before, &after) {
                (Some(before), Some(after)) => (
                    Some(before.tx),
                    Some(after.tx),
                    Some(before.rx),
                    Some(after.rx),
                ),
                _ => (None, None, None, None),
            }
        } else {
            (None, None, None, None)
        };
        let snapshot = crate::bulk_path::BulkPathSnapshot {
            attempt_id: attempt.attempt_id,
            transfer_id: transfer_id.to_owned(),
            direction: attempt.direction.to_owned(),
            capability: self.capability,
            path_class: attempt.path_class,
            direct_addr: attempt.direct_addr,
            bulk_bytes: attempt.bulk_bytes,
            wire_bytes: attempt.wire_bytes,
            chunks: attempt.chunks,
            // daemon 也不检测同连接上的并发流量: `null`, 不是 `false` (§4.3)。
            concurrency_suspected: None,
            endpoint: "daemon".to_owned(),
            relay_tx_before: tx_before,
            relay_tx_after: tx_after,
            relay_rx_before: rx_before,
            relay_rx_after: rx_after,
            aborted,
        };
        Some(snapshot.to_json())
    }
}

/// 载荷里客户端交来的 `attempt_id` (§2.2 的可选字段)。
///
/// 不合法 / 缺省都给 `None`: daemon 随后按 `transfer_id#序号` 派生一个。这里不替执行器判载荷。
fn payload_attempt_id(payload: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()?
        .get("attempt_id")?
        .as_str()
        .map(str::to_owned)
}

/// base64 串解码后的字节数 —— 不真解码, 只按长度与填充算。
///
/// `bulk_bytes` 记的是载荷字节, `wire_bytes` 记的是压上控制行的 base64 字节 (§4.3), 两者都要。
fn decoded_base64_len(encoded: &str) -> u64 {
    let padding = encoded
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count() as u64;
    (encoded.len() as u64 / 4).saturating_mul(3).saturating_sub(padding)
}

/// 响应里的载荷对象: 带 id 的信封取 `value`, 错误信与裸对象取它自己。
fn response_payload(
    outcome: &crate::control_frames::ControlExecutionOutcome,
) -> Option<serde_json::Value> {
    use crate::control_frames::ControlFrame;

    let [ControlFrame::ResponseLine(line)] = outcome.outbound_frames.as_slice() else {
        return None;
    };
    let value: serde_json::Value = serde_json::from_str(line.strip_prefix("@response ")?).ok()?;
    match value.get("value") {
        Some(inner @ serde_json::Value::Object(_)) => Some(inner.clone()),
        _ => match value {
            serde_json::Value::Object(_) => Some(value),
            _ => None,
        },
    }
}

/// 一条 bulk 请求的完整处理: 门禁 → 执行 → 记账 → 终态时把 daemon 半快照挂进响应。
///
/// 半快照**不新开传输**: 它落在已有的终态响应信封里 (§4.3)。上传的终态是 `@file-end`
/// (成功或失败), 下载的终态是 `eof:true` 的那一帧 `@file-read`, 中止的终态是 `@file-cancel`
/// 与门禁拒绝。
fn serve_bulk_request(
    transport: &mut ControlTransport,
    request_id: Option<u64>,
    command: &ControlCommand,
    sampler: &PathSampler,
    gate: &mut BulkPathGate,
    evidence: &mut DaemonPathEvidence,
) -> io::Result<()> {
    use crate::control_core::FileTransferVerb;

    let verb = crate::control_core::file_transfer_verb(command);
    let transfer_id = verb
        .as_ref()
        .and_then(verb_payload)
        .and_then(payload_transfer_id);

    // 基线必须先于执行器: open 一旦建会话/碰盘, 这次尝试就已经开始了 (§4.2)。
    if let (Some(verb), Some(id)) = (verb.as_ref(), transfer_id.as_deref()) {
        match verb {
            FileTransferVerb::PutFile(payload) => {
                evidence.begin(id, payload_attempt_id(payload).as_deref(), "put", sampler);
            }
            FileTransferVerb::GetFile(payload) => {
                evidence.begin(id, payload_attempt_id(payload).as_deref(), "get", sampler);
            }
            _ => {}
        }
    }

    match gate_bulk(gate, sampler, command) {
        GateVerdict::Gated {
            observed,
            refusal: None,
        } => {
            if let Some(id) = transfer_id.as_deref() {
                evidence.observe(id, observed, sampler);
            }
        }
        GateVerdict::Gated {
            observed,
            refusal: Some(code),
        } => {
            log::info!("iroh bulk refused code={code}");
            let mut outcome = crate::control_frames::ControlExecutionOutcome::from_response_line(
                crate::control_core::render_protocol_error_response(
                    request_id,
                    LANE_REFUSED_CODE,
                    code,
                ),
            );
            if let Some(id) = transfer_id.as_deref() {
                evidence.observe(id, observed, sampler);
                // 被拒的尝试同样要留证据: 这次没搬字节, 但"那一刻读到的是 relay / 读不出"
                // 正是结论本身。按 `aborted` 记 —— 它不是结论 (§4.3)。
                if let Some(evidence_json) = evidence.finalize(id, sampler, true) {
                    outcome = splice_evidence(outcome, evidence_json);
                }
            }
            ControlPeerSession::new("iroh-admitted").dispatch_outcome(outcome, transport)?;
            return Ok(());
        }
        GateVerdict::Ungated => {}
    }

    let Some(outcome) = crate::control_core::execute_file_transfer_request(request_id, command)
    else {
        return refuse_frame(transport, request_id, LANE_REFUSED_CODE, LANE_REFUSED);
    };

    let payload = response_payload(&outcome);
    let failed = payload
        .as_ref()
        .map(|value| value.get("error").is_some() || value.get("code").is_some())
        .unwrap_or(true);

    let mut terminal: Option<serde_json::Value> = None;
    if let (Some(verb), Some(id)) = (verb.as_ref(), transfer_id.as_deref()) {
        match verb {
            // 上传: 一块被 daemon 确认后才入账 —— 记账在发送之前会把从未落盘的字节算进去。
            FileTransferVerb::Chunk(payload) => {
                if !failed {
                    let data = serde_json::from_str::<serde_json::Value>(payload)
                        .ok()
                        .and_then(|value| {
                            value.get("data").and_then(serde_json::Value::as_str).map(str::to_owned)
                        });
                    if let Some(data) = data {
                        evidence.note_confirmed(id, decoded_base64_len(&data), data.len() as u64);
                    }
                }
            }
            // 下载: 字节由 daemon 发出, 应答里带的就是它这次真送出去的量。
            FileTransferVerb::Read(_) => {
                if !failed {
                    let length = payload
                        .as_ref()
                        .and_then(|value| value.get("length"))
                        .and_then(serde_json::Value::as_u64);
                    let data = payload
                        .as_ref()
                        .and_then(|value| value.get("data"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    if let (Some(length), Some(data)) = (length, data) {
                        evidence.note_confirmed(id, length, data.len() as u64);
                    }
                    let eof = payload
                        .as_ref()
                        .and_then(|value| value.get("eof"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    if eof {
                        terminal = evidence.finalize(id, sampler, false);
                    }
                }
            }
            FileTransferVerb::End(_) => {
                terminal = evidence.finalize(id, sampler, failed);
            }
            FileTransferVerb::Cancel(_) => {
                terminal = evidence.finalize(id, sampler, true);
            }
            _ => {}
        }
    }

    let outcome = match terminal {
        Some(evidence_json) => splice_evidence(outcome, evidence_json),
        None => outcome,
    };
    ControlPeerSession::new("iroh-admitted").dispatch_outcome(outcome, transport)?;
    Ok(())
}

/// 动词的请求载荷 (不含 `@file-list`, 它没有载荷)。
fn verb_payload<'a>(verb: &crate::control_core::FileTransferVerb<'a>) -> Option<&'a str> {
    use crate::control_core::FileTransferVerb;

    match verb {
        FileTransferVerb::PutFile(payload)
        | FileTransferVerb::GetFile(payload)
        | FileTransferVerb::Chunk(payload)
        | FileTransferVerb::End(payload)
        | FileTransferVerb::Read(payload)
        | FileTransferVerb::Cancel(payload) => Some(payload),
        FileTransferVerb::List => None,
    }
}

/// 把 `evidence` 挂进这个 outcome 唯一那条 `@response` 的对象里。
///
/// 不改传输、不改协议: daemon 半快照就落在已有的终态响应信封里 (§4.3)。解析不出对象时原样返回 ——
/// 证据是旁路产物, 不该让一条已经成立的响应因为"我加不上字段"而变成失败; 但也不假装挂上了。
fn splice_evidence(
    outcome: crate::control_frames::ControlExecutionOutcome,
    evidence: serde_json::Value,
) -> crate::control_frames::ControlExecutionOutcome {
    use crate::control_frames::{ControlExecutionOutcome, ControlFrame};

    let [ControlFrame::ResponseLine(line)] = outcome.outbound_frames.as_slice() else {
        return outcome;
    };
    let Some(body) = line.strip_prefix("@response ") else {
        return outcome;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return outcome;
    };
    // 带 id 的响应是 `{"id":N,"value":{...}}`: 载荷在 `value` 里。错误信是裸对象, 载荷就是它自己。
    let target = match value.get_mut("value") {
        Some(serde_json::Value::Object(payload)) => payload,
        Some(_) => return outcome,
        None => match &mut value {
            serde_json::Value::Object(payload) => payload,
            _ => return outcome,
        },
    };
    target.insert("evidence".to_owned(), evidence);
    ControlExecutionOutcome::from_response_line(format!("@response {value}"))
}

fn certificates(path: &Path) -> io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    rustls_pemfile::certs(&mut BufReader::new(File::open(path)?)).collect()
}

fn runtime() -> io::Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

async fn endpoint(
    key: SecretKey,
    bind: Option<SocketAddr>,
    alpn: &[u8],
    relay: Option<&Relay>,
) -> Result<Endpoint> {
    endpoint_with_network(key, bind, alpn, relay, false).await
}

async fn endpoint_with_network(
    key: SecretKey,
    bind: Option<SocketAddr>,
    alpn: &[u8],
    relay: Option<&Relay>,
    routable: bool,
) -> Result<Endpoint> {
    let transport = match relay {
        Some(relay) => Some(lane_relay(relay, routable)?),
        None => None,
    };
    endpoint_on(key, bind, &[alpn], transport, routable).await
}

/// The lane's own relay, which this process hosts or dials as a full relay: it
/// carries data, so it keeps QUIC address discovery on.
fn lane_relay(relay: &Relay, routable: bool) -> Result<RelayTransport> {
    relay_allowed(&relay.url, relay.qad_port, routable)?;
    Ok((
        RelayRoots::Lane(relay.root_cert.clone()).config()?,
        RelayMode::Custom(RelayMap::from_iter([RelayConfig::new(
            relay.url.clone(),
            Some(RelayQuicConfig::new(relay.qad_port)),
        )])),
    ))
}

fn directory_url_ok(url: &RelayUrl, routable: bool) -> Result<()> {
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("iroh directory relay requires an explicit HTTPS relay".into());
    }
    if !routable && url.host_str() != Some("127.0.0.1") {
        return Err("P1 iroh requires an explicit HTTPS loopback relay".into());
    }
    Ok(())
}

/// A directory relay is only dialled, so it needs no QAD hop: turning discovery
/// off keeps the directory request on the relay instead of letting it drift onto
/// a direct path, which is the path a NAT'd directory does not have.
fn directory_relay(relay: &DirectoryRelay, routable: bool) -> Result<RelayTransport> {
    directory_url_ok(&relay.url, routable)?;
    Ok((
        relay_roots(None, relay).config()?,
        RelayMode::Custom(RelayMap::from_iter([RelayConfig::new(
            relay.url.clone(),
            None,
        )])),
    ))
}

/// Which roots verify one endpoint's relays. A directory relay is trusted only
/// once the operator names its CA chain, and that named anchor is what allows an
/// endpoint shared with the lane to keep accepting public WebPKI roots beside the
/// lane's pinned chain — a public hop cannot be verified by a private anchor.
/// Widening therefore hangs on the explicit `directory.relay.root_cert`, never on
/// `via_relay` alone.
#[derive(Debug, PartialEq, Eq)]
enum RelayRoots {
    /// The lane's own chain, and nothing else.
    Lane(PathBuf),
    /// Only this chain, for a directory endpoint that dials one relay and nothing
    /// else.
    Pinned(PathBuf),
    /// The lane's pinned chain plus the directory relay's named chain. Still
    /// pinned: no endpoint ever falls back to public WebPKI, so a public
    /// directory relay needs the operator to hand in that root bundle as
    /// `root_cert` instead of trusting every public CA.
    LaneAndDirectory(PathBuf, PathBuf),
}

/// The one place that decides what an endpoint trusts: a directory-only endpoint
/// pins the chain the profile named, while an endpoint that also carries the lane
/// pins the two chains together. Neither ever falls back to public WebPKI.
fn relay_roots(lane: Option<&Relay>, directory: &DirectoryRelay) -> RelayRoots {
    match lane {
        Some(lane) => {
            RelayRoots::LaneAndDirectory(lane.root_cert.clone(), directory.root_cert.clone())
        }
        None => RelayRoots::Pinned(directory.root_cert.clone()),
    }
}

impl RelayRoots {
    fn config(&self) -> io::Result<CaTlsConfig> {
        Ok(match self {
            Self::Lane(chain) | Self::Pinned(chain) => {
                CaTlsConfig::custom_roots(certificates(chain)?)
            }
            Self::LaneAndDirectory(lane, directory) => {
                CaTlsConfig::custom_roots(certificates(lane)?)
                    .with_extra_roots(certificates(directory)?)
            }
        })
    }
}

/// A directory and the lane live on one endpoint once the directory is relay
/// carried: a peer identity holds one connection per relay, so a second endpoint
/// of the same peer on the same relay would evict the first one every few
/// seconds. That endpoint keeps both relays in one map and one root set.
fn shared_relay(
    lane: &Relay,
    directory: &DirectoryRelay,
    routable: bool,
) -> Result<RelayTransport> {
    relay_allowed(&lane.url, lane.qad_port, routable)?;
    directory_url_ok(&directory.url, routable)?;
    let mut map = RelayMap::from_iter([RelayConfig::new(
        lane.url.clone(),
        Some(RelayQuicConfig::new(lane.qad_port)),
    )]);
    if directory.url != lane.url {
        map.insert(
            directory.url.clone(),
            Arc::new(RelayConfig::new(directory.url.clone(), None)),
        );
    }
    Ok((
        relay_roots(Some(lane), directory).config()?,
        RelayMode::Custom(map),
    ))
}

async fn endpoint_on(
    key: SecretKey,
    bind: Option<SocketAddr>,
    alpns: &[&[u8]],
    transport: Option<RelayTransport>,
    routable: bool,
) -> Result<Endpoint> {
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .alpns(alpns.iter().map(|alpn| alpn.to_vec()).collect())
        .clear_ip_transports()
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .relay_mode(RelayMode::Disabled);
    if let Some(bind) = bind {
        builder = builder.bind_addr(bindable(bind, routable)?)?;
    }
    if let Some((roots, mode)) = transport {
        builder = builder.ca_tls_config(roots).relay_mode(mode);
    }
    Ok(builder.bind().await?)
}

async fn directory_endpoint(
    key: SecretKey,
    bind: Option<SocketAddr>,
    relay: Option<&DirectoryRelay>,
    routable: bool,
) -> Result<Endpoint> {
    let transport = match relay {
        Some(relay) => Some(directory_relay(relay, routable)?),
        None => None,
    };
    endpoint_on(key, bind, &[DIRECTORY_ALPN], transport, routable).await
}

async fn frame_get<T: DeserializeOwned>(read: &mut RecvStream) -> Result<T> {
    let bytes = timeout(IO, read.read_to_end(MAX_DIRECTORY)).await??;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn frame_put<T: Serialize>(write: &mut SendStream, value: &T) -> Result<()> {
    let buf = serde_json::to_vec(value)?;
    if buf.len() > MAX_DIRECTORY {
        return Err("iroh directory frame too large".into());
    }
    timeout(IO, write.write_all(&buf)).await??;
    write.finish()?;
    Ok(())
}

/// A renewal only has whatever lease time is left: an attempt that starts late must
/// not outlive the deadline it refreshes, or the daemon denies every peer for the
/// difference. A healthy renewal keeps the full directory budget.
fn renewal_budget(remaining: Duration) -> Duration {
    DIRECTORY_CONNECT.min(remaining)
}

async fn directory_call(
    ep: &Endpoint,
    dial: &DirectoryDial,
    request: DirectoryRequest,
    routable: bool,
    budget: Duration,
) -> Result<DirectoryReply> {
    let budget = budget.min(match dial.relay {
        Some(_) => DIRECTORY_CONNECT,
        None => CONNECT,
    });
    // One call touches several steps (connect, open, write, read), and a daemon
    // renews every `RENEW` against a `LEASE`: bounding the call as a whole is what
    // keeps it inside the lease, instead of adding up the steps and hoping each one
    // stops short of its own budget.
    let call = async {
        let conn = timeout(budget, ep.connect(dial.addr.clone(), DIRECTORY_ALPN)).await??;
        let (mut send, mut read) = timeout(IO, conn.open_bi()).await??;
        frame_put(&mut send, &request).await?;
        let reply = frame_get(&mut read).await?;
        // The lane logs `iroh_path=` for its own dial; the directory hop reports
        // what carried it too — but only once the reply is in hand, because a path
        // printed ahead of the result would leave a `relay` reading behind for a
        // lookup that then failed.
        if dial.relay.is_some() {
            match route(&conn, routable) {
                Ok(actual) => log_directory_path(actual),
                Err(err) => log::debug!("iroh_directory_path=unknown ({err})"),
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply)
    };
    match timeout(budget, call).await?? {
        DirectoryReply::Rejected { reason } => Err(reason.into()),
        reply => Ok(reply),
    }
}

/// What carried the last successful directory hop. A `via_relay` daemon renews
/// every five seconds, so the path is logged at `info` only when it changes and at
/// `debug` otherwise: a steady daemon must not add a line per renewal.
static DIRECTORY_PATH: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn log_directory_path(actual: Route) {
    let seen = match actual {
        Route::Relay => 1,
        _ => 2,
    };
    let previous = DIRECTORY_PATH.swap(seen, Ordering::Relaxed);
    if previous == seen {
        log::debug!("iroh_directory_path={}", actual.label());
    } else {
        log::info!("iroh_directory_path={}", actual.label());
    }
}

struct Registry {
    owners: HashMap<String, EndpointId>,
    readers: Vec<EndpointId>,
    leases: HashMap<String, (EndpointAddr, Instant)>,
    relay: RelayUrl,
    routable: bool,
}

impl Registry {
    fn request(&mut self, peer: EndpointId, request: DirectoryRequest) -> DirectoryReply {
        let reject = |reason: &str| DirectoryReply::Rejected {
            reason: reason.into(),
        };
        match request {
            DirectoryRequest::Register { name, addr } => {
                // One name, one owner, one relay; the IP candidates are whatever
                // interfaces the daemon currently has. A wildcard bind advertises
                // the enumerated addresses, never a literal unspecified one, and
                // the container is a set, so a repeat cannot survive either.
                let ip_candidates = addr.ip_addrs().count();
                if self.owners.get(&name) != Some(&peer)
                    || addr.id != peer
                    || ip_candidates == 0
                    || addr
                        .ip_addrs()
                        .any(|a| a.port() == 0 || candidate_ok(*a, self.routable).is_err())
                    || addr.relay_urls().count() != 1
                    || addr.relay_urls().next() != Some(&self.relay)
                {
                    // `iroh_registration_denied` covers five causes, so a pairing
                    // mistake across two profiles is unreadable without both sides.
                    log::info!(
                        "iroh registration denied name={name} expected_relay={} carried_relay={}",
                        self.relay,
                        addr.relay_urls()
                            .map(|url| url.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    return reject("iroh_registration_denied");
                }
                log::info!(
                    "iroh directory registered name={name} candidates={}",
                    addr.ip_addrs()
                        .map(|ip| ip.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                self.leases.insert(name, (addr, Instant::now() + LEASE));
                DirectoryReply::Registered
            }
            DirectoryRequest::Lookup { name } => {
                if !self.readers.contains(&peer) {
                    return reject("iroh_lookup_denied");
                }
                match self.leases.get(&name) {
                    Some((addr, expires)) if *expires > Instant::now() => {
                        DirectoryReply::Found { addr: addr.clone() }
                    }
                    _ => reject("iroh_target_not_registered"),
                }
            }
        }
    }
}

#[derive(Debug)]
struct RelayAccess(Vec<EndpointId>);

impl server::AccessControl for RelayAccess {
    async fn on_connect(&self, request: &server::ClientRequest) -> server::Access {
        if self.0.contains(&request.endpoint_id()) {
            server::Access::Allow
        } else {
            server::Access::Deny { reason: None }
        }
    }
}

pub(crate) fn run_rendezvous(path: &Path) -> Result<()> {
    let cfg: RendezvousConfig = profile(path)?;
    if cfg.registrations.is_empty() || !cfg.registrations.keys().all(|name| name_valid(name)) {
        return Err("iroh registration configuration requires NAMESPACE/NAME keys".into());
    }
    let key = identity(&cfg.key_file)?;
    let relay_key = rustls_pemfile::private_key(&mut BufReader::new(File::open(&cfg.relay_key)?))?
        .ok_or("missing P1 relay TLS key")?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certificates(&cfg.relay_cert)?, relay_key)?;
    // The directory endpoint joins this relay like any other peer, so a profile
    // that publishes it there has to let it in as well.
    let mut admitted: Vec<EndpointId> = cfg
        .lookups
        .iter()
        .chain(cfg.registrations.values())
        .copied()
        .collect();
    if cfg.directory_relay.is_some() {
        admitted.push(key.public());
    }
    let mut relay_config = server::RelayConfig::new(network(cfg.relay_http, cfg.routable)?);
    relay_config.access = Arc::new(RelayAccess(admitted));
    relay_config.tls = Some(server::TlsConfig::new(
        network(cfg.relay_https, cfg.routable)?,
        server::CertConfig::Manual { server_config: tls },
    ));
    let mut config = server::ServerConfig::default();
    config.relay = Some(relay_config);
    config.quic = Some(server::QuicConfig::new(network(cfg.qad, cfg.routable)?));
    runtime()?.block_on(async {
        let directory = directory_endpoint(
            key,
            Some(network(cfg.bind, cfg.routable)?),
            cfg.directory_relay.as_ref(),
            cfg.routable,
        )
        .await?;
        let relay = server::Server::spawn(config).await?;
        let tls_addr = relay.https_addr().ok_or("missing P1 HTTPS relay")?;
        let qad = relay.quic_addr().ok_or("missing P1 QAD listener")?;
        let advertised = match cfg.advertise {
            Some(ip) => SocketAddr::new(ip, tls_addr.port()),
            None => tls_addr,
        };
        let expected_relay = match cfg.registry_relay.clone() {
            Some(url) => {
                directory_url_ok(&url, cfg.routable)?;
                url
            }
            None => format!("https://{advertised}").parse()?,
        };
        let registry = Arc::new(Mutex::new(Registry {
            owners: cfg.registrations, readers: cfg.lookups, leases: HashMap::new(),
            relay: expected_relay, routable: cfg.routable,
        }));
        let registry_relay = cfg
            .registry_relay
            .as_ref()
            .map(|url| format!(" registry_relay={url}"))
            .unwrap_or_default();
        match &cfg.directory_relay {
            Some(relay) => log::info!(
                "iroh rendezvous ready relay_https={tls_addr} advertised={advertised} qad={qad} directory_relay={}{registry_relay}",
                relay.url
            ),
            None => log::info!(
                "iroh rendezvous ready relay_https={tls_addr} advertised={advertised} qad={qad}{registry_relay}"
            ),
        }
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                incoming = directory.accept(), if tasks.len() < MAX_SESSIONS => {
                    let incoming = incoming.ok_or("iroh directory stopped")?;
                    let registry = registry.clone();
                    tasks.spawn(async move {
                        let outcome = async {
                            let conn = timeout(IO, incoming).await??;
                            let (mut send, mut read) = timeout(IO, conn.accept_bi()).await??;
                            let request = frame_get(&mut read).await?;
                            let reply = registry.lock().map_err(|_| "iroh directory lock failed")?.request(conn.remote_id(), request);
                            frame_put(&mut send, &reply).await?;
                            timeout(IO, send.stopped()).await??;
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                        }.await;
                        if outcome.is_err() { log::debug!("iroh directory request ended without reply"); }
                    });
                }
            }
        }
    })
}

async fn register(
    ep: &Endpoint,
    dial: &DirectoryDial,
    cfg: &DaemonConfig,
    data: &Endpoint,
    budget: Duration,
) -> Result<Instant> {
    // Every interface the endpoint actually has, not just the first bound socket:
    // a wildcard bind covers several, and a peer may not reach the first one.
    let mut addrs: Vec<TransportAddr> = data
        .addr()
        .ip_addrs()
        .filter(|ip| ip.port() != 0 && candidate_ok(**ip, cfg.routable).is_ok())
        .map(|ip| TransportAddr::Ip(*ip))
        .collect();
    if addrs.is_empty() {
        return Err("missing daemon IP candidate".into());
    }
    addrs.push(TransportAddr::Relay(cfg.relay.url.clone()));
    let addr = EndpointAddr::from_parts(data.id(), addrs);
    match directory_call(
        ep,
        dial,
        DirectoryRequest::Register {
            name: cfg.name.clone(),
            addr,
        },
        cfg.routable,
        budget,
    )
    .await?
    {
        DirectoryReply::Registered => Ok(Instant::now() + LEASE),
        _ => Err("unexpected iroh directory registration response".into()),
    }
}

async fn await_route(conn: &Connection, routable: bool) -> Result<Route> {
    for _ in 0..20 {
        if let Ok(selected) = route(conn, routable) {
            return Ok(selected);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    route(conn, routable)
}

fn route(conn: &Connection, routable: bool) -> Result<Route> {
    let paths = conn.paths();
    let observed = paths.iter().find(|path| path.is_selected());
    let observed = observed.as_ref().map(|path| path.remote_addr());
    match observed {
        Some(TransportAddr::Relay(_)) => Ok(Route::Relay),
        Some(TransportAddr::Ip(ip)) if network(*ip, routable).is_ok() => Ok(Route::Direct),
        Some(TransportAddr::Ip(_)) if routable => Err("iroh has no usable path".into()),
        _ => Err("iroh has no usable loopback path".into()),
    }
}

/// Operator commands for the local admission policy: who may attach, and how to
/// take that away again. Everything is written through the same durable save the
/// daemon uses, so a command that returns has the marker on disk.
pub(crate) fn run_policy(action: crate::input::PolicyAction, path: &Path) -> Result<()> {
    use crate::input::PolicyAction;

    let cfg: DaemonConfig = profile(path)?;
    let policy_path = cfg
        .policy_file
        .ok_or("iroh daemon profile names no policy_file")?;
    let _locked = PolicyLock::acquire(&policy_path)?;
    let now = utc_ms();
    let mut policy: admission::Policy = match File::open(&policy_path) {
        Ok(file) => serde_json::from_reader(file)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => admission::Policy::default(),
        Err(err) => return Err(err.into()),
    };

    let subject = |subject: Option<String>, key: Option<PathBuf>| -> Result<EndpointId> {
        match (subject, key) {
            (Some(hex), None) => hex
                .parse()
                .map_err(|_| "subject is not an endpoint id".into()),
            (None, Some(key)) => Ok(identity(&key)?.public()),
            _ => Err("give exactly one of --subject or --subject-key".into()),
        }
    };

    match action {
        PolicyAction::List => {
            println!(
                "policy {} (allow={} revoked={} terminate={})",
                policy_path.display(),
                policy.allow.len(),
                policy.revoked.len(),
                policy.terminate.len()
            );
            for id in &policy.allow {
                let party = policy.parties.iter().find(|party| party.id == *id);
                let note = party.map(|party| party.note.as_str()).unwrap_or("");
                let registered = party
                    .map(|party| party.registered_ms)
                    .filter(|ms| *ms > 0)
                    .map(|ms| iso_ms(ms))
                    .unwrap_or_else(|| "未核".to_owned());
                println!(
                    "allow {id} fingerprint={} registered={registered} note={note}{}{}",
                    fingerprint(id),
                    if policy.revoked.contains(id) {
                        " revoked"
                    } else {
                        ""
                    },
                    if policy.terminate.contains(id) {
                        " terminate"
                    } else {
                        ""
                    }
                );
            }
            for id in &policy.revoked {
                if !policy.allow.contains(id) {
                    println!("revoked {id} fingerprint={}", fingerprint(id));
                }
            }
        }
        PolicyAction::Admit {
            subject: hex,
            subject_key,
            note,
        } => {
            let id = subject(hex, subject_key)?;
            if policy.revoked.contains(&id) || policy.terminate.contains(&id) {
                return Err(format!(
                    "{id} is still revoked on disk; clear its revocation before admitting it again"
                )
                .into());
            }
            policy.allow.insert(id);
            policy.note(id, &note, now);
            save(&policy_path, &policy)?;
            println!("admitted {id} fingerprint={}", fingerprint(&id));
        }
        PolicyAction::Revoke {
            subject: hex,
            subject_key,
            note,
        } => {
            let id = subject(hex, subject_key)?;
            policy.allow.insert(id);
            policy.revoked.insert(id);
            policy.note(id, &note, now);
            save(&policy_path, &policy)?;
            println!(
                "revoked {id}: new sessions and renewals are refused; live sessions run to their lease end"
            );
        }
        PolicyAction::Emergency {
            subject: hex,
            subject_key,
            note,
        } => {
            let id = subject(hex, subject_key)?;
            policy.allow.insert(id);
            policy.revoked.insert(id);
            policy.terminate.insert(id);
            policy.note(id, &note, now);
            save(&policy_path, &policy)?;
            println!("emergency {id}: revoked on disk, live sessions are terminated");
        }
    }
    Ok(())
}

/// Holds the policy file's lock for the whole read, change and save, so two
/// operators cannot overwrite each other's markers with a stale snapshot.
struct PolicyLock(#[allow(dead_code)] File);

#[cfg(unix)]
impl PolicyLock {
    fn acquire(policy_path: &Path) -> io::Result<Self> {
        let mut lock = policy_path.as_os_str().to_owned();
        lock.push(".lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(Path::new(&lock))?;
        // SAFETY: flock(2) on a descriptor this process owns; the lock is released
        // when the file closes.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

#[cfg(not(unix))]
impl PolicyLock {
    /// Windows has no `flock(2)` on this lane yet. The lock file is still created
    /// so an operator sees the path, but the read-modify-write is not serialised
    /// on this platform; the daemon's own saves stay single-writer because they
    /// run inside one process.
    fn acquire(policy_path: &Path) -> io::Result<Self> {
        let mut lock = policy_path.as_os_str().to_owned();
        lock.push(".lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(Path::new(&lock))?;
        Ok(Self(file))
    }
}

fn save(path: &Path, policy: &admission::Policy) -> Result<()> {
    admission::save_policy(path, policy).map_err(
        |err| -> Box<dyn std::error::Error + Send + Sync> {
            match err {
                admission::PersistenceError::NotWritten(err) => {
                    format!("the policy was not written: {err}").into()
                }
                admission::PersistenceError::DurabilityUnconfirmed(err) => {
                    format!("the policy was replaced but its durability is unconfirmed: {err}")
                        .into()
                }
            }
        },
    )
}

fn fingerprint(id: &EndpointId) -> String {
    let digest = <sha2::Sha256 as sha2::Digest>::digest(id.as_bytes());
    hex_prefix(&digest, 16)
}

fn hex_prefix(bytes: &[u8], take: usize) -> String {
    bytes
        .iter()
        .take(take)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn iso_ms(ms: u64) -> String {
    let seconds = ms / 1000;
    let (days, rem) = (seconds / 86_400, seconds % 86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Days since the epoch, civil from day number (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(crate) fn run_daemon(path: &Path) -> Result<()> {
    let cfg: DaemonConfig = profile(path)?;
    cfg.check_frame_budget()?;
    let frame_timeout = cfg.frame_timeout();
    if !name_valid(&cfg.name) || cfg.allow.is_empty() {
        return Err("iroh daemon requires a name and an explicit ping allowlist".into());
    }
    let key = identity(&cfg.key_file)?;
    let bind = bind_addr(cfg.bind, cfg.routable)?;
    runtime()?.block_on(async {
        let dial = cfg.directory.dial(cfg.routable)?;
        // A relay-carried directory shares the serving endpoint, so the daemon
        // keeps one relay connection per relay instead of two that evict each
        // other. Without the switch the two endpoints stay exactly as they were.
        let (directory, data) = match dial.relay.as_ref() {
            Some(relay) => {
                let served = endpoint_on(
                    key,
                    Some(bind),
                    &[CONTROL_ALPN, DIRECTORY_ALPN],
                    Some(shared_relay(&cfg.relay, relay, cfg.routable)?),
                    cfg.routable,
                )
                .await?;
                (served.clone(), served)
            }
            None => {
                let directory =
                    directory_endpoint(key.clone(), Some(bind), None, cfg.routable).await?;
                let data = endpoint_with_network(
                    key,
                    Some(bind),
                    CONTROL_ALPN,
                    Some(&cfg.relay),
                    cfg.routable,
                )
                .await?;
                (directory, data)
            }
        };
        // `online()` means "at least one relay's home connection is up", not "the
        // relay this lane publishes is up": with a directory relay in the map that
        // can be the other one. A successful registration proves the directory hop
        // is reachable; whether the lane relay carries this daemon is a separate
        // question, answered by that relay's own connection state.
        timeout(CONNECT, data.online()).await?;
        let deadline = register(&directory, &dial, &cfg, &data, DIRECTORY_CONNECT).await?;
        let allow = Arc::new(cfg.allow.clone());
        let routable = cfg.routable;
        let admission = match &cfg.policy_file {
            Some(path) => Some(Arc::new(Mutex::new(admission::Admission::new(
                path.clone(),
                data.id(),
            )))),
            None => None,
        };
        if let Some(state) = admission.clone() {
            tokio::spawn(lease_sweeper(state));
        }
        let expires = Arc::new(Mutex::new(deadline));
        let renewal_expires = expires.clone();
        let renewal_data = data.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(RENEW).await;
                let remaining = renewal_expires
                    .lock()
                    .map(|when| when.saturating_duration_since(Instant::now()))
                    .unwrap_or(DIRECTORY_CONNECT);
                match register(
                    &directory,
                    &dial,
                    &cfg,
                    &renewal_data,
                    renewal_budget(remaining),
                )
                .await
                {
                    Ok(deadline) => {
                        if let Ok(mut when) = renewal_expires.lock() { *when = deadline; }
                    }
                    Err(err) => log::info!("iroh directory renewal failed: {err}"),
                }
            }
        });
        log::info!(
            "iroh daemon registered pty={}",
            if admission.is_some() {
                "admitted (ping + pty + bulk)"
            } else {
                "ping-only (bulk needs an admitted lane)"
            }
        );
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                incoming = data.accept(), if tasks.len() < MAX_SESSIONS => {
                    let incoming = incoming.ok_or("iroh daemon stopped")?;
                    let allow = allow.clone();
                    let expires = expires.clone();
                    let admission = admission.clone();
                    tasks.spawn(async move {
                        let outcome = async {
                            let conn = timeout(IO, incoming).await??;
                            if conn.alpn() == DIRECTORY_ALPN {
                                // This endpoint carries the directory client, not a
                                // directory server: say so instead of serving a line
                                // the peer's lookup would drift into.
                                log::info!(
                                    "iroh directory alpn not served peer={}",
                                    conn.remote_id()
                                );
                                conn.close(UNSERVED_CODE.into(), UNSERVED.as_bytes());
                                return Ok(());
                            }
                            log::info!("iroh connection accepted peer={}", conn.remote_id());
                            let (send, read) = timeout(IO, conn.accept_bi()).await??;
                            let route = route(&conn, routable);
                            let path = route.as_ref().map(|route| route.label()).unwrap_or("pending");
                            let live = expires.lock().ok().is_some_and(|at| *at > Instant::now());
                            // The admitted lane answers to the policy file: a party the
                            // operator admitted there may not be in the static ping
                            // allowlist, and refusing it here would make `rdog policy
                            // admit` useless until the next daemon restart. Ask the
                            // policy before a task slot is taken, otherwise every
                            // dialling peer would hold one through the handshake.
                            let known = allow.contains(&conn.remote_id())
                                || identity_admitted(&admission, conn.remote_id());
                            if !live || !known {
                                log::info!("iroh admission deny path={path} handler_entries=0");
                                conn.close(DENIED_CODE.into(), DENIED.as_bytes());
                                return Ok(());
                            }
                            let transport = ControlTransport::Iroh(IrohLineTransport::new(
                                send,
                                read,
                                frame_timeout,
                            ));
                            let peer = conn.remote_id();
                            let served = match admission {
                                Some(state) => {
                                    let probe = conn.clone();
                                    tokio::task::spawn_blocking(move || {
                                        let sampler = PathSampler::Lane {
                                            conn: &probe,
                                            routable,
                                        };
                                        let mut gate =
                                            BulkPathGate::new(ADMITTED_LANE_PATH_CAPABILITY);
                                        serve_admitted(
                                            transport, peer, path, state, &sampler, &mut gate,
                                        )
                                    })
                                    .await?
                                }
                                // 默认 lane 不承载 bulk (裁定 F): 没有租约与主体判定的入口上
                                // 不放字节, 也就不需要路径采样。
                                None => tokio::task::spawn_blocking(move || {
                                    serve_ping(transport, path)
                                })
                                .await?,
                            };
                            match served {
                                Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                                    log::info!("iroh admitted lane refused: {err}");
                                    conn.close(DENIED_CODE.into(), DENIED.as_bytes());
                                }
                                Err(err) => {
                                    log::info!("iroh control connection ended without a reply: {err}")
                                }
                                Ok(()) => {
                                    // The peer reads the last frame before it closes
                                    // the connection; closing from this side would
                                    // discard a reply that is still in flight.
                                    let _ = timeout(IO, conn.closed()).await;
                                }
                            }
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                        }.await;
                        if let Err(err) = outcome {
                            log::info!("iroh control connection failed: {err}");
                        }
                    });
                }
            }
        }
    })
}

/// 未开准入策略时的默认 lane: 只答一次 `@ping`。
///
/// bulk 只走准入 lane —— 那条路径上有租约与主体判定; 这个入口没有, 所以它连字节都不收
/// (拒绝码由 `DEFAULT_LANE_PATH_CAPABILITY` 查表得到)。
fn serve_ping(mut transport: ControlTransport, path: &'static str) -> io::Result<()> {
    let line = transport
        .read_message()?
        .ok_or(io::ErrorKind::UnexpectedEof)?;
    let request = readonly_request(&line)?;
    let outcome = execute_ping(request.request_id, "");
    if let Some(id) = request.request_id {
        log::info!("iroh ping dispatch path={path} id={id}");
    }
    ControlPeerSession::new("iroh-readonly").dispatch_outcome(outcome, &mut transport)?;
    transport.close()
}

pub(crate) fn run_control(path: &Path, name: &str, line: &str) -> Result<()> {
    let request = ping_request(line)?;
    if !name_valid(name) {
        return Err("iroh target must be NAMESPACE/NAME".into());
    }
    let cfg: ClientConfig = profile(path)?;
    cfg.check_frame_budget()?;
    let key = identity(&cfg.key_file)?;
    let bind = bind_addr(cfg.bind, cfg.routable)?;
    runtime()?.block_on(async {
        let dial = cfg.directory.dial(cfg.routable)?;
        let directory =
            directory_endpoint(key.clone(), Some(bind), dial.relay.as_ref(), cfg.routable).await?;
        let lookup = directory_call(
            &directory,
            &dial,
            DirectoryRequest::Lookup {
                name: name.to_owned(),
            },
            cfg.routable,
            DIRECTORY_CONNECT,
        )
        .await;
        directory.close().await;
        let DirectoryReply::Found { addr } = lookup? else {
            return Err("unexpected iroh directory lookup response".into());
        };
        let (data, conn, actual) =
            connect_with_path(&key, Some(bind), &addr, cfg.path, &cfg.relay, cfg.routable).await?;
        log::info!("iroh_path={}", actual.label());
        let streams = timeout(IO, conn.open_bi()).await;
        let reply = match streams {
            Ok(Ok((send, read))) => {
                let mut transport =
                    ControlTransport::Iroh(IrohLineTransport::new(send, read, cfg.frame_timeout()));
                let line = line.to_owned();
                tokio::task::spawn_blocking(move || {
                    transport.write_message(&line)?;
                    transport
                        .read_message()?
                        .ok_or(io::ErrorKind::UnexpectedEof.into())
                })
                .await?
            }
            Ok(Err(err)) => Err(io::Error::other(err)),
            Err(err) => Err(io::Error::other(err)),
        };
        // A refusal closes the connection, so the reason can land after the
        // stream reports its end; wait for it briefly before treating the
        // reply as a protocol failure.
        let mut denied = false;
        for _ in 0..20 {
            match conn.close_reason() {
                Some(ConnectionError::ApplicationClosed(close)) => {
                    denied = close.error_code.into_inner() == u64::from(DENIED_CODE)
                        && close.reason.as_ref() == DENIED.as_bytes();
                    break;
                }
                Some(_) => break,
                None => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        data.close().await;
        if denied {
            return Err(DENIED.into());
        }
        let reply = reply?;
        let body: serde_json::Value = serde_json::from_str(
            reply
                .strip_prefix("@response ")
                .ok_or("invalid iroh ping response")?,
        )?;
        let value = if let Some(id) = request.request_id {
            if body["id"].as_u64() != Some(id) {
                return Err("iroh ping response id mismatch".into());
            }
            &body["value"]
        } else {
            &body
        };
        if value["pong"].as_bool() != Some(true) {
            return Err("invalid iroh pong".into());
        }
        println!("{reply}");
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Admitted PTY lane
//
// The formal iroh entry answers ping only unless the daemon profile names an
// admission policy file; bulk rides the admitted lane and nowhere else. When the
// profile does name one, every connection presents a locally
// signed grant, the daemon serves the request through the same PTY runtime the
// other control lanes use, and one sweeper reclaims sessions whose lease ran out
// or whose peer was revoked, with no further request needed.
// ---------------------------------------------------------------------------

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(7);
const LEASE_TICK: Duration = Duration::from_millis(200);
const DEFAULT_LEASE_TTL_MS: u64 = 60_000;
const RENEW_STEP: u32 = 3;
const RENEW_POLL: Duration = Duration::from_millis(100);
const LANE_REFUSED_CODE: i32 = 66;
const LANE_REFUSED: &str = "iroh_admitted_lane_supports_ping_pty_and_bulk_only";
/// A lease holds one PTY session, so a second open has to say so plainly.
const LEASE_BUSY: (i32, &str) = (66, "iroh_lease_session_conflict");
const SUBJECT_REFUSED_CODE: i32 = 43;
const SUBJECT_REFUSED: &str = "iroh_subject_denied";

fn utc_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Handshake {
    Admit { grant: admission::SignedGrant },
    Renew { grant: admission::SignedGrant },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum HandshakeReply {
    Admitted,
    Renewed,
    Refused { code: u32, reason: String },
}

fn handshake_reply(reply: &HandshakeReply) -> io::Result<String> {
    serde_json::to_string(reply).map_err(io::Error::other)
}

fn refuse_frame(
    transport: &mut ControlTransport,
    request_id: Option<u64>,
    code: i32,
    reason: &str,
) -> io::Result<()> {
    let line = crate::control_core::render_protocol_error_response(request_id, code, reason);
    transport.write_message(&line)
}

/// Whether the local policy file names this party, asked without a session.
fn identity_admitted(
    admission: &Option<Arc<Mutex<admission::Admission>>>,
    peer: EndpointId,
) -> bool {
    let Some(state) = admission else {
        return false;
    };
    match state.lock() {
        Ok(state) => state.party_permitted(peer),
        Err(_) => false,
    }
}

fn require_active(
    admission: &Arc<Mutex<admission::Admission>>,
    peer: EndpointId,
    lease: &str,
) -> admission::AdmissionResult<()> {
    admission
        .lock()
        .map_err(|_| admission::Rejection::Denied)?
        .active(peer, lease, utc_ms())
}

fn serve_admitted(
    mut transport: ControlTransport,
    peer: EndpointId,
    path: &'static str,
    admission: Arc<Mutex<admission::Admission>>,
    sampler: &PathSampler,
    gate: &mut BulkPathGate,
) -> io::Result<()> {
    let mut evidence = DaemonPathEvidence::new(gate.capability);
    transport.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    log::info!("iroh admitted lane reading the handshake");
    let Some(first) = transport.read_message()? else {
        log::info!("iroh admitted lane saw end of stream before a handshake");
        return Ok(());
    };
    log::info!("iroh admitted lane handshake bytes={}", first.len());
    let Ok(Handshake::Admit { grant }) = serde_json::from_str::<Handshake>(&first) else {
        // The admitted lane keeps answering the P1 ping one-shot, so the formal
        // CLI's diagnostic path needs no handshake; anything else is refused.
        // The local policy stays the authority: a revoked peer gets no ping.
        let admitted_ping = admission
            .lock()
            .map(|state| state.admits_ping(peer))
            .unwrap_or(false);
        if !admitted_ping {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("iroh ping refused by the local policy: peer={peer}"),
            ));
        }
        // bulk 要过租约 (`require_active`) 与路径门禁, 所以这里只认 ping ——
        // 未握手的首行若放 bulk 进来, 等于绕开租约。
        if let Ok(request) = ping_request(&first) {
            let outcome = execute_ping(request.request_id, "");
            ControlPeerSession::new("iroh-admitted").dispatch_outcome(outcome, &mut transport)?;
            return transport.close();
        }
        let refusal = admission::Rejection::Denied.code_reason();
        let reply = HandshakeReply::Refused {
            code: refusal.0,
            reason: refusal.1.to_owned(),
        };
        transport.write_message(&handshake_reply(&reply)?)?;
        log::info!("iroh handshake requires admit or ping first");
        return transport.close();
    };
    let lease = grant.claims.session.clone();
    match admission
        .lock()
        .map_err(|_| io::Error::other("admission lock poisoned"))?
        .admit(peer, &grant, utc_ms())
    {
        Ok(()) => transport.write_message(&handshake_reply(&HandshakeReply::Admitted)?)?,
        Err(rejection) => {
            let (code, reason) = rejection.code_reason();
            let reply = HandshakeReply::Refused {
                code,
                reason: reason.to_owned(),
            };
            transport.write_message(&handshake_reply(&reply)?)?;
            log::info!("iroh session refused peer={peer} path={path} reason={reason}");
            return transport.close();
        }
    }
    transport.set_read_timeout(None)?;
    log::info!("iroh session admitted peer={peer} path={path}");
    let result = serve_lane(
        &mut transport,
        peer,
        &lease,
        &admission,
        sampler,
        gate,
        &mut evidence,
    );
    result.and(transport.close())
}

/// The lane transport as the PTY bridge sees it.
///
/// The bridge owns the stream for the whole session, so the frames that are not
/// PTY input have to be handled here: a renewal typed into the remote program
/// never renews anything, and a close that names a foreign session must not end
/// this one.
struct LaneIo<'a> {
    transport: &'a mut ControlTransport,
    admission: &'a Arc<Mutex<admission::Admission>>,
    peer: EndpointId,
    subject: &'a str,
    session: &'a str,
}

impl crate::shell::PtyBridgeIo for LaneIo<'_> {
    fn read_message(&mut self) -> io::Result<Option<String>> {
        loop {
            let Some(message) = self.transport.read_message()? else {
                return Ok(None);
            };
            if message.starts_with('{') {
                match serde_json::from_str::<Handshake>(&message) {
                    Ok(Handshake::Renew { grant }) => {
                        let renewed = self
                            .admission
                            .lock()
                            .map_err(|_| io::Error::other("admission lock poisoned"))?
                            .renew(self.peer, &grant, utc_ms());
                        match renewed {
                            Ok(()) => log::info!("iroh lease renewed"),
                            Err(rejection) => {
                                let (code, reason) = rejection.code_reason();
                                log::info!(
                                    "iroh lease renewal refused code={code} reason={reason}"
                                );
                            }
                        }
                        continue;
                    }
                    Ok(_) => {
                        log::info!("iroh admitted lane refused a handshake inside a session");
                        continue;
                    }
                    Err(_) => {}
                }
            }
            if let Some(session_id) = lane_target(&message) {
                if session_id != self.session {
                    // A close for any other session is not this session's input,
                    // and the bridge matches only its own id.
                    log::info!(
                        "iroh lifecycle frame refused: session={session_id} is not this lane's"
                    );
                    continue;
                }
                if !matches!(
                    crate::pty_control::authorize_pty_subject(&session_id, self.subject)?,
                    crate::pty_control::SubjectAccess::Allowed
                ) {
                    log::info!("iroh close refused: foreign session={session_id}");
                    continue;
                }
            }
            return Ok(Some(message));
        }
    }

    fn write_message(&mut self, message: &str) -> io::Result<()> {
        self.transport.write_message(message).map_err(|err| {
            // Any write failure leaves a possibly partial frame behind, which the
            // guard has already made permanent for this stream.
            match err.kind() {
                io::ErrorKind::InvalidData => err,
                _ => io::Error::other(format!("{FRAME_BROKEN}: {err}")),
            }
        })
    }
}

impl crate::shell::PtyBridgeTransport for LaneIo<'_> {
    fn set_bridge_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.transport.set_read_timeout(timeout)
    }
}

/// A frame stream that lost part of a frame cannot carry anything else, so the
/// session ends with that reason instead of being left half open.
fn abort_broken_frame(session: &str, err: &io::Error) -> io::Result<()> {
    if !err.to_string().contains(FRAME_BROKEN) {
        return Ok(());
    }
    log::info!("iroh lane aborted reason={FRAME_BROKEN} session={session}");
    crate::pty_control::close_pty_session_with_reason(session, FRAME_BROKEN)?;
    Ok(())
}

fn lane_target(message: &str) -> Option<String> {
    // Anything that is not a lifecycle command for one session, a resize frame
    // included, has to fall through to the bridge untouched.
    match parse_control_line(message) {
        Ok(ControlParseResult::Control(ControlRequest {
            command: ControlCommand::PtyClose(request),
            ..
        })) => Some(request.session_id),
        Ok(ControlParseResult::Control(ControlRequest {
            command: ControlCommand::PtyDetach(request),
            ..
        })) => Some(request.session_id),
        _ => None,
    }
}

fn serve_lane(
    transport: &mut ControlTransport,
    peer: EndpointId,
    lease: &str,
    admission: &Arc<Mutex<admission::Admission>>,
    sampler: &PathSampler,
    gate: &mut BulkPathGate,
    evidence: &mut DaemonPathEvidence,
) -> io::Result<()> {
    let subject = peer.to_string();
    loop {
        let Some(line) = transport.read_message()? else {
            return Ok(());
        };
        if line.starts_with('{') {
            match serde_json::from_str::<Handshake>(&line) {
                Ok(Handshake::Renew { grant }) => {
                    // Renewal is one-way: the client keeps its session by
                    // renewing, and a refused renewal shows up as the lease
                    // running out rather than as a frame in the PTY stream.
                    match admission
                        .lock()
                        .map_err(|_| io::Error::other("admission lock poisoned"))?
                        .renew(peer, &grant, utc_ms())
                    {
                        Ok(()) => {}
                        Err(rejection) => {
                            let (code, reason) = rejection.code_reason();
                            log::info!("iroh lease renewal refused code={code} reason={reason}");
                        }
                    }
                }
                _ => {
                    refuse_frame(transport, None, LANE_REFUSED_CODE, LANE_REFUSED)?;
                }
            }
            continue;
        }

        if let Some(request) = crate::pty_control::parse_pty_open_request(&line)? {
            if let Err(rejection) = require_active(admission, peer, lease) {
                let (code, reason) = rejection.code_reason();
                refuse_frame(transport, None, code as i32, reason)?;
                continue;
            }
            // A lease holds one session: a detached session stays alive, and a
            // second open on the same lease would put it beyond every reclaim.
            let held = admission
                .lock()
                .map_err(|_| io::Error::other("admission lock poisoned"))?
                .session_of(lease);
            if let Some(held) = held.filter(|held| crate::pty_control::pty_session_registered(held))
            {
                log::info!("iroh pty refused: lease already holds session={held}");
                let (code, reason) = LEASE_BUSY;
                refuse_frame(transport, None, code, reason)?;
                continue;
            }
            let mut session = match crate::pty_control::open_attached_pty_session(request) {
                Ok(session) => session,
                Err(err) => {
                    refuse_frame(transport, None, 64, &err.to_string())?;
                    continue;
                }
            };
            let pty = session.session_id().to_owned();
            crate::pty_control::bind_pty_session_subject(&pty, &subject)?;
            admission
                .lock()
                .map_err(|_| io::Error::other("admission lock poisoned"))?
                .record_session(lease, pty.clone());
            log::info!("iroh pty opened session={pty} lease={lease}");
            let mut lane = LaneIo {
                transport,
                admission,
                peer,
                subject: &subject,
                session: &pty,
            };
            if let Err(err) = crate::shell::bridge_attached_pty_session(&mut lane, &mut session) {
                abort_broken_frame(&pty, &err)?;
                return Err(err);
            }
            continue;
        }

        if let Some(request) = crate::pty_control::parse_pty_attach_request(&line)? {
            if let Err(rejection) = require_active(admission, peer, lease) {
                let (code, reason) = rejection.code_reason();
                refuse_frame(transport, None, code as i32, reason)?;
                continue;
            }
            match crate::pty_control::authorize_pty_subject(&request.session_id, &subject)? {
                crate::pty_control::SubjectAccess::Allowed => {}
                crate::pty_control::SubjectAccess::Foreign => {
                    refuse_frame(transport, None, SUBJECT_REFUSED_CODE, SUBJECT_REFUSED)?;
                    continue;
                }
                crate::pty_control::SubjectAccess::Missing => {
                    refuse_frame(transport, None, 64, "PTY session not found")?;
                    continue;
                }
            }
            // A lease holds one session, and the runtime marks a PTY attached the
            // moment it is claimed. Both checks therefore have to run before the
            // claim, or a refusal would strand the target as attached to a stream
            // that is already gone.
            let held = admission
                .lock()
                .map_err(|_| io::Error::other("admission lock poisoned"))?
                .session_of(lease);
            if let Some(held) = held.filter(|held| {
                *held != request.session_id && crate::pty_control::pty_session_registered(held)
            }) {
                log::info!("iroh pty attach refused: lease already holds session={held}");
                let (code, reason) = LEASE_BUSY;
                refuse_frame(transport, None, code, reason)?;
                continue;
            }
            let Some(mut session) = crate::pty_control::attach_active_pty_session(request)? else {
                refuse_frame(transport, None, 64, "PTY session not found")?;
                continue;
            };
            let attached = session.session_id().to_owned();
            if let Err(rejection) = admission
                .lock()
                .map_err(|_| io::Error::other("admission lock poisoned"))?
                .attach_session(lease, &attached, crate::pty_control::pty_session_registered)
            {
                // Belt and braces: the claim above already marked the session as
                // attached. Release it as a detach, not a disconnect — the target
                // belongs to another lease, so it must survive this refusal.
                let _ = session.detach("attach_refused");
                let (code, reason) = rejection.code_reason();
                refuse_frame(transport, None, code as i32, reason)?;
                continue;
            }
            log::info!("iroh pty reattached session={attached}");
            let mut lane = LaneIo {
                transport,
                admission,
                peer,
                subject: &subject,
                session: &attached,
            };
            if let Err(err) = crate::shell::bridge_attached_pty_session(&mut lane, &mut session) {
                abort_broken_frame(&attached, &err)?;
                return Err(err);
            }
            continue;
        }

        let request = match parse_control_line(&line) {
            Ok(ControlParseResult::Control(request)) => request,
            _ => {
                refuse_frame(transport, None, LANE_REFUSED_CODE, LANE_REFUSED)?;
                continue;
            }
        };
        match request.command {
            ControlCommand::Ping => {
                let outcome = execute_ping(request.request_id, "");
                ControlPeerSession::new("iroh-admitted").dispatch_outcome(outcome, transport)?;
            }
            ControlCommand::PtyClose(close) => {
                if let Err(rejection) = require_active(admission, peer, lease) {
                    let (code, reason) = rejection.code_reason();
                    refuse_frame(transport, request.request_id, code as i32, reason)?;
                    continue;
                }
                match crate::pty_control::authorize_pty_subject(&close.session_id, &subject)? {
                    crate::pty_control::SubjectAccess::Allowed => {
                        let closed = crate::pty_control::close_pty_session_with_reason(
                            &close.session_id,
                            "force_close",
                        )?;
                        if closed {
                            let line = crate::control_core::render_control_response_payload(
                                request.request_id,
                                0,
                                b"",
                                b"",
                            );
                            transport.write_message(&line)?;
                        } else {
                            refuse_frame(transport, None, 64, "PTY session not found")?;
                        }
                    }
                    crate::pty_control::SubjectAccess::Foreign => {
                        refuse_frame(
                            transport,
                            request.request_id,
                            SUBJECT_REFUSED_CODE,
                            SUBJECT_REFUSED,
                        )?;
                    }
                    crate::pty_control::SubjectAccess::Missing => {
                        refuse_frame(transport, request.request_id, 64, "PTY session not found")?;
                    }
                }
            }
            ControlCommand::PtyDetach(detach) => {
                if let Err(rejection) = require_active(admission, peer, lease) {
                    let (code, reason) = rejection.code_reason();
                    refuse_frame(transport, request.request_id, code as i32, reason)?;
                    continue;
                }
                match crate::pty_control::authorize_pty_subject(&detach.session_id, &subject)? {
                    crate::pty_control::SubjectAccess::Allowed => {
                        match crate::pty_control::detach_active_pty_session(&detach.session_id) {
                            Ok(true) => {
                                let line = crate::control_core::render_control_response_payload(
                                    request.request_id,
                                    0,
                                    b"",
                                    b"",
                                );
                                transport.write_message(&line)?;
                            }
                            Ok(false) => refuse_frame(
                                transport,
                                request.request_id,
                                64,
                                "PTY session not found",
                            )?,
                            Err(err) => {
                                refuse_frame(transport, request.request_id, 64, &err.to_string())?
                            }
                        }
                    }
                    crate::pty_control::SubjectAccess::Foreign => {
                        refuse_frame(
                            transport,
                            request.request_id,
                            SUBJECT_REFUSED_CODE,
                            SUBJECT_REFUSED,
                        )?;
                    }
                    crate::pty_control::SubjectAccess::Missing => {
                        refuse_frame(transport, request.request_id, 64, "PTY session not found")?;
                    }
                }
            }
            other if crate::control_core::is_file_transfer_command(&other) => {
                // bulk 与 PTY 一样要过租约: 一条过期/被撤的会话不该还能搬字节。
                if let Err(rejection) = require_active(admission, peer, lease) {
                    let (code, reason) = rejection.code_reason();
                    refuse_frame(transport, request.request_id, code as i32, reason)?;
                    continue;
                }
                serve_bulk_request(
                    transport,
                    request.request_id,
                    &other,
                    sampler,
                    gate,
                    evidence,
                )?;
            }
            _ => refuse_frame(transport, None, LANE_REFUSED_CODE, LANE_REFUSED)?,
        }
    }
}

async fn lease_sweeper(admission: Arc<Mutex<admission::Admission>>) {
    loop {
        tokio::time::sleep(LEASE_TICK).await;
        let work = match admission.lock() {
            Ok(mut state) => state.sweep(utc_ms(), crate::pty_control::pty_session_registered),
            Err(_) => return,
        };
        for termination in work {
            log::info!(
                "iroh lease reclaim session={} reason={}",
                termination.pty,
                termination.reason
            );
            let _ = crate::pty_control::close_pty_session_with_reason(
                &termination.pty,
                termination.reason,
            );
        }
    }
}

fn connect_route(
    data: &Endpoint,
    addr: &EndpointAddr,
    path: Route,
    relay: &Relay,
    routable: bool,
) -> Result<EndpointAddr> {
    let addrs = match path {
        Route::Direct | Route::Auto => {
            let candidates = addr
                .ip_addrs()
                .map(|ip| network(*ip, routable).map(TransportAddr::Ip))
                .collect::<Result<Vec<_>>>()?;
            if candidates.is_empty() {
                return Err("no iroh direct candidate".into());
            }
            candidates
        }
        Route::Relay if addr.relay_urls().any(|url| *url == relay.url) => {
            vec![TransportAddr::Relay(relay.url.clone())]
        }
        Route::Relay => return Err("iroh directory has no configured relay candidate".into()),
    };
    Ok(EndpointAddr::from_parts(addr.id, addrs))
}

async fn connect_once(
    data: &Endpoint,
    addr: &EndpointAddr,
    candidate: Route,
    relay: &Relay,
    routable: bool,
) -> Result<(Connection, Route)> {
    let target = connect_route(data, addr, candidate, relay, routable)?;
    let budget = match candidate {
        Route::Relay => RELAY_CONNECT,
        Route::Direct | Route::Auto => CONNECT,
    };
    let conn = timeout(budget, data.connect(target, CONTROL_ALPN)).await??;
    let actual = await_route(&conn, routable).await?;
    Ok((conn, actual))
}

async fn candidate_endpoint(
    key: &SecretKey,
    bind: Option<SocketAddr>,
    candidate: Route,
    relay: &Relay,
    routable: bool,
) -> Result<Endpoint> {
    match candidate {
        Route::Direct => {
            endpoint_with_network(key.clone(), bind, CONTROL_ALPN, None, routable).await
        }
        Route::Relay | Route::Auto => {
            endpoint_with_network(key.clone(), None, CONTROL_ALPN, Some(relay), routable).await
        }
    }
}

async fn connect_with_path(
    key: &SecretKey,
    bind: Option<SocketAddr>,
    addr: &EndpointAddr,
    path: Route,
    relay: &Relay,
    routable: bool,
) -> Result<(Endpoint, Connection, Route)> {
    let candidates = match path {
        Route::Auto => vec![Route::Direct, Route::Relay],
        single => vec![single],
    };
    let mut last: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for candidate in candidates {
        // Each candidate gets the endpoint its own profile would use: a remote
        // that a stalled attempt already knows keeps retrying that path, so the
        // relay retry has to start from a fresh relay-only endpoint.
        let data = candidate_endpoint(key, bind, candidate, relay, routable).await?;
        match connect_once(&data, addr, candidate, relay, routable).await {
            Ok((conn, actual)) if actual == candidate => return Ok((data, conn, actual)),
            Ok(_) => {
                data.close().await;
                return Err("iroh connection selected a prohibited route".into());
            }
            Err(err) => {
                log::info!("iroh_path={} unavailable: {err}", candidate.label());
                data.close().await;
                last = Some(err);
            }
        }
    }
    Err(last.unwrap_or_else(|| "iroh has no reachable path".into()))
}

fn spawn_lease_renewer(
    mut writer: crate::control_transport::ControlTransportWriter,
    key: SecretKey,
    target: EndpointId,
    session: String,
    ttl: Duration,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let wait = ttl / RENEW_STEP;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            // Wake in short slices so a finished session never waits a whole
            // renewal period to join this thread.
            let mut elapsed = Duration::ZERO;
            while elapsed < wait {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let slice = RENEW_POLL.min(wait - elapsed);
                std::thread::sleep(slice);
                elapsed += slice;
            }
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let Ok(grant) = admission::SignedGrant::issue(&key, target, &session, utc_ms(), ttl)
            else {
                return;
            };
            let Ok(frame) = serde_json::to_string(&Handshake::Renew { grant }) else {
                return;
            };
            if writer.write_message(&frame).is_err() {
                return;
            }
        }
    })
}

fn one_shot_pty_line(transport: &mut ControlTransport, line: String) -> Result<()> {
    transport.write_message(&line)?;
    let Some(reply) = transport.read_message()? else {
        return Err("iroh control connection closed before the reply".into());
    };
    match crate::control_frames::ControlFrame::parse_inbound_result_message(&reply) {
        Ok(crate::control_frames::ControlFrame::ResponseLine(response)) => {
            let value: serde_json::Value =
                serde_json::from_str(response.strip_prefix("@response ").unwrap_or(&response))?;
            if value.get("error").is_some() {
                return Err(response.into());
            }
            println!("{response}");
            Ok(())
        }
        _ => Err("unexpected iroh control reply".into()),
    }
}

fn client_lane(
    mut transport: ControlTransport,
    key: SecretKey,
    target: EndpointId,
    ttl: Duration,
    request: IrohPtyRequest,
) -> Result<()> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let grant = admission::SignedGrant::issue(&key, target, &session_id, utc_ms(), ttl)
        .map_err(|_| "cannot issue an iroh session grant")?;
    transport.write_message(&serde_json::to_string(&Handshake::Admit { grant })?)?;
    log::info!("iroh handshake sent session={session_id}");
    transport.set_read_timeout(Some(CONNECT))?;
    let reply = transport
        .read_message()?
        .ok_or("iroh admission closed before a reply")?;
    log::info!("iroh handshake reply received");
    match serde_json::from_str::<HandshakeReply>(&reply)? {
        HandshakeReply::Admitted => {}
        HandshakeReply::Refused { code, reason } => return Err(format!("{reason} ({code})").into()),
        HandshakeReply::Renewed => return Err("unexpected iroh admission reply".into()),
    }
    transport.set_read_timeout(None)?;
    let one_shot = matches!(
        request,
        IrohPtyRequest::Detach { .. } | IrohPtyRequest::Close { .. }
    );
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let renewer = if one_shot {
        None
    } else {
        Some(spawn_lease_renewer(
            transport.try_clone_writer()?,
            key,
            target,
            session_id,
            ttl,
            stop.clone(),
        ))
    };
    let result: Result<()> = match request {
        IrohPtyRequest::Open { argv, cols, rows } => crate::pty_control::run_pty_client_transport(
            &mut transport,
            crate::pty_control::render_pty_open_line(&argv, cols, rows)?,
            true,
        )
        .map_err(Into::into),
        IrohPtyRequest::Attach {
            session_id,
            cols,
            rows,
        } => crate::pty_control::run_pty_attach_client_transport(
            &mut transport,
            crate::pty_control::render_pty_attach_line(&session_id, cols, rows)?,
            true,
        )
        .map_err(Into::into),
        IrohPtyRequest::Detach { session_id } => one_shot_pty_line(
            &mut transport,
            crate::pty_control::render_pty_detach_line(&session_id)?,
        ),
        IrohPtyRequest::Close { session_id } => one_shot_pty_line(
            &mut transport,
            crate::pty_control::render_pty_close_line(&session_id)?,
        ),
    };
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(renewer) = renewer {
        let _ = renewer.join();
    }
    result
}

/// A PTY request the formal CLI sends over iroh.
pub(crate) enum IrohPtyRequest {
    Open {
        argv: Vec<String>,
        cols: u16,
        rows: u16,
    },
    Attach {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    Detach {
        session_id: String,
    },
    Close {
        session_id: String,
    },
}

pub(crate) fn run_pty_control(path: &Path, name: &str, request: IrohPtyRequest) -> Result<()> {
    if !name_valid(name) {
        return Err("iroh target must be NAMESPACE/NAME".into());
    }
    let cfg: ClientConfig = profile(path)?;
    cfg.check_frame_budget()?;
    let key = identity(&cfg.key_file)?;
    let ttl = Duration::from_millis(cfg.lease_ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS));
    if ttl < admission::MIN_SESSION_TTL || ttl > admission::MAX_SESSION_TTL {
        return Err("iroh lease_ttl_ms is outside the supported range".into());
    }
    let bind = bind_addr(cfg.bind, cfg.routable)?;
    runtime()?.block_on(async {
        let dial = cfg.directory.dial(cfg.routable)?;
        let directory =
            directory_endpoint(key.clone(), Some(bind), dial.relay.as_ref(), cfg.routable).await?;
        let lookup = directory_call(
            &directory,
            &dial,
            DirectoryRequest::Lookup {
                name: name.to_owned(),
            },
            cfg.routable,
            DIRECTORY_CONNECT,
        )
        .await;
        directory.close().await;
        let DirectoryReply::Found { addr } = lookup? else {
            return Err("unexpected iroh directory lookup response".into());
        };
        let target = addr.id;
        let (data, conn, actual) =
            connect_with_path(&key, Some(bind), &addr, cfg.path, &cfg.relay, cfg.routable).await?;
        log::info!("iroh_path={}", actual.label());
        let (send, read) = timeout(IO, conn.open_bi()).await??;
        let transport =
            ControlTransport::Iroh(IrohLineTransport::new(send, read, cfg.frame_timeout()));
        let result =
            tokio::task::spawn_blocking(move || client_lane(transport, key, target, ttl, request))
                .await?;
        data.close().await;
        result
    })
}

pub(crate) struct IrohLineTransport {
    handle: Handle,
    send: Arc<Mutex<SendStream>>,
    guard: Arc<FrameGuard>,
    read: RecvStream,
    read_timeout: Option<Duration>,
    buffered: Vec<u8>,
    eof: bool,
}

pub(crate) struct IrohLineWriter {
    handle: Handle,
    send: Arc<Mutex<SendStream>>,
    guard: Arc<FrameGuard>,
}

impl IrohLineTransport {
    fn new(send: SendStream, read: RecvStream, frame_timeout: Duration) -> Self {
        Self {
            handle: Handle::current(),
            send: Arc::new(Mutex::new(send)),
            guard: Arc::new(FrameGuard::new(frame_timeout)),
            read,
            read_timeout: Some(IO),
            buffered: Vec::new(),
            eof: false,
        }
    }

    pub(crate) fn read_message(&mut self) -> io::Result<Option<String>> {
        loop {
            if let Some(line) = take_line(&mut self.buffered) {
                return Ok(Some(line));
            }
            if self.eof {
                return Ok(take_remainder(&mut self.buffered));
            }
            let handle = self.handle.clone();
            let wait = self.read_timeout;
            let read = &mut self.read;
            let mut chunk = [0_u8; 16 * 1024];
            let outcome = handle.block_on(async {
                match wait {
                    Some(wait) => timeout(wait, read.read(&mut chunk)).await.map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "iroh line read timed out")
                    }),
                    None => Ok(read.read(&mut chunk).await),
                }
            });
            match outcome?.map_err(io::Error::other)? {
                None => self.eof = true,
                Some(bytes) => {
                    self.buffered.extend_from_slice(&chunk[..bytes]);
                    if self.buffered.len() > crate::control_line_read::MAX_CONTROL_MESSAGE_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "iroh control message exceeds the line limit",
                        ));
                    }
                }
            }
        }
    }

    pub(crate) fn write_message(&mut self, message: &str) -> io::Result<()> {
        write_line(&self.handle, &self.send, &self.guard, message)
    }

    pub(crate) fn try_clone_writer(&self) -> IrohLineWriter {
        IrohLineWriter {
            handle: self.handle.clone(),
            send: self.send.clone(),
            guard: self.guard.clone(),
        }
    }

    pub(crate) fn set_read_timeout(&mut self, duration: Option<Duration>) {
        self.read_timeout = duration;
    }

    pub(crate) fn close(self) -> io::Result<()> {
        let send = self.send.clone();
        self.handle.block_on(async move {
            let mut stream = send.lock().expect("iroh send stream");
            stream.finish().map_err(io::Error::other)?;
            let stopped = timeout(IO, stream.stopped())
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "iroh close timed out"))?
                .map_err(io::Error::other)?;
            if stopped.is_some() {
                // The peer read the last frame and closed its receive half first;
                // finishing our send half is already complete.
                log::debug!("iroh peer closed its receive half before ours");
            }
            Ok(())
        })
    }
}

impl IrohLineWriter {
    pub(crate) fn write_message(&mut self, message: &str) -> io::Result<()> {
        write_line(&self.handle, &self.send, &self.guard, message)
    }
}

pub(crate) const FRAME_BROKEN: &str = "iroh_frame_broken";
/// A frame write may not be cut shorter than this; below it a healthy relay hop
/// already looks like a dead link.
const MIN_FRAME_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_FRAME_TIMEOUT: Duration = Duration::from_secs(120);

fn frame_budget(raw: Option<u64>) -> Result<Duration> {
    let timeout = raw.map_or(IO, Duration::from_millis);
    if timeout < MIN_FRAME_TIMEOUT || timeout > MAX_FRAME_TIMEOUT {
        return Err(format!(
            "iroh frame_timeout_ms must be between {} and {} ms",
            MIN_FRAME_TIMEOUT.as_millis(),
            MAX_FRAME_TIMEOUT.as_millis()
        )
        .into());
    }
    Ok(timeout)
}

pub(crate) struct FrameGuard {
    broken: AtomicBool,
    timeout: Duration,
}

impl FrameGuard {
    fn new(timeout: Duration) -> Self {
        Self {
            broken: AtomicBool::new(false),
            timeout,
        }
    }

    /// Hands the framed bytes to `write`, which must either store the whole frame
    /// or report a failure.
    fn ensure_intact(&self) -> io::Result<()> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(io::Error::other(FRAME_BROKEN));
        }
        Ok(())
    }

    /// Void the stream from inside a write that still holds it.
    fn mark_broken(&self) {
        self.broken.store(true, Ordering::SeqCst);
    }

    fn write<F>(&self, message: &str, write: F) -> io::Result<()>
    where
        F: FnOnce(&[u8]) -> io::Result<()>,
    {
        self.ensure_intact()?;
        let frame = frame_bytes(message)?;
        match write(&frame) {
            Ok(()) => Ok(()),
            Err(err) => {
                // A write that failed or ran out of time may have left part of this
                // frame on the stream. Later frames would then be read misaligned,
                // so this connection is done: abort it instead of writing more.
                self.mark_broken();
                Err(err)
            }
        }
    }
}

fn frame_bytes(message: &str) -> io::Result<Vec<u8>> {
    if message.len() > crate::control_line_read::MAX_CONTROL_MESSAGE_BYTES
        || message.contains(['\n', '\r'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid iroh line frame",
        ));
    }
    let mut frame = Vec::with_capacity(message.len() + 1);
    frame.extend_from_slice(message.as_bytes());
    frame.push(b'\n');
    Ok(frame)
}

fn write_line(
    handle: &Handle,
    send: &Arc<Mutex<SendStream>>,
    guard: &FrameGuard,
    message: &str,
) -> io::Result<()> {
    let send = send.clone();
    guard.write(message, |frame| {
        handle.block_on(async move {
            let mut stream = send.lock().expect("iroh send stream");
            // A writer that failed while this one waited for the lock has already
            // truncated the stream, so ask again now that the lock is held.
            guard.ensure_intact()?;
            let outcome = match timeout(guard.timeout, stream.write_all(frame)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(err)) => Err(io::Error::other(err)),
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "iroh line write timed out",
                )),
            };
            if outcome.is_err() {
                // Publish the broken state before this lock is released: a writer
                // that acquires it next must not append behind a half frame.
                guard.mark_broken();
            }
            outcome
        })
    })
}

fn take_line(buffered: &mut Vec<u8>) -> Option<String> {
    let end = buffered.iter().position(|byte| *byte == b'\n')?;
    let mut line: Vec<u8> = buffered.drain(..=end).collect();
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Some(String::from_utf8_lossy(&line).into_owned())
}

fn take_remainder(buffered: &mut Vec<u8>) -> Option<String> {
    if buffered.is_empty() {
        return None;
    }
    let line = String::from_utf8_lossy(buffered).into_owned();
    buffered.clear();
    Some(line)
}
