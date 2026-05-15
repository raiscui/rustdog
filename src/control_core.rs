use std::io;

use crate::{
    control_actions::{build_shell_command, ControlActionExecutor},
    control_frames::ControlExecutionOutcome,
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
            None,
            64,
            &err.to_string(),
        )),
    }
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
    render_response_error_object(request_id, code, error)
}

fn render_control_action_error_response(request_id: Option<u64>, err: &io::Error) -> String {
    let code = match err.kind() {
        io::ErrorKind::InvalidInput => 64,
        io::ErrorKind::PermissionDenied => 77,
        io::ErrorKind::Unsupported => 78,
        _ => 70,
    };
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
        control_actions::ActionExecutionResult,
        control_protocol::{
            parse_control_line, ControlCommand, ControlParseResult, KeyMode, KeyRequest,
        },
    };
    use std::sync::{Arc, Mutex};

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
                command: ControlCommand::Key(KeyRequest {
                    key: "F11".to_owned(),
                    hold_ms: 200,
                    mode: KeyMode::PressRelease,
                }),
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
            &[ControlCommand::Key(KeyRequest {
                key: "F11".to_owned(),
                hold_ms: 200,
                mode: KeyMode::PressRelease,
            })]
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
            &[ControlCommand::Key(KeyRequest {
                key: "F11".to_owned(),
                hold_ms: 200,
                mode: KeyMode::PressRelease,
            })]
        );
    }

    #[test]
    fn control_line_should_route_paste_requests_to_executor() {
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
            &[ControlCommand::Paste("hello".to_owned())]
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
                command: ControlCommand::Key(KeyRequest {
                    key: "F11".to_owned(),
                    hold_ms: 200,
                    mode: KeyMode::PressRelease,
                }),
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
                    ControlCommand::AxPress(_) => Ok(ActionExecutionResult {
                        exit_code: 0,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                        response_value_json: Some(
                            r#"{"kind":"ax","action":"press","performed":true}"#.to_owned(),
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

        assert_eq!(
            tree_response,
            r#"@response {"id":9,"value":{"kind":"ax-tree","schema":"rdog.ax.v1","capture_status":"complete"}}"#
        );
        assert_eq!(
            press_response,
            r#"@response {"id":10,"value":{"kind":"ax","action":"press","performed":true}}"#
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
}
