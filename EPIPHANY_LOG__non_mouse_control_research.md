## [2026-05-17 00:32:19] [Session ID: codex-20260517-non-mouse-control-research] 主题: non-mouse control 不能等同于 no-mouse API

### 发现来源
- 调研 `open-codex-computer-use` 的工具面和 macOS 实现时发现。

### 核心问题
- 这个仓库的“非鼠标能力”很强,但它并不是把所有能力都抽象成单独的 non-mouse API。
- 它更像是一个 action ladder: AX -> value -> pid-targeted keyboard -> targeted event -> explicit global pointer fallback.
- 同时,`get_app_state` 自身就可能 unhide / activate / raise 窗口,这说明“观测能力”也可能改变桌面状态。

### 为什么重要
- 如果 `rdog` 只学命名而不学分层,很容易把所有操作继续堆回 `@click`。
- 那样虽然看起来“更统一”,但实际上会更容易干扰人类,也更难审计每次动作到底有没有切桌面。

### 未来风险
- 如果把窗口恢复、键盘投递和鼠标 fallback 混在一个隐式路径里,以后排查误触和抢焦点问题会很难。

### 当前结论
- `rdog` 最值得借鉴的是显式分层和 fallback 顺序。
- 不能把“非鼠标控制”理解成“没有鼠标工具”,而应该理解成“鼠标只是最后的显式 fallback”.

### 后续讨论入口
- `specs/rdog-non-mouse-control-open-computer-use-research.md`
- `specs/rdog-ax-screenshot-manifest-control-plan.md`
- `specs/rdog-window-control-plan.md`
