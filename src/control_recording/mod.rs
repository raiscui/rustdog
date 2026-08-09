//! Recording Session capture backend.
//!
//! Per ticket `#16` (macOS Recorder capture backend) and
//! `specs/rdog-macos-operation-capture-research.md`. Defines the
//! cross-platform `RecorderCapture` trait and platform-specific
//! implementations. macOS uses `CGEventTap` (kCGSessionEventTap +
//! kCGEventTapOptionListenOnly); non-macOS platforms return
//! `RecorderError::Unavailable`.
//!
//! 录制功能按 specs 规划逐步接入主流程, 接入前允许 dead code, 避免
//! 每次编译的 never-used 噪音掩盖真正的问题。
#![allow(dead_code)]

#![allow(missing_docs)]

use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

/// Recorder error surface.
#[derive(Debug)]
pub enum RecorderError {
    /// Capture backend is not implemented on this platform.
    Unavailable,
    /// Required permission (Accessibility / Screen Recording / Input Monitoring) is missing.
    PermissionMissing(&'static str),
    /// Underlying capture backend failed to start or stopped unexpectedly.
    Backend(String),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecorderError::Unavailable => f.write_str("recorder capture unavailable on this platform"),
            RecorderError::PermissionMissing(name) => {
                write!(f, "recorder permission missing: {name}")
            }
            RecorderError::Backend(msg) => write!(f, "recorder backend failed: {msg}"),
        }
    }
}

impl std::error::Error for RecorderError {}

/// Reason a single capture event was dropped before reaching the journal writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Self-injected event filtered out (rdog's own CGEventPost).
    SelfEvent,
    /// Secure Input was active; raw value is not persisted.
    SecureInput,
    /// Bounded queue overflowed.
    QueueOverflow,
    /// Capture backend reported a gap (timeout / disabled / permission revoked).
    Gap,
}

/// Normalized capture event flowing from backend to journal writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    /// Physical keyboard event.
    Key {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Virtual keycode (CGKeyCode on macOS).
        keycode: u16,
        /// Down=true / up=false.
        down: bool,
        /// Raw text captured from the backend; later redacted by journal writer.
        text: Option<String>,
    },
    /// Physical mouse button event.
    MouseButton {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Global mouse x (points).
        x: i32,
        /// Global mouse y (points).
        y: i32,
        /// Button index.
        button: u8,
        /// Down=true / up=false.
        down: bool,
    },
    /// Mouse move (drag / focused interaction).
    MouseMove {
        /// Monotonic ms.
        monotonic_ms: u64,
        x: i32,
        y: i32,
    },
    /// Scroll wheel delta.
    Scroll {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Horizontal delta.
        delta_x: i32,
        /// Vertical delta.
        delta_y: i32,
    },
    /// AX semantic snapshot enrichment (focused element at the timestamp).
    AxSnapshot {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Best-effort identifier (role:title or empty if unknown).
        focused: String,
    },
    /// Workspace focus changed to a different app.
    WorkspaceFocus {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Bundle identifier of the activated app (best effort).
        bundle_id: Option<String>,
    },
    /// Window geometry changed (move/resize/state).
    WindowGeometry {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// App bundle id.
        bundle_id: String,
        /// Window title or role description.
        title: String,
        /// Origin x.
        x: i32,
        /// Origin y.
        y: i32,
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// Required lane status report (capture backend health).
    LaneStatus {
        /// Monotonic ms.
        monotonic_ms: u64,
        /// Lane name (e.g. "capture", "accessibility", "screen_recording").
        lane: String,
        /// Status code (e.g. "ok", "disabled_by_timeout", "permission_revoked").
        status: String,
    },
}

/// Recorder capture backend abstraction.
///
/// The trait is object-safe so the lifecycle owner can hold a
/// `Box<dyn RecorderCapture>` and dispatch on platform. Generic drain
/// is deliberately NOT part of the trait — see [`BoundedQueue::drain_all`]
/// for the consumer side.
pub trait RecorderCapture: Send {
    /// Start the backend; returns when the capture loop is running.
    fn start(&mut self) -> Result<(), RecorderError>;
    /// Stop the backend and join the worker thread.
    fn stop(&mut self) -> Result<(), RecorderError>;
    /// Borrow the internal queue so the lifecycle drain loop can pop events.
    fn queue(&self) -> &BoundedQueue;
    /// Mutable queue access for the capture worker.
    fn queue_mut(&mut self) -> &mut BoundedQueue;
    /// Wall-clock anchor captured at session start (Unix epoch ms).
    fn wall_clock_anchor_ms(&self) -> u64;
    /// Whether the underlying capture backend is healthy.
    fn is_healthy(&self) -> bool;
}

/// Construct the platform-appropriate recorder backend.
pub fn platform_capture() -> Box<dyn RecorderCapture> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsCapture::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(stub::StubCapture::new())
    }
}

/// Cooperative shutdown signal shared between the capture worker and the
/// lifecycle owner.
#[derive(Debug, Default)]
pub struct ShutdownSignal {
    flag: AtomicBool,
}

impl ShutdownSignal {
    /// New signal in `false` state.
    pub fn new() -> Self {
        Self { flag: AtomicBool::new(false) }
    }
    /// Returns the shared `Arc<Self>` so the worker and owner can both clone.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }
    /// Mark the worker should stop on next iteration.
    pub fn trigger(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    /// Check whether shutdown was triggered.
    pub fn is_triggered(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Bounded queue shared between the capture worker (producer) and the lifecycle
/// drain loop (consumer). When full, producers call `record_drop` and the
/// event is discarded.
#[derive(Debug)]
pub struct BoundedQueue {
    inner: Mutex<Vec<CaptureEvent>>,
    capacity: usize,
}

impl BoundedQueue {
    /// Default capacity: 1024 events.
    pub const DEFAULT_CAPACITY: usize = 1024;

    /// New queue with [`DEFAULT_CAPACITY`](Self::DEFAULT_CAPACITY).
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// New queue with explicit capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    /// Push an event; if the queue is full, discard and return `false`.
    pub fn push(&self, event: CaptureEvent) -> bool {
        let mut guard = self.inner.lock().expect("recorder queue poisoned");
        if guard.len() >= self.capacity {
            return false;
        }
        guard.push(event);
        true
    }

    /// Drain all queued events into the supplied closure.
    pub fn drain_all<F: FnMut(CaptureEvent)>(&self, mut f: F) -> usize {
        let mut guard = self.inner.lock().expect("recorder queue poisoned");
        let n = guard.len();
        for ev in guard.drain(..) {
            f(ev);
        }
        n
    }
}

impl Default for BoundedQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Self-event marker used by rdog's own input injection paths so the
/// recorder can drop events we triggered rather than the user.
pub const SELF_EVENT_MARKER: u64 = 0x7264_6f67_6461_7400; // "rdogdat\0"

/// Filter helper: drop events tagged with [`SELF_EVENT_MARKER`] in `user_data`.
///
/// Returns `Some(event)` if the event should be kept, `None` if it is a
/// self-injected event.
pub fn filter_self_event(user_data: u64, event: CaptureEvent) -> Option<CaptureEvent> {
    if user_data == SELF_EVENT_MARKER {
        None
    } else {
        Some(event)
    }
}

pub mod bundle;
pub mod cli;
pub mod humantime;
pub mod control_handler;
pub mod protocol;
pub mod delivery;
pub mod journal;
pub mod session;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod bundle_tests;

#[cfg(test)]
mod humantime_tests;

#[cfg(test)]
mod cli_tests;

#[cfg(test)]
mod protocol_tests;

#[cfg(test)]
mod delivery_tests;

#[cfg(test)]
mod control_handler_tests;

#[cfg(test)]
mod journal_tests;
