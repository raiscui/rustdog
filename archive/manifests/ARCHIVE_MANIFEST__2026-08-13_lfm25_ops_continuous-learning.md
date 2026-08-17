# `lfm25_ops` 支线上下文归档

## 归档日期

2026-08-13

## 归档原因

`lfm25_ops` 已完成上游 Pi macOS ops 迁移、全模型矩阵、TextEdit 多窗口契约修订和持续学习。当前没有待执行的支线实现工作,已验证的经验已进入正式长期知识库。

## 已完成的知识分流

| 候选 | 处置 | 长期载体 |
| --- | --- | --- |
| upstream Pi CLI 的工具选择、skill preload、隔离 agent 目录与 models schema | Capture | `docs/solutions/tooling-decisions/upstream-pi-macos-ops-cli-contract.md` |
| legacy Rust Pi 与 upstream v3 JSONL 的 route、多轮与工具事件聚合 | Capture | `docs/solutions/conventions/pi-jsonl-v3-semantic-aggregation.md` |
| TextEdit 多窗口运行时窗口基线 | Scoped Refresh: Keep | `docs/solutions/logic-errors/macos-ops-multi-window-runtime-baseline.md` |

`AGENTS.md` 已增加两份新 solution 的发现入口。`EXPERIENCE.md` 中已完成证据补齐的 upstream CLI 候选已从 inbox 正文移除,改为正式 solution 索引。未创建 skill 或 glossary,因为既有 runner 测试已提供稳定的复跑步骤,本轮也没有新术语。

## 验证证据

- 两份新增 solution 的 frontmatter 与 claims 校验均为 `OK`,无 claims flags。
- `PYTHONPATH=runner:vendor python3 -m unittest -v runner.test_pi_events runner.test_upstream_pi_contract` 通过,共 5 项测试。
- `git diff --check` 通过。

## 文件映射

| 原路径 | 归档路径 |
| --- | --- |
| `task_plan__lfm25_ops.md` | `archive/branch_contexts/lfm25_ops/task_plan__lfm25_ops.md` |
| `notes__lfm25_ops.md` | `archive/branch_contexts/lfm25_ops/notes__lfm25_ops.md` |
| `WORKLOG__lfm25_ops.md` | `archive/branch_contexts/lfm25_ops/WORKLOG__lfm25_ops.md` |
| `LATER_PLANS__lfm25_ops.md` | `archive/branch_contexts/lfm25_ops/LATER_PLANS__lfm25_ops.md` |
| `ERRORFIX__lfm25_ops.md` | `archive/branch_contexts/lfm25_ops/ERRORFIX__lfm25_ops.md` |

本支线没有 `EPIPHANY_LOG__lfm25_ops.md`,因此没有对应归档文件。

## 保留的后续事项

- 需要无 recoverable signal 的质量 baseline 时,按 interaction-efficiency workflow 对目标模型重新运行完整 8-case。
- Qwen 3.7 的 `role:AXWindow` 短格式错误是 canonical skill 兼容性候选,本轮未修改提示词或 rdog 协议。

LFM2.5 后续评测已按 2026-08-17 用户决定放弃,不保留重新运行矩阵的待办。归档内容仅保存已经发生的调查和验证事实。
