//! `@ax-*` / `@type-text` compact 语法 verb parser。
//!
//! 把紧凑语法字符串转成对应的 request 类型。本文件只装 8 个 `parse_*_payload`
//! 入口函数。底层 helper (parse_ax_target / parse_ax_mode_payload /
//! assign_once / key_mode_as_str 等) 仍在 `crate::control_ax` 提供,
//! 因为它们被 `control_ax/query.rs`、`control_observation`、`screenshot/tests`
//! 等多处共享。

use crate::control_ax::{
    assign_once, invalid_data, invalid_input, key_mode_as_str, matches_optional,
    parse_ax_action_name, parse_ax_depth, parse_ax_max_elements, parse_ax_mode_payload,
    parse_ax_scroll_direction, parse_ax_scroll_pages, parse_ax_target, parse_ax_tree_scope,
    parse_ax_value_mode, parse_bool_literal, parse_non_empty_string, parse_type_text_mode,
    reject_duplicate, required_field, to_invalid_input,
};
use crate::control_ax::types::{
    AxActionName, AxActionRequest, AxFocusRequest, AxMode, AxPressRequest, AxPressSequenceRequest,
    AxScrollDirection, AxScrollRequest, AxSetValueRequest, AxTarget, AxTreeRequest, AxTreeScope,
    AxValueSetMode, AxPressPostcondition, TypeTextMode, TypeTextRequest,
};
use crate::control_protocol::{
    normalize_object_field_name, object_inner, parse_compact_atom, parse_compact_ax_button_sequence,
    parse_compact_window_pair, parse_compact_window_selector, parse_quoted_payload,
    split_object_field, split_object_fields, CompactWindowSelector, KeyMode,
};
use crate::control_window::resolve_unique_app_window_id;
use std::io;

// ---- parse_ax_tree_payload (was lines 777-835) ----
pub fn parse_ax_tree_payload(input: &str) -> io::Result<AxTreeRequest> {
    let inner = object_inner(input, "@ax-tree")?;
    if inner.is_empty() {
        return Ok(AxTreeRequest::default());
    }

    let mut scope = None::<AxTreeScope>;
    let mut mode = None::<AxMode>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "scope" => assign_once(
                &mut scope,
                "scope",
                "@ax-tree",
                parse_ax_tree_scope(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-tree",
                parse_ax_mode_payload("@ax-tree", raw_value)?,
            )?,
            "depth" => assign_once(&mut depth, "depth", "@ax-tree", parse_ax_depth(raw_value)?)?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@ax-tree",
                parse_ax_max_elements(raw_value)?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@ax-tree",
                parse_bool_literal("@ax-tree", "include_values", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-tree 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let preset = mode.unwrap_or(AxMode::Full).preset();
    Ok(AxTreeRequest {
        scope: scope.unwrap_or(AxTreeScope::Windows),
        depth: depth.unwrap_or(preset.depth),
        max_elements: max_elements.unwrap_or(preset.max_elements),
        include_values: include_values.unwrap_or(preset.include_values),
    })
}

// ---- parse_ax_press_payload (was lines 837-922) ----
pub fn parse_ax_press_payload(input: &str) -> io::Result<AxPressRequest> {
    let trimmed = input.trim();
    if !trimmed.starts_with('{') {
        let fields = trimmed.split(',').collect::<Vec<_>>();
        let (window_selector, description, postcondition) = match fields.as_slice() {
            [_, _] => {
                let (window_selector, description) =
                    parse_compact_window_pair("@ax-press", trimmed)?;
                (window_selector, description, None)
            }
            [selector, description, role, expected_value, max_attempts] => {
                let window_selector =
                    parse_compact_window_selector("@ax-press", selector)?;
                let description = parse_compact_atom("@ax-press", description)?;
                let role = parse_compact_atom("@ax-press", role)?;
                let expected_value = parse_compact_atom("@ax-press", expected_value)?;
                let max_attempts = parse_compact_atom("@ax-press", max_attempts)?
                    .parse::<usize>()
                    .ok()
                    .filter(|attempts| (1..=3).contains(attempts))
                    .ok_or_else(|| {
                        invalid_data("@ax-press postcondition max_attempts 必须是1到3")
                    })?;
                (
                    window_selector,
                    description,
                    Some(AxPressPostcondition {
                        role,
                        expected_value,
                        max_attempts,
                    }),
                )
            }
            _ => {
                return Err(invalid_data(
                    "@ax-press 短格式必须是selector,description或selector,description,role,expected_value,max_attempts",
                ))
            }
        };
        let mut target = AxTarget {
            role: Some("AXButton".to_owned()),
            description: Some(description),
            ..AxTarget::default()
        };
        match window_selector {
            CompactWindowSelector::WindowId(window_id) => target.window_id = Some(window_id),
            CompactWindowSelector::App(app) => target.app = Some(app),
        }
        target.validate()?;
        return Ok(AxPressRequest {
            target,
            postcondition,
        });
    }

    let inner = object_inner(input, "@ax-press")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-press 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-press",
                parse_ax_target(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-press 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxPressRequest {
        target: required_field(target, "@ax-press", "target")?,
        postcondition: None,
    })
}

// ---- parse_ax_press_sequence_payload (was lines 924-953) ----
pub fn parse_ax_press_sequence_payload(input: &str) -> io::Result<AxPressSequenceRequest> {
    let trimmed = input.trim();
    if trimmed.starts_with('{') {
        return Err(invalid_data(
            "@ax-press-sequence 当前只接受 shell-safe 短格式",
        ));
    }

    let (window_selector, descriptions) =
        parse_compact_ax_button_sequence("@ax-press-sequence", trimmed)?;
    let targets = descriptions
        .into_iter()
        .map(|description| {
            let mut target = AxTarget {
                role: Some("AXButton".to_owned()),
                description: Some(description),
                ..AxTarget::default()
            };
            match &window_selector {
                CompactWindowSelector::WindowId(window_id) => {
                    target.window_id = Some(window_id.clone());
                }
                CompactWindowSelector::App(app) => target.app = Some(app.clone()),
            }
            target.validate()?;
            Ok(target)
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(AxPressSequenceRequest { targets })
}

// ---- parse_ax_action_payload (was lines 955-993) ----
pub fn parse_ax_action_payload(input: &str) -> io::Result<AxActionRequest> {
    let inner = object_inner(input, "@ax-action")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-action 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut action = None::<AxActionName>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-action",
                parse_ax_target(raw_value)?,
            )?,
            "action" => assign_once(
                &mut action,
                "action",
                "@ax-action",
                parse_ax_action_name(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-action 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxActionRequest {
        target: required_field(target, "@ax-action", "target")?,
        action: required_field(action, "@ax-action", "action")?,
    })
}

// ---- parse_ax_set_value_payload (was lines 995-1041) ----
pub fn parse_ax_set_value_payload(input: &str) -> io::Result<AxSetValueRequest> {
    let inner = object_inner(input, "@ax-set-value")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-set-value 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut value = None::<String>;
    let mut mode = None::<AxValueSetMode>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-set-value",
                parse_ax_target(raw_value)?,
            )?,
            "value" => assign_once(
                &mut value,
                "value",
                "@ax-set-value",
                parse_quoted_payload(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@ax-set-value",
                parse_ax_value_mode(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-set-value 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxSetValueRequest {
        target: required_field(target, "@ax-set-value", "target")?,
        value: required_field(value, "@ax-set-value", "value")?,
        mode: mode.unwrap_or(AxValueSetMode::Replace),
    })
}

// ---- parse_ax_focus_payload (was lines 1043-1098) ----
pub fn parse_ax_focus_payload(input: &str) -> io::Result<AxFocusRequest> {
    let inner = object_inner(input, "@ax-focus")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-focus 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut window_id = None::<String>;
    let mut activate = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-focus",
                parse_ax_target(raw_value)?,
            )?,
            "window_id" => assign_once(
                &mut window_id,
                "window_id",
                "@ax-focus",
                parse_non_empty_string("@ax-focus.window_id", raw_value)?,
            )?,
            "activate" => assign_once(
                &mut activate,
                "activate",
                "@ax-focus",
                parse_bool_literal("@ax-focus", "activate", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-focus 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    if target.is_none() && window_id.is_none() {
        return Err(invalid_data("@ax-focus 至少需要 `target` 或 `window_id`"));
    }
    if target.is_some() && window_id.is_some() {
        return Err(invalid_data(
            "@ax-focus 不能同时携带 `target` 和 `window_id`",
        ));
    }

    Ok(AxFocusRequest {
        target,
        window_id,
        activate: activate.unwrap_or(false),
    })
}

// ---- parse_ax_scroll_payload (was lines 1100-1146) ----
pub fn parse_ax_scroll_payload(input: &str) -> io::Result<AxScrollRequest> {
    let inner = object_inner(input, "@ax-scroll")?;
    if inner.is_empty() {
        return Err(invalid_data("@ax-scroll 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut direction = None::<AxScrollDirection>;
    let mut pages = None::<u16>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@ax-scroll",
                parse_ax_target(raw_value)?,
            )?,
            "direction" => assign_once(
                &mut direction,
                "direction",
                "@ax-scroll",
                parse_ax_scroll_direction(raw_value)?,
            )?,
            "pages" => assign_once(
                &mut pages,
                "pages",
                "@ax-scroll",
                parse_ax_scroll_pages(raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@ax-scroll 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    Ok(AxScrollRequest {
        target: required_field(target, "@ax-scroll", "target")?,
        direction: required_field(direction, "@ax-scroll", "direction")?,
        pages: pages.unwrap_or(1),
    })
}

// ---- parse_type_text_payload (was lines 1148-1210) ----
pub fn parse_type_text_payload(input: &str) -> io::Result<TypeTextRequest> {
    let inner = object_inner(input, "@type-text")?;
    if inner.is_empty() {
        return Err(invalid_data("@type-text 对象 payload 不能为空"));
    }

    let mut target = None::<AxTarget>;
    let mut text = None::<String>;
    let mut mode = None::<TypeTextMode>;
    let mut allow_clipboard = None::<bool>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@type-text",
                parse_ax_target(raw_value)?,
            )?,
            "text" => assign_once(
                &mut text,
                "text",
                "@type-text",
                parse_quoted_payload(raw_value)?,
            )?,
            "mode" => assign_once(
                &mut mode,
                "mode",
                "@type-text",
                parse_type_text_mode(raw_value)?,
            )?,
            "allow_clipboard" => assign_once(
                &mut allow_clipboard,
                "allow_clipboard",
                "@type-text",
                parse_bool_literal("@type-text", "allow_clipboard", raw_value)?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@type-text 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let mode = mode.unwrap_or(TypeTextMode::Auto);
    let allow_clipboard = allow_clipboard.unwrap_or(false);
    if matches!(mode, TypeTextMode::Clipboard) && !allow_clipboard {
        return Err(invalid_data(
            "@type-text mode:\"clipboard\" 需要显式 `allow_clipboard:true`",
        ));
    }

    Ok(TypeTextRequest {
        target: required_field(target, "@type-text", "target")?,
        text: required_field(text, "@type-text", "text")?,
        mode,
        allow_clipboard,
    })
}
