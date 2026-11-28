# iroh bulk 同 ID 幂等续传 —— 验收矩阵 (RAI-711)

从 RAI-705 拆出: RAI-705 只保留观测 (`bulk_chunk_ack_unanswered` + daemon 的
`iroh bulk ack …`), 自动重拨整段摘掉。本票要把"丢 ack 之后怎么安全地继续"做成能力。

## 0. 两条已定口径 (Captain 2026-11-28)

1. **对账的主判据是盘上的成品事实, 不是内存表**: 重放请求来时检查目标路径存在、大小等于源大小、
   sha256 等于源 sha —— 三者相等即返回原终态 (已提交), 不重传、不替换。内存表只当快速路径,
   **不得是唯一判据**。盘上不匹配且无内存记录 ⇒ **结果未知**, 不重传。
   - 这个判据天然跨重启, 不需要新表, 也不引入持久状态契约; 代价是只在重放路径上的一次哈希。
   - **假阳性是允许的**: 目标本来就在、内容恰好相同、但不是这次传的 —— 终态不可区分, 按成功返回
     是对的。文档必须写明这个判定语义, 别让人以为它证明了"本次写入发生"。
2. **总 deadline 120 s (初值)**: 路径重拨与 ack 重拨**共享同一条**, 不是各自记账。到期错误必须写清
   哪条预算耗尽 (路径 / ack / 总 deadline)、最后一次观测到的状态 (路径类别、`written`、最后一条
   daemon 行的时间戳), 以及"结果未知"的明确措辞; 不许退化成泛化的连接错误。若真机上 120 s 把正常
   慢链路 (20 MiB @ ~83 kB/s) 判死, 带数据回来调, 不偷偷改成无限。

## M0 证据关联 (先立这条, 否则别格都不算数)

daemon 入站行与 ack 行必须带 **`transfer_id` + `attempt` (序号) + `offset`**, 客户端错误文本带同一组
字段; 三件能按这组字段关联到**同一次**请求。

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 任一次需要重拨的命令 | 入站 `verb=@file-chunk`、出站 `iroh bulk ack …`、客户端错误/进度行三件同 attempt | 只能按"时间接近"猜关联; 或缺任一端 | daemon debug 日志 (`RDOG_LOG_LEVEL=debug`)、客户端 stderr 全文、`--bulk-path-json` 快照 |

字段名与出现位置写死在 `specs/rdog-iroh-admission-contract.md` 的日志小节 (格式一改, 判别流程立刻失效,
所以它属于契约而不是排障细节)。

## M1 已提交结果对账 (硬判据) —— **评审后重做口径**

评审 (#204, `bfd17f3`) 判 M1 地基两处洞, 矩阵按它们改写:

### M1-a 「缓存不作为判据」(severe 1)

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| A 提交后丢回执, B 正常替换/删除了目标, 再跑 A (内存 `COMMITTED` 里仍有 A) | 按**当前文件内容**判定: 目标已不同/不存在 ⇒ 不得报 `committed` | 缓存命中即报成功并清掉本地 ID ⇒ **错误的成功**, 比错误报告更糟 | `@put-file` 响应原文; 目标当前 sha/长度; 本地 store 是否被清 |

**判据**: 内存表最多作提示, **永不作为判据**; 每一次"已提交"结论都必须有当前盘上事实支撑 (顺带解掉缓存无界增长)。

### M1-b 「`Absent` 到提交之间必须互斥 + 原子 no-replace 发布」(severe 2)

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 对账探到 `Absent` 之后、重开拿到锁之前, 旧 `@file-end` 落盘并退役 | 对账放行与 daemon 提交**互斥**; `Absent` 只允许**原子 no-replace 发布** (Linux `renameat2(RENAME_NOREPLACE)`, Windows `MoveFileEx` 不带 `REPLACE_EXISTING`) | 回退到覆盖式 copy; 或发布前不复查目标 | 发布路径与返回值; 目标 mtime/内容前后; 是否出现覆盖 |

### M1-b 落地形状 (Captain 拍板 2026-11-28)

- `no_replace_publish` **只**在"探针说 `Absent` **且**本命令带 `verify_committed`"的会话上置位;
  正常首传 / 显式 `--restart` / 探针不可用的老对端**保持覆盖语义** —— 三条 severe 的防线是
  "重放路径绝不覆盖", 不是"所有上传绝不覆盖"。
- 发布: Linux `renameat2(AT_FDCWD, …, RENAME_NOREPLACE)`、macOS `renameatx_np(RENAME_EXCL)`、
  Windows `MoveFileExW` (不带 `REPLACE_EXISTING`); 目标已存在 ⇒ `file_transfer_replay_raced`,
  **不覆盖、不回退 copy**, 并清掉本次 attempt 的临时文件 (否则暂存目录会被这类冲突喂大)。
- `file_transfer_replay_raced` 语义: 有人在我们 probe 之后落了盘 —— 真实并发, 不是坏数据。
  客户端**不自动重传**, 文本说清"目标在核对之后被占用/替换, 请重跑以重新对账"; 进稳定码总表,
  **自动重试集: 否**。
- **锁的纪律**: 对账的哈希**不许算在 TRANSFERS 锁内** (那会把整条 lane 的提交堵在大文件 sha 上)。
  形状: 锁内取目标身份 (存在性 + 大小 + mtime/inode) → 锁外算 sha → 回锁内复核身份未变 ⇒ 才下结论;
  身份变了就按 `raced` 拒, 不猜。

## M1 原格 (内容判据, 仍然有效)

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 目标已提交, 同 id + 同目标 + 同 size + 同 sha 再 open | `state:"committed"` / `replayed:true` / `written:size`; 不建会话、不碰盘、不重传 | 出现任何 `@file-chunk`; 目标 mtime 变化; 返回 `open`/`resumed` | `@put-file` 响应原文; 目标 mtime 前后; `@file-chunk` 计数 |
| 同 id 但 size / 目标 / sha 任一不符 | `file_transfer_replay_mismatch`, 盘上成品字节不变 | 静默重传; 或改写成品 | 错误原文; 成品 sha 前后 |
| 盘上不匹配、内存也无记录 | **结果未知**, 明确措辞, 不重传 | 报告已提交却无对账依据; 或偷偷从 0 重传 | 错误原文 |
| 无目标、无半成品 | 正常 open (`written:0`) | 报"未知"挡死正常首传 | `@put-file` 响应原文 |

### M1b 混合版本 (协议面)

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 旧 daemon (忽略 `verify_committed`, 按 `serde_json::Value` 解析未知字段不报错) | 回执里**没有** `reconcile` 标记 ⇒ 客户端 **fail closed**: `file_transfer_reconcile_unsupported`, 取消刚开的会话, 一个 `@file-chunk` 都不发 | 静默继续重传 (会把"结果未知"变成"直接重传", 源变过就替换成品); 或假装对账成功 | `@put-file` 响应原文 (无 `reconcile`)、`@file-cancel` 是否发出、`@file-chunk` 计数 |

**可用例**: `a_peer_that_ignores_the_reconcile_request_fails_closed` (替身剥掉 `reconcile` 标记模拟旧 daemon)。

**两条 lane 同一条规则**: 判断写在共享驱动 `run_put_inner` 里, iroh 与 TCP 都带 `verify_committed`、都按 `reconcile` 标记 fail closed; `--restart` 不拦 (它的语义就是显式同意替换)。正常首传 (store 里没有旧 id) 不带这个字段, 升级压力只落在"上一次没干净收尾"的重跑上。

**前提 (M1 的 `Absent` 分支)**: "目标不存在 ⇒ 本命令未提交"依赖**两次尝试之间没有外部删除/移动目标**。
被人为删掉后再重放会让文件重新出现 —— 可接受, 但它是假设不是定理。

## M2 ID 预分配

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 第一次 open 直接失败 (拒绝/超时/连接断) | 同一条命令内第二次尝试用**同一个** `transfer_id` | 第二次是另一个 id | 两次 open 的请求行原文 |

## M3 generation 交接

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 旧 lane 在新 open 之后仍递一块 | `file_transfer_stale_generation`; `written` 与 partial 长度不变; 客户端不再发 `@file-cancel` | 删掉有效 prefix; 或 offset 被打乱 | 错误原文; partial 长度前后; `@file-cancel` 是否出现 |
| 新 generation 接着传 | 从 `written` 续上, 末态 sha 正确 | 重传已确认字节 | `@put-file` 的 `written` 与首条进度行 offset |

## M4 结构化错误匹配

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 对端读走请求、永不回执、连接仍开 | `kind=AckUnansweredLive`（**语义: 本端不知道对端做了什么**）。**不自动重拨** —— 自动重放要等盘上对账 + generation 交接落地 (RAI-705 已裁定该码不进自动重试集) | 仍按文案匹配; 或在对账落地前就自动重拨 | 客户端错误行 (带 kind) |
| 同上但对端已关 | `kind=AckUnansweredClosed` ⇒ 不重拨, 如实报错 | 仍重拨 | 同上 + `close_reason` 行 |
| 文本里含 `iroh_path_unknown` 的无关失败 (负对照) | 不触发重拨 | 触发了重拨 | 尝试次数 |

## M5 每轮独立 attempt ID

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 两轮尝试 (首轮丢 ack、次轮成功) | 两份快照、两个不同 `attempt_id`; 首轮 `bulk_bytes > 0` 仍在 | 首轮快照被覆盖 | 两份 `rdog.bulk-path.v1` 快照原文 |

## M6 确定性 seam (e2e)

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 测试构建, daemon 在首块 execute 之后、写流之前扣住该回执一次, 连接保持开 | 重拨 1 次、id 不变、第二次 open 的 `written` == 首块末尾、首块不重发、最终 sha 正确、`@file-end` 只成功一次、无残留 partial | 重发首块; 两次成功提交; 留下 partial | daemon 全量 debug 日志 + 客户端快照 + 两次 `@put-file` 响应原文 |

## M7 `--restart` 一次性

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| `--restart` 命令中丢一次 ack | `@file-cancel` 只出现一次 (首轮初始化); 恢复轮固定新 id + `resume:true` | 第二次 cancel 删掉首轮已收字节 | `@file-cancel` 计数; partial 长度序列 |

## M8 门禁与上界

| 输入 | 期望终态 | 失败判据 | 需要的原始行 |
| --- | --- | --- | --- |
| 任意重拨路径 | `bulk_path_pin` 仍 `false`; 逐 chunk 检查不变; 单命令尝试次数 ≤ 上限 | 任一格声明变化 | 既有 `bulk_tests` 三格断言 |
| 连续失败 | 触及总 deadline (120 s) 即收场, 错误文本写明已用尝试数、耗尽的是哪条预算与最后观测状态 | 逐项相加导致无上界 | 客户端错误行 + 总耗时 |

## 施工顺序

M4、M2 先行 (结构化错误 + ID 预分配: 改动小、是后面几条的地基), 再 M1、M3 (盘上对账 + generation),
最后 M5、M6、M7 与总 deadline。每格自带单测; e2e 只在 M6 加一条。

## M1 落地形态 (2026-11-28)

- `@put-file` 新增可选字段 `verify_committed:true`。**客户端只在"上一次没干净收尾"时带它** (store 里
  还留着那条 id), 所以正常首传不多一次哈希。
- daemon 判定顺序: 内存 `COMMITTED` 快速路径 → 盘上目标 (存在 + 大小相等 + sha256 相等)。盘上判据
  天然跨重启; 内存表丢了不影响正确性。
- 三种结局: 相等 ⇒ `state:"committed", replayed:true` (客户端 `replayed=committed`, **搬 0 字节**);
  目标存在但不符 ⇒ `file_transfer_replay_mismatch` (不覆盖、不重传); **目标不存在 ⇒ 正常 open**
  (提交必然留下目标文件, 所以"没有目标"是"没提交过"的正面证据, 不是"未知")。
- 假阳性语义在 `replay_state` 的注释里写死: 目标本来就在、内容恰好相同、不是这次传的, 也判已提交 ——
  终态不可区分, 按成功返回是对的, 但它**不证明本次写入发生过**。

# M1 改法 (2026-11-28, Captain 拍板): 放弃"已提交对账", 只保留"绝不覆盖"

## 为什么砍掉这一半

两轮只读评审、每轮 3 条 severe, 全部长在同一个想法上: **"我替你判断这次已经传好了"**。
每补一条就多一个口子 —— 内存缓存说成功而盘上已变(1)、探针与提交之间的 TOCTOU(2)、
哈希期间目标被换而仍报成功(3)、活跃同 ID 会话丢掉 no-replace 标记(4)、daemon 重启恢复会话丢掉策略(5)、
"换掉再换回"绕过按路径的身份复核(6)。这不是没补干净, 是**这个承诺本身要求的东西太多**:
它要求一次跨进程、跨重启、跨文件系统粒度都成立的"这就是那次写入"的证明。

所以砍掉它, 只留一个能证的不变量: **本命令绝不覆盖别人/别次的成品**。

## 新形状

### 客户端

- 只在"store 里还留着这条 (target, local, remote) 的旧 id"时, 在 `@put-file` 里带
  `no_replace_publish: true`; 正常首传不带, `--restart` 一律不带。
- **不再判断"是否已提交"**: 重跑要么正常发布成功, 要么拿到 `file_transfer_replay_raced`
  (目标在核对之后出现)。错误文本要求使用者**用原命令重新对账**, **不自动重试、不续传**。

### daemon

1. 该会话的发布走**内核级原子 no-replace**: Linux `renameat2(RENAME_NOREPLACE)`、
   macOS `renameatx_np(RENAME_EXCL)`、Windows `MoveFileExW` **不带** `MOVEFILE_COPY_ALLOWED`
   (评审 #4: 带它会在跨卷时退化成 copy+delete, 那就不再是原子的)。
2. 该标记**持久化进 session 索引**, 恢复时一并恢复; 恢复不出来或有歧义 ⇒ **拒绝该 open**
   (结构化码), **不默认覆盖**。
3. 同 ID `resume:true` 命中活跃 session ⇒ **只许加强不许降级**: 旧 session 没保护而新 open 要求它
   ⇒ 锁内升级标志; 升不了就拒。
4. 平台没有原子 no-replace 能力 ⇒ **fail closed 拒绝**; **不许**"先查再 rename"的降级。

### 删除的东西 (连带消失的口子)

`replay_state` / committed 与 mismatch 判据 / 全文件哈希 / 哈希前后身份复核 / 内存 `COMMITTED` 表 /
`file_transfer_replay_mismatch` / `file_transfer_replay_unreadable`。因此:

- 不再有"committed 重放"终态 ⇒ 那条"终态缺 daemon 半快照"的问题自然消失, 但报告里要写明;
- 哈希成本、无界缓存、跨重启回放这些议题一并作废 (不是延后, 是不做)。

### 失败分支与判据 (都要有用例)

| 分支 | 期望 | 失败判据 |
| --- | --- | --- |
| 目标不存在 | 原子发布成功 | 发布后目标内容非本次字节; 或临时文件残留 |
| 目标已存在 (撞名) | `file_transfer_replay_raced`, 别人的字节不动, 临时文件清掉 | 覆盖; 或只 warn 后退役会话 |
| 恢复索引时标记丢失/歧义 | **拒绝 open** (结构化码) | 默认按覆盖放行 |
| 活跃会话被要求升级标记 | 锁内升级成功, 或拒绝 | 静默降级为覆盖 |
| 平台无原子能力 | fail closed 拒绝 | "先查再 rename" 降级 |
| `--restart` | 不带该标记, 保持覆盖语义 | 被这条规则拦住 |

要求: 绿测覆盖上面每一条, 且走**真实 handler 与 wire** (不只是 `replay_state` 这种纯函数)。


## 三条补充口径 (Captain 2026-11-28)

### 1. 这次取舍的真实代价: 判断权还给用户

"上次其实已经传完、只是回执丢了"这种情形, 用户现在**不再拿到"已完成"**, 而是
`file_transfer_replay_raced`。错误文本必须直说:

> 目标在核对之后出现, 可能来自上一次已完成的上传; **请先核对目标内容**, 确认要覆盖再用 `--restart`。

这是砍掉已提交对账唯一真实的代价: **我们不再替他判断"已经好了", 把最后一步判断还给他**。
写在这里, 是为了让后来的人知道少掉的那条承诺是有意的, 不是漏了。

### 2. 旧索引条目的兼容口径 (一句话判据)

旧版本写下的索引条目**没有**这个策略字段, 恢复后该字段是 **"未知"而不是 `false`**:

- 普通续传 (新 open 不带 `no_replace_publish`) 照旧放行;
- **只要有任何一次 replay open 要求保护, 而该会话给不出保证 ⇒ 拒 open**, 绝不默认覆盖。

### 3. `--restart` 是"显式同意覆盖"的唯一入口

它**不受**新拒绝逻辑影响, 发布仍走覆盖语义; 判据表里单列一行, 免得后来的人把 `--restart`
也塞进 fail-closed。
