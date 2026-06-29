use serde_json::{Map, Number, Value};
use std::{fmt::Write as _, fs, io, path::Path};

/// UI script 的第一阶段内核。
///
/// 这里先只负责三件事:
/// - 读取 JSON array + PascalCase single-key step。
/// - 归一化 iced_emg 风格的 step。
/// - dry-run 编译成 rdog line-control 文本。
///
/// 它暂时不连接 daemon,也不暴露 CLI。这样 fixture tests 可以先把 DSL
/// 契约钉住,后续再接 `rdog ui-script run` 时不会把语法和 transport 混在一起调。
#[derive(Debug, Clone, PartialEq)]
pub struct UiScriptProgram {
    pub steps: Vec<UiScriptStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiScriptStep {
    Dialect(Map<String, Value>),
    Target(Map<String, Value>),
    Policy(Map<String, Value>),
    Scope(Map<String, Value>),
    SleepMs(u64),
    DelayMs(u64),
    Observe(Map<String, Value>),
    Screenshot(Map<String, Value>),
    Move(Map<String, Value>),
    Click(Map<String, Value>),
    MouseDown(Map<String, Value>),
    MouseUp(Map<String, Value>),
    KeyDown(Map<String, Value>),
    KeyUp(Map<String, Value>),
    KeyPress(Map<String, Value>),
    Text(Map<String, Value>),
    Action(Map<String, Value>),
    Barrier(Option<Map<String, Value>>),
    Expect(Map<String, Value>),
    WindowSize(WindowSizeStep),
    ControlLine(String),
    Exit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowSizeStep {
    pub width: f64,
    pub height: f64,
    pub mode: WindowSizeMode,
    pub target: Option<Map<String, Value>>,
    pub origin: Option<Value>,
    pub guard: Option<Map<String, Value>>,
    pub box_model: Option<String>,
    pub verify: Option<Value>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WindowSizeMode {
    Precondition,
    Resize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiScriptDryRun {
    pub steps: Vec<UiScriptDryRunStep>,
    pub control_lines: Vec<String>,
    pub summary: UiScriptRunSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiScriptDryRunStep {
    pub index: usize,
    pub kind: &'static str,
    pub effect: UiScriptDryRunEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiScriptDryRunEffect {
    Context(String),
    Local(String),
    Expect(Map<String, Value>),
    ControlLine(String),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiScriptRunSummary {
    pub step_count: usize,
    pub backend_request_count: usize,
    pub semantic_action_count: usize,
    pub mouse_fallback_count: usize,
}

#[derive(Debug, Clone, Default)]
struct UiScriptCompileContext {
    scope: Option<Map<String, Value>>,
    default_coordinate_space: Option<String>,
}

pub fn parse_script_file(path: &Path) -> io::Result<UiScriptProgram> {
    let content = fs::read_to_string(path).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "读取 UI script 文件失败: path={}, error={err}",
                path.display()
            ),
        )
    })?;
    parse_script_json(&content)
}

pub fn parse_script_json(input: &str) -> io::Result<UiScriptProgram> {
    let value: Value = serde_json::from_str(input).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("UI script JSON 解析失败: {err}"),
        )
    })?;
    let steps = value.as_array().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "UI script 顶层必须是 JSON array",
        )
    })?;

    let mut parsed_steps = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        parsed_steps.push(parse_step(index, step)?);
    }
    Ok(UiScriptProgram {
        steps: parsed_steps,
    })
}

pub fn compile_dry_run(program: &UiScriptProgram) -> io::Result<UiScriptDryRun> {
    let mut context = UiScriptCompileContext::default();
    let mut request_id = 1u64;
    let mut steps = Vec::with_capacity(program.steps.len());
    let mut control_lines = Vec::new();
    let mut semantic_action_count = 0usize;
    let mut mouse_fallback_count = 0usize;

    for (index, step) in program.steps.iter().enumerate() {
        let effect = match step {
            UiScriptStep::Dialect(payload) => {
                if let Some(value) = payload.get("coordinate_space").and_then(Value::as_str) {
                    context.default_coordinate_space = Some(value.to_owned());
                }
                UiScriptDryRunEffect::Context("dialect".to_owned())
            }
            UiScriptStep::Target(_) => UiScriptDryRunEffect::Context("target".to_owned()),
            UiScriptStep::Policy(payload) => {
                if let Some(value) = payload
                    .get("default_coordinate_space")
                    .and_then(Value::as_str)
                {
                    context.default_coordinate_space = Some(value.to_owned());
                }
                UiScriptDryRunEffect::Context("policy".to_owned())
            }
            UiScriptStep::Scope(payload) => {
                context.scope = Some(payload.clone());
                UiScriptDryRunEffect::Context("scope".to_owned())
            }
            UiScriptStep::SleepMs(ms) | UiScriptStep::DelayMs(ms) => {
                UiScriptDryRunEffect::Local(format!("sleep_ms:{ms}"))
            }
            UiScriptStep::Barrier(payload) => {
                let label = if payload.is_some() {
                    "barrier:observe"
                } else {
                    "barrier"
                };
                UiScriptDryRunEffect::Local(label.to_owned())
            }
            UiScriptStep::Expect(payload) => UiScriptDryRunEffect::Expect(payload.clone()),
            UiScriptStep::WindowSize(size) => match size.mode {
                WindowSizeMode::Precondition => UiScriptDryRunEffect::Local(format!(
                    "window_size_precondition:{}x{}",
                    trim_float(size.width),
                    trim_float(size.height)
                )),
                WindowSizeMode::Resize => {
                    let payload = window_resize_payload(size, &context)?;
                    semantic_action_count += 1;
                    push_control_line(
                        "@window-resize",
                        payload,
                        &mut request_id,
                        &mut control_lines,
                    )
                }
            },
            UiScriptStep::Exit => UiScriptDryRunEffect::Exit,
            UiScriptStep::Observe(payload) => {
                let mut payload = payload.clone();
                inject_scope_if_missing(&mut payload, &context);
                push_control_line("@observe", payload, &mut request_id, &mut control_lines)
            }
            UiScriptStep::Screenshot(payload) => {
                let mut payload = payload.clone();
                payload.remove("label");
                if payload.is_empty() {
                    push_control_line_without_payload(
                        "@screenshot",
                        &mut request_id,
                        &mut control_lines,
                    )
                } else {
                    push_control_line("@screenshot", payload, &mut request_id, &mut control_lines)
                }
            }
            UiScriptStep::Move(payload) => {
                let mut payload = normalize_mouse_payload(payload.clone(), &context, true)?;
                payload = normalize_mouse_coordinate_numbers(payload)?;
                mouse_fallback_count += 1;
                push_control_line("@mouse-move", payload, &mut request_id, &mut control_lines)
            }
            UiScriptStep::Click(payload) => {
                let mut payload = normalize_mouse_payload(payload.clone(), &context, true)?;
                payload = normalize_mouse_coordinate_numbers(payload)?;
                normalize_mouse_button_case(&mut payload);
                mouse_fallback_count += 1;
                push_control_line("@click", payload, &mut request_id, &mut control_lines)
            }
            UiScriptStep::MouseDown(payload) => {
                let payload = mouse_button_payload(payload, "press")?;
                mouse_fallback_count += 1;
                push_control_line(
                    "@mouse-button",
                    payload,
                    &mut request_id,
                    &mut control_lines,
                )
            }
            UiScriptStep::MouseUp(payload) => {
                let payload = mouse_button_payload(payload, "release")?;
                mouse_fallback_count += 1;
                push_control_line(
                    "@mouse-button",
                    payload,
                    &mut request_id,
                    &mut control_lines,
                )
            }
            UiScriptStep::KeyDown(payload) => {
                key_payload(payload, "press", &mut request_id, &mut control_lines)?
            }
            UiScriptStep::KeyUp(payload) => {
                key_payload(payload, "release", &mut request_id, &mut control_lines)?
            }
            UiScriptStep::KeyPress(payload) => key_payload(
                payload,
                "press_release",
                &mut request_id,
                &mut control_lines,
            )?,
            UiScriptStep::Text(payload) => {
                if !payload.contains_key("target") {
                    return Err(invalid_data(
                        "Text 首版需要显式 target,避免依赖远端当前焦点",
                    ));
                }
                semantic_action_count += 1;
                push_control_line(
                    "@type-text",
                    payload.clone(),
                    &mut request_id,
                    &mut control_lines,
                )
            }
            UiScriptStep::Action(payload) => {
                semantic_action_count += 1;
                action_payload(payload.clone(), &mut request_id, &mut control_lines)?
            }
            UiScriptStep::ControlLine(line) => {
                validate_control_line_escape(line)?;
                control_lines.push(line.clone());
                UiScriptDryRunEffect::ControlLine(line.clone())
            }
        };
        steps.push(UiScriptDryRunStep {
            index,
            kind: step.kind_name(),
            effect,
        });
    }

    Ok(UiScriptDryRun {
        summary: UiScriptRunSummary {
            step_count: program.steps.len(),
            backend_request_count: control_lines.len(),
            semantic_action_count,
            mouse_fallback_count,
        },
        steps,
        control_lines,
    })
}

fn parse_step(index: usize, value: &Value) -> io::Result<UiScriptStep> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_data(format!("UI script step #{index} 必须是 single-key object")))?;
    if object.len() != 1 {
        return Err(invalid_data(format!(
            "UI script step #{index} 必须只有一个 key,当前有 {} 个",
            object.len()
        )));
    }
    let (kind, payload) = object.iter().next().expect("len checked above");
    match kind.as_str() {
        "Dialect" => Ok(UiScriptStep::Dialect(object_payload(kind, payload)?)),
        "Target" => Ok(UiScriptStep::Target(object_payload(kind, payload)?)),
        "Policy" => Ok(UiScriptStep::Policy(object_payload(kind, payload)?)),
        "Scope" => Ok(UiScriptStep::Scope(object_payload(kind, payload)?)),
        "SleepMs" => Ok(UiScriptStep::SleepMs(u64_payload(kind, payload)?)),
        "DelayMs" => Ok(UiScriptStep::DelayMs(u64_payload(kind, payload)?)),
        "Observe" => Ok(UiScriptStep::Observe(object_payload(kind, payload)?)),
        "Screenshot" => Ok(UiScriptStep::Screenshot(optional_object_payload(
            kind, payload,
        )?)),
        "Move" | "CursorMove" => Ok(UiScriptStep::Move(object_payload(kind, payload)?)),
        "Click" => Ok(UiScriptStep::Click(object_payload(kind, payload)?)),
        "MouseDown" => Ok(UiScriptStep::MouseDown(object_payload(kind, payload)?)),
        "MouseUp" => Ok(UiScriptStep::MouseUp(object_payload(kind, payload)?)),
        "KeyDown" => Ok(UiScriptStep::KeyDown(object_payload(kind, payload)?)),
        "KeyUp" => Ok(UiScriptStep::KeyUp(object_payload(kind, payload)?)),
        "KeyPress" => Ok(UiScriptStep::KeyPress(object_payload(kind, payload)?)),
        "Text" | "TextInput" => Ok(UiScriptStep::Text(object_payload(kind, payload)?)),
        "Action" => Ok(UiScriptStep::Action(object_payload(kind, payload)?)),
        "Barrier" => Ok(UiScriptStep::Barrier(optional_object_payload_or_null(
            kind, payload,
        )?)),
        "Expect" => Ok(UiScriptStep::Expect(object_payload(kind, payload)?)),
        "WindowSize" => parse_window_size(payload).map(UiScriptStep::WindowSize),
        "ControlLine" => payload
            .as_str()
            .map(|line| UiScriptStep::ControlLine(line.to_owned()))
            .ok_or_else(|| invalid_data("ControlLine payload 必须是字符串")),
        "Exit" => {
            if !payload.is_null() {
                return Err(invalid_data("Exit payload 只能是 null"));
            }
            Ok(UiScriptStep::Exit)
        }
        _ => Err(invalid_data(format!("未知 UI script step: {kind}"))),
    }
}

fn parse_window_size(payload: &Value) -> io::Result<WindowSizeStep> {
    let object = object_payload("WindowSize", payload)?;
    let width = required_f64(&object, "WindowSize.width")?;
    let height = required_f64(&object, "WindowSize.height")?;
    let mode = object.get("mode").and_then(Value::as_str).ok_or_else(|| {
        invalid_data("WindowSize 必须声明 mode:\"precondition\" 或 mode:\"resize\"")
    })?;
    let mode = match mode {
        "precondition" => WindowSizeMode::Precondition,
        "resize" => WindowSizeMode::Resize,
        _ => {
            return Err(invalid_data(format!(
                "WindowSize mode 当前只支持 \"precondition\" 或 \"resize\",收到: {mode}"
            )))
        }
    };
    let target = optional_object_field(&object, "target", "WindowSize.target")?;
    let guard = optional_object_field(&object, "guard", "WindowSize.guard")?;
    let origin = object.get("origin").cloned();
    let box_model = object.get("box").and_then(Value::as_str).map(str::to_owned);
    let verify = object.get("verify").cloned();

    if mode == WindowSizeMode::Precondition {
        return Ok(WindowSizeStep {
            width,
            height,
            mode,
            target,
            origin,
            guard,
            box_model,
            verify,
        });
    }

    if target.is_none() {
        return Err(invalid_data(
            "WindowSize mode:\"resize\" 需要 target,用于编译到 @window-resize.target",
        ));
    }
    if let Some(box_model) = box_model.as_deref() {
        if box_model != "outer" {
            return Err(invalid_data(format!(
                "WindowSize mode:\"resize\" 第一版只支持 box:\"outer\",收到: {box_model}"
            )));
        }
    }
    if let Some(origin) = origin.as_ref() {
        validate_window_resize_origin(origin)?;
    }
    if let Some(verify) = verify.as_ref() {
        validate_window_resize_verify(verify)?;
    }

    Ok(WindowSizeStep {
        width,
        height,
        mode,
        target,
        origin,
        guard,
        box_model,
        verify,
    })
}

fn object_payload(kind: &str, payload: &Value) -> io::Result<Map<String, Value>> {
    payload
        .as_object()
        .cloned()
        .ok_or_else(|| invalid_data(format!("{kind} payload 必须是对象")))
}

fn optional_object_payload(kind: &str, payload: &Value) -> io::Result<Map<String, Value>> {
    if payload.is_null() {
        return Ok(Map::new());
    }
    object_payload(kind, payload)
}

fn optional_object_payload_or_null(
    kind: &str,
    payload: &Value,
) -> io::Result<Option<Map<String, Value>>> {
    if payload.is_null() {
        return Ok(None);
    }
    object_payload(kind, payload).map(Some)
}

fn u64_payload(kind: &str, payload: &Value) -> io::Result<u64> {
    payload
        .as_u64()
        .ok_or_else(|| invalid_data(format!("{kind} payload 必须是毫秒整数")))
}

fn required_f64(object: &Map<String, Value>, field: &str) -> io::Result<f64> {
    let value = object
        .get(field.rsplit('.').next().unwrap_or(field))
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid_data(format!("{field} 必须是数字")))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid_data(format!("{field} 必须是正数")));
    }
    Ok(value)
}

fn optional_object_field(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> io::Result<Option<Map<String, Value>>> {
    object
        .get(field)
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| invalid_data(format!("{label} 必须是对象")))
        })
        .transpose()
}

fn window_resize_payload(
    size: &WindowSizeStep,
    context: &UiScriptCompileContext,
) -> io::Result<Map<String, Value>> {
    let mut payload = Map::new();
    let target = size
        .target
        .clone()
        .ok_or_else(|| invalid_data("WindowSize mode:\"resize\" 缺少 target"))?;
    payload.insert("target".to_owned(), Value::Object(target));

    let mut size_payload = Map::new();
    size_payload.insert(
        "width".to_owned(),
        Value::Number(Number::from(resize_dimension_u32(
            size.width,
            "WindowSize.width",
        )?)),
    );
    size_payload.insert(
        "height".to_owned(),
        Value::Number(Number::from(resize_dimension_u32(
            size.height,
            "WindowSize.height",
        )?)),
    );
    size_payload.insert("unit".to_owned(), Value::String("os-logical".to_owned()));
    size_payload.insert(
        "box".to_owned(),
        Value::String(size.box_model.as_deref().unwrap_or("outer").to_owned()),
    );
    payload.insert("size".to_owned(), Value::Object(size_payload));

    payload.insert(
        "origin".to_owned(),
        normalize_window_resize_origin(size.origin.as_ref())?,
    );

    if let Some(guard) = size.guard.clone().or_else(|| context.scope.clone()) {
        payload.insert("guard".to_owned(), Value::Object(guard));
    }

    payload.insert(
        "verify".to_owned(),
        size.verify.clone().unwrap_or(Value::Bool(true)),
    );

    Ok(payload)
}

fn resize_dimension_u32(value: f64, field: &str) -> io::Result<u32> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
        return Err(invalid_data(format!(
            "{field} 在 mode:\"resize\" 下必须是正整数 logical px,收到: {value}"
        )));
    }
    if value > f64::from(u32::MAX) {
        return Err(invalid_data(format!("{field} 超过 u32 上限: {value}")));
    }
    Ok(value as u32)
}

fn validate_window_resize_origin(value: &Value) -> io::Result<()> {
    if value.as_str() == Some("keep") {
        return Ok(());
    }
    let Some(object) = value.as_object() else {
        return Err(invalid_data(
            "WindowSize.origin 只支持 \"keep\" 或 {\"x\":...,\"y\":...}",
        ));
    };
    validate_integral_value_field(object, "x", "WindowSize.origin.x")?;
    validate_integral_value_field(object, "y", "WindowSize.origin.y")?;
    Ok(())
}

fn validate_window_resize_verify(value: &Value) -> io::Result<()> {
    if let Some(flag) = value.as_bool() {
        if flag {
            return Ok(());
        }
        return Err(invalid_data(
            "WindowSize.verify:false 暂不支持;@window-resize 必须执行后验验证",
        ));
    }
    let Some(object) = value.as_object() else {
        return Err(invalid_data(
            "WindowSize.verify 只支持 true 或 {\"tolerance_px\":...}",
        ));
    };
    if let Some(tolerance_px) = object.get("tolerance_px") {
        if tolerance_px.as_u64().is_some() {
            return Ok(());
        }
        return Err(invalid_data(
            "WindowSize.verify.tolerance_px 必须是无符号整数",
        ));
    }
    Err(invalid_data("WindowSize.verify 对象需要 tolerance_px 字段"))
}

fn normalize_window_resize_origin(origin: Option<&Value>) -> io::Result<Value> {
    let Some(origin) = origin else {
        return Ok(Value::String("keep".to_owned()));
    };
    if origin.as_str() == Some("keep") {
        return Ok(Value::String("keep".to_owned()));
    }
    let object = origin
        .as_object()
        .ok_or_else(|| invalid_data("WindowSize.origin 只支持 \"keep\" 或对象"))?;
    let mut normalized = object.clone();
    normalize_integral_number_field(&mut normalized, "x")?;
    normalize_integral_number_field(&mut normalized, "y")?;
    Ok(Value::Object(normalized))
}

fn validate_integral_value_field(
    object: &Map<String, Value>,
    field: &str,
    label: &str,
) -> io::Result<()> {
    let value = object
        .get(field)
        .ok_or_else(|| invalid_data(format!("{label} 缺失")))?;
    if value.as_i64().is_some() || value.as_u64().is_some() {
        return Ok(());
    }
    let Some(float) = value.as_f64() else {
        return Err(invalid_data(format!("{label} 必须是整数")));
    };
    if !float.is_finite() || float.fract() != 0.0 {
        return Err(invalid_data(format!("{label} 必须是整数,收到: {float}")));
    }
    Ok(())
}

fn inject_scope_if_missing(payload: &mut Map<String, Value>, context: &UiScriptCompileContext) {
    if payload.contains_key("scope") {
        return;
    }
    if let Some(scope) = context.scope.as_ref() {
        payload.insert("scope".to_owned(), Value::Object(scope.clone()));
    }
}

fn normalize_mouse_payload(
    mut payload: Map<String, Value>,
    context: &UiScriptCompileContext,
    inject_guard: bool,
) -> io::Result<Map<String, Value>> {
    if !payload.contains_key("coordinate_space") && payload.get("target").is_none() {
        let Some(default_coordinate_space) = context.default_coordinate_space.as_ref() else {
            return Err(invalid_data(
                "Move/Click 坐标动作缺少 coordinate_space,也没有 Dialect/Policy 默认坐标语义",
            ));
        };
        payload.insert(
            "coordinate_space".to_owned(),
            Value::String(default_coordinate_space.clone()),
        );
    }
    if inject_guard && !payload.contains_key("guard") {
        if let Some(scope) = context.scope.as_ref() {
            payload.insert("guard".to_owned(), Value::Object(scope.clone()));
        }
    }
    Ok(payload)
}

fn normalize_mouse_coordinate_numbers(
    mut payload: Map<String, Value>,
) -> io::Result<Map<String, Value>> {
    for field in ["x", "y", "dx", "dy"] {
        normalize_integral_number_field(&mut payload, field)?;
    }
    Ok(payload)
}

fn normalize_integral_number_field(
    payload: &mut Map<String, Value>,
    field: &str,
) -> io::Result<()> {
    let Some(value) = payload.get(field).cloned() else {
        return Ok(());
    };
    if let Some(number) = value.as_i64() {
        payload.insert(field.to_owned(), Value::Number(Number::from(number)));
        return Ok(());
    }
    let Some(float) = value.as_f64() else {
        return Err(invalid_data(format!("{field} 必须是数字")));
    };
    if !float.is_finite() || float.fract() != 0.0 {
        return Err(invalid_data(format!(
            "{field} 当前必须是整数 logical 坐标,收到: {float}"
        )));
    }
    payload.insert(field.to_owned(), Value::Number(Number::from(float as i64)));
    Ok(())
}

fn normalize_mouse_button_case(payload: &mut Map<String, Value>) {
    if let Some(button) = payload.get("button").and_then(Value::as_str) {
        payload.insert(
            "button".to_owned(),
            Value::String(button.to_ascii_lowercase()),
        );
    }
}

fn mouse_button_payload(source: &Map<String, Value>, mode: &str) -> io::Result<Map<String, Value>> {
    let mut payload = Map::new();
    let button = source
        .get("button")
        .and_then(Value::as_str)
        .unwrap_or("left")
        .to_ascii_lowercase();
    payload.insert("button".to_owned(), Value::String(button));
    payload.insert("mode".to_owned(), Value::String(mode.to_owned()));
    if let Some(hold_ms) = source.get("hold_ms").and_then(Value::as_u64) {
        payload.insert("hold_ms".to_owned(), Value::Number(Number::from(hold_ms)));
    }
    Ok(payload)
}

fn key_payload(
    source: &Map<String, Value>,
    mode: &str,
    request_id: &mut u64,
    control_lines: &mut Vec<String>,
) -> io::Result<UiScriptDryRunEffect> {
    let key = source
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_data("KeyPress/KeyDown/KeyUp 需要 key 字段"))?;
    let mut payload = Map::new();
    payload.insert("key".to_owned(), Value::String(key.to_owned()));
    payload.insert("mode".to_owned(), Value::String(mode.to_owned()));
    if let Some(hold_ms) = source.get("hold_ms").and_then(Value::as_u64) {
        payload.insert("hold_ms".to_owned(), Value::Number(Number::from(hold_ms)));
    }
    Ok(push_control_line(
        "@key",
        payload,
        request_id,
        control_lines,
    ))
}

fn action_payload(
    mut payload: Map<String, Value>,
    request_id: &mut u64,
    control_lines: &mut Vec<String>,
) -> io::Result<UiScriptDryRunEffect> {
    let kind = payload
        .remove("kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| invalid_data("Action 需要 kind 字段"))?;
    let payload = payload
        .remove("payload")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| invalid_data("Action 需要 payload 对象"))?;
    let command = match kind.as_str() {
        "web-find" => "@web-find",
        "web-act" => "@web-act",
        "ax-action" => "@ax-action",
        "ax-set-value" => "@ax-set-value",
        "type-text" => "@type-text",
        "window-find" => "@window-find",
        "window-activate" => "@window-activate",
        "window-close" => "@window-close",
        "window-resize" => "@window-resize",
        _ => return Err(invalid_data(format!("Action.kind 当前不支持: {kind}"))),
    };
    Ok(push_control_line(
        command,
        payload,
        request_id,
        control_lines,
    ))
}

fn validate_control_line_escape(line: &str) -> io::Result<()> {
    if !line.starts_with('@') {
        return Err(invalid_data(
            "ControlLine 只能发送显式 line-control request",
        ));
    }
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("@script") || lower.starts_with("@cmd") {
        return Err(invalid_data(
            "ControlLine 默认不允许 @script / @cmd,避免 UI script 混入 shell 执行",
        ));
    }
    Ok(())
}

fn push_control_line(
    command: &str,
    payload: Map<String, Value>,
    request_id: &mut u64,
    control_lines: &mut Vec<String>,
) -> UiScriptDryRunEffect {
    let line = format!("{command}#{}:{}", *request_id, object_to_protocol(&payload));
    *request_id += 1;
    control_lines.push(line.clone());
    UiScriptDryRunEffect::ControlLine(line)
}

fn push_control_line_without_payload(
    command: &str,
    request_id: &mut u64,
    control_lines: &mut Vec<String>,
) -> UiScriptDryRunEffect {
    let line = format!("{command}#{}", *request_id);
    *request_id += 1;
    control_lines.push(line.clone());
    UiScriptDryRunEffect::ControlLine(line)
}

fn object_to_protocol(object: &Map<String, Value>) -> String {
    let mut output = String::from("{");
    for (index, (key, value)) in object.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        let _ = write!(output, "\"{key}\":{}", value_to_protocol(value));
    }
    output.push('}');
    output
}

fn value_to_protocol(value: &Value) -> String {
    match value {
        Value::Object(object) => object_to_protocol(object),
        Value::Array(values) => {
            let inner = values
                .iter()
                .map(value_to_protocol)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{inner}]")
        }
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
    }
}

fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl UiScriptStep {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Dialect(_) => "Dialect",
            Self::Target(_) => "Target",
            Self::Policy(_) => "Policy",
            Self::Scope(_) => "Scope",
            Self::SleepMs(_) => "SleepMs",
            Self::DelayMs(_) => "DelayMs",
            Self::Observe(_) => "Observe",
            Self::Screenshot(_) => "Screenshot",
            Self::Move(_) => "Move",
            Self::Click(_) => "Click",
            Self::MouseDown(_) => "MouseDown",
            Self::MouseUp(_) => "MouseUp",
            Self::KeyDown(_) => "KeyDown",
            Self::KeyUp(_) => "KeyUp",
            Self::KeyPress(_) => "KeyPress",
            Self::Text(_) => "Text",
            Self::Action(_) => "Action",
            Self::Barrier(_) => "Barrier",
            Self::Expect(_) => "Expect",
            Self::WindowSize(_) => "WindowSize",
            Self::ControlLine(_) => "ControlLine",
            Self::Exit => "Exit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_protocol::{parse_control_line, ControlCommand, ControlParseResult};
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("ui_script")
            .join(name)
    }

    fn parse_fixture(name: &str) -> io::Result<UiScriptProgram> {
        parse_script_file(&fixture_path(name))
    }

    #[test]
    fn parser_should_accept_iced_compatible_fixture() {
        let program = parse_fixture("iced_compatible_basic.json").unwrap();
        assert_eq!(program.steps.len(), 7);

        let dry_run = compile_dry_run(&program).unwrap();
        assert_eq!(dry_run.summary.step_count, 7);
        assert_eq!(dry_run.summary.backend_request_count, 4);
        assert_eq!(dry_run.summary.mouse_fallback_count, 2);
        assert_eq!(dry_run.control_lines[0], "@screenshot#1");
        assert!(dry_run.control_lines[1].contains("@mouse-move#2"));
        assert!(dry_run.control_lines[1].contains("\"coordinate_space\":\"os-logical\""));
        assert!(dry_run.control_lines[2].contains("@click#3"));
        assert!(dry_run.control_lines[2].contains("\"button\":\"left\""));
        assert_eq!(dry_run.control_lines[3], "@screenshot#4");
        assert!(matches!(
            dry_run.steps.last().map(|step| &step.effect),
            Some(UiScriptDryRunEffect::Exit)
        ));
    }

    #[test]
    fn runner_should_inject_scope_into_observe_and_mouse_guard() {
        let program = parse_fixture("rdog_target_scope_observe_expect.json").unwrap();
        let dry_run = compile_dry_run(&program).unwrap();

        assert_eq!(dry_run.summary.backend_request_count, 2);
        assert!(dry_run.control_lines[0].starts_with("@observe#1:"));
        assert!(dry_run.control_lines[0].contains("\"scope\":{\"display\":{\"id\":\"d2\"}}"));
        assert!(dry_run.control_lines[1].starts_with("@click#2:"));
        assert!(dry_run.control_lines[1].contains("\"guard\":{\"display\":{\"id\":\"d2\"}}"));
        assert_eq!(dry_run.summary.mouse_fallback_count, 1);
    }

    #[test]
    fn parser_should_accept_window_size_precondition_fixture() {
        let program = parse_fixture("window_size_precondition.json").unwrap();
        let dry_run = compile_dry_run(&program).unwrap();

        assert_eq!(dry_run.summary.backend_request_count, 1);
        assert_eq!(
            dry_run.steps[0].effect,
            UiScriptDryRunEffect::Local("window_size_precondition:1200x800".to_owned())
        );
        assert_eq!(dry_run.control_lines[0], "@observe#1:{\"mode\":\"window\"}");
    }

    #[test]
    fn runner_should_compile_window_size_resize_to_window_resize_control_line() {
        let program = parse_fixture("window_size_resize.json").unwrap();
        let dry_run = compile_dry_run(&program).unwrap();

        assert_eq!(dry_run.summary.backend_request_count, 1);
        assert_eq!(dry_run.summary.semantic_action_count, 1);
        assert_eq!(dry_run.summary.mouse_fallback_count, 0);
        assert!(dry_run.control_lines[0].starts_with("@window-resize#1:"));
        assert!(dry_run.control_lines[0].contains("\"guard\":{\"display\":{\"id\":\"d2\"}}"));

        let parsed = parse_control_line(&dry_run.control_lines[0]).unwrap();
        let ControlParseResult::Control(request) = parsed else {
            panic!("WindowSize resize should compile to a control request");
        };
        assert_eq!(request.request_id, Some(1));
        let ControlCommand::WindowResize(request) = request.command else {
            panic!("WindowSize resize should compile to @window-resize");
        };
        assert_eq!(request.target.query.app_contains.as_deref(), Some("Chrome"));
        assert_eq!(request.target.query.title_contains.as_deref(), Some("Docs"));
        assert_eq!(request.size.width, 1200);
        assert_eq!(request.size.height, 800);
        assert_eq!(
            request.origin,
            crate::control_window::WindowResizeOrigin::Point { x: 100, y: 120 }
        );
        assert!(request.guard.is_some());
        assert_eq!(request.verify.tolerance_px, 2);
    }

    #[test]
    fn runner_should_keep_expect_payload_for_real_runner() {
        let program = parse_fixture("ping_expect_response.json").unwrap();
        let dry_run = compile_dry_run(&program).unwrap();

        assert_eq!(dry_run.summary.backend_request_count, 1);
        assert!(matches!(
            &dry_run.steps[1].effect,
            UiScriptDryRunEffect::Expect(payload)
                if payload.get("kind").and_then(Value::as_str) == Some("response_contains")
        ));
        assert!(matches!(
            &dry_run.steps[2].effect,
            UiScriptDryRunEffect::Expect(payload)
                if payload.get("kind").and_then(Value::as_str) == Some("response_status")
        ));
    }

    #[test]
    fn parser_should_reject_window_size_resize_without_target() {
        let err = parse_script_json(
            r#"[{"WindowSize":{"width":1200.0,"height":800.0,"mode":"resize"}}]"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("需要 target"));
    }

    #[test]
    fn parser_should_reject_multi_key_step_fixture() {
        let err = parse_fixture("negative_multi_key_step.json").unwrap_err();
        assert!(err.to_string().contains("必须只有一个 key"));
    }

    #[test]
    fn runner_should_reject_missing_coordinate_space_fixture() {
        let program = parse_fixture("negative_missing_coordinate_space.json").unwrap();
        let err = compile_dry_run(&program).unwrap_err();
        assert!(err.to_string().contains("缺少 coordinate_space"));
    }

    #[test]
    fn parser_should_reject_window_size_without_mode_fixture() {
        let err = parse_fixture("negative_window_size_without_mode.json").unwrap_err();
        assert!(err
            .to_string()
            .contains("mode:\"precondition\" 或 mode:\"resize\""));
    }

    #[test]
    fn control_line_escape_should_reject_shell_commands() {
        let program = parse_script_json(r#"[{"ControlLine":"@cmd:\"echo unsafe\""}]"#).unwrap();
        let err = compile_dry_run(&program).unwrap_err();
        assert!(err.to_string().contains("不允许 @script / @cmd"));
    }
}
