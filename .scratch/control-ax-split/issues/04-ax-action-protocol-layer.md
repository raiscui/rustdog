# 04: ax_action protocol 层 (parse 函数)

**What to build:** 所有 action payload 解析逻辑集中在 ax_action/protocol.rs，测试工程师可以为每个 parse_* 函数编写纯函数单元测试，无需 mock 平台 API。验证协议解析逻辑（JSON → struct 转换）的正确性，覆盖所有 13 个 action 的 parse 路径。

**Blocked by:** #03 (ax_action 路由表基础)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 创建 `src/ax_action/protocol.rs`
- [ ] 从 control_ax.rs 移动所有 parse_* 函数到 protocol.rs（至少 7 个函数）
- [ ] 每个 parse_* 函数返回 `io::Result<AnyActionRequest>`，其中 AnyActionRequest 是统一的枚举类型
- [ ] 定义 `AnyActionRequest` 枚举，包含所有 action 类型的变体
- [ ] 更新 ROUTES 表，将占位 parser 替换为实际的 parse_* 函数引用
- [ ] 为每个 parse_* 函数添加单元测试（至少 2 个测试/函数，覆盖正常和错误场景）
- [ ] 添加 parse 错误测试：验证无效 payload 返回清晰的错误信息
- [ ] `cargo test --package rustdog --lib ax_action::protocol` 全部通过
- [ ] 所有 parse 测试独立运行，不依赖 perform 逻辑或平台 API

## Implementation notes

- Parse 函数是纯函数（JSON → struct），易于测试，这是"易赢"场景
- 参考现有 control_ax.rs 中的 parse_ax_press_payload, parse_ax_action_payload 等
- AnyActionRequest 枚举示例：`enum AnyActionRequest { Press(AxPressRequest), Action(AxActionRequest), ... }`
- 保持原有类型定义（AxPressRequest, AxActionRequest）在 types.rs 中
