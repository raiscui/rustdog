> **本文件是 wayfinder research 产出,仅存在于 `research/rust-p2p-libraries` 分支,不会被合并。**
> 关联 ticket: #120(审计:五个 Rust P2P 库作为跨网 lane 候选) · 所属 map: #106
>
> **中文摘要**
>
> - **唯一能解开 #110"一个二进制 vs TURN"冲突的是 iroh。** `iroh-relay` 是**库**(`server` feature → `iroh_relay::server::Server::spawn()`),同一 crate 又提供 `[[bin]] name = "iroh-relay"` → 云端可以是单个 `rdog rendezvous` 二进制内嵌 relay server。许可证 MIT OR Apache-2.0,TLS 走 rustls(**无 OpenSSL / 无 C 工具链**),上游有专门的 *"integration tests for embedding the relay"*(n0-computer/iroh#4206)。**不需要 coturn、不需要 TURN 进程。**
> - **最反直觉的发现:`iroh-relay` 1.3.0 的 relay 数据面走 WebSocket/TCP,不是 QUIC/UDP**(四处独立源码互证:`client.rs:255-345` / `relay/actor.rs:451,541` / `socket.rs:122-125` 注释 / `server/http_server.rs:573` WS upgrade)→ **relay 兜底能活过只放行 TCP 的企业网络,五个候选里独有。**
> - **iroh 也是唯一满足"P2P 是硬需求"的**:直连建立在 QUIC NAT Traversal 扩展 + QAD + `portmapper` 之上;官方文档给出**有出处的成功率数字**("roughly 9 out of 10 network configurations allow a direct connection");且**失败不粘滞**(`HOLEPUNCH_ATTEMPTS_INTERVAL=5s` / `UPGRADE_INTERVAL=60s`)——正是 RustDesk "sticky degradation" 教训的正面答案。连接模型是 **relay-first-then-upgrade**。
> - **rathole 不是 P2P 库**:源码 grep 打洞/端口预测/STUN **零命中**,纯 TCP 反向隧道,**永远进不了直连集合**;且 published 0.5.0 无法同时做到"无 OpenSSL"和"有 WebSocket"。
> - **VNT 完全不在 crates.io**(必须 vendored git),数据面是 TUN/IP 包而非行协议,模型不匹配。
> - **rust-p2p 机制最诚实**(对称 NAT ±range 端口预测、relay 包级盲转发),但 44 stars / `rustp2p-quic` 仅 72 下载 / 0 反向依赖 → 机制参考实现,不承重。
> - **三个共同成本**:①**异步侵入**(全部 tokio-based,而 rdog `src/` 零 `async fn`;建议独立线程 + `block_on` 桥,勿全仓 async 化)②**MSRV 必须抬**(iroh 1.3.0 要 Rust 1.91,rdog 现 1.89)③**许可证干净**(全 Apache-2.0 / MIT,无 GPL 污染)。
> - **QUIC 无消息上限**(RFC 9000 §2.2),rdog 保留自己的行分帧即可,**不需要** WebRTC 那套 SCTP 分片层。
> - **身份系统必须并存**:iroh 强制第二套身份(`EndpointId` = Ed25519 公钥),需要目录表 `namespace/daemon_name → (EndpointId, RelayUrl)`——这是 #114 的实际工作量。
> - **relay 仍是寻址信任根**:读不到 payload,但能谎报候选、丢包、拖延(RustDesk 的 MITM 教训同样成立)。
> - **建议先做两个可判定实验,不要先写实现**:①`rdog rendezvous` 内嵌 `iroh-relay` 的最小原型,测直连成功率 / relay WS 连通性 / 8 MiB 真实行过 relay;②用现有 rdog 行协议在 iroh QUIC 流上跑通 `@ping`/`@screenshot`/`--pty`,量化 async 桥接侵入面。

I have a complete evidence base. Writing up the audit now.

---

# Rust P2P 库审计报告 — 跨网 lane 候选（rdog #120 / 支撑 #109 #110 #111 #115）

> 审计时间：2026-10-20。所有事实均来自 crates.io API、GitHub API、以及对候选仓库源码的浅克隆直读。未核验项集中在最后一节，不做"合理推测"填补。

## 1. Verdict

**唯一能解开 #110"一个二进制 vs TURN"冲突的是 iroh**：`iroh-relay` 是一个**库**，其 `server` feature 暴露 `iroh_relay::server::Server::spawn()`，而同一个 crate 又提供 `[[bin]] name = "iroh-relay"`（`required-features = ["server"]`）——云端可以做成单个 `rdog rendezvous` 二进制内嵌 relay server，许可证是 MIT OR Apache-2.0，TLS 走 rustls（**无 OpenSSL / C 工具链**），上游甚至有专门的 *"integration tests for embedding the relay"* 提交（n0-computer/iroh#4206, 2026-05-22）。**本次审计最关键、最反直觉的发现是：iroh-relay 1.3.0 的 relay 数据面走的是 WebSocket/TCP，不是 QUIC/UDP** —— 客户端 `connect()` 把 `https` 改写成 `ws`/`wss` 后用 `tokio_websockets` 拨号（`iroh-relay/src/client.rs:255-345`），`iroh` crate 的 relay actor 用的正是这个 `iroh_relay::client::Client`（`iroh/src/socket/transports/relay/actor.rs:451,541`），服务端在 HTTP server 上做 WS upgrade（`iroh-relay/src/server/http_server.rs:573 handle_relay_ws_upgrade`）——**因此 relay 兜底能活过只放行 TCP 的企业网络，这是五个候选里独有的**。iroh 同时是五者中唯一有真 P2P 直连路径的：其直连建立在 QUIC NAT Traversal 扩展之上（`noq` 的 `initiate_nat_traversal_round()`），官方文档给出"roughly 9 out of 10 network configurations allow a direct connection"的可引用成功率（docs.iroh.computer/concepts/nat-traversal），且**失败模式不粘滞**——iroh 始终保留 relay 路径并每 5s（`HOLEPUNCH_ATTEMPTS_INTERVAL`）重试打洞、每 60s（`UPGRADE_INTERVAL`）尝试升级路径，正好正面回应 RustDesk 的 "sticky degradation" 教训。**rathole 根本不是 P2P 库**：`grep -rniE "hole.?punch|port.?predict|reflexive|stun" src/` 在其源码里零命中，它是一条纯 TCP 反向隧道（"NAT traversal" 仅指客户端主动外拨），只能进兜底集合，永远进不了直连集合。**rust-p2p / rustp2p-quic 是机制上最诚实的一个**（有 Cone/Symmetric NAT 分类、端口预测、盲转发 relay，且明写"relays only inspect the rustp2p header and never terminate QUIC connections"），但 44 stars、`rustp2p-quic` 0.1.1 仅 72 次下载 / 0 反向依赖、crates.io 名 `rust-p2p-core` 与仓库内 `rustp2p-core` 不一致，**不足以承载 v1 的"真 P2P 是硬要求"**。**VNT 完全不在 crates.io 上**（`vnt`/`vnt-core`/`vnt2` 全部查无此 crate），必须 vendored git 依赖，仓库语言构成含 Java/JS/Python/Vue，且数据面是 TUN/IP 包而非行协议。**异步侵入是五者共同的、非区分性成本**：全部依赖 tokio，而 rdog 的 `src/` 今天零 `async fn` / 零 `tokio::`；iroh 侵入最大（iroh+iroh-base+iroh-relay+noq+netwatch+n0-*），但只有它值得这个代价。**身份系统必须并存的结论也成立**：iroh 的 `EndpointAddr{ id: EndpointId, addrs: BTreeSet<TransportAddr> }` 用公钥做身份，rdog 的是 zenoh keyexpr 目录 `rdog/<ns>/daemon/<name>/member/<id>/control`（`src/zenoh_identity.rs:165-214`），iroh 不会取代 rdog 的目录模型，只会**多出第二套身份**（直接接续 #114）。**综合裁决**：#109 的直连 lane 应收敛为 {iroh, 自研 QUIC + QNT}，#110 判定为"被 iroh-relay 解开"，但必须同时记录 iroh 的 MSRV 代价（iroh 与 iroh-relay 1.3.0 均要求 Rust **1.91**，rdog 现声明 **1.89**，需抬 MSRV）与其 2026 年高频 `feat(iroh-relay)!:` 破坏性变更（比 RustDesk 锁 `webrtc` 分叉更频繁，只是破在 relay crate 上）。

## 2. Per-library 事实表

| | **rathole** | **VNT** | **iroh** | **rust-p2p-core** | **rustp2p-quic** |
|---|---|---|---|---|---|
| crates.io 名 | `rathole` | **不在 crates.io**（`vnt`/`vnt-core`/`vnt2` 均 404） | `iroh`（+`iroh-relay`/`iroh-base`/`iroh-gossip`） | `rust-p2p-core`（仓库内包名 `rustp2p-core`，**不一致**） | `rustp2p-quic` |
| 当前版本 | 0.5.0（2023-10-01 发布） | 仓库 v2.0.11（2026-10-17 release） | 1.3.0（2026-09-28） | 0.4.1（2026-02-09） | 0.1.1（2026-08-11） |
| 总下载 / 近 90d | 44,443 / 3,726 | N/A | 3,433,472 / 2,148,536 | 17,288 / 758 | 72 / 未核验 |
| 反向依赖数 | 1 | N/A | **309**（`iroh-relay` 28） | 2 | **0** |
| GitHub | `rathole-org/rathole`（原 rapiz1/rathole，已改名） | `vnt-dev/vnt` | `n0-computer/iroh` | `rustp2p/rustp2p` | 同左 |
| Stars / Forks / Open issues | 14,358 / 847 / 98 | 3,259 / 359 / 105 | 12,793 / 780 / 183 | **44** | 44 |
| 最后提交 | 2026-08-23 | 2026-10-17 | 2026-10-20 | 2026-10-17 | 同左 |
| 许可证 | Apache-2.0 | Apache-2.0 | MIT OR Apache-2.0 | Apache-2.0 | Apache-2.0 |
| 纯 Rust? | 是（但 `tls` feature = `tokio-native-tls` → Linux 走 OpenSSL；`noise` feature 可绕开） | vnt-core 是纯 Rust；**整个仓库不是**（Java 64KB / JS 71KB / Python 15KB / Vue 175KB / `dll/` 预编译产物） | 是（rustls，无 OpenSSL；C 依赖：无） | 是（rustls via quinn） | 是（quinn 0.11 + rustls ring） |
| MSRV | 未声明 | 未声明 | **1.91**（rdog 现为 1.89 → 需抬） | 未声明 | 未声明 |
| NAT 机制 | **无打洞**。纯 TCP 反向隧道（客户端外拨建立控制通道） | UDP/TCP 打洞 + 端口预测（Cone/Symmetric 二分）+ 指数退避 5s→1h | QUIC NAT Traversal 扩展（`noq` 的 QNT）+ QAD 地址发现 + `portmapper`(UPnP/NAT-PMP/PPCP) | STUN 探测 + 双向同步打洞 + **对称 NAT 端口预测**（±range，用 relay 观测端口 + STUN 端口作基线） | 复用 rustp2p-core 传输 + PeerId 寻址 QUIC overlay |
| 打洞失败行为 | N/A（无打洞） | 退避后重试，保留 relay 路由（`route_timeout_task` 自然过期） | **保留 relay 路径**并持续重打洞（5s 重试 / 60s 升级），不放弃 | 退避重试（`should_punch` 按 batch_count 限流，最长 360s 间隔） | 直连路由 metric==0 优先，否则走 relay 路由（metric=跳数） |
| 可嵌入 relay server? | 可（lib+bin 同 crate），但语义是**反代**不是 P2P | vnt-core 内含 server 模块，**但需 vendored git** | **可，官方支持**：`server` feature → `iroh_relay::server::Server::spawn()` | 无独立 relay server crate；任一可达节点可作 relay（转发层） | 同左，relay 在 protocol.rs 内实现 |
| relay 传输层 | TCP（可选 WebSocket 作为 client↔server 控制通道） | 自有 UDP 隧道 | **WebSocket/TCP（ws://wss://）**；另存在 `QuicServer::spawn` 的 QUIC 服务端路径 | 自己的 UDP/TCP 传输 | 同左 |
| 生产使用 | Docker pulls **817,692**（2023-10-01 截止） | 3.2k stars，国内广泛使用（具体案例未核验） | n0 官方"run in production for the public relays"；28 个反向依赖 | 未核验（无生产证据） | 未核验 |
| 活跃度 | 仓库活跃（2026-08）但**发布版 0.5.0 已 3 年未更**，HEAD 已改名 feature（`websocket-rustls` 等）且未发布 | **非常活跃** | **非常活跃** | 活跃但极早期 | 极早期 |

## 3. Evidence 表（关键论断 → 来源 → 核验状态）

| 论断 | 来源 | 核验 |
|---|---|---|
| rathole 源码中不存在任何打洞/端口预测/STUN/reflexive 逻辑 | 本地 clone `rathole/src/` grep `hole.?punch\|udp\|port.?predict\|reflexive\|stun` → 仅命中 UDP-over-TCP 封装与 `UDP_BUFFER_SIZE` 常量 | ✅ 已核验 |
| rathole 是"客户端外拨 + 服务端开数据通道回连"的反向隧道 | `rathole/src/server.rs:250-300`（`DataChannelHello` → `do_data_channel_handshake`）、`:276-330`（token+nonce digest 认证） | ✅ 已核验 |
| rathole **已发布**的 0.5.0 确实含 `src/transport/websocket.rs` 且 `websocket` 在 default features | 下载 `rathole-0.5.0.crate` 解包：`tar tzf` 命中 `src/transport/websocket.rs`；`Cargo.toml` `default = [... "websocket" ...]` | ✅ 已核验 |
| rathole 默认 TLS 走 `tokio-native-tls`（Linux → OpenSSL）；仓库 HEAD 已加 `rustls` feature 但**未发布** | crates.io deps: `normal tokio-native-tls ^0.3 optional=true`；`rathole/Cargo.toml` HEAD 有 `rustls = [...]` 但 0.5.0 feature 表无 `rustls` | ✅ 已核验 |
| `iroh-relay` 是可嵌入的库 server | `iroh-relay/Cargo.toml`：`[features] server = [...]`；`iroh-relay/src/lib.rs:41-42` `#[cfg(feature="server")] pub mod server;`；`src/server.rs:692` `pub async fn spawn(config: ServerConfig) -> Result<Self, SpawnError>` | ✅ 已核验 |
| 同一 crate 同时提供 `iroh-relay` 二进制 | `iroh-relay/Cargo.toml:174-177` `[[bin]] name="iroh-relay" path="src/main.rs" required-features=["server"]` | ✅ 已核验 |
| iroh-relay **无 C/OpenSSL 依赖**，TLS 走 rustls | `iroh-relay/Cargo.toml`：`rustls = "0.23"`、`noq = { features=["rustls"] }`；无 `openssl`/`native-tls`/`ring` 需 C 的项 | ✅ 已核验 |
| **iroh-relay 的 relay 数据面走 WebSocket/TCP** | 客户端：`iroh-relay/src/client.rs:255-345`（`set_scheme(http→ws / ws→ws / else→wss)`，`tokio_websockets::ClientBuilder`）；`iroh/src/socket/transports/relay/actor.rs:451,541`（用 `iroh_relay::client::Client`）；`iroh/src/socket.rs:122-125` 注释"manages the WebSocket connection… (DNS + TCP + TLS + WebSocket upgrade)"；服务端：`iroh-relay/src/server/http_server.rs:573` `handle_relay_ws_upgrade` | ✅ 已核验（4 处独立源码） |
| iroh relay 是**盲转发**（不读 payload） | 官方文档："The relay server doesn't actively participate… simply forwards encrypted packets between nodes without knowledge of whether they contain application data"；协议层：`iroh-relay/src/protos/relay.rs` 只做 `ClientToRelayDatagram`/`RelayToClientDatagram` 帧转发 | ✅ 已核验 |
| iroh 直连成功率有公开数据："roughly 9 out of 10 network configurations allow a direct connection" | https://docs.iroh.computer/concepts/nat-traversal | ✅ 已核验（官方文档原文） |
| iroh 打洞失败**不粘滞**，持续重试并升级 | `iroh/src/socket/remote_map/remote_state.rs:50-52`（`HOLEPUNCH_ATTEMPTS_INTERVAL = 5s`、`GOOD_ENOUGH_LATENCY = 10ms`、`UPGRADE_INTERVAL = 60s`）、`:517-569`（`trigger_holepunching`）、`:944-985`（`do_holepunching` → `conn.initiate_nat_traversal_round()`） | ✅ 已核验 |
| iroh 的候选地址来自 QUIC NAT Traversal 扩展本身 + relay 观测 | `remote_state.rs:536-546` `conn.get_remote_nat_traversal_addresses()`；`iroh/src/net_report.rs` 基于 Tailscale netcheck；QAD 引用 `iroh/src/lib.rs:250` → draft-ietf-quic-address-discovery-00 | ✅ 已核验 |
| iroh 的连接模型是"先 relay 再升级到 direct"，不是"direct 优先" | 官方文档四步（1 Initial Contact Through a Relay → 2 Sharing Connection Information → 3 Simultaneous Outbound Connection → 4 Fallback to Relay）；`iroh/src/socket.rs` 文档 "RelayOnly path selection" 反证 relay 路径是常态 | ✅ 已核验 |
| iroh 端到端身份 = 公钥 `EndpointId`，地址 = `BTreeSet<TransportAddr>` | `iroh-base/src/endpoint_addr.rs:42-62`：`EndpointAddr{ id: EndpointId, addrs: BTreeSet<TransportAddr> }`，`TransportAddr = Relay(RelayUrl) | Ip(SocketAddr) | Custom` | ✅ 已核验 |
| iroh/iroh-relay 1.3.0 要求 Rust 1.91，rdog 现声明 1.89 | `iroh/Cargo.toml` 与 `iroh-relay/Cargo.toml` `rust-version = "1.91"`；rdog `Cargo.toml` `rust-version = "1.89"` | ✅ 已核验 |
| iroh-relay 支持 relay 鉴权 token | n0-computer/iroh#4205 "support setting auth tokens for relays"（2026-05-06） | ✅ 已核验 |
| 上游有"嵌入 relay"的集成测试 | n0-computer/iroh#4206 "test(iroh-relay): integration tests for embedding the relay"（2026-05-22） | ✅ 已核验 |
| rdog 控制协议是**按行文本**、单行上限 8 MiB、文件块 6.4 MiB base64 | `src/control_line_read.rs:30`、`:1-20` 文档注释；`src/control_file_transfer.rs:44` `CHUNK_BASE64_LIMIT = (MAX*4)/5` | ✅ 已核验 |
| rdog `src/` 今天没有任何 async runtime | `grep -rn "tokio::\|async fn\|futures::" src/` → 0 命中；`Cargo.toml` 无 tokio 直接依赖 | ✅ 已核验 |
| rdog 身份 = zenoh keyexpr 目录 `rdog/<ns>/daemon/<name>/member/<id>/control` | `src/zenoh_identity.rs:165-214` | ✅ 已核验 |
| rathole 生态规模 | crates.io reverse_dependencies = 1；Docker Hub `rapiz1/rathole` pull_count = 817,692（last_updated 2023-10-01） | ✅ 已核验 |
| iroh 生态规模 | crates.io reverse_dependencies = 309（iroh）/ 28（iroh-relay） | ✅ 已核验 |
| rustp2p-quic 生态为空 | crates.io reverse_dependencies = 0，downloads = 72 | ✅ 已核验 |
| VNT 未发布到 crates.io | crates.io `/crates/vnt`、`/crates/vnt-core`、`/crates/vnt2`、`/crates/vnt-ipc` 全部 "does not exist"；`q=vnt` 搜索无相关项 | ✅ 已核验 |
| rustp2p 的 relay 是**包级盲转发**，不解密 QUIC | `rustp2p-quic/src/protocol.rs:1240-1241` "Forwarding is packet-level. Relays only inspect the rustp2p header and never terminate QUIC connections or decrypt QuicRelay payloads" | ✅ 已核验 |
| rustp2p 有对称 NAT 端口预测（RustDesk 式 give-up 的对冲） | `rustp2p-core/src/punch/mod.rs:17-38` `symmetric_prediction_candidates`（以 relay 观测端口 + STUN 端口为基线做 ±range 预测）；`docs/nat-punch-principles.md` §3 NAT 组合矩阵 | ✅ 已核验 |
| VNT 打洞退避曲线 5s→1h | `vnt-core/src/context/nat.rs:150-165` `RETRY_BACKOFF = [5s,10s,30s,60s,300s,900s,3600s]`、`PUNCH_COOLDOWN=5s`、`NAT_CHANGE_CAP=600s` | ✅ 已核验 |
| QUIC 无消息边界，rdog 必须自带分帧 | RFC 9000 §2.2："Streams are an ordered byte-stream abstraction with no other structure visible to QUIC. STREAM frame boundaries are not expected to be preserved…" | ✅ 已核验 |
| QUIC 单包上限 65527 字节 | RFC 9000 §14（`max_udp_payload_size`）："The default for this parameter is the maximum permitted UDP payload of 65527" | ✅ 已核验 |
| 全部五者均为 tokio-based | crates.io deps：iroh/iroh-relay `tokio`、rathole `tokio ^1 features=full`、rust-p2p-core `tokio ^1.42 features=full`、rustp2p-quic `tokio`、vnt-core `tokio features=full` | ✅ 已核验 |
| n0 称 "95% 的连接数据走直连" | 第三方博客转述（guancyxx.cn） | ❌ **未核验**（未找到 n0 一手出处） |
| rustp2p / VNT 的直连成功率公开数据 | 检索未发现 | ❌ **未核验** |
| rdog 是否能在 250ms 延迟预算内跑通（#118） | 无实测 | ❌ **未核验** |
| iroh-relay 的 `QuicServer` 路径何时被选用 | 代码存在（`iroh-relay/src/quic.rs`），但客户端走 WS；选用条件未追完 | ⚠️ 部分核验 |

## 4. 逐库机制深读

### 4.1 rathole — 纯 TCP 反向隧道，**不是 P2P**

`rathole` 自称 "A reverse proxy for NAT traversal"，但"traversal"指的是**客户端主动外拨**：NAT 后的设备向有公网 IP 的 server 建立 TCP 连接，server 为该连接保留一个 control channel；公网来客连到 server 的服务端口时，server 沿**已建立的这条 client 连接**开一条 data channel 把字节送回 NAT 后设备（`rathole/src/server.rs:250-300`）。认证是 token+nonce 的 digest（`:276-330`），控制通道可选 Noise（`snowstorm`）/ TLS / **WebSocket**（`src/transport/websocket.rs`，`tokio-tungstenite`）。

- **它能做到的**：把"没有端口映射的 daemon"变成"经由云端公网端口可达"——但**云端承载全部字节**，这是 relay 而非 P2P，直接违反 v1 的"真 P2P 是硬要求"。
- **它做不到的**：任何形式的直连。零打洞代码。
- **成本**：发布版 0.5.0 已 3 年未更新（仓库 HEAD 2026-08-23 活跃但**未重新发布**，且已把 feature 改名 `websocket-native-tls`/`websocket-rustls`、新增 `rustls`——published 0.5.0 无 `rustls`，只有 `tls = tokio-native-tls` → Linux 上 OpenSSL）。要纯 Rust 又要 WebSocket，published 0.5.0 只能选 `default-features=false, features=["server","client","noise","websocket"]`，而 `websocket` 又依赖 `tls` → 仍带 native-tls；`embedded` feature（server+client+hot-reload+noise）可去掉 TLS 但同时丢掉 WebSocket。**换言之：published 0.5.0 无法同时做到"无 OpenSSL"和"有 WebSocket"**。这是选 rathole 的硬伤。

### 4.2 VNT — 全功能 VPN 产品，不是可嵌入的 crate

VNT 的定位是异地组网 VPN（`vnt-core` 里有 TUN/IP 注入、端口映射、加密 IP 包协议、桌面端 Tauri UI）。它的服务端是一个 hub：所有节点（含控制端）都连 hub，hub 转发加密的 IP 包。**它不是"控制端 ↔ 目标 daemon"的定向隧道**，而是"加入同一个虚拟网络"——与 rdog 的"按 `namespace/daemon_name` 定向寻址"模型不匹配，要用 VNT 就得把 rdog 控制协议塞进 TUN 之上（要么跑在 VNT 造出的虚拟网卡上，要么把行协议包成 IP 载荷）。

打洞机制本身是认真的：`PunchPolicy` 四象限（IPv4/IPv6 × TCP/UDP）、Cone/Symmetric 二分、对称 NAT 端口预测、`PunchBackoff` 5s→1h 指数退避、并发打洞限流 4。**但**：crates.io 上不存在（必须 vendored git 依赖，版本漂移无保护）、仓库语言构成含 Java/JS/Python/Vue、`dll/` 目录带预编译产物、数据面是 IP 包而非行协议。

### 4.3 iroh — 唯一同时具备"真直连 + 可嵌入 relay + TCP 存活"的候选

**地址发现与打洞**（iroh 1.x 已重构：`src/magicsock/` 目录**不存在**）：
- `iroh/src/socket.rs` 是新的传输层（"a socket that can change its communication path while in use"），维护 direct/relay 多路径；
- 打洞状态机在 `iroh/src/socket/remote_map/remote_state.rs`（1,682 行的 `connection.rs` 同级），调用 `conn.initiate_nat_traversal_round()`（由 `noq`/`noq-proto` 的 `n0_nat_traversal` 模块实现，即 **QUIC NAT Traversal 扩展**）与 `conn.get_remote_nat_traversal_addresses()` 取对端候选；
- 候选来源三条：本地接口地址（`netwatch`）、**relay 观测到的 reflexive address**（官方文档明写 "The relay server observes each node's public IP address and port"）、以及 QAD（QUIC Address Discovery，draft-ietf-quic-address-discovery）探针；另有 `portmapper`（UPnP/NAT-PMP/PPCP）尝试在本地路由器上开洞；
- **重试/升级节奏**：`HOLEPUNCH_ATTEMPTS_INTERVAL = 5s`（候选没变化则不重复打）、`UPGRADE_INTERVAL = 60s`（已有非 relay 路径也定期尝试升级到更优路径）、`GOOD_ENOUGH_LATENCY = 10ms`（低于此延迟不折腾）。**这是"永不粘滞降级"的机制化保证**，正是 RustDesk 教训的正面答案。
- **fallback 触发条件**：不是"打洞失败就降级"，而是**始终先走 relay**（relay 是 bootstrap、是候选发现代理、是兜底），direct 是**升级**。路径状态由 `path_state.rs` 管理（非 relay 路径上限 `MAX_NON_RELAY_PATHS`，relay 路径不计入剪枝）。

**Relay server 可嵌入性**：`iroh-relay` 的 `server` feature 打开 `pub mod server`，`ServerConfig` → `Server::spawn()` → 内部 `QuicServer::spawn` + HTTP server（WS upgrade 承载 relay 协议）+ metrics server + 可选 ACME（`tokio-rustls-acme`，`CertConfig::AcmeConfig`）。默认 features `["metrics","tls-ring"]` → rustls + ring，**无 OpenSSL**。附带能力：per-client 限流（可热更新，`ClientRateLimit`）、auth token（`CLIENT_AUTH_HEADER` / `Authorization: Bearer`）、TLS key-export 客户端鉴权（`KeyMaterialClientAuth`）、`AccessControl` trait（`AllowAll` 可自定义）——**这些正好映射 rdog 的 #113 认证模型与 #115 策略模型**。

**未解的风险**：relay 仍是**寻址信任根**（RustDesk 的 MITM 教训）——它无法读 payload（盲转发 + 内层 QUIC E2E TLS），但可以谎报候选地址、丢包、拖延。缓解手段是 iroh 的端到端公钥身份（`EndpointId` 钉住对端后，relay 无法冒充对端完成 QUIC 握手），但"拒绝服务/降级"面无法消除。

### 4.4 rust-p2p / rustp2p-quic — 机制最诚实，规模最小

`rustp2p-core`（17,288 下载）：UDP/TCP 隧道原语、route table、STUN/NAT 探测（`NatType{Cone,Symmetric}`）、打洞执行器（`punch/mod.rs`）、`tachyonix` 依赖。`rustp2p-quic`（72 下载，0 反向依赖，0.1.1）：PeerId 寻址的 QUIC overlay，**用户载荷全程 QUIC/TLS 端到端加密**，relay 只在包层转发（`protocol.rs:1240-1241` 原文："Relays only inspect the rustp2p header and never terminate QUIC connections or decrypt QuicRelay payloads"）。

**对称 NAT 处理**是最有价值的一点：`symmetric_prediction_candidates()` 以 **relay 观测到的端口 + STUN 映射端口**为基线做 ±range 端口预测，并用 `should_punch()` 限流（batch_count ≤ 8 时不限，之后按 count/8 秒、最长 360s 间隔）——这正是 RustDesk "对称 NAT 直接放弃" 的反面。**但**：44 stars、`rustp2p-quic` 零生态、crates.io 包名 `rust-p2p-core` 与仓库内 `rustp2p-core` 不一致（依赖时需 `package = "rust-p2p-core"` 改名，是供应链气味）、0.1.1 版本号与 core 的 0.4.1 编号体系割裂。**定位：可作为"机制参考实现"和长期候选，不适合 v1 承重。**

## 5. 传输层适配（8 MiB 单行 / PTY 双向流 / `@screenshot`+`@savefile`）

| 维度 | 事实 | 影响 |
|---|---|---|
| 8 MiB 单行 | QUIC 是**字节流**（RFC 9000 §2.2："Streams are an ordered byte-stream abstraction with no other structure visible to QUIC"），**没有消息上限**；单 UDP 包上限 65527（RFC 9000 §14）但那只是包大小，不是消息上限 | rdog **必须保留自己的行分帧**（`read_line_limited` + 8 MiB 上限逻辑可原样复用）。8 MiB 行在 QUIC 上无阻碍。⚠️ 推断（非实测）：iroh-relay 的 WS 帧上限 `MAX_FRAME_SIZE = 1 MiB`，但它承载的是**内层 QUIC 包（≤ `MAX_PACKET_SIZE = 64 KiB`）**，8 MiB 行会被 QUIC 层切成许多小包穿过 relay，因此不构成限制——**此推论需实测确认** |
| PTY 双向流 | QUIC 双向流天然适配；rathole 是 TCP 字节流也适配；rustp2p-quic 提供 `open_bi`/`send_to`（datagram + stream 双模） | iroh 与 rathole 无额外分帧成本；rustp2p 的 datagram 模式不适合 8 MiB 行，只能走 stream |
| 背压 | QUIC 有流控（`initial_max_data` / `initial_max_stream_data`），但 rdog 是**同步阻塞**代码，异步流控必须桥接成阻塞写 | 这是 async 侵入的真正成本点：rdog 现有 `ControlTransport` 是同步 trait（`read_message`/`write_message`/`set_read_timeout`），iroh 需要一层 `blocking::recv()`/`send()` 桥（`noq` 提供了 blocking 包装吗？**未核验**） |
| `@screenshot` 多帧 + `@savefile` | rdog 已有 base64 分块（6.4 MiB/块）机制，走同一条行协议 | 跨网 lane 无需特殊改造，**前提是保留行协议语义**——这是选择"iroh QUIC 之上跑 rdog 行协议"而非"改用 QUIC 原生多路复用"的核心理由 |
| TCP-only 网络 | iroh-relay 的 relay 是 WS/TCP ✅；iroh 的 **direct 路径必须 UDP**（UDP 被封则打洞物理上不可能）；rathole 纯 TCP ✅；rustp2p/VNT 的 relay 走自有 UDP 传输 ❓（未完全核验） | iroh 的组合最健壮：**TCP-only 网络 → 只能走 relay，但 relay 活着**；这是本审计中 iroh 相对所有候选的决定性优势 |

## 6. 反驳与成本（同等重要）

**维护风险**：
- iroh 生态最大（309 反向依赖）、上游最活跃，但 **`iroh-relay` 在 2026 年有大量 `feat(iroh-relay)!:` 破坏性变更**（#4205 auth token、#4207 ClientRequest、#4094 迁移 EndpointInfo 到 iroh-dns、#4156 错误类型重构、#4300 CaRootsConfig→CaTlsConfig 重命名）——**破在 relay crate 上**。这与 RustDesk 锁 `webrtc` 0.13 并 fork 的性质相同（上游破坏性变更驱动下游 pin/fork），只是 iroh 用 semver major 消化（0.x→1.x 刚过），**风险等级取决于是否接受"iroh 与 iroh-relay 必须严格同版本锁"**。建议：固定 `iroh = "=1.3.0"` + `iroh-relay = "=1.3.0"`，并把 relay crate 的升级纳入 rdog 的回归门禁。
- rathole：发布版 3 年未更新，但**内容极小**（3,173 行 Rust）且已转 Apache-2.0，fork 成本低。
- rustp2p / VNT：上游活跃，但**都不在 crates.io**（VNT）或**包名不一致**（rustp2p），供应链无版本保护。

**异步侵入**：全部 tokio-based，rdog 需新建 runtime（建议独立线程 + `block_on` 桥，而非全仓 async 化）。侵入量级：`iroh` ≫ `rustp2p-quic` > `VNT` > `rathole`。iroh 还要额外吃下 `netwatch`（网卡监听）、`n0-watcher`/`n0-future`（n0 自己的 async 封装）、`noq`（n0 的 QUIC，quinn 血统）、`papaya`（并发 map）、`ed25519-dalek`（身份）。**rdog 已有 zenoh 1.8 传递性带入 tokio，这条"反正迟早有 tokio"的论证成立但不能当作免费——zenoh 的 tokio 与 iroh 的 tokio 特性集可能冲突（`rt-multi-thread` 等），需实测。**

**许可证**：**五者全部无 GPL/AGPL 污染**——Apache-2.0（rathole/VNT/rustp2p）、MIT OR Apache-2.0（iroh）。rdog 自身是 GPL-3.0-only（`Cargo.toml`），Apache-2.0/MIT 依赖可自由链接进 GPL 分发，**分发侧无风险**。需注意的只是"依赖树里每个传递依赖的许可证"，而这一项我**未逐个核验**（见 §7）。VNT 仓库含预编译 `dll/` 二进制产物，若 vendored 需单独审二进制来源与许可证。

**UDP 被封**：见 §5 表。**只有 iroh 的组合能在 UDP 被封时仍连通**（relay 走 WS/TCP）。

**IPv6-only / CGN 叠加**：
- iroh：`net_report` 明确探测 v4/v6 可达性与 NAT 情况（对标 Tailscale netcheck）；打洞策略是 `PunchPolicySet` 四象限（`rustp2p` 是同构设计）；`portmapper` 尝试 UPnP/NAT-PMP/PPCP 开洞；**CGN 叠加**（运营商级 NAT + 家庭 NAT 双层）会同时命中"对称 NAT 端口变化"与"多层映射"，iroh 的应对是**端口预测 + 多候选并发 + relay 兜底**——与 Tailscale 同源经验，一致。
- rustp2p：显式建模 Symmetric NAT 并做端口预测，机制上是对冲得最狠的。
- VNT：Cone/Symmetric 二分 + 退避重试。
- **三者的真实跨网成功率都没有公开实测数据**（§7）。

**iroh 身份模型 vs rdog 目录模型**：`EndpointAddr{ id: EndpointId, addrs: BTreeSet<TransportAddr{Relay(RelayUrl)|Ip(SocketAddr)|Custom}> }`。rdog 的 `namespace/daemon_name` 是 **zenoh keyexpr 目录**（`rdog/<ns>/daemon/<name>/member/<member_id>/control`），带 `member_id` 区分同 daemon 的多实例。iroh 的 `EndpointId` 是 Ed25519 公钥，`RelayUrl` 是中继地址。**结论：iroh 强制第二套身份系统**，不替换 rdog 的。落地需要一张目录表 `namespace/daemon_name → (EndpointId, RelayUrl)`——这正是 #114「目录服务数据模型与 namespace 全局化」的工作量，**#109 的选型不应假装它不存在**。

## 7. 架构组合对比

| | **(i) iroh direct + rdog 自研 WS relay 兜底** | **(ii) 全 iroh 栈（内嵌 iroh-relay）** | **(iii) rathole 反向隧道打底 + P2P 叠加** | **(iv) VNT / rust-p2p 路线** |
|---|---|---|---|---|
| **云端跑什么** | `rdog rendezvous`：内嵌 `iroh-relay` 的 **QAD/地址观测 + HTTP API**（供 iroh 候选发现），另跑 **rdog 自研 WS relay**（转发 rdog 行协议帧） | `rdog rendezvous`：内嵌 `iroh_relay::server::Server::spawn()`（WS relay 数据面 + HTTP API + metrics + 可选 ACME），一份二进制 | `rdog rendezvous`：内嵌 rathole **server**（TCP + 可选 WS 控制通道），云端开一个公网端口 | VNT：跑 VNT server（hub）；rustp2p：云端作为**可达 bootstrap 节点**兼 relay |
| **daemon 跑什么** | `iroh::Endpoint`（直连 lane）+ 保留 rdog 现有 `ControlTransportKind::Tcp/WebSocket`（兜底 lane） | `iroh::Endpoint` + relay map 指向云端 | rathole **client**（外拨云端，token 认证），本地 service = rdog daemon 自身 | VNT client（组网）+ rdog 跑在虚拟网卡上；或 rustp2p-quic Endpoint |
| **控制端跑什么** | `iroh::Endpoint` + rdog 现有行协议客户端 | 同左 | rathole 语义下的"公网来客"：连云端公网端口 | VNT client / rustp2p-quic Endpoint |
| **直连路径** | iroh QUIC 直连（QNT 打洞），**载荷 = rdog 行协议** | 同左 | **不存在**（rathole 无打洞）；"P2P 叠加"在默认配置下**不被使用** | VNT/rustp2p 的 UDP 打洞直连（VNT 走 TUN/IP，rustp2p 走 QUIC overlay） |
| **兜底路径** | rdog 自研 WS relay（TCP）——**云端看得到行协议明文，除非 rdog 自行 E2E 加密**（RustDesk 盲中继教训 → 必须自加密） | iroh-relay WS relay（TCP，**盲中继**，内层 QUIC E2E） | 无（relay 本身就是底座） | VNT hub 转发（hub 看得到 IP 载荷，**需 rdog 自加密**）；rustp2p relay（**包级盲转发**，不解密 QUIC ✅） |
| **依赖足迹** | iroh 全家 + rdog 现有 `tungstenite`（**已存在**） | iroh 全家 + `iroh-relay`(server: hyper/axum 栈/dashmap/rcgen/rcgen-acme/tokio-websockets) —— **最重** | rathole 3.2k 行 + tokio —— **最轻** | VNT：需 vendored git（Rust 1.3M 行 + 非 Rust 资产）；rustp2p：core+quic 两个 crate |
| **失败模式** | 两条 relay 协议并存（iroh 的 + rdog 的）→ 双份代码路径；WS lane 需自研 E2E 与限流 | relay 成为单点信任根（可谎报候选/拒绝服务）；**relay crate 破坏性变更频繁**，必须严格锁版本；MSRV 抬到 1.91 | 云端承载全部字节（带宽成本）；**永远没有直连 → 违反 v1 硬要求**；published 0.5.0 无法同时"无 OpenSSL + 有 WebSocket" | VNT 未上 crates.io（供应链无保护）、数据面模型不匹配、TUN 需要特权；rustp2p 生态为零（72 下载）、包名不一致 |
| **许可证** | MIT/Apache-2.0 + GPL-3.0-only ✅ | 同左 ✅ | Apache-2.0 ✅ | Apache-2.0 ✅（但 vendored 二进制产物需单独审） |
| **对 #109 的推动** | **"直连 lane = iroh"**；同时把"自研"选项的形态从"自研打洞协议"纠正为"自研 QUIC + QNT 扩展" | 同左，但给 iroh-relay 定了性：**是兜底基础设施，不是直连** | **"rathole 出局于直连 lane"**（零打洞代码，源码级证据） | **"v1 出局"**（成熟度不足） |
| **对 #110 的推动** | **解开**：一个二进制同时承载 iroh-relay server（QAD+兜底）与 rdog 自研 relay；**不需要 coturn/TURN 进程** | **解开**（最直接）：官方支持嵌入 relay 的库 + 集成测试 | **不解开真 P2P 冲突**（只解"一个二进制"，不解"必须有直连"） | 不解开（要么 vendored 第二个进程，要么把云做成 rustp2p 节点） |
| **对 #111 的推动** | 端点 = `RelayUrl` + 对端 `EndpointId`；**没有 ICE 角色**（云不是 controlling agent） | 同左 | 云端公网 TCP 端口 = 唯一"公网端点" | 云端 hub 地址 = 唯一"公网端点" |
| **对 #115 的推动** | 策略必须按**已建立会话的路径**判定（relay/direct），不能按"是否用了 relay 建连"判定 | 同左；且 iroh-relay 自带 per-client 限流 + auth token，可承载策略执行点 | 策略只能是"是否允许走云" | 同左 |

## 8. 对 #109 / #110 / #111 / #115 的具体修正

### #109（直连 lane 技术选型）— 修正候选集合与"自研"的成本形态

> **修正**：把 shortlist 从 {webrtc / str0m / 自研} 改为 **{iroh, 自研 QUIC + QUIC-NAT-Traversal(QNT) 扩展}**，并**把 rathole / VNT / rust-p2p 明确移出"直连 lane"集合**（它们只能进兜底或长期候选）。
>
> 依据：(a) rathole 源码零打洞代码；(b) rustp2p 生态为空（72 下载 / 0 反向依赖）、VNT 不在 crates.io；(c) **自研的真实成本被低估**——iroh 1.x 的直连不是自创协议，而是 `noq` 里的 **QUIC NAT Traversal 扩展**（`noq_proto::n0_nat_traversal`）+ QAD（draft-ietf-quic-address-discovery-00），自研等于自己实现 QNT + QAD，而非"自己写个打洞"；同时必须承认 **UDP 被封时任何 UDP 打洞方案都不可能直连**，这是所有候选的共同上限。

### #110（一个二进制 vs TURN）— 冲突被 iroh-relay 解开，但信任根问题要单列

> **修正**：把议题从"标准 ICE 需要 TURN 服务器"重构为"**我们不需要标准 ICE**"。`iroh-relay` 是**库**（`server` feature → `iroh_relay::server::Server::spawn()`），与 `iroh-relay` 二进制同 crate、MIT OR Apache-2.0、rustls 无 OpenSSL、上游有"embedding the relay"集成测试（#4206）——**云端单二进制内嵌 relay 可行，不需要 coturn/TURN 进程**。
>
> 同时必须把**两个被 #110 掩盖的问题写进结论**：(1) **relay 仍是寻址信任根**（RustDesk 教训）——它读不到 payload（盲转发 + 内层 QUIC E2E），但能谎报候选、丢包、拖延；对策是 #113 用 `EndpointId` 钉住身份 + relay auth token（iroh-relay #4205 已支持）；(2) **iroh-relay 1.3.0 的 relay 数据面走 WebSocket/TCP**——这既是 UDP 被封网络能活下来的原因，也意味着**不能假设 relay 端口是 UDP**，防火墙策略与运维手册要按 TCP 写。

### #111（ICE 角色与公网端点）— 换掉 ICE 词汇，改成 RelayUrl + EndpointId + 候选三元组

> **修正**：iroh 体系里**没有 ICE 角色**。云端**不是** controlling agent，两个 peer（控制端与目标 daemon）**不是** controlled/controlling 关系。云端的真实角色是三重：**rendezvous（QAD 候选发现代理）+ 盲 relay（兜底数据面）+ 身份目录**。
>
> 因此 #111 应改为裁决三件事：(a) **"公网端点"是什么**——不是 IP:port，而是 `RelayUrl`（https 形式，`RelayUrl` 解析 https/ws）+ 对端 `EndpointId`；(b) **候选从哪来**——由云端观测对端的 reflexive address（官方文档："The relay server observes each node's public IP address and port"），以及 QAD 探针与 `portmapper`(UPnP/NAT-PMP/PPCP) 的本机映射；**不预设对端地址可知**；(c) **namespace/daemon_name → EndpointId 的目录映射**归 #114，#111 只需声明它存在这个依赖，并处理"一个 daemon 多实例"（rdog 现有 `member_id`）与一个 `EndpointId` 的关系。

### #115（direct-only / relay-allowed 策略模型）— 策略必须按"路径"判定，且必须可重新裁决

> **修正**：iroh 的模型是 **relay-first-then-upgrade**（连接**必须**先经 relay 建立，direct 是后续升级），所以 "direct-only" **不可能**表达为"不许用 relay"——那等于不许建连。策略必须改到**已建立会话的路径**上判定：`direct-only` = "会话当前路径是 relay 即拒绝执行/断开并强制重打洞"。
>
> 同时补三条：(a) **不可粘滞**——RustDesk 教训要求每次路径变更都重判策略，而 iroh 本身每 5s 重打洞、每 60s 尝试升级（`HOLEPUNCH_ATTEMPTS_INTERVAL` / `UPGRADE_INTERVAL`），策略层必须能跟上这个节奏，不能只在 connect 时裁决一次；(b) **对称 NAT 下要有明确裁决**——rdog 不能接受 RustDesk 式"对称 NAT 直接放弃"，因此 `direct-only + 对称 NAT` 必须是一个**带 reason code 的显式硬失败**，而不是静默走 relay；(c) **relay 作为策略执行点**——iroh-relay 自带 per-client 限流（可热更新）与 auth token，可在 relay 侧执行"此 namespace 禁止 relay"这类粗粒度策略，把"direct-only 立刻失败"从秒级降到毫秒级。

## 9. 未核验 / 开放问题（明确列出）

1. **n0 "95% 连接数据走直连"** 的一手出处未找到（只有第三方博客转述）——需向 n0 求证或放弃引用；本报告只采用官方文档的 "9 out of 10 network configurations"。
2. **rustp2p / VNT 的直连成功率** 无任何公开实测数据；rustp2p 的 `docs/nat-punch-principles.md` 是**原理说明**不是实测报告。
3. **iroh-relay 的 WS 帧上限 1 MiB 对 8 MiB rdog 行是否真的无影响** —— 我的推论是"内层 QUIC 包 ≤64 KiB 故不构成限制"，**未实测**。需用 `MAX_CONTROL_MESSAGE_BYTES` 的真实行做一次端到端压测。
4. **`noq`（quinn 血统）是否提供 blocking 桥**——rdog 同步 trait 到异步 QUIC 流的桥接成本未知；未核验。
5. **zenoh 1.8 带入的 tokio 与 iroh 需要的 tokio 特性集是否冲突**（`rt-multi-thread` 等）——未实测。
6. **iroh-relay 的 `QuicServer`（QUIC/UDP）路径与 WS 路径的选用条件/共存规则**未追完；`iroh-relay` 的 HTTP API（QAD/地址观测）具体端点与鉴权语义未逐条核验。
7. **CGN 叠加（运营商 NAT + 家庭 NAT 双层）** 在 iroh / rustp2p / VNT 下的实际表现——只有代码层推理，无实测；需在 #117 的三 NAT 域环境里验证。
8. **IPv6-only 网络**行为：代码层有 `PunchPolicy::IPv6*` / net_report v6 探测，但无实测。
9. **依赖树全量许可证审计**未做（只确认了 5 个候选自身的 SPDX 与顶层关键依赖）；若最终选 iroh，需对 `iroh-relay`(server feature) 的完整依赖树做一次 license scan 再进 GPL-3.0-only 分发。
10. **VNT 的 `dll/` 预编译产物来源与许可证**未核验。
11. **rdog 现有 zenoh 控制面（`rdog control` → zenoh session channel）在跨网场景下是否继续保留**——本报告假设新 lane 是**独立的第四种 transport**（`ControlTransportKind` 追加成员），但"zenoh 面是否也走新 lane"属于 #112 范围，未评估。
12. **`rustp2p` 的 crates.io 名 `rust-p2p-core` 与仓库内 `rustp2p-core` 不一致**：不确定是发布时改名还是历史遗留，**未核验**。
13. **rathole 仓库 HEAD 与 crates.io 0.5.0 的完整 diff** 未逐文件比对（只比对了 feature 表与 transport 文件清单）。

---

### 建议的下一步（最小、最能解锁决策的一步）

不要先写代码。先做两件可判定的实证：**(1)** 用 `iroh` 1.3.0 + `iroh-relay` 1.3.0 写一个**最小可嵌入原型**（`rdog rendezvous` 子命令内 `Server::spawn()` + 两条 daemon 经三个 NAT 域互连），实测三件事：直连成功率、relay 走 WS 的实际连通性、以及一条 8 MiB 真实行能否过 relay；**(2)** 同期用**现有 rdog 行协议**在 iroh QUIC 流上跑通 `@ping`/`@screenshot`/`--pty`，量化 async 桥接的实际侵入面。这两步能把 §9 的第 1/3/4/5 项从"未核验"变成实测结论，并同时给 #118（250ms 预算）与 #115（策略按路径判定）提供数据。
