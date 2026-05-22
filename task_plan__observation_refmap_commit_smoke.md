# 任务计划: observation refmap P0-P5 commit 与 macOS live smoke

## 目标

把 observation refmap P0-P5 当前工作树按主题拆成本地 commits,然后用 `mac.lab` 做真实 GUI smoke,验证 `@observe -> ref target -> mouse fallback -> verify` 证据链。

## 阶段

- [ ] 阶段A: 工作树和上下文盘点。
- [ ] 阶段B: 按主题拆分 staging,形成可审查 commit 列表。
- [ ] 阶段C: 运行提交前验证并创建本地 commits。
- [ ] 阶段D: 用 `rdog control mac.lab` 跑 live GUI smoke。
- [ ] 阶段E: 记录证据,更新支线 WORKLOG / ERRORFIX,交付结果。

## 关键问题

1. 是否 push: 本轮用户只说提交,所以只做 local commits,不 push。
2. 是否处理 `.codex/skills/.DS_Store`: 这是 macOS 噪音文件,不属于 observation refmap,不提交。
3. 是否使用默认 notes: 不使用。默认 `notes.md` 已接近 1000 行,本轮写入支线文件。

## 做出的决定

- 决定: commit 按主题拆分,优先按 protocol/storage/action/docs/test evidence 分组。
- 理由: P0-P5 跨越 storage、selector、observe、mouse fallback、docs/skill 和结构减负,按主题比按时间更便于后续 bisect 和 review。

## 当前状态

**目前在阶段A** - 已读取 `rdog-control` skill、项目六文件、git status 和当前分支,准备分析 diff 分组。

## [2026-05-22 11:50:22] [Session ID: db93c592-07b8-4e82-af49-37791f2a5c8b] [记录类型]: commit 分组决策

### 已确认

- 当前分支是 `main`,HEAD 为 `6d4e5c1 Commit the full current workspace snapshot`。
- 当前没有子模块输出,本轮不需要处理 submodule 指针。
- `.codex/skills/.DS_Store` 是未跟踪 macOS 噪音文件,不纳入本轮提交。
- `task_plan__agent_desktop_review.md`、`notes__agent_desktop_review.md`、`WORKLOG__agent_desktop_review.md` 属于外部仓库对标支线,不纳入 P0-P5 commit。

### commit 主题

- Commit 1: runtime / protocol / tests。包含 Rust 源码、配置样例、Cargo 依赖和 selector fixtures。
- Commit 2: docs / skill / long-term spec。包含 README、AGENTS、specs 和 `.codex/skills/rdog-control`。
- Commit 3: execution records。包含默认六文件追加记录和 `__observation_refmap*` 支线上下文文件。
- Commit 4: 本轮 commit + live smoke 记录。只在 live smoke 完成后提交。

### 当前状态

**进入阶段B/C** - 准备先跑完整验证,再 staging Commit 1。

## [2026-05-22 11:50:59] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: 继续执行提交与 smoke

### 当前观察

- worktree 仍处于 P0-P5 整理提交前状态。
- 当前分支是 `main`,当前 HEAD 是 `6d4e5c1`。
- `.codex/skills/.DS_Store` 和 `__agent_desktop_review` 支线文件不属于本轮提交。

### 下一步行动

- 先按 runtime / protocol / tests 做 Commit 1 的 staging。
- staging 后检查 `git diff --cached --name-status`、`git diff --cached --stat` 和 `git diff --cached --check`。
- 如果 staged 文件只包含 P0-P5 runtime 主题,创建本地 commit。

### 当前状态

**继续阶段B/C** - 从上次未完成的 Commit 1 staging 开始执行。

## [2026-05-22 11:53:30] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: 用户切换到文档跟进

### 新目标

根据 observation refmap 的 P0-P5 语义,先同步更新 `rdog-control` skill、spec、README 和相关索引文档。

### 调整后的阶段

- [ ] 阶段F: 以 `specs/rdog-observation-scoped-refmap-plan.md` 和当前代码为真相源,盘点文档缺口。
- [ ] 阶段G: 更新 skill / spec / README / AGENTS 索引,确保 observation ref、selector、`@observe`、mouse fallback 的说法一致。
- [ ] 阶段H: 执行文档一致性检查和基础编译/测试检查。
- [ ] 阶段I: 再回到按主题提交与 live GUI smoke。

### 当前状态

**进入阶段F** - 已切换到 docs-sync 工作流,先只做文档跟进。

## [2026-05-22 11:55:45] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: 文档跟进盘点中

### 已确认

- `@observe` 的当前 schema 是 `rdog.observe.v1`。
- `@observe` 默认 `mode:"hybrid"`,并默认开启 visual、AX、window、manifest、refs、selectors 摘要。
- `target` 支持 `app` / `process` / `process_name`、`bundle_id`、`window_title` / `title`、`window_title_contains` / `title_contains`。
- `refs.sample[]` 当前包含 `section`、`observation_id`、`ref`、`kind`、可选 `name`。
- 鼠标 ref target 在动作前会重新解析 AX/window 当前 rect;坐标 target 标记为 `coordinate_fallback`; selector target 默认 no-action,显式 `auto_refind:true` 才可走 verified rebound。

### 遇到错误

- 只读盘点时误查了不存在的 `src/control_observation/store.rs`。
- 已用 `rg --files src/control_observation` 确认真实 durable 文件是 `src/control_observation/durable.rs`。

### 当前状态

**阶段F 继续** - 准备阅读 durable 文件和文档差异,然后做文档补丁。

## [2026-05-22 12:05:43] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: 文档跟进完成

### 已完成

- [x] 阶段F: 已对照 `src/control_observation/*`、`src/control_mouse/target.rs`、`src/config.rs` 盘点 observation refmap 的当前协议和状态文件。
- [x] 阶段G: 已更新 `rdog-control` skill、skill references、README、`AGENTS.md`、`specs/control-line-protocol.md`、`specs/code-agent-rdog-control-usage.md`、`specs/rdog-observation-scoped-refmap-plan.md`。
- [x] 阶段H: 已执行文档与协议相关验证。

### 验证

- `python3 /Users/cuiluming/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过。
- `git diff --check`: 通过。
- `cargo test --package rustdog --bin rdog --quiet`: 260 个测试通过。

### 当前状态

**阶段I 待继续** - 文档跟进已经完成,下一步回到按主题提交与 macOS live GUI smoke。

## [2026-05-22 12:08:18] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: Commit 1 完成

### 已完成

- [x] Commit 1: runtime / protocol / tests。
- commit: `e07362e Implement observation refmap runtime`

### 遇到错误

- 第一次 `git commit` 被 PreToolUse hook 拦截,原因是 inline commit message 缺少 Lore 格式和 `Co-authored-by: OmX <omx@oh-my-codex.dev>` trailer。
- 已按最近提交格式补齐 Lore trailers 后提交成功。

### 当前状态

**阶段B/C 继续** - 下一步 staging docs / skill / spec commit。

## [2026-05-22 12:10:25] [Session ID: 0bb1198d-77aa-4dd1-bf4d-65b82e83c8ea] [记录类型]: Commit 2 完成

### 已完成

- [x] Commit 2: docs / skill / long-term spec。
- commit: `bca4381 Document observation refmap workflows`

### 当前状态

**阶段B/C 继续** - 下一步 staging 执行记录 commit。
