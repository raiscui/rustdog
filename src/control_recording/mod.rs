#![allow(dead_code)]
#![allow(missing_docs)]

use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

pub mod model;
#[cfg(any(windows, test))]
pub use model::CapturedInput;
pub use model::{CaptureEvent, CaptureSpec, CaptureStop, CaptureTarget};
#[cfg(windows)]
pub use model::{NativeRect, WindowIdentity};

#[derive(Debug)]
pub enum RecorderError {
    Unavailable,
    PermissionMissing(&'static str),
    Backend(String),
}
impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("recorder capture unavailable on this platform"),
            Self::PermissionMissing(name) => write!(f, "recorder permission missing: {name}"),
            Self::Backend(message) => write!(f, "recorder backend failed: {message}"),
        }
    }
}
impl std::error::Error for RecorderError {}

pub trait RecorderCapture: Send {
    fn start(&mut self, spec: &CaptureSpec) -> Result<(), RecorderError>;
    fn stop(
        &mut self,
        drain: &mut dyn FnMut(&BoundedQueue) -> std::io::Result<()>,
    ) -> Result<CaptureStop, RecorderError>;
    fn parameter_boundary(&mut self) -> Result<(), RecorderError>;
    fn queue(&self) -> &BoundedQueue;
    fn wall_clock_anchor_ms(&self) -> u64;
    fn cleanup_pending(&mut self) -> bool;
    fn is_healthy(&self) -> bool;
}

pub fn platform_capture() -> Box<dyn RecorderCapture> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsCapture::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(stub::StubCapture::new())
    }
}

#[derive(Debug)]
pub struct BoundedQueue {
    inner: Mutex<Vec<CaptureEvent>>,
    spare: Mutex<Vec<CaptureEvent>>,
    capacity: usize,
    overflow: AtomicBool,
}
impl BoundedQueue {
    pub const DEFAULT_CAPACITY: usize = 1024;
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Vec::with_capacity(capacity)),
            spare: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
            overflow: AtomicBool::new(false),
        }
    }
    pub fn push(&self, event: CaptureEvent) -> bool {
        let mut queue = self.inner.lock().expect("recorder queue poisoned");
        if queue.len() >= self.capacity {
            self.overflow.store(true, Ordering::Release);
            return false;
        }
        queue.push(event);
        true
    }
    pub fn overflowed(&self) -> bool {
        self.overflow.load(Ordering::Acquire)
    }
    pub fn drain_all<F: FnMut(CaptureEvent)>(&self, mut f: F) -> usize {
        let mut events = self.spare.lock().expect("recorder consumer poisoned");
        {
            let mut queue = self.inner.lock().expect("recorder queue poisoned");
            std::mem::swap(&mut *queue, &mut *events);
        }
        let count = events.len();
        for event in events.drain(..) {
            f(event);
        }
        count
    }
}
impl Default for BoundedQueue {
    fn default() -> Self {
        Self::new()
    }
}

// 本次 Windows 实测 hook 只收到注入 extra info 的低 32 位, 因此 marker 限于 u32.
pub const SELF_EVENT_MARKER: u32 = 0x6461_7400;
#[cfg(any(windows, test))]
pub(crate) mod desktop;
pub const TEXT_MODE: &str = "parameter_intent_only";
pub mod cli;
pub(crate) mod replay_source;
pub(crate) mod replay;
pub(crate) mod replay_normalize;
#[cfg(test)]
mod cli_tests;
pub mod control_handler;
#[cfg(test)]
mod control_handler_tests;
pub mod delivery;
#[cfg(test)]
mod delivery_tests;
pub mod humantime;
#[cfg(test)]
mod humantime_tests;
pub mod journal;
#[cfg(test)]
mod journal_tests;
pub mod owner;
mod private_files;
pub mod protocol;
#[cfg(test)]
mod protocol_tests;
pub mod session;
#[cfg(not(windows))]
mod stub;
#[cfg(windows)]
pub mod windows;
