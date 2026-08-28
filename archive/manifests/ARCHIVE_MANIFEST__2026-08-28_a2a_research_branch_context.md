# ARCHIVE MANIFEST — 2026-08-28 a2a_research 支线归档

## 归档范围

| 原路径 | 新路径 |
| --- | --- |
| `task_plan__a2a_research.md` | `archive/branch_contexts/a2a_research/task_plan__a2a_research.md` |
| `notes__a2a_research.md` | `archive/branch_contexts/a2a_research/notes__a2a_research.md` |
| `WORKLOG__a2a_research.md` | `archive/branch_contexts/a2a_research/WORKLOG__a2a_research.md` |

该支线无 LATER_PLANS / ERRORFIX / EPIPHANY_LOG 变体文件 (咨询调研型支线, 未产生错误修复与延期事项)。

## 为什么归档

- 支线任务链路已完成: A2A 协议调研 (v1.0 / Linux Foundation / 三 binding) ->
  channel 分发设计讨论 -> 伴生 agent 模式结论 -> herdr 用而不集成 ->
  daemon 并发模型验证 -> 演进方案正式化。
- 全部知识已由正式 spec `specs/rdog-task-spawn-control-plan.md` 承接
  (commit 6cf386a, 含 5 条决策记录与替代方案否决理由, AGENTS.md 已索引),
  Phase 1 实施已在 `feature/task-spawn-phase1` 分支进行中。
- 支线三文件最后记录时间为 2026-08-28 09:40, 收尾状态明确, 无未消费内容。

## 候选去向

- A2A 语义借鉴 / channel 双分发语义 / 伴生 agent 模式 / herdr 边界 / daemon
  并发真相 -> `specs/rdog-task-spawn-control-plan.md` (全部承接, 无未迁移候选)。
- "daemon 并发模型三条 lane + 同 session 串行" 的事实描述 -> spec 的动机章节
  (已含代码证据引用 zenoh_control.rs:212/195, daemon_bridge.rs:52/313)。

## 未迁移候选与证据缺口

无。本轮 Compound Gate 未在本支线发现需要额外 Capture 的成熟经验:
调研结论是设计输入 (spec 即正确载体), 不是可独立复现的 Bug/模式。

## 关联

- 本轮 continuous-learning 记录: `WORKLOG.md` [2026-08-28] 闲时整理条目
- LATER_PLANS 原 `[2026-08-26 18:24:18]` A2A 咨询结论条目已随本归档清理
  (内容由 spec 承接)。
