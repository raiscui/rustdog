# rdog Replay preflight、guard 与 verification policy

## Status

本规格是 Wayfinder ticket [定义 replay preflight、guard 与 verification policy](https://github.com/raiscui/rustdog/issues/4) 的 resolution asset。

它固化显式 replay 在执行动作前的必检项、缺失或歧义的 fail-closed 边界、post-action verification 范围以及 best-effort profile 能放宽的最小条件,使 controller 和 daemon 在跨 host replay 时不需要再做产品级判断。

本规格是 policy 入口,不实现 daemon-side 校验逻辑或 controller-side 状态机。

## Scope

本规格只定义:

- Bundle provenance gate
- 8 个顺序 preflight gate
- preflight reject reason code 命名空间
- state-mutating / state-read / input-primitives 三类 action 的 post-action verification policy
- best-effort profile 与 strict 的差异
- Replay safety rollback trigger 与收口动作
- Replay outcome 报告契约

以下内容由其他规格负责:

- semantic promotion 与 coordinate fallback: `specs/rdog-recording-semantic-promotion-policy.md`
- Participating Window 与 Window Geometry Precondition: `specs/rdog-recording-window-geometry-policy.md`
- display scope / window identity / focus verification: `specs/rdog-display-aware-control-chain-plan.md`、`specs/rdog-display-scope-control-plan.md`
- mouse / AX / window 各自 action 契约: `specs/rdog-mouse-control-coordinate-plan.md`、`specs/rdog-ax-screenshot-manifest-control-plan.md`、`specs/rdog-window-control-plan.md`、`specs/rdog-non-mouse-semantic-control-plan.md`
- observation scoped refmap: `specs/rdog-observation-scoped-refmap-plan.md`
- Recording Bundle 物理形态: `specs/rdog-recording-bundle-schema.md`
- Recording Session lifecycle: `specs/rdog-recording-session-lifecycle.md`

## Terms

- **preflight**: action 执行前的顺序 gate 检查链,任一失败立即 fail closed。
- **gate**: preflight 中的单个检查单元,有独立判定标准与 stable reason code。
- **verification**: action 执行后的 fresh re-observe 校验。
- **profile**: Replay session 的执行策略,值为 `strict` 或 `best-effort`。
- **quarantine**: state-mutating action verification 失败后,隔离同一 Replay session 剩余 step。
- **roll back**: Replay session 在 trigger 触发后停止后续 step,不回滚已发生 side effect。
- **trigger**: 引发 roll back 的事件类。
- **Replay session metadata**: controller-side 元数据,daemon 不持久化。
- **provenance**: Bundle manifest 的 `producer.version` 与 `compiler.version`。

## Invariants

1. 显式 replay 必须先通过 Bundle provenance gate,再进入 8 gate preflight,再执行任何 action。
2. 任一 preflight gate 失败立即 fail closed,不修改 target state,不发起 partial action。
3. state-mutating action 必须 post-action verification,strict profile 下 verified:false 必进入 quarantine。
4. safety rollback 不自动回滚 side effect,因为部分 action(paste、focus、activate)不可逆。
5. best-effort profile 不允许放宽任何 preflight gate,仅放宽 verification 与 coordinate fallback。
6. controller-side Replay session metadata 不进入 Bundle manifest,不进入 daemon lifecycle metadata。
7. stable reason codes 与 ticket `#9` 的 `delivery_failed` reason codes 命名风格一致。
8. safety rollback 报告不携带 raw error 文本、堆栈或 target identifier 明细。
9. 用户取消(controller 本地 Ctrl-C / daemon SIGTERM)与 gate 失败等价 fail closed,触发 roll back。
10. controller 断线后 daemon-side action 仍由现有 connection 失败路径处理,不依赖本规格。

## Entry contract

### Bundle provenance gate

- `manifest.producer.version` 必须与 daemon 当前 producer version 兼容。
- `manifest.compiler.version` 必须与 daemon 当前 compiler version 兼容。
- 不兼容时 fail closed,reason code `bundle_provenance_incompatible`。
- `bundle_provenance_incompatible` 是 stable reason code,`structured_field` 携带 `{missing: ["producer.version"], incompatible: ["compiler.version"]}`。
- daemon 不自动 upgrade / downgrade。
- controller 必须重新拉取 Bundle(走 `#9` 远程交付 retry 路径),不修改 manifest。
- best-effort profile 不允许跳过 provenance 检查。
- provenance 检查只在 Replay session 第一次发生 action 时执行,后续 action 跳过。

## Preflight gates

### Order

```
1. permission gate
2. parameter gate
3. application gate
4. display topology gate
5. participating-window gate
6. geometry precondition gate
7. selector freshness gate
8. coordinate guard gate
```

### Per-gate rules

#### permission gate

- 必需 TCC 权限全部获批(Accessibility、Screen Recording、Input Monitoring 等)。
- daemon `@capabilities` 报告与 Bundle provenance 一致。
- 失败 reason code:`permission_denied`。
- `structured_field` 携带 `{missing_capabilities: ["accessibility"]}`。

#### parameter gate

- 所有 required replay parameter 已 resolved 且 descriptor 与 Bundle 一致。
- required parameter 的 type 校验通过。
- 失败 reason code:`parameter_unbound`。
- `structured_field` 携带 `{unbound_parameters: ["recording_id"]}`。

#### application gate

- target app reachable(`bundle_id` 命中、`LSApplication` 存在)。
- target app 未 terminated。
- launch policy 与 Bundle 一致。
- 失败 reason code:`application_unreachable`。
- `structured_field` 携带 `{unreachable_apps: ["com.example.App"]}`。

#### display topology gate

- Referenced Display 在当前 session 可解析(唯一或显式 `display_id`)。
- `@capabilities` 报告的 display 集合与 Bundle 期望匹配。
- 失败 reason code:`display_topology_invalid`。
- `structured_field` 携带 `{missing_displays: ["d2"], ambiguous_displays: ["d3"]}`。

#### participating-window gate

- 每个 Participating Window 唯一命中。
- 0 命中 → `WINDOW_MISSING`。
- 多命中 → `WINDOW_AMBIGUOUS`。
- 复用现有 `window_missing` 与 `window_ambiguous` reason code,`structured_field` 携带 `{window_role: "recording_target"}`。

#### geometry precondition gate

- 每个 Participating Window 已通过 Phase 1(`@window-resize` + structured verify report)。
- 或确认已有 Initial Window Snapshot 且 rect 未变。
- 失败 reason code:`geometry_precondition_failed`。
- `structured_field` 携带 `{failed_windows: ["com.example.App#Document"]}`。

#### selector freshness gate

- semantic locator re-findable,fresh timeout 内命中。
- ownership 未变。
- 失败 reason code:`selector_stale`。
- `structured_field` 携带 `{failed_locators: ["AXButton#Submit"]}`。

#### coordinate guard gate

- 显式 `guard:{display:{...}}` 存在。
- point/path 落在 display 内。
- `coordinate_space` 与 Bundle 一致。
- 失败 reason codes:
  - `coordinate_guard_missing`:guard 字段缺失。
  - `coordinate_guard_invalid`:point/path 越界或 `coordinate_space` 失配。
- `structured_field` 携带 `{guard_required: true, display_id: "d2"}`。

### Cross-gate rules

- 按依赖顺序检查,任一 gate 失败立即 fail closed,后续 gate 不执行。
- 不存在跨 gate 短路或合并。
- 用户取消在任意 gate 之间被检测时立即进入 cancel-safe rollback,与 gate 失败等价 fail closed。
- gate 失败时不修改任何 target state、不发起 partial action。
- 所有 gate 检查通过前不允许进入 action 执行阶段。

## Post-action verification

### Action class

| Class | 含义 | 必须 verification |
| --- | --- | --- |
| state-mutating | 改变 target app/widget state | 必须 |
| state-read | 只读,不改变 state | 不需要 |
| input-primitives | 纯输入事件,无状态语义 | 不需要 |

### State-mutating actions

- `@window-activate` / `@window-close` / `@window-resize`
- `@ax-press` / `@ax-action` / `@ax-set-value` / `@ax-focus`
- `@click` / `@drag`
- `@type-text` (含 paste)
- `@key` 含 paste 或 clipboard mode

### State-read actions

- `@window-find` / `@ax-tree` / `@ax-find` / `@web-find` / `@observe`
- `@screenshot`

### Input-primitives actions

- `@mouse-move` (不带 click)
- `@wheel`

### Verification rules

- verification 必须使用 `fresh re-observe`,不允许使用 pre-action observation 缓存。
- verification 超时或失败时,controller 收到 `performed:true,verified:false` 等价结构。
- state-mutating action 收到 `verified:false` 后必须进入 quarantine:同一 Replay session 剩余 step 全部 fail closed,新 step 不发起。
- quarantine 状态写入 Replay session metadata,不写入 lifecycle metadata,不进入 Bundle。
- quarantine 不自动回滚已发生的 side effect(paste、focus、activate 不可逆)。
- best-effort profile 下 `verified:false` 不进入 quarantine,只按结构化字段返回。

## Best-effort profile

### Trigger

- `@replay#id:{profile:"best-effort",...}` 显式 opt-in。
- 默认值:`strict`。
- profile 只写 Replay session metadata,不写入 Bundle manifest。

### Per-gate profile table

| Gate / 行为 | strict | best-effort |
| --- | --- | --- |
| permission gate | fail closed | fail closed |
| parameter gate | fail closed | fail closed |
| application gate | fail closed | fail closed |
| display topology gate | fail closed | fail closed |
| participating-window gate | fail closed | fail closed |
| geometry precondition gate | fail closed | fail closed |
| selector freshness gate | fail closed | fail closed |
| coordinate guard gate | fail closed | fail closed |
| post-action verification | 必须 | 允许跳过 |
| coordinate fallback (无 semantic) | 禁用 | 允许,sandboxed risk warning |

### Profile report

- best-effort session 完成后,daemon 报告包含 `profile:"best-effort"` 与 `coordinate_fallback_used: <count>`。
- profile 字段只写入 Replay session metadata。

## Safety rollback

### Trigger classes

| Trigger | 含义 |
| --- | --- |
| `preflight_gate_failed` | 任一 preflight gate 失败 |
| `verification_failed` | strict profile 下 state-mutating action verification 失败 |
| `coordinate_action_failed` | coordinate action 在 backend 层失败 |
| `user_cancelled` | controller 本地 Ctrl-C / daemon SIGTERM |
| `controller_disconnected` | controller 连接丢失 |

### Rollback action

- 立即停止 Replay session 内所有未执行 step。
- 已发起但未完成的 in-flight action 不强行中断,等待 backend 自行返回或超时。
- in-flight action 超时上限 = 当前 action 的现有 timeout;不为收口引入新超时。
- 已成功的 side effect 不回滚。
- quarantine 写入 Replay session metadata,与 Replay outcome 一致。
- Replay session metadata 是 controller-side,daemon 不持久化。

### Stable reason codes

新增 stable codes:

- `verification_failed`
- `coordinate_action_failed`
- `controller_disconnected`

复用 preflight reject reason codes(`permission_denied` 等)。

### Report contract

```json
{
  "replay_outcome": "rolled_back",
  "roll_back_reason_code": "<stable code>",
  "roll_back_trigger": "<trigger class>",
  "completed_steps": <count>,
  "skipped_steps": <count>,
  "verified_false_steps": <count>
}
```

Rules:

- `replay_outcome` 允许值:`"completed"` / `"rolled_back"` / `"cancelled"`。
- `cancelled` 与 `rolled_back` 区别:用户主动中断使用 `cancelled`,trigger 触发使用 `rolled_back`。
- 任何 trigger 都不会产生 raw error 文本、堆栈、target identifier 明细。
- daemon 报告 `replay_outcome` 与 `roll_back_*` 字段,不报告其他未登记字段。
- 同一 Replay session 内多个 trigger 触发时,只记录首个 trigger 与 reason code,后续 trigger 不覆盖。

## Cross references

- `specs/rdog-recording-bundle-schema.md`:Bundle 物理形态与 provenance schema。
- `specs/rdog-recording-semantic-promotion-policy.md`:semantic first 与 coordinate fallback 规则。
- `specs/rdog-recording-window-geometry-policy.md`:Window Geometry Precondition 与 Phase 1/2。
- `specs/rdog-display-aware-control-chain-plan.md`:display scope / window identity / focus verification。
- `specs/rdog-display-scope-control-plan.md`:display scope resolver。
- `specs/rdog-mouse-control-coordinate-plan.md`:coordinate guard 与 `os-logical` 坐标契约。
- `specs/rdog-ax-screenshot-manifest-control-plan.md`:AX action verification 与 manifest。
- `specs/rdog-window-control-plan.md`:window action verification。
- `specs/rdog-non-mouse-semantic-control-plan.md`:AXPress 与 AX value 写入。
- `specs/rdog-observation-scoped-refmap-plan.md`:observation refmap 与 re-find。
- `specs/rdog-recording-session-lifecycle.md`:lifecycle 状态机。
- `specs/control-line-protocol.md`:line-control 协议。

## Open questions

无。本规格已包含 ticket `#4` question 列出的所有边界。
