---
title: e2e 子进程 HOME 隔离与三方同源凭证 — spawn 点全覆盖纪律
date: 2026-08-29
last_updated: 2026-08-30
module: e2e-tests
component: zenoh_router_client tests / auth / tls material
problem_type: test_failure
severity: medium
status: active
tags:
  - e2e
  - home-isolation
  - credentials
  - usrpwd
  - spawn-points
verified_by:
  - "PR #92: 19 处 spawn 点 HOME 注入后全量 1034/1034 (此前部分测试的通过是凭证匹配噪声)"
  - "对拍: test session 用真实 HOME 凭证连隔离 daemon 时 usrpwd 全拒 (连接探测不通), 修复后 probe 通"
  - "tls e2e: 隔离 HOME 内 tls-init 自持材料, 错 CA BadSignature 拒绝场景不受真实目录污染"
root_cause: "子进程继承测试进程的真实 HOME; 认证层落地后 daemon 读真实 ~/.rdog/auth.toml, 与测试 session 读到的凭证是否一致成了隐式耦合, 单点遗漏即全链失败且表象多样 (假 flake / 超时 / OpenSyn 被拒)"
resolution_type: "进程级 test_isolated_home (temp dir) + ensure_test_home_credentials 手写测试凭证 + 全部 spawn 点 (daemon/control/agent/script 包裹) 统一 .env(HOME) 注入; 测试 session 凭证与 daemon 同源"
---

# e2e 子进程 HOME 隔离与三方同源凭证 — spawn 点全覆盖纪律

## Problem

认证层 (usrpwd) 落地后, e2e 里存在**三方凭证源**:

1. daemon 子进程 (读 `$HOME/.rdog/auth.toml`);
2. control/agent 子进程 (同上);
3. 测试进程自己的 zenoh session (直投消息用)。

任何一方 HOME 不一致 → 凭证不匹配 → OpenSyn 被 GENERIC 拒。
失败表象高度多样 (pub 未达 / query 超时 / 注册失败), 且**部分测试碰巧通过**
(恰好在真实 HOME 与测试 HOME 用同一份凭证时), 让问题呈现随机态。

## Guidance

1. **进程级隔离 HOME** (temp dir, 测试二进制进程内 OnceLock);
2. **手写测试凭证** (不走 daemon 启动生成 — 太重), daemon/client/session 同读一份;
3. **spawn 点穷举**: daemon `-c config` / control 各 helper / agent 子进程 /
   `script -q` 包裹的 TTY control (env 要传给 script, script 传给子进程);
4. **新增 spawn 点 checklist**: 写子进程 spawn 时 grep
   `Command::new(rdog_binary_path())` 确认上方有 `.env("HOME", ...)`;
5. **凭证读取源同源**: 测试进程内读凭证的 helper 必须读隔离 HOME
   (不是 `std::env::var("HOME")`) — 见 silent-replace-anchor-assertion
   (这条正是静默替换失败的受害者)。

## Evidence

- PR #92: 19 处 spawn 点, 全量 1034/1034;
- 排障记录 ERRORFIX [2026-08-29]: 五步证伪链 + 对拍测试
  `diag_pub_reaches_mailbox_with_and_without_agent` (保留作快速判别器);
- 对比 2026-08-19 的 RDOG_ZENOH__ENABLED=false 隔离 (配置分层污染):
  本次是凭证层污染, 同属"子进程读真实用户数据"家族。

## Why This Matters

认证把"测试环境一致性"从 nice-to-have 变成 hard gate:
凭证不匹配 = 连接被拒 = 全链失败。而 spawn 点是逐个出现的
(每加一个测试可能加一个 spawn), 没有纪律就会逐个漏。

## When to Apply

- 任何 spawn rdog 二进制的 e2e (daemon/control/agent/tls-init);
- 认证或 TLS 开启后的全部集成测试;
- 测试进程自己开 zenoh session 的场景。

## When Not to Apply

- 纯单测 (不 spawn 子进程);
- 显式测试真实用户配置的专项 (应明确标注并自带隔离)。

## Related

- docs/solutions/test-failures/silent-replace-anchor-assertion.md
  (本次隔离改造中静默替换失败的直接受害者)
- docs/solutions/best-practices/parallel-test-global-state-single-lock.md
- ERRORFIX.md [2026-08-29]
