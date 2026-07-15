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
