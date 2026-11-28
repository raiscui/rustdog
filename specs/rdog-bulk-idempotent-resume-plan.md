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

## M1 已提交结果对账 (硬判据)

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
