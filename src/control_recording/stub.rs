//! Non-macOS stub recorder backend.
//!
//! Per ticket `#16` scope: macOS is the first platform. Other platforms
//! return `RecorderError::Unavailable` until a future Linux/Windows backend
//! ticket is opened.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{BoundedQueue, RecorderCapture, RecorderError, ShutdownSignal};

/// Stub capture backend. Starts / stops cleanly; `drain` returns immediately
/// with zero events.
pub struct StubCapture {
    started: bool,
    anchor_ms: AtomicU64,
    queue: BoundedQueue,
    shutdown: ShutdownSignal,
}

impl StubCapture {
    /// New stub.
    pub fn new() -> Self {
        Self {
            started: false,
            anchor_ms: AtomicU64::new(0),
            queue: BoundedQueue::new(),
            shutdown: ShutdownSignal::new(),
        }
    }
}

impl Default for StubCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl RecorderCapture for StubCapture {
    fn start(&mut self) -> Result<(), RecorderError> {
        if self.started {
            return Err(RecorderError::Backend("already started".into()));
        }
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.anchor_ms.store(ms, Ordering::SeqCst);
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), RecorderError> {
        if !self.started {
            return Err(RecorderError::Backend("not started".into()));
        }
        self.shutdown.trigger();
        self.started = false;
        Ok(())
    }

    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }

    fn queue_mut(&mut self) -> &mut BoundedQueue {
        &mut self.queue
    }

    fn wall_clock_anchor_ms(&self) -> u64 {
        self.anchor_ms.load(Ordering::SeqCst)
    }

    fn is_healthy(&self) -> bool {
        self.started
    }
}
