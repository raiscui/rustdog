# spike 实测结果

> 跑测时间:2026-10-22。环境:**本机 macOS (arm64)、loopback、debug 构建**。
> 绝对值不可外推到真实 WAN;**完整性、量级、比值**是有效结论。

## 1. 结论速览

| # | 待验断言 | 结果 |
|---|---|---|
| 1 | `iroh-relay` 能以**库**形式内嵌进 rdog 自己的二进制(单二进制约束) | ✅ **通过** —— `Server::spawn()` 在进程内跑起来,输出 `http://127.0.0.1:PORT`,无 coturn / 无 TURN 进程 / 无 OpenSSL |
| 2 | relay 兜底在"没有直连能力"时仍能建连 | ✅ **通过** —— `clear_ip_transports()` 后只能走 relay,建连成功 |
| 3 | **一条 8 MiB 真实行**能否穿过 relay | ✅ **通过** —— 双向完整,`req_len == resp_len == 8388607`,响应体正确 |
| 4 | iroh 引入 rdog 的**async 侵入面** | ✅ **95 行有效胶水**(`src/line_bridge.rs`,135 行含注释) |

## 2. 延迟矩阵(8 MiB 双向 = 16 MiB 总流量)

每个用例**全新起 relay + node**,避免状态串扰。

| 场景 | elapsed_ms | 备注 |
|---|---|---|
| direct,1 KiB | 10 | |
| **relay,1 KiB** | **4** | **relay 反而更快** —— 直连要先做地址发现/路径选择 |
| direct,1 MiB | 387 | |
| **relay,1 MiB** | **259** | 同上,relay 仍更快 |
| direct,8 MiB | 7061 | |
| **relay,8 MiB** | **10754** | 大 payload 下 **relay 慢约 1.5×** |

**读法**:
- **小 payload:relay 更快。** 因为 relay 路径的连接是预先建立的,直连要先完成地址发现与路径选择。这条在真实 WAN 上同样成立(少一次 RTT 级的协商)。
- **大 payload:直连更快,约 1.5×。** relay 多一层 WebSocket 帧 + 转发。
- **8 MiB 双向在 loopback 上就要 7–10 秒** —— 按 1 Mbps 上行折算,公网上光是**上行**那一半(8 MiB)就要约 67 秒。→ **rdog 的截图负载在真实 WAN 上必须靠直连才能满足 250ms 目标**,这是 #118 延迟预算的关键输入。

## 3. 依赖足迹与侵入面

| 项 | 值 |
|---|---|
| async 桥接胶水 | **95 行**有效代码(`src/line_bridge.rs`) |
| 协议复刻 | 31 行(`src/protocol.rs`) |
| 角色/编排 | 299 行(`src/main.rs`) |
| Cargo.lock 包总数 | **417** |
| 关键依赖 | `iroh 1.4.1` · `iroh-relay 1.4.1` · `iroh-base 1.4.1` · `noq 1.4.0`(quinn 血统)· `netwatch 0.19.3` · `n0-watcher 1.0.1` · `rustls 0.23.46` · `ring 0.17.14` · `ed25519-dalek 3.0.0` |
| **C / OpenSSL 依赖** | **无**(rustls + ring)—— 验证了 #120 的结论 |
| debug 二进制 | 56 MB(release 会小得多) |

## 4. 过程中实测到的问题(留给 rdog 设计)

1. **node 侧同步 worker 不能串行。** 第一版把同步侧写成"一个 worker 依次服务多条连接",实测中**第二条连接被第一条堵死** —— 客户端没干净关闭时,服务端 `read_message()` 迟迟等不到 EOF,后续连接全部排队。→ **rdog 的 daemon 必须并发服务多个控制会话。** 已改为每连接一线程。
2. **ALPN 必须两端都注册。** 只在 `connect` 侧传 ALPN 而不在 endpoint 上 `.alpns(...)` 声明,握手直接失败:`aborted by peer: the cryptographic handshake failed: error 120: peer doesn't support any known protocol`。
3. **`ServerConfig` 是 `#[non_exhaustive]`**,外部 crate 不能用结构体字面量(含 `..Default::default()` 也不行),只能 `default()` 出来再逐字段赋值。
4. **`Server::http_url()` 在 `test-utils` feature 后面**,稳定接口是 `http_addr()`。
5. **`tracing_subscriber::fmt()` 默认写 stdout**,会污染机器可读的 JSON 输出 → 必须 `.with_writer(std::io::stderr)`。

## 5. 还没验的(需要真实环境,见 #124)

- **真实 NAT 域之间的直连成功率** —— 本机双节点同机,直连必然成功。需要在家用宽带 / 移动网络(CGN)/ 云 VPS 三个真实 NAT 域各跑一个端点。
- **真正的"只放行 TCP"出口** —— 本机用 `clear_ip_transports()` 模拟"没有直连能力",**不等于**真的 UDP 被封时的表现。
- **250ms 延迟预算** —— 需要真实 WAN 路径。
- **多会话并发** —— 上面第 4 节第 1 条只是修好了,没有做并发压测。
