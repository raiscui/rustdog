---
title: 读直到安静类 helper 必须区分响应前/后安静 — 本地绿 CI 红的环境决定性家族
date: 2026-08-28
last_updated: 2026-08-30
module: recording-e2e
component: read_response_line / e2e read helpers
problem_type: test_failure
severity: high
status: active
tags:
  - ci
  - environment-deterministic
  - read-timeout
  - quiet-window
  - slow-runner
verified_by:
  - "main macos 3 连红 + PR 重跑不翻 (2026-08-28), 本地同测试全绿"
  - "修复 (PR #77): 安静返回加 @response 前置后, recording 6/6 本地绿 + CI macos recording 全过 (首轮 process_lease 抽签, 重跑绿)"
  - "同族复发即指纹: 2026-08-28 recording_manual_cancel 族 stable 红, 修 read_response_line 后消失"
root_cause: "read_response_line 注释写'读直到安静', 实现是首次 200ms WouldBlock 即返回; 慢 runner 上 daemon 处理请求 >200ms 时读到响应前安静 -> 空串 -> parse().unwrap() None; 快机器 <200ms 永远绿"
resolution_type: "安静返回加语义前置: output 已含 '@response ' 才 return, 否则 continue 等 deadline; helper 的注释与实现必须同义"
---

# 读直到安静类 helper 必须区分响应前/后安静 — 本地绿 CI 红的环境决定性家族

## Problem

"read until quiet" 是 e2e 里常见的响应读取策略 (响应可能多帧/分包,
不能按单条边界读)。但当"安静"被实现为**任意一次读超时就返回**时:

- 慢 runner (CI) 上 daemon 处理时间 > quiet 窗口 → 读到的是"响应前的安静"
  → 空串/残缺 → 上游 unwrap panic;
- 快机器 (本地 dev) 处理 < quiet 窗口 → 永远读到完整响应 → 永远绿。

这是环境决定性失败的又一个家族成员 (与 TERM=dumb 同族):
**本地绿 + CI 稳定红 + 重跑不翻**, 却常被误判为 flake 抽签。

## Guidance

1. **安静语义必须二分**: "响应后安静" (已见过响应标记, 多帧间安静, 收工)
   vs "响应前安静" (还在等响应, 继续等到 deadline):

```rust
Err(WouldBlock | TimedOut) => {
    if output.contains("@response ") { return output; }  // 响应后安静: 收工
    continue;                                             // 响应前安静: 继续等
}
```

2. **helper 注释与实现必须同义**: 注释写"读直到安静"而实现是
   "首次安静即返回"就是这次事故的直接诱因 — 文档描述的是理想语义,
   代码是退化实现, review 时没人对齐;
3. **判别指纹**: 本地全绿 (含同测试) + CI 稳定红 + 重跑不翻 = 环境决定性,
   优先怀疑时序窗口类 helper, 而不是调 retry/timeout。

## Evidence

- ERRORFIX [2026-08-28 21:35] 完整记录 H1 (探活) 证伪 → H2 成立链;
- PR #77 修复 11 行, 修复后该族从 main 的稳定红消失 (后续 PR #84/#90/#91/#92
  的 macos 均绿);
- 家族谱系: TERM=dumb (tty-term solution) / 本条 (quiet window) /
  GNU kill 负 pid (logic-errors solution) — 三者都是"本地与 CI 环境差异
  决定性失败", 诊断入口相同。

## Why This Matters

稳定红污染 main 的 CI 信号, 所有后续 PR 的"与 main 同轮比对"口径失效;
且 unwrap None 的 panic 点离真因 (读 helper) 隔了一层, 排障成本高。

## When to Apply

- 任何 "read until quiet / read until marker" 类 e2e helper;
- 多帧响应 (savefile 前缀 + response) 的读取;
- 快慢机器行为差异明显的测试 (CI runner vs 本地 dev)。

## When Not to Apply

- 单帧定界协议 (能按行/按标记直接读);
- 已有显式长度前缀的帧协议。

## Related

- docs/solutions/test-failures/tty-term-dumb-environment-deterministic-failure.md
  (家族诊断纪律: 显式固定假设环境)
- docs/solutions/logic-errors/gnu-coreutils-kill-neg-pid-argument-ambiguity.md
  (同族: 本地/CI 环境差异决定性)
- ERRORFIX.md [2026-08-28 21:35]
