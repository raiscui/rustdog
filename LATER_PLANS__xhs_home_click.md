## [2026-06-01 15:05:34] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 后续计划: window-scoped web/gui probe 产品化

### 背景
- 小红书“首页”真实 e2e 已经用 before/after screenshot diff 闭环。
- 本轮主要绕路点是 `@web-find target:{browser:"active"}` 在多 Chrome 窗口下返回 `BROWSER_WINDOW_AMBIGUOUS`。
- 坐标 fallback 虽然最终成功,但它依赖 fresh screenshot 和人工/脚本裁剪判断,不适合作为长期默认路径。

### 建议下一步
- 给 `@web-find` 或 `@gui-probe` 增加 window-scoped target,例如 `target.window_id` 或 `target.window_ref`。
- 让 probe 在一次请求里返回:当前窗口、截图、AX/Web 候选、可点击目标和验证建议。
- 对带真实副作用的 live replay 继续保持 opt-in,不要塞回默认 fixture runner。

### 验收口径
- 多 Chrome 窗口时,能够指定小红书窗口并在该窗口内查找“首页”。
- 不依赖 `target:{browser:"active"}` 的唯一性假设。
- 若最终执行点击,仍必须用点击前后瀑布流内容变化作为成功标准。

## [2026-06-01 15:30:57] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 完成记录: `@web-find target.window_id` 已落地

### 已完成部分
- `@web-find` 支持 `target:{window_id:"pid:.../window:..."}`。
- live read-only smoke 已在小红书 Chrome 窗口上返回 `status:"complete"`、`scope:"target_window_web_area"`、`match_count:1`。
- `@web-act` 复用同一 target schema,但仍需要显式 side-effect 意图。

### 尚未完成部分
- `@gui-probe` 仍未实现为独立 composite 命令。
- `target.window_ref` 尚未实现,后续应从 observation ref 安全解析到 window id。

## [2026-06-01 18:03:20] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 完成记录: `@web-find target.window_ref` 已落地

### 已完成部分
- `@web-find` 支持 `target:{window_ref:"@e1",observation_id:"obs-..."}`。
- live read-only smoke 已证明它能解析 `@window-find` 返回的窗口 ref,并在该 Chrome 窗口内完成 WebArea 查找。
- `@web-act` 复用同一 target schema,但仍需要显式 side-effect 意图。

### 尚未完成部分
- `@gui-probe` 仍未实现为独立 composite 命令。
- 如果需要跨 observation / daemon restart 恢复窗口,后续应走 selector/refind,不要延长 short ref 生命周期。
