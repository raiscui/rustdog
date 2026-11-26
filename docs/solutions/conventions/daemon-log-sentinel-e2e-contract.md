---
title: "daemon log 输出行是 e2e 测试的隐性启动 sentinel"
date: 2026-08-22
last_updated: 2026-11-27
module: tests
component: e2e-tests
problem_type: convention
severity: medium
status: active
tags:
  - e2e
  - log-sentinel
  - logger
  - daemon-startup
  - output-stream
verified_by:
  - "rg -c 'wait_until_output_contains' tests/zenoh_router_client.rs -> 28; tests/zenoh_router_client_windows.rs -> 3 (2026-08-22)"
  - "rg -n 'fn start_zenoh_daemon_with_combined_output' tests/ -> tests/zenoh_router_client.rs:174 与 tests/zenoh_unixpipe_fast_path/support.rs:236"
  - "ready 语义 (RAI-576, 2026-11-27): 修复前 10/10 轮 ready 先于 `agent inbox 通配 sub 已声明`, 修复后 8/8 轮声明先于 ready; `daemon_ready_should_be_logged_after_mailbox_subscriptions` 把 ready 挪回声明之前即红 (card=1917, inbox=1987, ready=1364)"
---

# daemon log 输出行是 e2e 测试的隐性启动 sentinel

## Context

`rdog` daemon 的 `log::info!` 输出被 e2e 测试当作 "启动就绪" 判定信号
(sentinel): 大量 Zenoh e2e 用 `wait_until_output_contains(buffer, "zenoh router
daemon ready", ...)` 之类的日志子串等待 daemon 可用。类似地, `Connection
Received` (listener)、`PTY ready`、`remote PTY closed` 等都是 e2e 关注的 log
marker。这些依赖没有集中声明 - 它们散落在各个测试的等待调用里, 是隐性契约。

2026-06-19 的一次教训: 把 `init_logger` 从 stdout 切到 stderr 看似一行修复,
实际连带 4 个 e2e 文件 (control_lanes / control_pty / shell_pty /
zenoh_router_client) 失败; 其中 zenoh_router_client 有 24+ 个测试只 pipe stdout,
当时用 `sh -c "exec rdog ... 2>&1"` wrapper 临时兼容, 后续才演进为合流 helper
彻底解耦。

## Guidance

1. **改任何 "输出路径" 相关的东西前, 先 grep 谁在等这些 marker**:
   - `rg "log::info|log::error|log::warn" src/` 看产生侧;
   - `rg "wait_until_output_contains" tests/` 看消费侧;
   - 一次性把所有依赖改掉, 或者用合流 buffer 兼容 (更省事)。
2. **新 daemon 启动行为的 "ready" 日志变更, 同步改 e2e 等待串**; 日志文案是
   公开契约, 不是内部实现细节。
2a. **"ready" 这一行的定义是「订阅已建立, 可以投递」, 不是「router 起来了」。**
   2026-11-27 (RAI-576) 之前 `zenoh router daemon ready` 打在 agent inbox / card
   通配 sub 声明**之前**, 于是按它投递的客户端可能撞进「订阅还没声明」的窗口 ——
   pub 无缓存, 丢了补拉不回来, 表现是 mailbox e2e 在多条互不相关的 PR 上间歇红
   (`agent_mailbox_should_deduplicate_repeated_delivery_by_id` /
   `agent_message_delivered_before_agent_start_should_be_recovered_via_mailbox`)。
   现在 ready 排在两条声明之后, 并有 `agent card|inbox 通配 sub 已声明` 两行供日志自证
   顺序; 守卫用例是 `daemon_ready_should_be_logged_after_mailbox_subscriptions`。
   **再往后挪动启动步骤时, 仍然是这条规矩: 凡是「ready 之后投递就该成功」的机制,
   必须在 ready 之前就绪。**
   **启动形态只有一个**: card / inbox 两个通配 sub 的声明块是无条件的 `{ ... }`
   (没有配置开关, 也不按 profile 分叉), 且声明失败走 `?` 直接让 daemon 启动失败 ——
   所以不存在「不订阅 mailbox 的形态」, ready 永远有对象可等, 不会卡在一个永不满足的条件
   后面。以后若要给 mailbox 加开关, 那个开关必须同时决定 ready 的位置。
2b. **zenoh 的声明是异步传播的, `.wait()` 只代表本地注册** —— 见
   `zenoh-pub-sub-declaration-propagation.md`。所以「声明调用返回」与「对端 router
   能据此路由」不是同一件事; 需要更强保证时要的是**一次往返读数**, 不是把日志再挪一步。
3. **多测试共用的 daemon 启动应走合流 helper**
   (`start_zenoh_daemon_with_combined_output`): 调用方只看到一个合流的
   `Arc<Mutex<String>>`, 未来 init_logger 路径再变, 只改 helper 内部, 24+ 个
   测试都不用动。这是已验证的现成模板, 不要再新造 per-test pipe 组合。
4. 改 log level / log target / log format 的评审要点同上: e2e 的等待串按
   level 过滤时, level 降级会让 sentinel 永远不出现, 表现为测试挂起而非明确
   失败。

## Evidence

- 消费侧规模: `rg -c 'wait_until_output_contains' tests/zenoh_router_client.rs`
  为 28 处, Windows 变体 3 处 (2026-08-22 实测)。
- 合流 helper 双入口: `tests/zenoh_router_client.rs:174` 与
  `tests/zenoh_unixpipe_fast_path/support.rs:236` 各自实现
  `start_zenoh_daemon_with_combined_output` (两测试模块独立, 不共享 util)。
- 2026-06-19 stdout->stderr 事故的完整过程保留在 `EXPERIENCE.md`
  [2026-06-20 09:30:00] 条目 (含 4 个 e2e 连带修改与 sh wrapper 退役史)。

## Why This Matters

- 这类契约断裂的表现是 "e2e 大面积挂起/超时", 与功能无关却极其耗时; 且只在
  改动者没跑全量 e2e 时漏进 main。
- sentinel 依赖是双向的: 改日志的人不知道测试在等, 写测试的人不知道日志会
  改。唯一低成本的防护是把 "grep 两侧" 固化成改动前动作。

## When to Apply

- 修改 `init_logger` / fern 配置 / 日志输出目标 (stdout/stderr/file) 时。
- 修改 daemon 启动路径上任何 "ready" 类日志的文案、level 或顺序时。
- 新写需要等待 daemon 就绪的 e2e 时 (优先复用合流 helper)。

## When Not to Apply

- 纯 CLI 子命令的一次性输出 (无 e2e 消费) 不在此契约范围。
- hidden-file 日志目标 (`RDOG_HIDDEN_LOG`) 不被 e2e pipe, 变更风险低。

## Related

- `tests/zenoh_router_client.rs` / `tests/zenoh_unixpipe_fast_path/support.rs`
  (合流 helper 模板)
