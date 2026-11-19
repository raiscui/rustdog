use serde::Serialize;
use super::{model::{CapturedInput, DisplayGeometry, NativeRect, WindowIdentity}, replay_source::ReplayFact};

#[derive(Serialize)]
pub(crate) struct Segment {
    pub source_seqs: Vec<u64>,
    pub action: Action,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Action {
    Move { window: WindowIdentity, x: i32, y: i32 },
    Click { window: WindowIdentity, x: i32, y: i32, button: u8, hold_ns: u64 },
    Drag { window: WindowIdentity, button: u8, points: Vec<Point>, duration_ns: u64 },
    Wheel { window: WindowIdentity, x: i32, y: i32, delta_x: i32, delta_y: i32 },
    UnknownInput { window: Option<WindowIdentity>, segment: u64 },
    Unresolved { reason: &'static str },
}

#[derive(Serialize)]
pub(crate) struct Point { pub x: i32, pub y: i32, pub offset_ns: u64 }

struct Pending {
    window: WindowIdentity,
    button: u8,
    start_time: u64,
    last_time: u64,
    source_seqs: Vec<u64>,
    points: Vec<Point>,
    window_geometry: Option<(NativeRect, u32)>,
    displays: Option<Vec<DisplayGeometry>>,
}

fn fail_pending(pending: &mut Option<Pending>, output: &mut Vec<Segment>) {
    if let Some(pending) = pending.take() {
        output.push(Segment { source_seqs: pending.source_seqs, action: Action::Unresolved { reason: "unpaired_mouse_down" } });
    }
}

pub(crate) fn normalize(facts: &[ReplayFact]) -> Vec<Segment> {
    let mut output = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut windows: Vec<(WindowIdentity, NativeRect, u32)> = Vec::new();
    let mut displays: Option<Vec<DisplayGeometry>> = None;
    let mut redacting = false;
    for fact in facts {
        let ReplayFact::Capture { journal_seq, captured_monotonic_ns, input, .. } = fact else {
            if let (Some(active), ReplayFact::Mark { journal_seq, .. }) = (&mut pending, fact) {
                active.source_seqs.push(*journal_seq);
            }
            continue;
        };
        match input {
            CapturedInput::MouseDown { window, x, y, button, .. } => {
                fail_pending(&mut pending, &mut output);
                if redacting {
                    output.push(Segment { source_seqs: vec![*journal_seq], action: Action::Unresolved { reason: "mouse_during_redaction" } });
                    continue;
                }
                let window_geometry = windows.iter().find(|(id, _, _)| id == window).map(|(_, rect, dpi)| (*rect, *dpi));
                pending = Some(Pending { window: *window, button: *button, start_time: *captured_monotonic_ns, last_time: *captured_monotonic_ns, source_seqs: vec![*journal_seq], points: vec![Point { x: *x, y: *y, offset_ns: 0 }], window_geometry, displays: displays.clone() });
            }
            CapturedInput::MouseMove { window, x, y, .. } => {
                if let Some(active) = pending.as_mut().filter(|active| active.window == *window && *captured_monotonic_ns >= active.last_time) {
                    active.source_seqs.push(*journal_seq);
                    active.points.push(Point { x: *x, y: *y, offset_ns: *captured_monotonic_ns - active.start_time });
                    active.last_time = *captured_monotonic_ns;
                } else {
                    fail_pending(&mut pending, &mut output);
                    output.push(Segment { source_seqs: vec![*journal_seq], action: Action::Move { window: *window, x: *x, y: *y } });
                }
            }
            CapturedInput::MouseUp { window, x, y, button, .. } => {
                let Some(mut active) = pending.take() else {
                    output.push(Segment { source_seqs: vec![*journal_seq], action: Action::Unresolved { reason: "unpaired_mouse_up" } });
                    continue;
                };
                if active.window != *window || active.button != *button || *captured_monotonic_ns < active.last_time {
                    output.push(Segment { source_seqs: active.source_seqs, action: Action::Unresolved { reason: "mismatched_mouse_pair" } });
                    output.push(Segment { source_seqs: vec![*journal_seq], action: Action::Unresolved { reason: "unpaired_mouse_up" } });
                    continue;
                }
                active.source_seqs.push(*journal_seq);
                let duration_ns = *captured_monotonic_ns - active.start_time;
                let start = &active.points[0];
                let click = active.points.iter().all(|point| point.x == start.x && point.y == start.y) && *x == start.x && *y == start.y;
                let action = if click {
                    Action::Click { window: *window, x: *x, y: *y, button: *button, hold_ns: duration_ns }
                } else {
                    active.points.push(Point { x: *x, y: *y, offset_ns: duration_ns });
                    Action::Drag { window: *window, button: *button, points: active.points, duration_ns }
                };
                output.push(Segment { source_seqs: active.source_seqs, action });
            }
            CapturedInput::Scroll { window, x, y, delta_x, delta_y, .. } => {
                fail_pending(&mut pending, &mut output);
                let action = if *delta_x == 0 && *delta_y == 0 { Action::Unresolved { reason: "empty_wheel" } } else { Action::Wheel { window: *window, x: *x, y: *y, delta_x: *delta_x, delta_y: *delta_y } };
                output.push(Segment { source_seqs: vec![*journal_seq], action });
            }
            CapturedInput::Redaction { window, segment, active: true } => {
                fail_pending(&mut pending, &mut output);
                redacting = true;
                output.push(Segment { source_seqs: vec![*journal_seq], action: Action::UnknownInput { window: *window, segment: *segment } });
            }
            CapturedInput::Redaction { active: false, .. } => {
                if let Some(active) = &mut pending { active.source_seqs.push(*journal_seq); }
                if redacting { fail_pending(&mut pending, &mut output); }
                redacting = false;
            }
            CapturedInput::WindowContext { window, rect, dpi, .. } => {
                if let Some(active) = &mut pending {
                    active.source_seqs.push(*journal_seq);
                    if active.window != *window || active.window_geometry != Some((*rect, *dpi)) {
                        fail_pending(&mut pending, &mut output);
                    }
                }
                windows.retain(|(id, _, _)| id != window);
                windows.push((*window, *rect, *dpi));
            }
            CapturedInput::DisplayContext { displays: current, .. } => {
                if let Some(active) = &mut pending {
                    active.source_seqs.push(*journal_seq);
                    if active.displays.as_ref() != Some(current) {
                        fail_pending(&mut pending, &mut output);
                    }
                }
                displays = Some(current.clone());
            }
            CapturedInput::SemanticCandidate { .. } | CapturedInput::SemanticMissing { .. } => {
                if let Some(active) = &mut pending { active.source_seqs.push(*journal_seq); }
            }
        }
    }
    fail_pending(&mut pending, &mut output);
    output.sort_by_key(|segment| segment.source_seqs[0]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(seq: u64, time: u64, input: CapturedInput) -> ReplayFact {
        ReplayFact::Capture { journal_seq: seq, monotonic_ns: seq * 100, capture_seq: seq,
            captured_monotonic_ns: time, input }
    }

    fn window() -> WindowIdentity {
        WindowIdentity { pid: 7, hwnd: 42, process_start: 11, generation: 1 }
    }

    #[test]
    fn normalizes_drag_and_wheel_without_losing_source_or_timing() {
        let w = window();
        let facts = vec![
            fact(1, 10, CapturedInput::MouseDown { window: w, x: 10, y: 20, button: 1, injected: false }),
            fact(2, 40, CapturedInput::MouseMove { window: w, x: 30, y: 40, injected: false }),
            fact(3, 80, CapturedInput::MouseUp { window: w, x: 50, y: 60, button: 1, injected: false }),
            fact(4, 90, CapturedInput::Scroll { window: w, x: 50, y: 60, delta_x: 0, delta_y: -120, injected: false }),
        ];
        let result = normalize(&facts);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_seqs, [1, 2, 3]);
        match &result[0].action {
            Action::Drag { window, points, duration_ns, button } => {
                assert_eq!((*window, *button, *duration_ns), (w, 1, 70));
                assert_eq!((points[1].x, points[1].y, points[1].offset_ns), (30, 40, 30));
                assert_eq!((points[2].x, points[2].y, points[2].offset_ns), (50, 60, 70));
            }
            _ => panic!("expected drag"),
        }
        assert!(matches!(result[1].action, Action::Wheel { delta_y: -120, .. }));
    }

    #[test]
    fn refuses_mismatched_or_out_of_order_pointer_pairs() {
        let w = window();
        let facts = vec![
            fact(1, 50, CapturedInput::MouseDown { window: w, x: 1, y: 2, button: 1, injected: false }),
            fact(2, 40, CapturedInput::MouseUp { window: w, x: 1, y: 2, button: 1, injected: false }),
        ];
        let result = normalize(&facts);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|segment| matches!(segment.action, Action::Unresolved { .. })));
        assert_eq!(result[0].source_seqs, [1]);
        assert_eq!(result[1].source_seqs, [2]);
    }

    #[test]
    fn preserves_pair_across_same_geometry_context_and_semantic_metadata() {
        let w = window();
        let rect = NativeRect { x: 10, y: 20, width: 300, height: 200 };
        let context = |focused| CapturedInput::WindowContext { window: w, rect, dpi: 96, focused };
        let result = normalize(&[
            fact(1, 1, context(false)),
            fact(2, 2, CapturedInput::MouseDown { window: w, x: 30, y: 40, button: 1, injected: false }),
            fact(3, 3, context(true)),
            fact(4, 4, CapturedInput::SemanticMissing { for_sequence: 2 }),
            fact(5, 5, CapturedInput::MouseUp { window: w, x: 30, y: 40, button: 1, injected: false }),
        ]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_seqs, [2, 3, 4, 5]);
        assert!(matches!(result[0].action, Action::Click { button: 1, hold_ns: 3, .. }));
    }

    #[test]
    fn refuses_pair_when_interleaved_window_geometry_changes() {
        let w = window();
        let rect = NativeRect { x: 10, y: 20, width: 300, height: 200 };
        let mut moved = rect;
        moved.x += 1;
        let result = normalize(&[
            fact(1, 1, CapturedInput::WindowContext { window: w, rect, dpi: 96, focused: true }),
            fact(2, 2, CapturedInput::MouseDown { window: w, x: 30, y: 40, button: 1, injected: false }),
            fact(3, 3, CapturedInput::WindowContext { window: w, rect: moved, dpi: 96, focused: true }),
            fact(4, 4, CapturedInput::MouseUp { window: w, x: 30, y: 40, button: 1, injected: false }),
        ]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_seqs, [2, 3]);
        assert!(result.iter().all(|segment| matches!(segment.action, Action::Unresolved { .. })));
    }
}
