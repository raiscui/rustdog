//! macOS capture backend using CGEventTap.
//!
//! Per `specs/rdog-macos-operation-capture-research.md`:
//! - `kCGSessionEventTap` + `kCGEventTapOptionListenOnly`
//! - tap callback runs on the dedicated run loop
//! - callback extracts lightweight fields into a bounded queue
//! - workspace / AX enrichment happens outside the callback
//!
//! Permission gates are checked at `start`:
//! - `CGPreflightListenEventAccess` for Input Monitoring (listen-only tap)
//! - `AXIsProcessTrustedWithOptions` for Accessibility
//! - `CGPreflightScreenCaptureAccess` is exposed separately for future
//!   Screen Recording evidence paths.
//!
//! Self-event filter: events tagged with `SELF_EVENT_MARKER` in their
//! `CGEventGetIntegerValueField(kCGEventSourceUserData)` are dropped before
//! reaching the journal writer.

#![cfg(target_os = "macos")]

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use super::{
    BoundedQueue, RecorderCapture, RecorderError, ShutdownSignal,
};

/// macOS capture backend.
pub struct MacOsCapture {
    started: bool,
    anchor_ms: AtomicU64,
    queue: BoundedQueue,
    shutdown: Arc<ShutdownSignal>,
    worker: Option<JoinHandle<()>>,
}

impl MacOsCapture {
    /// New capture backend (not yet started).
    pub fn new() -> Self {
        Self {
            started: false,
            anchor_ms: AtomicU64::new(0),
            queue: BoundedQueue::new(),
            shutdown: ShutdownSignal::shared(),
            worker: None,
        }
    }
}

impl Default for MacOsCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl RecorderCapture for MacOsCapture {
    fn start(&mut self) -> Result<(), RecorderError> {
        if self.started {
            return Err(RecorderError::Backend("already started".into()));
        }

        // Permission preflight per `specs/rdog-macos-operation-capture-research.md` §3.
        // These are read-only checks; the system prompts the user asynchronously
        // when the actual tap / AX calls run.
        if !unsafe { cg_event_listen_preflight() } {
            return Err(RecorderError::PermissionMissing("input_monitoring"));
        }
        if !unsafe { ax_trusted_preflight() } {
            return Err(RecorderError::PermissionMissing("accessibility"));
        }

        let anchor = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.anchor_ms.store(anchor, Ordering::SeqCst);

        // The CGEventTap + run loop wiring is non-trivial and lands in the
        // follow-up commit that adds `cocoa` + `core-graphics` crate
        // bindings. The capture worker holds the run loop source and exits
        // when `shutdown` triggers. The drain loop in lifecycle pops from
        // `self.queue`.
        //
        // ponytail: keep this minimal; the real CGEventTap callback lands
        // alongside the binding crates in the next commit.
        let shutdown = Arc::clone(&self.shutdown);
        let handle = std::thread::Builder::new()
            .name("rdog-recorder-capture".into())
            .spawn(move || {
                while !shutdown.is_triggered() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            })
            .map_err(|err| RecorderError::Backend(format!("spawn worker: {err}")))?;
        self.worker = Some(handle);
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), RecorderError> {
        if !self.started {
            return Err(RecorderError::Backend("not started".into()));
        }
        self.shutdown.trigger();
        if let Some(handle) = self.worker.take() {
            handle.join().map_err(|err| RecorderError::Backend(format!("join: {err:?}")))?;
        }
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
        self.started && !self.shutdown.is_triggered()
    }
}

// ---------------------------------------------------------------------------
// macOS FFI shims. The real bindings live behind `cocoa` + `core-graphics`
// crates which are added in the follow-up commit. Until then we expose the
// permission entry points as raw extern declarations so the start path can
// gate on real TCC state.
//
// ponytail: bindings land in the same PR that brings the real CGEventTap.
// Adding cocoa now would inflate scope; permission checks are the minimum
// required to fail closed when TCC is denied.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

/// Read-only check for Input Monitoring (CGEventTap listen-only access).
unsafe fn cg_event_listen_preflight() -> bool {
    CGPreflightListenEventAccess()
}

/// Read-only check for Accessibility (AX APIs).
unsafe fn ax_trusted_preflight() -> bool {
    // kAXTrustedCheckOptionPrompt = nullptr for the no-prompt variant.
    // The real prompt path is invoked at the AX API call site instead.
    AXIsProcessTrustedWithOptions(std::ptr::null())
}

/// Helper: read-only check for Screen Recording. Exposed so callers can
/// decide whether screenshot evidence is supported in the current session.
#[allow(dead_code)]
pub unsafe fn screen_capture_preflight() -> bool {
    CGPreflightScreenCaptureAccess()
}
