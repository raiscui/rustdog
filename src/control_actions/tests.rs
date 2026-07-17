use super::*;
use crate::{
    control_mouse::{
        ClickRequest, MouseAnchor, MouseButtonName, MouseCoordinateSpace, MouseEndpoint,
        MouseSelectorTarget, DEFAULT_MOUSE_CLICK_HOLD_MS, DEFAULT_MOUSE_CLICK_INTERVAL_MS,
    },
    control_observation::{
        record_observation, ObservationRefEntry, ObservationRoot, SelectorRefindPolicy,
    },
};
use std::cell::Cell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
struct FakeExecutor {
    calls: std::sync::Mutex<Vec<ControlCommand>>,
}

impl ControlActionExecutor for FakeExecutor {
    fn execute(&self, command: &ControlCommand, _shell: &str, _cancel: Option<&crate::cancellation::CancellationToken>) -> io::Result<ActionExecutionResult> {
        self.calls
            .lock()
            .expect("fake executor lock should work")
            .push(command.clone());
        Ok(ActionExecutionResult {
            exit_code: 0,
            stdout: b"FAKE_OK".to_vec(),
            stderr: Vec::new(),
            response_value_json: None,
        })
    }
}

#[derive(Default)]
struct RecordingKeyInputEventSink {
    requests: std::sync::Mutex<Vec<KeyRequest>>,
}

impl KeyInputEventSink for RecordingKeyInputEventSink {
    fn publish_key_event(&self, request: &KeyRequest) -> io::Result<()> {
        self.requests
            .lock()
            .expect("recording sink lock should work")
            .push(request.clone());
        Ok(())
    }
}

struct AlwaysFailingKeyInputEventSink;

impl KeyInputEventSink for AlwaysFailingKeyInputEventSink {
    fn publish_key_event(&self, _request: &KeyRequest) -> io::Result<()> {
        Err(io::Error::other("keyboard event publish failed"))
    }
}

#[test]
fn execute_control_command_should_delegate_builtins_to_executor() {
    let executor = FakeExecutor::default();
    let output = executor
        .execute(
            &ControlCommand::Key(KeyRequest::legacy("F11", 200, KeyMode::PressRelease)),
            "/bin/sh",
            None,
        )
        .unwrap();

    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout, b"FAKE_OK");
    assert_eq!(
        executor
            .calls
            .lock()
            .expect("fake executor lock should work")
            .as_slice(),
        &[ControlCommand::Key(KeyRequest::legacy(
            "F11",
            200,
            KeyMode::PressRelease,
        ))]
    );
}

#[test]
fn execute_control_command_should_run_script_via_shell() {
    let executor = SystemControlActionExecutor::default();
    let (shell, script, expected_stdout) = script_test_case();
    let output = executor
        .execute(&ControlCommand::Script(script.to_owned()),shell, None)
        .unwrap();

    assert_eq!(output.exit_code, 0);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end_matches(['\r', '\n']),
        expected_stdout
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.stderr.is_empty() || is_deleted_cwd_shell_warning(&stderr),
        "unexpected shell stderr: {stderr}"
    );
}

#[test]
fn target_window_id_from_ax_target_should_resolve_observation_ref() {
    let header = record_observation(
        "ax",
        "@ax-tree",
        ObservationRoot {
            schema: "rdog.ax.v1".to_owned(),
            platform: "macos".to_owned(),
            coordinate_space: "os-logical".to_owned(),
        },
        vec![ObservationRefEntry {
            ref_id: "@e2".to_owned(),
            backend_id: "pid:9/window:3/path:0.1".to_owned(),
            kind: "element".to_owned(),
        }],
    )
    .expect("observation should record");
    let target = crate::control_ax::AxTarget {
        ref_id: Some("@e2".to_owned()),
        observation_id: Some(header.observation_id),
        ..crate::control_ax::AxTarget::default()
    };

    let window_id = target_window_id_from_ax_target(Some(&target))
        .expect("ref target should resolve")
        .expect("resolved ref should carry a window id");

    assert_eq!(window_id, "pid:9/window:3");
}

#[test]
fn ax_focus_should_not_run_when_window_activation_verification_failed() {
    let request = crate::control_ax::AxFocusRequest {
        target: None,
        window_id: Some("pid:9/window:2".to_owned()),
        activate: true,
    };
    let focus_called = Cell::new(false);
    let result = execute_ax_focus_with(
        &request,
        |_| Ok(failed_window_activation_report()),
        |_| {
            focus_called.set(true);
            Ok(crate::control_ax::AxFocusReport::success(
                "test",
                None,
                Some("pid:9/window:2".to_owned()),
                true,
            ))
        },
    )
    .unwrap();

    assert!(!focus_called.get(), "activation失败后不能继续执行AX focus");
    let response: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("ax-focus失败应该返回结构化响应"),
    )
    .unwrap();
    assert_eq!(response["kind"], "ax-focus");
    assert_eq!(response["status"], "failed");
    assert_eq!(response["performed"], false);
    assert_eq!(response["error_code"], "WINDOW_FOCUS_NOT_ACQUIRED");
    assert_eq!(response["activation"]["verify"]["status"], "failed");
}

fn failed_window_activation_report() -> crate::control_window::WindowActionReport {
    crate::control_window::WindowActionReport {
        kind: "window-action",
        schema: crate::control_window::WINDOW_SCHEMA,
        platform: "macos".to_owned(),
        action: "activate",
        status: "failed".to_owned(),
        window_id: Some("pid:9/window:2".to_owned()),
        snapshot_id: Some("window-snapshot-test".to_owned()),
        observed_at_unix_ms: Some(42),
        strategy: None,
        target_pid: None,
        process_scope: None,
        termination_attempted: None,
        failed_step: Some("verify_focus".to_owned()),
        error_code: Some("WINDOW_FOCUS_NOT_ACQUIRED"),
        before_rect: None,
        requested_size: None,
        requested_rect: None,
        after_rect: None,
        delta: None,
        verify: Some(crate::control_window::WindowActionVerifyReport::Activate(
            crate::control_window::WindowActivateVerifyReport {
                status: "failed".to_owned(),
                focused: false,
                frontmost: true,
                hidden: false,
                minimized: false,
                timeout_ms: 2_000,
                elapsed_ms: 2_000,
            },
        )),
        guard: None,
        clamp_reason: None,
        steps: Vec::new(),
    }
}

#[test]
fn selector_mouse_target_without_auto_refind_should_return_no_action_before_backend() {
    let executor = SystemControlActionExecutor::default();
    let result = executor
        .execute(
            &ControlCommand::Click(ClickRequest {
                x: None,
                y: None,
                target: Some(MouseEndpoint::Selector(MouseSelectorTarget {
                    selector_id: "sel-v1-action".to_owned(),
                    auto_refind: false,
                    policy: SelectorRefindPolicy::Safe,
                    min_confidence_milli: 900,
                    anchor: MouseAnchor::Center,
                })),
                guard: None,
                button: MouseButtonName::Left,
                count: 1,
                hold_ms: DEFAULT_MOUSE_CLICK_HOLD_MS,
                interval_ms: DEFAULT_MOUSE_CLICK_INTERVAL_MS,
                coordinate_space: MouseCoordinateSpace::OsLogical,
            }),
            "/bin/sh",
            None,
        )
        .expect("selector handoff should not require mouse backend");

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let response_json = result
        .response_value_json
        .expect("selector handoff should produce structured response");
    let response_value: serde_json::Value =
        serde_json::from_str(&response_json).expect("response should parse");
    assert_eq!(response_value["performed"].as_bool(), Some(false));
    assert_eq!(
        response_value["target_resolution"]["gate_decision"].as_str(),
        Some("handoff_required")
    );
}

fn script_test_case() -> (&'static str, &'static str, &'static str) {
    #[cfg(windows)]
    {
        ("cmd.exe", "echo SCRIPT_OK", "SCRIPT_OK")
    }

    #[cfg(not(windows))]
    {
        ("/bin/sh", "printf SCRIPT_OK", "SCRIPT_OK")
    }
}

fn is_deleted_cwd_shell_warning(stderr: &str) -> bool {
    // 并发单测里 `figment::Jail` 会切换并清理进程级 cwd。
    // `/bin/sh` 继承到这个瞬间状态时,可能只是在启动阶段打印 getcwd 警告。
    stderr.contains("getcwd") && stderr.contains("No such file")
}

#[test]
fn execute_key_should_publish_event_after_successful_key_plan() {
    let sink = RecordingKeyInputEventSink::default();
    let request = KeyRequest::legacy("F11", 200, KeyMode::PressRelease);
    let performed = Arc::new(AtomicBool::new(false));
    let performed_for_closure = Arc::clone(&performed);

    let result = execute_key_with_dependencies(
        &request,
        move |_| {
            performed_for_closure.store(true, Ordering::Relaxed);
            Ok(())
        },
        Some(&sink),
    )
    .expect("key execution should succeed");

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert!(performed.load(Ordering::Relaxed));
    assert_eq!(
        sink.requests
            .lock()
            .expect("recording sink lock should work")
            .as_slice(),
        &[request]
    );
}

#[test]
fn execute_key_should_not_publish_event_when_key_plan_fails() {
    let sink = RecordingKeyInputEventSink::default();
    let err = execute_key_with_dependencies(
        &KeyRequest::legacy("F11", 200, KeyMode::PressRelease),
        |_| Err(io::Error::other("key input failed")),
        Some(&sink),
    )
    .expect_err("key execution should fail");

    assert_eq!(err.to_string(), "key input failed");
    assert!(sink
        .requests
        .lock()
        .expect("recording sink lock should work")
        .is_empty());
}

#[test]
fn execute_key_should_fail_when_event_publish_fails() {
    let err = execute_key_with_dependencies(
        &KeyRequest::legacy("F11", 200, KeyMode::PressRelease),
        |_| Ok(()),
        Some(&AlwaysFailingKeyInputEventSink),
    )
    .expect_err("publish failure should bubble up");

    assert!(err.to_string().contains("keyboard event publish failed"));
}

#[test]
fn structured_global_key_success_response_should_report_structured_global_success() {
    let mut request = KeyRequest::legacy("F11", 200, KeyMode::PressRelease);
    request.response_mode = KeyResponseMode::Structured;
    let response_json = structured_global_key_success_response(&request)
        .expect("structured global response should serialize")
        .expect("structured mode should produce a response");
    let response_value: serde_json::Value =
        serde_json::from_str(&response_json).expect("response json should parse");

    assert_eq!(response_value["kind"].as_str(), Some("key"));
    assert_eq!(
        response_value["backend"].as_str(),
        Some("global-input-simulation")
    );
    assert_eq!(response_value["delivery"].as_str(), Some("global"));
    assert_eq!(response_value["key"].as_str(), Some("F11"));
    assert_eq!(response_value["performed"].as_bool(), Some(true));
    assert_eq!(response_value["status"].as_str(), Some("ok"));
    assert!(response_value.get("target_pid").is_none());
    assert!(response_value.get("window_id").is_none());
}

#[test]
fn execute_paste_hotkey_should_use_platform_shortcut_and_structured_report() {
    let hotkey_performed = Arc::new(AtomicBool::new(false));
    let text_injected = Arc::new(AtomicBool::new(false));
    let hotkey_performed_for_closure = Arc::clone(&hotkey_performed);
    let text_injected_for_closure = Arc::clone(&text_injected);

    let result = execute_paste_with_dependencies(
        &PasteRequest::hotkey(),
        move |request| {
            hotkey_performed_for_closure.store(true, Ordering::Relaxed);
            assert_eq!(request.key, platform_paste_shortcut());
            assert_eq!(request.hold_ms, DEFAULT_KEY_HOLD_MS);
            assert_eq!(request.mode, KeyMode::PressRelease);
            Ok(())
        },
        move |_| {
            text_injected_for_closure.store(true, Ordering::Relaxed);
            Ok(())
        },
    )
    .expect("paste hotkey should execute through hotkey branch");

    assert!(hotkey_performed.load(Ordering::Relaxed));
    assert!(!text_injected.load(Ordering::Relaxed));
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());

    let response_json = result
        .response_value_json
        .expect("hotkey paste should produce structured response");
    let response_value: serde_json::Value =
        serde_json::from_str(&response_json).expect("paste response should parse");
    assert_eq!(response_value["kind"].as_str(), Some("paste"));
    assert_eq!(response_value["delivery"].as_str(), Some("global-hotkey"));
    assert_eq!(
        response_value["delivered_via"].as_str(),
        Some(platform_paste_delivered_via())
    );
    assert_eq!(response_value["used_hotkey"].as_bool(), Some(true));
    assert_eq!(response_value["used_keyboard"].as_bool(), Some(true));
    assert_eq!(response_value["requires_focus"].as_bool(), Some(true));
    assert_eq!(response_value["performed"].as_bool(), Some(true));
    assert_eq!(response_value["status"].as_str(), Some("ok"));
}

#[test]
fn execute_paste_legacy_text_should_keep_text_injection_compatibility() {
    let hotkey_performed = Arc::new(AtomicBool::new(false));
    let text_injected = Arc::new(AtomicBool::new(false));
    let hotkey_performed_for_closure = Arc::clone(&hotkey_performed);
    let text_injected_for_closure = Arc::clone(&text_injected);

    let result = execute_paste_with_dependencies(
        &PasteRequest::legacy_text("hello"),
        move |_| {
            hotkey_performed_for_closure.store(true, Ordering::Relaxed);
            Ok(())
        },
        move |text| {
            text_injected_for_closure.store(true, Ordering::Relaxed);
            assert_eq!(text, "hello");
            Ok(())
        },
    )
    .expect("legacy paste should execute through text injection branch");

    assert!(!hotkey_performed.load(Ordering::Relaxed));
    assert!(text_injected.load(Ordering::Relaxed));
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert!(result.response_value_json.is_none());
}

#[test]
fn parse_key_action_should_support_function_keys_and_modifiers() {
    let action = parse_key_action("ctrl+v").unwrap();

    assert_eq!(
        action,
        KeyAction {
            modifiers: vec![Key::Control],
            main_key: Key::Unicode('v'),
        }
    );

    let action = parse_key_action("F11").unwrap();
    assert_eq!(
        action,
        KeyAction {
            modifiers: Vec::new(),
            main_key: Key::F11,
        }
    );
}

#[test]
fn parse_key_action_should_support_side_specific_modifier_tokens() {
    let action = parse_key_action("right-control+c").unwrap();
    assert_eq!(
        action,
        KeyAction {
            modifiers: vec![Key::RControl],
            main_key: Key::Unicode('c'),
        }
    );

    let action = parse_key_action("left-shift+tab").unwrap();
    assert_eq!(
        action,
        KeyAction {
            modifiers: vec![Key::LShift],
            main_key: Key::Tab,
        }
    );
}

#[cfg(target_os = "macos")]
#[test]
fn parse_key_action_should_support_right_option_as_single_key() {
    let action = parse_key_action("right-option").unwrap();
    assert_eq!(
        action,
        KeyAction {
            modifiers: Vec::new(),
            main_key: Key::ROption,
        }
    );
}

#[test]
fn parse_key_action_should_reject_unsupported_key_names() {
    let err = parse_key_action("ctrl+hyper").unwrap_err();

    assert!(err.to_string().contains("首版不支持的 @key 按键"));
}

#[test]
fn build_key_execution_plan_should_default_to_press_release_with_hold() {
    let plan = build_key_execution_plan(&KeyRequest::legacy(
        "right-control+c",
        200,
        KeyMode::PressRelease,
    ))
    .unwrap();

    assert_eq!(
        plan,
        vec![
            KeyPlanStep::Press(Key::RControl),
            KeyPlanStep::Press(Key::Unicode('c')),
            KeyPlanStep::Hold(200),
            KeyPlanStep::Release(Key::Unicode('c')),
            KeyPlanStep::Release(Key::RControl),
        ]
    );
}

#[test]
fn build_key_execution_plan_should_support_press_only() {
    let plan =
        build_key_execution_plan(&KeyRequest::legacy("right-control+c", 200, KeyMode::Press))
            .unwrap();

    assert_eq!(
        plan,
        vec![
            KeyPlanStep::Press(Key::RControl),
            KeyPlanStep::Press(Key::Unicode('c')),
        ]
    );
}

#[test]
fn build_key_execution_plan_should_support_release_only() {
    let plan = build_key_execution_plan(&KeyRequest::legacy(
        "right-control+c",
        200,
        KeyMode::Release,
    ))
    .unwrap();

    assert_eq!(
        plan,
        vec![
            KeyPlanStep::Release(Key::Unicode('c')),
            KeyPlanStep::Release(Key::RControl),
        ]
    );
}

#[test]
fn to_io_error_should_upgrade_uipi_failures_to_permission_denied() {
    let err = to_io_error("simulating input failed: (not all input events were sent. they may have been blocked by UIPI)");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("UIPI"));
    assert!(err.to_string().contains("相同或更高权限级别"));
}

#[test]
fn to_io_error_should_upgrade_macos_accessibility_failures_to_permission_denied() {
    let err = to_io_error("the application does not have the permission to simulate input");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("辅助功能权限"));
    assert!(err.to_string().contains("重启该进程"));
}


// ============================================================================
// Phase F-3.5: `@open-app` PermissionDenied / app_not_found / ok error envelope
// 注入 mock (PermissionDenied live trigger 验证 trait 注入路径)
// ============================================================================
/// Helper 构造一个指定退出码的 ExitStatus (mock 测试帮手函数).
///
/// Unix `ExitStatus::from_raw` 接收的是 wait4() status word (而不是裸 exit code).
/// 退出码 `c` 在 wait status 里编码为 `(c << 8)`, 低 7 位必须为 0 (signal=None).
/// 直接 `from_raw(c)` 会让 `.code()` 返 None, 不是 Some(c).
fn fake_exit_status(code: u8) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(i32::from(code) << 8)
}



/// Mock: 模拟 `Command::new("open")` 进程 spawn 失败 (PATH 缺失或不存在的 binary),
/// 这是 daemon 真实环境触发 PermissionDenied 的路径。
struct MockOpenAppPermissionDenied;

impl OpenAppCommand for MockOpenAppPermissionDenied {
    fn run(&self, app_name: &str) -> io::Result<std::process::Output> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("`open` 命令不可用: PATH 隔离导致 spawn 失败 (app_name={app_name:?})"),
        ))
    }
}

/// Mock: 模拟 `open` 命令能 spawn 但 exit 1 (典型 app not found 场景)。
/// 通过伪造 ExitStatus 来精确模拟 `status.code() == Some(1)` 路径。
struct MockOpenAppAppNotFound;

impl OpenAppCommand for MockOpenAppAppNotFound {
    fn run(&self, _app_name: &str) -> io::Result<std::process::Output> {
        let status = fake_exit_status(1);
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: b"Unable to find application
".to_vec(),
        })
    }
}

/// Mock: 模拟 `open` 命令成功 (exit 0), 走 happy path ok envelope。
struct MockOpenAppSuccess;

impl OpenAppCommand for MockOpenAppSuccess {
    fn run(&self, _app_name: &str) -> io::Result<std::process::Output> {
        let status = fake_exit_status(0);
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

#[test]
fn execute_open_app_emits_permission_denied_envelope_when_spawn_fails() {
    let request = OpenAppRequest {
        app_name: "NonExistent.App".to_string(),
        wait_ms: 0,
    };

    // 注入 mock: spawn 直接返 Err(NotFound)
    let result = execute_open_app(&request, &MockOpenAppPermissionDenied)
        .expect("executor itself should not error; envelope encodes the failure");

    // exit_code != 0 表示上层要按失败处理 (64 与现有 parse error 同 code)
    assert_ne!(result.exit_code, 0);

    // payload 必须含 permission_denied envelope 关键字段
    let payload: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("payload should be present"),
    )
    .expect("payload should be valid JSON");

    assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
    assert_eq!(
        payload.get("error_code"),
        Some(&serde_json::json!("permission_denied"))
    );
    assert!(
        payload.get("error_message").is_some(),
        "permission_denied envelope must include error_message"
    );

    // Phase F-1/F-2 contract: retry.strategy="never" + retry.hint 必填
    let retry = payload
        .get("retry")
        .expect("permission_denied envelope must include retry");
    assert_eq!(
        retry.get("strategy"),
        Some(&serde_json::json!("never")),
        "permission_denied retry.strategy must be 'never'"
    );
    assert!(
        retry.get("hint").is_some(),
        "permission_denied retry.hint must be present"
    );
}

#[test]
fn execute_open_app_emits_app_not_found_envelope_when_open_exits_nonzero() {
    let request = OpenAppRequest {
        app_name: "FakeApp".to_string(),
        wait_ms: 0,
    };

    // 注入 mock: open 退出码 1 + stderr (app not found)
    let result = execute_open_app(&request, &MockOpenAppAppNotFound)
        .expect("executor itself should not error");

    assert_ne!(result.exit_code, 0);

    let payload: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("payload should be present"),
    )
    .expect("payload should be valid JSON");

    // 注意: app_not_found 区别于 permission_denied,
    // 不走 permission_denied_envelope_json (无 retry 字段, evidence 必填)
    assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
    assert_eq!(
        payload.get("error_code"),
        Some(&serde_json::json!("app_not_found"))
    );

    let evidence = payload
        .get("evidence")
        .expect("app_not_found envelope must include evidence");
    assert_eq!(
        evidence.get("exit_code"),
        Some(&serde_json::json!(1)),
        "evidence.exit_code must mirror open process status"
    );
    assert_eq!(
        evidence.get("app_name"),
        Some(&serde_json::json!("FakeApp"))
    );

    // app_not_found 没有 retry 字段 (设计区别于 permission_denied)
    assert!(
        payload.get("retry").is_none(),
        "app_not_found envelope must not include retry (区别于 permission_denied)"
    );
}

#[test]
fn execute_open_app_emits_ok_envelope_when_open_succeeds() {
    let request = OpenAppRequest {
        app_name: "TextEdit".to_string(),
        wait_ms: 0,
    };

    // happy path: open 退出码 0
    let result = execute_open_app(&request, &MockOpenAppSuccess)
        .expect("executor itself should not error on success");

    // 成功路径 exit_code == 0
    assert_eq!(result.exit_code, 0);

    let payload: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("payload should be present"),
    )
    .expect("payload should be valid JSON");

    assert_eq!(payload.get("ok"), Some(&serde_json::json!(true)));
    assert_eq!(
        payload.get("dispatched_to"),
        Some(&serde_json::json!("@open-app"))
    );
    assert_eq!(
        payload.get("app_name"),
        Some(&serde_json::json!("TextEdit"))
    );
    assert_eq!(payload.get("wait_ms"), Some(&serde_json::json!(0)));

    // happy path 不应包含 error_code / retry / evidence
    assert!(payload.get("error_code").is_none());
    assert!(payload.get("retry").is_none());
    assert!(payload.get("evidence").is_none());
}


// ============================================================================
// Phase F-3.5 follow-up: `@cancel#seq` self-target bug fix unit tests
//
// Bug: control_core.rs:141 `command =>` 默认分支会把 Cancel 命令自己的 seq
//      先 register 进 cancel_registry, 然后 execute_cancel(..., &self.cancel_registry)
//      .signal(self_seq) 会返 true, 让 `@cancel#seq#N:{target_seq:N}` 误报成功。
//
// Fix: control_core.rs default 分支加 `is_cancel_command` guard, 跳过 register/unregister。
//      这两个 unit test 在 executor 层面锁住 execute_cancel 语义
//      (单测不依赖 control_core routing, smoke_cancel_seq test 5 是集成证明).
// ============================================================================

/// `execute_cancel` 在目标 seq 不在 registry 时返 envelope error_code=unknown_target_seq。
///
/// 这是 self-target bug 的根因场景: cancel 命令自己的 seq 不该在 registry 里。
/// 测试不预 register, 直接调 execute_cancel, 期望 unknown_target_seq envelope.
#[test]
fn execute_cancel_emits_unknown_target_seq_when_target_not_in_registry() {
    use crate::control_protocol::CancelRequest;

    let registry = crate::cancellation::CancelRegistry::new();
    let request = CancelRequest { target_seq: 999 };

    let result = execute_cancel(&request, &registry)
        .expect("executor itself should not error; envelope encodes the failure");

    // exit_code != 0 表示上层要按失败处理
    assert_ne!(result.exit_code, 0);

    let payload: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("payload should be present"),
    )
    .expect("payload should be valid JSON");

    assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
    assert_eq!(
        payload.get("error_code"),
        Some(&serde_json::json!("unknown_target_seq"))
    );
    assert_eq!(
        payload.get("dispatched_to"),
        Some(&serde_json::json!("@cancel#seq"))
    );
    assert_eq!(
        payload.get("target_seq"),
        Some(&serde_json::json!(999))
    );

    // evidence 字段必须说明 registry state (设计区别于其它 error_code)
    let evidence = payload
        .get("evidence")
        .expect("unknown_target_seq envelope must include evidence");
    assert_eq!(
        evidence.get("registry_state"),
        Some(&serde_json::json!("empty_or_completed"))
    );
}

/// `execute_cancel` 在目标 seq 已 register 时返 ok=true, signaled=true.
///
/// 这是 non-self-target happy path: 取消一个真实在跑的 cmd (e.g. test 1
/// `@wait` 注册 seq=1 后 `@cancel#seq#2:{target_seq:1}` 取消它). 此测试
/// 验 happy path envelope shape, 锁住 fix 没破坏现有 cancel 行为.
#[test]
fn execute_cancel_emits_ok_when_target_signal_succeeds() {
    use crate::control_protocol::CancelRequest;

    let registry = crate::cancellation::CancelRegistry::new();
    // 预注册 seq=42 模拟"有一个 in-flight 命令在跑"
    let _token = registry.register(42);

    let request = CancelRequest { target_seq: 42 };
    let result = execute_cancel(&request, &registry)
        .expect("executor itself should not error on success");

    assert_eq!(result.exit_code, 0);

    let payload: serde_json::Value = serde_json::from_str(
        result
            .response_value_json
            .as_deref()
            .expect("payload should be present"),
    )
    .expect("payload should be valid JSON");

    assert_eq!(payload.get("ok"), Some(&serde_json::json!(true)));
    assert_eq!(payload.get("signaled"), Some(&serde_json::json!(true)));
    assert_eq!(
        payload.get("dispatched_to"),
        Some(&serde_json::json!("@cancel#seq"))
    );
    assert_eq!(payload.get("target_seq"), Some(&serde_json::json!(42)));

    // happy path 没有 error_code / evidence 字段
    assert!(payload.get("error_code").is_none());
    assert!(payload.get("evidence").is_none());

    // 关键不变量: 取消成功后, 调用方负责 unregister. 测试模拟"调用方忘了 unregister"
    // 也通过 (token 继续存活, 不会破坏 cancel 语义).
}
