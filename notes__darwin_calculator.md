## [2026-07-26 17:00:00] [Session ID: omx-1784789038072-clve0o] 笔记: Calculator Darwin Phase 0 证据

### 现场事实

- 当前运行中的 rdog daemon PID 为 39508。
- 8080 没有监听进程,`llama-server` 尚未启动。
- 计算器尚未运行,所以本轮没有产生任何计算器 GUI 成功或失败结论。
- rustdog 主工作树包含大量其他 Session 的修改,不能作为 Darwin hill-climbing 的 git 回滚面。

### 隔离决策

- 创建 `/tmp/rdog-darwin-calculator` worktree 和 `auto-optimize/20260726-165432-rdog-calculator` 分支。
- 复制当前 1.8 skill 作为真实基线。源文件和副本 SHA-256 一致。
- baseline commit 是 `8da8231e2865273ebee7f2c9ecb96f88759e893f`。
- 后续每轮只编辑隔离 worktree 的 `SKILL.md`;测试 prompt 和验收器保持固定。

### 测试设计理由

- happy path 检查 open-app、表达式输入、求值和结果读取的完整链路。
- stale-state case 检查是否会把旧显示误判为本轮结果。
- divide-by-zero case 检查错误状态读取与 fail-closed,防止模型编造结果。

### 尚未验证

- 计算器 AX 树是否稳定暴露表达式输入和结果显示。
- rdog 的 open-app、按键输入、窗口归属和 fresh 结果读取是否在该应用上完整工作。
- Bonsai 8B 在 with-skill 与 baseline 条件下的差异。

## [2026-07-26 17:32:00] [Session ID: omx-1784789038072-clve0o] 笔记: Calculator rdog 最小可证伪实验

### 静态证据

- `parse_open_app_payload` 只接受对象 payload,`execute_open_app` 在 macOS 调用 `open -a <app_name>`。
- `PasteRequest` 源码明确把 `@paste:"text"` 标为 legacy 文本注入兼容层,并写明“不建议作为普通文本输入路径”。
- Calculator 按钮可由 `@ax-find` 返回稳定 AX id,再用 `@ax-press` 执行。

### 动态证据

- `@open-app` 成功打开唯一 Calculator 窗口,窗口 frontmost 且 interactable。
- fresh AX 返回 22 个按钮,包括“全部清除”、数字、加、乘、除和等于。
- `@paste:"1+2*3"` 后按 Return,命令层返回成功,但 fresh AX 只显示 `1`。因此 action success 不足以证明输入完整。
- 改用 7 个语义 `@ax-press`,全部 `performed:true`;fresh AX 返回表达式 `1+2×3` 和结果 `7`。
- 除零语义按钮路径返回表达式 `1÷0` 和真实结果 `未定义`。

### 当前结论

- 已验证 rdog 程序具备本轮 Calculator 主任务和错误结果读取能力。
- `@paste` 部分输入是已验证失败现象,但现有静态证据更支持“调用了不适合 Calculator 的 legacy lane”,尚不足以认定 rdog 程序 bug。
- skill 当前把 `@paste` 列入通用 GUI 动作,却没有在主文件编码 Calculator/表达式输入的失败分支,这是待基线验证的候选 skill 短板。

### 被推翻的验证方向

- Pi `context-preview` 不显示显式 `--skill` 内容。baseline 与 with-skill preview 除时间戳外相同,不能用于证明最终 prompt 注入。
- 后续只用 CLI argv 单变量断言与正式 Pi JSONL/usage 证明实验边界。

## [2026-07-26 18:58:00] [Session ID: omx-1784789038072-clve0o] 笔记: Pi skill 注入有效边界

- `--skill` + custom/append system prompt 两种尝试都没有产生完整 skill token 增量。
- profile `skills:["rdog-control"]` 只增加 6 tokens,仍不足以代表完整 skill 内容。
- 将共同 prompt 与完整隔离 `SKILL.md` 明确合并后,with-skill 比 baseline 首轮增加 2,937 tokens。
- 因此本轮 Darwin 效果评估采用 deterministic prompt injection。该接线直接评估 `SKILL.md` 内容贡献,不评估 Pi 当前隐式 loader 的安装行为。
