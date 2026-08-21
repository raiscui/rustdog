# 10: 打破循环依赖

**What to build:** control_observation 和 ax_query 之间不再有循环依赖。所有 observation 中的 capture 调用都通过 ObservationCapture adapter，所有 cache 调用都通过 ObservationStore。依赖方向单向：observation → query，query 不知道 observation 的存在。cargo 依赖图验证通过，证明循环依赖已消除。

**Blocked by:** #08 (AxSnapshotCache 迁移), #09 (ax_query 模块创建)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 更新 control_observation 中所有 `control_ax::capture_*` 调用为 `ObservationCapture::capture_for_observation`
- [ ] 更新 control_observation 中所有 `resolve_cached_ax_tree` 调用为 `ObservationStore::get_cached_ax_snapshot`
- [ ] ObservationCapture 内部调用 `ax_query::capture_window_snapshot`（不再调用 control_ax）
- [ ] 删除 control_ax.rs 中的旧 AxObservationCache 代码（static OnceLock 和相关函数）
- [ ] 验证 ax_query 模块不导入 control_observation（单向依赖）
- [ ] 运行 `cargo build --package rustdog` 验证无循环依赖错误
- [ ] 运行依赖图检查：`cargo tree` 或 `cargo-geiger` 验证依赖方向
- [ ] `cargo test` 全量测试通过（至少 796 tests）
- [ ] 集成测试验证 implicit_observe 仍然工作（5s TTL 复用，ADR-0005）

## Implementation notes

- 这是拆分的核心目标之一：消除循环依赖（Friction #2）
- ObservationCapture 是唯一的"observation → query"接口
- AxSnapshotCache 是唯一的"observation cache"实现
- 验证 ADR-0005 兼容性：implicit_observe 的 5s TTL 复用通过 CachePolicy::ImplicitObserve 保留
- 验证 ADR-0006 兼容性：@flow 中的 @computer-act 仍然返回完整字段
