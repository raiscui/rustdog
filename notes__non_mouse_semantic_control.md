## [2026-05-17 10:36:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: 非鼠标语义控制第一批实现切面

### 静态证据
- `src/control_protocol.rs`
  - `ControlCommand` 只有 `AxPress`,没有 `AxAction` / `AxSetValue` / `TypeText`。
  - parser 入口只接了 `ax-press`。
- `src/control_ax.rs`
  - `AxActionReport.action` 当前是硬编码 `press`。
  - `AxBackend` trait 当前只有 `snapshot()` 和 `press()`。
- `src/control_ax/macos.rs`
  - `press_target_id()` 和 `map_ax_action_error()` 的错误文案都写死 `AXPress`。
- `src/control_actions.rs`
  - 目前只有 `execute_ax_press()`。
- `src/shell.rs`
  - fake executor 只有 `ControlCommand::AxPress` 的 `AX_PRESS:*` 输出。

### 实现策略
- 不在这一轮把 `@click` 改成 AX-first。
- 先把 AX 后端抽象升级为:
  - `perform_action(target, action)`
  - `set_value(target, value, mode)`
- 协议层新增:
  - `@ax-action`
  - `@ax-set-value`
  - `@type-text`
- 第一版 `@type-text` 只支持:
  - `mode:"ax-value"`
  - `mode:"auto"` 但只走 AXValue 分支
- `@ax-press` 保留,内部映射成 `@ax-action action:"AXPress"`。

### 风险提醒
- `src/control_protocol.rs` / `src/shell.rs` 已经偏大,如果再直接堆 parser 和 fake arms,后续会更难维护。
- 因此实现时尽量把新增结构和 parser 继续收在 `src/control_ax.rs`,避免把 AX 相关知识散落到更多地方。

## [2026-05-17 10:50:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Ralph 收尾自审结论

### 现象
- 第一轮 focused verification 已经通过,但二次文本扫描又发现 `src/control_ax/macos.rs` 里残留 `@ax-press target id ...` 错误文案。
- `parse_type_text_payload()` 在主实现完成后,仍会静默接受 `allow_clipboard:true`,这和当前运行时能力不一致。

### 假设
- 这两处都属于协议真实性问题,不是执行链路故障。
- 如果不修,agent 会被错误文案误导,以为:
  - 新命令的 target id 失败仍然是 `@ax-press` 专属错误。
  - `@type-text` 已经具备 clipboard fallback。

### 已验证修正
- `src/control_ax.rs`
  - `parse_ax_target()` 的重复字段和字段解析文案统一改为 `AX target`。
  - `@type-text allow_clipboard:true` 改为显式报错。
  - 新增 parser test,锁住:
    - 不再出现 `@ax-press target`
    - `allow_clipboard:true` 会失败
- `src/control_ax/macos.rs`
  - `parse_target_id()` 的非法 target id 错误文案统一改为 `AX target id ...`
  - 新增单测,确保不再回退到 `@ax-press target`

### 动态证据
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
  - 9 passed
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
  - 14 passed
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
  - 11 passed
- 复跑收尾验证:
  - `cargo test --tests --no-run`
  - `cargo build --package rustdog --bin rdog`
  - `cargo fmt -- --check`
  - `git diff --check`
  - 全部通过

### 结论
- 当前 Phase 1 非鼠标语义控制已经从“主链能跑”收敛到“协议口径和实际能力一致”。
- 尚未进入的 Phase 2 仍然是:
  - `@key delivery`
  - `@ax-focus`
  - `@ax-scroll`
  - `@type-text targeted-keyboard / clipboard`

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Phase 1 review 后补真修复策略

### 现象
- review 证明 `1d580eb` 仍有 3 个不够“说真话”的点:
  - append 可能静默覆盖
  - `@type-text` 会复用 `AX set value` 的 unsupported 错误口径
  - redaction report 固定写 `false`

### 修复策略
- append:
  - 只有在当前 `AXValue` 可读且可转成字符串时才允许 append
  - 否则返回结构化 invalid input,拒绝偷偷 replace
- type-text:
  - 保持当前只走 AXValue 路径
  - 但在 Unsupported / PermissionDenied / 其它错误上保留 `@type-text` 自己的协议名
- redaction:
  - 用目标元素的 `AXRole` / `AXSubrole` 推导是否是 secure element
  - secure 时把 old/new redacted 标成 `true`
  - 非 secure 才标 `false`

### 需要同步的文档口径
- `append` 不再写成“先读取再拼接”这么轻描淡写。
- 要明确“当前值不可读时 append 失败”。
- `old_value_redacted/new_value_redacted` 不再暗示固定有值,而是表达真实 redaction 状态。

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: review 修复后的结论

### 已修复事实
- `src/control_ax/macos.rs`
  - `append` 现在走 `build_final_ax_value()`。
  - 当前 `AXValue` 不可读时,明确报错 `无法执行 append`,不再静默 replace。
  - `target_value_is_redacted()` 会读取目标元素的 `AXRole` / `AXSubrole`,再复用 `looks_like_secure_element()` 推导 redaction。
- `src/control_ax.rs`
  - `AxSetValueReport::success()` 不再硬编码 redaction 为 `false`,而是接收真实值。
  - `perform_default_type_text()` 经过 `remap_type_text_ax_value_error()` 包一层协议名映射。
  - 非 macOS unsupported 文案从 `AX set value` 纠正为 `type-text` 自己的路径描述。
- `specs/rdog-non-mouse-semantic-control-plan.md`
  - append 语义补成“当前值不可读即失败”。

### 动态证据
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
  - 11 passed
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
  - 14 passed
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
  - 11 passed
- `cargo build --package rustdog --bin rdog`
  - 通过
- `git diff --check`
  - 通过

### 结论
- 这轮修复把 Phase 1 从“主链可用”推进到“append / type-text / redaction 三个敏感点也说真话”。
- 现在再开 Phase 2,不用继续背着 Phase 1 的协议真实性债。
