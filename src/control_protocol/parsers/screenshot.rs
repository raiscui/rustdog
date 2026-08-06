use std::io;

use super::{
    normalize_object_field_name, parse_quoted_payload, split_object_field, split_object_fields,
};
use crate::control_ax::{parse_ax_mode_payload, AxMode};
use crate::control_protocol::{
    ScreenshotCoordinateSpace, ScreenshotDisplaySelector, ScreenshotLayout, ScreenshotRequest,
    ScreenshotTarget, ScreenshotWindowTarget, DEFAULT_SCREENSHOT_QUALITY,
};

pub(crate) fn parse_screenshot_payload(input: &str) -> io::Result<ScreenshotRequest> {
    let trimmed = input.trim();

    if !trimmed.starts_with('{') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot payload 当前必须是对象: {input}"),
        ));
    }

    let inner = trimmed
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@screenshot 对象 payload 必须使用大括号包裹: {input}"),
            )
        })?
        .trim();

    if inner.is_empty() {
        return Ok(ScreenshotRequest::default());
    }

    let mut target = None::<ScreenshotTarget>;
    let mut window = None::<ScreenshotWindowTarget>;
    let mut display = None::<ScreenshotDisplaySelector>;
    let mut layout = None::<ScreenshotLayout>;
    let mut coordinate_space = None::<ScreenshotCoordinateSpace>;
    let mut quality = None::<u8>;
    let mut include_ax = None::<bool>;
    let mut ax_required = None::<bool>;
    let mut ax_mode = None::<AxMode>;
    let mut ax_depth = None::<u8>;
    let mut ax_max_elements = None::<u16>;
    let mut ax_include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "quality" => {
                if quality.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `quality` 字段重复",
                    ));
                }
                quality = Some(parse_screenshot_quality(raw_value)?);
            }
            "target" => {
                if target.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `target` 字段重复",
                    ));
                }
                target = Some(parse_screenshot_target(raw_value)?);
            }
            "window" => {
                if window.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `window` 字段重复",
                    ));
                }
                window = Some(parse_screenshot_window_target(raw_value)?);
            }
            // 兼容模型常见的顶层写法,内部归一化为 window target。
            "window_id" => {
                if window.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 的 window 与 window_id 不能同时出现",
                    ));
                }
                window = Some(ScreenshotWindowTarget {
                    window_id: Some(parse_quoted_payload(raw_value)?),
                    ref_id: None,
                    observation_id: None,
                });
            }
            "format" => {
                let format = parse_quoted_payload(raw_value)?;
                if !format.eq_ignore_ascii_case("jpeg") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@screenshot 当前只支持 format=\"jpeg\": {format}"),
                    ));
                }
            }
            "display" => {
                if display.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `display` 字段重复",
                    ));
                }
                display = Some(parse_screenshot_display(raw_value)?);
            }
            "layout" => {
                if layout.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `layout` 字段重复",
                    ));
                }
                layout = Some(parse_screenshot_layout(raw_value)?);
            }
            "coordinate_space" => {
                if coordinate_space.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `coordinate_space` 字段重复",
                    ));
                }
                coordinate_space = Some(parse_screenshot_coordinate_space(raw_value)?);
            }
            "include_ax" => {
                if include_ax.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `include_ax` 字段重复",
                    ));
                }
                include_ax = Some(parse_bool_field("@screenshot", "include_ax", raw_value)?);
            }
            "ax_required" => {
                if ax_required.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `ax_required` 字段重复",
                    ));
                }
                ax_required = Some(parse_bool_field("@screenshot", "ax_required", raw_value)?);
            }
            "ax_mode" => {
                if ax_mode.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `ax_mode` 字段重复",
                    ));
                }
                ax_mode = Some(parse_ax_mode_payload("@screenshot", raw_value)?);
            }
            "ax_depth" => {
                if ax_depth.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `ax_depth` 字段重复",
                    ));
                }
                ax_depth = Some(parse_screenshot_ax_depth(raw_value)?);
            }
            "ax_max_elements" => {
                if ax_max_elements.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `ax_max_elements` 字段重复",
                    ));
                }
                ax_max_elements = Some(parse_screenshot_ax_max_elements(raw_value)?);
            }
            "ax_include_values" => {
                if ax_include_values.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "@screenshot 对象 payload 的 `ax_include_values` 字段重复",
                    ));
                }
                ax_include_values = Some(parse_bool_field(
                    "@screenshot",
                    "ax_include_values",
                    raw_value,
                )?);
            }
            _ => {
                // 窗口截图的 locator 必须归属到 `window` 对象,避免顶层
                // `ref` / `pid` 与 display screenshot 的字段语义相互混淆。
                if matches!(
                    field_name.as_str(),
                    "ref" | "ref_id" | "observe" | "app" | "pid" | "process" | "title"
                ) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "@screenshot 的 `{field_name}` 只能写在 window 对象中;例如 target:\"window\",window:{{window_id:\"pid:123/window:0\"}}"
                        ),
                    ));
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@screenshot 对象 payload 包含未知字段: {field_name}"),
                ));
            }
        }
    }

    let target = target.unwrap_or(if window.is_some() {
        ScreenshotTarget::Window
    } else {
        ScreenshotTarget::Display
    });
    match target {
        ScreenshotTarget::Display if window.is_some() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@screenshot target=display 不能携带 window",
            ));
        }
        ScreenshotTarget::Window if window.is_none() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@screenshot target=window 需要 window:{window_id:\"...\"} 或 ref + observation_id",
            ));
        }
        _ => {}
    }
    if matches!(target, ScreenshotTarget::Window) && (display.is_some() || layout.is_some()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot target=window 不接受 display 或 layout;窗口范围由 window target 决定",
        ));
    }
    let display = display.unwrap_or(ScreenshotDisplaySelector::All);
    let layout = layout.unwrap_or(match display {
        ScreenshotDisplaySelector::All => ScreenshotLayout::Composite,
        ScreenshotDisplaySelector::Primary => ScreenshotLayout::Single,
    });
    let coordinate_space = coordinate_space.unwrap_or(ScreenshotCoordinateSpace::OsLogical);

    validate_screenshot_layout(display, layout)?;
    let ax_mode = ax_mode.unwrap_or(AxMode::Full);
    let ax_preset = ax_mode.preset();

    Ok(ScreenshotRequest {
        target,
        window,
        display,
        layout,
        coordinate_space,
        quality: quality.unwrap_or(DEFAULT_SCREENSHOT_QUALITY),
        include_ax: include_ax.unwrap_or(false),
        ax_required: ax_required.unwrap_or(false),
        ax_mode,
        ax_depth: ax_depth.unwrap_or(ax_preset.depth),
        ax_max_elements: ax_max_elements.unwrap_or(ax_preset.max_elements),
        ax_include_values: ax_include_values.unwrap_or(ax_preset.include_values),
    })
}

fn parse_screenshot_target(input: &str) -> io::Result<ScreenshotTarget> {
    let target = parse_quoted_payload(input)?;
    if target.eq_ignore_ascii_case("display") {
        return Ok(ScreenshotTarget::Display);
    }
    if target.eq_ignore_ascii_case("window") {
        return Ok(ScreenshotTarget::Window);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@screenshot target 只支持 \"display\" 或 \"window\": {target}"),
    ))
}

fn parse_screenshot_window_target(input: &str) -> io::Result<ScreenshotWindowTarget> {
    let inner = input
        .trim()
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "@screenshot.window 必须使用对象,例如 {window_id:\"pid:123/window:0\"}",
            )
        })?
        .trim();
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot.window 不能为空",
        ));
    }

    let mut window_id = None::<String>;
    let mut ref_id = None::<String>;
    let mut observation_id = None::<String>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "window_id" => assign_once_window_field(
                &mut window_id,
                "window_id",
                parse_quoted_payload(raw_value)?,
            )?,
            "ref" | "ref_id" => {
                assign_once_window_field(&mut ref_id, "ref", parse_quoted_payload(raw_value)?)?
            }
            "observation_id" => assign_once_window_field(
                &mut observation_id,
                "observation_id",
                parse_quoted_payload(raw_value)?,
            )?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@screenshot.window 包含未知字段: {field_name}"),
                ))
            }
        }
    }

    let has_window_id = window_id.is_some();
    let has_ref = ref_id.is_some();
    let has_observation_id = observation_id.is_some();
    if has_window_id && (has_ref || has_observation_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot.window 的 window_id 不能与 ref / observation_id 混用",
        ));
    }
    if !has_window_id && (!has_ref || !has_observation_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot.window 需要 window_id 或 ref + observation_id",
        ));
    }

    Ok(ScreenshotWindowTarget {
        window_id,
        ref_id,
        observation_id,
    })
}

fn assign_once_window_field(
    slot: &mut Option<String>,
    name: &str,
    value: String,
) -> io::Result<()> {
    if slot.replace(value).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot.window 的 `{name}` 字段重复"),
        ));
    }
    Ok(())
}

fn parse_screenshot_display(input: &str) -> io::Result<ScreenshotDisplaySelector> {
    let display = parse_quoted_payload(input)?;
    if display.eq_ignore_ascii_case("all") {
        return Ok(ScreenshotDisplaySelector::All);
    }
    if display.eq_ignore_ascii_case("primary") {
        return Ok(ScreenshotDisplaySelector::Primary);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@screenshot 当前只支持 display=\"all\" 或 display=\"primary\": {display}"),
    ))
}

fn parse_screenshot_layout(input: &str) -> io::Result<ScreenshotLayout> {
    let layout = parse_quoted_payload(input)?;
    if layout.eq_ignore_ascii_case("composite") {
        return Ok(ScreenshotLayout::Composite);
    }
    if layout.eq_ignore_ascii_case("single") {
        return Ok(ScreenshotLayout::Single);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@screenshot 当前只支持 layout=\"composite\" 或 layout=\"single\": {layout}"),
    ))
}

fn parse_screenshot_coordinate_space(input: &str) -> io::Result<ScreenshotCoordinateSpace> {
    let coordinate_space = parse_quoted_payload(input)?;
    if coordinate_space.eq_ignore_ascii_case("os-logical") {
        return Ok(ScreenshotCoordinateSpace::OsLogical);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@screenshot 当前只支持 coordinate_space=\"os-logical\": {coordinate_space}"),
    ))
}

fn validate_screenshot_layout(
    display: ScreenshotDisplaySelector,
    layout: ScreenshotLayout,
) -> io::Result<()> {
    match (display, layout) {
        (ScreenshotDisplaySelector::All, ScreenshotLayout::Composite)
        | (ScreenshotDisplaySelector::Primary, ScreenshotLayout::Single) => Ok(()),
        (ScreenshotDisplaySelector::All, ScreenshotLayout::Single) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot display=\"all\" 必须使用 layout=\"composite\"",
        )),
        (ScreenshotDisplaySelector::Primary, ScreenshotLayout::Composite) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot display=\"primary\" 必须使用 layout=\"single\"",
        )),
    }
}

fn parse_screenshot_quality(input: &str) -> io::Result<u8> {
    let quality = input.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot 的 `quality` 必须是无符号整数: {input}"),
        )
    })?;

    if !(1..=100).contains(&quality) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot 的 `quality` 必须在 1..=100 之间",
        ));
    }

    Ok(quality)
}

fn parse_screenshot_ax_depth(input: &str) -> io::Result<u8> {
    let depth = input.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot 的 `ax_depth` 必须是无符号整数: {input}"),
        )
    })?;

    if depth == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot 的 `ax_depth` 必须大于 0",
        ));
    }

    Ok(depth)
}

fn parse_screenshot_ax_max_elements(input: &str) -> io::Result<u16> {
    let max_elements = input.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@screenshot 的 `ax_max_elements` 必须是无符号整数: {input}"),
        )
    })?;

    if max_elements == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@screenshot 的 `ax_max_elements` 必须大于 0",
        ));
    }

    Ok(max_elements)
}

fn parse_bool_field(kind: &str, field_name: &str, input: &str) -> io::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 的 `{field_name}` 必须是 true 或 false: {input}"),
        )),
    }
}
