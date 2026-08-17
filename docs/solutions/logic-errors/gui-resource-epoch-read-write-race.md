---
title: "GUI resource epoch 必须覆盖 capture 与 mutation 的完整交错"
date: 2026-08-13
last_updated: 2026-08-13
module: control
component: observation-resource-lane
problem_type: logic_error
severity: high
status: active
tags:
  - computer-act
  - observation
  - concurrency
  - stale-write
  - resource-epoch
verified_by:
  - "same observation and PID probe: both mutations reached dispatch before the fix"
  - "cargo nextest run -j 2 --bin rdog: 812 passed, 1 skipped"
  - "successor observation epoch regression: old epoch + 2, old ref stale, successor ref writable"
root_cause: "observation epoch only identified capture creation time; mutation neither consumed a per-PID version nor invalidated captures started while dispatch was in flight"
resolution_type: "daemon-owned per-PID lane with capture-start epoch snapshots and pre/post-dispatch epoch increments"
---

# GUI resource epoch 必须覆盖 capture 与 mutation 的完整交错

## Problem

两个客户端可以从同一 observation 取得同一 PID 的 ref,随后并发执行 mutation。
旧的 wire `epoch` 只等于 observation 创建时间,因此两条请求都能越过 fast reject。

只在 dispatch 前增加一次 PID epoch 仍不完整。capture 如果在 epoch 已增加、底层副作用尚未完成时开始,
可能读取旧 UI,却被标为 mutation 后的新版本。

## Symptoms

- 修复前的同步 probe 证明,同 observation、同 PID 的两条 mutation 都能进入 dispatch。
- TCP 不同连接和 Zenoh query 可以并发进入共享 executor,单连接顺序不能提供 daemon 级保护。
- capture 与 observation record 分离时,record 阶段读取当前 epoch 会把旧内容错误标为最新状态。

## What Didn't Work

- 直接把 wire `epoch` 改成 PID write epoch: 一个 observation 可以包含多个 PID,单一顶层值没有明确语义。
- 只在 observation record 时读取当前 PID epoch: capture 期间发生的 mutation 会污染版本归属。
- 只在 dispatch 前递增一次: dispatch 进行中开始的 capture 仍可能取得新 epoch 和旧 UI。
- 按 window id 分 lane: 首版无法证明同进程不同窗口的 AX/input mutation 可以安全并行。

## Verified Root Cause

静态证据: `check_observation_epoch_fast_reject` 只比较 observation 的 `created_at_unix_ms`。
旧路径随后直接进入 `dispatch_underlying`,没有 per-PID compare-and-increment。AX/window capture 又在采集完成后才创建 observation。

动态证据: 最小同步实验让两条共享 observation/ref/PID 的 mutation 同时抵达 dispatch probe,测试在修复前通过。
加入 resource lane 后,并发测试只允许一条进入;capture-before-write 和 capture-during-write 两种交错都会在 mutation 完成后 stale。

## Solution

1. daemon 用 PID 作为第一版 resource key,维护共享 epoch map 和 per-PID dispatch lock。
2. AX 与 window producer 在真实 capture 前复制一致的 epoch token,record 时按最终 refs 保存各 PID 快照。
3. ref mutation 在 PID lane 内比较 observation 快照与当前 epoch。
4. dispatch 前将 epoch 增加到进行中版本,dispatch 返回后无论成功失败再增加到完成版本。
5. stale mutation 返回 `stale_resource_epoch` 与 `retry.strategy = "re_observe_then_retry"`。
6. wire observation epoch 保持创建时间语义;坐标动作没有可靠 PID 时不猜资源归属。
7. mutation 完成后 successor capture 必须发生在 PID lane 的 post-increment 之后,
   这样返回的 observation 才能直接作为下一条 mutation 的基线。

## Why This Works

同 PID lock 保证 compare、两次递增和 dispatch 形成一个串行区间。capture 不持有 lane lock,
所以只读路径不会阻塞 mutation;但它在 mutation 前或期间取得的 token 都会在完成态递增后失效。
不同 PID 只短暂共享 epoch map lock,底层 dispatch 仍可并行。

## Verification

- `cargo test -j 2 --bin rdog control_resource_lane::tests:: -- --nocapture`
  - 5 passed,覆盖 stale write、失败后失效、不同 PID 并行和 dispatch 期间 capture。
- `cargo test -j 2 --bin rdog control_computer_act::tests::epoch_check -- --nocapture`
  - 5 passed,旧 observation 创建时间 epoch 契约未回退。
- `cargo test -j 2 --bin rdog control_computer_act::tests::stale_resource_epoch_top_level_response_preserves_retry_contract -- --exact`
  - 1 passed,结构化 retry 没有在顶层包装丢失。
- `cargo check -j 2 --bin rdog`
  - 通过,无 warning 或 error。
- `cargo nextest run -j 2 --bin rdog`
  - 812 passed,1 skipped。
- `cargo test -j 2 --bin rdog control_computer_act::tests::successor_observation_can_drive_next_same_pid_mutation -- --exact --nocapture`
  - 1 passed,证明 successor 取得完成态 epoch,旧 ref stale,successor ref 可继续同 PID mutation。

## Prevention

- 新增 PID-backed observation producer 时,必须在真实采集前取得 capture token,不能在 record 时补采样。
- write epoch 必须覆盖完整不确定副作用区间,不能只标记 dispatch 开始。
- 任何新的 mutation seam 都应复用 daemon coordinator,不要在 transport 或 session 内复制状态。
- 并发测试至少覆盖 write/write、read/write、失败后失效和不同资源并行。

## Related

- `src/control_resource_lane.rs`
- `src/control_observation.rs`
- `src/control_computer_act/mod.rs`
- `specs/rdog-computer-act-spec.md`
- `docs/pi-computer-use-comparison.md`
