# 11: 清理与文档

**What to build:** 所有 deprecated facade 已删除，新模块文档完整，CONTEXT.md 与代码同步。开发者可以通过模块文档快速理解 ax_query、ax_action、ax_input 的职责和接口。ADR-0008 验证通过，确认所有设计决策已落地。全量测试通过，证明拆分无回归。

**Blocked by:** #02 (迁移 control_actions 到 ax_input), #06 (迁移 control_actions 到 ax_action), #10 (打破循环依赖)

**Status:** done-as-built (2026-08-28) — deprecated facade 全删, 模块文档/ADR/spec
同步完成; control_ax.rs 保留为 verb 层 + 共享内核 (未整文件删除, 与本 ticket
原始设想不同), 见 ADR-0008 Amendment 2 与两份 spec 状态头。

## Acceptance criteria

- [ ] 删除 control_ax.rs 中所有标记为 `#[deprecated]` 的 facade 函数
- [ ] 检查并删除未使用的 control_ax 代码（如果 control_ax.rs 已空，考虑删除整个文件）
- [ ] 为 ax_query/mod.rs 添加 module-level 文档（职责、接口、示例）
- [ ] 为 ax_action/mod.rs 添加 module-level 文档（统一入口、路由表、添加新 action 的步骤）
- [ ] 为 ax_input/mod.rs 添加 module-level 文档（简单 API vs 高级 API、80/20 原则）
- [ ] 验证 CONTEXT.md 中的 ObservationCapture 和 AX Snapshot Cache 定义与实现一致
- [ ] 验证 ADR-0008 中的所有决策已落地（21 个 Implementation Decisions）
- [ ] 更新 specs/control-ax-split-implementation-plan.md 的状态（标记所有 Phase 为完成）
- [ ] `cargo test` 全量测试通过（至少 796 tests）
- [ ] `cargo clippy -- -D warnings` 无警告
- [ ] `cargo build` 无 deprecated 警告（所有 facade 已删除）
- [ ] 运行现有 benchmark suite，验证无性能回归

## Implementation notes

- 这是拆分的最终验收阶段
- 检查 control_ax.rs 是否还有未迁移的代码（types、helper 函数等）
- 如果 control_ax.rs 完全为空，可以删除该文件并从 mod.rs 中移除
- Module-level 文档应包含："What it does"、"When to use"、"Examples"
- ADR-0008 验证清单：检查 21 个决策是否都有对应的代码实现
- 性能验收：cache hit rate 不低于当前水平
