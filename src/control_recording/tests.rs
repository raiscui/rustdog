//! Unit tests for the Recorder capture backend framework.
//!
//! These tests exercise the trait surface, self-event filter, bounded queue,
//! and shutdown signalling on every platform. macOS-only permission gating
//! is exercised through the trait contract (start / stop / queue) without
//! requiring real TCC entitlements.

use super::*;

#[test]
fn stub_capture_lifecycle_records_wall_clock_anchor() {
    let mut capture = platform_capture();
    assert!(
        !capture.is_healthy(),
        "capture should not be healthy before start"
    );
    assert_eq!(capture.wall_clock_anchor_ms(), 0);

    capture.start().expect("start");
    assert!(
        capture.is_healthy(),
        "capture should be healthy after start"
    );
    let anchor = capture.wall_clock_anchor_ms();
    assert!(anchor > 0, "anchor must be set after start");

    let mut drained = 0;
    let _ = capture.queue().drain_all(|_event| drained += 1);
    assert_eq!(drained, 0, "stub queue has no events");

    capture.stop().expect("stop");
    assert!(!capture.is_healthy());
}

#[test]
fn stub_cannot_double_start_or_double_stop() {
    let mut capture = platform_capture();
    capture.start().expect("start");
    let err = capture.start().expect_err("double start must fail");
    assert!(matches!(err, RecorderError::Backend(_)));

    capture.stop().expect("stop");
    let err = capture.stop().expect_err("double stop must fail");
    assert!(matches!(err, RecorderError::Backend(_)));
}

#[test]
fn self_event_marker_drops_injected_events() {
    let event = CaptureEvent::Key {
        monotonic_ms: 1,
        keycode: 0x00,
        down: true,
        text: None,
    };
    assert!(filter_self_event(SELF_EVENT_MARKER, event.clone()).is_none());
    assert!(filter_self_event(0, event).is_some());
}

#[test]
fn bounded_queue_caps_at_capacity_and_loses_overflow() {
    let queue = BoundedQueue::with_capacity(2);
    assert!(queue.push(CaptureEvent::LaneStatus {
        monotonic_ms: 1,
        lane: "capture".into(),
        status: "ok".into(),
    }));
    assert!(queue.push(CaptureEvent::LaneStatus {
        monotonic_ms: 2,
        lane: "capture".into(),
        status: "ok".into(),
    }));
    assert!(
        !queue.push(CaptureEvent::LaneStatus {
            monotonic_ms: 3,
            lane: "capture".into(),
            status: "ok".into(),
        }),
        "third push must drop on full queue"
    );

    let mut drained = Vec::new();
    let n = queue.drain_all(|ev| drained.push(ev));
    assert_eq!(n, 2);
    assert_eq!(drained.len(), 2);
}

#[test]
fn shutdown_signal_propagates_to_is_triggered() {
    let s = ShutdownSignal::shared();
    assert!(!s.is_triggered());
    s.trigger();
    assert!(s.is_triggered());
}

#[test]
fn platform_capture_factory_returns_dyn_compatible_trait_object() {
    // Compile-time guarantee: the factory must satisfy the trait and be
    // usable as Box<dyn RecorderCapture>.
    let capture: Box<dyn RecorderCapture> = platform_capture();
    let _ = capture.wall_clock_anchor_ms();
}
