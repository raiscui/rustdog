//! 测试派生子进程的 panic 安全收尾 (RAI-423)。
//!
//! 夹具里 `spawn` 出来的 `rdog daemon` 原先只在用例正常走到末尾时被 `stop_child` 杀掉;
//! 一旦断言 panic(unwind),那一行永远执行不到,子进程被 init 收养(PPID=1)并继续占着端口、
//! 文件和 CPU。连续跑批次时会越积越多,把后续对时序敏感的用例拖成假红 ——
//! 2026-11-26 实测一轮下来积了 27 个孤儿 daemon。
//!
//! [`ChildGuard`] 把子进程的所有权收进来,在 `Drop` 里 kill + wait。正常路径 `stop()`
//! 提前收尾,panic 路径靠 unwinding 触发 `Drop`,两条路都不会漏。
//!
//! 引入方式:
//!
//! ```ignore
//! #[path = "support/child_guard.rs"]
//! mod child_guard;
//!
//! use child_guard::{ChildGuard, ChildHandle};
//! ```
//!
//! 夹具助手写成 `fn stop_child(child: &mut impl ChildHandle)`,那么 `&mut Child` 与
//! `&mut ChildGuard` 都能直接传进去,调用点不用改。
#![allow(dead_code)]

use std::ops::{Deref, DerefMut};
use std::process::Child;

/// 既能接受裸 [`Child`] 也能接受 [`ChildGuard`] 的统一入口。
pub trait ChildHandle {
    fn child_mut(&mut self) -> &mut Child;
}

impl ChildHandle for Child {
    fn child_mut(&mut self) -> &mut Child {
        self
    }
}

/// 持有子进程所有权,`Drop` 时确保它被 kill + wait。
pub struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    /// 交出子进程所有权(需要 `wait_with_output` 这类消费 `self` 的调用时用)。
    /// 交出后本 guard 不再负责它,调用方必须自己收尾。
    pub fn take(&mut self) -> Option<Child> {
        self.child.take()
    }

    /// 立刻收尾;之后 `Drop` 不会再碰它。
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ChildHandle for ChildGuard {
    fn child_mut(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("child guard should still own its child (已被 stop() 收尾)")
    }
}

impl Deref for ChildGuard {
    type Target = Child;

    fn deref(&self) -> &Child {
        self.child
            .as_ref()
            .expect("child guard should still own its child (已被 stop() 收尾)")
    }
}

impl DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Child {
        self.child_mut()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}
