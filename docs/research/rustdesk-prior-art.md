> **本文件是 wayfinder research 产出,仅存在于 `research/rustdesk-prior-art` 分支,不会被合并。**
> 关联 ticket: #119(审计:RustDesk 的服务器是否 P2P,能否作为 rdog 跨网互联的先例) · 所属 map: #106
>
> **中文摘要**
>
> - **首要更正:任务简报的前提错了。** 简报假设 RustDesk 是"自研 UDP 打洞 + 自研 TCP 中继、**不用**标准 ICE/STUN/TURN、**不用** webrtc crate"——这对 2026-10 的 master **不成立**。RustDesk 现在**用了纯 Rust 的 `webrtc` crate**(webrtc-rs 0.13,fork 到 `rustdesk-org/webrtc` rev `49c89bd8`),走**标准 ICE**,且 `hbb_common` 的 `webrtc` feature 在 `rustdesk` 的 `Cargo.toml` 里**默认开启**。实际是**三条直连 lane 并存**(自研 UDP 打洞 / 自研 TCP 打洞 / 标准 ICE-WebRTC)+ 一条自研 TCP 中继兜底。这个混血状态本身就是给 rdog 的警示。
> - **服务器不是 P2P,是中心化的。** `hbbs` 只做信令与引荐、**不承载 payload**;`hbbr` 是 **blind 字节中继**(只做两腿 `send_raw` 拷贝、不解密)。**但拓扑正是 rdog 要的形状**:中心化 rendezvous + P2P 优先 + 中继兜底,已在百万级用户规模跑了 5 年以上 → **拓扑层是强先例**。
> - **它不化解 #110 的冲突,只是绕开。** RustDesk 服务器端 grep 全仓 **0 命中** TURN/STUN 实现,`Cargo.toml` 无相关依赖 → 它没有回答"如何在单二进制里提供 ICE relay candidate"。它的实际答案是 #110 的**候选方向 3**(ICE 只用于发现直连,兜底走自研 relay)——这为候选 3 提供了**生产验证**,但它不是 TURN 实现。而且 RustDesk 服务器是**一个 Cargo 包、两个进程**(`hbbs` + `hbbr`),对"只跑 rdog 一个二进制"**零先例**。
> - **它化解了 #108 的 SCTP 尺寸担忧。** RustDesk 在 SCTP data channel 上跑**应用层分片/重组**(1 字节片头 + 60000 B 片载荷 + 64 KiB 收缓冲),跑出与 TCP lane 同级的 **≈1 GiB 逻辑消息上限**(源码注释明写 "Kept at parity with TCP on purpose"),且**只接受 ordered + fully reliable,否则拒绝绑定该 channel**。→ rdog 的 8 MiB 行上限**不是** WebRTC lane 的阻塞项,需要的是约 100 行的传输层分帧层。
> - **NAT 穿越层:RustDesk 是反例,不是先例。** 它的 NAT 分类只有 3 值,判据是"两条 TCP 连接的源端口是否相同";**只要任一端判为 SYMMETRIC,服务器直接放弃 v4 UDP 打洞、强制走中继**——不做端口预测、不做 peer-reflexive 推测。它没有解决 rdog 要求的"真 P2P 是硬需求"在对称 NAT / CGN 叠加下的可达性。
> - **加密信任模型:强先例 + 强警告。** 中继确实 blind(peer↔peer 会话密钥用 `crypto_box_seal` 封装给对端 X25519 公钥,中继只见密文),但 **rendezvous 服务器是对端身份的信任根**(签 `IdPk{id, pk, dtls_fingerprint}`)→ **云节点一旦被攻破就能替换对端公钥完成 MITM**。rdog 必须把**策略权威**与**身份权威**在云节点上分离。
> - **最贵的一条教训:lane 选择的粘性降级。** rustdesk#16605(2026-10-19 报,次日修):**一次直连失败会让该 peer 永久降级为 relay-only**——`direct_failures` 只在状态跳变时更新、中继会话永不重置它,且直连超时被压到 **1000 ms 地板(比一次 SYN 重传的 1 s RTO 还短)**。实测代价:一次 10 小时会话烧掉 **3.9 GB** 中继流量。
> - **反直觉的一条:自建部署默认关掉直连。** RustDesk 把 UDP 打洞 / IPv6 打洞 / WebRTC 三个直连 lane **默认关闭**,除非 rendezvous 服务器是 `*.rustdesk.com`(`common.rs:1263` `is_public()` + `:1303`)。→ **RustDesk 只在"服务器不是我自己的"时才信任直连路径**;rdog 的云节点是 rdog 自己的,**不能照抄这个默认**。
> - **策略两级语义可直接对位 rdog。** `force_relay`(**硬**,服务器下发 flag,禁用所有直连 lane)vs `policy_relay`(**软**,只影响 ICE transport policy)——这正是 rdog "direct-only / relay-allowed" 需要的三态里的两态,且强制点在**服务器侧下发**,与 rdog 约束 12(云节点持有权威策略、控制端不能覆盖)**完全一致**。
> - **webrtc crate 的变动风险已被实证。** RustDesk 因 0.14+ 需要 `usize::is_multiple_of`(rustc ≥ 1.87)而其 CI 锁 1.75,**把 webrtc 钉在 0.13 并 fork pin**。→ #109 里"webrtc 0.21 API 易变"不是杞人忧天,真实项目已经为此付出长期 pin-fork 负担。
> - **安全面实证。** UDP 打洞请求可被用作反射/放大 DDoS(rustdesk-server#670;其引用的 `CVE-2026-30784` **已被 CNA 撤回**,不可当活跃 CVE 引用);1 GiB 帧上限可打崩 hbbs/hbbr(#712);中继配对竞态实测失败率 ≈12%(#708);OSS `hbbs`↔客户端链路的服务端加密**仍是 open PR**(#691/#699/#706)。
> - **最关键的数据缺口:直连成功率没有任何公开数字。** 官方只有 "in the majority of cases, hole punching is successful" 这种定性说法,没有 benchmark、论文或系统性统计。→ **#109 的"用证据排除另外两个"在成功率维度无法只靠 RustDesk 满足,必须自建实测(#117)。**

# RustDesk 架构核验报告（对 rdog 跨 NAT 设计的证据输入）

> 核验时间2026-10-20 ｜ 核验对象：`rustdesk/rustdesk` @ `master`（`4b92080e3e`, 2026-10-20）、`rustdesk/hbb_common` @ `5d52530b636fb296a324260b074780cf559e0093`（子模块）、`rustdesk/rustdesk-server` @ `master`
> 方法：直接读源码 + 官方文档 + GitHub issue/PR/NVD 交叉验证。**未修改任何 rdog 仓库文件。**

## 0. 先纠正任务简报里的一个前提（重要）

任务简报假设"RustDesk = 自研 UDP 打洞 + 自研 TCP 中继，**不用**标准 ICE/STUN/TURN，**不用** webrtc crate"。**这个前提对当前 master（2026-10）已经不成立**：

- RustDesk 现在**用了纯 Rust 的 `webrtc` crate**（webrtc-rs 0.13，fork 到 `rustdesk-org/webrtc` rev `49c89bd8`），走**标准 ICE**。
- `hbb_common` 有 `webrtc` feature（`dep:webrtc`），`rustdesk` 的 `Cargo.toml` **默认开启**：`hbb_common = { path = "libs/hbb_common", features = ["webrtc"] }`。
- `hbb_common/src/webrtc.rs` 有 2844 行，导入 `webrtc::ice_transport::ice_server::RTCIceServer`、`RTCIceTransportPolicy` 等标准 ICE 类型；默认 4 个 STUN 服务器（`webrtc.rs:164`）。
- 但 **TURN 仍然是可选的、外部配置的**（`OPTION_ICE_SERVERS`，格式 `turn://user:pass@host:port`，`webrtc.rs:402-440`），RustDesk **自己的服务器不实现 TURN**（见下）。

也就是说：RustDesk 现在是**三条直连 lane 并存**（自研 UDP 打洞 / 自研 TCP 打洞 / 标准 ICE-WebRTC），**外加一条自研 TCP 中继 fallback**。这个混血状态本身就是给 rdog 的重要警示。

---

## 1. 结论（Verdict）

1. **RustDesk 的服务器不是 P2P，是中心化的。** 它由两个可执行文件组成：`hbbs`（ID/rendezvous 信令服务器）和 `hbbr`（中继服务器）。`hbbs` 只做信令与引荐，**不承载会话 payload**；`hbbr` 是**明 agnostic 的字节搬运工**（blind byte relay），只做两腿之间的 `send_raw` 拷贝，不解密。
2. **但它的"拓扑"正是 rdog 要的形状**：中心化 rendezvous + P2P 优先 + 中继兜底，且这条形态已在百万级用户规模上跑了5 年以上。**在拓扑层，RustDesk 是强先例。**
3. **它不dissolve #110 的 TURN/单二进制冲突**，只是**绕开**了：RustDesk 的答案是"ICE 只用于发现直连候选，兜底走自己的 TCP 中继"，而且它的服务器**一个 TURN/STUN 服务端都没有实现**。RustDesk 服务器是**一个 Cargo 包、两个进程**（`hbbs` + `hbbr`），所以它对"只跑一个二进制"**不提供任何先例**。
4. **它确实dissolve 了 #108 里 SCTP 尺寸那一条**：RustDesk 用**应用层分片/重组**（1 字节片头 + 60000 字节片载荷 + 64 KiB 收缓冲）在65536 字节的 SCTP data channel 上跑出**与 TCP lane 同级的 ~1 GiB 逻辑消息上限**（`webrtc.rs:143/146/159`）。rdog 的 8 MiB 行上限**不是 WebRTC lane 的阻塞项**，需要的是一个 ~100 行的传输层分帧层。
5. **在 NAT穿越机制层，RustDesk 不是可抄的先例，而是反例**：它的 NAT 分类只有 3 个值（UNKNOWN/ASYMMETRIC/SYMMETRIC），判据是"两条 TCP连接的源端口是否相同"；**只要任一端判为 SYMMETRIC，服务器就直接放弃 v4 UDP 打洞、强制走中继**，不做端口预测、不做 peer-reflexive 地址推测。它没有解决 rdog 要求的"真 P2P 是硬需求"在对称 NAT / CGN 叠加场景下的可达性。
6. **在加密信任模型层，RustDesk 是强先例也有强警告**：中继确实"瞎"（peer↔peer 会话密钥用 `crypto_box_seal` 直接封装给对端的 X25519 公钥，中继只见密文），**但 rendezvous 服务器是对端身份的信任根**（它签署 `IdPk{id, pk, dtls_fingerprint}`）。**云节点一旦被攻破，就能替换对端公钥完成 MITM**——这正是 rdog "云节点持有权威策略"设计最该警惕的耦合。
7. **最贵的一条教训是 lane 选择的粘性**：RustDesk 刚在 2026-10-20 修掉一个 bug（#16605）——**一次直连失败会让该 peer 永久降级为 relay-only**，因为 `direct_failures` 计数器只在状态跳变时更新、且直连超时被压到 1000 ms 地板（比一次 SYN 重传的 1 s RTO 还短），中继会话永不重置它。rdog 的 lane 选型状态机必须把这条当反面教材写进设计。
8. **还有一条对 rdog 极其关键、且方向相反的教训**：RustDesk 把 UDP 打洞 / IPv6 打洞 / WebRTC 三个直连 lane **默认关闭**，除非 rendezvous 服务器是 `*.rustdesk.com`（`common.rs:1263` `is_public()` + `common.rs:1303`）。也就是说 **RustDesk 只在"服务器不是我自己的"时才信任直连路径**；自建部署默认只跑 TCP 打洞兜底。rdog 的云节点是 rdog 自己的，**不能照抄 rustdesk.com 的默认**。

---

## 2. 证据表

### 2.1 拓扑

| 断言 | 来源 | 状态 |
|---|---|---|
| 服务器是两个可执行文件：`hbbs`（ID/rendezvous）与 `hbbr`（relay），同一 Cargo 包 | `rustdesk-server/Cargo.toml` `[[bin]] name = "hbbr"`；`src/main.rs`（hbbs）；`src/hbbr.rs` | ✅ 已核验 |
| `hbbs` 端口：TCP 21116、21115（NAT test）、21118(WS)、21114(Pro HTTP API)；UDP 21116 | `hbb_common/src/config.rs:120-123`；<https://rustdesk.com/docs/en/self-host/> | ✅ 已核验 |
| `hbbr` 端口：TCP 21117、21119(WS) | 同上 | ✅ 已核验 |
| `hbbs` **只做信令/引荐，不承载会话 payload**（转发 PunchHole*/RequestRelay/RelayResponse/IceCandidate，并把 `relay_server` 地址派发给双方） | `rustdesk-server/src/rendezvous_server.rs:541-573`（RequestRelay 转发）、`:674-684`（PunchHoleSent 转发）、`:770-809`（relay_server 选择） | ✅ 已核验 |
| `hbbr` 是**纯字节中继**，按 `uuid` 配对两腿，双向 `send_raw` 拷贝，**无任何加解密** | `rustdesk-server/src/relay_server.rs:461-497`（配对）、`:500-602`（`relay()` 数据面） | ✅ 已核验 |
| 非 WS 双腿时切到 **raw 模式（完全无分帧，纯字节流）** | `relay_server.rs:476-480`（`peer.set_raw(); stream.set_raw();`）+ `hbb_common/src/bytes_codec.rs` 的 `raw` 分支 | ✅ 已核验 |
| 官方口径："多数情况下打洞成功，中继服务器根本用不上" | <https://rustdesk.com/docs/en/self-host/> | ✅ 已核验（**仅定性，无数字**） |
| 官方承认只暴露 443 时"只能走 WSS 中继，**直连不可用**" | 同上（Ports Required 节） | ✅ 已核验 |
| 官方 wiki："Rendezvous mediator implements client side of **rendezvous protocol**" | <https://github.com/rustdesk/rustdesk/wiki/How-does-RustDesk-work%3F> | ✅ 已核验 |

### 2.2 NAT 穿越机制

| 断言 | 来源 | 状态 |
|---|---|---|
|协议层是**经典 rendezvous 协议**（非 ICE）：`PunchHoleRequest`/`PunchHole`/`PunchHoleSent`/`PunchHoleResponse` | `hbb_common/protos/rendezvous.proto:31-47, 73-84, 102-111, 136-157` | ✅ 已核验 |
| NAT 类型只有 3 值：`UNKNOWN_NAT=0, ASYMMETRIC=1, SYMMETRIC=2` | `rendezvous.proto:96-100` | ✅ 已核验 |
| NAT 判据：同一本地地址向 rendezvous 的**两个不同端口**发 TCP，比对服务器观测到的源端口；相同→ASYMMETRIC，不同→SYMMETRIC | `rustdesk/src/common.rs:705-756` | ✅ 已核验 |
| 走 SOCKS / WS 时**强制置为 SYMMETRIC** | `common.rs:674-679` | ✅ 已核验 |
| **任一端为 SYMMETRIC → 服务器放弃 v4 UDP 打洞，直接走中继**（无端口预测、无 peer-reflexive 推测） | `rustdesk/src/rendezvous_mediator.rs:1091-1099`（`let relay_v4 = ph.nat_type == SYMMETRIC \|\| Config::get_nat_type() == SYMMETRIC`） | ✅ 已核验 |
| UDP 打洞实现：服务器向对端**观测到的地址**发 `PunchHoleSent`，然后在同一 socket 上监听，再交 KCP | `rendezvous_mediator.rs:1198`（`punch_udp_hole`）、`:1464`（`udp_nat_listen`，`KcpStream::accept`） | ✅ 已核验 |
| TCP 打洞是**兜底 backstop**：所有直连 lane 都关掉时它仍然跑 | `rustdesk/src/client.rs:361-366`（`tcp_punch_allowed()`） | ✅ 已核验 |
| **标准 ICE/WebRTC lane 已存在**：用纯 Rust `webrtc` crate 0.13，默认 4 个 STUN | `hbb_common/Cargo.toml`（`webrtc = { version = "0.13.0", optional = true }`）；`hbb_common/src/webrtc.rs:26-39, 164-169` | ✅ 已核验 |
| TURN **仅外部可选配置**，RustDesk **不内置** TURN；`Relay`-only ICE policy 需要 TURN，否则拒绝建 offerer | `webrtc.rs:402-440`（`get_ice_server_from_url`）、`:489`（`has_turn_server`）、`rustdesk/src/client.rs:610-621` | ✅ 已核验 |
| **RustDesk 服务器端零 TURN/STUN 实现** | 全仓 `rustdesk-server` grep `turn_server/allocate_credential/refresh_permission/stun_server` = 0 命中；`Cargo.toml` 无 turn/ice/webrtc/stun 依赖 | ✅ 已核验 |
| SDP 与 trickle ICE 候选**走 rendezvous 通道**（云端是信令服务器） | `rendezvous.proto:46`（`webrtc_sdp_offer`）、`:110`（`webrtc_sdp_answer`）、`:276-281`+`:308`（`IceCandidate`） | ✅ 已核验 |
| **实网直连成功率**：官方只有"多数情况"这种定性说法，**未找到任何数字** | <https://rustdesk.com/docs/en/self-host/>；全仓/全 issue 检索 | ⚠️ **未核验 / 无公开数据** |
| NAT rebinding 恢复（ICE restart 等） | 在 `client.rs` / `rendezvous_mediator.rs` / `webrtc.rs` 中**未找到**显式 ICE-restart / rebinding 恢复逻辑 | ❌ **未找到实现** |

### 2.3 中继侧

| 断言 | 来源 | 状态 |
|---|---|---|
| 中继传输 = **TCP**（+ WebSocket 变体） | `rustdesk-server/src/relay_server.rs:86-88`（`Listening on tcp` / `on websocket`） | ✅ 已核验 |
| 分帧：1–4 字节变长头（`len<<2 \| head_len-1`），**`MAX_FRAME_LENGTH = 0x3FFF_FFFF`（≈1 GiB）** | `hbb_common/src/bytes_codec.rs:8, 47-78, 120-141` | ✅ 已核验 |
| 帧头上限可配（`set_max_packet_length`），越限在**头部**就死，避免缓冲整帧 | `bytes_codec.rs:63-66`（`max_packet_length` 检查在 `decode_head` 内） | ✅ 已核验 |
| 预分配上限 256 KiB，防"宣称巨大帧逼你分配" | `bytes_codec.rs:6, 68-74` | ✅ 已核验 |
| peer↔peer 加密：`secretbox`（XSalsa20-Poly1305），会话密钥用 `crypto_box_seal` 直接封装给**对端**的 X25519 公钥 | `hbb_common/src/tcp.rs:382-424`（`Encrypt::dec/enc/decode`）、`rustdesk/src/common.rs:2397+`（`create_symmetric_key_msg`） | ✅ 已核验 |
| 双向子密钥派生（BLAKE2b generichash + KX transcript） | `tcp.rs:348-380`（`new_split` / `derive_subkey`） | ✅ 已核验 |
| **中继是 blind 的**：只搬密文 | `relay_server.rs:500-602`（数据面无 crypto）+上述端到端加密 | ✅ 已核验 |
| **但 rendezvous 服务器是对端身份的信任根**（签 `IdPk{id,pk,dtls_fingerprint}`），客户端用服务器 Ed25519 公钥验签 | `rendezvous.proto:4-13`；`rustdesk/src/common.rs:2367-2388`（`decode_id_pk_dtls`）、`rustdesk/src/client.rs:1652-1700`（`rs_pk = get_rs_pk(key.is_empty() ? config::RS_PUB_KEY : key)`） | ✅ 已核验 |
| **无可信身份 → 会话降级为未认证**（`is_secured()` false，明文路径） | `client.rs:1688-1698`（"falls back to non-secure"） | ✅ 已核验 |
| WebRTC 通道额外做 **DTLS 指纹绑定**（对端把指纹签进 IdPk，不匹配即 fail-closed，判定为 MITM） | `rendezvous.proto:7-10`；`client.rs:1709-1720`；`common.rs:2390-2395`（`dtls_fingerprint_bound`） | ✅ 已核验 |
| 中继限速：单会话 128 MB/s、总 1 GB/s、惩罚档 32 MB/s；超 66 % 阈值持续 1800 s 后降速；30 s 空闲超时 | `relay_server.rs:40-44, 108-160, 562-566, 587-598` | ✅ 已核验（**单位标注与实现不一致，见 §5**） |
| 中继成本量级：一次 10 小时会话约 **3.9 GB 出向流量**（被迫走中继所致） | rustdesk/rustdesk#16605 | ✅ 已核验（用户自述，非官方统计） |

### 2.4 SCTP / 消息尺寸（对 #108 的关键修正）

| 断言 | 来源 | 状态 |
|---|---|---|
| RustDesk **在 SCTP data channel 上跑应用层分片**：1 字节片头（`FRAG_MORE=1`/`FRAG_END=0`），片载荷 60000 B，收缓冲 64 KiB | `hbb_common/src/webrtc.rs:138-152` | ✅ 已核验 |
| **逻辑消息上限 = `MAX_FRAME_LENGTH`（≈1 GiB），与 TCP lane 对齐**，注释明写"Kept at parity with TCP on purpose" | `webrtc.rs:154-159` | ✅ 已核验 |
| 必须 **ordered + fully reliable**，否则**拒绝绑定该 channel**（片头无序号，重排/丢包会静默粘包） | `webrtc.rs:741-751` | ✅ 已核验 |
| 官方默认 STUN 列表的教训："Four networks, not four names"——同 host 的多个 URL 不提供冗余（曾经同一毫秒一起挂） | `webrtc.rs:155-163` | ✅ 已核验 |

### 2.5 lane 选型 / fallback 阶梯

| 断言 | 来源 | 状态 |
|---|---|---|
| 有专门函数把"直连优先"做成不变量：P2P 结果直接赢，**中继结果被 hold 住一个 preference window** 等直连 | `rustdesk/src/client.rs:215-338`（`race_transports_prefer_webrtc`），窗口时长由 `OPTION_RELAY_FALLBACK_DELAY` 配置（`:641-646`） | ✅ 已核验 |
| 区分 **force_relay（硬）** 与 **policy_relay（软）**；ICE policy 跟 policy 走而非 force | `client.rs:527-530, 610-621` | ✅ 已核验 |
| `direct_failures` 计数器**只在跳变时更新**、且中继会话不重置 → 单次失败即永久 relay-only | `client.rs:401-409`；rustdesk#16605（2026-10-19 报，2026-10-20 修） | ✅ 已核验 |
| 直连超时被压到 `punch_time_used*6`，**地板 1000 ms < 一次 SYN 重传（1 s RTO）** | rustdesk#16605（引 `client.rs L1419-L1451`，1.5.0）；`CONNECT_TIMEOUT = 18_000`（`hbb_common/src/config.rs:45`） | ✅ 已核验（issue 内日志为证） |
| 打洞并发有上限（UDP/TCP 各 32），超出即"declined" | `rendezvous_mediator.rs:116, 119`（`PunchPool::new(32)`） | ✅ 已核验 |
| **自建服务器时 UDP/IPv6/WebRTC 打洞默认关闭**，只有 TCP 打洞兜底 | `common.rs:1303-1314` + `is_public()`（`:1263`，仅认 `*.rustdesk.com`）；rustdesk#16605 复现步骤第 1 条亦确认 | ✅ 已核验 |

### 2.6 反例 / 成本 / 安全

| 断言 | 来源 | 状态 |
|---|---|---|
| **UDP 打洞请求可被用作反射/放大 DDoS**（伪造源 IP → 服务器向受害者回包），**即使开了 `-k`** | rustdesk-server#670（2026-07-01 报） | ✅ 已核验（缺陷） |
| **该 issue 标题引用的 `CVE-2026-30784` 已被 CNA 撤回（Rejected）**，不是有效 CVE | NVD API：`"Rejected reason: This CVE ID has been withdrawn by its CVE Numbering Authority."`（published 2026-03-05） | ✅ 已核验（**不要引用为活跃 CVE**） |
| 现状：master 的 `handle_punch_hole_request` **已校验 `licence_key`**；且 UDP `PunchHoleRequest` handler 已被移除 | `rustdesk-server/src/rendezvous_server.rs:722-734`；PR #703 "drop unsupported UDP PunchHoleRequest and LocalAddr handlers" | ✅ 已核验 |
| **1 GiB 帧上限可被用来打崩服务器**："memory allocation of 1070071572 bytes failed"，hbbs/hbbr 直接崩 | rustdesk-server#712（2026-10-09） | ✅ 已核验（**与 rdog 8 MiB 上限的动机完全镜像，但方向相反**） |
| **中继配对竞态**：两腿同时到达时 177/200 与 176/200 配对失败（≈12% 失败率），生产 4 个月出现 11 次 | rustdesk-server#708（2026-09-26，仍 open） | ✅ 已核验 |
| WebSocket 空闲 30 s 被服务端单方面关闭 | rustdesk-server#704（2026-10，仍 open） | ✅ 已核验 |
| peer 在线状态判断 i32 溢出，离线 24.9–49.7 天被误报 ONLINE | rustdesk-server#675 | ✅ 已核验 |
| **OSS `hbbs` 尚未实现服务端 `secure_tcp` 密钥交换**（客户端↔服务器链路在 OSS 上仍是明文，除非走 WS/TLS） | 仍 open 的 PR：#691、#699、#706 | ✅ 已核验（PR 状态） |
| 连接层代码长期高频改动 | `src/client.rs` 2024-12 起每月 2–7 次提交（2026-09 达 15 次）；`src/rendezvous_mediator.rs` 同量级。*（`per_page=100` 截断，为下界）* | `gh api repos/rustdesk/rustdesk/commits?path=src/client.rs` | ✅ 已核验（下界） |
| IPv6 P2P **静默退化**（对端收不到 `socket_addr_v6`，全部回落中继，无报错） | rustdesk#16372（2026-09-27 报，次日修） | ✅ 已核验 |

---

## 3. 对比表：RustDesk vs rdog 三条候选 lane

| 维度 | **RustDesk 现状**（三层并存） | **候选 A：zenoh router 中继** | **候选 B：WebRTC + coturn** | **候选 C：自研 UDP 直连 + 自研 TCP 中继** |
|---|---|---|---|---|
| **NAT 穿越** | 三条并存：① 自研 rendezvous-协议 UDP 打洞（KCP over punched UDP）② 自研 TCP 打洞 ③ 标准 ICE/WebRTC | ❌ **零 NAT 穿越**（#107 已核验）；zenoh 只能全量中继 | ✅ 标准 ICE：srflx + prflx + （TURN）relay candidate；ICE-lite 对 NAT 后设备友好 | ⚠️ 等价 RustDesk 的 ①②：symmetric NAT **直接放弃**（无端口预测）；等于把"真 P2P 是硬需求"在对称 NAT 下作废 |
| **中继传输** | TCP 21117 / WS 21119；1–4 字节变长帧头，**上限 ≈1 GiB**；双非 WS 时 raw 纯字节流 | zenoh 自己的传输/framing（zenoh 1.8） | TURN：UDP（RFC 5766）/ TCP / TLS；**或** rdog 自研 TCP 中继做兜底 | 自研 TCP 中继（可复用 RustDesk 的 1–4 字节变长帧头设计） |
| **消息尺寸余量** | ✅ **≈1 GiB**，且 **WebRTC lane 用应用层分片做到与 TCP lane 同级**（`webrtc.rs:143-159`）；rdog 的 8 MiB **不是**障碍 | 需实测（zenoh 内部消息上限未知） | ✅ 可达 8 MiB+（分片重组已验证）；⚠️ SCTP 裸上限 65536 B | ✅ TCP 字节流天然无上限；⚠️ UDP 直连必须自带分片/重组（RustDesk 用 KCP） |
| **加密信任模型** | 中继 **blind**（peer↔peer 用 `crypto_box_seal` 封装给对端公钥）；但 **rendezvous 服务器是对端身份信任根** → 云端被攻破可 MITM；无可信身份时会话降级为未认证 | zenoh 自身加密（需核验是否端到端） | DTLS 端到端 + **指纹绑定到签名身份**（RustDesk 的做法可抄） | 可抄 RustDesk：`crypto_box_seal` + 指纹绑定 + **权威策略与身份信任根分离** |
| **依赖成本** | 客户端：`webrtc` crate 0.13（fork pin 到 rev）+ sodiumoxide + KCP；服务器：**零** webrtc/turn 依赖 | 已有 zenoh 1.8 依赖 | `webrtc` crate（tokio-based，rdog 线程模型冲突，#109 关注点）+ **coturn 独立进程**（违反"单二进制"） | 最低：无新 crate，但**全部自研**，含 NAT 分类、重绑定恢复、拥塞控制 |
| **长期维护负担** | ❌ 高：连接层每月 2–7 次提交；2026 年内 NAT/punch/webrtc/relay 专项修复 ≥8 个（#14449/#14490/#16224/#16225/#16254/#16256/#16372/#16605） | 中：zenoh 上游维护，但 rdog 无 NAT 能力 | 中高：webrtc crate API 变动（0.13→0.14→0.17 已迫使 RustDesk 锁版本并 pin fork）；TURN 另有 300s/3600s 义务 | ❌ **最高**：自研 NAT 穿越是**永久性维护条目**，RustDesk 用 5 年 + 每周级修复证明 |
| **直连成功可能性** | 自建部署**默认只有 TCP 打洞**；官方口径"多数情况成功"（无数值）；symmetric NAT → 100% 中继；IPv6 lane 会静默退化 | **0%（直连）**，永远是中继 | 取决于 ICE：锥形 NAT 高；对称 NAT 需 TURN（=中继，但**被 ICE 统一管理**，省掉 rdog 自研状态机） | 与 RustDesk 同级：锥形高、symmetric 归零 |
| **参考层级** | **拓扑 ✅ / 加密模型 ✅ / lane 状态机 ⚠️ / NAT 机制 ❌ / 中继协议 ✅** | — | — | — |

**一句话对比结论**：RustDesk 证明了"**中心化 rendezvous + P2P 优先 + 中继兜底**"这个**拓扑**成立；它**没有**证明"**在对称 NAT 下还能直连**"——它恰恰选择了放弃。它对 rdog 最大的可复用价值有三块：(1) 中继**必须是 blind 的**且对端身份要**绑定到签名指纹**；(2) WebRTC lane 的**分片重组层**可以做到与 TCP lane 同级尺寸上限；(3) lane 选型状态机必须防"**粘性降级**"。

---

## 4. 对四个 ticket 的具体修正

### #109（直连 lane 技术选型）

**新增证据**：
- RustDesk 用 `webrtc` crate **0.13** 且 pin 到 **fork rev**（`rustdesk-org/webrtc` rev `49c89bd8`），理由是 0.14+依赖 `usize::is_multiple_of`（需 rustc ≥1.87）而他们 CI 锁 1.75（`hbb_common/Cargo.toml` 注释）。**这是对 #109 "webrtc 0.21 API 易变"担忧的直接实证**：webrtc-rs 的破坏性升级**已经**在真实项目里造成了 pin-fork 的长期负担。
- 但 RustDesk **没有**在 rdog 关心的线程模型问题上给出答案——它整个是 tokio-based 的。
- **选型标准应增加一条**："能否支持**应用层分片/重组**以突破 SCTP 65536 B限制"，以及"分片序号/乱序处理是否 fail-closed"（RustDesk 的答案是：只接受 ordered+fully-reliable，否则拒绑 channel，`webrtc.rs:741-751`）。
- **候选集应增补一条**：候选 C（自研 UDP + 自研 TCP 中继）在 RustDesk 那里是**真实存在并跑了 5 年的**方案，不是纸上谈兵——所以 #109 应把它当**并列候选**评估，而不是降级项。但它必须按 RustDesk 的代价定价。

### #110（TURN / 单二进制冲突）

**核心修正**：**RustDesk 不dissolve 这个冲突，它是绕开的。**
- RustDesk 服务器端 **grep 全仓 0 命中** TURN/STUN 实现，`Cargo.toml` 无相关依赖 → RustDesk **没有**回答"如何在单二进制里提供 ICE relay candidate"。
- RustDesk 的实际答案是 #110 的**候选方向 3**："放弃标准 ICE 的 relay candidate，ICE 只用于直连发现，兜底走自研 relay"。**这为 #110 候选 3 提供了生产验证**，但它**不是**一个 TURN 实现。
- **另一条同等重要的观察**：RustDesk 的服务器是**一个 Cargo 包、两个进程**（`hbbs` + `hbbr`）。所以 RustDesk 对"只跑 rdog 一个二进制"**零先例**。若 rdog 坚持单进程，RustDesk 帮不上忙；若 rdog 接受"一个包、两个子命令"（`rdog rendezvous` + `rdog relay`），RustDesk 是完美先例。
- **建议 #110 收敛为**：候选 3（ICE 仅做发现 + 自研 relay 兜底）应升为主选，并明确写死"**不声称实现 RFC 5766**"；候选 1（自研最小 TURN）降为"仅当确实需要 ICE relay candidate 时才做"。
- **必须驳回的一条**："RustDesk 有 webrtc crate 所以 TURN 冲突已解决" —— 不成立，RustDesk 明确把 TURN 当**外部可选依赖**，且 `Relay`-only ICE 在没有 TURN 时**建不出 offerer**（`client.rs:617`）。

### #111（ICE 角色与公网端点）

**新增证据**：
- RustDesk 的 ICE 角色安排：**不做 ICE-lite/full 的区分**，两端都是 full ICE（`webrtc.rs` 用 `APIBuilder` 建 `RTCPeerConnection`），控制端当 offerer（`webrtc_offerer`），受控端当 answerer（`rendezvous_mediator.rs:852-860` "Build the WebRTC answerer for a punch-hole offer"）。
- **公网端点获取有三条独立来源**：① 本地 GUA 发现（IPv6）② **STUN**（`stun.l.google.com:19302` 等）③ 自研打洞时由 rendezvous 服务器回告的对端**观测地址**。RustDesk 的第三种（服务器回告）是标准 ICE 没有的，对 rdog 很有价值：**云节点本来就知道每端的公网映射地址，可以直接下发**。
- **RustDesk 有一个反直觉的实证**：STUN 只给 srflx；RustDesk 自己另外实现了 "Found public IPv6 address **locally**"（不经 STUN，`common.rs:2439`），并且 #16372 显示**这条 IPv6 lane 会静默退化**。
- **对称 NAT 的现实**：RustDesk 的答案是**放弃**。#111 第 3问（"对称 NAT 下 srflx 不可用，没有端口映射还能拿到什么"）在 RustDesk 的答案是：**什么都不做，直接中继**。rdog 若要"真 P2P 是硬需求"，这一格必须靠 **TURN** 或**端口预测**填，RustDesk 帮不上。
- **NAT rebinding 恢复**：RustDesk **未找到**显式 ICE restart 实现 → #111 第 5 问在 RustDesk 处**没有先例**，rdog 需自研或在选型阶段把"ICE restart 支持"列为硬性准入条件。

### #115（direct-only / relay-allowed 策略模型）

**新增证据**：
- RustDesk 已有**两级策略语义**，可直接对位 rdog 的 direct-only/relay-allowed：
  - `is_force_relay()` = **硬**（服务器/管理员下发 `force_relay` 字段，直接禁用所有直连 lane，`client.rs:492, 500, 518, 558`）；
  - `is_policy_relay()` = **软**（"relay-by-POLICY"，影响 ICE transport policy，但不阻止 ICE 开自己的 socket 走直连——`client.rs:527-530` 注释明确说 "ICE opens its own sockets and may still go direct"）。
  - 这正是 rdog "direct-only / relay-allowed" 需要的三态里的两态，**且强制点在客户端**（由服务器下发的 flag 携带），不是客户端本地配置 → 与 rdog 约束 12（云节点持有权威策略、控制端不能覆盖）**完全一致**。
- **强制点的三条落点**（对应 #115 第 4 问）：RustDesk 的落点是 **① 服务器决定是否发 punch（对称 NAT → 只发 relay）② 客户端决定是否启用直连 lane（force_relay flag + 本地 option）③ 客户端在 direct/policy_relay 下决定 ICE transport policy**。**注意：RustDesk 没有"云端拒绝把流量转进 relay"这一条**——中继是兜底而非被拒对象。
- **必须写进 rdog 设计的反面教材（rustdesk#16605）**：
  1. 策略状态**不能只记在"跳变"上**，必须有**衰减/重试**语义——RustDesk 因为 `direct_failures` 只在跳变时更新且中继会话不重置，导致**一次瞬时失败 = 永久中继**（实测 10 小时会话 3.9 GB 中继流量）。
  2. **直连超时地板必须 > 一次 SYN 重传（≥1 s RTO，实际建议 ≥3 s）**，RustDesk 的 1000 ms 地板比 1 s RTO 还短。
  3. **中继建立后应保留一个短窗口继续试直连**，成功则切换（RustDesk 的 2.5 s WebRTC preference window + `OPTION_RELAY_FALLBACK_DELAY` 是可抄的形状）。
- **可观测性**：RustDesk 的 `hbbr` 内建 `usage` 命令（按 IP 打印 elapsed/total/highest/speed，`relay_server.rs:307-330`），`hbbs` 有 `relay-servers` / `ALWAYS_USE_RELAY` 配置。rdog #115 第 5 问（直连失败率如何暴露）**在 RustDesk 处没有对位实现**——只有中继侧带宽统计，没有直连成功率统计。

---

## 5. 未验证 / 开放问题（明确列出）

1. **直连成功率没有公开数字。** 官方只有"in the majority of cases, hole punching is successful"这种定性说法（rustdesk.com/docs/en/self-host/）。未找到官方 benchmark、论文或 issue 中的系统性统计。**任何"RustDesk 直连成功率 X%"的说法目前都没有证据支撑。**
2. **Reddit 讨论未取到**——官方文档引用的 <https://www.reddit.com/r/rustdesk/comments/1cr8kfv/> 被 Reddit 网络安全策略拦截（返回 "You've been blocked by network security"），其中的用户轶事未能核验。
3. **`hbbr` 限速单位疑似不一致**：配置以 "Mb/s" 命名并按 `v * 1024 * 1024` 存储（实为 bytes/s），而数据面 `let nb = bytes.len() * 8`（bits）送进 `limiter.consume(nb)` → 实际速率上限可能是标称的 8 倍。**未实测确认**，只做静态推断，不作为结论。
4. **`MAX_UNAUTHORIZED_MESSAGE` 的具体数值未定位**（定义不在已下载的 `hbb_common/src/{config,tcp,bytes_codec}.rs`，也不在 `libs/base/src/protos/mod.rs`）。只核实到使用点：`rustdesk/src/server.rs:230` 在握手前用 `set_max_packet_length` 限制未授权对端的帧大小（授权后放开）。
5. **RustDesk OSS `hbbs` 与客户端之间的链路加密现状未完整核验**：客户端侧 `secure_tcp` / `key_exchange` 已实现（`common.rs:2254-2321`，服务端用 Ed25519 签 X25519 公钥），但服务端侧的对应实现仍是 open PR（#691/#699/#706）。**OSS 部署下该链路是否真的被加密，未实测**。
6. **RustDesk Server Pro（闭源）是否含 TURN /更好的中继，未核验**——Pro 有 Web 控制台、多中继管理等（官方文档），其内部 NAT 能力不可见。
7. **RustDesk 的 IPv6 打洞是否算"真直连"、在双栈运营商网络下的成功率，未量化**（#16372 只给出"静默退化"这一个可复现故障）。
8. **RustDesk 的 NAT 分类是否有假阴性**：判据只是"两条 TCP 连接的源端口是否相同"。理论上 **UDP 上的对称 NAT 会被误判为 ASYMMETRIC**（TCP 与 UDP 的映射行为可以不同）。未找到 RustDesk 讨论此点的文档或 issue → **这条推断未经 RustDesk 证据证实**，但对 rdog 的 NAT 分类设计是重要警示。
9. **webrtc crate 0.13 → 0.21 的实际破坏性变更面未逐条比对**（只确认了 RustDesk 因 `usize::is_multiple_of` 而锁版本 + pin fork 这一个具体原因）。
10. **RustDesk 从未发布过"自研打洞 vs ICE/WebRTC"的成功率对比**。rdog 选型若以"哪条成功率更高"为依据，**目前没有可引用的数据**——#109 的"用证据排除另外两个"这一产出要求在直连成功率维度**无法仅靠 RustDesk 满足**，需要自建实测。
11. **`direct_failures` 的完整状态机与其修复方式未核验**（#16605 于 2026-10-20 07:57 被关闭，但关闭的修复 commit 未在 timeline 中列出 cross-reference，需读 diff 才能确认 RustDesk 是否采纳了 issue 建议的方案 2/3）。

---

## 6. 建议的下一步（最小、可执行）

1. **立刻做**（不需要新信息）：把本报告 §1 的 5 条结论回填进 #109 / #110 / #111 / #115，并**修正 #108 已被 #109/#110 引用的一处前提**——"SCTP 尺寸使 WebRTC lane 不可行"应改为"需要一个应用层分片/重组层，RustDesk 已给出可抄的实现（`webrtc.rs:138-159`）"。
2. **补一条 RustDesk 不能回答的实测任务**：既然"直连成功率"是 #109 的裁决依据且**无公开数据**，rdog 必须自建 lab（对称 NAT / 锥形 NAT / CGN叠加 / IPv6 / 端口预测）测量三条候选 lane 的成功率——**RustDesk 不能替代这个 ticket**。
3. **若要抄 RustDesk 的 lane 状态机**：抄 `race_transports_prefer_webrtc` 的"直连优先窗口"形状（`client.rs:215-338`）+ `force_relay`/`policy_relay` 二分（`client.rs:527-530, 610-621`），并**显式反着做** RustDesk 的两个错误：不要把"直连超时"压到一次 SYN 重传以下，不要让失败计数器只记跳变且永不衰减。
4. **若要抄 RustDesk 的加密模型**：抄"中继 blind + 对端公钥 `crypto_box_seal` + DTLS 指纹绑定签名身份"三层（`rendezvous.proto:4-13`、`common.rs:2390-2395`），但**必须把 rdog 的"策略权威"与"身份权威"在云节点上分离**——RustDesk 把两者都压在 rendezvous 服务器上，导致云端一旦被攻破即可 MITM，这是 rdog 单一云节点设计不能照抄的一处。
