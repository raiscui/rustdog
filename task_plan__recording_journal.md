# 任务计划: 定义 rdog.recording.v1 Recording Journal 模型

## [2026-07-21 20:43:07] [Session ID: omx-1784512435044-92wxat] [任务启动]: Recording Journal Wayfinder ticket

## 目标

形成可直接进入实施规划的 `rdog.recording.v1` 正式规格,让 Recording Journal 成为 append-only 单一真相源,同时不把 Replay Script、Bundle policy 或实现细节混进模型。

## 阶段

- [ ] 阶段 1: claim ticket,固定问题、依赖和停止条件。
- [ ] 阶段 2: 读取 glossary、已关闭 ticket、现有协议规格和 capture research。
- [ ] 阶段 3: 逐项确认 envelope、ordering、事件族、durability、crash 与兼容性。
- [ ] 阶段 4: 编写并验证正式 Journal 规格。
- [ ] 阶段 5: 发布 resolution、关闭 ticket、更新 map/frontier并收口支线记录。

## 两个方向

1. 不惜代价的完整方案: 自描述事件日志、细粒度 provenance、chunk/hash chain、任意点恢复和丰富演进机制。能力最完整,但会提前引入 Bundle 与存储实现复杂度。
2. 先把边界做对的简化方案: versioned event envelope、严格 append ordering、有限事件族、明确 flush barrier、crash truncation 规则和 unknown-event 兼容。后续可扩展,但不预设分布式日志或复杂索引。

## 做出的决定

- 采用方向 2: 用户已明确要求不要过度设计。只保留能够保证录制保真、故障可判定和 schema 可演进的字段。
- Journal 是唯一 source;Replay Script、在线预览和 Bundle manifest 都是派生产物或外层资产。
- 本 ticket 是 Wayfinder grilling,每次只确认一个真正影响模型的决定点。

## 关键问题

1. 单条记录的最小稳定 envelope 是什么?
2. 物理事件、语义候选、window/display、权限/redaction/gap/mark 如何分族?
3. monotonic 与 wall-clock 时间如何同时表达而不制造双重排序源?
4. 哪些操作构成 durability barrier,crash 后如何识别并截断 incomplete tail?
5. reader 如何处理 schema minor/major 版本和未知事件?

## 停止条件

- 正式规格覆盖 ticket body 列出的全部问题,但不进入 recorder 生产实现。
- Mermaid 图、Markdown、引用和 diff 验证通过。
- Resolution comment、ticket close、map pointer 和新 frontier 均完成动态验证。

## 当前状态

**阶段 1 进行中**: 下一动作是 assign ticket 给当前 GitHub 账号,随后读取依赖规格。

## [2026-07-21 20:44:16] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Journal ticket 已 claim

- [x] Ticket `定义 rdog.recording.v1 Recording Journal 模型` 保持 open。
- [x] Assignee 已动态确认为 `raiscui`。
- [x] 唯一 blocker `调研 macOS 全局操作捕获与权限生命周期` 已 closed。
- [x] 阶段 1 完成。
- [ ] 阶段 2: 读取 glossary、capture research、lifecycle 规格和既有 replay/control 契约。

## 当前状态

**阶段 2 进行中**: 只提取 Journal 必须复用的身份、时间、权限、窗口、语义与故障边界。

## [2026-07-21 20:46:11] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Journal 依赖契约已提取

- [x] 已读取 glossary、lifecycle spec、macOS capture research 和 `rdog-control` skill。
- [x] 已核对 flow、window、observation、semantic、mouse、display scope 与多屏坐标契约。
- [x] 已确认 wall-clock 只属于 Session metadata,`os-logical` 是唯一坐标语义。
- [x] 已确认 short-lived observation ref、window id 和 session display id 不能充当持久身份。
- [x] 阶段 2 完成。
- [ ] 阶段 3: 从 ordering model 开始,逐项确认最小 Journal 模型。

## 当前状态

**阶段 3 进行中**: 第一个决定点是 global append order 与 physical capture provenance 是否使用职责分离的两个 sequence。

## [2026-07-21 23:21:17] [Session ID: omx-1784512435044-92wxat] [决策]: canonical order 与 capture provenance 分离

### 已确认契约

- `journal_seq` 由单一 Journal writer 在成功追加时分配,严格递增,是所有 entry 的唯一 canonical order。
- `capture_seq` 由物理 callback 在 bounded queue 前分配,只用于 gap 检测、semantic candidate 关联和 mark capture boundary。
- `monotonic_ns` 只表达时间与间隔,reader 不得按 timestamp 覆盖 `journal_seq` 重排。
- wall-clock 只保存在 Session 起始 metadata,不在每条 entry 重复保存。
- 两个 sequence 职责不同,不会形成两个排序真相源。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认 canonical on-disk encoding 与最小 envelope 形态。

## [2026-07-21 23:46:33] [Session ID: omx-1784512435044-92wxat] [决策]: canonical Journal 使用 UTF-8 JSON Lines

### 已确认契约

- canonical on-disk Journal 是 UTF-8 JSON Lines,每行一个完整 envelope。
- 第一行是普通 `session_start` entry,使用 `journal_seq:0`,不增加独立文件头格式。
- 每行公共字段固定为 `schema`、`recording_id`、`journal_seq`、`kind`、`monotonic_ns` 和 `payload`。
- `capture_seq` 只在物理事件及其关联记录中出现。
- live Journal 不使用压缩流,保持 append、flush 和 incomplete tail 恢复简单。
- stop 后 Recording Bundle 可以压缩整个 JSONL 文件,但不改变 canonical 内容。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认稳定顶层 event families,不展开每个 payload 的全部字段。

## [2026-07-21 23:50:46] [Session ID: omx-1784512435044-92wxat] [决策]: 固定 9 个稳定顶层事件族

### 已确认契约

- 顶层 `kind` 固定为 `session_start`、`physical`、`semantic_candidate`、`context`、`lane_status`、`redaction`、`gap`、`mark`、`session_terminal`。
- 具体平台事件或动作类型放在 `payload.type`,不为每个 key/window/permission subtype 增加顶层 schema。
- `gap` 独立存在,因为它直接决定 Journal completeness。
- `redaction` 独立存在,用于审计 Secure Input 与安全性未知区间。
- `session_terminal` 只表达 capture boundary 的 `frozen` 或 `failed`,不表达 Bundle commit 状态。
- cancel 删除未提交 Journal;daemon crash orphan 没有 `session_terminal`。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认 Participating Window、app 与 display topology 的 recording-scoped identity/snapshot 策略。

## [2026-07-21 23:51:56] [Session ID: omx-1784512435044-92wxat] [决策]: recording-scoped identity 与完整状态快照

### 已确认契约

- `session_start` 保存 time anchor、profile、初始 lane states 和完整 display topology snapshot。
- app、window、display 使用 recording-scoped key,例如 `app-1`、`window-1`、`display-1`。
- runtime PID、short-lived window id 和 session display id 只作为 observed locator/hint,不作为永久身份。
- display topology 变化时追加完整新 snapshot,不保存 patch。
- Participating Window 首次出现时追加完整 identity、locator、durable selector、outer rect、display 和 state snapshot。
- 窗口 geometry/state 变化时追加完整 observed state,不保存字段级 patch。
- Journal 不保存 observation ref。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认 physical entry 与 asynchronous semantic candidate 的 append-only 关联方式。

## [2026-07-22 00:01:11] [Session ID: omx-1784512435044-92wxat] [决策]: physical 不可变,semantic candidate 后续追加

### 已确认契约

- `physical` entry 在 writer 收到 raw event 后追加,不等待 AX、Web 或窗口语义查询。
- physical payload 保存 backend raw timestamp、normalized input fields、source/target PID、source marker 和 tap generation;敏感字段受 redaction 规则约束。
- `semantic_candidate` 作为后续独立 entry,通过 `capture_seq` 引用物理事件。
- 同一物理事件允许 0 到多个 candidate。
- candidate 只记录事实型 provenance:`source`、`sampling`、`observed_monotonic_ns` 和 `limitations`。
- `sampling` 最小枚举为 `event_time`、`notification_time`、`async_enrichment`。
- 不保存虚假精确的浮点 confidence。
- semantic candidate 不重写 physical entry,也不表示 Replay compiler 已选择该动作。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认 completeness/privacy/control entries 的 transition 语义。

## [2026-07-22 00:02:20] [Session ID: omx-1784512435044-92wxat] [决策]: 控制记录只表达 transition

### 已确认契约

- `lane_status` 只在状态变化时追加,记录 lane、完整新状态、reason、recoverable 和 generation;不写周期 heartbeat。
- `redaction` 使用 enter/exit、cause、scope 和 capture boundary;不保存逐键 marker、键值或 suppressed count。
- `gap` 记录缺失 `capture_seq` range、cause、dropped count、recoverability 和恢复结果。
- required gap 无法恢复时,Session 最终必须 `failed`。
- `mark` 只记录 mark id、label、dedupe key、capture boundary 和 evidence status/ref;不内嵌 screenshot/AX 大对象。
- `session_terminal` 记录 `frozen` 或 `failed`、end capture seq、final lane states、gap/redaction summary 和 reason。

### 当前状态

**阶段 3 进行中**: 下一决定点只确认 durability barriers、pending enrichment drain 和 crash tail validation。

## [2026-07-22 11:40:26] [Session ID: omx-1784512435044-92wxat] [决策]: 三类 durability barrier 与 fail-closed crash tail

### 已确认契约

- 普通 entry 允许 userspace buffering,不逐条 fsync。
- `session_start`、committed `mark` 和 `session_terminal` 是仅有的 durability barriers;成功响应前必须写完完整 JSONL 行并同步文件。
- mark 只等待 physical entries 到 capture boundary,不等待 optional semantic enrichment。
- clean stop 冻结 capture 后排空 physical queue,并在有界时间内排空已接受的 semantic/context work。
- required work 超时或丢失时写 `gap`,terminal 必须 `failed`;optional work 可以记录 degraded 后 `frozen`。
- reader 要求 `journal_seq` 从 0 连续递增。
- crash 只允许忽略最后一个没有换行的不完整 JSONL tail;中间 parse error、重复 sequence 或 sequence gap 一律 corrupt。
- 没有有效 `session_terminal` 的 Journal 是 crash orphan,不恢复、不续写,按 lifecycle privacy-first cleanup 删除。
- 不增加 WAL、hash chain、checkpoint index 或跨重启 resume。

### 当前状态

**阶段 3 进行中**: 最后一个决定点只确认 v1 additive evolution 与 unknown event reader policy。

## [2026-07-22 12:05:01] [Session ID: omx-1784512435044-92wxat] [阶段完成]: schema evolution 与兼容性收口

### 已确认契约

- schema 字符串固定为 `rdog.recording.v1`,不增加 `v1.1` 或重复版本字段。
- v1 内只允许新增 optional fields 或现有 kind 下的新 `payload.type`;不得删除、重命名或改变既有字段语义。
- envelope、ordering、time、coordinate、privacy 或 completeness 发生不兼容变化时升级为 `rdog.recording.v2`。
- 同一 Journal 禁止 mixed-major schema。
- 通用 validator/archiver 可以保留未知 optional field 或 payload subtype 的原始 JSONL 行。
- Replay compiler 遇到未知 `kind` 或未知 `payload.type` 必须 fail closed。
- v1 顶层 9 个 kind 固定;新增顶层 kind 需要 v2,不引入 per-entry critical/advisory 标志。

### 阶段状态

- [x] 阶段 1: claim ticket,固定问题、依赖和停止条件。
- [x] 阶段 2: 读取 glossary、closed tickets 和依赖规格。
- [x] 阶段 3: envelope、ordering、事件族、身份、provenance、durability、crash 和兼容性。
- [ ] 阶段 4: 编写并验证 `specs/rdog-recording-journal-model.md` 和 `AGENTS.md` 索引。
- [ ] 阶段 5: 发布 resolution、关闭 ticket、更新 map/frontier并收口支线记录。

### 当前状态

**阶段 4 进行中**: 按已确认契约写正式规格,不增加未讨论的日志子系统或存储抽象。

## [2026-07-22 12:13:44] [Session ID: omx-1784512435044-92wxat] [阶段完成]: Journal 正式规格通过验证

### 完成项

- [x] 创建 `specs/rdog-recording-journal-model.md`。
- [x] 在 `AGENTS.md` 增加长期知识索引。
- [x] 补清 self-injected event 必须在 canonical `capture_seq` 分配前过滤。
- [x] 两个 Mermaid 图均通过 `beautiful-mermaid-rs --ascii`。
- [x] 6 个 JSON code block 均通过 `jq` parse。
- [x] 18 个 Markdown fence 成对。
- [x] 8 个引用规格均存在。
- [x] `git diff --cached --check` 无输出。
- [x] staged 区只有 `AGENTS.md` 和正式规格。

### 验证证据

- Append flow Unicode 输出: `13704` bytes。
- Barrier sequence Unicode 输出: `11926` bytes。
- 正式规格: `520` 行,未超过 1000 行限制。

### 阶段状态

- [x] 阶段 1: claim 与依赖。
- [x] 阶段 2: 规格研究。
- [x] 阶段 3: Journal 决策。
- [x] 阶段 4: 正式规格与验证。
- [ ] 阶段 5: commit/push、resolution、ticket close、map/frontier和支线记录收口。

### 当前状态

**阶段 5 进行中**: 先发布规格 commit,再用固定 permalink 收口 GitHub ticket。

## [2026-07-22 12:16:03] [Session ID: omx-1784512435044-92wxat] [阶段进展]: 规格与 tracker 已发布

### 已完成

- [x] 正式规格 commit `b677264d75cd6588def6bcf23bbde22c2a1651c0` 已推送到 `origin/main`。
- [x] Resolution comment 已发布: `https://github.com/raiscui/rustdog/issues/10#issuecomment-5041790005`。
- [x] Ticket `定义 rdog.recording.v1 Recording Journal 模型` 已关闭。
- [x] map Decisions so far 已增加一行 Journal pointer。
- [x] topology、Composite 和 Evidence retention 三项 fog 均保留。
- [x] GitHub native dependency graph 已重新查询。

### 新 frontier

- `定义敏感输入脱敏与 Replay 参数模型`。
- `验证语义提升与坐标 fallback 的可行性`。

### 当前状态

**阶段 5 进行中**: 创建支线 WORKLOG,提交并推送四个支线上下文文件,再执行最终远端和工作树复核。
