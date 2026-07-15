//! 行级控制协议的 cancel 机制。
//!
//! 设计目标 (ADR-0005 ticket 03):
//! - 任何 in-flight 命令都可以被 `@cancel#seq#M:{target_seq:N}` 取消。
//! - 长操作 (`@wait` 的 sleep、未来的 mouse-held 等) 每 50ms 醒一次
//!   检查 cancellation token;被取消时立刻返回。
//! - 取消后被取消命令的 response 携带 `error_code:"cancelled"` 与
//!   `evidence.cancelled_at_step`, 客户端据此判断是否需要重试。
//!
//! 取消语义细节:
//! - `target_seq` 命中 in-flight 命令: 该命令的 token 被标记 cancelled,
//!   等长操作下次 check 时返回 cancelled response。
//! - `target_seq` 不存在 (已完成 / 从未存在): 返回 `unknown_target_seq`,
//!   不算错误 (cancel 命令本身 OK,只是没找到目标)。
//! - `target_seq` 是 cancel 命令自己的 seq (自杀): 同样按"不存在的 seq"
//!   处理,因为 cancel 命令本身不会 register token。
//!
//! 跟 ADR-0004 的 E2 envelope 对齐: cancelled 是 error_code 的一种,
//!   retry strategy 是 `manual_only` (cancel 后用户应主动决定下一步)。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 一个可被外部 `signal` 标记的可取消信号。
///
/// 内部 `Arc<AtomicBool>` 让 executor 持有 token, `CancelRegistry`
/// 也持有同一份,signal 时两边都生效。
#[derive(Clone, Default)]
pub(crate) struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 检查 token 是否已被 signal。
    ///
    /// 长操作 (sleep / mouse-held) 在每个 await chunk 之前调一次。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 把 token 标记为 cancelled。
    ///
    /// 幂等 — 多次调用效果跟单次一样。
    pub(crate) fn signal(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

/// in-flight 命令的注册表: `seq -> CancellationToken`。
///
/// dispatcher 在 `executor.execute()` 前后 register / unregister。
/// `@cancel#seq` 命令通过 `signal(seq)` 触发对应 token。
///
/// Mutex (不是 RwLock) 因为 register/unregister/signal 都很短,
/// 简单胜过并发吞吐。
#[derive(Default)]
pub(crate) struct CancelRegistry {
    inner: Mutex<HashMap<u64, CancellationToken>>,
}

impl CancelRegistry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 注册一个新 in-flight 命令,返回它的 cancellation token。
    ///
    /// 如果同 seq 已经在表里 (重复 seq 极少见但可能), 复用现有 token —
    /// 这种情况 cancel 会同时影响两个语义上同 seq 的调用, 调用方应避免。
    pub(crate) fn register(&self, seq: u64) -> CancellationToken {
        let mut guard = self.inner.lock().expect("cancel registry mutex poisoned");
        let entry = guard.entry(seq).or_insert_with(CancellationToken::new);
        entry.clone()
    }

    /// 注销一个已完成的命令。
    pub(crate) fn unregister(&self, seq: u64) {
        let mut guard = self.inner.lock().expect("cancel registry mutex poisoned");
        guard.remove(&seq);
    }

    /// Signal 一个 in-flight 命令取消。
    ///
    /// 返回 `true` 当且仅当 seq 在 registry 里 (说明真有目标在跑);
    /// `false` 时 cancel 命令本身仍 OK,只是登记无 op — 返回 unknown_target_seq。
    pub(crate) fn signal(&self, seq: u64) -> bool {
        let guard = self.inner.lock().expect("cancel registry mutex poisoned");
        if let Some(token) = guard.get(&seq) {
            token.signal();
            true
        } else {
            false
        }
    }
}

/// 在 sleep 期间每 50ms 检查一次 cancellation token。
///
/// 返回 `Ok(actual_ms)` 当 sleep 完整跑完,`Err(())` 当被取消。
///
/// 50ms 是 ADR-0005 §Cancellation 给出的上限;实际等待时间最长比请求
/// duration_ms 多 50ms。
pub(crate) fn sleep_cancellable(duration_ms: u64, cancel: &CancellationToken) -> Result<u64, ()> {
    use std::time::Instant;
    const CHECK_INTERVAL_MS: u64 = 50;

    let start = Instant::now();
    let mut remaining = duration_ms;

    while remaining > 0 {
        if cancel.is_cancelled() {
            return Err(());
        }
        let chunk = remaining.min(CHECK_INTERVAL_MS);
        std::thread::sleep(std::time::Duration::from_millis(chunk));
        remaining -= chunk;
    }
    Ok(start.elapsed().as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_registry_register_unregister_signal() {
        let registry = CancelRegistry::new();
        let token = registry.register(42);
        assert!(!token.is_cancelled());
        assert!(registry.signal(42));
        assert!(token.is_cancelled());
        registry.unregister(42);
        assert!(!registry.signal(42)); // 不存在
    }

    #[test]
    fn cancel_registry_signal_unknown_seq_returns_false() {
        let registry = CancelRegistry::new();
        assert!(!registry.signal(99));
    }

    #[test]
    fn sleep_cancellable_returns_actual_when_not_cancelled() {
        let token = CancellationToken::new();
        let actual = sleep_cancellable(20, &token).unwrap();
        assert!(actual >= 20);
        assert!(actual < 200); // 50ms tolerance per ticket 03 spirit
    }

    #[test]
    fn sleep_cancellable_returns_err_when_cancelled_mid_sleep() {
        let token = CancellationToken::new();
        // 在另一个 thread 50ms 后 signal
        let token_clone = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            token_clone.signal();
        });

        let result = sleep_cancellable(2000, &token);
        assert!(result.is_err(), "expected Err when cancelled mid-sleep");
    }
}
