use super::*;
use crate::control_mouse::backend::{AppliedPoint, AppliedPointSpace};
use enigo::{Axis, Button, Coordinate, Direction, InputError};

#[derive(Default)]
struct FakeMouseBackend {
    calls: Vec<MousePlanStep>,
    fail_on_call: Option<usize>,
    release_fail: bool,
    /// 冒充一个**做了坐标换算**的后端: 回报自己"实际发给 OS"的落点。
    ///
    /// `None` = 老实的透传后端 (enigo 语义: 落点 = 协议点, 空间 os-logical)。
    /// 用它可以证伪"回执只是把 plan 里的协议点抄了一遍"这种接线。
    applied_point_override: Option<AppliedPoint>,
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
    fn move_mouse(
        &mut self,
        x: i32,
        y: i32,
        coordinate: Coordinate,
    ) -> Result<Option<AppliedPoint>, InputError> {
        self.record_or_fail(MousePlanStep::Move { x, y, coordinate })?;
        // 相对移动没有绝对落点, 与两个真后端一致地回 None。
        Ok(if coordinate == Coordinate::Abs {
            Some(self.applied_point_override.unwrap_or(AppliedPoint {
                point: MousePoint { x, y },
                space: AppliedPointSpace::OsLogical,
            }))
        } else {
            None
        })
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
        applied_point_override: None,
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
        applied_point_override: None,
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

/// 回归断言 (2026-10-06 真机取证): 回执 `resolved_os_point` 必须是**后端真正
/// 发给操作系统的那一点**, 而不是 plan 里的协议坐标。
///
/// 起因: Windows 走 `WinNativeMouseBackend`, 内部按命中屏 scale 把 os-logical
/// 换算成物理像素才 `SetCursorPos`; 回执此前记的是换算前的协议点 —— 在 4K@200%
/// 的机器上等于报了一个屏幕上根本不存在的位置, agent 拿它判断"点了哪里"必被带偏。
///
/// 这里让假后端冒充"做过换算"的后端: 入参 (605,1056), 回报 (1210,2112)/physical。
/// 若接线仍是抄 plan 的协议点, 断言立刻红。三处一起断: 数据源 (后端回报)、
/// 空间标签、以及 JSON 真的把这两个字段吐出去 (此前它们被误嵌在
/// `release_recovery` 分支内, 正常回执永远吐不出来)。
#[test]
fn receipt_reports_the_point_the_backend_actually_applied() {
    let plan = build_click_plan(&ClickRequest {
        x: Some(605),
        y: Some(1056),
        target: None,
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 0,
        interval_ms: 0,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();
    let mut backend = FakeMouseBackend {
        applied_point_override: Some(AppliedPoint {
            point: MousePoint { x: 1210, y: 2112 },
            space: AppliedPointSpace::Physical,
        }),
        ..Default::default()
    };

    let report = perform_mouse_plan(&mut backend, &plan).unwrap();

    assert_eq!(
        report.resolved_os_point,
        Some(MousePoint { x: 1210, y: 2112 }),
        "回执落点应取后端回报的值, 而非 plan 里的协议坐标"
    );
    assert_eq!(
        report.resolved_os_point_space,
        Some(AppliedPointSpace::Physical),
        "落点空间由后端申报; 只给点不给空间等于让 agent 自己猜缩放"
    );
    // 协议坐标仍原样留在请求字段 —— agent 要换算回自己的截图时用得着, 不能被
    // 物理落点顶替 (那是另一个空间的数)。
    assert_eq!((report.x, report.y), (Some(605), Some(1056)));

    let value: serde_json::Value =
        serde_json::from_str(&report.to_value_json()).expect("回执应是合法 JSON");
    assert_eq!(value["resolved_os_point"]["x"], 1210);
    assert_eq!(value["resolved_os_point"]["y"], 2112);
    assert_eq!(value["resolved_os_point_space"], "physical");
}

/// 不做换算的后端 (enigo 语义) 必须**如实回报同一个坐标与同一个空间** ——
/// 修好 Windows 侧不能把 macOS 侧说成别的空间, 反向漂移同样是撒谎。
#[test]
fn passthrough_backend_reports_the_same_point_as_os_logical() {
    let plan = build_click_plan(&ClickRequest {
        x: Some(120),
        y: Some(340),
        target: None,
        guard: None,
        button: MouseButtonName::Left,
        count: 1,
        hold_ms: 0,
        interval_ms: 0,
        coordinate_space: MouseCoordinateSpace::OsLogical,
    })
    .unwrap();
    let mut backend = FakeMouseBackend::default();

    let report = perform_mouse_plan(&mut backend, &plan).unwrap();

    assert_eq!(report.resolved_os_point, Some(MousePoint { x: 120, y: 340 }));
    assert_eq!(
        report.resolved_os_point_space,
        Some(AppliedPointSpace::OsLogical)
    );
}

/// 顶层 `screenshot` 与显式坐标是并列写法: 带 screenshot 时 x/y 是图上像素,
/// 不能落回 `MouseEndpoint::Coordinate` (那是 os-logical)。
#[test]
fn click_with_screenshot_field_should_parse_into_screenshot_endpoint() {
    let request = parse_click_payload(r#"{screenshot:"1759670400000",x:1104,y:213}"#)
        .expect("screenshot click should parse");

    assert_eq!(request.x, None);
    assert_eq!(request.y, None);
    assert_eq!(
        request.target,
        Some(MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
            screenshot_id: "1759670400000".to_owned(),
            point: MousePoint { x: 1104, y: 213 },
        }))
    );
}

/// 不带 screenshot 的老写法语义不变 —— 回归防线: 少一个字段就把 os-logical
/// 坐标解释成图上像素, 是最坏的一类退化。
#[test]
fn click_without_screenshot_field_should_stay_os_logical_coordinate() {
    let request = parse_click_payload("{x:1104,y:213}").expect("coordinate click should parse");

    assert_eq!(request.x, Some(1104));
    assert_eq!(request.y, Some(213));
    assert_eq!(request.target, None);
}

#[test]
fn click_screenshot_field_should_reject_missing_pixel() {
    let err = parse_click_payload(r#"{screenshot:"1759670400000"}"#)
        .expect_err("screenshot click without x/y should fail");

    assert!(err.to_string().contains("`x`"), "{err}");
}

#[test]
fn click_screenshot_field_should_reject_mixing_with_target() {
    let err = parse_click_payload(r#"{screenshot:"a",target:{x:1,y:2}}"#)
        .expect_err("screenshot with target should fail");

    assert!(err.to_string().contains("不能与 `target` 混用"), "{err}");
}

/// 端点内写法与顶层写法同义, 让 @drag/@wheel/@mouse-move 的 target 也能引用截图。
#[test]
fn screenshot_endpoint_should_parse_inside_target() {
    let request = parse_drag_payload(
        r#"{from:{screenshot:"1759670400000",x:10,y:20},to:{x:30,y:40}}"#,
    )
    .expect("drag with screenshot endpoint should parse");

    assert_eq!(
        request.from,
        MouseEndpoint::ScreenshotPoint(MouseScreenshotTarget {
            screenshot_id: "1759670400000".to_owned(),
            point: MousePoint { x: 10, y: 20 },
        })
    );
}

/// 图上像素点是字面坐标, 没有目标矩形可锚: 收下 anchor 等于默默忽略调用方意图。
#[test]
fn screenshot_endpoint_should_reject_anchor_fields() {
    let err = parse_click_payload(r#"{target:{screenshot:"a",x:1,y:2,anchor:"center"}}"#)
        .expect_err("screenshot endpoint with anchor should fail");

    assert!(
        err.to_string().contains("不能携带 anchor/offset/selector 字段"),
        "{err}"
    );
}

#[test]
fn screenshot_endpoint_should_not_mix_with_ref_target() {
    let err =
        parse_click_payload(r#"{target:{screenshot:"a",x:1,y:2,ref:"@e1",observation_id:"o"}}"#)
            .expect_err("screenshot endpoint mixed with ref should fail");

    assert!(
        err.to_string().contains("必须且只能包含 x/y、screenshot、ref"),
        "{err}"
    );
}
