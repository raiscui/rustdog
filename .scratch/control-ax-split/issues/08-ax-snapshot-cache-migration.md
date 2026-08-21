# 08: AxSnapshotCache 迁移到 ObservationStore

**What to build:** AxSnapshotCache 从 control_ax 移动到 control_observation 的 ObservationStore，成为加速层而非真相源。ObservationStore 是 resource epoch 的单一真相源，cache 只负责验证和加速。支持多种 TTL policy（ImplicitObserve 5s 满足 ADR-0005，Progressive 300s 用于长时间查询）。测试工程师可以验证"stale epoch 导致 cache miss"这一关键路径，确保 cache 不会返回过期 snapshot。

**Blocked by:** #07 (ObservationCapture adapter 实现)

**Status:** ready-for-agent

## Acceptance criteria

- [ ] 在 control_observation 中创建 `ax_cache.rs`
- [ ] 定义 `AxSnapshotCache` struct（entries, order, capacity）
- [ ] 定义 `AxSnapshotCacheEntry` struct（snapshot, epochs, policy, captured_at_unix_ms）
- [ ] 定义 `CachePolicy` enum（ImplicitObserve { ttl_ms: 5000 }, Progressive { ttl_ms: 300000 }）
- [ ] 实现 `insert(observation_id, snapshot, epochs, policy)`（FIFO eviction at capacity）
- [ ] 实现 `get(observation_id, current_epochs) -> Option<&AxSnapshot>`（验证 TTL 和 epoch match）
- [ ] 将 AxSnapshotCache 集成到 ObservationStore（作为字段持有）
- [ ] 添加 **Priority #1 测试**：验证 stale epoch 导致 cache miss
- [ ] 添加 TTL 测试：ImplicitObserve 5s 和 Progressive 300s 分别验证
- [ ] 添加真相源测试：验证 cache 通过 ObservationStore 验证 epoch，而非自己持有真相
- [ ] 添加 FIFO eviction 测试：验证容量达到 64 后旧条目被移除
- [ ] `cargo test --package rustdog --lib control_observation::ax_cache` 全部通过（至少 20 个测试）

## Implementation notes

- 这是"关键路径优先"测试策略的核心（Q13-C）
- Cache 的 epochs 字段是"捕获时的 epoch 快照"（不可变），不是真相源
- 验证逻辑：调用 capture_resource_epochs() 获取当前 epoch，对比 cache 中的快照
- Policy 由调用方指定，而非 cache 自动推断（显式优于隐式）
- 参考 specs/control-ax-module-split.md 中的 AxSnapshotCache 测试策略
