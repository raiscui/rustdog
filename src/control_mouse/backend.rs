use super::{
    plan::{MouseExecutionPlan, MousePlanStep},
    report::{MouseExecutionReport, MouseReleaseRecovery},
};
use crate::control_display_scope::display_contains_point;
use crate::control_mouse::request::MousePoint;
use crate::control_overlay;
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
    // 最近一次 os-logical 移动坐标: 点击命中时在屏幕上闪烁提示用户
    let mut last_os_point = None::<(i32, i32)>;

    for step in &plan.steps {
        let result = match *step {
            MousePlanStep::Move { x, y, coordinate } => {
                if coordinate == Coordinate::Abs {
                    last_os_point = Some((x, y));
                }
                let result = backend.move_mouse(x, y, coordinate);
                if result.is_ok() {
                    // 实测 (#102): warp/moved 事件与紧随的 click 之间需要让
                    // WindowServer 收敛, 否则 click 落在移动前的旧位置
                    // (跨显示器移动尤甚, 表现为两次点击都落在 one 位置)。
                    thread::sleep(Duration::from_millis(40));
                }
                result
            }
            MousePlanStep::Button { button, direction } => {
                let result = backend.button(button, direction);
                if result.is_ok() {
                    // 可视化反馈: 点击/按下时在屏幕上闪烁标记, 让用户看到 agent 动作
                    if matches!(direction, Direction::Click | Direction::Press) {
                        if let Some((x, y)) = last_os_point {
                            control_overlay::flash_click(x, y);
                        }
                    }
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

    let mut report = plan.report.clone();
    enrich_report_with_display(&mut report, last_os_point);
    Ok(report)
}

/// 用 display catalog 补全点击落点的归属信息 (display_id / scale)。
///
/// 失败静默: 归属信息是**增强证据**, 拿不到 (catalog 枚举失败/无命中屏)
/// 不该让一次成功的点击变成失败 —— 坐标本身已经执行成功。
/// 无命中屏 (坐标在所有屏之外) 同样静默, agent 可从缺字段判断异常。
fn enrich_report_with_display(report: &mut MouseExecutionReport, os_point: Option<(i32, i32)>) {
    let Some((x, y)) = os_point else {
        return;
    };
    report.resolved_os_point = Some(MousePoint { x, y });
    let Ok(displays) = crate::screenshot::current_display_summaries() else {
        return;
    };
    let Some(display) = displays
        .iter()
        .find(|display| display_contains_point(display, x, y))
    else {
        return;
    };
    report.display_id = Some(display.display_id.clone());
    report.applied_scale_factor = Some(display.scale_factor);
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
