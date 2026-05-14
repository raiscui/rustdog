use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, InputError, Mouse};
use serde_json::json;
use std::{io, thread, time::Duration};

pub const DEFAULT_MOUSE_CLICK_HOLD_MS: u64 = 80;
pub const DEFAULT_MOUSE_CLICK_INTERVAL_MS: u64 = 120;
pub const DEFAULT_MOUSE_DRAG_DURATION_MS: u64 = 450;
pub const DEFAULT_MOUSE_DRAG_STEPS: u16 = 24;
pub const MAX_MOUSE_CLICK_COUNT: u8 = 3;
pub const MAX_MOUSE_DRAG_STEPS: u16 = 120;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseCoordinateSpace {
    OsLogical,
    Relative,
}

impl MouseCoordinateSpace {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::OsLogical => "os-logical",
            Self::Relative => "relative",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseButtonName {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButtonName {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Back => "back",
            Self::Forward => "forward",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseButtonMode {
    Press,
    Release,
    Click,
}

impl MouseButtonMode {
    pub fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Release => "release",
            Self::Click => "click",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MousePoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseMoveRequest {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseButtonRequest {
    pub button: MouseButtonName,
    pub mode: MouseButtonMode,
    pub hold_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickRequest {
    pub x: i32,
    pub y: i32,
    pub button: MouseButtonName,
    pub count: u8,
    pub hold_ms: u64,
    pub interval_ms: u64,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragRequest {
    pub from: MousePoint,
    pub to: MousePoint,
    pub button: MouseButtonName,
    pub duration_ms: u64,
    pub steps: u16,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelRequest {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub delta_x: i32,
    pub delta_y: i32,
    pub coordinate_space: MouseCoordinateSpace,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseReleaseRecovery {
    NotNeeded,
    ReleaseSucceeded,
    ReleaseFailed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Button,
    Click,
    Drag,
    Wheel,
}

impl MouseAction {
    fn as_protocol_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Button => "button",
            Self::Click => "click",
            Self::Drag => "drag",
            Self::Wheel => "wheel",
        }
    }
}

/// 鼠标动作成功后的结构化证据。
///
/// 这里直接生成 JSON value 字符串,上层只负责按 request id 包一层。
/// 这样可以避免把 mouse metadata 当普通 stdout 再做一次 JSON 转义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseExecutionReport {
    pub action: MouseAction,
    pub coordinate_space: Option<MouseCoordinateSpace>,
    pub backend: &'static str,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub dx: Option<i32>,
    pub dy: Option<i32>,
    pub from: Option<MousePoint>,
    pub to: Option<MousePoint>,
    pub button: Option<MouseButtonName>,
    pub mode: Option<MouseButtonMode>,
    pub count: Option<u8>,
    pub hold_ms: Option<u64>,
    pub interval_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub steps: Option<u16>,
    pub delta_x: Option<i32>,
    pub delta_y: Option<i32>,
    pub wheel_order: Vec<&'static str>,
    pub released: Option<bool>,
    pub release_recovery: MouseReleaseRecovery,
}

impl MouseExecutionReport {
    pub fn to_value_json(&self) -> String {
        let mut value = json!({
            "kind": "mouse",
            "action": self.action.as_protocol_str(),
            "backend": self.backend,
            "status": "ok",
        });

        if let Some(coordinate_space) = self.coordinate_space {
            value["coordinate_space"] = json!(coordinate_space.as_protocol_str());
        }
        if let Some(x) = self.x {
            value["x"] = json!(x);
        }
        if let Some(y) = self.y {
            value["y"] = json!(y);
        }
        if let Some(dx) = self.dx {
            value["dx"] = json!(dx);
        }
        if let Some(dy) = self.dy {
            value["dy"] = json!(dy);
        }
        if let Some(from) = self.from {
            value["from"] = json!({ "x": from.x, "y": from.y });
        }
        if let Some(to) = self.to {
            value["to"] = json!({ "x": to.x, "y": to.y });
        }
        if let Some(button) = self.button {
            value["button"] = json!(button.as_protocol_str());
        }
        if let Some(mode) = self.mode {
            value["mode"] = json!(mode.as_protocol_str());
        }
        if let Some(count) = self.count {
            value["count"] = json!(count);
        }
        if let Some(hold_ms) = self.hold_ms {
            value["hold_ms"] = json!(hold_ms);
        }
        if let Some(interval_ms) = self.interval_ms {
            value["interval_ms"] = json!(interval_ms);
        }
        if let Some(duration_ms) = self.duration_ms {
            value["duration_ms"] = json!(duration_ms);
        }
        if let Some(steps) = self.steps {
            value["steps"] = json!(steps);
        }
        if let Some(delta_x) = self.delta_x {
            value["delta_x"] = json!(delta_x);
        }
        if let Some(delta_y) = self.delta_y {
            value["delta_y"] = json!(delta_y);
        }
        if !self.wheel_order.is_empty() {
            value["wheel_order"] = json!(self.wheel_order);
        }
        if let Some(released) = self.released {
            value["released"] = json!(released);
        }
        if self.release_recovery != MouseReleaseRecovery::NotNeeded {
            value["release_recovery"] = json!(match self.release_recovery {
                MouseReleaseRecovery::NotNeeded => "not-needed",
                MouseReleaseRecovery::ReleaseSucceeded => "release-succeeded",
                MouseReleaseRecovery::ReleaseFailed => "release-failed",
            });
        }

        serde_json::to_string(&value).expect("mouse report should serialize")
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseExecutionPlan {
    pub steps: Vec<MousePlanStep>,
    pub report: MouseExecutionReport,
}

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

pub fn parse_mouse_move_payload(input: &str) -> io::Result<MouseMoveRequest> {
    let inner = object_inner(input, "@mouse-move")?;
    if inner.is_empty() {
        return Err(invalid_data("@mouse-move 对象 payload 不能为空"));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;
    let mut dx = None::<i32>;
    let mut dy = None::<i32>;
    let mut coordinate_space = None::<MouseCoordinateSpace>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", "@mouse-move", parse_i32_field("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", "@mouse-move", parse_i32_field("y", raw_value)?)?,
            "dx" => assign_once(
                &mut dx,
                "dx",
                "@mouse-move",
                parse_i32_field("dx", raw_value)?,
            )?,
            "dy" => assign_once(
                &mut dy,
                "dy",
                "@mouse-move",
                parse_i32_field("dy", raw_value)?,
            )?,
            "coordinate_space" => assign_once(
                &mut coordinate_space,
                "coordinate_space",
                "@mouse-move",
                parse_mouse_coordinate_space(raw_value, true)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@mouse-move 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let request = MouseMoveRequest {
        x,
        y,
        dx,
        dy,
        coordinate_space: coordinate_space.unwrap_or(MouseCoordinateSpace::OsLogical),
    };
    validate_mouse_move_shape(&request, io::ErrorKind::InvalidData)?;
    Ok(request)
}

pub fn parse_mouse_button_payload(input: &str) -> io::Result<MouseButtonRequest> {
    let inner = object_inner(input, "@mouse-button")?;
    if inner.is_empty() {
        return Err(invalid_data("@mouse-button 对象 payload 不能为空"));
    }

    let mut button = None::<MouseButtonName>;
    let mut mode = None::<MouseButtonMode>;
    let mut hold_ms = None::<u64>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "button" => assign_once(
                &mut button,
                "button",
                "@mouse-button",
                parse_mouse_button_name(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@mouse-button",
                parse_mouse_button_mode(raw_value)?,
            )?,
            "hold_ms" => assign_once(
                &mut hold_ms,
                "hold_ms",
                "@mouse-button",
                parse_u64_field("hold_ms", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@mouse-button 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(MouseButtonRequest {
        button: button.unwrap_or(MouseButtonName::Left),
        mode: mode.unwrap_or(MouseButtonMode::Click),
        hold_ms: hold_ms.unwrap_or(DEFAULT_MOUSE_CLICK_HOLD_MS),
    })
}

pub fn parse_click_payload(input: &str) -> io::Result<ClickRequest> {
    let inner = object_inner(input, "@click")?;
    if inner.is_empty() {
        return Err(invalid_data("@click 对象 payload 不能为空"));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;
    let mut button = None::<MouseButtonName>;
    let mut count = None::<u8>;
    let mut hold_ms = None::<u64>;
    let mut interval_ms = None::<u64>;
    let mut coordinate_space = None::<MouseCoordinateSpace>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", "@click", parse_i32_field("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", "@click", parse_i32_field("y", raw_value)?)?,
            "button" => assign_once(
                &mut button,
                "button",
                "@click",
                parse_mouse_button_name(raw_value)?,
            )?,
            "count" => assign_once(&mut count, "count", "@click", parse_click_count(raw_value)?)?,
            "hold_ms" => assign_once(
                &mut hold_ms,
                "hold_ms",
                "@click",
                parse_u64_field("hold_ms", raw_value)?,
            )?,
            "interval_ms" => assign_once(
                &mut interval_ms,
                "interval_ms",
                "@click",
                parse_u64_field("interval_ms", raw_value)?,
            )?,
            "coordinate_space" => assign_once(
                &mut coordinate_space,
                "coordinate_space",
                "@click",
                parse_mouse_coordinate_space(raw_value, false)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@click 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(ClickRequest {
        x: required_field(x, "@click", "x")?,
        y: required_field(y, "@click", "y")?,
        button: button.unwrap_or(MouseButtonName::Left),
        count: count.unwrap_or(1),
        hold_ms: hold_ms.unwrap_or(DEFAULT_MOUSE_CLICK_HOLD_MS),
        interval_ms: interval_ms.unwrap_or(DEFAULT_MOUSE_CLICK_INTERVAL_MS),
        coordinate_space: coordinate_space.unwrap_or(MouseCoordinateSpace::OsLogical),
    })
}

pub fn parse_drag_payload(input: &str) -> io::Result<DragRequest> {
    let inner = object_inner(input, "@drag")?;
    if inner.is_empty() {
        return Err(invalid_data("@drag 对象 payload 不能为空"));
    }

    let mut from = None::<MousePoint>;
    let mut to = None::<MousePoint>;
    let mut button = None::<MouseButtonName>;
    let mut duration_ms = None::<u64>;
    let mut steps = None::<u16>;
    let mut coordinate_space = None::<MouseCoordinateSpace>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "from" => assign_once(&mut from, "from", "@drag", parse_mouse_point(raw_value)?)?,
            "to" => assign_once(&mut to, "to", "@drag", parse_mouse_point(raw_value)?)?,
            "button" => assign_once(
                &mut button,
                "button",
                "@drag",
                parse_mouse_button_name(raw_value)?,
            )?,
            "duration_ms" => assign_once(
                &mut duration_ms,
                "duration_ms",
                "@drag",
                parse_u64_field("duration_ms", raw_value)?,
            )?,
            "steps" => assign_once(&mut steps, "steps", "@drag", parse_drag_steps(raw_value)?)?,
            "coordinate_space" => assign_once(
                &mut coordinate_space,
                "coordinate_space",
                "@drag",
                parse_mouse_coordinate_space(raw_value, false)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@drag 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(DragRequest {
        from: required_field(from, "@drag", "from")?,
        to: required_field(to, "@drag", "to")?,
        button: button.unwrap_or(MouseButtonName::Left),
        duration_ms: duration_ms.unwrap_or(DEFAULT_MOUSE_DRAG_DURATION_MS),
        steps: steps.unwrap_or(DEFAULT_MOUSE_DRAG_STEPS),
        coordinate_space: coordinate_space.unwrap_or(MouseCoordinateSpace::OsLogical),
    })
}

pub fn parse_wheel_payload(input: &str) -> io::Result<WheelRequest> {
    let inner = object_inner(input, "@wheel")?;
    if inner.is_empty() {
        return Err(invalid_data("@wheel 对象 payload 不能为空"));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;
    let mut delta_x = None::<i32>;
    let mut delta_y = None::<i32>;
    let mut coordinate_space = None::<MouseCoordinateSpace>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", "@wheel", parse_i32_field("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", "@wheel", parse_i32_field("y", raw_value)?)?,
            "delta_x" => assign_once(
                &mut delta_x,
                "delta_x",
                "@wheel",
                parse_i32_field("delta_x", raw_value)?,
            )?,
            "delta_y" => assign_once(
                &mut delta_y,
                "delta_y",
                "@wheel",
                parse_i32_field("delta_y", raw_value)?,
            )?,
            "coordinate_space" => assign_once(
                &mut coordinate_space,
                "coordinate_space",
                "@wheel",
                parse_mouse_coordinate_space(raw_value, false)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@wheel 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let request = WheelRequest {
        x,
        y,
        delta_x: delta_x.unwrap_or(0),
        delta_y: delta_y.unwrap_or(0),
        coordinate_space: coordinate_space.unwrap_or(MouseCoordinateSpace::OsLogical),
    };
    validate_wheel_shape(&request, io::ErrorKind::InvalidData)?;
    Ok(request)
}

pub fn build_mouse_move_plan(request: &MouseMoveRequest) -> io::Result<MouseExecutionPlan> {
    validate_mouse_move_shape(request, io::ErrorKind::InvalidInput)?;
    let (x, y, coordinate) = match request.coordinate_space {
        MouseCoordinateSpace::OsLogical => {
            let x = request.x.expect("validated x");
            let y = request.y.expect("validated y");
            ensure_supported_absolute_coordinate(x, y)?;
            (x, y, Coordinate::Abs)
        }
        MouseCoordinateSpace::Relative => (
            request.dx.expect("validated dx"),
            request.dy.expect("validated dy"),
            Coordinate::Rel,
        ),
    };

    Ok(MouseExecutionPlan {
        steps: vec![MousePlanStep::Move { x, y, coordinate }],
        report: base_report(MouseAction::Move, Some(request.coordinate_space), "enigo")
            .with_point_fields(request.x, request.y, request.dx, request.dy),
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
        report: base_report(MouseAction::Button, None, "enigo").with_button_fields(request),
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
    ensure_supported_absolute_coordinate(request.x, request.y)?;

    let button = to_enigo_button(request.button);
    let mut steps = vec![MousePlanStep::Move {
        x: request.x,
        y: request.y,
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
        report: base_report(MouseAction::Click, Some(request.coordinate_space), "enigo")
            .with_click_fields(request),
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
    ensure_supported_absolute_coordinate(request.from.x, request.from.y)?;
    ensure_supported_absolute_coordinate(request.to.x, request.to.y)?;

    let button = to_enigo_button(request.button);
    let mut steps = vec![
        MousePlanStep::Move {
            x: request.from.x,
            y: request.from.y,
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
        let point = interpolate_point(request.from, request.to, step_index, request.steps);
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
        report: base_report(MouseAction::Drag, Some(request.coordinate_space), "enigo")
            .with_drag_fields(request),
    })
}

pub fn build_wheel_plan(request: &WheelRequest) -> io::Result<MouseExecutionPlan> {
    validate_wheel_shape(request, io::ErrorKind::InvalidInput)?;

    let mut steps = Vec::new();
    if let (Some(x), Some(y)) = (request.x, request.y) {
        ensure_supported_absolute_coordinate(x, y)?;
        steps.push(MousePlanStep::Move {
            x,
            y,
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
            (request.x.is_some() || request.y.is_some()).then_some(request.coordinate_space),
            "enigo",
        )
        .with_wheel_fields(request, wheel_order),
    })
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

fn parse_mouse_point(input: &str) -> io::Result<MousePoint> {
    let inner = object_inner(input, "mouse point")?;
    if inner.is_empty() {
        return Err(invalid_data("mouse point 对象 payload 不能为空"));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", "mouse point", parse_i32_field("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", "mouse point", parse_i32_field("y", raw_value)?)?,
            _ => {
                return Err(invalid_data(format!(
                    "mouse point 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(MousePoint {
        x: required_field(x, "mouse point", "x")?,
        y: required_field(y, "mouse point", "y")?,
    })
}

fn parse_mouse_button_name(input: &str) -> io::Result<MouseButtonName> {
    let button = parse_quoted_payload(input)?;
    match button.to_ascii_lowercase().as_str() {
        "left" => Ok(MouseButtonName::Left),
        "right" => Ok(MouseButtonName::Right),
        "middle" => Ok(MouseButtonName::Middle),
        "back" => Ok(MouseButtonName::Back),
        "forward" => Ok(MouseButtonName::Forward),
        _ => Err(invalid_data(format!("不支持的鼠标按钮: {button}"))),
    }
}

fn parse_mouse_button_mode(input: &str) -> io::Result<MouseButtonMode> {
    let mode = parse_quoted_payload(input)?;
    match mode.to_ascii_lowercase().as_str() {
        "press" => Ok(MouseButtonMode::Press),
        "release" => Ok(MouseButtonMode::Release),
        "click" => Ok(MouseButtonMode::Click),
        _ => Err(invalid_data(format!(
            "@mouse-button 的 `mode` 不支持该值: {mode}"
        ))),
    }
}

fn parse_mouse_coordinate_space(
    input: &str,
    allow_relative: bool,
) -> io::Result<MouseCoordinateSpace> {
    let coordinate_space = parse_quoted_payload(input)?;
    match coordinate_space.to_ascii_lowercase().as_str() {
        "os-logical" => Ok(MouseCoordinateSpace::OsLogical),
        "relative" if allow_relative => Ok(MouseCoordinateSpace::Relative),
        "relative" => Err(invalid_data("当前命令不支持 coordinate_space=\"relative\"")),
        _ => Err(invalid_data(format!(
            "当前只支持 coordinate_space=\"os-logical\": {coordinate_space}"
        ))),
    }
}

fn parse_i32_field(field_name: &str, input: &str) -> io::Result<i32> {
    input
        .parse::<i32>()
        .map_err(|_| invalid_data(format!("`{field_name}` 必须是 32 位整数: {input}")))
}

fn parse_u64_field(field_name: &str, input: &str) -> io::Result<u64> {
    input
        .parse::<u64>()
        .map_err(|_| invalid_data(format!("`{field_name}` 必须是无符号整数: {input}")))
}

fn parse_click_count(input: &str) -> io::Result<u8> {
    let count = input
        .parse::<u8>()
        .map_err(|_| invalid_data(format!("@click 的 `count` 必须是无符号整数: {input}")))?;

    if !(1..=MAX_MOUSE_CLICK_COUNT).contains(&count) {
        return Err(invalid_data(format!(
            "@click 的 `count` 必须在 1..={MAX_MOUSE_CLICK_COUNT} 之间"
        )));
    }

    Ok(count)
}

fn parse_drag_steps(input: &str) -> io::Result<u16> {
    let steps = input
        .parse::<u16>()
        .map_err(|_| invalid_data(format!("@drag 的 `steps` 必须是无符号整数: {input}")))?;

    if steps == 0 || steps > MAX_MOUSE_DRAG_STEPS {
        return Err(invalid_data(format!(
            "@drag 的 `steps` 必须在 1..={MAX_MOUSE_DRAG_STEPS} 之间"
        )));
    }

    Ok(steps)
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, kind: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!(
            "{kind} 对象 payload 的 `{field_name}` 字段重复"
        )));
    }
    *slot = Some(value);
    Ok(())
}

fn required_field<T>(value: Option<T>, kind: &str, field_name: &str) -> io::Result<T> {
    value.ok_or_else(|| invalid_data(format!("{kind} 对象 payload 缺少必填字段 `{field_name}`")))
}

fn validate_mouse_move_shape(request: &MouseMoveRequest, kind: io::ErrorKind) -> io::Result<()> {
    match request.coordinate_space {
        MouseCoordinateSpace::OsLogical => {
            if request.x.is_none() || request.y.is_none() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move coordinate_space=\"os-logical\" 必须包含 `x` 和 `y`",
                ));
            }
            if request.dx.is_some() || request.dy.is_some() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move 不能同时包含绝对坐标和相对坐标",
                ));
            }
        }
        MouseCoordinateSpace::Relative => {
            if request.dx.is_none() || request.dy.is_none() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move coordinate_space=\"relative\" 必须包含 `dx` 和 `dy`",
                ));
            }
            if request.x.is_some() || request.y.is_some() {
                return Err(io::Error::new(
                    kind,
                    "@mouse-move 不能同时包含绝对坐标和相对坐标",
                ));
            }
        }
    }
    Ok(())
}

fn validate_wheel_shape(request: &WheelRequest, kind: io::ErrorKind) -> io::Result<()> {
    if request.delta_x == 0 && request.delta_y == 0 {
        return Err(io::Error::new(
            kind,
            "@wheel 的 `delta_x` 和 `delta_y` 至少要有一个非 0",
        ));
    }
    match (request.x, request.y) {
        (Some(_), Some(_)) => {
            if request.coordinate_space != MouseCoordinateSpace::OsLogical {
                return Err(io::Error::new(
                    kind,
                    "@wheel 提供 x/y 时只支持 coordinate_space=\"os-logical\"",
                ));
            }
            Ok(())
        }
        (None, None) => Ok(()),
        _ => Err(io::Error::new(kind, "@wheel 的 `x` 和 `y` 必须同时提供")),
    }
}

fn base_report(
    action: MouseAction,
    coordinate_space: Option<MouseCoordinateSpace>,
    backend: &'static str,
) -> MouseExecutionReport {
    MouseExecutionReport {
        action,
        coordinate_space,
        backend,
        x: None,
        y: None,
        dx: None,
        dy: None,
        from: None,
        to: None,
        button: None,
        mode: None,
        count: None,
        hold_ms: None,
        interval_ms: None,
        duration_ms: None,
        steps: None,
        delta_x: None,
        delta_y: None,
        wheel_order: Vec::new(),
        released: None,
        release_recovery: MouseReleaseRecovery::NotNeeded,
    }
}

impl MouseExecutionReport {
    fn with_point_fields(
        mut self,
        x: Option<i32>,
        y: Option<i32>,
        dx: Option<i32>,
        dy: Option<i32>,
    ) -> Self {
        self.x = x;
        self.y = y;
        self.dx = dx;
        self.dy = dy;
        self
    }

    fn with_button_fields(mut self, request: &MouseButtonRequest) -> Self {
        self.button = Some(request.button);
        self.mode = Some(request.mode);
        self.hold_ms = Some(request.hold_ms);
        self.released = (request.mode == MouseButtonMode::Click).then_some(true);
        self
    }

    fn with_click_fields(mut self, request: &ClickRequest) -> Self {
        self.x = Some(request.x);
        self.y = Some(request.y);
        self.button = Some(request.button);
        self.count = Some(request.count);
        self.hold_ms = Some(request.hold_ms);
        self.interval_ms = Some(request.interval_ms);
        self.released = Some(true);
        self
    }

    fn with_drag_fields(mut self, request: &DragRequest) -> Self {
        self.from = Some(request.from);
        self.to = Some(request.to);
        self.button = Some(request.button);
        self.duration_ms = Some(request.duration_ms);
        self.steps = Some(request.steps);
        self.released = Some(true);
        self
    }

    fn with_wheel_fields(mut self, request: &WheelRequest, wheel_order: Vec<&'static str>) -> Self {
        self.x = request.x;
        self.y = request.y;
        self.delta_x = Some(request.delta_x);
        self.delta_y = Some(request.delta_y);
        self.wheel_order = wheel_order;
        self
    }
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

fn interpolate_point(from: MousePoint, to: MousePoint, step_index: u16, steps: u16) -> MousePoint {
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            x: 10,
            y: 20,
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
            from: MousePoint { x: 0, y: 0 },
            to: MousePoint { x: 10, y: 10 },
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
            from: MousePoint { x: 0, y: 0 },
            to: MousePoint { x: 10, y: 10 },
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
            from: MousePoint { x: 0, y: 0 },
            to: MousePoint { x: 10, y: 10 },
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
            coordinate_space: MouseCoordinateSpace::OsLogical,
        })
        .unwrap();

        let report_json = plan.report.to_value_json();

        assert!(report_json.contains(r#""kind":"mouse""#));
        assert!(report_json.contains(r#""action":"move""#));
        assert!(report_json.contains(r#""coordinate_space":"os-logical""#));
        assert!(report_json.contains(r#""x":10"#));
    }
}
