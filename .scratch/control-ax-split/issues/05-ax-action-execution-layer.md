# 05: ax_action execution 层 (perform 函数)

**What to build:** 所有 action 执行逻辑集中在 ax_action/execute.rs，平台相关代码（macOS AX API 调用）移动到 platform/macos.rs。用户通过 `execute_ax_action("press", payload)` 统一入口执行任何 action，内部自动 parse → execute 流程。集成测试验证端到端 routing 正确工作。

**Blocked by:** #04 (ax_action protocol 层)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 创建 `src/ax_action/execute.rs`
- [ ] 从 control_ax.rs 移动所有 perform_* 函数到 execute.rs（至少 7 个函数）
- [ ] 每个 perform_* 函数接受 `AnyActionRequest`，提取对应变体后执行
- [ ] 创建 `src/ax_action/platform/` 目录
- [ ] 移动 `src/control_ax/macos.rs` (67KB) 到 `src/ax_action/platform/macos.rs`，内容保持不变
- [ ] 在 platform/macos.rs 顶部添加 TODO 注释："当多平台支持需要时，提取为 ax_platform 模块"
- [ ] 更新 ROUTES 表，将占位 executor 替换为实际的 perform_* 函数引用
- [ ] 添加路由表集成测试：验证 parse → execute 流程端到端工作
- [ ] 添加未知 action 集成测试：验证路由表查找失败场景
- [ ] `cargo test --package rustdog --lib ax_action` 全部通过（包括 protocol + execute + routing）

## Implementation notes

- Perform 函数调用平台 API，测试覆盖率低于 parse（需要平台 mock，复杂度高）
- 优先通过集成测试验证 routing 正确性，平台 API 调用通过现有的集成测试覆盖
- 参考现有 control_ax.rs 中的 perform_default_ax_press, perform_default_ax_action 等
- 移动 macos.rs 时只调整 `use` 路径，内部逻辑保持不变（67KB 代码零改动）
