## [2026-05-17 00:32:19] [Session ID: codex-20260517-non-mouse-control-research] 任务名称: open-codex-computer-use 非鼠标控制调研

### 任务内容
- 调研 `https://github.com/iFurySt/open-codex-computer-use` 的实现和使用规范。
- 重点确认它如何支持尽量完整的非鼠标控制。
- 对照 `rustdog` 现有 AX/window 协议,提炼后续可迁移的设计。

### 完成过程
- clone 了官方仓库的浅副本到 `/tmp/open-codex-computer-use-research`。
- 读取了 `README.md`、`docs/ARCHITECTURE.md`、`skills/open-computer-use/SKILL.md` 以及 usage / troubleshooting reference。
- 重点检查了 `ToolDefinitions.swift`、`ComputerUseService.swift`、`InputSimulation.swift`、`AccessibilitySnapshot.swift`、`MCPServer.swift`。
- 把结论整理进 `notes__non_mouse_control_research.md` 和 `specs/rdog-non-mouse-control-open-computer-use-research.md`。
- 使用 `beautiful-mermaid-rs` 验证了新增 mermaid 图语法。

### 总结感悟
- 这个仓库最有价值的地方不是鼠标 API,而是它把 AX / keyboard / value / window recovery 组织成了分层动作链。
- 如果 `rdog` 想尽量不干扰人类操作,下一步应该继续压缩鼠标作为默认路径,而不是继续给鼠标补更多能力。
- `get_app_state` 里包含恢复 hidden/minimized 窗口的行为,这类“为了可操作性而改变桌面状态”的动作必须在 `rdog` 里保持显式,不能悄悄发生。

### 验证
- `beautiful-mermaid-rs --ascii`: 新增 flowchart 和 sequenceDiagram 均通过。
- `git diff --check`: 通过。
- 本轮没有运行任何 live 鼠标类测试。

## [2026-05-17 00:49:04] [Session ID: codex-20260517-non-mouse-control-plan] 任务名称: 非鼠标语义控制改进方案

### 任务内容
- 基于 `open-codex-computer-use` 调研结果,生成 `rdog` 的非鼠标语义控制改进方案。
- 方案聚焦 `@ax-action`, `@ax-set-value`, `@ax-focus`, `@type-text`, `@ax-scroll`, `@key delivery` 等显式协议能力。
- 方案明确把鼠标命令放在最后 fallback,避免干扰用户正在操作的桌面。

### 完成过程
- 阅读了当前 `rdog` 的协议入口和 AX / window 能力边界。
- 把调研结论整理成 `.omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
- 补充了 Option A / Option B 的选型说明、推荐实施顺序、验收标准和风险缓解。
- 校验了 Mermaid 图和 `git diff --check`。

### 总结感悟
- `open-codex-computer-use` 的启发不在于 "更多鼠标 API",而在于把 AX / value / targeted key / scroll 分层做清楚。
- `rdog` 后续如果要更适合 agent 使用,就应该把这层分层显式化,而不是把所有动作都压进 `@click`。
- 对用户当前桌面无干扰,应继续作为协议和 skill 的第一约束。
