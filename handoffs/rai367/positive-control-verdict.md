# 正对照判读（RAI-367 §4.4）

> 判据：RUST-14。执行：RUST-5（RAI-366 真机拓扑：云机 × rais-razer）。
> 被测二进制：分支 `capy/rai-367-bulk-gate` head `93089d842`；
> 云机客户端 sha256 `5be64ec1ac313915651b34a782c8a0a8e9318cf3df17a6f0086a3b9d50eb5598`（debug）、
> razer daemon sha256 `3658f78f1650f7c7b9100b95c8b619272e4137044490dc9db61d9fda4b43657a`（release, `--no-default-features`）。

## 结论（一句话）

**门禁成立，仪表未成立。** 真机上 bulk 动词被**动词级**拒于能力码 `iroh_bulk_path_pin_unavailable`，且
该拓扑上数据跳确实可得直连；但**正对照没拿到计数**，所以"bulk 在 relay 上的字节为 0"这一条**仍不做结论**。

## 逐条判读（对着单子的判据表）

| 项 | 原始证据 | 判定 |
| --- | --- | --- |
| 前置：daemon 起得来且目录注册成功 | `11:08:46.679Z … home is now relay https://use1-1.relay.n0.iroh.link./` + `iroh_directory_path=relay` + `iroh daemon registered pty=ping-only`，均早于 11:09:15Z 的客户端读数 | **成立** |
| S2 直连可得 | `s2a.err`: `iroh_path=direct`；`s2b.err`: `iroh_path=direct`；两次调用各自的 `date -u` 是 `11:09:15Z` / `11:09:17Z`，即**两个独立进程各一次**，不是一次 ping 重复打印 | **成立（2/2）** |
| S2 的目录跳与数据跳无关 | 第 2 次同一次调用里 `iroh_directory_path=relay` 与 `iroh_path=direct` 并存 | 成立，符合"目录一跳、数据一跳"的预期 |
| S3 门禁拒绝 | 三个动词各回 `error: iroh_bulk_path_pin_unavailable`、`exit=1`、**没有一条返回 `@response`**（`@file-list` 也没有空列表成功） | **成立** |
| S3 是**动词级**拒绝而非参数错 | 三条动词（含无载荷的 `@file-list`）拿到**同一个**能力码；代码上 `ping_only` 按命令变体判定，早于任何载荷解析（`src/control_iroh.rs` 的 `is_bulk_command`） | **成立** |
| S3 无字节流动 | `accepting`/`reading` 计数 0；daemon 日志 `grep -i pc.bin` 计数 0；任务目录 `.rdog-partial` 计数 0 | 成立（三条独立证据一致） |
| S1 正对照计数 | 未取得（客户端无 relay 句柄，`relay_sample_available=false`；按 RAI-476 待办） | **未成立** |

## 按单子的判读规则得出的结论

单子里写死的那条规则是：**拿不到正对照计数时，S3 的"零"不得用于任何结论。** 因此：

- **可以下的结论**：「bulk 不经过 relay」这条不变量今天由**能力拒绝**保证 —— 真机上 bulk 动词在
  动词级被 `iroh_bulk_path_pin_unavailable` 拒绝，一个字节都没发出去。这不是"默认只允许直连"，
  而是"这条通道根本没有 bulk"。
- **不能下的结论**：「bulk 在直连上完成且 relay 字节为 0」。它需要三件今天没有的东西 ——
  iroh 适配器的 bulk 数据面、`bulk_path_pin=true`、daemon 侧 relay 采样（RAI-476）。

## 记两条方法

1. **"打不通"与"跑不了"在同一份回执里长得一样。** 上一轮失败的真因是二进制来自 #157 之前的
   tree（daemon 起不来、目录没注册），失败串却像网络问题。这条前置检查现在是单子的第一项。
2. **判据不能只看 exit code。** 三个动词都 `exit=1` 还不够 —— 要看**有没有 `@response`**、
   被拒的是**动词**还是**参数**。前者是能力拒绝（我们要的），后者是"参数写错所以拒"，不是证据。

---

## 后续证据推翻本判读的两条结论 (2026-11-27)

新证据: 同源 `334271c` 的真机补证 (RUST-38, 报告 `drive://captain-remote-p2p/reports/RUST-38.md`),
A 云机 → B razer 的 **524,288 B 直连上传两端哈希一致**, 同 attempt `cli-5070` 由官方 join 得
`relay_bytes_zero:true`。代码侧一并核过 (`origin/main`):

| 事实 | 落点 |
| --- | --- |
| 客户端走 iroh 准入 lane 传 bulk + relay 括号采样 | `7f7f83a` |
| daemon 侧出自己那一半快照 (真实 bracket 采样, `endpoint:"daemon"`, `concurrency_suspected:null`) | `src/control_iroh.rs:1030` 附近 |
| 真机 `--put` 首字节 panic 的修复 | `2fa1c4c` |
| 两端 join 的官方实现 | `src/bulk_path.rs` `join_inputs` |

**旧结论不再成立的两条**:

1. ~~「A 类 (bulk 在直连上完成) 不可达: iroh lane 不承载 bulk 数据面」~~ —— 数据面已落地, A 类已有
   读数 (上面那 524,288 B)。
2. ~~「仪表未成立: daemon 侧没有 relay 计数, 快照只有客户端那一半」~~ —— daemon 半已出, join 可做。

**仍然成立、且被这次证据加强的两条**:

- 「bulk 不过 relay 由**能力拒绝**保证」—— 四次自然 relay 回退与一次强制 relay 都在发出 bulk 请求
  **之前**被拒 (`iroh_direct_path_unavailable`), 无目标文件、无 `.rdog-partial`;
- 「没有正对照的零推不出结论」—— 缺 daemon 半的那五次 join 报 `bulk_path_endpoint_missing`,
  不算证据; **而"带了后端 join"也不等于正对照成立**, 见下面 2026-11-28 的更正。

### 2026-11-28 更正: 「正对照成立」这句我写错了

我把"这一轮的零带着后端 join 的结论"读成了正对照成立 —— **两者不是一回事**。正对照要证明的是
**同一套仪表能测到非零** (同链路控制面交互的 relay delta 严格 > 0); 而 join 证明的只是"该次尝试
两端采样 delta 都为 0"。没有非零读数, 就无法排除"计数器根本没接上", 而后者在数值上与真零同形。

RUST-38 报告原文写的就是「出货 `@ping` 也没有同仪表的两端计数字段, 正对照未建立」, 并明确
"不能借成功直连的 join 判据给 abort 判零"。缺证已登记 **RAI-706** (Assignee RUST-5)。

因此本轮准确的口径是:

| 场景 | 能主张 |
| --- | --- |
| A→B 524,288 B **成功直连**那一次 | 两端快照 join 的 delta 为 0 → 「该次尝试 relay 采样为 0」成立 (A 类成立) |
| 五次 **先拒** (自然 relay 回退 + 强制 relay) | 只主张"拒在发出 bulk 请求前、目标无文件、接收端无 partial"; **不**签"双端 relay 零负载" (daemon 半缺失) |
| 正对照 (证明仪表能测非零) | **未建立**, 归 RAI-706 |

**新出现的一条边界 (不是 `iroh_direct_path_lost`)**: C → B 直连首块在 30 秒等 ACK 失败, 而 B 侧
有 262,144 B 半成品且与源前缀哈希一致, 客户端记录 0 个已确认 chunk。这是**单端确认与实写的差异**:
路径没有丢, 是 ACK 没在预算内回来。按记账口径 (只记被对端确认的字节) 客户端记 0 是对的, 不得据此
主张"中途路径丢失"; 该场景的正确后续是**从 262144 续传**, 并单独立票查 ACK 为什么 30 秒没回。
