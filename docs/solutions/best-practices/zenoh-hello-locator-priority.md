---
title: "Zenoh autodiscovery 不能盲信 Hello locator 顺序"
date: 2026-08-22
last_updated: 2026-08-22
module: zenoh
component: zenoh-runtime-session
problem_type: best_practice
severity: medium
status: active
tags:
  - zenoh
  - autodiscovery
  - locator-priority
  - multi-nic
  - windows
verified_by:
  - "cargo nextest run -j 2 -E 'test(locator_priority)' (2026-08-22 干净 HEAD worktree 复跑)"
  - "src/zenoh_runtime/session.rs:110-194 autodiscover_router_endpoints + prioritize_hello_locators + locator_sort_key"
---

# Zenoh autodiscovery 不能盲信 Hello locator 顺序

## Context

多网卡主机 (典型: Windows + VPN/虚拟网卡) 上, Zenoh router 的 Hello 可能带出多张
网卡的 locator。如果前面排着多个 `169.254.*` link-local / tentative 地址, 把
"发现到了 router" 直接交给 `zenoh::open()` 内部按原始顺序逐个连接, 3 秒 scouting
窗口会先被这些慢连接耗尽, 还没轮到真正可达的 LAN IP - 表现为 "明明看见了 router
却连接超时"。2026-04 的 Windows 现场调试确认了这一模式; 延长 timeout 只能缓解,
不能修复。

## Guidance

1. **manual scout -> 排序 -> 显式 open**, 不把连接顺序交给 `zenoh::open()` 内部:
   自己先 `zenoh::scout(WhatAmI::Router, ...)` 拿到 Hello, 对 locator 排序后再把
   排序结果作为显式 connect endpoint 交给 `zenoh::open()`。
2. 排序优先级 (本仓库 `locator_sort_key` 的实现):
   - `0` loopback (本机 router 最快);
   - `1` 普通可达 IP (真实 LAN);
   - `2` link-local (`169.254.*` 等, 慢且经常不可达);
   - `3` 无法解析的 locator 兜底。
3. **先过滤 serial locator**: 串口 locator 不是 TCP 可达 endpoint, 混入排序队列
   只会拖慢连接。
4. 显式 `--entry-point tcp/<preferred-ip>:<port>` 正常而 autodiscovery 超时的
   现场, 优先怀疑 locator 顺序而不是 daemon / queryable 故障。
5. scout 循环必须有自己的 deadline 与超时错误语义 (`recv_timeout` + 剩余时间
   计算), scout 提前结束 (Err) 与超时要区分报错。

## Evidence

- `src/zenoh_runtime/session.rs` 的 `autodiscover_router_endpoints` 顶部注释块
  原样记录了 Windows 多网卡死地址问题与 "先 scout 一次, 排序后显式 open" 的
  决策; `prioritize_hello_locators` 过滤 serial 后按 `locator_sort_key` 排序去重。
- `src/zenoh_runtime/session/tests.rs::locator_priority_should_prefer_preferred_tcp_over_link_local_and_serial`
  锁定优先级语义; 2026-08-22 在干净 HEAD (8af9e12) 独立 worktree 复跑:
  1 passed, 0 failed。
- 该模式自 2026-04 落地后, `specs/zenoh-control-plane-plan.md` 的 "autodiscovery
  默认 + `--entry-point` fallback" 口径保持稳定。

## Why This Matters

- 连接层 "看得到连不上" 的故障最容易把排查引向错误方向 (daemon 状态、防火墙、
  queryable 注册), 白耗现场时间; locator 顺序是第一优先怀疑项。
- 盲目加 timeout 会把 3 秒症状变成 10 秒症状, 还掩盖真实原因; 排序是一次性
  修复, 跨平台共享。

## When to Apply

- 任何把 Zenoh autodiscovery/autoconnect 用于多网卡环境的 client (macOS 的
  VPN 接口同样会出现虚拟网卡地址)。
- 新增 transport 或修改 `resolve_target` / session open 路径时, 保持排序逻辑
  在 scout 结果与 `zenoh::open()` 之间, 不能绕过。

## When Not to Apply

- 用户显式指定 `--entry-point` 时直接使用指定 endpoint, 无需 scout 排序。
- 单网卡、纯 loopback 的同机 unixpipe fast path 不经过这条路径。

## Related

- `specs/zenoh-control-plane-plan.md` (autodiscovery 默认 + entry-point fallback 的产品口径)
- `specs/zenoh-unixpipe-fast-path-plan.md` (同机 fast path, 不走 scouting)
