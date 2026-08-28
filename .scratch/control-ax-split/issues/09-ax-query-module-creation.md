# 09: ax_query 模块创建

**What to build:** 创建无状态的 ax_query 模块，提供 AX tree 捕获和查询能力。用户可以通过 `capture_window_snapshot(window_id)` 捕获 AX 树，通过 `find_element(snapshot, selector)` 查询元素，无需关心 cache 逻辑（cache 由 ObservationStore 管理）。query.rs 和 tree.rs 的核心逻辑保持不变，只调整模块边界。

**Blocked by:** #07 (ObservationCapture adapter 实现)

**Status:** done-as-built (2026-08-28) — ax_query 落地为无状态捕获核心;
query.rs 经核实是 @ax-find/@ax-get verb 实现而非纯查询引擎, 保留在 control_ax。
与本 ticket 验收项的差异见 ADR-0008 Amendment 2。

## Acceptance criteria

- [ ] 创建 `src/ax_query/` 目录结构（mod.rs, capture.rs, types.rs）
- [ ] 在 mod.rs 中定义公开接口（约 5 个函数）：
  - `capture_window_snapshot(window_id: &str) -> io::Result<AxSnapshot>`
  - `capture_tree(query: &AxQuery) -> io::Result<AxSnapshot>`
  - `find_element(snapshot: &AxSnapshot, selector: &Selector) -> Option<&AxElement>`
- [ ] 从 control_ax/tree.rs 提取 capture 逻辑到 capture.rs
- [ ] 移动 control_ax/query.rs (45.6KB) 到 ax_query/query.rs，内容保持不变
- [ ] 重新导出 query.rs 中的公开函数
- [ ] 添加单元测试：验证 capture_window_snapshot 返回非空 snapshot
- [ ] 添加单元测试：验证 find_element 查询逻辑正确
- [ ] `cargo test --package rustdog --lib ax_query` 通过
- [ ] 验证 ax_query 模块是无状态的（不持有任何 static 变量或全局 cache）

## Implementation notes

- ax_query 是无状态模块，不拥有 cache（cache 由 ObservationStore 持有）
- query.rs (45.6KB) 是核心查询逻辑，保持不变，只调整 mod 结构
- capture 逻辑从 tree.rs 提取，但 tree.rs 的其他部分可能仍在 control_ax（按需移动）
- 这个 ticket 完成后，#07 中的 ObservationCapture 可以从 `control_ax::capture_*` 改为 `ax_query::capture_*`
