## [2026-05-20 19:28:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 问题: P2 初版 selector resolve / fixture / history 契约不完整

### 现象

- Ralph reviewer 第一轮返回 `CHANGES_REQUESTED`。
- 阻断点包括:
  - `@selector-resolve` 对 0 候选 / 多候选没有结构化 selector error。
  - `matched_fields` 只是 selector 字段回显,不是候选实际解释。
  - golden fixture 只有 AX element,缺 AX window 和 window。
  - `include_history:true` 只是返回 `[last_seen]`,没有真实 durable history。

### 原因

- 初版实现把 P2 的 “inspect + dry-run surface” 做通了,但没有把 “可解释 resolve” 和 “history 真实来源” 当成协议验收面处理。
- fixture 只锁住了最常见 AX element,没有覆盖 AX window 和 native window 两个同级 selector kind。

### 修复

- `@selector-resolve` 增加 finalize gate:
  - 0 候选返回 `SELECTOR_NOT_FOUND`。
  - 多候选返回 `AMBIGUOUS_SELECTOR`。
  - backend 权限 / unsupported 映射成 selector error。
- `matched_fields` / `missing_fields` 改为基于候选实际字段比较。
- durable store 新增 `selector_history(selector_id, limit)`。
- `@selector-get include_history:true` 返回真实 durable history。
- 新增 `ax_window_selector_v1.json` 和 `window_selector_v1.json` fixtures。

### 验证

- `control_observation::selector::tests`: 6 passed。
- `control_observation::durable::tests`: 4 passed。
- `control_observation::tests`: 10 passed。
- 完整回归矩阵和 skill validate 通过。
- Ralph reviewer 第二轮复审 `APPROVED`。
