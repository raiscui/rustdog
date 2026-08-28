# 01: ax_input 模块基础结构

**What to build:** 创建 ax_input 模块，提供简化的文本输入接口。用户（control_actions 的维护者）可以通过 `type_text(content, mode)` 和 `send_key(key, modifiers)` 这样的简单 API 输入文本，80% 的场景无需构造复杂的 `TypeTextRequest` 结构体。同时提供 `type_text_with_config` 高级 API 用于 20% 需要完整控制的场景。

**Blocked by:** None (可以立即开始)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 创建 `src/ax_input/` 目录结构（mod.rs, types.rs）
- [ ] 在 mod.rs 中实现简单 API：`type_text(content: &str, mode: TypeMode)` 和 `send_key(key: Key, modifiers: &[Modifier])`
- [ ] 在 mod.rs 中实现高级 API：`type_text_with_config(request: TypeTextRequest)` 和 `send_key_with_config(request: KeyRequest)`
- [ ] 简单 API 使用合理默认值（KeyDelivery::default(), target_window: None, verification: None）
- [ ] 移动 `src/control_ax/input.rs` (2.8KB) 到 `src/ax_input/input.rs`，内容保持不变
- [ ] 为简单 API 添加单元测试，验证默认值隐藏了 Request 复杂性（至少 10 个测试）
- [ ] 为高级 API 添加单元测试，验证自定义配置生效（至少 5 个测试）
- [ ] `cargo test --package rustdog --lib ax_input` 全部通过
- [ ] `cargo clippy -- -D warnings` 无警告

## Implementation notes

- 参考 specs/control-ax-split-implementation-plan.md Phase 1 的代码示例
- 简单 API 的设计目标是"隐藏 Request 类型"，而非"合并成一个函数"
- 高级 API 直接传递 Request，零成本转发到底层 perform 函数
- 这是整个拆分的"流程模板"，后续 ticket 将复用这个模式
