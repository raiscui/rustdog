# 06: 迁移 control_actions.rs 到 ax_action

**What to build:** control_actions.rs 的维护者现在可以使用统一的 `execute_ax_action(action, payload)` 执行所有 AX action，而不需要记住 13 个不同的 perform_* 函数名。所有旧的 control_ax action 函数调用都已迁移到新接口，旧函数标记为 deprecated。CI 测试全部通过，验证迁移无回归。

**Blocked by:** #05 (ax_action execution 层)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 更新 control_actions.rs 中所有 `perform_default_ax_press` 等调用为 `ax_action::execute_ax_action`
- [ ] 重构调用方代码，将 action 名称和 payload 传递给统一入口
- [ ] 在 control_ax.rs 中标记旧 perform_* 函数为 `#[deprecated(since = "0.9.0", note = "use ax_action::execute_ax_action instead")]`
- [ ] 旧 deprecated 函数内部调用新 ax_action 函数（完整代理，必须传递所有字段）
- [ ] 检查 control_computer_act/ 和 control_flow/ 中的 action 调用，如有需要则一并迁移
- [ ] `cargo test` 全量测试通过（至少 796 tests）
- [ ] `cargo build` 显示 deprecated 警告，确认标记生效
- [ ] 验证外部 API (@computer-act, @ax-tree 命令) 行为不变

## Implementation notes

- 调用方从 `perform_default_ax_press(&request)` 改为 `execute_ax_action("press", serde_json::to_value(&request)?)`
- 如果 control_computer_act 内部已经在构造 JSON payload，可以直接传递给 execute_ax_action
- Facade 生命周期：所有内部调用方迁移后删除（在 #11 中执行）
- ADR-0006 要求：确保 @flow 中的 @computer-act 调用返回完整的 density/trace_summary/verification 字段
