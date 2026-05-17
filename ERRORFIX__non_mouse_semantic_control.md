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
