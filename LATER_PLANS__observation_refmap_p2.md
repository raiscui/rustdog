## [2026-05-20 19:28:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 备忘: P2 之后的 selector resolver 增强

### 延期事项

- P3 / P2b 再处理 action by selector。当前 `@selector-resolve` 只返回 dry-run 候选和 fresh observation ref,不直接执行 AXPress / focus / set-value / close。
- P3 再处理 confidence ranking 和 automatic semantic re-find。当前多候选返回 `AMBIGUOUS_SELECTOR`,不自动挑选。
- P3/P4 再考虑把 anchors 真正纳入 ranking。当前 P2 fixtures 和 schema 已保留 anchors 字段,但 resolver 第一版主要用 app/window/element constraints。
- 后续 selector 数量明显变大时,再把 durable selector lookup 从线性扫描 index 优化成 map/index 结构。
- `@capabilities` 相关 skill/doc 更新已跟随 GUI agent 工作流进入本轮文档,但如果要做协议级 conformance,应另开 capability surface 专项计划。

### 当前结论

- 这些点不是 P2 阻断项。
- 当前 P2 的验收边界已经收口在 stable selector schema、真实 history、显式 dry-run resolve 和结构化错误。
