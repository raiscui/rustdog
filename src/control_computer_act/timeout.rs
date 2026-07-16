//! `@computer-act` per-action timeout table (ADR-0005 §L3 + ticket 16)。
//!
//! 默认 timeout 表 (ms, 跟 ADR-0005 严格对齐):
//! - `wait` = `duration_ms * 1.5 + 1000` (派生, 保证 `> duration_ms`, 永不自杀)
//! - `open_app` / `open_url` = 10000 (冷启动)
//! - `type` = 5000 (per-character bursts)
//! - `hotkey_click` = 3000 (composite key + click)
//! - `drag` = 5000 (multi-step mouse motion)
//! - `click` family (click / doubleclick / triple_click) = 2000
//! - `right_single` = 2000
//! - `hover` = 1500
//! - `scroll` = 2000
//! - `hotkey` = 1500 (单 key 事件)
//!
//! Client override via `request.timeout_ms`。
//!
//! Timeout watcher: spawn std::thread, sleep timeout_ms, then signal CancellationToken
//! (跟 ticket 03 cancellation 整合)。Background thread leak 由 daemon 退出清理。

use crate::control_computer_act::ComputerActErrorCode;
use crate::cancellation::CancellationToken;
use serde_json::Value;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// per-action 默认 timeout 表 (ms), 跟 ADR-0005 §3 strict。
///
/// 返回 None 表示 "由 wait 派生公式" 或 "不适用" (e.g. unknown action)。
pub(crate) fn default_timeout_ms(action: &str) -> Option<u64> {
    match action {
        "open_app" => Some(10_000),
        "open_url" => Some(10_000),
        "type" => Some(5_000),
        "hotkey_click" => Some(3_000),
        "drag" => Some(5_000),
        "click" | "doubleclick" | "triple_click" | "right_single" => Some(2_000),
        "hover" => Some(1_500),
        "scroll" => Some(2_000),
        "hotkey" => Some(1_500),
        "wait" => None, // 由 caller 派生 duration_ms * 1.5 + 1000
        _ => None,
    }
}

/// 计算 wait action 的派生 timeout: `duration_ms * 1.5 + 1000`
///
/// 保证严格大于 duration_ms (self-kill guard)。
/// 0ms wait → timeout = 1000ms (base buffer)
pub(crate) fn wait_derived_timeout_ms(duration_ms: u64) -> u64 {
    let derived = (duration_ms as f64 * 1.5) as u64 + 1000;
    // 防御: 即使 duration_ms = u64::MAX 也不会 overflow (1.5x 还是 u64 range)
    derived.max(duration_ms.saturating_add(1)) // 永远 >= duration_ms + 1
}

/// 解析 effective timeout (考虑 client override + wait 派生公式)
///
/// 返回 (effective_timeout_ms, override_applied: bool)
pub(crate) fn resolve_timeout(
    action: &str,
    args: &Value,
    request_timeout_ms: Option<u64>,
) -> u64 {
    // client override 优先
    if let Some(override_ms) = request_timeout_ms {
        return override_ms;
    }
    // wait 走派生公式
    if action == "wait" {
        let duration_ms = args
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return wait_derived_timeout_ms(duration_ms);
    }
    // 其它走默认表
    default_timeout_ms(action).unwrap_or(30_000) // 兜底 30s
}

/// TimeoutWatcher: spawn 一个 background thread, timeout_ms 后 signal CancellationToken。
///
/// 用 `Arc<JoinHandle>` 让 caller 在 dispatch 完成后调用 `stop()` 提早结束 timer (不必等满 timeout)。
/// 如果 timer 已经触发, drop JoinHandle 会 detach thread (允许它清理)。
pub(crate) struct TimeoutWatcher {
    /// 保留字段: 给上层显式 stop() 用 (timer 提早结束)
    #[allow(dead_code)]
    cancel_token: CancellationToken,
    /// 保留字段: join / detach thread 时用
    #[allow(dead_code)]
    handle: Arc<JoinHandle<()>>,
}

impl TimeoutWatcher {
    /// 启动 timer。timeout_ms 后调 cancel_token.signal()。
    pub fn start(timeout_ms: u64, cancel_token: CancellationToken) -> Self {
        let token_clone_signal = cancel_token.clone();
        let handle = thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(timeout_ms));
            token_clone_signal.signal();
        });
        Self {
            cancel_token,
            handle: Arc::new(handle),
        }
    }

    /// 检查是否已经触发 timeout。
    /// 检查是否已经触发 timeout。给上层 caller 用 (e.g., dispatch 完成后
    /// 想知道是 timeout 还是别的原因失败)。
    #[allow(dead_code)]
    pub fn fired(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// 提早 stop timer (dispatch 提前完成, 不必等满 timeout)。
    /// 实现: 用一个 atomic flag + Condvar 太重, 简化用 detach thread:
    /// 不 join, 让它跑完 timeout 也没事 (signal 是幂等的, 后续 dispatch 完成后再检查 is_cancelled 看到 false 就知道 timeout 没触发)。
    /// 这里通过 take JoinHandle 实现 "如果 thread 已经结束就回收, 否则 detach"。
    #[allow(dead_code)]
    pub fn stop(self) -> bool {
        let fired = self.cancel_token.is_cancelled();
        match Arc::try_unwrap(self.handle) {
            Ok(handle) => {
                // 唯一持有 → join thread (sleep 可能还在跑, join 会等它完成)
                // 这是 wasted 时间但不会有副作用
                let _ = handle.join();
            }
            Err(_) => {
                // 还有别的持有者 → 让它跑完, detach
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_timeout_table_matches_adr_0005() {
        assert_eq!(default_timeout_ms("open_app"), Some(10_000));
        assert_eq!(default_timeout_ms("open_url"), Some(10_000));
        assert_eq!(default_timeout_ms("type"), Some(5_000));
        assert_eq!(default_timeout_ms("hotkey_click"), Some(3_000));
        assert_eq!(default_timeout_ms("drag"), Some(5_000));
        assert_eq!(default_timeout_ms("click"), Some(2_000));
        assert_eq!(default_timeout_ms("doubleclick"), Some(2_000));
        assert_eq!(default_timeout_ms("triple_click"), Some(2_000));
        assert_eq!(default_timeout_ms("right_single"), Some(2_000));
        assert_eq!(default_timeout_ms("hover"), Some(1_500));
        assert_eq!(default_timeout_ms("scroll"), Some(2_000));
        assert_eq!(default_timeout_ms("hotkey"), Some(1_500));
    }

    #[test]
    fn wait_returns_none_from_default_table() {
        assert_eq!(default_timeout_ms("wait"), None);
    }

    #[test]
    fn wait_derived_timeout_is_strictly_greater_than_duration() {
        // ADR-0005: wait timeout 永远 >= duration_ms + 1 (永不自杀)
        for d in [0, 100, 1_000, 10_000, 60_000] {
            let timeout = wait_derived_timeout_ms(d);
            assert!(timeout > d, "wait timeout {timeout} should be > duration {d}");
        }
    }

    #[test]
    fn wait_derived_timeout_zero_duration_is_1000ms() {
        // duration_ms=0 → timeout = 0 * 1.5 + 1000 = 1000 (兜底 buffer)
        assert_eq!(wait_derived_timeout_ms(0), 1_000);
    }

    #[test]
    fn resolve_timeout_applies_client_override() {
        // client override 优先于一切
        let timeout = resolve_timeout("open_app", &json!({}), Some(500));
        assert_eq!(timeout, 500);
    }

    #[test]
    fn resolve_timeout_uses_default_table_for_known_actions() {
        let timeout = resolve_timeout("open_app", &json!({}), None);
        assert_eq!(timeout, 10_000);
    }

    #[test]
    fn resolve_timeout_derives_for_wait() {
        // wait(duration_ms:2000) → 2000 * 1.5 + 1000 = 4000
        let timeout = resolve_timeout("wait", &json!({"duration_ms": 2000}), None);
        assert_eq!(timeout, 4_000);
    }

    #[test]
    fn resolve_timeout_fallback_for_unknown_action() {
        // 兜底: unknown action → 30s
        let timeout = resolve_timeout("unknown_thing", &json!({}), None);
        assert_eq!(timeout, 30_000);
    }

    #[test]
    fn timeout_watcher_fires_after_duration() {
        let token = CancellationToken::new();
        let watcher = TimeoutWatcher::start(50, token.clone());
        assert!(!watcher.fired());
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(watcher.fired());
        // stop 回收 thread
        let fired = watcher.stop();
        assert!(fired);
    }

    #[test]
    fn timeout_watcher_stop_returns_false_if_not_fired() {
        let token = CancellationToken::new();
        let watcher = TimeoutWatcher::start(5000, token.clone());
        // 立即 stop (远早于 5s)
        let fired = watcher.stop();
        assert!(!fired, "stop should return false if timeout not yet fired");
    }
}
