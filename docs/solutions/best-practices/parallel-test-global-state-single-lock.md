---
title: 同进程并行测试共享全局状态必须用唯一共享锁, 锁放模块顶层
date: 2026-08-28
last_updated: 2026-11-27
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
  - "src/test_support.rs cwd_test_guard: figment::Jail 改进程 cwd 的用例与 spawn 子进程的用例共享一把锁; cargo test --bin rdog -- --test-threads=4 连跑 10 轮 1107 passed (2026-11-26, 修前 6 轮红 3 次)"
  - "反例边界: observation 64-entry singleton 在同进程 cargo test 并行下互相驱逐, exact 单跑通过 / full 失败; nextest 进程隔离规避, 专项治理在 LATER_PLANS (2026-08-20)"
  - "src/control_file_transfer.rs orphaned_partial_is_reclaimed_by_the_resume_window: 断言从全局聚合 `reaped` 改为自己的键与文件, sweep 时钟改取过去; 复现用例在旧实现下红在 `left: 2`; cargo test --bin rdog 全量并行 20 轮 0 红 (2026-11-27, RAI-594)"
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
- 锁只保证**串行**, 不保证**隔离**: 断言必须盯本用例自己的键与文件,
  **不把函数的全局聚合返回值** (`reaped` / `count` / `len`) 当断言对象。
- 用例要造假时钟时, 用**小数值假值** (真实 epoch ms 远大于它, 别人按真实时刻登记的条目对它
  恒为 0 空闲), 不要用「真实时钟 + 偏移」或未来时刻 —— 偏移够大或时刻在未来, 就会把并行用例
  按真实时刻登记的状态判成过期, 连键带文件一起删掉 (见 Evidence 第 5 条)。
  这只是**单向**保护: 别人用真实时钟 sweep 一样会立刻回收你的小假戳, 所以那把共享锁仍要拿。
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

3. **进程级 cwd (`src/test_support.rs`)**:
   `figment::Jail` 把进程 cwd 切到自己的临时目录, 退出后连目录一起删掉;
   `script_success_should_keep_legacy_bare_stdout_response_shape` 这类 shell-lane
   用例并行 spawn `/bin/sh` 时子进程继承到即将被删的 cwd, `getcwd` 失败:
   `sh: 0: getcwd() failed: No such file or directory`, 于是本该"成功无 stderr"
   的脚本变成 `shell_stderr` 结构化失败。Jail 自带的锁是私有 static
   (`figment-0.10.19/src/jail.rs` 的 `LOCK`), 只串行化 Jail 之间, 外部拿不到,
   所以必须另立一把共享锁: 切/删 cwd 的一方 (36 个 Jail 用例) 与 spawn 子进程
   并断言 stderr/stdout 干净的一方 (`control_actions` 的 shell 用例) 都取
   `cwd_test_guard()`。修前 `cargo test --bin rdog -- --test-threads=4` 6 轮红 3 次,
   修后 10 轮全绿 (1107 passed)。

4. **反例 (已知未修边界)**: 64-entry observation singleton store。
   `cargo test -j 2 --bin rdog` 同进程并行下, 长 direct-ref 测试的
   observation 被其他并行测试驱逐 (exact 单跑通过, full 复现失败)。
   处置: 项目标准 nextest 进程隔离规避, 专项 test seam 治理记于
   LATER_PLANS; 明确不通过提高 `DEFAULT_MAX_OBSERVATIONS` 掩盖,
   容量语义由显式 retention 测试验证。

5. **全局聚合返回值 + 越过真实时刻的假时钟** (`src/control_file_transfer.rs`,
   `orphaned_partial_is_reclaimed_by_the_resume_window`, RAI-594):
   `reap_idle_transfers` 一轮扫遍会话表与孤儿表, 返回的 `reaped` 是
   "本轮**全局**回收了几条", 用例却把它当断言对象 (`assert_eq!(reaped, 0)`);
   并行用例按真实时刻登记的条目被一起数进去 —— 一条外来上传就够 (会话 +1,
   同轮转出的孤儿再 +1), macOS CI 上收到的正是 2。
   同一段代码还有方向相反的另一半: 用例把锚点设在 `now_ms() + 1_000_000`,
   于是**这一轮 sweep 反过来把别人按真实时刻登记的会话与孤儿全判成过期**,
   既污染计数又真删别人的状态 (快照 + 清空 + 还原只护住了孤儿表, 护不住会话表,
   而且快照与插入是两次独立加锁, 中间能塞进别人的条目)。
   锁在这条上不够 —— 它只串行化, 挡不住"返回值是全局聚合"。
   修法两条同时成立才有效: 断言只盯**本用例自己的键与文件**;
   sweep 改用**小数值假时钟** (按真实 epoch ms 登记的条目对它的 `now - last_activity`
   恒为 0, 这一轮扫不到别人; 而"真实时钟 + 偏移"只要偏移够大就会扫到别人)。
   这是**单向**保护: 真实时钟的 sweep 一样会立刻回收小假戳, 反方向仍靠共享锁。
   另把窗口内那个采样点从空闲阈值上挪开 (改取"窗口 - 1", 年龄 9_999 远大于 1_000) ——
   卡在阈值上时, 把孤儿误按 `idle_timeout_ms` 回收的实现也能混过去。
   这条采样锁的是**阈值替换**, 不锁窗口边界的等号: 两侧采样点各距窗口边界 1 ms,
   窗口判据从 `>=` 写成 `>` 仍然全过。
   负对照: 让回收删文件但不删键 ⇒ 红在"孤儿条目必须从表里消失";
   让回收无视窗口 ⇒ 红在"窗口内必须留着"。

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
