# 02: 迁移 control_actions.rs 到 ax_input

**What to build:** control_actions.rs 的维护者现在可以使用简洁的 `ax_input::type_text()` 而非手动构造 `TypeTextRequest`。所有旧的 control_ax input 函数调用都已迁移到新接口，旧函数标记为 deprecated 防止新代码使用。CI 测试全部通过，验证迁移无回归。

**Blocked by:** #01 (ax_input 模块基础结构)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 更新 control_actions.rs 中所有 `perform_default_type_text` 调用为 `ax_input::type_text`
- [ ] 更新 control_actions.rs 中所有 `perform_default_key_delivery` 调用为 `ax_input::send_key`
- [ ] 如果存在需要自定义配置的场景，使用 `ax_input::type_text_with_config`
- [ ] 在 control_ax.rs 中标记旧函数为 `#[deprecated(since = "0.9.0", note = "use ax_input::type_text instead")]`
- [ ] 旧 deprecated 函数内部调用新 ax_input 函数（完整代理，不能省略功能）
- [ ] `cargo test` 全量测试通过（至少 796 tests）
- [ ] `cargo build` 显示 deprecated 警告，确认标记生效
- [ ] 检查其他潜在调用方（control_computer_act, control_flow）是否也需要迁移

## Implementation notes

- 迁移时优先使用简单 API，只在必要时使用高级 API
- 保留旧函数作为 facade，确保外部调用方（如果有）不受影响
- Facade 生命周期：所有内部调用方迁移后删除（在 #11 中执行）
- 参考 ADR-0008 中的 "Facade 退出策略"
