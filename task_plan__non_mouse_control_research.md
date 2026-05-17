# 任务计划: open-codex-computer-use 非鼠标控制调研

## [2026-05-17 00:21:14] [Session ID: codex-20260517-non-mouse-control-research] [计划]: 调研非鼠标控制能力

### 目标
- 调研 `open-codex-computer-use` 的能力结构。
- 为 `rdog` 后续实现“尽量完整、以 AX/窗口/键盘/剪贴板/菜单为主、避免干扰人类鼠标操作”的控制方案提供依据。

### 阶段
- [ ] 阶段1: 官方仓库与本地代码调研。
- [ ] 阶段2: 对照 rustdog 当前 AX/window/mouse 能力。
- [ ] 阶段3: 输出 repo-local 方案文档。
- [ ] 阶段4: 记录交付和后续落地建议。

### 约束
- 当前用户正在使用计算机,本轮不运行任何真实鼠标移动、点击、拖拽、滚轮测试。
- 调研可以 clone/read 代码、读文档、写方案,但不启动会抢焦点或操作桌面的流程。
- 优先研究非鼠标能力: AX tree/action、键盘、文本输入、菜单、窗口激活/关闭、剪贴板、脚本执行、进程/应用控制。

### 当前状态
**正在阶段1** - 已打开官方仓库页面,下一步浅 clone 仓库并读取 README、package/tool 定义和 MCP server 实现。

## [2026-05-17 00:32:19] [Session ID: codex-20260517-non-mouse-control-research] [状态]: 调研完成

### 已完成
- 完成官方仓库与本地代码调研。
- 完成与 `rustdog` 当前 AX/window 能力的对照。
- 完成 repo-local 方案文档 `specs/rdog-non-mouse-control-open-computer-use-research.md`。
- 完成笔记和调研工作日志。

### 关键结论
- `open-codex-computer-use` 的核心不是鼠标,而是 semantic AX actions + settable value + pid-targeted keyboard + targeted scroll/drag 的分层.
- 它默认也避免全局 pointer fallback,这和“不要干扰人类操作”的目标一致。
- `rdog` 不应该把这一思路再压回 `@click` 里,而应该把它变成显式的 AX / window / text 协议层。

### 状态
**调研已完成** - 后续只保留实现建议和等待用户下一个明确动作.

## [2026-05-17 00:34:34] [Session ID: codex-20260517-non-mouse-control-research] [验证]: 文档校验完成

### 验证
- `beautiful-mermaid-rs --ascii`: 两个新增 mermaid 图均通过。
- `git diff --check`: 通过。

### 状态
**支线调研已收口** - 本轮没有运行任何真实鼠标控制命令.

## [2026-05-17 00:42:41] [Session ID: codex-20260517-non-mouse-control-plan] [计划]: 生成非鼠标语义控制改进方案

### 目标
- 基于已经完成的 `open-codex-computer-use` 调研,生成一份可交给后续 `$ralph` / 实施智能体执行的改进方案。
- 方案必须以非鼠标控制为主,不把能力继续藏进 `@click` 的隐式 fallback。
- 本轮只做方案落盘和文档验证,不运行任何 live 鼠标类测试。

### 阶段
- [x] 阶段1: 整理调研结论和当前 `rdog` 协议边界。
- [ ] 阶段2: 创建 `.omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
- [ ] 阶段3: 验证方案中的 Mermaid 图。
- [ ] 阶段4: 记录交付结果。

### 当前状态
**正在阶段2** - 下一步创建改进方案文件,并把 Option A / Option B、验收标准、实施步骤和验证路径写清楚。

## [2026-05-17 00:49:04] [Session ID: codex-20260517-non-mouse-control-plan] [状态]: 改进方案已落盘并验证

### 已完成
- 已创建 `.omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
- 已将调研结论整理成可执行的改进方案,包含 Option A / Option B、推荐决策、实施步骤、风险与验证路径。
- 已验证方案中的两个 Mermaid 图语法。
- 已确认 `git diff --check` 通过,没有引入裸 Markdown 语法问题。

### 当前状态
**计划文档已完成** - 后续如果用户要求执行,可以直接交给 `$ralph .omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
