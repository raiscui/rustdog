# 任务计划: 非鼠标语义控制实现

## [2026-05-17 10:27:25] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [计划]: Ralph 执行非鼠标语义控制方案

### 目标
- 按 `.omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md` 推进实现。
- 首先实现不会移动真实鼠标的语义主干: `@ax-action`, `@ax-set-value`, `@type-text mode:"ax-value"` 和相关文档 / skill。
- 后续根据验证情况继续推进 `@key delivery`, `@ax-focus`, `@ax-scroll`。

### 阶段
- [x] 阶段0: 创建 Ralph context snapshot 并写入 Ralph state。
- [ ] 阶段1: 调研当前 AX/parser/action/fake executor 测试结构。
- [ ] 阶段2: 实现 `@ax-action` 与 `@ax-press` 兼容映射。
- [ ] 阶段3: 实现 `@ax-set-value` 与 `@type-text mode:"ax-value"`。
- [ ] 阶段4: 更新 docs / specs / `rdog-control` skill。
- [ ] 阶段5: 运行 focused tests, build, diff check,并做 Ralph completion audit。

### 约束
- 不运行 live 鼠标移动、点击、拖拽或滚轮测试。
- 不触碰已有 unrelated dirty worktree 文件。
- 不让 `@click` 隐式变成 AX-first 大杂烩。
- 不使用剪贴板 fallback,除非协议显式 `allow_clipboard:true`。

### 当前状态
**正在阶段1** - 下一步读取当前 AX 数据结构、parser 测试和 fake executor 行为,确定最小可验证实现切面。

## [2026-05-17 10:36:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [状态]: 已确认当前实现切面

### 已确认事实
- `src/control_protocol.rs` 目前只支持 `@ax-tree`, `@ax-find`, `@ax-get`, `@ax-press`,没有 `@ax-action`, `@ax-set-value`, `@type-text`。
- `src/control_ax.rs` 当前 `AxBackend` 只有 `snapshot()` 和 `press()`。
- `src/control_ax/macos.rs` 当前 target 解析和执行路径都写死为 `AXPress`。
- `src/control_actions.rs` 目前只有 `execute_ax_press()` 这条执行分支。
- `src/shell.rs` fake executor 也只有 `ControlCommand::AxPress` 的假实现。
- 当前已有的 `tests/control_ax_e2e.rs` 已覆盖真实 `@ax-find` / `@ax-get` / `@ax-press` 行为,适合后续扩展 `@ax-action` 和 value 写入验证。

### 决策
- 先实现 Phase 1 + Phase 2 的主干:
  - `@ax-action`
  - `@ax-set-value`
  - `@type-text mode:"ax-value"` 与 `mode:"auto"` 的 AXValue 分支
- `@key delivery`, `@ax-focus`, `@ax-scroll` 暂放到下一轮,避免一次性把协议面铺太大而难以验证。

### 当前状态
**进入阶段2** - 下一步修改协议枚举、parser 和 AX backend 抽象。

## [2026-05-17 10:47:53] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [状态]: 进入 Ralph 收尾修正

### 现象
- 当前实现主体已落地,focused tests / build / diff check 在上一轮通过。
- 但本地复查发现两个协议语义缺口:
  - `parse_ax_target()` 的重复字段错误文案仍残留 `@ax-press target`,会误导新的 `@ax-action` / `@ax-set-value` / `@type-text` 调用者。
  - `parse_type_text_payload()` 会静默接受 `allow_clipboard:true`,但运行时并没有 clipboard fallback。

### 当前假设
- 这是协议表述层问题,不是执行主链故障。
- 只要先修正 parser 契约并补测试,就能用最小验证证明当前非鼠标语义控制不会向 agent 暗示不存在的能力。

### 下一步
- [ ] 修正 `src/control_ax.rs` 中 AX target 重复字段错误文案。
- [ ] 让 `@type-text allow_clipboard:true` 明确报错。
- [ ] 补 parser 测试并重新跑 focused verification。

### 当前状态
**正在阶段5** - 先做 parser/deslop 收尾,再做完整验证和 Ralph completion audit。

## [2026-05-17 10:50:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: 非鼠标语义控制 Phase 1 收尾完成

### 阶段更新
- [x] 阶段0: 创建 Ralph context snapshot 并写入 Ralph state。
- [x] 阶段1: 调研当前 AX/parser/action/fake executor 测试结构。
- [x] 阶段2: 实现 `@ax-action` 与 `@ax-press` 兼容映射。
- [x] 阶段3: 实现 `@ax-set-value` 与 `@type-text mode:"ax-value"`。
- [x] 阶段4: 更新 docs / specs / `rdog-control` skill。
- [x] 阶段5: 运行 focused tests, build, diff check,并做 Ralph completion audit。

### 收尾结论
- 已补齐两处协议真实性缺口:
  - `AX target` 泛化错误口径
  - `@type-text allow_clipboard:true` 显式拒绝
- 已补充对应 parser / macOS target id 测试。
- 本轮不进入鼠标 live E2E,也不进入 `@key delivery` / `@ax-focus` / `@ax-scroll` 第二阶段实现。

### 验证结果
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --tests --no-run`
- `cargo build --package rustdog --bin rdog`
- `cargo fmt -- --check`
- `git diff --check`
- 以上全部通过

### 当前状态
**本轮执行完成** - 下一步做 local commit,并把 Phase 2 留作后续独立实现线。
