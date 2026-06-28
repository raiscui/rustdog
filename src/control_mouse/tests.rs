use super::*;
use enigo::{Axis, Button, Coordinate, Direction, InputError};

#[derive(Default)]
struct FakeMouseBackend {
    calls: Vec<MousePlanStep>,
    fail_on_call: Option<usize>,
    release_fail: bool,
}

impl FakeMouseBackend {
    fn record_or_fail(&mut self, step: MousePlanStep) -> Result<(), InputError> {
        self.calls.push(step);
        if self.fail_on_call == Some(self.calls.len()) {
            return Err(InputError::Simulate("injected failure"));
        }
        Ok(())
    }
}

impl MouseBackend for FakeMouseBackend {
    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> Result<(), InputError> {
        self.record_or_fail(MousePlanStep::Move { x, y, coordinate })
    }

    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError> {
        if self.release_fail && direction == Direction::Release {
            self.calls.push(MousePlanStep::Button { button, direction });
            return Err(InputError::Simulate("release failed"));
        }
        self.record_or_fail(MousePlanStep::Button { button, direction })
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError> {
        self.record_or_fail(MousePlanStep::Scroll { length, axis })
    }
}

#[test]
fn click_plan_should_move_press_hold_release() {
    let plan = build_click_plan(&ClickRequest {
        x: Some(10),
        y: Some(20),
        target: None,
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 80,
        interval_ms: 120,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();

    assert_eq!(
        plan.steps,
        vec![
            MousePlanStep::Move {
                x: 10,
                y: 20,
                coordinate: Coordinate::Abs,
            },
            MousePlanStep::Button {
                button: Button::Left,
                direction: Direction::Press,
            },
            MousePlanStep::Hold(80),
            MousePlanStep::Button {
                button: Button::Left,
                direction: Direction::Release,
            },
        ]
    );
    assert_eq!(plan.report.released, Some(true));
}

#[test]
fn mouse_button_press_should_not_auto_release() {
    let plan = build_mouse_button_plan(&MouseButtonRequest {
        button: MouseButtonName::Left,
        mode: MouseButtonMode::Press,
        hold_ms: 80,
    })
    .unwrap();

    assert_eq!(
        plan.steps,
        vec![MousePlanStep::Button {
            button: Button::Left,
            direction: Direction::Press,
        }]
    );
    assert_eq!(plan.report.released, None);
}

#[test]
fn drag_plan_should_sample_from_to_and_release() {
    let plan = build_drag_plan(&DragRequest {
        from: MouseEndpoint::Coordinate(MousePoint { x: 0, y: 0 }),
        to: MouseEndpoint::Coordinate(MousePoint { x: 10, y: 10 }),
        guard: None,
        button: MouseButtonName::Left,
        duration_ms: 4,
        steps: 2,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();

    assert_eq!(
        plan.steps,
        vec![
            MousePlanStep::Move {
                x: 0,
                y: 0,
                coordinate: Coordinate::Abs,
            },
            MousePlanStep::Button {
                button: Button::Left,
                direction: Direction::Press,
            },
            MousePlanStep::Hold(2),
            MousePlanStep::Move {
                x: 5,
                y: 5,
                coordinate: Coordinate::Abs,
            },
            MousePlanStep::Hold(2),
            MousePlanStep::Move {
                x: 10,
                y: 10,
                coordinate: Coordinate::Abs,
            },
            MousePlanStep::Button {
                button: Button::Left,
                direction: Direction::Release,
            },
        ]
    );
}

#[test]
fn drag_interpolation_should_not_overflow_i32_delta() {
    assert_eq!(
        interpolate_point(
            MousePoint {
                x: i32::MIN,
                y: i32::MIN,
            },
            MousePoint {
                x: i32::MAX,
                y: i32::MAX,
            },
            2,
            2,
        ),
        MousePoint {
            x: i32::MAX,
            y: i32::MAX,
        }
    );
}

#[test]
fn drag_failure_after_press_should_attempt_release() {
    let plan = build_drag_plan(&DragRequest {
        from: MouseEndpoint::Coordinate(MousePoint { x: 0, y: 0 }),
        to: MouseEndpoint::Coordinate(MousePoint { x: 10, y: 10 }),
        guard: None,
        button: MouseButtonName::Left,
        duration_ms: 4,
        steps: 2,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();
    let mut backend = FakeMouseBackend {
        fail_on_call: Some(4),
        release_fail: false,
        calls: Vec::new(),
    };

    let err = perform_mouse_plan(&mut backend, &plan).unwrap_err();

    assert!(err.to_string().contains("已在失败恢复中释放鼠标按钮"));
    assert_eq!(
        backend.calls.last(),
        Some(&MousePlanStep::Button {
            button: Button::Left,
            direction: Direction::Release,
        })
    );
}

#[test]
fn drag_failure_should_report_release_failure() {
    let plan = build_drag_plan(&DragRequest {
        from: MouseEndpoint::Coordinate(MousePoint { x: 0, y: 0 }),
        to: MouseEndpoint::Coordinate(MousePoint { x: 10, y: 10 }),
        guard: None,
        button: MouseButtonName::Left,
        duration_ms: 4,
        steps: 2,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();
    let mut backend = FakeMouseBackend {
        fail_on_call: Some(4),
        release_fail: true,
        calls: Vec::new(),
    };

    let err = perform_mouse_plan(&mut backend, &plan).unwrap_err();

    assert!(err.to_string().contains("release 也失败"));
}

#[test]
fn wheel_plan_should_use_vertical_then_horizontal_order() {
    let plan = build_wheel_plan(&WheelRequest {
        x: Some(10),
        y: Some(20),
        target: None,
        guard: None,
        delta_x: 2,
        delta_y: -3,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();

    assert_eq!(
        plan.steps,
        vec![
            MousePlanStep::Move {
                x: 10,
                y: 20,
                coordinate: Coordinate::Abs,
            },
            MousePlanStep::Scroll {
                length: -3,
                axis: Axis::Vertical,
            },
            MousePlanStep::Scroll {
                length: 2,
                axis: Axis::Horizontal,
            },
        ]
    );
    assert_eq!(plan.report.wheel_order, vec!["vertical", "horizontal"]);
}

#[test]
fn report_should_render_structured_mouse_json() {
    let plan = build_mouse_move_plan(&MouseMoveRequest {
        x: Some(10),
        y: Some(20),
        dx: None,
        dy: None,
        target: None,
        guard: None,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();

    let report_json = plan.report.to_value_json();

    assert!(report_json.contains(r#""kind":"mouse""#));
    assert!(report_json.contains(r#""action":"move""#));
    assert!(report_json.contains(r#""coordinate_space":"os-logical""#));
    assert!(report_json.contains(r#""x":10"#));
}
