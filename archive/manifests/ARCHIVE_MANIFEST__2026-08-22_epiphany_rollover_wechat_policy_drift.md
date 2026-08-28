# Archive Manifest: EPIPHANY_LOG 续档 (WeChat 政策漂移发现触发)

- 日期: 2026-08-22
- Session: zcode-sess_fa3b551c (continuous-learning 全量整理)

## 归档对象

| 原路径 | 新路径 |
| --- | --- |
| `EPIPHANY_LOG.md` (999 行) | `archive/default_history/EPIPHANY_LOG_2026-08-22_124616_before_wechat_policy_drift_rollover.md` |

## 为什么归档

- 旧档 999 行, 本轮需追加 "skill token 优化静默删除 WeChat 安全政策" 重大发现,
  追加即超过 1000 行续档线, 按规则先归档再续新档。
- 归档发生在 continuous-learning 完整流程中 (Capture 已完成, 校验已通过), 符合
  "未执行 Capture 与验证不得归档" 的前置条件。

## 旧档内容处置 (31 条逐一载体核查)

- Zenoh 边界/身份/duplicate/queryable (2026-04 ~ 2026-05, 7 条):
  由 `specs/zenoh-control-plane-plan.md` / `specs/bidirectional-control-plane-plan.md` /
  `EXPERIENCE.md` duplicate-name 条目承接。
- PTY 相关 (2026-05, 5 条): 由 `specs/pty-control-plan.md` 与
  `EXPERIENCE.md` [2026-05-07] 条目承接。
- CLI 更名权限主体 (2026-05-11): 由 `EXPERIENCE.md` 同主题条目 +
  `docs/solutions/best-practices/macos-tcc-stable-codesign-identity.md` 邻接承接。
- log stdout/stderr 事故 (2026-06-19, 3 条): 本轮新 Capture
  `docs/solutions/conventions/daemon-log-sentinel-e2e-contract.md` 正式承接
  (此前仅 EXPERIENCE 索引)。
- unixpipe FIFO 与 flake 排查 (2026-06, 4 条): `specs/zenoh-unixpipe-fast-path-plan.md`
  与历史诊断记录承接, 问题已解决。
- heredoc/命令替换坑 (2026-07-16): 用户级 skill
  `self-learning.shell-heredoc-backtick-command-substitution` 承接。
- computer-act Phase F 收口系列 (2026-07-17, 5 条): 代码 + 测试 +
  `LATER_PLANS.md` LP-ticket-15 条目承接, 全部落地。
- parser 兼容 / 可恢复协议成本 / outcome 三态 (2026-08, 3 条):
  `EXPERIENCE.md` 索引 + `workflows/macos-ops-interaction-efficiency.md` +
  `docs/glossary.md` outcome 条目承接。

## 未迁移候选

- 无。旧档没有发现未被任何载体承接的活跃风险条目; 全部为已落地或已承接历史。
