## [2026-05-28 16:21:47] [Session ID: codex-native-20260528-gui-bench] 后续计划: Phase 3B dense variant 和 bench artifact

### 背景
- Phase 3A 已经实现只读 `@gui-bench` fixture runner。
- 当前 runner 只支持 `baseline-low-level`,用于证明旧低级链路没有达到 density target。

### 后续建议
- 增加 `@web-find` / `@web-act` dense variant fixture,让 `@gui-bench` 能直接对比 baseline 和 dense path。
- 增加可选的 `target/rdog-bench/...` JSON artifact 输出,便于后续 CI 或人工审阅保存 bench 结果。

### 验收边界
- dense variant 仍然不能依赖公网网站。
- 如果后续引入 live replay,必须显式 opt-in,不能让 `@gui-bench` 默认触发真实 GUI 副作用。
- `dense_target_passed` 必须继续和 `status` 分离,避免把 runner 完成和密度达标混成一个含义。

### 完成记录
- [2026-05-28 17:00:22] [Session ID: codex-native-20260528-gui-bench-p3b] 已完成 dense variants、`variant:"all"` 对比和 `write_artifact:true` 可选输出。
- 后续如果继续扩展,应进入新的 Phase 3C: live replay opt-in 或 CI artifact collection,不要复用本条已完成计划。

## [2026-05-28 17:18:30] [Session ID: codex-native-20260528-gui-bench-p3c] 后续计划状态: Phase 3C 已完成 CI artifact collection

### 完成记录
- Phase 3C 已完成 CI artifact collection。
- 已通过真实 line-control receiver 测试覆盖 `@gui-bench ... variant:"all",write_artifact:true`。
- artifact 路径、schema、runs、threshold failures 和测试清理行为已有自动验证。

### 剩余后续
- 如果继续扩展,下一阶段应命名为 Phase 3D 或单独支线: live replay opt-in。
- live replay 必须继续显式 opt-in,不能让 `@gui-bench` 默认触发真实 GUI 副作用。

## [2026-05-28 18:08:17] [Session ID: codex-native-20260528-gui-bench-p3d] 后续计划状态: Phase 3D 已完成 live replay opt-in

### 完成记录
- Phase 3D 已完成 live replay opt-in。
- 默认 `@gui-bench` 仍是 `runner:"fixture"`。
- live replay 必须显式设置 `runner:"live",allow_side_effects:true`。
- live replay 已拒绝 `variant:"all"`,避免一次请求执行多个真实 GUI 操作。

### 剩余后续
- `src/control_protocol/tests.rs` 目前已超过 1000 行,这是既有测试文件结构问题。本轮只做最小字段调整,后续可单独拆分 protocol 测试模块。
- 如果继续扩展 live replay,建议新增真实 opt-in smoke 或额外 case,但不要改变默认 fixture runner。

### 完成记录
- [2026-05-29 00:05:09] [Session ID: codex-native-20260528-protocol-tests-split] 已完成 protocol 测试拆分,主文件降到 926 行。

## [2026-05-29 17:42:55] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 后续计划: bounded AX verification 与 live visual verifier

### 背景
- Phase 3F 已证明 `@web-find` 可以通过 targeted WebArea subtree refresh 找到深层“首页”。
- 真实点击“首页”后,`@web-act verify:true` 仍可能在页面变更期间超时,即使 action 已经执行且截图 diff 已证明瀑布流变化。
- `@web-act verify:false` 可返回,但仍要花约 11s 做定位;直接复用 AX id 的 `@ax-action` 约 0.03s。

### 后续建议
- 为 AX subtree refresh / AX snapshot verification 增加 bounded timeout 或隔离线程,避免 side effect 已执行但 final response 卡死。
- 给 live replay 增加 opt-in visual verifier,把前后截图 diff 变成结构化 evidence,但仍不能进入默认 fixture runner。
- 将“cached AX id -> direct @ax-action -> screenshot diff -> stale 时回退 @web-find”的快速策略沉淀成正式 cookbook 流程。

### 验收边界
- 任何真实 GUI 副作用仍必须显式 opt-in。
- 默认 `@gui-bench runner:"fixture"` 继续只读。
- 对 feed-changing 任务,`performed:true` 只能证明动作已发送,不能单独证明任务成功。
