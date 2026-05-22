## [2026-05-21 07:30:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: observation refmap P4 `@observe`

## 来源

### 来源1: `specs/rdog-observation-scoped-refmap-plan.md`

- P4 定位是新增 `@observe` 统一入口。
- `@observe` 目标是把观察操作收束成一个总入口,减少 agent 需要记的命令面。
- P4 bundle 建议包含 observation header、windows summary、refs、selectors、optional manifest / screenshot。
- mode 建议为 `visual`、`ax`、`window`、`hybrid`。
- 明确约束: `@observe` 不是马上替换 `@screenshot include_ax`、`@ax-tree`、`@window-find`。前提是现有命令稳定,并且统一 observation 头部字段已经定义好。
- 风险边界: 不要让 `@observe` 过早取代现有命令,不要把 mouse 变隐藏主路径,不要混淆短期 ref 和永久 selector。

### 来源2: P3 落地记录

- `WORKLOG__observation_refmap_p3.md` 显示 P3 已落地 `@selector-refind`。
- P3 response 已固定 `rdog.selector.refind.v1` 和 `rdog.selector.score.v1`。
- `rebound` 必须带 `fresh_target` + `verify_hint`。
- `blocked` 是正常 response,没有 `fresh_target`。
- stale durable payload 已带 `refind_available`、`refind_command`、`recovery_recipe`。
- `LATER_PLANS__observation_refmap_p3.md` 提醒 P4/P5 应复用 `@selector-refind` scoring contract,不要另造恢复评分。

### 来源3: 当前代码触点

- `src/control_observation.rs:40-55` 已定义 `ObservationHeader`,字段包含 `observation_id`、`session_id`、`created_at_unix_ms`、`ttl_ms`、`scope`、`source_command`、`root`、`ref_count`、`selector_count`。
- `src/control_observation.rs:253-280` 的 `record_observation_with_selectors()` 同时写入 ephemeral refmap 和 durable selector 记录,应继续作为 `@observe` 的记录真相源。
- `src/control_ax.rs:382-430` 的 `AxSnapshot::with_observation()` 已能为 AX snapshot 生成 window / element refs 和 selector drafts。
- `src/control_ax/query.rs:390-422` 的 `build_ax_find_response_json()` 会把 `@ax-find` 结果转换为带 observation 的 response。
- `src/control_ax/query.rs:424-450` 的 `build_ax_get_response_json()` 会为局部 AX get 生成新 observation。
- `src/control_window.rs:83-97` 的 `WindowFindResponse` 已有 `observation`、`snapshot_id`、`matches`。
- `src/control_window.rs:683-714` 的 `attach_window_observation()` 已为 window candidates 分配 refs 和 durable selector drafts。
- `src/screenshot.rs:200-225` 的 screenshot AX path 已把 `@screenshot include_ax` 转成 AX observation。
- `src/screenshot.rs:383-390` 和 `src/screenshot.rs:683-704` 显示 screenshot bundle 只在 `@response` 里返回 filename summary,真实 image / manifest 继续走 savefile。
- `src/control_protocol.rs:52-84` 目前没有 `Observe` command variant。
- `src/control_protocol.rs:356-381` 是现有显式命令 parser 入口,新增 `@observe` 应在这里接入。
- `src/control_core.rs:65-110` 显示 screenshot / selector 类读操作由 core 直接 dispatch,其他命令走 executor。
- `src/control_actions.rs:142-148` 明确 selector 类命令不应进入 side-effect executor。P4 `@observe` 也应保持只读,避免进入 action executor。

### 来源4: 结构健康事实

- `src/control_observation.rs` 当前 1271 行,超过项目 Rust 文件健康线。
- `src/control_observation/refind.rs` 当前 922 行,接近健康线。
- `src/control_window.rs` 当前 1090 行,也超过健康线。
- `src/screenshot.rs` 当前 924 行,接近健康线。

## 综合发现

### 推荐方向

- P4 应新增 `@observe` 作为薄 facade,内部组合现有 AX / window / screenshot producer,而不是另建第二套 observation store。
- 推荐 response schema 为 `rdog.observe.v1`。
- 推荐首版只读,不执行 activate、focus、AXPress、set-value、mouse fallback。
- screenshot 文件继续走 `@savefile`,bundle 中只引用 filename / manifest filename,避免把大图塞入 `@response`。
- `@observe` 应只统一入口和 bundle shape,不提前废弃旧命令。

### 必须进入计划的结构前置

- 新增 `src/control_observation/observe.rs`,承载 `ObserveRequest`、`ObserveMode`、bundle builder、summary extraction 和 tests。
- 未来如果实现时继续膨胀,再把 `src/control_observation.rs` 拆为 `store.rs` / `resolve.rs` / `response.rs`,但 P4 首轮至少不能把 `@observe` 塞回大文件。

### P4 不做的内容

- 不做 P5 mouse ref 化。
- 不做 action by selector。
- 不让旧 `@eN` 跨 daemon 重启直接有效。
- 不让 `@observe` 替代 `@screenshot` / `@ax-tree` / `@window-find`。
- 不另造一套 semantic re-find scoring。

## [2026-05-21 07:35:01] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Architect review 后的 P4 硬约束

## 综合发现

### Hybrid ownership

- P4 禁止创建 "merged observation"。
- hybrid top-level `observation` 只能选择一个主 observation,推荐 AX observation。
- window / visual / AX section 可以各自保留 section-level `observation`。
- 所有 ref sample 必须带 `observation_id`,不能只暴露裸 `@eN`。

### Target 语义

- P4 首版 `target` 只过滤 window / AX summary。
- visual screenshot 仍是 virtual desktop / composite screenshot。
- 如果 response 中有 visual section,需要表达 `target_applied:false`,避免 agent 以为截图已裁剪到目标窗口。

### 文件健康线

- `src/control_observation.rs`、`src/control_ax.rs`、`src/control_window.rs` 已超过 1000 行或明显超线。
- P4 实现只能在这些文件做必要接线,新增实质 orchestration 必须下沉到子模块。
- `src/screenshot.rs` 接近 1000 行,若可组合 screenshot producer 需要新增较多代码,应拆出子模块。

## [2026-05-21 17:37:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: observe.rs 分层边界

## 综合发现

### request 层

- `ObserveMode` / `ObserveTarget` / `ObserveRequest` 和 payload parser 可以完整隔离在 `observe/request.rs`。
- `ObserveTarget` 的 `to_window_query()` / `matches_ax_window()` 是 producer 的查询适配边界,应保持为 sibling 可见,不要上升到公共协议 API。

### producer 层

- producer 是唯一应该调用 screenshot / AX / window lower-level producer 的层。
- visual path 仍然只生成 screenshot savefile summary,`target_applied:false` 语义不变。
- primary observation 选择仍保持 AX -> window -> visual 的优先级,visual record error 必须继续显式传播。

### response 和 refs 层

- response 只消费 `ProducedSections`,负责 `rdog.observe.v1` final bundle 和 request id 包装。
- refs 层只做 `section + observation_id + ref` sample 和 selector count,避免和 producer 混在一起形成新的大文件。
