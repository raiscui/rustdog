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

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [状态]: 根据 review 进入 Phase 1 补真修复

### 现象
- `1d580eb` 的主方向正确,但 review 暴露出 3 个真实性缺口:
  - `@ax-set-value mode:"append"` 在读不到旧值时会静默退化成 replace。
  - `@type-text` 在不支持路径上会回报 `AX set value 当前只支持 macOS`,协议名不真实。
  - `old_value_redacted/new_value_redacted` 当前是硬编码 `false`,不是实测结论。

### 当前假设
- 这 3 个点都在 Phase 1 边界内,应该立即修复,不该带入 Phase 2。
- 修复后需要 focused tests 重新证明:
  - append 不再静默覆盖
  - type-text 的错误口径独立
  - redaction report 不再伪造固定 false

### 下一步
- [ ] 修正 `src/control_ax/macos.rs` 的 append 语义与 redaction report。
- [ ] 修正 `src/control_ax.rs` 的 type-text 错误映射与 report 结构。
- [ ] 同步 specs 文案并补 focused tests / build 验证。

### 当前状态
**正在补真修复** - 先改执行语义和 report,再跑 focused verification。

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: review 补真修复已验证通过

### 已完成
- [x] 修正 `src/control_ax/macos.rs` 的 append 语义与 redaction report。
- [x] 修正 `src/control_ax.rs` 的 type-text 错误映射与 report 结构。
- [x] 同步 specs 文案并补 focused tests / build 验证。

### 验证结果
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture` -> 11 passed
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture` -> 14 passed
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture` -> 11 passed
- `cargo build --package rustdog --bin rdog` -> 通过
- `git diff --check` -> 通过

### 当前状态
**准备本地提交** - 下一步把这轮 review fix 单独做成 local commit,然后再开 Phase 2。

## [2026-05-17 11:06:25] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [计划]: 进入 Phase 2 非鼠标投递能力

### 目标
- 实现 `@key delivery`
- 实现 `@ax-focus`
- 实现 `@ax-scroll`
- 实现 `@type-text` 的 `targeted-keyboard` / `clipboard`

### 约束
- 保持 Phase 1 真相面不回退
- 不运行 live 鼠标测试
- `@window-activate` 仍是唯一允许显式改变桌面可见状态的入口
- targeted input 必须诚实报告实际 delivery,不能伪装成比真实更强的能力

### 分解策略
- [ ] Phase 2A: 扩展协议结构,为 `@key` / `@ax-focus` / `@ax-scroll` / `@type-text` 增加字段与 parser
- [ ] Phase 2B: 落地 macOS targeted keyboard / AX focus / AX scroll / clipboard backend
- [ ] Phase 2C: 串起 executor / fake executor / focused tests
- [ ] Phase 2D: 同步 specs / skill / usage 文档

### 当前状态
**正在 Phase 2A** - 先扩请求结构和返回模型,再把 macOS 后端挂上去。

## [2026-05-17 11:14:33] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [状态]: 锁定 Phase 2 第一轮实现切面

### 已确认事实
- `@key` 当前只有 `key/hold_ms/mode`,成功时仍走老的简单 `EXEC_OK` / `@response 0` 风格。
- `TypeTextReport` 已经有 `delivered_via` 和 `used_clipboard`,适合继续承载 targeted-keyboard / clipboard 的真实投递结果。
- `@window-activate` 已经是唯一显式窗口恢复入口,并且 `window_id` 语义已经稳定,适合给 `@key delivery:"window-targeted"` 和 `@ax-focus` 复用。
- `src/shell.rs` / `src/control_core.rs` / `src/control_protocol.rs` 都有围绕旧 `KeyRequest` 的断言,改协议时必须同步修。

### 当前主假设
- 第一轮 Phase 2 可以先只支持 `pid` / `window_id` 定向,不必把 `app` / `bundle_id` 一次性铺开。
- `@key` 需要保留旧字符串 payload 的老返回形态,对象 payload 只有在显式带 `delivery` / `pid` / `window_id` 时才返回结构化成功报告。
- `@type-text` 的 `targeted-keyboard` 与 `clipboard` 更适合复用同一条 `type-text` response schema,由 `delivered_via` 区分真实路径。

### 下一步
- [ ] 先扩 `src/control_protocol.rs` / `src/control_ax.rs` 的请求结构与 parser。
- [ ] 再落地 macOS backend,优先做不改变桌面状态的路径。
- [ ] 最后同步 executor / fake executor / focused tests / docs。

### 当前状态
**继续 Phase 2A** - 先把协议结构做成可测形态,再进入后端实现。

## [2026-05-17 12:08:33] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: Phase 2 非鼠标投递能力第一轮已落地

### 阶段更新
- [x] Phase 2A: 扩展协议结构,为 `@key` / `@ax-focus` / `@ax-scroll` / `@type-text` 增加字段与 parser
- [x] Phase 2B: 落地 macOS targeted keyboard / AX focus / AX scroll / clipboard backend
- [x] Phase 2C: 串起 executor / fake executor / focused tests
- [x] Phase 2D: 同步 specs / skill / usage 文档

### 已完成结果
- `@key` 新增 `delivery:"global" | "pid-targeted" | "window-targeted"`。
- `@ax-focus` 已支持 `target` / `window_id` 二选一,且 `activate:true` 时显式复用 `@window-activate`。
- `@ax-scroll` 已支持 `direction/pages`,macOS 第一版真实回报 `delivered_via:"pid-scroll-event"`。
- `@type-text` 已支持:
  - `mode:"ax-value"`
  - `mode:"targeted-keyboard"`
  - `mode:"clipboard",allow_clipboard:true`
  - `mode:"auto"` 梯子
- 文档 / spec / skill / AGENTS 长期索引已同步。

### 验证结果
- `cargo fmt`
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`
- `cargo test --package rustdog --bin rdog --no-run`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`
- 以上全部通过

### 当前状态
**本轮 Phase 2 已完成** - 下一步可以 review diff,确认是否拆成本地 commit,以及是否继续做 live ignored E2E。

## [2026-05-17 13:06:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [续写]: Phase 2 提交前收口

### 目标
- 只 review 本轮 Phase 2 非鼠标语义控制相关 diff。
- 避开 worktree 中其他实验线和未完成支线文件。
- 在 review 无阻塞问题后,做 local commit,不 push。

### 待办
- [x] 审阅 `src/control_protocol.rs` / `src/control_ax.rs` / `src/control_ax/macos.rs` / `src/control_actions.rs` / `src/control_core.rs` / `src/shell.rs` / `src/zenoh_control.rs`
- [x] 审阅同步过的 `specs/rdog-non-mouse-semantic-control-plan.md` / `specs/code-agent-rdog-control-usage.md` / `AGENTS.md`
- [x] 复看 focused verification 证据是否仍成立
- [x] 只暂存本轮 Phase 2 相关文件并做 local commit

### 当前状态
**正在 review diff** - 先确认有没有需要在提交前修的小问题,再收口提交。

## [2026-05-17 13:13:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: Phase 2 第一轮已本地提交

### 结果
- 已完成 local commit: `3725ce9`
- 提交只包含本轮非鼠标语义控制相关文件。
- 当前 worktree 剩余改动属于其他支线,未纳入本次提交。

### 当前状态
**本轮目标完成** - 下一步若继续推进,最自然的是进入 live ignored E2E 或补 `@key global structured success` 的更细单测。

## [2026-05-17 13:18:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [续写]: Phase 2.1 live E2E + `@key` 结构化单测

### 目标
- 新增一个不依赖鼠标的 macOS live ignored E2E。
- 证明已授权 rdog 能在真实桌面上完成非鼠标语义控制链路。
- 同时补一条 `@key delivery:\"global\"` + `response_mode == Structured` 的 focused unit test。

### 待办
- [x] 阅读现有 `tests/control_ax_e2e.rs` / 相关 live helper,选定可稳定复用的目标 app 和断言。
- [x] 设计 live ignored E2E: 不碰鼠标,优先覆盖 `@ax-focus` + `@type-text targeted-keyboard` 或 `@ax-scroll` / `@key pid-targeted`。
- [x] 补 `execute_key` / `control_core` 层 focused unit test,锁住 global structured success response。
- [x] 跑新增 focused test + live ignored E2E,记录动态证据。

### 约束
- 不做任何鼠标移动、点击、拖拽、滚轮 live 操作。
- live E2E 必须以真实窗口行为为成功判据,不能只看返回码。
- 只改本轮非鼠标语义控制相关文件。

### 当前状态
**正在进入 Phase 2.1** - 先找现有 live ignored 测试夹具和最稳的真实窗口交互路径。

## [2026-05-17 14:07:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: Phase 2.1 live E2E 与 unit seam 已完成

### 结果
- 已新增 `@key delivery:"global"` structured success 的 focused unit seam test。
- 已新增并跑通不碰鼠标的 macOS live ignored E2E。
- 已用真实 TextEdit 窗口证明:
  - `@ax-focus activate:true`
  - `@type-text mode:"targeted-keyboard"`

### 动态证据
- unit:
  - `control_actions::tests::structured_global_key_success_response_should_report_structured_global_success`
- live:
  - `daemon_control_lane_should_focus_hidden_textedit_and_type_without_mouse`
  - 真实观测:
    - `window_id=pid:551/window:0`
    - `target_id=pid:551/window:0/path:0.0`
    - `pid=551`

### 当前状态
**本轮目标完成** - 下一步可以 review 这轮新增 diff 并做 local commit。
