## [2026-07-26 17:00:00] [Session ID: omx-1784789038072-clve0o] 任务名称: Calculator Darwin Phase 0 初始化

### 任务内容

- 建立不污染 rustdog 脏主工作树的独立 Darwin 实验环境。
- 固化当前 1.8 `rdog-control` skill,创建结果表和 3 个 calculator 测试 prompt。

### 完成过程

- 核实 daemon、8080、计算器进程和主工作树状态。
- 创建 `/tmp/rdog-darwin-calculator` worktree与独立分支。
- 校验 skill 副本 SHA-256、测试 JSON 和 `git diff --check`。
- 创建 baseline commit `8da8231e2865273ebee7f2c9ecb96f88759e893f`。

### 总结感悟

- 这轮必须把程序修复与 skill 优化拆开。否则即使分数提高,也无法判断收益来自哪一个变量。
- 对计算器任务,仅看到数字 `7` 仍不足以证明完整执行。窗口归属、动作结果和 fresh 后验必须一起保存。
