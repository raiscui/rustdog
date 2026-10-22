# iroh-crossnet-spike

**wayfinder #124 的原型 spike。** 不进 rdog 主 workspace,独立 crate。

目的:把 [#120](https://github.com/raiscui/rustdog/issues/120) / [#125](https://github.com/raiscui/rustdog/issues/125) 审计里"未核验"的四项,用**可判定的实验**变成实测结论:

| # | 待验断言 | 本 spike 怎么验 |
|---|---|---|
| 1 | `iroh-relay` 能以**库**形式内嵌进 rdog 自己的二进制(单二进制约束) | `relay` 模式:`iroh_relay::server::Server::spawn()` 跑在进程内,不是外部 coturn 进程 |
| 2 | relay 兜底在**只放行 TCP** 的网络里仍能建连 | `client --relay-only`:用 `clear_ip_transports()` 去掉所有 IP 传输,端点失去直连能力,只能走 relay |
| 3 | **一条 8 MiB 真实行**能否穿过 relay | `client --bytes 8388608`:按 rdog 的 `MAX_CONTROL_MESSAGE_BYTES` 构造等长请求行,服务端回等长响应行 |
| 4 | iroh 引入 rdog 的**async 侵入面**有多大 | `src/line_bridge.rs`:把 iroh 的 async QUIC 流桥成 rdog 现有同步 `ControlTransport` 语义的最小胶水 —— **它的行数就是答案** |

## 跑法

```bash
# 1) 云端角色:内嵌 relay
cargo run --bin iroh-spike -- relay --http-bind 127.0.0.1:0
# 输出: {"relay":"http://127.0.0.1:PORT"}

# 2) daemon 角色:绑 endpoint,打印目录服务要下发的内容形状
cargo run --bin iroh-spike -- node --relay http://127.0.0.1:PORT
# 输出: {"id":"...","relay":"...","addr":{"id":...,"addrs":[...]}}

# 3) 控制端角色:连过去发一行读一行
cargo run --bin iroh-spike -- client --relay http://127.0.0.1:PORT --target '<addr-json>'

# 4) 强制 relay 路径 + 8 MiB 行
cargo run --bin iroh-spike -- client --relay http://127.0.0.1:PORT --target '<addr-json>' \
    --relay-only --bytes 8388608
```

## 线程模型(刻意对齐 rdog 现状)

rdog 的 daemon 是**线程式**的,`src/` 里没有 async runtime(`grep -rn "async fn\|tokio::" src/` 零命中)。
所以本 spike 用:**一个独立 tokio 运行时线程承载 async 侧 + 普通 std 线程跑同步侧(行桥)**。
这既是 spike 的真实性要求,也让"接 iroh 要动多少"有形。

行桥的读侧用 `tokio::sync::mpsc::Receiver::blocking_recv()` —— 它在 async 上下文里会 panic,
所以同步角色**必须**在非 async 线程上跑。

## 还没验的(需要真实环境)

- **真实 NAT 域之间的直连成功率**:本机双节点同机,直连必然成功。需要在家用宽带 + 移动网络(CGN)+ 云 VPS 三个真实 NAT 域里各跑一个端点。
- **只放行 TCP 的网络**:本机用 `clear_ip_transports()` 模拟了"没有直连能力",但不是真的 TCP-only 出口(没有验证 UDP 确实被封时的表现)。
- **250ms 延迟预算**:需要真实 WAN 路径。

## 已知环境约束(本机沙箱)

这台机器的沙箱禁止在主工作目录树之外做 rename/unlink,因此:

- `CARGO_HOME` 必须放在主仓库树内(`.tmp/cargo-home`),不能是默认的 `~/.cargo`;
- cargo 的 tar 解包器会因为 `chmod` 被拒而失败,故 crate 由脚本**手工解包**并写 `.cargo-ok` 标记;
- 少数文件名被沙箱按名字拦截(`.gitmodules`、`.vscode/settings.json`、`.mcp.json`),解包时跳过即可(不影响编译)。
