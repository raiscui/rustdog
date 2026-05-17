## [2026-05-17 10:50:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 1 落地与 Ralph 收尾

### 任务内容
- 落地 `@ax-action`、`@ax-set-value`、`@type-text` 的第一批非鼠标语义控制能力。
- 保持 `@ax-press` 兼容映射,并把 agent-facing 文档与 skill 更新到非鼠标优先策略。
- 在 Ralph 收尾阶段修正协议真实性缺口,确保文档、parser 和运行时能力一致。

### 完成过程
- 扩展 `src/control_ax.rs`:
  - 新增 action / set-value / type-text 请求与 report 结构。
  - 扩展 `AxBackend` 为 `perform_action()` 与 `set_value()`。
  - 保留 `@ax-press` 作为 `AXPress` 兼容入口。
- 扩展 `src/control_ax/macos.rs`:
  - 新增通用 AX action 执行与 AXValue 写入。
  - 收尾统一 `AX target id` 错误文案,避免新协议回退成旧 `@ax-press target` 口径。
- 扩展协议与执行层:
  - `src/control_protocol.rs`
  - `src/control_actions.rs`
  - `src/control_core.rs`
  - `src/shell.rs`
- 更新长期规格和 agent 使用说明:
  - `specs/rdog-non-mouse-semantic-control-plan.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `AGENTS.md`
  - `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`
- Ralph 收尾时补掉两处真实性问题:
  - `@type-text allow_clipboard:true` 不再静默接受,改为显式拒绝。
  - `AX target` / `AX target id` 相关错误文案全部泛化。

### 验证
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --tests --no-run`
- `cargo build --package rustdog --bin rdog`
- `cargo fmt -- --check`
- `git diff --check`
- 全部通过

### 总结感悟
- 非鼠标语义控制的关键不是“再增加几条命令”,而是让 agent 明确知道哪些能力真的存在,哪些还没有开放。
- `allow_clipboard` 这类未来能力字段,在未实现前必须显式拒绝,不能默默吞掉。
- `AX target` 这类共享定位语义一旦升级成通用层,错误文案也必须同步泛化,否则 agent 会被历史命名误导。

### 提交记录
- local commit: `6497ab6`
- 提交标题: `Make non-mouse control truthful for agents`

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 1 review fix

### 任务内容
- 根据对 `1d580eb` 的 code review,修复 append 语义、type-text 错误口径和 redaction report 真实性问题。

### 完成过程
- 在 `src/control_ax/macos.rs` 中把 append 行为改成:
  - 仅在当前 `AXValue` 可读时才允许 append
  - 当前值不可读时直接失败
- 在 `src/control_ax/macos.rs` 中新增 target redaction 推导,让 report 使用真实 secure 状态。
- 在 `src/control_ax.rs` 中新增 `remap_type_text_ax_value_error()`,让 `@type-text` 不再冒用 `AX set value` 协议名。
- 同步修正 `specs/rdog-non-mouse-semantic-control-plan.md` 的 append 文案。
- 补充 focused tests,锁住:
  - append 不可读时报错
  - type-text 错误口径独立
  - redaction report 不再固定 false

### 验证
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`
- 全部通过

### 总结感悟
- append 这类“看起来只是字符串处理”的能力,只要偷做一次 silent fallback,就会直接破坏 agent 对协议的信任。
- report 字段一旦对外暴露,宁可少报,也不能伪造固定值。

## [2026-05-17 12:08:33] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 2 第一轮实现

### 任务内容
- 落地 `@key delivery`、`@ax-focus`、`@ax-scroll`、`@type-text` 的 targeted-keyboard / clipboard 真实现。
- 保持旧 `@key` 兼容,同时让新协议显式汇报真实 delivery。
- 同步更新 specs / usage / skill / AGENTS 长期索引。

### 完成过程
- 扩展 `src/control_protocol.rs`
  - `KeyRequest` 新增 `delivery/pid/window_id/response_mode`
  - 新增 `@ax-focus`、`@ax-scroll` parser
  - `@type-text` 新增 `targeted-keyboard` / `clipboard`
- 扩展 `src/control_ax.rs`
  - 新增 `KeyDeliveryReport`、`AxFocusReport`、`AxScrollReport`
  - 扩展 `AxBackend` 到 focus / scroll / type_text
- 扩展 `src/control_ax/macos.rs`
  - 新增 `CGEventPostToPid` 键盘 / 滚动定向投递
  - 新增 AX focus helper
  - 新增 clipboard opt-in 文本输入和剪贴板恢复
- 扩展执行层
  - `src/control_actions.rs`
  - `src/control_core.rs`
  - `src/shell.rs`
  - `src/zenoh_control.rs`
- 同步长期文档:
  - `specs/rdog-non-mouse-semantic-control-plan.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `AGENTS.md`
  - `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`

### 验证
- `cargo fmt`
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`
- `cargo test --package rustdog --bin rdog --no-run`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`
- 全部通过

### 总结感悟
- `@key` 这种老命令做演进时,关键不是“能不能多传几个字段”,而是要把 legacy success 和 structured success 的边界说清楚。
- `@ax-scroll` 这类名字和真实后端不完全同构的能力,必须把 `delivered_via`、`line_steps` 这种“真实发生了什么”显式回给 agent。
- clipboard fallback 只要一旦开放,就必须做到显式 opt-in 和恢复剪贴板,否则很容易和“避免干扰人类操作”目标冲突。

### 提交记录
- local commit: `3725ce9`
- 提交标题: `Deliver truthful non-mouse semantic control for agent-safe GUI automation`

## [2026-05-17 13:12:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 2 提交收口

### 任务内容
- 对 Phase 2 第一轮 diff 做提交前复核。
- 重新跑 focused verification,确认这轮能力已经满足本地提交条件。
- 只提交非鼠标语义控制相关文件,不混入其他支线。

### 完成过程
- 复跑并确认以下证据仍成立:
  - `control_ax::tests`
  - `control_protocol::tests`
  - `control_actions::tests`
  - `control_core::tests`
  - `cargo build --package rustdog --bin rdog`
  - 针对本轮文件集合的 `git diff --check`
- 审阅并确认本次提交边界:
  - 协议层: `src/control_protocol.rs`
  - 语义控制层: `src/control_ax.rs`, `src/control_ax/macos.rs`
  - 执行层: `src/control_actions.rs`, `src/control_core.rs`, `src/shell.rs`, `src/zenoh_control.rs`
  - 长期文档: `specs/rdog-non-mouse-semantic-control-plan.md`, `specs/code-agent-rdog-control-usage.md`, `AGENTS.md`
  - 本支线 context 文件
- 完成 local commit:
  - `3725ce9 Deliver truthful non-mouse semantic control for agent-safe GUI automation`

### 总结感悟
- 一旦 worktree 同时跑多条线,提交前显式收窄文件集合很关键,否则很容易把未验证实验线一起带进历史。
- 对 agent-facing 协议来说,真实 response schema 比“看起来更方便的 fallback”更重要。
