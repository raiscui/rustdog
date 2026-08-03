use super::*;
use crate::control_ax::{
    capture_current_ax_subtree, capture_default_ax_snapshot, perform_default_ax_action,
    AxActionName, AxActionRequest, AxCapturedSubtree, AxPerformedActionReport, AxTarget,
};

const WEB_ACT_SCHEMA: &str = "rdog.web-act.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebActRequest {
    pub find: WebFindRequest,
    pub action: WebActAction,
    pub verify: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WebActAction {
    Press,
}

impl WebActAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Press => "press",
        }
    }

    fn ax_action(self) -> AxActionName {
        match self {
            Self::Press => AxActionName::Press,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct WebActResponse {
    kind: &'static str,
    schema: &'static str,
    status: &'static str,
    scope: &'static str,
    action: &'static str,
    performed: bool,
    verified: bool,
    platform: String,
    capture_status: String,
    permission_status: String,
    coordinate_space: &'static str,
    target: WebFindTargetReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<crate::control_observation::ObservationHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_scope: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<WebFindWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    web_area: Option<WebFindArea>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_match: Option<WebFindMatch>,
    match_count: usize,
    returned_count: usize,
    truncated: bool,
    matches: Vec<WebFindMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_result: Option<AxPerformedActionReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification: Option<WebActVerification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    trace: Vec<WebFindTraceStep>,
}

impl WebActResponse {
    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("web-act response 序列化失败: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct WebActVerification {
    status: &'static str,
    verified: bool,
    match_count: usize,
    returned_count: usize,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<crate::control_observation::ObservationHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_target_id: Option<String>,
    same_target_id: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub fn parse_web_act_payload(input: &str) -> io::Result<WebActRequest> {
    let inner = object_inner(input, "@web-act")?;
    if inner.is_empty() {
        return Err(invalid_data("@web-act 对象 payload 不能为空"));
    }

    let mut target = None::<WebFindTarget>;
    let mut query = None::<WebFindQuery>;
    let mut display_scope = None::<DisplayScope>;
    let mut roles = None::<Vec<String>>;
    let mut limit = None::<u16>;
    let mut depth = None::<u8>;
    let mut max_elements = None::<u16>;
    let mut include_values = None::<bool>;
    let mut action = None::<WebActAction>;
    let mut verify = None::<bool>;

    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();

        match field_name.as_str() {
            "target" => assign_once(
                &mut target,
                "target",
                "@web-act",
                parse_web_find_target(raw_value)?,
            )?,
            "match" => assign_once(
                &mut query,
                "match",
                "@web-act",
                parse_web_find_match(raw_value)?,
            )?,
            "scope" => assign_once(
                &mut display_scope,
                "scope",
                "@web-act",
                parse_display_scope(raw_value, "@web-act.scope")?,
            )?,
            "display_id" => {
                return Err(invalid_data(
                    "@web-act.display_id 不是请求字段;请使用 scope:{display:{id:\"...\"}}",
                ))
            }
            "roles" => assign_once(
                &mut roles,
                "roles",
                "@web-act",
                parse_string_array(raw_value, "@web-act.roles")?,
            )?,
            "limit" => assign_once(&mut limit, "limit", "@web-act", parse_limit(raw_value)?)?,
            "depth" => assign_once(
                &mut depth,
                "depth",
                "@web-act",
                parse_u8(raw_value, "depth")?,
            )?,
            "max_elements" => assign_once(
                &mut max_elements,
                "max_elements",
                "@web-act",
                parse_u16(raw_value, "max_elements")?,
            )?,
            "include_values" => assign_once(
                &mut include_values,
                "include_values",
                "@web-act",
                parse_bool(raw_value, "@web-act", "include_values")?,
            )?,
            "action" => assign_once(
                &mut action,
                "action",
                "@web-act",
                parse_web_act_action(raw_value)?,
            )?,
            "verify" => assign_once(
                &mut verify,
                "verify",
                "@web-act",
                parse_bool(raw_value, "@web-act", "verify")?,
            )?,
            _ => {
                return Err(invalid_data(format!(
                    "@web-act 对象 payload 包含未知字段: {field_name}"
                )))
            }
        }
    }

    let roles = roles.unwrap_or_else(|| {
        DEFAULT_WEB_FIND_ROLES
            .iter()
            .map(|role| (*role).to_owned())
            .collect()
    });
    if roles.is_empty() {
        return Err(invalid_data("@web-act.roles 不能为空数组"));
    }

    Ok(WebActRequest {
        find: WebFindRequest {
            target: target.unwrap_or_default(),
            query: required_field(query, "@web-act", "match")?,
            display_scope,
            roles,
            limit: limit.unwrap_or(DEFAULT_WEB_FIND_LIMIT),
            depth: depth.unwrap_or(DEFAULT_WEB_FIND_DEPTH),
            max_elements: max_elements.unwrap_or(DEFAULT_WEB_FIND_MAX_ELEMENTS),
            include_values: include_values.unwrap_or(DEFAULT_WEB_FIND_INCLUDE_VALUES),
        },
        action: required_field(action, "@web-act", "action")?,
        verify: verify.unwrap_or(true),
    })
}

pub fn build_default_web_act_response_json(request: &WebActRequest) -> io::Result<String> {
    let snapshot = capture_default_ax_snapshot(&request.find.tree_request())?;
    build_web_act_response_json_with(
        &snapshot,
        request,
        perform_default_ax_action,
        || capture_default_ax_snapshot(&request.find.tree_request()),
        |target_id, tree_request| capture_current_ax_subtree(target_id, tree_request).map(Some),
    )
}

fn build_web_act_response_json_with<A, V, R>(
    snapshot: &AxSnapshot,
    request: &WebActRequest,
    mut perform_action: A,
    mut capture_verify_snapshot: V,
    mut refresh_web_area: R,
) -> io::Result<String>
where
    A: FnMut(&AxActionRequest) -> io::Result<AxPerformedActionReport>,
    V: FnMut() -> io::Result<AxSnapshot>,
    R: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    let mut trace = vec![trace_step(
        "capture-ax-snapshot",
        snapshot.capture_status.clone(),
        None,
    )];
    let snapshot = snapshot.clone().with_observation("@web-act")?;

    if snapshot.capture_status != "complete" {
        return web_act_response(WebActResponseInput {
            snapshot: &snapshot,
            request,
            status: "blocked",
            performed: false,
            verified: false,
            window: None,
            web_area: None,
            selected_match: None,
            matches: Vec::new(),
            action_result: None,
            verification: None,
            error_code: Some("AX_SNAPSHOT_UNAVAILABLE"),
            message: Some("AX snapshot 不可用,无法执行 @web-act".to_owned()),
            display_scope: None,
            trace,
            match_count: 0,
            truncated: snapshot.truncated,
        });
    }

    let resolution =
        resolve_web_matches(&snapshot, &request.find, &mut trace, &mut refresh_web_area);
    let WebMatchResolution::Resolved {
        selected_window,
        web_area,
        mut matches,
        mut match_count,
        mut truncated,
        display_scope,
    } = resolution
    else {
        return web_act_resolution_blocker(&snapshot, request, resolution, trace);
    };

    if match_count == 0 {
        return web_act_response(WebActResponseInput {
            snapshot: &snapshot,
            request,
            status: "not_found",
            performed: false,
            verified: false,
            window: Some(selected_window),
            web_area: Some(web_area.as_ref()),
            selected_match: None,
            matches,
            action_result: None,
            verification: None,
            error_code: Some("WEB_MATCH_NOT_FOUND"),
            message: Some(format!(
                "AXWebArea 内没有找到文本匹配 `{}` 的 page-owned 控件",
                request.find.query.text
            )),
            display_scope: display_scope.clone(),
            trace,
            match_count,
            truncated,
        });
    }

    if match_count > 1 {
        return web_act_response(WebActResponseInput {
            snapshot: &snapshot,
            request,
            status: "needs_disambiguation",
            performed: false,
            verified: false,
            window: Some(selected_window),
            web_area: Some(web_area.as_ref()),
            selected_match: None,
            matches,
            action_result: None,
            verification: None,
            error_code: Some("WEB_MATCH_AMBIGUOUS"),
            message: Some(format!(
                "AXWebArea 内找到 {match_count} 个匹配目标,不自动执行副作用 action"
            )),
            display_scope: display_scope.clone(),
            trace,
            match_count,
            truncated,
        });
    }

    let Some(mut selected_match) = matches.first().cloned() else {
        return web_act_response(WebActResponseInput {
            snapshot: &snapshot,
            request,
            status: "blocked",
            performed: false,
            verified: false,
            window: Some(selected_window),
            web_area: Some(web_area.as_ref()),
            selected_match: None,
            matches,
            action_result: None,
            verification: None,
            error_code: Some("WEB_MATCH_NOT_RETURNED"),
            message: Some("匹配目标存在,但受 limit 限制未返回可执行候选".to_owned()),
            display_scope: display_scope.clone(),
            trace,
            match_count,
            truncated,
        });
    };

    if !selected_match
        .actions
        .iter()
        .any(|action| action == request.action.ax_action().protocol_str())
    {
        return web_act_response(WebActResponseInput {
            snapshot: &snapshot,
            request,
            status: "blocked",
            performed: false,
            verified: false,
            window: Some(selected_window),
            web_area: Some(web_area.as_ref()),
            selected_match: Some(selected_match),
            matches,
            action_result: None,
            verification: None,
            error_code: Some("WEB_ACTION_UNAVAILABLE"),
            message: Some("匹配目标没有暴露 AXPress,本阶段不做 mouse fallback".to_owned()),
            display_scope: display_scope.clone(),
            trace,
            match_count,
            truncated,
        });
    }

    let action_request = action_request_for(request, &selected_match);
    trace.push(trace_step(
        "perform-action",
        "attempt",
        Some(format!(
            "action={},target_id={}",
            request.action.ax_action().protocol_str(),
            selected_match.id
        )),
    ));

    let action_result = match perform_action(&action_request) {
        Ok(report) => {
            trace.push(trace_step("perform-action", "ok", report.target_id.clone()));
            report
        }
        Err(err) => {
            trace.push(trace_step("perform-action", "error", Some(err.to_string())));
            if !should_retry_web_action(&err) {
                return web_act_response(WebActResponseInput {
                    snapshot: &snapshot,
                    request,
                    status: "action_failed",
                    performed: false,
                    verified: false,
                    window: Some(selected_window),
                    web_area: Some(web_area.as_ref()),
                    selected_match: Some(selected_match),
                    matches,
                    action_result: None,
                    verification: None,
                    error_code: Some("WEB_ACTION_FAILED"),
                    message: Some(err.to_string()),
                    display_scope: display_scope.clone(),
                    trace,
                    match_count,
                    truncated,
                });
            }

            let retry = retry_web_action_after_refind(
                request,
                &mut trace,
                &mut capture_verify_snapshot,
                &mut perform_action,
                &mut refresh_web_area,
            )?;
            let Some(retry) = retry else {
                return web_act_response(WebActResponseInput {
                    snapshot: &snapshot,
                    request,
                    status: "action_failed",
                    performed: false,
                    verified: false,
                    window: Some(selected_window),
                    web_area: Some(web_area.as_ref()),
                    selected_match: Some(selected_match),
                    matches,
                    action_result: None,
                    verification: None,
                    error_code: Some("WEB_ACTION_RETRY_FAILED"),
                    message: Some(format!("action 失败且 re-find retry 未能恢复: {err}")),
                    display_scope: display_scope.clone(),
                    trace,
                    match_count,
                    truncated,
                });
            };
            selected_match = retry.selected_match;
            matches = retry.matches;
            match_count = retry.match_count;
            truncated = retry.truncated;
            retry.action_result
        }
    };

    let verification = if request.verify {
        Some(verify_web_action(
            request,
            &selected_match.id,
            selected_window,
            web_area.as_ref(),
            &mut trace,
            &mut capture_verify_snapshot,
            &mut refresh_web_area,
        )?)
    } else {
        trace.push(trace_step("verify", "skipped", None));
        Some(WebActVerification {
            status: "skipped",
            verified: false,
            match_count: 0,
            returned_count: 0,
            truncated: false,
            observation: None,
            matched_target_id: None,
            same_target_id: false,
            message: Some("request.verify=false".to_owned()),
        })
    };
    let verified = verification.as_ref().is_some_and(|report| report.verified);
    let status = if request.verify && !verified {
        "verification_failed"
    } else {
        "complete"
    };
    let error_code = if request.verify && !verified {
        Some("WEB_ACTION_VERIFICATION_FAILED")
    } else {
        None
    };
    let message = if request.verify && !verified {
        Some("action 已执行,但 verification 没有重新匹配到 page-owned target".to_owned())
    } else {
        None
    };

    web_act_response(WebActResponseInput {
        snapshot: &snapshot,
        request,
        status,
        performed: action_result.performed,
        verified,
        window: Some(selected_window),
        web_area: Some(web_area.as_ref()),
        selected_match: Some(selected_match),
        matches,
        action_result: Some(action_result),
        verification,
        error_code,
        message,
        display_scope,
        trace,
        match_count,
        truncated,
    })
}

struct WebActRetryResult {
    selected_match: WebFindMatch,
    matches: Vec<WebFindMatch>,
    match_count: usize,
    truncated: bool,
    action_result: AxPerformedActionReport,
}

fn retry_web_action_after_refind<A, V, R>(
    request: &WebActRequest,
    trace: &mut Vec<WebFindTraceStep>,
    capture_retry_snapshot: &mut V,
    perform_action: &mut A,
    refresh_web_area: &mut R,
) -> io::Result<Option<WebActRetryResult>>
where
    A: FnMut(&AxActionRequest) -> io::Result<AxPerformedActionReport>,
    V: FnMut() -> io::Result<AxSnapshot>,
    R: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    trace.push(trace_step(
        "retry-refind",
        "attempt",
        Some("first action failed with stale-like target error".to_owned()),
    ));
    let retry_snapshot = capture_retry_snapshot()?;
    trace.push(trace_step(
        "retry-capture-ax-snapshot",
        retry_snapshot.capture_status.clone(),
        None,
    ));
    let retry_snapshot = retry_snapshot.clone().with_observation("@web-act.retry")?;
    if retry_snapshot.capture_status != "complete" {
        trace.push(trace_step(
            "retry-refind",
            "blocked",
            Some("retry AX snapshot 不可用".to_owned()),
        ));
        return Ok(None);
    }

    let resolution = resolve_web_matches(&retry_snapshot, &request.find, trace, refresh_web_area);
    let WebMatchResolution::Resolved {
        matches,
        match_count,
        truncated,
        ..
    } = resolution
    else {
        trace.push(trace_step(
            "retry-refind",
            "not_found",
            Some("retry 未能重新解析 browser window 或 AXWebArea".to_owned()),
        ));
        return Ok(None);
    };

    if match_count != 1 {
        trace.push(trace_step(
            "retry-refind",
            "needs_disambiguation",
            Some(format!("retry_match_count={match_count}")),
        ));
        return Ok(None);
    }

    let Some(selected_match) = matches.first().cloned() else {
        trace.push(trace_step(
            "retry-refind",
            "not_found",
            Some("retry match 未返回可执行候选".to_owned()),
        ));
        return Ok(None);
    };
    if !selected_match
        .actions
        .iter()
        .any(|action| action == request.action.ax_action().protocol_str())
    {
        trace.push(trace_step(
            "retry-refind",
            "blocked",
            Some("retry match 没有 AXPress".to_owned()),
        ));
        return Ok(None);
    }

    let action_request = action_request_for(request, &selected_match);
    trace.push(trace_step(
        "retry-perform-action",
        "attempt",
        Some(format!("target_id={}", selected_match.id)),
    ));
    match perform_action(&action_request) {
        Ok(action_result) => {
            trace.push(trace_step(
                "retry-perform-action",
                "ok",
                action_result.target_id.clone(),
            ));
            Ok(Some(WebActRetryResult {
                selected_match,
                matches,
                match_count,
                truncated,
                action_result,
            }))
        }
        Err(err) => {
            trace.push(trace_step(
                "retry-perform-action",
                "error",
                Some(err.to_string()),
            ));
            Ok(None)
        }
    }
}

fn action_request_for(request: &WebActRequest, selected_match: &WebFindMatch) -> AxActionRequest {
    AxActionRequest {
        target: AxTarget {
            id: Some(selected_match.id.clone()),
            ..AxTarget::default()
        },
        action: request.action.ax_action(),
    }
}

fn should_retry_web_action(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::InvalidInput {
        return true;
    }
    let message = err.to_string().to_ascii_lowercase();
    message.contains("stale")
        || message.contains("expired")
        || message.contains("not found")
        || message.contains("不存在")
        || message.contains("失效")
}

fn verify_web_action<V, R>(
    request: &WebActRequest,
    original_target_id: &str,
    selected_window: &AxWindow,
    web_area: &AxElement,
    trace: &mut Vec<WebFindTraceStep>,
    capture_verify_snapshot: &mut V,
    refresh_web_area: &mut R,
) -> io::Result<WebActVerification>
where
    V: FnMut() -> io::Result<AxSnapshot>,
    R: FnMut(&str, &AxTreeRequest) -> io::Result<Option<AxCapturedSubtree>>,
{
    if let Some((_refreshed_web_area, matches, match_count, truncated)) = refresh_web_area_matches(
        selected_window,
        web_area,
        &request.find,
        trace,
        refresh_web_area,
    ) {
        trace.push(trace_step(
            "verify",
            if match_count > 0 {
                "matched"
            } else {
                "not_found"
            },
            Some("source=web_area_subtree".to_owned()),
        ));
        return Ok(verification_from_matches(
            original_target_id,
            &matches,
            match_count,
            truncated,
            None,
        ));
    }

    trace.push(trace_step(
        "verify",
        "fallback_full_snapshot",
        Some("AXWebArea subtree refresh unavailable".to_owned()),
    ));
    let verify_snapshot = capture_verify_snapshot()?;
    trace.push(trace_step(
        "verify-capture-ax-snapshot",
        verify_snapshot.capture_status.clone(),
        None,
    ));
    let verify_snapshot = verify_snapshot
        .clone()
        .with_observation("@web-act.verify")?;

    if verify_snapshot.capture_status != "complete" {
        return Ok(WebActVerification {
            status: "blocked",
            verified: false,
            match_count: 0,
            returned_count: 0,
            truncated: verify_snapshot.truncated,
            observation: verify_snapshot.observation,
            matched_target_id: None,
            same_target_id: false,
            message: Some("verification AX snapshot 不可用".to_owned()),
        });
    }

    let resolution = resolve_web_matches(&verify_snapshot, &request.find, trace, refresh_web_area);
    let WebMatchResolution::Resolved {
        matches,
        match_count,
        truncated,
        ..
    } = resolution
    else {
        return Ok(WebActVerification {
            status: "not_found",
            verified: false,
            match_count: 0,
            returned_count: 0,
            truncated: verify_snapshot.truncated,
            observation: verify_snapshot.observation,
            matched_target_id: None,
            same_target_id: false,
            message: Some("verification 未能重新解析 browser window 或 AXWebArea".to_owned()),
        });
    };

    Ok(verification_from_matches(
        original_target_id,
        &matches,
        match_count,
        truncated,
        verify_snapshot.observation,
    ))
}

fn verification_from_matches(
    original_target_id: &str,
    matches: &[WebFindMatch],
    match_count: usize,
    truncated: bool,
    observation: Option<crate::control_observation::ObservationHeader>,
) -> WebActVerification {
    let matched_target_id = matches
        .iter()
        .find(|matched| matched.id == original_target_id)
        .or_else(|| matches.first())
        .map(|matched| matched.id.clone());
    let same_target_id = matched_target_id
        .as_deref()
        .map(|id| id == original_target_id)
        .unwrap_or(false);
    let verified = match_count > 0;

    WebActVerification {
        status: if verified { "matched" } else { "not_found" },
        verified,
        match_count,
        returned_count: matches.len(),
        truncated,
        observation,
        matched_target_id,
        same_target_id,
        message: (!verified).then(|| "verification 未重新匹配到 page-owned target".to_owned()),
    }
}

struct WebActResponseInput<'a> {
    snapshot: &'a AxSnapshot,
    request: &'a WebActRequest,
    status: &'static str,
    performed: bool,
    verified: bool,
    window: Option<&'a AxWindow>,
    web_area: Option<&'a AxElement>,
    selected_match: Option<WebFindMatch>,
    matches: Vec<WebFindMatch>,
    action_result: Option<AxPerformedActionReport>,
    verification: Option<WebActVerification>,
    error_code: Option<&'static str>,
    message: Option<String>,
    display_scope: Option<serde_json::Value>,
    trace: Vec<WebFindTraceStep>,
    match_count: usize,
    truncated: bool,
}

fn web_act_response(input: WebActResponseInput<'_>) -> io::Result<String> {
    let response = WebActResponse {
        kind: "web-act",
        schema: WEB_ACT_SCHEMA,
        status: input.status,
        scope: input.request.find.target.scope_str(),
        action: input.request.action.as_str(),
        performed: input.performed,
        verified: input.verified,
        platform: input.snapshot.platform.clone(),
        capture_status: input.snapshot.capture_status.clone(),
        permission_status: input.snapshot.permission_status.clone(),
        coordinate_space: input.snapshot.coordinate_space,
        target: WebFindTargetReport {
            browser: input.request.find.target.browser.as_str(),
            app: input.request.find.target.app.clone(),
            window_id: input.request.find.target.window_id.clone(),
            window_ref: input.request.find.target.window_ref.clone(),
            observation_id: input.request.find.target.observation_id.clone(),
            window_title_contains: input.request.find.target.window_title_contains.clone(),
        },
        observation: input.snapshot.observation.clone(),
        display_scope: input.display_scope,
        window: input.window.map(web_find_window),
        web_area: input.web_area.map(web_find_area),
        selected_match: input.selected_match,
        match_count: input.match_count,
        returned_count: input.matches.len(),
        truncated: input.truncated,
        matches: input.matches,
        action_result: input.action_result,
        verification: input.verification,
        error_code: input.error_code,
        message: input.message,
        trace: input.trace,
    };
    response.to_value_json()
}

fn web_act_resolution_blocker(
    snapshot: &AxSnapshot,
    request: &WebActRequest,
    resolution: WebMatchResolution<'_>,
    trace: Vec<WebFindTraceStep>,
) -> io::Result<String> {
    match resolution {
        WebMatchResolution::BrowserWindowNotFound(message) => {
            web_act_response(WebActResponseInput {
                snapshot,
                request,
                status: "not_found",
                performed: false,
                verified: false,
                window: None,
                web_area: None,
                selected_match: None,
                matches: Vec::new(),
                action_result: None,
                verification: None,
                error_code: Some("BROWSER_WINDOW_NOT_FOUND"),
                message: Some(message),
                display_scope: None,
                trace,
                match_count: 0,
                truncated: snapshot.truncated,
            })
        }
        WebMatchResolution::BrowserWindowAmbiguous(message) => {
            web_act_response(WebActResponseInput {
                snapshot,
                request,
                status: "needs_disambiguation",
                performed: false,
                verified: false,
                window: None,
                web_area: None,
                selected_match: None,
                matches: Vec::new(),
                action_result: None,
                verification: None,
                error_code: Some("BROWSER_WINDOW_AMBIGUOUS"),
                message: Some(message),
                display_scope: None,
                trace,
                match_count: 0,
                truncated: snapshot.truncated,
            })
        }
        WebMatchResolution::BrowserWindowRefInvalid(message) => {
            web_act_response(WebActResponseInput {
                snapshot,
                request,
                status: "blocked",
                performed: false,
                verified: false,
                window: None,
                web_area: None,
                selected_match: None,
                matches: Vec::new(),
                action_result: None,
                verification: None,
                error_code: Some("WINDOW_REF_INVALID"),
                message: Some(message),
                display_scope: None,
                trace,
                match_count: 0,
                truncated: snapshot.truncated,
            })
        }
        WebMatchResolution::DisplayScopeInvalid(message) => web_act_response(WebActResponseInput {
            snapshot,
            request,
            status: "blocked",
            performed: false,
            verified: false,
            window: None,
            web_area: None,
            selected_match: None,
            matches: Vec::new(),
            action_result: None,
            verification: None,
            error_code: Some("DISPLAY_SCOPE_INVALID"),
            message: Some(message),
            display_scope: None,
            trace,
            match_count: 0,
            truncated: snapshot.truncated,
        }),
        WebMatchResolution::WebAreaNotFound { selected_window } => {
            web_act_response(WebActResponseInput {
                snapshot,
                request,
                status: "not_found",
                performed: false,
                verified: false,
                window: Some(selected_window),
                web_area: None,
                selected_match: None,
                matches: Vec::new(),
                action_result: None,
                verification: None,
                error_code: Some("AX_WEB_AREA_NOT_FOUND"),
                message: Some("目标浏览器窗口里没有找到 AXWebArea".to_owned()),
                display_scope: None,
                trace,
                match_count: 0,
                truncated: snapshot.truncated,
            })
        }
        WebMatchResolution::Resolved { .. } => unreachable!("resolved result is not a blocker"),
    }
}

fn parse_web_act_action(input: &str) -> io::Result<WebActAction> {
    let action = parse_non_empty_string("@web-act.action", input)?;
    match action.to_ascii_lowercase().as_str() {
        "press" | "axpress" => Ok(WebActAction::Press),
        _ => Err(invalid_data(format!(
            "@web-act.action 当前只支持 \"press\": {action}"
        ))),
    }
}

#[cfg(test)]
mod tests;
