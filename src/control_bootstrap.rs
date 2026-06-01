use std::io;

use crate::{
    control_ax::parse_bool_literal,
    control_capabilities::current_capabilities_report_value,
    control_frames::{ControlExecutionOutcome, ControlFrame},
    control_observation::{observe::build_observe_bundle, parse_observe_payload, ObserveRequest},
    control_protocol::{
        normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
        split_object_fields,
    },
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED: &str =
    "BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED";
pub(crate) const BOOTSTRAP_SCHEMA: &str = "rdog.bootstrap.v1";

/// `@bootstrap` 的首版模式。
///
/// `Basic` 只做 liveness + capabilities 预检。
/// `Gui` 允许组合只读 observe bundle,但不执行任何 GUI side effect。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapMode {
    Basic,
    Gui,
}

impl Default for BootstrapMode {
    fn default() -> Self {
        Self::Basic
    }
}

impl BootstrapMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Gui => "gui",
        }
    }

    fn parse(input: &str) -> io::Result<Self> {
        match parse_quoted_payload(input)?.as_str() {
            "basic" => Ok(Self::Basic),
            "gui" => Ok(Self::Gui),
            other => Err(invalid_data(format!("@bootstrap.mode 不支持: {other}"))),
        }
    }
}

/// 能力探测策略。
///
/// 第一版只允许 `fresh`。`cached` 保留给后续 TTL cache,现在必须显式拒绝,
/// 避免 agent 把未实现缓存误读成已经使用了新鲜探测。
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapCapabilityPolicy {
    Fresh,
}

impl Default for BootstrapCapabilityPolicy {
    fn default() -> Self {
        Self::Fresh
    }
}

impl BootstrapCapabilityPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
        }
    }

    fn parse(input: &str) -> io::Result<Self> {
        match parse_quoted_payload(input)?.as_str() {
            "fresh" => Ok(Self::Fresh),
            "cached" => Err(invalid_data(
                bootstrap_cached_policy_error_value().to_string(),
            )),
            other => Err(invalid_data(format!(
                "@bootstrap.capability_policy 不支持: {other}"
            ))),
        }
    }
}

/// `@bootstrap` 的只读请求 schema。
///
/// parser 阶段只负责确认输入安全、结构清楚。真正的 capabilities / observe
/// 产出在后续 story 里复用现有 producer,这里不复制执行逻辑。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootstrapRequest {
    pub(crate) mode: BootstrapMode,
    pub(crate) capability_policy: BootstrapCapabilityPolicy,
    pub(crate) observe: Option<ObserveRequest>,
    pub(crate) include_trace: bool,
}

impl Default for BootstrapRequest {
    fn default() -> Self {
        Self {
            mode: BootstrapMode::default(),
            capability_policy: BootstrapCapabilityPolicy::default(),
            observe: None,
            include_trace: true,
        }
    }
}

pub(crate) fn parse_bootstrap_payload(input: &str) -> io::Result<BootstrapRequest> {
    if input.trim().is_empty() {
        return Ok(BootstrapRequest::default());
    }

    let inner = object_inner(input, "@bootstrap")?;
    if inner.is_empty() {
        return Ok(BootstrapRequest::default());
    }

    let mut mode = None::<BootstrapMode>;
    let mut capability_policy = None::<BootstrapCapabilityPolicy>;
    let mut observe = None::<ObserveRequest>;
    let mut include_trace = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "mode" => assign_once(&mut mode, "mode", BootstrapMode::parse(raw_value)?)?,
            "capability_policy" => assign_once(
                &mut capability_policy,
                "capability_policy",
                BootstrapCapabilityPolicy::parse(raw_value)?,
            )?,
            "observe" => assign_once(&mut observe, "observe", parse_observe_payload(raw_value)?)?,
            "include_trace" => assign_once(
                &mut include_trace,
                "include_trace",
                parse_bool_literal("@bootstrap", "include_trace", raw_value)?,
            )?,
            "action" | "click" | "press" | "type" | "key" | "allow_side_effects" => {
                return Err(invalid_data(format!(
                    "@bootstrap 是 read-only preflight,不接受可能产生 GUI side effect 的字段: {field_name}"
                )))
            }
            _ => {
                return Err(invalid_data(format!(
                    "@bootstrap 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let request = BootstrapRequest {
        mode: mode.unwrap_or_default(),
        capability_policy: capability_policy.unwrap_or_default(),
        observe,
        include_trace: include_trace.unwrap_or(true),
    };

    validate_bootstrap_request(request)
}

pub(crate) fn build_bootstrap_outcome(
    request_id: Option<u64>,
    request: &BootstrapRequest,
) -> io::Result<ControlExecutionOutcome> {
    let capabilities = current_capabilities_report_value()?;
    let observe_bundle = match request.mode {
        BootstrapMode::Basic => None,
        BootstrapMode::Gui => {
            let observe_request = request.observe.clone().unwrap_or_default();
            Some(build_observe_bundle(request_id, &observe_request)?)
        }
    };

    let savefile_count = observe_bundle
        .as_ref()
        .map(|bundle| bundle.savefile_frames.len())
        .unwrap_or(0);
    let observation = observe_bundle
        .as_ref()
        .map(|bundle| bundle.value.clone())
        .unwrap_or_else(|| json!({"status": "not_requested"}));
    let lanes = bootstrap_lanes(&observation);
    let errors = bootstrap_lane_errors(&lanes);
    let status = bootstrap_status(&capabilities, &observation);

    let mut value = json!({
        "kind": "bootstrap",
        "schema": BOOTSTRAP_SCHEMA,
        "status": status,
        "mode": request.mode.as_str(),
        "observed_at_unix_ms": now_unix_ms(),
        "liveness": {
            "status": "complete",
            "reply": "pong",
        },
        "capability_policy": {
            "requested": request.capability_policy.as_str(),
            "effective": "fresh",
            "cache_ttl_ms": 0,
        },
        "capabilities": capabilities,
        "observation": observation,
        "lanes": lanes,
        "errors": errors,
        "frames": {
            "savefile_count": savefile_count,
            "final_response_order": "savefiles-before-response",
        },
    });

    if request.include_trace {
        value["trace"] = bootstrap_trace(&value);
    }

    let value_json = serde_json::to_string(&value)
        .map_err(|err| io::Error::other(format!("bootstrap response 序列化失败: {err}")))?;
    let response_line = render_structured_response(request_id, &value_json);
    let mut outbound_frames = observe_bundle
        .map(|bundle| bundle.savefile_frames)
        .unwrap_or_default();
    outbound_frames.push(ControlFrame::ResponseLine(response_line));

    Ok(ControlExecutionOutcome { outbound_frames })
}

pub(crate) fn bootstrap_cached_policy_error_value() -> Value {
    json!({
        "kind": "bootstrap",
        "schema": BOOTSTRAP_SCHEMA,
        "status": "blocked",
        "error_code": BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED,
        "message": "capability_policy:\"cached\" is reserved for a future TTL cache; use capability_policy:\"fresh\"",
    })
}

fn validate_bootstrap_request(request: BootstrapRequest) -> io::Result<BootstrapRequest> {
    if matches!(request.mode, BootstrapMode::Basic) && request.observe.is_some() {
        return Err(invalid_data(
            "@bootstrap mode:\"basic\" 不接受 observe;请改用 mode:\"gui\"",
        ));
    }

    Ok(request)
}

fn bootstrap_lanes(observation: &Value) -> Value {
    if observation
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "not_requested")
    {
        return json!({
            "visual": {"status": "not_requested"},
            "accessibility": {"status": "not_requested"},
            "windows": {"status": "not_requested"},
        });
    }

    json!({
        "visual": observation
            .get("visual")
            .cloned()
            .unwrap_or_else(|| json!({"status": "not_requested"})),
        "accessibility": observation
            .get("accessibility")
            .cloned()
            .unwrap_or_else(|| json!({"status": "not_requested"})),
        "windows": observation
            .get("windows")
            .cloned()
            .unwrap_or_else(|| json!({"status": "not_requested"})),
    })
}

fn bootstrap_lane_errors(lanes: &Value) -> Value {
    let mut errors = Vec::new();

    for lane in ["visual", "accessibility", "windows"] {
        let Some(status) = lanes
            .get(lane)
            .and_then(|section| section.get("status"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        if matches!(status, "complete" | "not_requested") {
            continue;
        }

        let code = match status {
            "permission_denied" => 77,
            "unsupported" => 78,
            _ => 70,
        };
        errors.push(json!({
            "lane": lane,
            "status": status,
            "code": code,
        }));
    }

    Value::Array(errors)
}

fn bootstrap_status(capabilities: &Value, observation: &Value) -> &'static str {
    let capabilities_status = capabilities
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let observe_status = observation
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("not_requested");

    if matches!(capabilities_status, "blocked" | "error") {
        "blocked"
    } else if capabilities_status != "complete"
        || !matches!(observe_status, "complete" | "not_requested")
    {
        "degraded"
    } else {
        "complete"
    }
}

fn bootstrap_trace(value: &Value) -> Value {
    let observe_status = value
        .get("observation")
        .and_then(|observation| observation.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("not_requested");

    json!([
        {"step": "liveness", "status": "complete"},
        {"step": "capabilities", "status": value["capabilities"]["status"]},
        {"step": "observe", "status": observe_status},
    ])
}

fn render_structured_response(request_id: Option<u64>, value_json: &str) -> String {
    match request_id {
        Some(request_id) => format!(r#"@response {{"id":{request_id},"value":{value_json}}}"#),
        None => format!("@response {value_json}"),
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn assign_once<T>(slot: &mut Option<T>, field_name: &str, value: T) -> io::Result<()> {
    if slot.is_some() {
        return Err(invalid_data(format!(
            "@bootstrap 的 `{field_name}` 字段重复"
        )));
    }

    *slot = Some(value);
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
