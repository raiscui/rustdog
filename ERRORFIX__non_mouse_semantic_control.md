## [2026-05-17 15:11:59] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: `@ax-scroll` 返回成功但真实 TextEdit 不滚动

### 问题
- `@ax-scroll` 旧实现通过 `CGEventPostToPid` 向目标 pid 发送 scroll event。
- response 会返回 `performed:true`,但 live E2E 证明 TextEdit 的滚动条没有变化。
- 这违反了本项目“协议必须说真话”的要求。

### 原因
- `CGEventPostToPid` scroll event 并不能保证被目标 TextEdit scroll view 消费。
- 第一版测试又只看 `AXScrollBar.value`,在后续修复中也发现这个字段本身不稳定。

### 修复
- 删除旧的 `pid-scroll-event` 路径和相关 dead code。
- macOS `@ax-scroll` 改为:
  - 根据目标元素定位同窗口 `AXScrollBar`
  - 检查 `AXValue` 可写
  - 写入新的 0..1 比例值
  - 回报 `delivered_via:"ax-scrollbar-value"`
- live E2E 改为优先检查 `AXValueIndicator.rect.y` 变化。

### 验证
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_scroll_real_textedit_without_mouse --exact --ignored --nocapture`
- 结果: 1 passed
- 真实证据: `before=109`, `after=211`

## [2026-05-17 15:30:18] [Session ID: codex-20260517-clipboard-restore] 错误修复: clipboard fallback 可能覆盖人类剪贴板新内容

### 问题
- `@type-text mode:"clipboard"` 旧实现会先读出系统剪贴板,写入临时文本,投递后再无条件恢复旧内容。
- 这个流程在机器被人类同时使用时,可能把人类刚更新的剪贴板内容覆盖回旧值。

### 原因
- 恢复动作没有检查当前剪贴板是否仍然等于 rdog 的临时写入值。
- 也就是说,恢复逻辑缺少“剪贴板状态未被第三方改变”的前置条件。

### 修复
- 新增 `ClipboardRestoreStatus`。
- clipboard 路径改为 `restore-if-unchanged`:
  - 只有当前剪贴板仍等于 rdog 临时文本时才恢复旧值
  - 如果剪贴板已被改写,就跳过恢复
- `TypeTextReport` 新增:
  - `clipboard_restore_policy`
  - `clipboard_restored`
  - `clipboard_restore_skipped_reason`

### 验证
- `cargo test --package rustdog --bin rdog -- control_ax::tests::type_text_clipboard_report_should_expose_restore_status control_ax::macos::tests::clipboard_restore_decision_should_restore_only_when_temporary_value_survived --nocapture`
- `cargo test --package rustdog --bin rdog -- control_ax:: --nocapture`
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_route_ax_commands_to_executor --exact --nocapture`
- `cargo fmt`
- `cargo fmt -- --check`
- `git diff --check -- src/control_ax.rs src/control_ax/macos.rs specs/rdog-non-mouse-semantic-control-plan.md specs/code-agent-rdog-control-usage.md task_plan__non_mouse_semantic_control.md`

### 备注
- 本轮没有跑 live clipboard E2E,因为用户正在交互,不希望剪贴板测试扰动现场。
