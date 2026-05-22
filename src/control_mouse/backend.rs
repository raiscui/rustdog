use super::{
    plan::{MouseExecutionPlan, MousePlanStep},
    report::{MouseExecutionReport, MouseReleaseRecovery},
};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, InputError, Mouse};
use std::{io, thread, time::Duration};

pub trait MouseBackend {
    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> Result<(), InputError>;
    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError>;
    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError>;
}

impl MouseBackend for Enigo {
    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> Result<(), InputError> {
        Mouse::move_mouse(self, x, y, coordinate)
    }

    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError> {
        Mouse::button(self, button, direction)
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError> {
        Mouse::scroll(self, length, axis)
    }
}

/// 执行鼠标 plan,并在组合动作中尽量修复遗留按下状态。
///
/// `@mouse-button mode:"press"` 的 plan 没有后续步骤,因此不会被这里自动 release。
/// 只有 press 后的后续 step 失败时,才执行恢复 release。
pub fn perform_mouse_plan<B: MouseBackend>(
    backend: &mut B,
    plan: &MouseExecutionPlan,
) -> io::Result<MouseExecutionReport> {
    let mut pressed_button = None::<Button>;

    for step in &plan.steps {
        let result = match *step {
            MousePlanStep::Move { x, y, coordinate } => backend.move_mouse(x, y, coordinate),
            MousePlanStep::Button { button, direction } => {
                let result = backend.button(button, direction);
                if result.is_ok() {
                    match direction {
                        Direction::Press => pressed_button = Some(button),
                        Direction::Release if pressed_button == Some(button) => {
                            pressed_button = None
                        }
                        Direction::Click | Direction::Release => {}
                    }
                }
                result
            }
            MousePlanStep::Hold(hold_ms) => {
                thread::sleep(Duration::from_millis(hold_ms));
                Ok(())
            }
            MousePlanStep::Scroll { length, axis } => backend.scroll(length, axis),
        };

        if let Err(err) = result {
            return recover_after_mouse_failure(backend, pressed_button, err);
        }
    }

    Ok(plan.report.clone())
}

fn recover_after_mouse_failure<B: MouseBackend>(
    backend: &mut B,
    pressed_button: Option<Button>,
    err: InputError,
) -> io::Result<MouseExecutionReport> {
    let Some(button) = pressed_button else {
        return Err(io::Error::other(err.to_string()));
    };

    let (recovery, release_error) = match backend.button(button, Direction::Release) {
        Ok(()) => (MouseReleaseRecovery::ReleaseSucceeded, None),
        Err(release_err) => (MouseReleaseRecovery::ReleaseFailed, Some(release_err)),
    };
    let message = match (recovery, release_error) {
        (MouseReleaseRecovery::ReleaseSucceeded, None) => {
            format!("{err}; release_recovery=release-succeeded; 已在失败恢复中释放鼠标按钮")
        }
        (MouseReleaseRecovery::ReleaseFailed, Some(release_err)) => {
            format!("{err}; release_recovery=release-failed; 已尝试释放鼠标按钮,但 release 也失败: {release_err}")
        }
        _ => format!("{err}; release_recovery=unknown"),
    };

    Err(io::Error::other(message))
}
