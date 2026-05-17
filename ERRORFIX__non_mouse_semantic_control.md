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

## [2026-05-17 21:20:04] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: `@paste` 的协议文档还把裸命令写成 `@response 0`

### 问题
- `specs/control-line-protocol.md` 的“无 request id 的成功且无输出”段落里,还把 `@paste` 放进了 `@response 0` 的适用场景。
- 这和当前实现不一致,因为裸 `@paste` 现在返回的是 structured paste report,里面要明确告诉 agent 它走的是 hotkey,以及实际使用的是 `cmd-v` 还是 `ctrl-v`。

### 原因
- 文档是在 `@paste` 语义拆分前写的,更新主线协议时没有把响应语义那一节一起同步。

### 修复
- 将该段的适用场景改成 legacy `@key` 和 legacy `@paste:"text"`。
- 补充说明裸 `@paste` 成功时会返回 structured paste value,而不是 `0`。

### 验证
- `git diff --check`
- `rg -n '@paste.*@response 0|@paste.*稳定|@paste.*普通文本|@paste.*deterministic|Bare \`@paste\`|legacy \`@paste|structured paste|global-hotkey' specs /Users/cuiluming/.codex/skills/rdog-control/SKILL.md`
- `python3 /Users/cuiluming/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`

### 备注
- 这个问题不是代码回归,但它会直接误导 agent,所以必须写进错误记录。

## [2026-05-18 00:05:23] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: clipboard live E2E 只落下单个 `v`

### 问题
- `@type-text mode:"clipboard"` 的真实 live ignored E2E 结果不是完整字符串,而是只在 TextEdit 里留下了一个 `v`。

### 原因
- macOS targeted delivery 的 `cmd+v` 事件只发了 modifier down/up,但主键事件没有携带 modifier flags。
- 结果目标 App 把它当成了普通字母 `v`,而不是系统粘贴快捷键。

### 修复
- 在 `src/control_ax/macos.rs` 里补入 `CGEventSetFlags` 和 modifier flag mask。
- `post_key_request_to_pid` 改成在 main key event 和 modifier 序列里显式设置 active modifier flags。
- 同时把 clipboard live E2E 的 editor 查找改成轻量 `@ax-get depth:2,max_elements:300`,避免重型 AX 树扫描拖慢测试。

### 验证
- live ignored clipboard E2E 重新运行后,TextEdit 真正收到了完整文本,并且 `clipboard_restored=true`。
- 辅助 unit test 继续通过:
  - `control_ax::tests::type_text_clipboard_report_should_expose_restore_status`
  - `control_ax::macos::tests::clipboard_restore_decision_should_restore_only_when_temporary_value_survived`

### 备注
- 这个 bug 只会在真实桌面上暴露,所以一定要留 live evidence,不能只看协议和 unit test。
