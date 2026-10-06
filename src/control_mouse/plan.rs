use super::{
    backend::MOUSE_BACKEND_NAME,
    report::{base_report, MouseAction, MouseExecutionReport},
    request::{
        validate_mouse_move_shape, validate_wheel_shape, ClickRequest, DragRequest,
        MouseButtonMode, MouseButtonName, MouseButtonRequest, MouseCoordinateSpace, MouseEndpoint,
        MouseMoveRequest, MousePoint, WheelRequest, MAX_MOUSE_CLICK_COUNT, MAX_MOUSE_DRAG_STEPS,
    },
};
use enigo::{Axis, Button, Coordinate, Direction};
use std::io;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MousePlanStep {
    Move {
        x: i32,
        y: i32,
        coordinate: Coordinate,
    },
    Button {
        button: Button,
        direction: Direction,
    },
    Hold(u64),
    Scroll {
        length: i32,
        axis: Axis,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseExecutionPlan {
    pub steps: Vec<MousePlanStep>,
    pub report: MouseExecutionReport,
}

pub fn build_mouse_move_plan(request: &MouseMoveRequest) -> io::Result<MouseExecutionPlan> {
    validate_mouse_move_shape(request, io::ErrorKind::InvalidInput)?;
    let (x, y, coordinate, report_x, report_y, report_dx, report_dy) =
        match request.coordinate_space {
            MouseCoordinateSpace::OsLogical => {
                let point = match &request.target {
                    Some(target) => endpoint_coordinate(target, "@mouse-move.target")?,
                    None => MousePoint {
                        x: request.x.expect("validated x"),
                        y: request.y.expect("validated y"),
                    },
                };
                let x = point.x;
                let y = point.y;
                ensure_supported_absolute_coordinate(x, y)?;
                (x, y, Coordinate::Abs, Some(x), Some(y), None, None)
            }
            MouseCoordinateSpace::Relative => (
                request.dx.expect("validated dx"),
                request.dy.expect("validated dy"),
                Coordinate::Rel,
                None,
                None,
                request.dx,
                request.dy,
            ),
        };

    Ok(MouseExecutionPlan {
        steps: vec![MousePlanStep::Move { x, y, coordinate }],
        report: base_report(MouseAction::Move, Some(request.coordinate_space), MOUSE_BACKEND_NAME)
            .with_point_fields(report_x, report_y, report_dx, report_dy),
    })
}

pub fn build_mouse_button_plan(request: &MouseButtonRequest) -> io::Result<MouseExecutionPlan> {
    let button = to_enigo_button(request.button);
    let mut steps = Vec::new();

    match request.mode {
        MouseButtonMode::Press => steps.push(MousePlanStep::Button {
            button,
            direction: Direction::Press,
        }),
        MouseButtonMode::Release => steps.push(MousePlanStep::Button {
            button,
            direction: Direction::Release,
        }),
        MouseButtonMode::Click => {
            steps.push(MousePlanStep::Button {
                button,
                direction: Direction::Press,
            });
            push_hold(&mut steps, request.hold_ms);
            steps.push(MousePlanStep::Button {
                button,
                direction: Direction::Release,
            });
        }
    }

    Ok(MouseExecutionPlan {
        steps,
        report: base_report(MouseAction::Button, None, MOUSE_BACKEND_NAME)
            .with_button_fields(request),
    })
}

pub fn build_click_plan(request: &ClickRequest) -> io::Result<MouseExecutionPlan> {
    if request.coordinate_space != MouseCoordinateSpace::OsLogical {
        return Err(invalid_input(
            "@click 当前只支持 coordinate_space=\"os-logical\"",
        ));
    }
    if !(1..=MAX_MOUSE_CLICK_COUNT).contains(&request.count) {
        return Err(invalid_input(format!(
            "@click 的 `count` 必须在 1..={MAX_MOUSE_CLICK_COUNT} 之间"
        )));
    }
    let point = click_coordinate(request)?;
    ensure_supported_absolute_coordinate(point.x, point.y)?;

    let button = to_enigo_button(request.button);
    let mut steps = vec![MousePlanStep::Move {
        x: point.x,
        y: point.y,
        coordinate: Coordinate::Abs,
    }];

    for click_index in 0..request.count {
        steps.push(MousePlanStep::Button {
            button,
            direction: Direction::Press,
        });
        push_hold(&mut steps, request.hold_ms);
        steps.push(MousePlanStep::Button {
            button,
            direction: Direction::Release,
        });
        if click_index + 1 < request.count {
            push_hold(&mut steps, request.interval_ms);
        }
    }

    Ok(MouseExecutionPlan {
        steps,
        report: base_report(MouseAction::Click, Some(request.coordinate_space), MOUSE_BACKEND_NAME)
            .with_click_fields(request, point),
    })
}

pub fn build_drag_plan(request: &DragRequest) -> io::Result<MouseExecutionPlan> {
    if request.coordinate_space != MouseCoordinateSpace::OsLogical {
        return Err(invalid_input(
            "@drag 当前只支持 coordinate_space=\"os-logical\"",
        ));
    }
    if request.steps == 0 || request.steps > MAX_MOUSE_DRAG_STEPS {
        return Err(invalid_input(format!(
            "@drag 的 `steps` 必须在 1..={MAX_MOUSE_DRAG_STEPS} 之间"
        )));
    }
    let from = endpoint_coordinate(&request.from, "@drag.from")?;
    let to = endpoint_coordinate(&request.to, "@drag.to")?;
    ensure_supported_absolute_coordinate(from.x, from.y)?;
    ensure_supported_absolute_coordinate(to.x, to.y)?;

    let button = to_enigo_button(request.button);
    let mut steps = vec![
        MousePlanStep::Move {
            x: from.x,
            y: from.y,
            coordinate: Coordinate::Abs,
        },
        MousePlanStep::Button {
            button,
            direction: Direction::Press,
        },
    ];
    let per_step_hold_ms = std::cmp::max(1, request.duration_ms / u64::from(request.steps));

    for step_index in 1..=request.steps {
        steps.push(MousePlanStep::Hold(per_step_hold_ms));
        let point = interpolate_point(from, to, step_index, request.steps);
        steps.push(MousePlanStep::Move {
            x: point.x,
            y: point.y,
            coordinate: Coordinate::Abs,
        });
    }

    steps.push(MousePlanStep::Button {
        button,
        direction: Direction::Release,
    });

    Ok(MouseExecutionPlan {
        steps,
        report: base_report(MouseAction::Drag, Some(request.coordinate_space), MOUSE_BACKEND_NAME)
            .with_drag_fields(request, from, to),
    })
}

pub fn build_wheel_plan(request: &WheelRequest) -> io::Result<MouseExecutionPlan> {
    validate_wheel_shape(request, io::ErrorKind::InvalidInput)?;

    let mut steps = Vec::new();
    let target_point = wheel_coordinate(request)?;
    if let Some(point) = target_point {
        ensure_supported_absolute_coordinate(point.x, point.y)?;
        steps.push(MousePlanStep::Move {
            x: point.x,
            y: point.y,
            coordinate: Coordinate::Abs,
        });
    }

    let mut wheel_order = Vec::new();
    if request.delta_y != 0 {
        steps.push(MousePlanStep::Scroll {
            length: request.delta_y,
            axis: Axis::Vertical,
        });
        wheel_order.push("vertical");
    }
    if request.delta_x != 0 {
        steps.push(MousePlanStep::Scroll {
            length: request.delta_x,
            axis: Axis::Horizontal,
        });
        wheel_order.push("horizontal");
    }

    Ok(MouseExecutionPlan {
        steps,
        report: base_report(
            MouseAction::Wheel,
            target_point.is_some().then_some(request.coordinate_space),
            MOUSE_BACKEND_NAME,
        )
        .with_wheel_fields(request, target_point, wheel_order),
    })
}

fn to_enigo_button(button: MouseButtonName) -> Button {
    match button {
        MouseButtonName::Left => Button::Left,
        MouseButtonName::Right => Button::Right,
        MouseButtonName::Middle => Button::Middle,
        MouseButtonName::Back => Button::Back,
        MouseButtonName::Forward => Button::Forward,
    }
}

fn push_hold(steps: &mut Vec<MousePlanStep>, hold_ms: u64) {
    if hold_ms > 0 {
        steps.push(MousePlanStep::Hold(hold_ms));
    }
}

fn click_coordinate(request: &ClickRequest) -> io::Result<MousePoint> {
    match &request.target {
        Some(target) => endpoint_coordinate(target, "@click.target"),
        None => Ok(MousePoint {
            x: request.x.ok_or_else(|| invalid_input("@click 缺少 `x`"))?,
            y: request.y.ok_or_else(|| invalid_input("@click 缺少 `y`"))?,
        }),
    }
}

fn wheel_coordinate(request: &WheelRequest) -> io::Result<Option<MousePoint>> {
    match &request.target {
        Some(target) => endpoint_coordinate(target, "@wheel.target").map(Some),
        None => match (request.x, request.y) {
            (Some(x), Some(y)) => Ok(Some(MousePoint { x, y })),
            (None, None) => Ok(None),
            _ => Err(invalid_input("@wheel 的 `x` 和 `y` 必须同时提供")),
        },
    }
}

fn endpoint_coordinate(endpoint: &MouseEndpoint, label: &str) -> io::Result<MousePoint> {
    match endpoint {
        MouseEndpoint::Coordinate(point) => Ok(*point),
        MouseEndpoint::ObservationRef(_)
        | MouseEndpoint::Selector(_)
        | MouseEndpoint::ScreenshotPoint(_) => Err(invalid_input(format!(
            "{label} 需要先经过 mouse target resolver 才能构建执行 plan"
        ))),
    }
}

pub(crate) fn interpolate_point(
    from: MousePoint,
    to: MousePoint,
    step_index: u16,
    steps: u16,
) -> MousePoint {
    let step_index = i64::from(step_index);
    let steps = i64::from(steps);
    let x_delta = i64::from(to.x) - i64::from(from.x);
    let y_delta = i64::from(to.y) - i64::from(from.y);
    let x = i64::from(from.x) + (x_delta * step_index) / steps;
    let y = i64::from(from.y) + (y_delta * step_index) / steps;
    MousePoint {
        x: x as i32,
        y: y as i32,
    }
}

fn ensure_supported_absolute_coordinate(x: i32, y: i32) -> io::Result<()> {
    if (x < 0 || y < 0) && !cfg!(target_os = "macos") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "当前平台 backend 尚未证明支持负数 os-logical 多显示器坐标",
        ));
    }
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_mouse::request::{MouseDisplayGuard, MouseEndpoint};

    /// 造一个最小的 os-logical 点端点 — 五个动作的 fixture 共用。
    fn point(x: i32, y: i32) -> MouseEndpoint {
        MouseEndpoint::Coordinate(MousePoint { x, y })
    }

    fn click_request() -> ClickRequest {
        ClickRequest {
            x: Some(1200),
            y: Some(540),
            target: None,
            guard: None::<MouseDisplayGuard>,
            button: MouseButtonName::Left,
            count: 1,
            hold_ms: 80,
            interval_ms: 120,
            coordinate_space: MouseCoordinateSpace::OsLogical,
        }
    }

    /// 回归断言 (2026-10-06 真机取证): 回执 `backend` 字段必须报**当前平台
    /// 真正发事件的那个后端**, 不能无条件写死 "enigo"。
    ///
    /// 起因: Windows 上 `@click` 实际走 `WinNativeMouseBackend`
    /// (SetCursorPos + SendInput), 回执却一直报 "enigo" —— 行为对、标签撒谎。
    /// 本用例两平台通用且不写死平台字符串: 期望值取自 `MOUSE_BACKEND_NAME`,
    /// 五个动作 (click/move/button/drag/wheel) 一并覆盖, 结构体字段与序列化
    /// JSON 两侧都断言, 防止只修一半。
    ///
    /// 已知局限 (2026-10-06 实测): macOS 上期望值本身就是 "enigo", 把写死的
    /// "enigo" 换回点击位测试**照样绿** —— 这条断言只在 Windows 目标上
    /// (期望 "win-native") 才具备判别力, 那边是否跑过需在统一编译环节确认。
    #[test]
    fn mouse_receipt_reports_the_backend_that_actually_runs() {
        let plans = [
            (
                "click",
                build_click_plan(&click_request()).unwrap(),
            ),
            (
                "move",
                build_mouse_move_plan(&MouseMoveRequest {
                    x: Some(1200),
                    y: Some(540),
                    dx: None,
                    dy: None,
                    target: None,
                    guard: None,
                    coordinate_space: MouseCoordinateSpace::OsLogical,
                })
                .unwrap(),
            ),
            (
                "button",
                build_mouse_button_plan(&MouseButtonRequest {
                    button: MouseButtonName::Left,
                    mode: MouseButtonMode::Click,
                    hold_ms: 80,
                })
                .unwrap(),
            ),
            (
                "drag",
                build_drag_plan(&DragRequest {
                    from: point(100, 100),
                    to: point(200, 200),
                    guard: None,
                    button: MouseButtonName::Left,
                    duration_ms: 500,
                    steps: 5,
                    coordinate_space: MouseCoordinateSpace::OsLogical,
                })
                .unwrap(),
            ),
            (
                "wheel",
                build_wheel_plan(&WheelRequest {
                    x: Some(1200),
                    y: Some(540),
                    target: None,
                    guard: None,
                    delta_x: 0,
                    delta_y: 3,
                    coordinate_space: MouseCoordinateSpace::OsLogical,
                })
                .unwrap(),
            ),
        ];

        for (action, plan) in plans {
            // 结构体字段与最终序列化 JSON 两侧都要对: 只改其中一侧的回归
            // (例如有人改 to_value_json 绕开常量) 同样会被这里抓住。
            assert_eq!(
                plan.report.backend, MOUSE_BACKEND_NAME,
                "@{action} 回执的 backend 字段应报实际执行的后端, 而非写死值"
            );
            let value: serde_json::Value =
                serde_json::from_str(&plan.report.to_value_json()).expect("回执应是合法 JSON");
            assert_eq!(
                value["backend"], MOUSE_BACKEND_NAME,
                "@{action} 序列化后的 backend 字段应报实际执行的后端"
            );
        }
    }
}
