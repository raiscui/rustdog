use std::io;

use crate::{
    control_actions::{build_shell_command, ControlActionExecutor},
    control_bootstrap::build_bootstrap_outcome,
    control_capabilities::current_capabilities_report_json,
    control_flow::execute_flow_request,
    control_frames::ControlExecutionOutcome,
    control_observation::{
        build_observe_outcome, build_selector_get_response_json,
        build_selector_refind_response_json, build_selector_resolve_response_json,
    },
    control_protocol::{parse_control_line, ControlCommand, ControlParseResult, ControlRequest},
    screenshot::execute_screenshot_request,
};

/// 统一执行一条显式控制请求,并返回 outbound outcome。
///
/// 这里故意把“执行语义 + 响应序列化”收口在一起:
/// - TCP adapter 可以直接复用
/// - Zenoh adapter 未来也能直接复用
/// - 这样可以避免 transport 各自拼一套响应格式
/// - 也为后续一个请求产出多个 frame 提前铺底
pub fn execute_explicit_control_request<E: ControlActionExecutor>(
    request: &ControlRequest,
    shell: &str,
    executor: &E,
) -> ControlExecutionOutcome {
    match &request.command {
        ControlCommand::Ping => ControlExecutionOutcome::from_response_line(
            render_response_string(request.request_id, "pong"),
        ),
        ControlCommand::PtyClose(pty_close) => {
            match crate::pty_control::close_active_pty_session(&pty_close.session_id) {
                Ok(true) => ControlExecutionOutcome::from_response_line(
                    render_control_response_payload(request.request_id, 0, b"", b""),
                ),
                Ok(false) => {
                    ControlExecutionOutcome::from_response_line(render_protocol_error_response(
                        request.request_id,
                        64,
                        &format!("PTY session not found: {}", pty_close.session_id),
                    ))
                }
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::PtyDetach(pty_detach) => {
            match crate::pty_control::detach_active_pty_session(&pty_detach.session_id) {
                Ok(true) => ControlExecutionOutcome::from_response_line(
                    render_control_response_payload(request.request_id, 0, b"", b""),
                ),
                Ok(false) => {
                    ControlExecutionOutcome::from_response_line(render_protocol_error_response(
                        request.request_id,
                        64,
                        &format!("PTY session not found: {}", pty_detach.session_id),
                    ))
                }
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::Screenshot(screenshot_request) => {
            match execute_screenshot_request(request.request_id, screenshot_request) {
                Ok(outcome) => outcome,
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::Capabilities => match current_capabilities_report_json() {
            Ok(report_json) => ControlExecutionOutcome::from_response_line(
                render_structured_success_response(request.request_id, &report_json),
            ),
            Err(err) => ControlExecutionOutcome::from_response_line(
                render_control_action_error_response(request.request_id, &err),
            ),
        },
        ControlCommand::Observe(observe_request) => {
            match build_observe_outcome(request.request_id, observe_request) {
                Ok(outcome) => outcome,
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::Bootstrap(bootstrap_request) => {
            match build_bootstrap_outcome(request.request_id, bootstrap_request) {
                Ok(outcome) => outcome,
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::Flow(flow_request) => {
            execute_flow_request(request.request_id, flow_request, shell, |line| {
                parse_and_execute_control_line(line, shell, executor)
            })
        }
        ControlCommand::SelectorGet(selector_request) => {
            match build_selector_get_response_json(selector_request) {
                Ok(value_json) => ControlExecutionOutcome::from_response_line(
                    render_structured_success_response(request.request_id, &value_json),
                ),
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::SelectorResolve(selector_request) => {
            match build_selector_resolve_response_json(selector_request) {
                Ok(value_json) => ControlExecutionOutcome::from_response_line(
                    render_structured_success_response(request.request_id, &value_json),
                ),
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        ControlCommand::SelectorRefind(selector_request) => {
            match build_selector_refind_response_json(selector_request) {
                Ok(value_json) => ControlExecutionOutcome::from_response_line(
                    render_structured_success_response(request.request_id, &value_json),
                ),
                Err(err) => ControlExecutionOutcome::from_response_line(
                    render_control_action_error_response(request.request_id, &err),
                ),
            }
        }
        command => match executor.execute(command, shell) {
            Ok(result) => {
                let response = match result.response_value_json {
                    Some(value_json) => {
                        render_structured_success_response(request.request_id, &value_json)
                    }
                    None => render_control_response_payload(
                        request.request_id,
                        result.exit_code,
                        &result.stdout,
                        &result.stderr,
                    ),
                };
                ControlExecutionOutcome::from_response_line(response)
            }
            Err(err) => ControlExecutionOutcome::from_response_line(
                render_control_action_error_response(request.request_id, &err),
            ),
        },
    }
}

/// 统一执行一条裸 shell 行,并返回 line-control 响应。
///
/// 裸 shell 行继续保持顺序流语义:
/// - 不绑定 request id
/// - 不承诺 PTY / cwd 状态保持
/// - 每一行都是一次独立 shell 命令
pub fn execute_literal_shell_line(command: &str, shell: &str) -> ControlExecutionOutcome {
    if command.is_empty() {
        return ControlExecutionOutcome::default();
    }

    match build_shell_command(shell, command).output() {
        Ok(output) => ControlExecutionOutcome::from_response_line(render_control_response_payload(
            None,
            output.status.code().unwrap_or(-1),
            &output.stdout,
            &output.stderr,
        )),
        Err(err) => ControlExecutionOutcome::from_response_line(
            render_control_action_error_response(None, &err),
        ),
    }
}

/// 解析并执行一条 line-control 文本。
///
/// 这个入口是 transport 无关的统一控制执行层:
/// - `@...` 进入显式控制协议
/// - 普通文本进入裸 shell 行
/// - 解析错误统一渲染为 `@response {"code":...}`
pub fn parse_and_execute_control_line<E: ControlActionExecutor>(
    line: &str,
    shell: &str,
    executor: &E,
) -> ControlExecutionOutcome {
    match parse_control_line(line) {
        Ok(ControlParseResult::Control(request)) => {
            execute_explicit_control_request(&request, shell, executor)
        }
        Ok(ControlParseResult::LiteralShellLine(command)) => {
            execute_literal_shell_line(&command, shell)
        }
        Err(err) => ControlExecutionOutcome::from_response_line(render_protocol_error_response(
            parse_request_id_for_error_response(line),
            64,
            &err.to_string(),
        )),
    }
}

/// 在 parser 已经失败时,尽量从控制行 header 保留 request id。
///
/// 这只用于错误响应相关性:
/// - payload 校验失败时,`@cmd#42:...` 仍应返回 `id:42`
/// - request id 本身非法时,这里返回 None,避免伪造不可确认的 id
fn parse_request_id_for_error_response(line: &str) -> Option<u64> {
    let command = line.strip_prefix('@')?.trim_start();
    if command.starts_with('@') {
        return None;
    }

    let header = command
        .split_once(':')
        .map(|(header, _)| header)
        .unwrap_or(command)
        .trim();
    let (_, request_id) = header.split_once('#')?;
    request_id.trim().parse::<u64>().ok()
}

/// 统一把 shell / action 执行结果序列化成 line-control 响应。
///
/// 设计口径:
/// - 成功且无输出 -> `@response 0`
/// - 成功且只有 stdout -> `@response "<stdout>"`
/// - 只要出现非零退出码或 stderr -> `@response {...}` 对象
pub fn render_control_response_payload(
    request_id: Option<u64>,
    exit_code: i32,
    stdout: &[u8],
    stderr: &[u8],
) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);

    if exit_code == 0 && stderr.is_empty() {
        if stdout.is_empty() {
            return render_response_number(request_id, 0);
        }

        return render_response_string(request_id, &stdout);
    }

    let stdout = escape_json_string(&stdout);
    let stderr = escape_json_string(&stderr);
    let value = wrap_response_value(
        request_id,
        &format!("{{\"exit_code\":{exit_code},\"stdout\":\"{stdout}\",\"stderr\":\"{stderr}\"}}"),
    );
    render_response_value(&value)
}

pub fn render_structured_success_response(request_id: Option<u64>, value_json: &str) -> String {
    let wrapped = wrap_response_value(request_id, value_json);
    render_response_value(&wrapped)
}

/// line-control 协议错误统一走 code/error 对象响应。
pub fn render_protocol_error_response(request_id: Option<u64>, code: i32, error: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(error) {
        if let serde_json::Value::Object(object) = &mut value {
            object
                .entry("code".to_owned())
                .or_insert(serde_json::Value::from(code));
            if let Some(request_id) = request_id {
                object.insert("id".to_owned(), serde_json::Value::from(request_id));
            }
            if let Ok(rendered) = serde_json::to_string(&value) {
                return render_response_value(&rendered);
            }
        }
    }

    render_response_error_object(request_id, code, error)
}

fn render_control_action_error_response(request_id: Option<u64>, err: &io::Error) -> String {
    let code = match err.kind() {
        io::ErrorKind::InvalidInput => 64,
        io::ErrorKind::PermissionDenied => 77,
        io::ErrorKind::Unsupported => 78,
        _ => 70,
    };

    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&err.to_string()) {
        if let serde_json::Value::Object(object) = &mut value {
            object
                .entry("code".to_owned())
                .or_insert(serde_json::Value::from(code));
            if let Some(request_id) = request_id {
                object.insert("id".to_owned(), serde_json::Value::from(request_id));
            }
            if let Ok(rendered) = serde_json::to_string(&value) {
                return render_response_value(&rendered);
            }
        }
    }

    render_response_error_object(request_id, code, &err.to_string())
}

fn render_response_value(value: &str) -> String {
    format!("@response {value}")
}

fn wrap_response_value(request_id: Option<u64>, value: &str) -> String {
    match request_id {
        Some(request_id) => format!("{{\"id\":{request_id},\"value\":{value}}}"),
        None => value.to_owned(),
    }
}

fn render_response_string(request_id: Option<u64>, value: &str) -> String {
    let escaped = escape_json_string(value);
    let wrapped = wrap_response_value(request_id, &format!("\"{escaped}\""));
    render_response_value(&wrapped)
}

fn render_response_number(request_id: Option<u64>, value: i32) -> String {
    let wrapped = wrap_response_value(request_id, &value.to_string());
    render_response_value(&wrapped)
}

fn render_response_error_object(request_id: Option<u64>, code: i32, error: &str) -> String {
    let escaped = escape_json_string(error);
    let value = match request_id {
        Some(request_id) => {
            format!("{{\"id\":{request_id},\"code\":{code},\"error\":\"{escaped}\"}}")
        }
        None => format!("{{\"code\":{code},\"error\":\"{escaped}\"}}"),
    };
    render_response_value(&value)
}

fn escape_json_string(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());

    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_actions::{ActionExecutionResult, SystemControlActionExecutor},
        control_frames::ControlFrame,
        control_protocol::{
            parse_control_line, ControlCommand, ControlParseResult, KeyMode, KeyRequest,
            PasteRequest,
        },
    };
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use serde_json::Value;
    use std::{
        fs,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone, Default)]
    struct FakeExecutor {
        commands: Arc<Mutex<Vec<ControlCommand>>>,
    }

    impl ControlActionExecutor for FakeExecutor {
        fn execute(
            &self,
            command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
            self.commands
                .lock()
                .expect("commands lock should work")
                .push(command.clone());

            Ok(ActionExecutionResult {
                exit_code: 0,
                stdout: b"EXEC_OK".to_vec(),
                stderr: Vec::new(),
                response_value_json: None,
            })
        }
    }

    #[derive(Clone)]
    struct FakePermissionDeniedExecutor;

    impl ControlActionExecutor for FakePermissionDeniedExecutor {
        fn execute(
            &self,
            _command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "blocked by UIPI",
            ))
        }
    }

    fn parse_response_payload(response: &str) -> Value {
        serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("response should be wrapped as @response"),
        )
        .expect("response should be valid json")
    }

    fn temp_flow_dir(name: &str) -> std::path::PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_millis();
        std::env::temp_dir().join(format!(
            "rdog-flow-core-{name}-{millis}-{}",
            std::process::id()
        ))
    }

    fn escape_json(input: &str) -> String {
        input.replace('\\', "\\\\").replace('"', "\\\"")
    }

    #[test]
    fn explicit_request_should_wrap_ping_as_response_line() {
        let response = execute_explicit_control_request(
            &ControlRequest {
                request_id: Some(7),
                command: ControlCommand::Ping,
            },
            "/bin/sh",
            &FakeExecutor::default(),
        )
        .into_single_response_line();

        assert_eq!(response, r#"@response {"id":7,"value":"pong"}"#);
    }

    #[test]
    fn explicit_request_should_route_command_to_executor() {
        let executor = FakeExecutor::default();
        let recorded = Arc::clone(&executor.commands);

        let response = execute_explicit_control_request(
            &ControlRequest {
                request_id: Some(42),
                command: ControlCommand::Key(KeyRequest::legacy("F11", 200, KeyMode::PressRelease)),
            },
            "/bin/sh",
            &executor,
        )
        .into_single_response_line();

        assert_eq!(response, r#"@response {"id":42,"value":"EXEC_OK"}"#);
        assert_eq!(
            recorded
                .lock()
                .expect("commands lock should work")
                .as_slice(),
            &[ControlCommand::Key(KeyRequest::legacy(
                "F11",
                200,
                KeyMode::PressRelease,
            ))]
        );
    }

    #[test]
    fn parse_error_response_should_keep_protocol_error_shape() {
        let response = match parse_control_line(r#"@script:"printf a\nb""#) {
            Ok(ControlParseResult::Control(_)) | Ok(ControlParseResult::LiteralShellLine(_)) => {
                panic!("expected parse failure")
            }
            Err(err) => render_protocol_error_response(None, 64, &err.to_string()),
        };

        assert!(response.contains(r#""code":64"#));
        assert!(response.contains("首版不支持多行 payload"));
    }

    #[test]
    fn zenoh_profile_should_accept_key_requests() {
        let executor = FakeExecutor::default();
        let recorded = Arc::clone(&executor.commands);

        let response = parse_and_execute_control_line(r#"@key#9:"F11""#, "/bin/sh", &executor)
            .into_single_response_line();

        assert_eq!(response, r#"@response {"id":9,"value":"EXEC_OK"}"#);
        assert_eq!(
            recorded
                .lock()
                .expect("commands lock should work")
                .as_slice(),
            &[ControlCommand::Key(KeyRequest::legacy(
                "F11",
                200,
                KeyMode::PressRelease,
            ))]
        );
    }

    #[test]
    fn control_line_should_route_legacy_paste_requests_to_executor() {
        let executor = FakeExecutor::default();
        let recorded = Arc::clone(&executor.commands);

        let response = parse_and_execute_control_line(r#"@paste:"hello""#, "/bin/sh", &executor)
            .into_single_response_line();

        assert_eq!(response, r#"@response "EXEC_OK""#);
        assert_eq!(
            recorded
                .lock()
                .expect("commands lock should work")
                .as_slice(),
            &[ControlCommand::Paste(PasteRequest::legacy_text("hello"))]
        );
    }

    #[test]
    fn control_line_should_route_bare_paste_hotkey_requests_to_executor() {
        let executor = FakeExecutor::default();
        let recorded = Arc::clone(&executor.commands);

        let response = parse_and_execute_control_line(r#"@paste#12"#, "/bin/sh", &executor)
            .into_single_response_line();

        assert_eq!(response, r#"@response {"id":12,"value":"EXEC_OK"}"#);
        assert_eq!(
            recorded
                .lock()
                .expect("commands lock should work")
                .as_slice(),
            &[ControlCommand::Paste(PasteRequest::hotkey())]
        );
    }

    #[test]
    fn control_line_should_execute_literal_shell_line() {
        #[cfg(windows)]
        let (shell, command) = ("cmd.exe", "echo LITERAL_OK");
        #[cfg(not(windows))]
        let (shell, command) = ("/bin/sh", "printf LITERAL_OK");

        let response = parse_and_execute_control_line(command, shell, &FakeExecutor::default())
            .into_single_response_line();

        assert!(
            response.contains("LITERAL_OK"),
            "literal shell response should contain command stdout, got {response}"
        );
    }

    #[test]
    fn explicit_request_should_render_permission_denied_as_code_77() {
        let response = parse_and_execute_control_line(
            r#"@key:"F11""#,
            "/bin/sh",
            &FakePermissionDeniedExecutor,
        )
        .into_single_response_line();

        assert!(response.contains(r#""code":77"#));
        assert!(response.contains("blocked by UIPI"));
    }

    #[test]
    fn explicit_request_should_forward_structured_invalid_input_json() {
        #[derive(Clone)]
        struct StructuredInvalidInputExecutor;

        impl ControlActionExecutor for StructuredInvalidInputExecutor {
            fn execute(
                &self,
                _command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    r#"{"kind":"window-ambiguous","code":64,"error":"matched multiple windows","match_count":2}"#,
                ))
            }
        }

        let response = parse_and_execute_control_line(
            r#"@window-close:{app:"Terminal",title_contains:"rdog"}"#,
            "/bin/sh",
            &StructuredInvalidInputExecutor,
        )
        .into_single_response_line();

        assert_eq!(
            response,
            r#"@response {"code":64,"error":"matched multiple windows","kind":"window-ambiguous","match_count":2}"#
        );
    }

    #[test]
    fn explicit_request_should_forward_structured_other_error_json() {
        #[derive(Clone)]
        struct StructuredOtherExecutor;

        impl ControlActionExecutor for StructuredOtherExecutor {
            fn execute(
                &self,
                _command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                Err(io::Error::other(
                    r#"{"kind":"screenshot-stale-frame","error_code":"SCREENSHOT_STALE_FRAME","error":"stale visual frame","display_count":2}"#,
                ))
            }
        }

        let response =
            parse_and_execute_control_line(r#"@key#17:"F11""#, "/bin/sh", &StructuredOtherExecutor)
                .into_single_response_line();

        assert_eq!(
            response,
            r#"@response {"code":70,"display_count":2,"error":"stale visual frame","error_code":"SCREENSHOT_STALE_FRAME","id":17,"kind":"screenshot-stale-frame"}"#
        );
    }

    #[test]
    fn explicit_request_should_render_structured_success_without_double_escaping() {
        #[derive(Clone)]
        struct StructuredExecutor;

        impl ControlActionExecutor for StructuredExecutor {
            fn execute(
                &self,
                _command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                Ok(ActionExecutionResult {
                    exit_code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    response_value_json: Some(
                        r#"{"kind":"mouse","action":"move","coordinate_space":"os-logical","x":1,"y":2}"#
                            .to_owned(),
                    ),
                })
            }
        }

        let response = execute_explicit_control_request(
            &ControlRequest {
                request_id: Some(10),
                command: ControlCommand::Key(KeyRequest::legacy("F11", 200, KeyMode::PressRelease)),
            },
            "/bin/sh",
            &StructuredExecutor,
        )
        .into_single_response_line();

        assert_eq!(
            response,
            r#"@response {"id":10,"value":{"kind":"mouse","action":"move","coordinate_space":"os-logical","x":1,"y":2}}"#
        );
    }

    #[test]
    fn explicit_request_should_route_ax_commands_to_executor() {
        #[derive(Clone)]
        struct StructuredAxExecutor;

        impl ControlActionExecutor for StructuredAxExecutor {
            fn execute(
                &self,
                command: &ControlCommand,
                _shell: &str,
            ) -> io::Result<ActionExecutionResult> {
                match command {
                    ControlCommand::AxTree(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax-tree","schema":"rdog.ax.v1","capture_status":"complete"}"#
                                .to_owned(),
                        ),
                    }),
                    ControlCommand::AxFind(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax-find","schema":"rdog.ax.v1","match_count":1}"#
                                .to_owned(),
                        ),
                    }),
                    ControlCommand::AxGet(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax-get","schema":"rdog.ax.v1","target_type":"element"}"#
                                .to_owned(),
                        ),
                    }),
                    ControlCommand::AxAction(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax-action","action":"AXShowMenu","performed":true}"#
                                .to_owned(),
                        ),
                    }),
                    ControlCommand::AxPress(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax","action":"press","performed":true}"#.to_owned(),
                        ),
                    }),
                    ControlCommand::AxSetValue(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax-set-value","mode":"append","performed":true}"#
                                .to_owned(),
                        ),
                    }),
                    ControlCommand::TypeText(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"type-text","delivered_via":"ax-value","performed":true}"#
                                .to_owned(),
                        ),
                    }),
                    _ => Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unexpected command",
                    )),
                }
            }
        }

        let tree_response = parse_and_execute_control_line(
            r#"@ax-tree#9:{scope:"windows"}"#,
            "/bin/sh",
            &StructuredAxExecutor,
        )
        .into_single_response_line();
        let press_response = parse_and_execute_control_line(
            r#"@ax-press#10:{target:{id:"pid:1/window:0/path:0"}}"#,
            "/bin/sh",
            &StructuredAxExecutor,
        )
        .into_single_response_line();
        let action_response = parse_and_execute_control_line(
            r#"@ax-action#11:{target:{id:"pid:1/window:0/path:0"},action:"AXShowMenu"}"#,
            "/bin/sh",
            &StructuredAxExecutor,
        )
        .into_single_response_line();
        let set_value_response = parse_and_execute_control_line(
            r#"@ax-set-value#12:{target:{id:"pid:1/window:0/path:0"},value:"hello",mode:"append"}"#,
            "/bin/sh",
            &StructuredAxExecutor,
        )
        .into_single_response_line();
        let type_text_response = parse_and_execute_control_line(
            r#"@type-text#13:{target:{id:"pid:1/window:0/path:0"},text:"hello",mode:"ax-value"}"#,
            "/bin/sh",
            &StructuredAxExecutor,
        )
        .into_single_response_line();

        assert_eq!(
            tree_response,
            r#"@response {"id":9,"value":{"kind":"ax-tree","schema":"rdog.ax.v1","capture_status":"complete"}}"#
        );
        assert_eq!(
            press_response,
            r#"@response {"id":10,"value":{"kind":"ax","action":"press","performed":true}}"#
        );
        assert_eq!(
            action_response,
            r#"@response {"id":11,"value":{"kind":"ax-action","action":"AXShowMenu","performed":true}}"#
        );
        assert_eq!(
            set_value_response,
            r#"@response {"id":12,"value":{"kind":"ax-set-value","mode":"append","performed":true}}"#
        );
        assert_eq!(
            type_text_response,
            r#"@response {"id":13,"value":{"kind":"type-text","delivered_via":"ax-value","performed":true}}"#
        );
    }

    #[test]
    fn explicit_request_should_return_outcome_with_single_response_frame() {
        let outcome = execute_explicit_control_request(
            &ControlRequest {
                request_id: Some(1),
                command: ControlCommand::Ping,
            },
            "/bin/sh",
            &FakeExecutor::default(),
        );

        assert_eq!(outcome.outbound_frames.len(), 1);
    }

    #[test]
    fn explicit_request_should_render_capabilities_report() {
        let response =
            parse_and_execute_control_line("@capabilities#12", "/bin/sh", &FakeExecutor::default())
                .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("capabilities response should be wrapped as @response"),
        )
        .expect("capabilities response should be valid json");

        assert_eq!(parsed["id"], 12);
        assert_eq!(parsed["value"]["kind"], "capabilities");
        assert_eq!(parsed["value"]["schema"], "rdog.capabilities.v1");
        assert_eq!(parsed["value"]["gui_agent_recipe"][0], "@capabilities");
        assert_eq!(
            parsed["value"]["capabilities"]["line_control"]["status"],
            "available"
        );
    }

    #[test]
    fn explicit_request_should_execute_minimal_flow_shell_lane() {
        let response = parse_and_execute_control_line(
            r#"@flow#44:{"schema":"rdog.flow.v1","policy":{"allow_shell":true},"steps":[{"Cmd":{"run":"printf flow-ok","capture":"cmd1"}},{"Expect":{"kind":"cmd_stdout_contains","capture":"cmd1","contains":"flow-ok"}},{"Exit":null}]}"#,
            "/bin/sh",
            &SystemControlActionExecutor::default(),
        )
        .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("@flow response should be wrapped as @response"),
        )
        .expect("@flow response should be valid json");

        assert_eq!(parsed["id"], 44);
        assert_eq!(parsed["value"]["schema"], "rdog.flow.v1");
        assert_eq!(parsed["value"]["status"], "ok");
        assert_eq!(parsed["value"]["captures"]["cmd1"]["stdout"], "flow-ok");
    }

    #[test]
    fn explicit_request_should_consume_flow_control_line_response() {
        let response = parse_and_execute_control_line(
            r#"@flow#45:{"schema":"rdog.flow.v1","steps":[{"ControlLine":"@ping"},{"Expect":{"kind":"response_contains","contains":"pong"}}]}"#,
            "/bin/sh",
            &SystemControlActionExecutor::default(),
        )
        .into_single_response_line();
        let parsed = parse_response_payload(&response);

        assert_eq!(parsed["id"], 45);
        assert_eq!(parsed["value"]["status"], "ok");
        assert_eq!(parsed["value"]["response_count"], 1);
        assert_eq!(parsed["value"]["completed_steps"], 2);
    }

    #[test]
    fn explicit_request_should_lift_flow_save_artifact_before_final_response() {
        let dir = temp_flow_dir("save-artifact");
        fs::create_dir_all(&dir).expect("temp dir should create");
        let artifact_path = dir.join("report.txt");
        fs::write(&artifact_path, "artifact-ok").expect("artifact should write");
        let line = format!(
            r#"@flow#46:{{"schema":"rdog.flow.v1","policy":{{"allow_file_read":true}},"steps":[{{"SaveArtifact":{{"path":"{}","mime":"text/plain","filename":"report.txt"}}}},{{"Expect":{{"kind":"artifact_exists","artifact":"report.txt"}}}}]}}"#,
            escape_json(
                artifact_path
                    .to_str()
                    .expect("artifact path should be utf8")
            ),
        );

        let outcome = parse_and_execute_control_line(
            &line,
            "/bin/sh",
            &SystemControlActionExecutor::default(),
        );
        assert_eq!(outcome.outbound_frames.len(), 2);
        let ControlFrame::SaveFile(savefile) = &outcome.outbound_frames[0] else {
            panic!("first frame should be @savefile");
        };
        assert_eq!(savefile.request_id, Some(46));
        assert_eq!(savefile.filename, "report.txt");
        assert_eq!(savefile.mime, "text/plain");
        assert_eq!(
            BASE64_STANDARD
                .decode(savefile.data.as_bytes())
                .expect("savefile data should decode"),
            b"artifact-ok"
        );

        let ControlFrame::ResponseLine(response) = &outcome.outbound_frames[1] else {
            panic!("last frame should be final @response");
        };
        let parsed = parse_response_payload(response);
        assert_eq!(parsed["value"]["status"], "ok");
        assert_eq!(parsed["value"]["artifacts"][0], "report.txt");
    }

    #[test]
    fn explicit_request_should_emit_flow_trace_savefile_before_final_response() {
        let outcome = parse_and_execute_control_line(
            r#"@flow#47:{"schema":"rdog.flow.v1","options":{"trace":"savefile"},"steps":[{"ControlLine":"@ping"},{"Expect":{"kind":"response_contains","contains":"pong"}}]}"#,
            "/bin/sh",
            &SystemControlActionExecutor::default(),
        );
        assert_eq!(outcome.outbound_frames.len(), 2);
        let ControlFrame::SaveFile(trace) = &outcome.outbound_frames[0] else {
            panic!("first frame should be trace @savefile");
        };
        assert_eq!(trace.request_id, Some(47));
        assert_eq!(trace.filename, "flow-trace-47.jsonl");
        assert_eq!(trace.mime, "application/jsonl");
        let trace_jsonl = String::from_utf8(
            BASE64_STANDARD
                .decode(trace.data.as_bytes())
                .expect("trace data should decode"),
        )
        .expect("trace should be utf8");
        assert!(trace_jsonl.contains(r#""kind":"ControlLine""#));
        assert!(trace_jsonl.contains(r#""kind":"Expect""#));

        let ControlFrame::ResponseLine(response) = &outcome.outbound_frames[1] else {
            panic!("last frame should be final @response");
        };
        let parsed = parse_response_payload(response);
        assert_eq!(parsed["value"]["trace_record_count"], 2);
    }

    #[test]
    fn explicit_request_should_render_observe_without_action_executor() {
        let executor = FakeExecutor::default();
        let response =
            parse_and_execute_control_line(r#"@observe#77:{mode:"window"}"#, "/bin/sh", &executor)
                .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("observe response should be wrapped as @response"),
        )
        .expect("observe response should be valid json");

        assert_eq!(parsed["id"], 77);
        assert_eq!(parsed["value"]["kind"], "observe");
        assert_eq!(parsed["value"]["schema"], "rdog.observe.v1");
        assert_eq!(parsed["value"]["mode"], "window");
        assert_eq!(parsed["value"]["windows"]["status"], "skipped");
        assert!(executor
            .commands
            .lock()
            .expect("commands lock should work")
            .is_empty());
    }

    #[test]
    fn explicit_request_should_render_basic_bootstrap_without_action_executor() {
        let executor = FakeExecutor::default();
        let response = parse_and_execute_control_line("@bootstrap#21", "/bin/sh", &executor)
            .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("bootstrap response should be wrapped as @response"),
        )
        .expect("bootstrap response should be valid json");

        assert_eq!(parsed["id"], 21);
        assert_eq!(parsed["value"]["kind"], "bootstrap");
        assert_eq!(parsed["value"]["schema"], "rdog.bootstrap.v1");
        assert_eq!(parsed["value"]["mode"], "basic");
        assert_eq!(parsed["value"]["liveness"]["reply"], "pong");
        assert_eq!(parsed["value"]["capabilities"]["kind"], "capabilities");
        assert_eq!(parsed["value"]["observation"]["status"], "not_requested");
        assert_eq!(parsed["value"]["frames"]["savefile_count"], 0);
        assert!(parsed["value"]["trace"].is_array());
        assert!(executor
            .commands
            .lock()
            .expect("commands lock should work")
            .is_empty());
    }

    #[test]
    fn explicit_request_should_render_gui_bootstrap_with_observe_bundle() {
        let executor = FakeExecutor::default();
        let response = parse_and_execute_control_line(
            r#"@bootstrap#22:{mode:"gui",observe:{mode:"window"},include_trace:false}"#,
            "/bin/sh",
            &executor,
        )
        .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("bootstrap response should be wrapped as @response"),
        )
        .expect("bootstrap response should be valid json");

        assert_eq!(parsed["id"], 22);
        assert_eq!(parsed["value"]["kind"], "bootstrap");
        assert_eq!(parsed["value"]["mode"], "gui");
        assert_eq!(parsed["value"]["observation"]["kind"], "observe");
        assert_eq!(parsed["value"]["observation"]["mode"], "window");
        assert_eq!(parsed["value"]["lanes"]["windows"]["status"], "skipped");
        assert_eq!(
            parsed["value"]["frames"]["final_response_order"],
            "savefiles-before-response"
        );
        assert!(parsed["value"]["trace"].is_null());
        assert!(executor
            .commands
            .lock()
            .expect("commands lock should work")
            .is_empty());
    }

    #[test]
    fn parse_error_should_preserve_bootstrap_cached_policy_structure() {
        let response = parse_and_execute_control_line(
            r#"@bootstrap#42:{capability_policy:"cached"}"#,
            "/bin/sh",
            &FakeExecutor::default(),
        )
        .into_single_response_line();
        let parsed: Value = serde_json::from_str(
            response
                .strip_prefix("@response ")
                .expect("bootstrap error should be wrapped as @response"),
        )
        .expect("bootstrap error should be valid json");

        assert_eq!(parsed["kind"], "bootstrap");
        assert_eq!(parsed["schema"], "rdog.bootstrap.v1");
        assert_eq!(parsed["id"], 42);
        assert_eq!(parsed["status"], "blocked");
        assert_eq!(
            parsed["error_code"],
            "BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED"
        );
        assert_eq!(parsed["code"], 64);
    }
}
