---
title: "Durable observation 根目录增长必须在物化和测试边界治理"
date: 2026-08-19
last_updated: 2026-08-19
module: control
component: durable-observation-store
problem_type: logic_error
severity: high
status: active
tags:
  - observation
  - durable-store
  - retention
  - test-isolation
  - filesystem
verified_by:
  - "cleanup dry-run classified 7422 empty test stores and rejected unknown or non-empty stores"
  - "quarantine verification reported 7422 verified moves and zero errors before deletion"
  - "cargo nextest run -j 2 --no-fail-fast: 936 passed, 21 skipped"
  - "real HOME observations root remained at 88 directories and 23 MiB after full test runs"
root_cause: "default durable stores were eagerly materialized per daemon name, while integration tests generated one-shot identities against the real user state root; per-store retention could not bound sibling directory growth"
resolution_type: "lazy materialization, dated single-store rotation, owner-locked age cleanup, explicit test isolation, and quarantine-first legacy cleanup"
---

# Durable observation 根目录增长必须在物化和测试边界治理

## Problem

默认 durable observation 以 daemon name 隔离状态。旧实现会在 daemon 启动时立即创建 store,
即使该 daemon 从未记录 observation。大量使用随机 daemon name 的集成测试因此持续污染真实用户目录。

单个 store 的 `retention_observations` 和 `retention_bytes` 只限制 store 内数据,
不能限制平台默认 observation 根目录下 sibling store 的数量。

## Symptoms

- 治理前平台默认 observation 根目录的一级目录达到 7,510 个。
- dry-run 识别出 7,422 个严格满足条件的空测试 store,总逻辑大小 3,316,471 bytes。
- 63 个含 observation 数据的目录、24 个不匹配测试 allowlist 的目录和 1 个含未知文件的目录被保留。
- 目录项本身的文件系统成本远高于空 JSON/metadata 的逻辑字节数,整体占用约 81 MiB。

## What Didn't Work

- 只调小单 store 的 count/byte retention: 无法约束 daemon name 对应的 sibling 目录数量。
- 根据 daemon name 在生产代码里猜测测试身份: 会把测试约定泄漏到运行时,也可能误删真实数据。
- 直接批量删除旧目录: 无法证明目录为空、无 selector、没有未知文件,不满足数据安全边界。
- 改成共享 SQLite: 不能修复测试写入真实 HOME,还会引入迁移、并发和新依赖。
- 立即增加 root size/count 配额: 当前没有真实生产 daemon-name churn 证据,无法合理定义淘汰顺序。

## Verified Root Cause

静态证据: 默认路径按 daemon name 建 store。旧 `open` 路径会立即创建 `tmp`、`meta.json` 和
`index.json`;普通 TCP、PTY、WebSocket 与 Zenoh 集成测试又使用一次性 daemon identity,
但没有关闭 durable observation 或传入临时 `state_dir`。每个 store 自己执行 retention,
根目录没有跨 store 的生命周期边界。

动态证据: dry-run 的 7,422 个候选全部命中已知测试 identity 规则,且 observation、selector
和 JSONL 均为空。隔离测试后,TCP 与 Zenoh smoke 前后真实 HOME 的目录计数不变。
quarantine 逐项复核 7,422 个 target 存在、source 不存在、持久数据为空,错误数为 0。

## Solution

1. 新 store 的 `open` 只建立内存空 index。第一次 `record_observation` 才创建目录和文件。
2. 平台默认路径使用 `observations/YYYY-MM-DD/<daemon-name>/`。日期表示 store 最近写入日期。
3. 跨日写入原子移动同一个 store,保留完整 observation 和 selector history,不按天分裂状态。
4. daemon 默认每 3,600 秒检查一次,只清理 7 天前的日期目录。
5. 清理和 store 物化共用 root maintenance lock;每个 daemon 再持有 owner lock,活动 store 不删除。
6. 清理器只删除能识别为 observation store 的子目录。未知目录保留并计入
   `skipped_unknown_stores`,日期目录只有完全为空时才删除。
7. 显式 `observation.state_dir` 仍是精确 store 路径,不增加日期层,也不参加默认 root 清理。
8. 普通集成测试显式关闭 durable observation。durable 专项测试必须使用临时目录。
9. 旧数据只通过 dry-run、严格 allowlist、同卷 quarantine、逐项复核、最后删除的顺序治理。

## Why This Works

延迟物化消除了“启动即产生空 store”的源头。测试隔离切断了随机 identity 写入真实 HOME 的路径。
日期目录提供了不扫描 store 内容也能判断的年龄边界,但 store 仍保持单一真相源。

root lock 避免清理与跨日移动并发,owner lock 保护活动 daemon。`looks_like_observation_store`
门槛让清理 fail closed,无法识别的数据不会进入删除分支。

## Verification

- `cargo test -j 2 --bin rdog control_observation::durable::tests::`
  - 12 passed,覆盖延迟物化、跨日移动、7 天边界、活动 owner 和未知目录保留。
- `cargo test -j 2 --bin rdog observation_`
  - 42 passed,覆盖 observation 和配置加载/校验。
- `cargo test -j 2 --test recording_e2e`
  - 5 passed,测试专用跨进程锁避免 host-global recording 能力并发污染。
- `cargo nextest run -j 2 --no-fail-fast`
  - 936 passed,21 skipped。
- `cargo check -j 2`
  - 通过,无 error。
- `rustfmt --check` 与 `git diff --check`
  - 通过。
- 一次性治理报告:
  - `/tmp/rdog-observation-cleanup-dry-run-20260819.json`
  - `/tmp/rdog-observation-quarantine-20260819.json`
  - `/tmp/rdog-observation-quarantine-verify-20260819.json`
  - dry-run 的 `by_rule` 是可重叠的分类计数,因此其合计可能高于唯一 `candidate_count`。本批以候选列表、`moved_count` 和逐项 verify 的唯一路径计数为审计主证据:三者均为 7,422,复核错误为 0。

## Prevention

- 新增 daemon E2E 时,默认关闭 durable observation;只有专项测试可以写临时 `state_dir`。
- 新的自动清理路径必须有明确格式识别、活动 owner 保护和未知数据 fail-closed 行为。
- retention 设计必须分别回答单 store 数据量和 root store 数量,不能用前者代替后者。
- 只有出现真实生产 identity churn 后,才增加 root-level size/count 配额和对应淘汰策略。

## Related

- `src/control_observation.rs`
- `src/control_observation/durable.rs`
- `src/control_observation/durable/tests.rs`
- `src/config.rs`
- `specs/rdog-observation-scoped-refmap-plan.md`
- `rdog_macos.toml`
- `rdog_linux.toml`
- `rdog_win.toml`
