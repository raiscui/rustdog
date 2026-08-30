---
title: zenoh pub 的即时性幻觉 — 声明异步传播与无匹配订阅者即丢
date: 2026-08-29
last_updated: 2026-08-30
module: agent-messaging
component: zenoh session / e2e tests / task_control
problem_type: integration_issue
severity: medium
status: active
tags:
  - zenoh
  - pub-sub
  - declaration-propagation
  - e2e-timing
  - mailbox
verified_by:
  - "agent runtime e2e (PR #78): reply_sub 声明后 1s 传播窗口修复前稳定失败, 修复后 20 轮绿"
  - "对拍实验 (PR #92 排障): 同款 pub 在 sub 声明存在与否下行为不同, 排除声明顺序/pure pub 模式变量"
  - "mailbox e2e: publisher 声明后 sleep 200ms 的窗口让全链稳定 (1034/1034)"
root_cause: "zenoh 的 declare_subscriber/declare_publisher 是异步传播到 router 的; pub 在订阅声明传播完成前发出时无匹配路由即被丢弃 (无缓存无重试)"
resolution_type: "e2e/集成测试契约: sub 声明后留传播窗口 (200ms-1s) 再触发对端 pub; 同一 session 内先声明 sub 再 pub"
---

# zenoh pub 的即时性幻觉 — 声明异步传播与无匹配订阅者即丢

## Problem

把 zenoh 的 pub/sub 当成"声明即可用"的即时语义写测试或集成代码时,
会出现**低概率但确定性的消息丢失**:

- reply_sub 声明后立即投递 task → agent 的回复 pub 到达时订阅声明
  尚未传播到 router → 回复无匹配被丢 → 测试在 15s 超时上挂死;
- 该失败在 daemon 忙时 (声明传播快) 消失, 在空闲时出现 — 呈现
  "有时过有时不过"的假 flake 形态, 实为传播窗口竞态。

## Guidance

1. **先声明, 留窗口, 再触发对端**: sub 声明后 sleep 200ms~1s (按流量
   估算传播时间), 再触发对端的 pub;
2. **同一 session 内组合声明**: reply_sub 与 task publisher 放同一 session,
   session 必须活到等待结束 (drop 后 sub 失效);
3. **daemon 侧证据链**: 关键投递路径加 INFO 日志
   (`mailbox_deliver` 的 "mailbox delivered"), 测试失败时能区分
   "pub 未达" vs "订阅者未收";
4. **pub 无缓存**: 不要假设 zenoh 会为"稍后上线的订阅者"补投 —
   持久语义要用 queryable 补拉显式实现 (rdog mailbox 的 design)。

## Evidence

- `tests/zenoh_router_client.rs` agent_runtime e2e: 修复前 15s 超时稳定失败,
  agent 日志显示 "handled task, replied_to=..." (agent 认为已回复) —
  pub 与 sub 永远错过;
- 修复 (sub 声明 + 1s 窗口) 后 20+ 轮全量绿 (1034/1034);
- mailbox e2e 的 publisher-declare-then-sleep-200ms 模式零失败。

## Why This Matters

这类竞态被误诊为 flake 会导致: 加 retry 掩盖 (真实丢消息风险移入生产)、
调大 timeout 掩盖 (慢化所有测试)、或在错误层修 (给 agent 加回复重发)。
正确修法是把"声明传播窗口"写进测试与集成的时序契约。

## When to Apply

- 任何 zenoh sub 声明后立即依赖该订阅收到消息的测试/代码;
- agent/服务互发消息的双向场景 (A 声明收 B 的回复, 再触发 B);
- 低流量环境 (daemon 空闲时传播窗口反而更长)。

## When Not to Apply

- queryable request/reply (`.get()` 自带匹配语义, 无此问题);
- liveliness token (有持久语义, 晚加入可查);
- 同一 payload 已有 mailbox 类持久兜底的路径 (丢消息可补拉)。

## Related

- specs/rdog-agent-messaging-plan.md (mailbox 补拉即本问题的持久化正解)
- docs/solutions/test-failures/tty-term-dumb-environment-deterministic-failure.md
  (同类"假 flake 实为确定性失败"的诊断纪律)
- src/task_control.rs (PTY 同款 25ms 轮询模式 — 拉模式规避本竞态)
