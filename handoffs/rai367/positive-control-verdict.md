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

### 2026-11-28 更正: 上一版「正对照成立」**作废**

> 依据 #201 写进 main 的那段判读, 正对照被记成"已成立"。**该结论作废** —— 它不是措辞问题, 是判据用错:
> join 只能证明"该次尝试两端 delta 都为 0", 而正对照要证明"同一套仪表能测到非零"。缺非零读数就排除
> 不了"计数器没接上", 而那与真零同形。缺证在 RAI-706。**读本文时以本节为准, 上面的「成立」按作废处理。**

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
| 正对照 (证明仪表能测非零) | **已按三句口径收口** (2026-11-28, RAI-706 合入后), 见本文末节 |

**新出现的一条边界 (不是 `iroh_direct_path_lost`)**: C → B 直连首块在 30 秒等 ACK 失败, 而 B 侧
有 262,144 B 半成品且与源前缀哈希一致, 客户端记录 0 个已确认 chunk。这是**单端确认与实写的差异**:
路径没有丢, 是 ACK 没在预算内回来。按记账口径 (只记被对端确认的字节) 客户端记 0 是对的, 不得据此
主张"中途路径丢失"; 该场景的正确后续是**从 262144 续传**, 并单独立票查 ACK 为什么 30 秒没回。

---

## 2026-11-28 收口: RAI-706 落地后的正式口径

RAI-706 (`#208` merge `8b947f11`, 基线 `66e5adaf` 含 #207 直连探针) 已合入。真机读数
(`be0957f2ffb6f310c4e7588cdc9ba1abb952d391`, Linux 客户端 → Windows rais-razer daemon):

| 路径 / 开关 / 请求 | 正式 CLI | daemon 观测到的请求行 | 双端 bulk bytes | join | 目标 / partial |
| --- | --- | --- | --- | --- | --- |
| relay / 关 / put | exit 1 `iroh_direct_path_unavailable` | 0 | 0 / 未取得 | `bulk_path_endpoint_missing`; null | 均无 |
| relay / 开 / put | exit 1 `iroh_direct_path_unavailable` | `@put-file` 102 B + `@file-cancel` 81 B | 0 / 0 | `bulk_path_inconclusive_aborted`; null | 均无 |
| relay / 关 / get | exit 1 `iroh_direct_path_unavailable` | 0 | 0 / 未取得 | `bulk_path_endpoint_missing`; null | 均无, daemon 源未变 |
| relay / 开 / get | exit 1 `iroh_direct_path_unavailable` | `@put-file` 102 B + `@file-cancel` 81 B | 0 / 0 | `bulk_path_inconclusive_aborted`; null | 均无, daemon 源未变 |
| direct / 开 / put | exit 0 | `@put-file` + `@file-chunk` + `@file-end` | 65,762 / 65,762 | `joined`; true | 目标已提交, partial 无 |

**正式口径 (三句, 一个字不放宽)**:

1. relay 上永远没有 bulk 字节是**能力拒绝**的结果, 不是缺测 —— 制品里 bulk 起不到 relay 上;
2. 可核的是 wire 上该请求被拒、两端无目标无 partial、两端 abort 快照按同 attempt 配对、
   `relay_bytes_zero` 保持 `null` (不填零);
3. 仪表的非零能力由**直连路径**证明 (65,762 B 双端 SHA-256 一致、join `true`)。

**② 的覆盖范围**: "两端 abort 快照按同 attempt 配对"只成立于**取证开启**的两组 (`relay / 开` 的 put 与
get), 即上表 `bulk_path_inconclusive_aborted` 那两行。取证关闭的两行在 open 前就被客户端
`BulkPathRecorder::begin` 门禁拒, daemon 侧 0 行请求、没有对端半份, join 只能报
`bulk_path_endpoint_missing` —— 那两行**不**提供配对证据, 也别把 ② 的 "wire 上被拒" 读到它们头上。
四行共有的唯一跨行结论是 `relay_bytes_zero: null`。原始回执不在仓库, 见
`drive://captain-remote-p2p/handoffs/RAI-706/receipts/be0957f-summary.txt`。

**不主张**: "同一套仪表在 relay 上测到了非零" —— 那需要制品允许 relay bulk, 与门禁冲突。

**读这份读数时不能漏的两条**: 客户端 relay 括号两组各涨 TX +3,252 B / RX +3,768 B, 那是取证
open/cancel 帧 (非文件负载), 不能读成"relay 无流量"; 而 relay 先拒那一格 `relay_bytes_zero` 是
`null` 而不是 `true` —— 那次尝试被 abort, 不是"测到了零"。

**仍缺的一条** (不在本收口范围): 传输层 pin, 所以能主张的仍是"每个边界上路径直连", 不是"整条连接从未
经过 relay"; 中途 `direct → relay` 的 `iroh_direct_path_lost` 真机实测仍缺 (住 §8 第 1 条)。
