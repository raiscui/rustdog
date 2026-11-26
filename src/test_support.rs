//! 跨模块单测共享的进程级夹具。
//!
//! `cargo test` 默认 harness 在同一进程内多线程并行跑用例, 进程级状态
//! (cwd / 环境变量 / 全局 subscriber) 因此被隐式共享。纪律与已踩坑案例见
//! `docs/solutions/best-practices/parallel-test-global-state-single-lock.md`。

use std::sync::{Mutex, MutexGuard};

#[path = "../tests/support/test_ports.rs"]
pub(crate) mod test_ports;

/// 串行化"改进程级 cwd 的用例"与"子进程依赖 cwd 的用例"。
///
/// `figment::Jail` 会把进程 cwd 切到自己的临时目录, 退出后再把目录删掉;
/// 它内部的锁只串行化 Jail 之间 (私有 static, 外部拿不到), 挡不住并行启动
/// `/bin/sh` 的用例 —— 子进程继承到即将被删的 cwd 后 `getcwd` 失败, 输出
/// `shell-init: error retrieving current directory`, 于是本该"成功无 stderr"
/// 的脚本变成 `shell_stderr` 结构化失败。
///
/// 锁必须全局唯一: 各用例各自 `static Mutex` 等于各自为政。
static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

/// 取 cwd 串行锁。两个方向的用例都取: 切/删 cwd 的一方 (Jail),
/// 以及 spawn 子进程且对 stderr/stdout 做严格断言的一方。
pub(crate) fn cwd_test_guard() -> MutexGuard<'static, ()> {
    CWD_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 串行化"真的启动 shell 子进程"的用例。
///
/// 2026-11-26 实测 (RAI-429): windows runner 上 `build_shell_command("powershell.exe", …)`
/// 偶发宿主级失败 —— PowerShell 起来了但托管运行时装载不成功, stderr 为
/// `Internal Windows PowerShell error ... failed with error 8009001d`。被测对象是 builder
/// 的建台标志, 不是 PowerShell 自己能不能起来, 所以两件事一起做: 让"启动 shell 的用例"
/// 互斥 (降低同时涌入的 console-less 子进程数), 再对**启动失败**做有界重试。
///
/// 与 `cwd_test_guard` 分开是刻意的: 那条锁保护的是 cwd 语义, 持锁时间短得多,
/// 混用会把无关用例也拖成串行。
static SHELL_SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// 取 shell 子进程串行锁。
pub(crate) fn shell_spawn_guard() -> MutexGuard<'static, ()> {
    SHELL_SPAWN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
