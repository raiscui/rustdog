//! 跨模块单测共享的进程级状态锁。
//!
//! `cargo test` 默认 harness 在同一进程内多线程并行跑用例, 进程级状态
//! (cwd / 环境变量 / 全局 subscriber) 因此被隐式共享。纪律与已踩坑案例见
//! `docs/solutions/best-practices/parallel-test-global-state-single-lock.md`。

use std::sync::{Mutex, MutexGuard};

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
