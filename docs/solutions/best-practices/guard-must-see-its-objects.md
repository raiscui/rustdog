---
title: CI 守卫必须看见它要判的对象; 任何「读不出来」的分支默认要倒向红
date: 2026-11-26
last_updated: 2026-11-26
module: scripts/ci
component: 账本守卫 (check-bookkeeping.py) + target 执行器 (run-integration-targets.py)
problem_type: best_practice
severity: high
status: active
tags:
  - ci
  - guards
  - fail-closed
  - bookkeeping
  - merge-gate
verified_by:
  - "RAI-433 账本守卫: 修前 main 上的范围恒为 1 个 merge commit (它恰好落进「只改账本」豁免), 造一个不写账本的 PR 合进 main → exit 0; 修后同一形状 exit 1, 范围 50 个 commit / 已记账 29 (2026-11-26, #166)"
  - "占位超期改按时间判 + 日期 fail-closed: 三种坏日期 (缺失 / 垃圾 / 在未来) 修前全部 exit 0, 修后全部 exit 1 且原因分别点名 (2026-11-26, #170)"
  - "参照点只认 HEAD 一线父链: 分支内 `merge main`(-5d) + 今天落 main 的形状, 旧实现读「占位超期 5.0 天」红、新实现读「落点 0.0 天, 参照 <落地 merge>」绿 (2026-11-26)"
  - "per-target 墙钟: 挂在 target 上的超时会撑到 job timeout 被 cancel, 日志只剩 The job has been cancelled; 加了 target 上限后点名 `<target> exit=124` (2026-11-26, #156)"
related_solutions:
  - docs/solutions/best-practices/parallel-test-global-state-single-lock.md
  - docs/solutions/conventions/e2e-isolated-home-credentials.md
---

# CI 守卫必须看见它要判的对象

## 症状

一个守卫长期报绿, 但它**从来没看过要判的东西**。红不红都不说明问题, 因为范围里根本没有对象。

2026-11-26 一天之内在同一个账本守卫上撞了三次, 三次的形态不同, 方向完全相同 —— **默认倒向「放过」**:

| # | 形态 | 结果 |
| --- | --- | --- |
| 1 | `commit_shas` 带 `--first-parent`, 而 main 落的是 merge commit, PR 的工作 commit 在第二父线上 | 范围恒为 1 个 merge commit, 而它恰好落进「只改账本」豁免 → 任何 PR 都能不写账本绿灯合入 |
| 2 | 用「位置数」当「多久算忘」 (`--main-grace 3`) | 一次合并自带 5 个 commit 就超过宽限 → 合入当天 main 立刻红; 抬到 10 后仍有 26 个 commit 的合并会红 |
| 3 | `commit_epoch` 的 `except ValueError: return time.time()` | 日期读不出来 = 「还很新」 = 永不引爆; 缺失 / 垃圾 / 未来 三种坏日期全部 exit 0 |

## 判据 (写守卫时逐条问)

1. **它数哪些对象?** 在「一次合并前后」各数一遍, 数字应当是同一量级。若合入前 1、合入后 1, 那它没看见合并带进来的东西。
2. **第一父链还是全历史?** main 上的工作 commit 在 merge 的第二父线上; 只走 `--first-parent` 就永远看不见它们。反过来, **merge commit 本身不该要求记账** —— 它的 sha 在创建之前不可能出现在账本里, 要求它记账是逻辑上做不到的事, 训练出来的只是「反正它红着」。
3. **「多久算忘」用时间还是位置?** 位置口径要一直追着最大的合跑 (实测本仓最近 12 次合并的工作 commit 数 1–26 不等), 无论阈值调到几, 下一张更大的 PR 一合入就红。时间口径与合并规模解耦。
4. **时间从哪一刻起算?** 必须从「**落到 main 的那一刻**」起算: 只认 HEAD 的**一线父链**上「最早的、以它为先祖的那个 commit」。分支内 `merge main` 的同步 merge 不在父链上, 用它当落点会让长寿命分支合入当天就红 (本仓近 35 次合并里有 4 次是同步 merge, 触发面是真的)。
5. **任何一个「读不出来 / 解析失败 / 空值」分支, 先问它倒向红还是倒向绿。** 兜底成 `now()`、兜底成「跳过」、兜底成「宽限」都是倒向绿。
6. **输出要能自证。** 只说「落 main 已 N 天」时, 一旦挑错了参照点, 读日志的人无从发现; 带上参照 commit 的短 sha (`参照 <sha>`) 之后, 输出就没法替你撒谎。
7. **上限要落在「会被杀的那一层」之外。** per-target 超时只终止直接子进程时, `cargo test` 派生的 daemon 会留在 runner 上污染同 job 后续 target; 预算也要按 job 的真实剩余时间算, 而不是「本步开始 + 常数」。

## 修法与代价

- 把「对象集合」和「判据」分开表达, 别用一个数字同时承担两者 (#170 的最终形态: 无条目 → 位置宽限; 有占位 → 时间超期)。
- 守卫自己要有**负对照**: 造一个应该红的输入, 确认它真的红; 再造一个「只差一点点」的输入, 确认边界移动正确。今天三次假绿的负对照都是**形状恰好绕开失效分支** —— 造形状时要问「我改的那一行在什么输入下会被走到」, 而不是「我能不能跑通一个例子」。
- 成本: 一线父链上找落点用二分 (`git merge-base --is-ancestor` 单调), 链长 403 时约 200ms/占位; 父链列表**一次运行只取一次**, 每个 sha 只算一次并复用。

## 相关

- `docs/agents/ci-merge-gate.md` §3 / §3b / §3c: 红点怎么读、target 覆盖与三层墙钟、堆叠 PR = 未验证
- `docs/agents/knowledge-carriers.md` 约定 ③: 账本按票分文件 + sha 合入后补 + 两个判据
- `docs/solutions/best-practices/parallel-test-global-state-single-lock.md`: 同一族教训的另一面 (锁只保证串行, 不保证隔离)
