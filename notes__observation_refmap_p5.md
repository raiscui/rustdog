## [2026-05-21 19:48:04] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P5 mouse ref 化 brownfield facts

## 来源

### 来源1: roadmap

- `specs/rdog-observation-scoped-refmap-plan.md:17-18` 把核心原则写成: 短期 ref 负责快,永久 selector 负责稳,语义 re-find 负责恢复,鼠标只是最后兜底。
- `specs/rdog-observation-scoped-refmap-plan.md:304-321` 明确 P5 是最后一层 mouse ref 化:
  - `@click` 接受 `target.ref` 或 `target.selector`。
  - `@drag` 接受 `from.ref` / `to.ref`。
  - `@hover` 接受 `target.ref`。
  - `@wheel` 接受 scroll container 的 `ref`。
  - 坐标保留,但只是 fallback。
- `specs/rdog-observation-scoped-refmap-plan.md:407-415` 明确 P5 目标:
  - mouse 也能尽量复用 observation ref / selector。
  - 坐标只保留为显式 fallback。
  - 鼠标不再是无语义主路径。

### 来源2: 既有 mouse coordinate contract

- `specs/rdog-mouse-control-coordinate-plan.md:5-6` 已固定 mouse 必须复用 `@screenshot` manifest 的 `os-logical` 坐标,不能另造坐标系。
- `specs/rdog-mouse-control-coordinate-plan.md:124-147` 已定义 `@click` 是 `move -> press -> hold -> release` 的安全组合,坐标必须是 `os-logical`。
- `specs/rdog-mouse-control-coordinate-plan.md:149-172` 已定义 `@drag` 的 press/release 恢复边界。
- `specs/rdog-mouse-control-coordinate-plan.md:180-201` 已定义 `@wheel` 可选先 move 到坐标再滚动,并固定 delta 约束。

### 来源3: 非鼠标语义控制边界

- `specs/rdog-non-mouse-semantic-control-plan.md:23-31` 明确鼠标是显式 fallback,不是默认路径,且 GUI agent 先读 `@capabilities` 再决定 AX / mouse / type-text lane。
- `specs/rdog-non-mouse-semantic-control-plan.md:36-86` 已覆盖 `@ax-action` 和 `@ax-set-value` 语义主路径。
- `specs/rdog-non-mouse-semantic-control-plan.md:88-137` 已覆盖 `@type-text` 的 AXValue / targeted keyboard / clipboard 梯子。

### 来源4: 当前 Rust 代码触点

- `src/control_mouse.rs:91-119` 当前 `ClickRequest`、`DragRequest`、`WheelRequest` 只保存坐标字段,没有 ref/selector target。
- `src/control_mouse.rs:403-582` 当前 parser 要求 `@click` 必须有 `x/y`,`@drag` 必须有 `from/to` 坐标点,`@wheel` 只接受可选 `x/y`。
- `src/control_mouse.rs:639-768` 当前 plan builder 直接从坐标生成 enigo plan。
- `src/control_observation.rs:284-289` 已有 `resolve_observation_ref(observation_id, ref_id)`。
- `src/control_observation.rs:367-379` 已有 `stale_observation_ref_error()`。
- `src/control_ax.rs:232-243` 的 `AxTarget` 已支持 `ref_id + observation_id`。
- `src/control_ax.rs:245-290` 已限制 `ref + observation_id` 不能与 semantic locator 混用。
- `src/control_ax.rs:1425-1435` 已把 `target.ref + observation_id` 解析成 AX backend id,并在当前 snapshot 不包含 backend id 时返回 stale。
- `src/control_window.rs:52-58` 的 `WindowCommandTarget` 已支持 `ref_id + observation_id`。
- `src/control_window.rs:753-767` 和 `src/control_window.rs:770-797` 已能解析 window target ref。
- `src/control_actions.rs:119-125` 当前 mouse command 仍由 `control_actions` 直接调用 `build_*_plan(request)`。

### 来源5: 文件健康线

- `src/control_mouse.rs` 当前 1409 行,超过项目 Rust 文件健康线。
- `src/control_ax.rs` 当前 2295 行,`src/control_observation.rs` 当前 1273 行,`src/control_window.rs` 当前 1090 行。
- P5 不能继续把 mouse target 解析、ref 解析、selector 恢复、response metadata 都塞进 `src/control_mouse.rs` 单文件。

## 综合发现

### P5 应采用的主形态

- 增强 mouse request,让 `@click`、`@drag`、`@wheel` 接受 endpoint target:
  - 坐标 endpoint: `{x,y}`。
  - observation ref endpoint: `{ref:"@eN",observation_id:"obs-..."}`。
  - selector endpoint: `{selector_id:"sel-...",auto_refind:true|false}` 或显式 `selector:{...}` 需要计划阶段定边界。
- 编译期和 response 层要说清楚 endpoint 来源:
  - `coordinate`
  - `observation_ref`
  - `selector_refind`

### 不能做的事

- 不能把 selector 直接变成 action by selector 的隐式成功。selector 解析必须可审计,低置信度或多候选时返回候选集。
- 不能让 `@click:{target:{selector_id:"..."}}` 默认静默 re-find 后点击。至少第一版需要显式 `auto_refind:true` 或分阶段只支持 selector dry-run handoff。
- 不能让 ref target 失败后自动跌回旧 x/y 坐标。坐标 fallback 必须由 agent 显式发送。

## [2026-05-21 19:55:30] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Architect review 约束

## 综合发现

### 需要收紧的边界

- P5 第一版不应把 selector target 直接做成方便主路径。
- selector target 应拆成:
  - no-action handoff。
  - 显式 opt-in gated selector action。
- gated selector action 的前置条件必须比草案更硬:
  - typed refind decision。
  - audit response。
  - spy backend no-action tests。
  - 明确 stop rule。

### 必须进入最终计划的测试口径

- stale ref 时 fake mouse backend 调用次数为 0。
- rect missing 时 fake mouse backend 调用次数为 0。
- selector ambiguous / blocked / not_found / low confidence 时 fake mouse backend 调用次数为 0。
- parser 拒绝 `ref` / `selector_id` / `id` 混用。

## [2026-05-21 20:05:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: final plan 收口结论

### Review 结论

- Architect 对 draft v1 给出 `ITERATE`,核心要求是 selector target 第一版不能直接触发 mouse action。
- 最终计划已把 selector 拆成 `Phase 4A no-action handoff` 和 `Phase 4B explicit gated action`。
- Critic 对 draft v2 给出 `APPROVE`。

### 已采纳的可选改进

- Acceptance Criteria 单独加入 `rect missing / TARGET_RECT_UNAVAILABLE` 必须 `performed:false`,且 spy backend 调用次数为 0。
- Verification Plan 收紧为更精确的 `--exact` focused test 命令,并保留 nextest 分组建议。
- 明确 P5 不新增独立 `@hover`;hover-by-ref 作为 `@mouse-move target` 交付。

### 最终计划路径

- `.omx/plans/ralplan-rdog-observation-refmap-p5.md`
