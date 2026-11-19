use std::{io, time::Duration};
use crate::control_flow::{FlowGuardedMouseStep, FlowMouseAction, FlowMouseVerification};

#[cfg(any(windows, test))]
pub(super) fn raw_wheel_events(delta_x: i32, delta_y: i32) -> [(enigo::Axis, i32); 2] {
    [(enigo::Axis::Horizontal, delta_x), (enigo::Axis::Vertical, delta_y)]
}

#[cfg(any(windows, test))]
pub(super) fn precondition(verify: &FlowMouseVerification, before: i64) -> bool {
    match verify {
        FlowMouseVerification::Focus { .. } => before == 0,
        FlowMouseVerification::Selection { start, end, .. } =>
            before != i64::from(u32::from(*start) | (u32::from(*end) << 16)),
        _ => true,
    }
}

#[cfg(any(windows, test))]
pub(super) fn postcondition(verify: &FlowMouseVerification, before: i64, after: i64) -> bool {
    match verify {
        FlowMouseVerification::Position { .. } | FlowMouseVerification::Focus { .. } => after == 1,
        FlowMouseVerification::Scroll { increase, .. } => if *increase { after > before } else { after < before },
        FlowMouseVerification::Selection { start, end, .. } => after == i64::from(u32::from(*start) | (u32::from(*end) << 16)),
    }
}

pub(crate) fn validate_guarded(step: &FlowGuardedMouseStep) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "受守卫鼠标动作无效");
    if step.pid == 0 || step.root_hwnd == 0 || step.observation_id.is_empty() || step.window_ref.is_empty()
        || step.rect.width == 0 || step.rect.height == 0 { return Err(invalid()); }
    validate_action(&step.action, &step.verify)?;
    validate_points_in_rect(&step.action, &step.rect)
}

pub(crate) fn validate_action(action: &FlowMouseAction, verify: &FlowMouseVerification) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "受守卫鼠标动作无效");
    let valid_button = |button: u8| (1..=3).contains(&button);
    match action {
        FlowMouseAction::Move { point } if point.offset_ns == 0 => {}
        FlowMouseAction::Click { point, button, hold_ns } if point.offset_ns == 0 && valid_button(*button) && *hold_ns <= 5_000_000_000 => {}
        FlowMouseAction::Drag { points, button } if valid_button(*button) && (2..=128).contains(&points.len())
            && points[0].offset_ns == 0 && points.last().is_some_and(|p| p.offset_ns <= 5_000_000_000)
            && points.windows(2).all(|pair| pair[0].offset_ns <= pair[1].offset_ns) => {}
        FlowMouseAction::Wheel { point, delta_x, delta_y } if point.offset_ns == 0 && (*delta_x != 0 || *delta_y != 0)
            && i16::try_from(*delta_x).is_ok() && i16::try_from(*delta_y).is_ok() => {}
        _ => return Err(invalid()),
    }
    match (action, verify) {
        (FlowMouseAction::Move { .. }, FlowMouseVerification::Position { control_hwnd }) if *control_hwnd != 0 => {}
        (FlowMouseAction::Click { .. }, FlowMouseVerification::Focus { control_hwnd }) if *control_hwnd != 0 => {}
        (FlowMouseAction::Click { .. } | FlowMouseAction::Drag { .. },
            FlowMouseVerification::Selection { control_hwnd, start, end })
            if *control_hwnd != 0 && start <= end => {}
        (FlowMouseAction::Wheel { delta_x, delta_y, .. },
            FlowMouseVerification::Scroll { control_hwnd, vertical, .. })
            if *control_hwnd != 0 && if *vertical { *delta_y != 0 } else { *delta_x != 0 } => {}
        _ => return Err(invalid()),
    }
    Ok(())
}

pub(crate) fn validate_points_in_rect(action: &FlowMouseAction, rect: &crate::control_flow::FlowWindowRect) -> io::Result<()> {
    let inside = |point: &crate::control_flow::FlowMousePoint| {
        i64::from(point.x) >= i64::from(rect.x) && i64::from(point.y) >= i64::from(rect.y)
            && i64::from(point.x) < i64::from(rect.x) + i64::from(rect.width)
            && i64::from(point.y) < i64::from(rect.y) + i64::from(rect.height)
    };
    let valid = match action {
        FlowMouseAction::Move { point } | FlowMouseAction::Click { point, .. }
        | FlowMouseAction::Wheel { point, .. } => inside(point),
        FlowMouseAction::Drag { points, .. } => points.iter().all(inside),
    };
    if valid { Ok(()) } else { Err(io::Error::new(io::ErrorKind::InvalidInput, "目标点不在当前窗口内")) }
}

pub(crate) fn execute_guarded(step: &FlowGuardedMouseStep, timeout: Duration) -> io::Result<()> {
    validate_guarded(step)?;
    #[cfg(windows)]
    { super::win_backend::execute_guarded(step, timeout) }
    #[cfg(not(windows))]
    { let _ = timeout; Err(io::Error::new(io::ErrorKind::Unsupported, "受守卫鼠标动作仅支持 Windows")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_flow::{FlowMousePoint, FlowWindowRect};

    fn point(x: i32, offset_ns: u64) -> FlowMousePoint {
        FlowMousePoint { x, y: 20, offset_ns }
    }

    fn step(action: FlowMouseAction) -> FlowGuardedMouseStep {
        FlowGuardedMouseStep { pid: 12, root_hwnd: 34, observation_id: "obs".into(),
            window_ref: "window".into(), rect: FlowWindowRect { x: 0, y: 0, width: 100, height: 100 },
            action, verify: FlowMouseVerification::Selection { control_hwnd: 56, start: 0, end: 2 } }
    }

    #[test]
    fn validates_recorded_pointer_action_shapes() {
        assert!(validate_guarded(&step(FlowMouseAction::Drag {
            points: vec![point(10, 0), point(20, 50)], button: 1,
        })).is_ok());
        assert!(validate_guarded(&step(FlowMouseAction::Drag {
            points: vec![point(10, 20), point(20, 10)], button: 1,
        })).is_err());
        assert!(validate_guarded(&step(FlowMouseAction::Wheel {
            point: point(10, 0), delta_x: 0, delta_y: 0,
        })).is_err());
    }

    #[test]
    fn position_evidence_is_only_valid_for_pointer_movement() {
        let mut request = step(FlowMouseAction::Move { point: point(10, 0) });
        request.verify = FlowMouseVerification::Position { control_hwnd: 34 };
        assert!(validate_guarded(&request).is_ok());
        request.action = FlowMouseAction::Click { point: point(10, 0), button: 1, hold_ns: 0 };
        assert!(validate_guarded(&request).is_err());
    }

    #[test]
    fn focus_verification_requires_a_real_focus_change() {
        let verify = FlowMouseVerification::Focus { control_hwnd: 56 };
        assert!(!precondition(&verify, 1));
        assert!(precondition(&verify, 0));
        assert!(!postcondition(&verify, 0, 0));
        assert!(postcondition(&verify, 0, 1));
    }

    #[test]
    fn rejects_any_outside_point_before_dispatch() {
        let rect = FlowWindowRect { x: 0, y: 0, width: 100, height: 100 };
        let inside = FlowMouseAction::Drag { points: vec![point(10, 0), point(99, 10)], button: 1 };
        let outside = FlowMouseAction::Drag { points: vec![point(10, 0), point(100, 10)], button: 1 };
        assert!(validate_points_in_rect(&inside, &rect).is_ok());
        assert!(validate_points_in_rect(&outside, &rect).is_err());
    }

    #[test]
    fn preserves_wheel_deltas_without_notch_scaling() {
        let events = raw_wheel_events(15, 120);
        assert_eq!(events[0], (enigo::Axis::Horizontal, 15));
        assert_eq!(events[1], (enigo::Axis::Vertical, 120));
    }
}
