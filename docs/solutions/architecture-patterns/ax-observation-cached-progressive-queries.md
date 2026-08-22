---
title: "AX 查询复用 observation 缓存必须保持单一真相源与 fail-closed"
date: 2026-08-22
last_updated: 2026-08-22
module: control
component: ax-observation-cache
problem_type: architecture_pattern
severity: medium
status: active
tags:
  - ax-observation-cache
  - progressive-query
  - resource-epoch
  - fail-closed
  - read-only-cache
verified_by:
  - "cargo nextest run -j 2 --bin rdog -E 'test(cached) | test(bounded_query) | test(response_budget_preserves)' (2026-08-22 干净 HEAD worktree 复跑)"
  - "cargo test -j 2 --bin rdog control_ax::tests::parse_ax_tree_payload_should_require_cached_fields_as_a_pair -- --exact"
  - "cargo test -j 2 --bin rdog control_ax::tests::cached_ax_get_should_reject_resource_epoch_after_write -- --exact"
  - "cargo test -j 2 --bin rdog control_ax::tests::cached_ax_queries_should_fail_closed_for_unknown_observation_or_ref -- --exact"
  - "cargo test -j 2 --bin rdog control_ax::tests::cached_ax_get_should_normalize_expired_observation_reason_code -- --exact"
related_solutions:
  - docs/solutions/logic-errors/gui-resource-epoch-read-write-race.md
---

# AX 查询复用 observation 缓存必须保持单一真相源与 fail-closed

## Context

`@ax-tree` / `@ax-get` 缺省每次 live capture, 在大窗口上成本高。observation
(`@observe` / `@computer-act` 隐式观察) 已经产出过完整 AX snapshot, 后续只读查询
应能复用它降低 GUI 查询的请求成本 (借鉴 upstream Pi 的 observation cached query,
见 `docs/pi-computer-use-comparison.md`)。难点是复用不能破坏 GUI mutation 的
resource epoch 语义: mutation 之后, 旧 snapshot 的读必须失效, 否则 agent 会基于
mutation 前状态做决策。该机制由 2026-08-18 至 2026-08-20 的三个提交落地
(`feat(ax): add cached progressive queries`、`feat(ax): bound cached progressive
queries`、`test(ax): harden cached query validation`)。

## Guidance

缓存架构有六条不变量, 修改 `src/control_ax.rs` 缓存路径前必须全部保持:

1. **写入路径独占**: `AxObservationCache` 只在 observation 构建完成路径
   (`with_observation` 末尾调用 `register_ax_observation_snapshot`) 写入;
   查询路径 (`resolve_cached_ax_tree` / `resolve_cached_ax_get`) 只读。
   只缓存 `capture_status == "complete"` 且 `permission_status == "granted"` 的
   snapshot, LRU 容量 64 条。
2. **epoch 真相源分层**: 当前 resource epoch 的单一真相源在
   `src/control_resource_lane.rs`; 缓存 entry 只保存 capture-start 时从
   observation store 解析出的 per-ref epoch 快照, 注册时不重读当前 epoch。
   缓存永远不做 epoch 的权威判断, 只做一致性校验。
3. **失效粒度分两档**: `@ax-get` (带 ref) 只绑定 ref 所属的单个 PID resource,
   该 resource 的当前 epoch 与 capture 时一致即可复用; `@ax-tree` (无 ref) 绑定
   整个 observation, observation 内任一 PID resource 发生 mutation 即整体失效。
4. **epoch 双重比较**: 请求 `expected_epoch` 必须同时等于缓存记录的 capture
   epoch 和 resource lane 当前 epoch。前者保护客户端视角, 后者保护服务器视角;
   只比一边都会漏掉另一半 stale 窗口。
5. **fail-closed 收敛到稳定 error_code**: 未知 observation、缓存缺失、identity
   不一致、epoch 漂移统一返回 `stale_observation_cache`; observation store 的
   `STALE_REF` 归一为 `target_not_found`; 无 PID resource 归属返回
   `cache_unavailable`; 权限失效返回 `permission_denied`。retry hint 统一
   `re_observe_then_retry`。上游不得依赖 observation store 内部错误码或错误文本
   做分支判断。
6. **受限只读视图**: 缓存中的完整 snapshot 永不被修改。查询通过
   `bounded_for_query` / `bounded_for_query_with_target` 按 `depth` /
   `max_elements` / `include_values` 生成受限副本, 超限用 `truncated` 明示;
   `@ax-get` 保留目标节点所在路径。响应再走统一 response budget
   (`bound_response_line_with_limits`) 的字节/行边界。

协议入口 (`parse_ax_tree_payload`) 要求顶层 `observation_id` 与 `epoch` 成对
出现, 只写一个直接报错; 两者都缺省时保持 live capture 语义。这保证缓存查询是
显式 opt-in, 不改变既有请求行为。

## Evidence

- `src/control_ax.rs` 的 `AxObservationCacheEntry` / `AxObservationCache::insert`
  注释明确 "该缓存只由 observation 注册路径写入,查询路径只读"; `insert` 内部
  注释明确 "observation store 才是资源 epoch 的单一真相源"。
- `resolve_cached_snapshot` 的双路 epoch 校验: 带 `resource_key` 时比较
  `captured_epoch == expected_epoch && current == captured_epoch`; 无
  `resource_key` 时要求 `expected_epoch == observation.created_at_unix_ms` 且
  所有涉及 resource 的当前 epoch 与 capture 时一致。
- `resolve_cached_ax_get` 把 observation store 错误归一为
  `target_not_found` / `stale_observation_cache`, 不透传内部错误。
- `src/control_ax/types.rs` 的 `AxTreeRequest.observation_id` / `epoch` 字段
  注释: "只读缓存查询使用的 observation 身份。缺省时保持 live capture 语义"。
- `cargo nextest run -j 2 --bin rdog -E 'test(cached) | test(bounded_query) | test(response_budget_preserves)'`
  覆盖 round trip + stale epoch 拒绝、write 后 epoch 拒绝、未知 observation/ref
  fail-closed、reason code 归一、pair 校验、受限视图不改缓存原件、response
  budget 多字节边界。2026-08-22 在干净 HEAD (8af9e12) 独立 worktree 复跑:
  13 passed, 0 failed。其中 `ax_tree_executor_uses_cached_observation_without_live_capture`
  证明缓存命中的 `@ax-tree` 不再触发 live capture;
  `cached_ax_get_executor_returns_stable_target_not_found_code` 证明 executor
  层拿到的就是稳定 reason code。

## Why This Matters

- 缓存与 epoch 真相源混写会让 `gui-resource-epoch-read-write-race.md` 固定的
  capture/mutation 交错保护形同虚设: 注册时重读 epoch 会把 "capture-start 快照"
  变成 "注册时状态", 掩盖 dispatch 交错。
- epoch 只比单边会留下另一半 stale 窗口: mutation 后客户端还持旧 epoch 时,
  服务器侧比较能拦住; 服务器 epoch 被其它路径推进时, 客户端比较能拦住。
- 上游若依赖 observation store 内部错误码, observation 生命周期演进 (TTL、清理、
  schema 变化) 会连锁破坏 AX 查询协议; 收敛到缓存协议自己的 reason code 后,
  内部实现可以自由演化。

## When to Apply

- 给任何 "读路径复用先前 capture" 的能力 (AX、window、screenshot manifest、
  web) 设计缓存时。
- 请求协议需要区分 "live capture" 与 "observation-scoped 读" 时。
- 缓存命中判断需要绑定 GUI mutation 语义时。

## When Not to Apply

- mutation 路径不得走这个缓存: write 必须重新经过 resource lane 的 epoch 推进
  与 successor observation 契约 (见 `specs/rdog-computer-act-spec.md`)。
- 无 PID resource 归属的目标 (纯坐标) 没有 epoch 可比, 直接 `cache_unavailable`,
  不猜资源归属。
- 权限已失效的缓存 (`permission_status != "granted"`) 不复用, 重新走 live
  capture 才能重新触发权限提示。

## Examples

```text
@ax-tree:{observation_id:"obs-abc",epoch:1234,depth:3,max_elements:400}
@ax-get#7:{target:{ref:"@e2",observation_id:"obs-abc"},epoch:1234}
```

两个请求都不触发新的 AX capture, 直接复用 `obs-abc` 的缓存 snapshot 生成
受限视图; `epoch` 与服务器当前状态不一致时返回
`error_code:"stale_observation_cache"` 和 `re_observe_then_retry` hint。

## Related

- `docs/solutions/logic-errors/gui-resource-epoch-read-write-race.md`
  (write path 的 epoch 交错语义; 本文档是其 read path 对偶)
- `docs/pi-computer-use-comparison.md` (observation cached query 的 upstream 借鉴来源)
- `specs/rdog-computer-act-spec.md` (observation_id + epoch 的 mutation 消费契约)
