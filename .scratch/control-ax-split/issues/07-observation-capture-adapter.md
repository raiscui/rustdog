# 07: ObservationCapture adapter 实现

**What to build:** 在 control_observation 中创建 ObservationCapture adapter，作为 observation 和 query 之间的长期 seam。测试工程师可以 mock 这个 adapter 来独立测试 observation 逻辑，无需启动完整的 AX query 系统。adapter 提供同步的 `capture_for_observation(window_id)` 接口，返回 (snapshot, selectors) 元组，满足 ADR-0005 对 implicit_observe 的要求。

**Blocked by:** None (可以立即开始)

**Status:** superseded (2026-08-28) — as-built 富化 seam 是 AxSnapshot::with_observation,
selector draft 构造留在 control_ax/tree.rs; 详见 ADR-0008 Amendment 2。请勿领取。

## Acceptance criteria

- [ ] 在 control_observation 中创建 `capture_adapter.rs`
- [ ] 定义 `ObservationCapture` struct（无状态）
- [ ] 实现 `capture_for_observation(window_id: &str) -> io::Result<(AxSnapshot, Selectors)>`
- [ ] capture_for_observation 内部调用 ax_query::capture_window_snapshot（当前暂时调用 control_ax，#09 后切换）
- [ ] 实现 `build_selectors(&snapshot) -> Selectors`，从 snapshot 提取 selectors
- [ ] 添加单元测试：验证返回 (snapshot, selectors) 元组，且 snapshot 非空
- [ ] 添加同步性测试：验证 capture_for_observation 是同步调用（elapsed < 1s）
- [ ] 添加 selectors 构造测试：验证 build_selectors 正确提取元素标识
- [ ] 更新 CONTEXT.md，添加 ObservationCapture 定义（已在之前完成，验证一致性）
- [ ] `cargo test --package rustdog --lib control_observation::capture_adapter` 通过

## Implementation notes

- ObservationCapture 是"长期 seam"，而非临时 adapter（在 CONTEXT.md 中明确定义）
- 它封装了"为 observation 捕获"的语义，返回格式不同于 ax_query 的底层接口
- 同步调用路径是 ADR-0005 的硬性要求（implicit_observe 5s TTL 复用）
- 当前暂时调用 control_ax::capture_current_ax_window_snapshot，在 #09 完成后改为 ax_query::capture_window_snapshot
