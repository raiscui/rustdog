---
title: 同进程并行测试共享全局状态必须用唯一共享锁, 锁放模块顶层
date: 2026-08-28
last_updated: 2026-08-28
module: tests
component: test isolation (env / tracing subscriber / observation store)
problem_type: best_practice
severity: medium
status: active
tags:
  - testing
  - parallel-isolation
  - global-state
  - tracing
  - env-vars
  - rust
verified_by:
  - "src/zenoh_runtime/test_support.rs env_test_guard + unique_test_dir: 进程级 env/临时目录唯一锁, cargo test 全量通过 (2026-07-18 实施)"
  - "src/screenshot/tests.rs TIMEOUT_TRACE_TEST_LOCK: tracing 全局 subscriber 串行化后 cargo test screenshot::tests:: 30 passed, 0 failed (2026-08-18)"
  - "反例边界: observation 64-entry singleton 在同进程 cargo test 并行下互相驱逐, exact 单跑通过 / full 失败; nextest 进程隔离规避, 专项治理在 LATER_PLANS (2026-08-20)"
related_solutions:
  - docs/solutions/test-failures/tty-term-dumb-environment-deterministic-failure.md
---

# 同进程并行测试共享全局状态必须用唯一共享锁, 锁放模块顶层

## Context

Rust 默认测试 harness 在同一进程内多线程并行跑测试。任何进程级全局状态 --
环境变量、全局 tracing subscriber、进程内 singleton store -- 都会被并行测试
隐式共享。本仓库三次独立踩中同一模式 (env / tracing / singleton store),
修复形态收敛为同一条纪律。

适用版本: cargo test 默认 harness (lib/bin 内联 `#[cfg(test)]` 测试);
`cargo nextest` 按测试进程隔离, 天然规避但掩盖问题, 不应作为不修的理由。

## Guidance

- 测试要触碰进程级全局状态时, 在**共同祖先模块顶层**声明唯一的
  `static LOCK: OnceLock<Mutex<()>>`, 所有入口 (helper 函数 + 直接访问
  全局状态的测试) 各取一次锁。
- 锁必须全局唯一。各测试各自 `static Mutex` 是各锁各的, 等于没锁。
- 锁的取用点要覆盖**全部**访问路径: helper 封装了大部分, 但绕过 helper
  直接 `with_default` / 直接写 env 的测试也要取锁。
- 不通过扩大容量/改默认值掩盖共享问题 (见反例边界)。

## Evidence

三例同模式, 每例有独立验证:

1. **进程级环境变量** (`src/zenoh_runtime/test_support.rs:9`):
   `env_test_guard()` 用 `OnceLock<Mutex<()>>` 串行化 TMPDIR/HOME 类
   进程级 env 写入, 配合 `unique_test_dir` 提供唯一目录;
   被 `src/zenoh_runtime/unixpipe/tests.rs`、`src/zenoh_runtime/local_default/tests.rs`、
   `src/zenoh_runtime/process_lease.rs` 共用。

2. **全局 tracing subscriber** (`src/screenshot/tests.rs:21`):
   `tracing::subscriber::with_default` 切换的是进程全局 subscriber,
   并行 trace 测试互相覆盖, 断言读到别的测试的 buffer。
   `TIMEOUT_TRACE_TEST_LOCK` 模块顶层唯一锁, `capture_trace` helper 与
   直接使用 `with_default` 的测试入口各取锁; 修复后
   `cargo test screenshot::tests::` 30 passed, 0 failed。

3. **反例 (已知未修边界)**: 64-entry observation singleton store。
   `cargo test -j 2 --bin rdog` 同进程并行下, 长 direct-ref 测试的
   observation 被其他并行测试驱逐 (exact 单跑通过, full 复现失败)。
   处置: 项目标准 nextest 进程隔离规避, 专项 test seam 治理记于
   LATER_PLANS; 明确不通过提高 `DEFAULT_MAX_OBSERVATIONS` 掩盖,
   容量语义由显式 retention 测试验证。

`rg -n 'TIMEOUT_TRACE_TEST_LOCK' src/screenshot/tests.rs` 与
`rg -n -A 6 'fn env_test_guard' src/zenoh_runtime/test_support.rs` 可复核实现。

## Why This Matters

不隔离时的失败形态是**串台**: 断言读到别的测试的事件/env/store 内容,
失败信息完全指向错误的方向 (看起来像业务逻辑错误, 实际是测试互踩)。
这类问题在单跑时永远不复现, 只在全量并行时出现, 极易被误判为 flake
或业务回归, 排查成本远高于顶层加一把锁。

## When to Apply

- 测试写进程级 env 变量 (std::env::set_var) 或依赖其唯一取值;
- 测试用 `tracing::subscriber::with_default` / `set_global_default`
  切换全局 subscriber;
- 测试读写进程内 lazy static / OnceLock singleton store;
- 同文件或同模块多个测试触碰同一份全局状态。

## When Not to Apply

- 测试只用局部状态 (普通变量、临时文件、per-test spawn 的子进程):
  子进程有自己的环境副本, 不需要锁;
- 跨进程并发问题: 锁管不了, 走文件锁 (参考 recording E2E 的
  host-global capture 测试文件锁) 或进程隔离;
- 已经全部走 cargo-nextest 的仓库仍建议修: nextest 掩盖不消除,
  `cargo test` 入口仍在。

## Examples

```rust
// 模块顶层: 唯一锁, OnceLock 免 static init 顺序问题
static TIMEOUT_TRACE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn capture_trace(...) -> ... {
    let _guard = TIMEOUT_TRACE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // ...触碰全局 subscriber...
}

// 绕过 helper 直接 with_default 的测试入口也要取同一把锁
```

## Related

- [tty-term-dumb-environment-deterministic-failure](../test-failures/tty-term-dumb-environment-deterministic-failure.md):
  同属测试隔离主题, 但机制不同 -- 那条是测试继承调用方环境 (TERM),
  本条是测试之间共享进程内全局状态。
- LATER_PLANS "observation singleton 单测并行隔离" 待办 (反例边界的专项治理)
- EXPERIENCE.md `[2026-07-18 17:00:00]` / `[2026-08-18 21:00:00]` 原始记录
