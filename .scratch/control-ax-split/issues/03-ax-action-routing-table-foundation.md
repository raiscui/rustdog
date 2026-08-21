# 03: ax_action 路由表基础

**What to build:** 创建 ax_action 模块，提供统一的 `execute_ax_action(action, payload)` 入口。用户可以看到一个包含所有 13 个 action 的 ROUTES 数据表，一目了然地知道支持哪些 action、对应的 parser 和 executor。新增 action 只需在表中添加一行，而非修改多处代码。

**Blocked by:** None (可以立即开始)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 创建 `src/ax_action/` 目录结构（mod.rs, types.rs）
- [ ] 在 mod.rs 中定义 `ActionRoute` 结构体（name, parser, executor, timeout_ms）
- [ ] 定义 `const ROUTES: &[ActionRoute]`，包含所有 13 个 action 的路由条目
- [ ] 实现 `execute_ax_action(action: &str, payload: Value) -> io::Result<ActionResult>`
- [ ] execute_ax_action 通过 ROUTES 表查找 action，调用对应的 parser 和 executor
- [ ] 未知 action 返回 `InvalidInput` 错误，错误信息包含 action 名称
- [ ] 创建占位 parser 和 executor 函数（返回 `todo!()` 或简单实现）
- [ ] 添加路由表完整性测试：验证所有 13 个 action 都有路由条目
- [ ] 添加未知 action 错误测试：验证错误信息格式正确
- [ ] `cargo test --package rustdog --lib ax_action` 通过

## Implementation notes

- 13 个 action: press, click, type, scroll, drag, hover, set_value, mouse_move, wheel, double_click, right_click, middle_click, action
- 参考 specs/control-ax-module-split.md 中的 ROUTES 表结构
- 这个 ticket 只建立路由框架，实际的 parser 和 executor 实现在 #04 和 #05 中完成
- 使用函数指针而非 trait object（零成本抽象）
