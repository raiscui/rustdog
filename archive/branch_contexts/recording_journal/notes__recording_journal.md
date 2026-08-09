## [2026-07-21 20:46:11] [Session ID: omx-1784512435044-92wxat] 笔记: Recording Journal 既有契约与首个决定点

## 来源

### `CONTEXT.md`

- 原文: "Recording Journal: Recording Session 产生的 canonical、append-only 操作记录,也是生成其他录制产物的唯一来源。"
- 结论: Replay Script、在线预览和 Bundle manifest 都不能反向成为 Journal source。

### `specs/rdog-recording-session-lifecycle.md`

- 原文: "Mark 是持久化 Journal barrier,不是 best-effort annotation。"
- 原文: "daemon 先提交所有已取得 sequence 的前序 events,再追加 mark entry。"
- 原文: "required lane failure 必须先写 failure/gap marker,再停止 capture。"
- 结论: Journal 必须为 mark、gap、terminal marker 提供与物理事件一致的全局追加顺序。

### `specs/rdog-macos-operation-capture-research.md`

- 原文: "callback 应只构造固定大小或有严格上限的 raw event,赋予 recorder sequence,然后尝试写入 bounded queue。"
- 原文: "wall-clock 只属于 session metadata。事件排序使用 monotonic timestamp + `capture_seq`,避免系统时间调整破坏 journal 顺序。"
- 原文: "`rdog.recording.v1` 必须有 raw event、semantic candidate、status/gap、secure period 和 provenance 记录。"
- 结论: callback sequence 用于物理流 gap/correlation;异步 semantic/status 记录还需要单 writer 的 append order。

### GUI control 规格

- observation ref 只在原 observation 内有效,Journal 只能保存 durable selector 或候选构造信息。
- `window_id` 是 short-lived locator,Participating Window 需要持久 locator/hints,不能把 PID window id 当永久身份。
- `display_id` 第一版稳定性是 `session`,持久记录必须保存 topology 与 hints,不能把 `d2` 当跨重启设备 ID。
- 坐标统一为 `os-logical`;Window Geometry 使用 outer rect。

## 综合发现

### 最小顺序模型候选

- `journal_seq`: 单 writer 在成功追加时分配,严格递增,是所有 entry 的唯一 canonical order。
- `capture_seq`: callback 在物理事件进入 bounded queue 前分配,只用于物理事件 gap 检测、语义候选关联和 mark 的 capture boundary。
- `monotonic_ns`: 记录发生或观察时间,用于时间差与诊断,不能覆盖 `journal_seq` 重排 entry。
- wall-clock 只放 Session header 的 anchor。每条 entry 不重复写 wall-clock,需要展示时由 anchor + monotonic delta 推导。

### 为什么不合并两个 sequence

- physical callback 与 semantic/window/status source 并发产生数据,单个 callback counter 无法诚实表达异步追加顺序。
- 只依赖 JSONL 文件行号无法检测截断、重复或 writer 错序。
- 两个字段职责不同,不是两个排序真相源:`journal_seq` 负责 canonical order,`capture_seq` 只负责物理 capture provenance。

### 最强备选

- 只保留一个全局 sequence,所有 source 先取号再由 writer 等待缺号。字段更少,但 queue drop、source stall 和 barrier 实现会要求 writer 做复杂重排与超时判定,不符合本轮简化约束。

## [2026-07-21 23:21:17] [Session ID: omx-1784512435044-92wxat] 笔记: ordering model 用户确认

### 用户决定

- 接受 `journal_seq` 与 `capture_seq` 职责分离的推荐方案。

### 结果

- 文件追加顺序只有一个真相源:`journal_seq`。
- callback 丢失与 mark capture boundary 继续使用 `capture_seq`,不把异步 semantic/status entry 强塞进物理 counter。
- timestamp 只用于时间解释和诊断,不参与 reader 重排。

### 下一候选

- 推荐 canonical on-disk encoding 使用 UTF-8 JSON Lines,一行一个完整 envelope。
- 第一条也是普通 envelope,使用 `journal_seq:0`、`kind:"session_start"`,不增加独立文件头格式。
- 每行最小公共字段为 `schema`、`recording_id`、`journal_seq`、`kind`、`monotonic_ns`、`payload`;`capture_seq` 仅在有关 entry 中出现。
- stop 后 Bundle 可以压缩 JSONL 文件,但 live Journal 本身不使用压缩流,避免破坏追加与 crash tail 恢复。

## [2026-07-21 23:46:33] [Session ID: omx-1784512435044-92wxat] 笔记: JSON Lines 用户确认

### 用户决定

- 接受 UTF-8 JSON Lines 作为 canonical on-disk Journal。

### 结果

- 首行 `session_start` 与后续事件共享同一 envelope,避免文件 header 与 event record 两套解析路径。
- 每行自描述,可独立审计;live append 和 crash tail 截断不依赖专用二进制工具。
- 性能优化留给 writer buffering 和 stop 后 Bundle compression,不改变 canonical schema。

### 下一候选

- 推荐只定义 9 个稳定顶层 kind:`session_start`、`physical`、`semantic_candidate`、`context`、`lane_status`、`redaction`、`gap`、`mark`、`session_terminal`。
- 具体 event type 放入 `payload.type`,例如 key-down、mouse-up、window-geometry、permission-revoked,避免顶层 kind 随平台 API 持续膨胀。
- `gap` 保持独立 kind,因为它直接决定 Journal completeness,不能埋在通用 status 内。
- `session_terminal` 表达 capture boundary 的 `frozen` 或 `failed`,不表达尚未发生的 Bundle commit 状态。
- cancel 会删除未提交 Journal;daemon crash 留下没有 `session_terminal` 的 orphan,由 startup cleanup 处理。

## [2026-07-21 23:50:46] [Session ID: omx-1784512435044-92wxat] 笔记: 顶层事件族用户确认

### 用户决定

- 用户以“继续”确认采用 9 个稳定顶层 event families。

### 结果

- 顶层 schema 保持有限,平台扩展进入 typed payload。
- completeness、privacy 和 terminal boundary 仍有独立可扫描 entry,不会埋入通用 status。

### 下一候选

- `session_start` 保存初始 time anchor、profile、lane states 和完整 display topology snapshot。
- app、window、display 使用 recording-scoped key,例如 `app-1`、`window-1`、`display-1`;物理和语义 entry 引用这些 key。
- runtime PID/window id/`d2` 只作为 observed locator/hint 保存,不作为跨录制永久身份。
- display topology 很少变化;变化时追加完整新 snapshot,不记录难恢复的 patch。
- Participating Window 首次出现时追加完整 identity/locator/state/outer rect snapshot;后续 geometry/state 变化也追加完整 observed state,不做字段级 patch。
- durable selector 是候选可重找描述,不能保存 observation ref。

## [2026-07-21 23:51:56] [Session ID: omx-1784512435044-92wxat] 笔记: identity 与 snapshot 用户确认

### 用户决定

- 接受 recording-scoped identity + 完整状态 snapshot 方案。

### 结果

- reader 不需要合并 topology/window patch,只需选目标 key 的最后一份完整 observed state。
- runtime locator 仍保留诊断价值,但 replay compiler 必须使用 durable selector/hints 重新解析。
- Journal 没有通用 entity database,只在 `session_start` 和 `context` entry 记录必要快照。

### 下一候选

- `physical` entry 在 writer 收到 raw event 后按 `capture_seq` 追加,不等待 AX/Web/窗口语义查询。
- `physical` payload 保存 backend raw timestamp、normalized input fields、source/target PID、source marker 和 tap generation;敏感字段受 redaction 规则约束。
- `semantic_candidate` 作为后续独立 entry,通过 `capture_seq` 引用物理事件;同一物理事件允许 0 到多个候选。
- candidate 只记录事实型 provenance:`source`、`sampling`、`observed_monotonic_ns` 和 `limitations`,不写虚假精确的浮点 confidence。
- `sampling` 最小枚举为 `event_time`、`notification_time`、`async_enrichment`。
- semantic candidate 永远不重写 physical entry,也不代表 compiler 已选择该动作。

## [2026-07-22 00:01:11] [Session ID: omx-1784512435044-92wxat] 笔记: append-only semantic enrichment 用户确认

### 用户决定

- 接受 raw physical 先追加、semantic candidate 后续按 `capture_seq` 关联的方案。

### 结果

- AX/Web 延迟不会阻塞 raw capture Journal append。
- compiler 可以审计候选来源和采样时机,但 candidate 本身不是 action decision。
- final stop 需要排空已接受的 enrichment work;mark 是否等待 optional enrichment 由 durability/barrier 决策明确。

### 下一候选

- `lane_status` 只在状态变化时追加,记录 lane、完整新状态、reason、recoverable 和 generation;不写周期 heartbeat。
- `redaction` 使用 enter/exit 区间,记录 cause、scope、capture boundary 和聚合 suppressed count;不写敏感值或逐键 marker。
- `gap` 记录缺失 capture range、cause、dropped count、recoverability 和恢复结果;required gap 不可恢复时最终必须 failed。
- `mark` 只记录 committed barrier 的 mark id、label、dedupe key、capture boundary 和 evidence status/ref,不内嵌 screenshot/AX 大对象。
- `session_terminal` 记录 `frozen` 或 `failed`、end capture seq、final lane states、gap/redaction summary 和 reason。
- 控制记录都用 transition/event,不建立周期 snapshot heartbeat。

## [2026-07-22 00:02:20] [Session ID: omx-1784512435044-92wxat] 笔记: transition 控制记录用户确认

### 用户决定

- 接受 lane、redaction、gap、mark 和 terminal 的 transition 模型。

### 结果

- Journal 不依赖 heartbeat 推断健康状态;初始状态来自 `session_start`,最终状态来自 `session_terminal`。
- redaction 只暴露区间和原因,不记录敏感内容或逐键统计。
- gap 是 completeness 的显式证据,required gap 不可恢复时禁止产生成功 terminal。

### 下一候选

- 普通 entry 允许 userspace buffering,不对每条记录执行 fsync。
- 只有 `session_start`、committed `mark` 和 `session_terminal` 是 durability barrier;响应成功前必须 flush 完整 JSONL 行并同步文件。
- mark 等待 physical entries 到 capture boundary,但不等待 optional semantic enrichment;后续 candidate 可引用 mark 之前的 `capture_seq`。
- clean stop 冻结 capture 后排空 physical queue,并在有界超时内排空已接受的 semantic/context work,再追加 `session_terminal:frozen` 并同步文件。
- required failure 在进程仍可运行时追加 gap/lane transition + `session_terminal:failed` 并尽力同步;同步失败仍是 failed,不能伪装为完整 Journal。
- reader 要求 `journal_seq` 从 0 连续递增。crash 时只允许忽略最后一个不完整且没有换行的 JSONL tail;中间 parse error、重复或 sequence gap 一律判 corrupt。
- daemon restart 不恢复或续写 active Journal。没有 valid terminal 的文件始终是 crash orphan,按 lifecycle privacy-first cleanup 删除。
- 不增加 WAL、hash chain、checkpoint index 或跨重启 resume。

## [2026-07-22 11:40:26] [Session ID: omx-1784512435044-92wxat] 笔记: durability 与 crash 用户确认

### 用户决定

- 接受 start/mark/terminal 三类 durability barrier 与 fail-closed crash tail 规则。

### 结果

- 常规吞吐不承担 per-event fsync 成本,但 controller 收到 barrier success 时有明确持久化语义。
- mark 不被 optional enrichment 延迟;stop 负责 drain 并给最终 completeness 结论。
- crash recovery 只负责验证与清理,不恢复 active Session。

### 下一候选

- schema 字符串保持 `rdog.recording.v1`,不增加 `v1.1` 或重复 `schema_version` 字段。
- v1 内只允许新增 optional fields 或新增 `payload.type`;不得删除、重命名、改变既有字段语义。
- envelope required fields、ordering/time/coordinate/privacy/completeness 语义发生不兼容变化时必须升级为 `rdog.recording.v2`。
- 同一 Journal 内所有 entry 的 schema 必须一致,禁止 mixed-major。
- 通用 validator/archiver 可以保留含未知 optional field 或未知 payload subtype 的原始 JSONL 行。
- Replay compiler 遇到未知 `kind` 或未知 `payload.type` 必须 fail closed 为 unsupported,不能静默跳过后生成看似完整的脚本。
- v1 顶层 kind 集合保持固定;新增顶层 kind 需要 v2。这样无需再增加 per-entry critical/advisory 标志。

## [2026-07-22 12:05:01] [Session ID: omx-1784512435044-92wxat] 笔记: schema compatibility 用户确认

### 用户决定

- 确认采用 v1 additive evolution + compiler unknown-event fail-closed 规则。

### 结果

- 阶段 3 的五类关键问题全部有明确答案。
- 顶层 schema 和 reader policy 足以支持正式规格,不再继续追加 grilling 决定点。
- 下一步只把已确认内容整理为长期规格并验证,不实施 Recorder runtime。

## [2026-07-22 12:13:44] [Session ID: omx-1784512435044-92wxat] 笔记: 正式规格验证结果

### 产物

- `specs/rdog-recording-journal-model.md`: `rdog.recording.v1` 正式 Journal model。
- `AGENTS.md`: Recorder journal writer/reader、validator、compiler input 与 orphan cleanup 的阅读索引。

### 验证

- Append flow Mermaid: `exit=0`,Unicode 输出 `13704` bytes。
- Barrier sequence Mermaid: `exit=0`,Unicode 输出 `11926` bytes。
- JSON blocks: `6` 个,全部通过 `jq -s` parse。
- Markdown fences: `18` 个,成对。
- 引用文件检查: `8/8` 存在。
- staged diff check: 无 whitespace error。

### 审阅修正

- self-injected event 不只需要过滤,还必须在 `capture_seq` 分配前过滤,否则会制造没有 gap/redaction 解释的假缺号。
- 该修正只补足已有 self-event marker 契约,没有新增事件族或字段。

## [2026-07-22 12:16:03] [Session ID: omx-1784512435044-92wxat] 笔记: tracker 收口与新 frontier

### Resolution

- 正式规格 permalink: `https://github.com/raiscui/rustdog/blob/b677264d75cd6588def6bcf23bbde22c2a1651c0/specs/rdog-recording-journal-model.md`。
- Resolution comment: `https://github.com/raiscui/rustdog/issues/10#issuecomment-5041790005`。
- Ticket 状态经 `gh issue view` 验证为 `CLOSED`。
- map 同时保留 Journal pointer 和三项 fog。

### Dependency graph

- `定义敏感输入脱敏与 Replay 参数模型`: open、unassigned,两个 blocker 均 closed。
- `验证语义提升与坐标 fallback 的可行性`: open、unassigned,两个 blocker 均 closed。
- 其余 open sub-issues 至少有一个 open blocker。

### 已验证结论

- 当前 frontier 恰好是上述两张 ticket,本 session 不继续 claim。
