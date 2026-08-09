## [2026-07-25 13:16:04] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义 Participating Window 与 geometry precondition 编译

### 任务内容

- 通过HITL grilling确认Participating Window、initial snapshot、intentional move/resize、transient surface、durable locator、window state、clamp、display topology和compiler sequence。
- 新增正式规格`specs/rdog-recording-window-geometry-policy.md`。
- 同步`AGENTS.md`知识索引,以及lifecycle、Journal和Semantic Promotion三份规格的ownership指针。

### 完成过程

- 复用现有`@window-find`、`@window-activate`、`@window-resize`、display resolver和`rdog.flow.v1`,没有新增Recorder专用window命令。
- 使用`beautiful-mermaid-rs --ascii`实际解析decision flow与execution sequence两张图。
- 验证Markdown围栏、必需章节、本地文档引用和scoped diff。
- 创建并push scoped commit`6973dfa3c9bc5edc9528c51448e0f7d6d9a60599`。
- 发布GitHub resolution comment,关闭ticket,更新Wayfinder map并清除对应fog。

### 验证证据

- 两个Mermaid block均解析成功。
- `git diff --check`通过。
- GitHub API返回的`origin/main` SHA与本地HEAD一致。
- Ticket状态为`CLOSED/COMPLETED`。
- Map存在geometry decision pointer,旧display topology fog不存在。

### 总结感悟

- Geometry恢复的单一真相源应是pre-action Initial Window Snapshot,不能用首个动作后的rect回填。
- Window/display identity必须先只读且唯一解析,再允许任何desktop side effect。
- Geometry freshness只证明窗口环境,永远不能替代semantic target freshness。
- 首版严格拒绝跨topology映射、fullscreen state replay和best-effort继续,比引入隐式猜测更符合录制还原一致性目标。
