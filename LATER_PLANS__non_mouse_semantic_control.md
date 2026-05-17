## [2026-05-17 15:30:18] [Session ID: codex-20260517-clipboard-restore] 后续计划: clipboard live ignored E2E

### 背景
- Phase 2.3 已经用 focused unit test 锁住 `restore-if-unchanged` 决策和 response schema。
- 本轮没有运行真实 clipboard live E2E,因为用户正在交互,真实剪贴板测试会短暂改写系统剪贴板。

### 后续建议
- 在用户不操作剪贴板时,补一条 ignored live E2E。
- 测试应证明:
  - `@type-text mode:"clipboard",allow_clipboard:true` 能向真实 TextEdit 文本区投递文本。
  - response 返回 `used_clipboard:true`。
  - response 返回 `clipboard_restore_policy:"restore-if-unchanged"`。
  - 常规成功路径下 `clipboard_restored:true`。
  - 测试结束后读取真实剪贴板,确认恢复到测试前内容。

### 验收边界
- 这条测试必须是 ignored/live opt-in。
- 测试前后必须尽量保存和恢复剪贴板。
- 如果剪贴板在测试期间被外部改变,测试不能把外部新内容覆盖回旧值。
