# ARCHIVE MANIFEST: 2026-08-09 continuous-learning 续档 (签名身份方案复盘)

## 归档时间

2026-08-09 22:35,Session: omx-1786268168901-f711dm

## 归档原因

完整 continuous-learning 复盘触发续档: `task_plan.md` 1056 行、`WORKLOG.md` 1002 行,
均超过 1000 行上限。

## 归档内容

| 原路径 | 新路径 | 原因 |
| --- | --- | --- |
| `task_plan.md` | `archive/default_history/task_plan_2026-08-09_223000_before_continuous_learning_rollover.md` | 超 1000 行续档 |
| `WORKLOG.md` | `archive/default_history/WORKLOG_2026-08-09_223000_before_continuous_learning_rollover.md` | 超 1000 行续档 |

新建当前档: `task_plan.md`、`WORKLOG.md` (含续档说明与当前活跃事项)。

## 已提取并 Capture 的候选

- **macOS TCC 授权身份稳定方案** (来自 2026-08-09 权限问题取证 + issue #40 实施):
  → `docs/solutions/best-practices/macos-tcc-stable-codesign-identity.md`
  → skill: `~/.codex/skills/self-learning.macos-codesign-stable-dr-check/`
  → 验证: frontmatter + claims 校验 0 flags; 本机动态证据 (DR 跨内容变化一致 + 真实 install 验证)
- **评测载体差异会被误判成模型退步** (来自 EXPERIENCE.md 2026-08-09 段, 经 bisect 8/8 x2 证据核验):
  → `docs/solutions/best-practices/eval-carrier-drift-vs-model-regression.md`
  → 验证: frontmatter + claims 校验 0 flags; bisect 结果与本会话证据链一致

## 未迁移候选及证据缺口

- EXPERIENCE.md 其余 20+ 段历史条目: 多数已被 specs/、rdog-control skill、AGENTS.md 索引承接,
  未逐条重新核验 (非本复盘 scope), 保留在原文件作为收件箱/索引描述。
- AGENTS.md / docs/ / specs/ 未发现因本复盘而过期的内容; specs/rdog-stable-signing-identity.md
  已作为 solution 的关联文档登记。
