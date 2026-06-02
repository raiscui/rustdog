# 任务计划: computer-use density / Web target 产品化续档

## [2026-06-01 18:06:52] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [续档]: `task_plan__computer_use_density.md` 超过 1000 行后的当前入口

### 续档原因
- 旧 `task_plan__computer_use_density.md` 已达到 1152 行,超过项目六文件 1000 行限制。
- 已按 `continuous-learning` 规则回读并提炼本支线的可复用经验。
- 旧文件已移动到 `archive/branch_contexts/computer_use_density/task_plan__computer_use_density_2026-06-01_180652.md`。

### 已沉淀的长期知识
- `specs/rdog-computer-use-density-plan.md`: 记录 `@web-find target.window_id` / `target.window_ref` 和后续 `@gui-probe` 边界。
- `.codex/skills/rdog-control/SKILL.md`: 记录多浏览器窗口下先用 `window_id` / `window_ref` 做 read-only WebArea 定位。
- `.codex/skills/rdog-control/references/cookbook-web-content.md`: 记录 browser active / window id / window ref 三种 Web target 用法。
- `.codex/skills/rdog-control/references/protocol.md`: 记录 line-control 示例。
- `notes__computer_use_density.md` / `WORKLOG__computer_use_density.md` / `ERRORFIX__computer_use_density.md`: 保留本支线的研究、交付和错误修复记录。

### 当前完成状态
- [x] `@web-find target.window_id` 已完成并通过 live read-only smoke。
- [x] `@web-find target.window_ref + observation_id` 已完成并通过 live read-only smoke。
- [x] `@web-act` 复用同一 target schema,但仍只在显式 side-effect 意图下使用。
- [x] 默认 `@gui-bench` fixture runner 仍保持只读,live replay 仍需 `runner:"live" + allow_side_effects:true`。
- [ ] 真正的 read-only `@gui-probe` composite 仍未实现。

### 当前验证证据
- `cargo fmt --check`: 通过。
- `cargo test --package rustdog --bin rdog -- control_web --quiet`: 17 passed。
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::web_gui --quiet`: 5 passed。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 16 passed。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 passed。
- `cargo test --package rustdog --bin rdog --quiet`: 314 passed。
- `git diff --check`: 通过。

### 下一步建议
- 如果继续产品化,下一阶段应单独实现真正的 read-only `@gui-probe` composite。
- `@gui-probe` 应复用已经落地的三种 target schema: `browser:"active"`、`window_id`、`window_ref + observation_id`。
- `@gui-probe` 默认仍必须只读,只返回 bootstrap / capability / window / WebArea / match / verification suggestion,不能执行 click 或 focus。

### 当前状态
**续档完成** - 后续继续本支线时,优先读取本文件作为当前入口;需要历史细节再打开 archive 中的旧 task plan。

## [2026-06-01 18:13:21] [Session ID: codex-native-20260601-window-ref-final-check] [验证]: `target.window_ref` 拆分后的 fresh closeout

### 本轮目标
- 只核对 `target.window_ref + observation_id` 这条 read-only window-scoped target 是否已经单独落地。
- 不把真正的 `@gui-probe` composite 混进本轮完成口径。
- 不执行 live click / focus / activate / type 等 GUI 副作用。

### 待办
- [ ] 核对当前工作区里 `control_web` / protocol parser / docs skill 的实现范围。
- [ ] 运行 fresh 格式化与测试验证。
- [ ] 给用户交付当前状态,明确 `@gui-probe` 后续单独做。

### 当前状态
**阶段1进行中** - 正在核对未跟踪实现文件和 tracked docs diff,随后重新跑关键验证命令。

## [2026-06-01 18:15:12] [Session ID: codex-native-20260601-window-ref-final-check] [完成]: `target.window_ref` fresh closeout

### 完成状态
- [x] 核对当前工作区里 `control_web` / protocol parser / docs skill 的实现范围。
- [x] 运行 fresh 格式化与测试验证。
- [x] 给用户交付当前状态,明确 `@gui-probe` 后续单独做。

### 验证证据
- `cargo fmt --check`: 通过。
- `git diff --check`: 通过。
- `cargo test --package rustdog --bin rdog -- control_web --quiet`: 17 passed。
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::web_gui --quiet`: 5 passed。
- `cargo test --package rustdog --bin rdog -- gui_bench --quiet`: 16 passed。
- `cargo test --package rustdog --test computer_use_density --quiet`: 3 passed。
- `cargo test --package rustdog --bin rdog --quiet`: 314 passed。

### 当前状态
**本轮完成** - 已单独收口 `target.window_ref + observation_id`。真正的 read-only `@gui-probe` composite 仍保持未实现,应作为下一阶段单独做。

## [2026-06-02 13:47:42] [Session ID: codex-native-20260602-git-commit-push] [行动前]: git 提交与推送

### 即将执行
- 核对当前 mixed worktree 的 tracked / untracked 文件。
- 按主题拆分提交,优先提交已经验证过的 computer-use density / window-scoped web target 相关改动。
- 排除 `.codegraph`、`.cursor`、`.DS_Store` 和无关支线文件。
- 提交前运行 fresh verification,提交后推送 `main` 到 `origin`。

### 为什么这样做
- 当前工作区包含多条支线和未跟踪文件,不能使用 `git add -A`。
- 用户要求“提交 推送”,但项目规则要求提交前必须重新验证,并且不能误带用户或其它支线改动。

### 当前状态
**提交准备中** - 正在审查 diff 和提交边界。
