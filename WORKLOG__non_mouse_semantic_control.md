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
