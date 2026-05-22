## [2026-05-19 09:00:25] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: observation refmap P0 brownfield 事实

## 来源

### 来源1: `specs/rdog-observation-scoped-refmap-plan.md`

- 要点:
  - P0 的明确目标是 observation 内生成 `@eN`、支持局部 drill-down、支持 stale ref、支持 observation header。
  - P1 才是 durable observation state,包括 daemon 重启后的 selector 线索、可审计 metadata 和 ephemeral refmap cache。
  - P2 到 P5 分别是 permanent selector、semantic re-find、`@observe` 和 mouse ref 化。
  - 关键禁令是 stale 不能静默猜、ambiguous 不能静默选、短期 ref 和永久 selector 不能混成一个字段。

### 来源2: `src/control_ax.rs`

- 要点:
  - `AxTarget` 当前字段是 `id` / `process` / `window_title` / `role` / `subrole` / `name` / `description`,没有 `ref` 或 `observation_id`。
  - `AxSnapshot` 当前序列化 `schema`、`platform`、`capture_status`、`permission_status`、`coordinate_space`、计数和 `windows`,没有 observation header。
  - `AxWindow` 和 `AxElement` 当前只有 backend `id`,没有 wire 字段 `ref`。
  - `parse_ax_target()` 当前遇到未知字段会报错,所以 P0 要显式扩展 parser,否则 `target:{ref:"@e1",observation_id:"..."}` 会被拒绝。
  - `resolve_target_id_in_snapshot()` 当前只支持 `id` 和 semantic locator。stale id 被普通 `InvalidInput` 字符串表达,还没有结构化 stale ref 错误。

### 来源3: `src/control_ax/query.rs`

- 要点:
  - `AxFindResponse` 目前没有 observation header,match 只返回 `id`。
  - `AxGetResponse` 目前没有 observation header,返回 `target_id`、`window` 或 `element`。
  - `build_ax_get_response_json()` 当前先在新 snapshot 里解析 target,再返回窗口/元素。P0 要决定 ref 是否解析到 stored observation entry,再用 live id 做当前动作。

### 来源4: `src/control_actions.rs`

- 要点:
  - AX-only 命令通过 `SystemControlActionExecutor` 分发: `@ax-tree`、`@ax-find`、`@ax-get`、`@ax-action`、`@ax-press`、`@ax-set-value`、`@type-text`、`@ax-focus`、`@ax-scroll`。
  - `@screenshot` 不走 action executor,由 `control_core` 直接调用 screenshot producer。
  - 因此 P0 需要给 AX-only 路径和 screenshot 路径同时定义 ObservationStore 接入点,不能只改 executor。

### 来源5: `src/screenshot.rs`

- 要点:
  - `@screenshot include_ax` 会在 `build_accessibility_manifest()` 中捕获 `AxSnapshot`。
  - composite screenshot manifest 的 `accessibility` 字段直接保存 `Option<AxSnapshot>`。
  - 如果 P0 只让 `@ax-tree` 有 ref,而 screenshot manifest 里的 `accessibility` 没有 ref,agent 工作流会分裂。
  - 因此计划建议 P0a 先接 AX-only, P0b 再接 screenshot include_ax,两者都属于 P0 完成范围。

### 来源6: `src/control_session.rs` 与 `specs/control-frame-refactor-plan.md`

- 要点:
  - `ControlPeerSession` 当前是薄 session core,负责 outbound frame queue、dispatch report、request id correlation 和 lifecycle gate。
  - 既有计划也明确它不拥有 screenshot backend、PTY runtime、transport 构造或平台权限探测。
  - P0 的 ObservationStore 不应落在 TCP/WebSocket/Zenoh transport 私有代码里。更稳的是新增 control observation 模块,由 AX/screenshot command path 调用。

## 综合发现

### 推荐 P0 边界

- P0 必须扩展 payload,但保持旧 `id:"pid:.../window:.../path:..."` 兼容。
- P0 的 store 只做进程内 TTL-bound observation refmap。daemon 重启后旧 ref 失效,返回结构化 `OBSERVATION_EXPIRED` 或 `STALE_REF`。
- P0 不做 persistent state dir、selector 索引、semantic re-find、`@observe`、mouse ref 化。只为这些能力预留 header / error / docs 边界。
- P0 应覆盖 `@ax-tree`、`@ax-find`、`@ax-get`、`@screenshot include_ax` 的 observation header 和 ref 输出。
- P0 应覆盖 `@ax-action`、`@ax-press`、`@ax-set-value`、`@type-text`、`@ax-focus`、`@ax-scroll` 的 `target.ref + observation_id` 输入解析和 stale 错误。

### 主要风险

- 如果把 `ObservationStore` 放进 transport,以后 TCP/WebSocket/Zenoh 会分叉。
- 如果把 `ref` 和 durable selector 合并,后续 P1/P2 会失去单一真相源。
- 如果 action ref 解析时重新抓 live snapshot 并偷偷匹配相似元素,会违背 stale 不能静默猜的原则。
- 如果 screenshot include_ax 不跟进,agent 最常用的观察入口会缺少 ref,只能继续依赖 backend id。

## [2026-05-19 09:06:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Architect / Critic 通道尝试

## 来源

### 来源1: OMX advisor

- `omx ask claude --agent-prompt architect` 失败,原因是本地 `/Users/cuiluming/.codex/prompts` 没有 `architect` role,可用 role 只有 `buffer_prompt`。
- `omx ask claude --prompt ...` 失败,原因是 provider 返回 402 insufficient balance。
- `omx ask gemini --prompt ...` 在 120 秒内没有返回内容,被 `timeout` 终止。

## 综合发现

- 外部 consensus review 本轮不可用,不是计划内容被拒绝。
- 最终计划需要保留本地 Architect / Critic 自检结论,并明确这个工具侧限制。
- 后续如果用户要求严格外部 consensus,需要先修复 Claude/Gemini provider 或安装 OMX native agent prompts。

## [2026-05-19 11:55:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: observation refmap P0 实现收束

## 来源

### 来源1: 当前实现与测试

- 要点:
  - `src/control_observation.rs` 已提供进程内 TTL-bound observation store,并返回结构化 `OBSERVATION_EXPIRED` / `STALE_REF`。
  - `AxSnapshot::with_observation("@ax-tree")`、`build_ax_find_response_json()`、`build_ax_get_response_json()`、`build_accessibility_manifest()` 都会写 observation header 和短期 `ref`。
  - `WindowFindResponse` 增加 observation header,`WindowCandidate` 增加 `ref`,并且 `@window-activate` / `@window-close` 可以通过 `target:{ref,observation_id}` 回查。
  - `@ax-focus activate:true` 也可以从 observation ref 回推 window_id,不再只吃 `target.id`。

### 来源2: 运行验证

- 要点:
  - `cargo fmt -- --check` 通过。
  - `cargo test --package rustdog --bin rdog control_observation::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_ax::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_ax::query::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_actions::tests` 通过。
  - `cargo test --package rustdog --bin rdog screenshot::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_window::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_window::macos::tests` 通过。
  - `cargo test --package rustdog --bin rdog control_protocol::tests` 通过。
  - `git diff --check` 通过。

## 综合发现

### 实现口径

- P0 的短期 ref 只属于一个 observation,不能跨重启复用,也不能和永久 selector 混用。
- `@screenshot include_ax` 必须和 AX-only 路径共享同一套 observation 语义,否则 agent 的观察链会裂开。
- `@window-find` 也应该被当成 observation source,不该等到 P1 再说。
