//! 控制请求的入站等待与执行状态. 不读取命令正文, 不把出站帧队列当成工作队列.

use std::{
    collections::HashMap,
    ops::Deref,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use zenoh::handlers::{Callback, DefaultHandler, FifoChannelHandler, IntoHandler};

#[derive(Default)]
struct Requests {
    queued: HashMap<u64, QueuedRequest>,
    active: HashMap<u64, Instant>,
}

struct QueuedRequest {
    channel_id: u64,
    enqueued_at: Instant,
}

static REQUESTS: OnceLock<Mutex<Requests>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

fn requests() -> &'static Mutex<Requests> {
    REQUESTS.get_or_init(|| Mutex::new(Requests::default()))
}

/// 生命周期守卫: 未执行的入站消息被丢弃时也会撤销等待计数.
pub(crate) struct RequestGuard {
    id: u64,
    kind: RequestKind,
}

#[derive(Clone, Copy)]
enum RequestKind {
    Queued,
    Active,
}

impl RequestGuard {
    fn queued(channel_id: u64, alive: &AtomicBool) -> Option<Self> {
        let mut state = requests()
            .lock()
            .expect("control diagnostics lock poisoned");
        if !alive.load(Ordering::Acquire) {
            return None;
        }

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        state.queued.insert(
            id,
            QueuedRequest {
                channel_id,
                enqueued_at: Instant::now(),
            },
        );
        Some(Self {
            id,
            kind: RequestKind::Queued,
        })
    }

    fn active() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        requests()
            .lock()
            .expect("control diagnostics lock poisoned")
            .active
            .insert(id, Instant::now());
        Self {
            id,
            kind: RequestKind::Active,
        }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let mut state = requests()
            .lock()
            .expect("control diagnostics lock poisoned");
        match self.kind {
            RequestKind::Queued => {
                state.queued.remove(&self.id);
            }
            RequestKind::Active => {
                state.active.remove(&self.id);
            }
        }
    }
}

pub(crate) fn executing() -> RequestGuard {
    RequestGuard::active()
}

pub(crate) struct Queued<T> {
    message: T,
    waiting: Option<RequestGuard>,
}

impl<T> Queued<T> {
    /// 出队即停止计算等待; 实际执行由统一控制入口单独记账.
    pub(crate) fn dequeue(self) -> T {
        let Self { message, waiting } = self;
        drop(waiting);
        message
    }
}

pub(crate) struct ChannelReceiver<T> {
    receiver: FifoChannelHandler<Queued<T>>,
    channel_id: u64,
    alive: Arc<AtomicBool>,
}

impl<T> Deref for ChannelReceiver<T> {
    type Target = FifoChannelHandler<Queued<T>>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T> Drop for ChannelReceiver<T> {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        requests()
            .lock()
            .expect("control diagnostics lock poisoned")
            .queued
            .retain(|_, request| request.channel_id != self.channel_id);
    }
}

/// 保留 Zenoh 默认 FIFO 容量与背压, 仅给实际入站消息附加计时守卫.
pub(crate) fn channel<T: Send + 'static>(
    track: impl Fn(&T) -> bool + Send + Sync + 'static,
) -> (Callback<T>, ChannelReceiver<T>) {
    let (send, receiver) = DefaultHandler::default().into_handler();
    let channel_id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed);
    let alive = Arc::new(AtomicBool::new(true));
    let callback_alive = Arc::clone(&alive);
    let callback = Callback::from(move |message| {
        let waiting = if track(&message) {
            RequestGuard::queued(channel_id, &callback_alive)
        } else {
            None
        };
        send.call(Queued { message, waiting });
    });
    (
        callback,
        ChannelReceiver {
            receiver,
            channel_id,
            alive,
        },
    )
}

pub(crate) fn snapshot() -> serde_json::Value {
    let state = requests()
        .lock()
        .expect("control diagnostics lock poisoned");
    let oldest_queued_ms = state
        .queued
        .values()
        .map(|request| request.enqueued_at.elapsed().as_millis())
        .max()
        .unwrap_or(0)
        .min(u128::from(u64::MAX)) as u64;
    let longest_active_ms = state
        .active
        .values()
        .map(|at| at.elapsed().as_millis())
        .max()
        .unwrap_or(0)
        .min(u128::from(u64::MAX)) as u64;
    serde_json::json!({
        "busy": !state.active.is_empty() || !state.queued.is_empty(),
        "active_requests": state.active.len(),
        "queued_requests": state.queued.len(),
        "oldest_queued_ms": oldest_queued_ms,
        "longest_active_ms": longest_active_ms,
        "queue_scope": "zenoh-ingress",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_message_lifetime_should_follow_receive_and_drop() {
        let _lock = TEST_LOCK.lock().unwrap();
        let before = snapshot()["queued_requests"].as_u64().unwrap();
        let (send, receive) = channel(|message| *message != "pty-input");
        send.call("pty-input");
        assert_eq!(snapshot()["queued_requests"], before);
        assert_eq!(receive.recv().unwrap().dequeue(), "pty-input");
        send.call("private-command");
        assert_eq!(snapshot()["queued_requests"], before + 1);
        assert!(!snapshot().to_string().contains("private-command"));
        assert_eq!(receive.recv().unwrap().dequeue(), "private-command");
        assert_eq!(snapshot()["queued_requests"], before);
        send.call("discard");
        drop(receive);
        assert_eq!(snapshot()["queued_requests"], before);
        send.call("after-close");
        assert_eq!(snapshot()["queued_requests"], before);
    }
}
