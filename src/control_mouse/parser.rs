use super::request::{
    validate_mouse_move_shape, validate_wheel_shape, ClickRequest, DragRequest, MouseAnchor,
    MouseButtonMode, MouseButtonName, MouseButtonRequest, MouseCoordinateSpace, MouseEndpoint,
    MouseMoveRequest, MousePoint, MouseRefTarget, MouseSelectorTarget, WheelRequest,
    DEFAULT_MOUSE_CLICK_HOLD_MS, DEFAULT_MOUSE_CLICK_INTERVAL_MS, DEFAULT_MOUSE_DRAG_DURATION_MS,
    DEFAULT_MOUSE_DRAG_STEPS, MAX_MOUSE_CLICK_COUNT, MAX_MOUSE_DRAG_STEPS,
};
use crate::control_observation::SelectorRefindPolicy;
use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use std::io;

pub fn parse_mouse_move_payload(input: &str) -> io::Result<MouseMoveRequest> {
    let inner = object_inner(input, "@mouse-move")?;
    if inner.is_empty() {
        return Err(invalid_data("@mouse-move 对象 payload 不能为空"));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;
    let mut dx = None::<i32>;
    let mut dy = None::<i32>;
    let mut target = None::<MouseEndpoint>;
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
            "target" => assign_once(
                &mut target,
                "target",
                "@mouse-move",
                parse_mouse_endpoint(raw_value, "@mouse-move.target")?,
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
        target,
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
    let mut target = None::<MouseEndpoint>;
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
            "target" => assign_once(
                &mut target,
                "target",
                "@click",
                parse_mouse_endpoint(raw_value, "@click.target")?,
            )?,
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

    validate_click_target_shape(x, y, target.as_ref())?;
    Ok(ClickRequest {
        x,
        y,
        target,
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

    let mut from = None::<MouseEndpoint>;
    let mut to = None::<MouseEndpoint>;
    let mut button = None::<MouseButtonName>;
    let mut duration_ms = None::<u64>;
    let mut steps = None::<u16>;
    let mut coordinate_space = None::<MouseCoordinateSpace>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "from" => assign_once(
                &mut from,
                "from",
                "@drag",
                parse_mouse_endpoint(raw_value, "@drag.from")?,
            )?,
            "to" => assign_once(
                &mut to,
                "to",
                "@drag",
                parse_mouse_endpoint(raw_value, "@drag.to")?,
            )?,
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
    let mut target = None::<MouseEndpoint>;
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
            "target" => assign_once(
                &mut target,
                "target",
                "@wheel",
                parse_mouse_endpoint(raw_value, "@wheel.target")?,
            )?,
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
        target,
        delta_x: delta_x.unwrap_or(0),
        delta_y: delta_y.unwrap_or(0),
        coordinate_space: coordinate_space.unwrap_or(MouseCoordinateSpace::OsLogical),
    };
    validate_wheel_shape(&request, io::ErrorKind::InvalidData)?;
    Ok(request)
}

fn parse_mouse_endpoint(input: &str, kind: &str) -> io::Result<MouseEndpoint> {
    let inner = object_inner(input, kind)?;
    if inner.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }

    let mut x = None::<i32>;
    let mut y = None::<i32>;
    let mut ref_id = None::<String>;
    let mut observation_id = None::<String>;
    let mut selector_id = None::<String>;
    let mut direct_id = None::<String>;
    let mut auto_refind = None::<bool>;
    let mut policy = None::<SelectorRefindPolicy>;
    let mut min_confidence_milli = None::<u16>;
    let mut anchor = None::<MouseAnchor>;
    let mut offset_dx = None::<i32>;
    let mut offset_dy = None::<i32>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "x" => assign_once(&mut x, "x", kind, parse_i32_field("x", raw_value)?)?,
            "y" => assign_once(&mut y, "y", kind, parse_i32_field("y", raw_value)?)?,
            "ref" | "ref_id" => assign_once(
                &mut ref_id,
                "ref",
                kind,
                parse_non_empty_string(&format!("{kind}.ref"), raw_value)?,
            )?,
            "observation_id" => assign_once(
                &mut observation_id,
                "observation_id",
                kind,
                parse_non_empty_string(&format!("{kind}.observation_id"), raw_value)?,
            )?,
            "selector_id" => assign_once(
                &mut selector_id,
                "selector_id",
                kind,
                parse_non_empty_string(&format!("{kind}.selector_id"), raw_value)?,
            )?,
            "id" => assign_once(
                &mut direct_id,
                "id",
                kind,
                parse_non_empty_string(&format!("{kind}.id"), raw_value)?,
            )?,
            "auto_refind" => assign_once(
                &mut auto_refind,
                "auto_refind",
                kind,
                parse_bool_field("auto_refind", raw_value)?,
            )?,
            "policy" => assign_once(
                &mut policy,
                "policy",
                kind,
                parse_selector_refind_policy(raw_value)?,
            )?,
            "min_confidence" => assign_once(
                &mut min_confidence_milli,
                "min_confidence",
                kind,
                parse_min_confidence_milli(raw_value)?,
            )?,
            "anchor" => assign_once(&mut anchor, "anchor", kind, parse_mouse_anchor(raw_value)?)?,
            "offset_dx" => assign_once(
                &mut offset_dx,
                "offset_dx",
                kind,
                parse_i32_field("offset_dx", raw_value)?,
            )?,
            "offset_dy" => assign_once(
                &mut offset_dy,
                "offset_dy",
                kind,
                parse_i32_field("offset_dy", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "{kind} 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let anchor = anchor_from_parts(kind, anchor, offset_dx, offset_dy)?;
    let has_coordinate = x.is_some() || y.is_some();
    let has_ref = ref_id.is_some();
    let has_selector = selector_id.is_some();
    let has_id = direct_id.is_some();
    let endpoint_kind_count = usize::from(has_coordinate)
        + usize::from(has_ref)
        + usize::from(has_selector)
        + usize::from(has_id);
    if endpoint_kind_count != 1 {
        return Err(invalid_data(format!(
            "{kind} 必须且只能包含 x/y、ref、selector_id、id 中的一类 target"
        )));
    }

    if has_coordinate {
        if auto_refind.is_some()
            || policy.is_some()
            || min_confidence_milli.is_some()
            || observation_id.is_some()
        {
            return Err(invalid_data(format!(
                "{kind} coordinate target 不能携带 ref/selector 字段"
            )));
        }
        return Ok(MouseEndpoint::Coordinate(MousePoint {
            x: required_field(x, kind, "x")?,
            y: required_field(y, kind, "y")?,
        }));
    }

    if has_id {
        return Err(invalid_data(format!(
            "{kind}.id 当前不能作为 mouse target,请使用 ref + observation_id 或显式坐标"
        )));
    }

    if has_ref {
        if selector_id.is_some()
            || auto_refind.is_some()
            || policy.is_some()
            || min_confidence_milli.is_some()
        {
            return Err(invalid_data(format!("{kind}.ref 不能与 selector 字段混用")));
        }
        return Ok(MouseEndpoint::ObservationRef(MouseRefTarget {
            observation_id: required_field(observation_id, kind, "observation_id")?,
            ref_id: required_field(ref_id, kind, "ref")?,
            anchor,
        }));
    }

    if observation_id.is_some() {
        return Err(invalid_data(format!(
            "{kind}.selector_id 不能携带 observation_id"
        )));
    }
    Ok(MouseEndpoint::Selector(MouseSelectorTarget {
        selector_id: required_field(selector_id, kind, "selector_id")?,
        auto_refind: auto_refind.unwrap_or(false),
        policy: policy.unwrap_or(SelectorRefindPolicy::Safe),
        min_confidence_milli: min_confidence_milli.unwrap_or(900),
        anchor,
    }))
}

fn validate_click_target_shape(
    x: Option<i32>,
    y: Option<i32>,
    target: Option<&MouseEndpoint>,
) -> io::Result<()> {
    if target.is_some() {
        if x.is_some() || y.is_some() {
            return Err(invalid_data("@click 的 `target` 不能与 x/y 混用"));
        }
        return Ok(());
    }
    required_field(x, "@click", "x")?;
    required_field(y, "@click", "y")?;
    Ok(())
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

fn parse_mouse_anchor(input: &str) -> io::Result<MouseAnchor> {
    let anchor = parse_quoted_payload(input)?;
    match anchor.to_ascii_lowercase().as_str() {
        "center" => Ok(MouseAnchor::Center),
        "top_left" | "top-left" => Ok(MouseAnchor::TopLeft),
        "top_right" | "top-right" => Ok(MouseAnchor::TopRight),
        "bottom_left" | "bottom-left" => Ok(MouseAnchor::BottomLeft),
        "bottom_right" | "bottom-right" => Ok(MouseAnchor::BottomRight),
        _ => Err(invalid_data(format!("不支持的 mouse anchor: {anchor}"))),
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

fn parse_bool_field(field_name: &str, input: &str) -> io::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_data(format!(
            "`{field_name}` 必须是 true 或 false: {input}"
        ))),
    }
}

fn parse_non_empty_string(kind: &str, input: &str) -> io::Result<String> {
    let value = parse_quoted_payload(input)?;
    if value.is_empty() {
        return Err(invalid_data(format!("{kind} 不能为空")));
    }
    Ok(value)
}

fn parse_selector_refind_policy(input: &str) -> io::Result<SelectorRefindPolicy> {
    let policy = parse_quoted_payload(input)?;
    match policy.to_ascii_lowercase().as_str() {
        "safe" => Ok(SelectorRefindPolicy::Safe),
        "manual" => Ok(SelectorRefindPolicy::Manual),
        _ => Err(invalid_data(format!("selector policy 不支持: {policy}"))),
    }
}

fn parse_min_confidence_milli(input: &str) -> io::Result<u16> {
    let ratio = input.parse::<f64>().map_err(|_| {
        invalid_data(format!(
            "min_confidence 必须是 0.0 到 1.0 之间的数字: {input}"
        ))
    })?;
    if !(0.0..=1.0).contains(&ratio) {
        return Err(invalid_data("min_confidence 必须在 0.0 到 1.0 之间"));
    }
    Ok((ratio * 1000.0).round() as u16)
}

fn anchor_from_parts(
    kind: &str,
    anchor: Option<MouseAnchor>,
    offset_dx: Option<i32>,
    offset_dy: Option<i32>,
) -> io::Result<MouseAnchor> {
    match (offset_dx, offset_dy) {
        (Some(dx), Some(dy)) => Ok(MouseAnchor::Offset { dx, dy }),
        (None, None) => Ok(anchor.unwrap_or(MouseAnchor::Center)),
        _ => Err(invalid_data(format!(
            "{kind} 的 offset_dx 和 offset_dy 必须同时提供"
        ))),
    }
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
