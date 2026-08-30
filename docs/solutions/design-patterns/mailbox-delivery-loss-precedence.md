---
title: mailbox 防丢优先于防重 — "未注册不缓存"被真实场景推翻的语义修正
date: 2026-08-29
last_updated: 2026-08-30
module: agent-messaging
component: task_control mailbox / agent_messaging
problem_type: design_pattern
severity: low
status: active
tags:
  - mailbox
  - at-least-once
  - delivery-semantics
  - design-revision
verified_by:
  - "pre-start e2e (PR #78 #76): agent 未启动时投递 -> agent 起后补拉恢复并回复, 修复后稳定绿"
  - "去重 e2e: 同 id 重投只留 1 条 + duplicate 计数 1 (通配缓存下保持)"
  - "容量淘汰: 256 上限丢最老 (dropped 计数), 跨主机重复缓存副本由它兜底"
root_cause: "首版'未注册不缓存'为解决跨主机重复缓存而设计, 但隐含'投递时消费方必须已在线'假设 — 与'agent 可下线、消息不丢'的 mailbox 核心承诺直接冲突"
resolution_type: "改为收到即缓存 (未注册自动建 entry); 跨主机重复副本由容量淘汰兜底; 去重窗口 (seen_ids) 保留防重"
---

# mailbox 防丢优先于防重 — "未注册不缓存"被真实场景推翻的语义修正

## Problem

agent mailbox (#73) 首版语义: daemon 通配订阅 inbox, 但只为"已注册的 agent"
缓存消息 — 意图是解决跨主机 pub 广播导致的多 daemon 重复缓存。

收口 e2e (#76) 的场景"先投递后起 agent"直接推翻了这个设计:
投递发生在 agent 注册之前 → 消息被丢弃 → agent 起后补拉为空 →
**mailbox 的核心承诺 ("agent 下线不丢消息") 在最典型的场景下失效**。

## Guidance

1. **投递语义分级**: at-least-once (可能重复, 不丢) vs at-most-once
   (不重复, 可能丢)。mailbox 的产品承诺是前者 — 所有实现细节必须服从;
2. **防重的正确层级是去重窗口** (seen_ids, 幂等消费),
   不是"拒绝缓存未注册者的消息";
3. **资源代价的兜底是容量淘汰** (256 条丢最老 + dropped 计数),
   不是提高投递门槛;
4. **设计评审要问"这个优化在哪个用户故事下会反噬"**:
   "未注册不缓存"在 user story #5 (下线恢复) 下直接反噬,
   这个问题本可以在 spec 评审时发现。

## Evidence

- 语义修正前后各一个 e2e: 修前 pre-start 场景稳定失败,
  修后 "先投递后起 agent" + "同 id 重投去重" 双场景稳定绿 (PR #78);
- 跨主机重复缓存担忧的实测: 通配 sub + 多 daemon 场景下副本随容量淘汰,
  无资源失控 (agent 只补拉本机 daemon)。

## Why This Matters

投递语义是消息系统的第一承诺。用"注册门槛"优化资源时牺牲它,
等于用次要需求侵蚀主要契约 — 这类修正越晚做, 依赖旧语义的上游越多。

## When to Apply

- 设计消息缓存/队列时, 明确 at-least-once vs at-most-once 并全程服从;
- 为资源保护加投递门槛前, 先对照全部 user story 找反噬场景。

## When Not to Apply

- 显式声明 at-most-once 的遥测/日志类通道 (丢弃可接受);
- 消费方在线率接近 100% 且丢失可重试的内部管道。

## Related

- specs/rdog-agent-messaging-plan.md (mailbox 契约)
- issue #76 (推翻场景的完整讨论)
