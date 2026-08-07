## [2026-07-28 22:45:00] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义 replay preflight、guard 与 verification policy (ticket #4)

### 任务内容

- 落盘 Wayfinder ticket `#4` 的 resolution asset。
- 收敛已散落在 semantic promotion / window geometry / display-aware chain 三个 closed 规格里的 preflight / guard / verification 边界为统一 policy 入口。
- 提交到合适分支,不污染用户 dirty worktree。

### 完成过程

- 6 项 HITL 决策:
  - 8 个顺序 preflight gate
  - 11 个 stable reject reason code
  - state-mutating/state-read/input-primitives 三类 action 的 verification policy
  - strict 与 best-effort 的差异(best-effort 只放宽 verification 与 coordinate fallback)
  - 5 类 safety rollback trigger 与统一收口动作
  - Bundle provenance gate 作为 Replay 入口前置检查
- 写规格 `specs/rdog-replay-preflight-guard-verification.md`(290 行)。
- AGENTS.md 追加新规格长期文件索引。
- 由于用户已经切到 `auto-optimize/20260728-2316-rdog-control` 分支做 rdog-control 优化,ticket #4 的 commit 落在这个本地分支(28cbab0),不 push 不污染 main。
- 关闭 ticket #4 + 更新 Wayfinder map #2。

### 总结感悟

- 跨 ticket 推进时,先核对 `gh issue list` 状态,不要假设 ticket 编号。
- 在用户 dirty worktree 上 commit 时,要选最低影响路径(本地 commit + 不 push + 不切分支),不主动 merge 到 main。
- 用户切分支时,我的 commit 可以跟着该分支,而不必切回 main。等用户 merge 时一并处理。
- ticket 的 GitHub issue state 与 commit 在哪个分支无关,可以独立推进 issue 关闭。
- 把已散落的 rule 收敛到一个 policy 入口时,要复用现有 stable code 词汇(snake_case + structured_field),避免引入第二套命名空间。
