use crate::{
    control_frames::{default_savefile_directory, SaveFileFrame},
    control_mouse::{
        build_click_plan, build_drag_plan, build_mouse_button_plan, build_mouse_move_plan,
        build_wheel_plan, perform_mouse_plan, MouseExecutionPlan,
    },
    control_protocol::{ControlCommand, KeyMode, KeyRequest},
};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
    thread,
    time::Duration,
};

/// 控制动作执行后的统一返回。
///
/// 这里不直接决定 line-control 的最终协议文案。
/// 上层会把它封装成 `@response ...` 请求/响应格式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionExecutionResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub response_value_json: Option<String>,
}

pub trait ControlActionExecutor {
    fn execute(&self, command: &ControlCommand, shell: &str) -> io::Result<ActionExecutionResult>;
}

/// `@key` 成功执行后,可选地向外部系统发布一条键盘事件。
///
/// 这里故意只暴露最小接口:
/// - 执行层只关心“本次 key request 成功了,要不要顺手发事件”
/// - transport / Zenoh / 日志等具体实现细节留给下游 sink 自己处理
pub trait KeyInputEventSink: Send + Sync {
    fn publish_key_event(&self, request: &KeyRequest) -> io::Result<()>;
}

pub struct SystemControlActionExecutor {
    key_input_event_sink: Option<Arc<dyn KeyInputEventSink>>,
    savefile_base_dir: Option<PathBuf>,
}

impl Default for SystemControlActionExecutor {
    fn default() -> Self {
        Self {
            key_input_event_sink: None,
            savefile_base_dir: None,
        }
    }
}

impl SystemControlActionExecutor {
    /// 创建一个会在 `@key` 成功后同步发布键盘事件的执行器。
    pub fn with_key_input_event_sink(key_input_event_sink: Arc<dyn KeyInputEventSink>) -> Self {
        Self {
            key_input_event_sink: Some(key_input_event_sink),
            savefile_base_dir: None,
        }
    }

    /// 创建一个使用自定义保存目录的执行器。
    ///
    /// 主要给测试或未来配置注入使用。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_savefile_base_dir(savefile_base_dir: PathBuf) -> Self {
        Self {
            key_input_event_sink: None,
            savefile_base_dir: Some(savefile_base_dir),
        }
    }
}

impl Clone for SystemControlActionExecutor {
    fn clone(&self) -> Self {
        Self {
            key_input_event_sink: self.key_input_event_sink.as_ref().map(Arc::clone),
            savefile_base_dir: self.savefile_base_dir.clone(),
        }
    }
}

impl ControlActionExecutor for SystemControlActionExecutor {
    fn execute(&self, command: &ControlCommand, shell: &str) -> io::Result<ActionExecutionResult> {
        match command {
            ControlCommand::Key(request) => {
                execute_key(request, self.key_input_event_sink.as_deref())
            }
            ControlCommand::Paste(text) => execute_paste(text),
            ControlCommand::Ping => Ok(ActionExecutionResult {
                exit_code: 0,
                stdout: b"pong".to_vec(),
                stderr: Vec::new(),
                response_value_json: None,
            }),
            ControlCommand::Script(script_text) => execute_script(shell, script_text),
            ControlCommand::Screenshot(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@screenshot 由 control_core 直接走 screenshot producer,不应进入默认 executor 分支",
            )),
            ControlCommand::MouseMove(request) => execute_mouse_plan(build_mouse_move_plan(request)?),
            ControlCommand::MouseButton(request) => {
                execute_mouse_plan(build_mouse_button_plan(request)?)
            }
            ControlCommand::Click(request) => execute_mouse_plan(build_click_plan(request)?),
            ControlCommand::Drag(request) => execute_mouse_plan(build_drag_plan(request)?),
            ControlCommand::Wheel(request) => execute_mouse_plan(build_wheel_plan(request)?),
            ControlCommand::PtyOpen(_)
            | ControlCommand::PtyClose(_)
            | ControlCommand::PtyDetach(_)
            | ControlCommand::PtyAttach(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@pty / @pty-close / @pty-detach / @pty-attach 由 PTY session runtime 处理,不应进入默认 executor 分支",
            )),
            ControlCommand::SaveFile(frame) => {
                execute_save_file(frame, self.savefile_base_dir.as_deref())
            }
        }
    }
}

fn execute_script(shell: &str, script_text: &str) -> io::Result<ActionExecutionResult> {
    let output = build_shell_command(shell, script_text).output()?;
    Ok(from_process_output(output))
}

pub(crate) fn build_shell_command(shell: &str, command_text: &str) -> Command {
    let mut command = Command::new(shell);

    match shell_program_name(shell).as_deref() {
        Some("bash") => {
            command
                .args(["--noprofile", "--norc", "-c"])
                .arg(command_text);
        }
        Some("zsh") => {
            command.args(["-f", "-c"]).arg(command_text);
        }
        Some("sh") => {
            command.args(["-c"]).arg(command_text);
        }
        Some("pwsh") | Some("pwsh.exe") | Some("powershell") | Some("powershell.exe") => {
            command
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
                .arg(command_text);
        }
        Some("cmd") | Some("cmd.exe") => {
            command.args(["/Q", "/D", "/C"]).arg(command_text);
        }
        _ => {
            command.args(["-c"]).arg(command_text);
        }
    }

    command
}

pub(crate) fn shell_program_name(shell: &str) -> Option<String> {
    std::path::Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

fn execute_key(
    request: &KeyRequest,
    key_input_event_sink: Option<&dyn KeyInputEventSink>,
) -> io::Result<ActionExecutionResult> {
    execute_key_with_dependencies(
        request,
        |request| {
            let key_plan = build_key_execution_plan(request)?;
            let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
            perform_key_plan(&mut enigo, &key_plan).map_err(to_io_error)
        },
        key_input_event_sink,
    )
}

fn execute_key_with_dependencies<F>(
    request: &KeyRequest,
    perform_key_request: F,
    key_input_event_sink: Option<&dyn KeyInputEventSink>,
) -> io::Result<ActionExecutionResult>
where
    F: FnOnce(&KeyRequest) -> io::Result<()>,
{
    // ------------------------------------------------------------
    // 先执行真实的本地键盘输入。
    // 只有这一段成功了,我们才把它视为“值得对外广播的 key event”。
    // ------------------------------------------------------------
    perform_key_request(request)?;

    // ------------------------------------------------------------
    // 发布动作是能力承诺的一部分:
    // - 没配置 sink 时,这里保持静默
    // - 配了 sink 却发布失败时,让请求显式失败,避免订阅方无感知丢事件
    // ------------------------------------------------------------
    if let Some(key_input_event_sink) = key_input_event_sink {
        key_input_event_sink.publish_key_event(request)?;
    }

    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: None,
    })
}

fn execute_paste(text: &str) -> io::Result<ActionExecutionResult> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
    enigo.text(text).map_err(to_io_error)?;

    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: None,
    })
}

fn execute_mouse_plan(plan: MouseExecutionPlan) -> io::Result<ActionExecutionResult> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
    let report = perform_mouse_plan(&mut enigo, &plan).map_err(to_io_error)?;

    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()),
    })
}

fn execute_save_file(
    frame: &SaveFileFrame,
    base_dir: Option<&Path>,
) -> io::Result<ActionExecutionResult> {
    let resolved_base_dir = match base_dir {
        Some(path) => path.to_path_buf(),
        None => default_savefile_directory()?,
    };
    let saved_path = frame.save_to_directory(&resolved_base_dir)?;
    let stdout = format!("saved file: {}\n", saved_path.display()).into_bytes();

    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout,
        stderr: Vec::new(),
        response_value_json: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyAction {
    modifiers: Vec<Key>,
    main_key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyPlanStep {
    Press(Key),
    Release(Key),
    Hold(u64),
}

fn parse_key_action(chord: &str) -> io::Result<KeyAction> {
    let mut modifiers = Vec::new();
    let mut tokens = chord
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let Some(key) = tokens.pop() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "@key payload 不能为空",
        ));
    };

    for token in tokens {
        modifiers.push(parse_modifier_key(token)?);
    }

    let main_key = parse_named_key(key).or_else(|| {
        if key.chars().count() == 1 {
            key.chars().next().map(Key::Unicode)
        } else {
            None
        }
    });

    let Some(main_key) = main_key else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("首版不支持的 @key 按键: {key}"),
        ));
    };

    Ok(KeyAction {
        modifiers,
        main_key,
    })
}

fn parse_named_key(key: &str) -> Option<Key> {
    match key.to_ascii_lowercase().as_str() {
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        "enter" | "return" => Some(Key::Return),
        "tab" => Some(Key::Tab),
        "space" => Some(Key::Space),
        "esc" | "escape" => Some(Key::Escape),
        "backspace" => Some(Key::Backspace),
        "delete" => Some(Key::Delete),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "up" => Some(Key::UpArrow),
        "down" => Some(Key::DownArrow),
        "left" => Some(Key::LeftArrow),
        "right" => Some(Key::RightArrow),
        _ => parse_modifier_key_token(key),
    }
}

/// 解析 `@key` 中的修饰键 token。
///
/// 这里和主键解析分开处理,这样:
/// - 报错文案能明确说明是“修饰键不支持”
/// - 同一个 token 也能在单键场景下当作主键使用
fn parse_modifier_key(token: &str) -> io::Result<Key> {
    parse_modifier_key_token(token).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("不支持的修饰键: {}", token.to_ascii_lowercase()),
        )
    })
}

/// 解析可作为修饰键的 token。
///
/// 设计口径:
/// - generic 名称继续保留,兼容首版行为
/// - side-specific 名称只在底层库确实有对应枚举时才暴露
/// - 这些 token 既可作为修饰键,也可在“单独按下一个修饰键”时作为主键
fn parse_modifier_key_token(token: &str) -> Option<Key> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(Key::Control),
        "left-ctrl" | "left-control" | "lctrl" | "lcontrol" => Some(Key::LControl),
        "right-ctrl" | "right-control" | "rctrl" | "rcontrol" => Some(Key::RControl),
        "shift" => Some(Key::Shift),
        "left-shift" | "lshift" => Some(Key::LShift),
        "right-shift" | "rshift" => Some(Key::RShift),
        "cmd" | "command" | "meta" | "super" => Some(Key::Meta),
        "left-cmd" | "left-command" | "left-meta" | "left-super" => Some(Key::Meta),
        "alt" => Some(Key::Alt),
        "option" | "left-alt" | "left-option" => Some(Key::Option),
        #[cfg(target_os = "macos")]
        "right-alt" | "right-option" => Some(Key::ROption),
        #[cfg(target_os = "macos")]
        "right-cmd" | "right-command" | "right-meta" | "right-super" => Some(Key::RCommand),
        _ => None,
    }
}

fn build_key_execution_plan(request: &KeyRequest) -> io::Result<Vec<KeyPlanStep>> {
    let action = parse_key_action(&request.key)?;
    Ok(build_key_steps(&action, request.mode, request.hold_ms))
}

fn build_key_steps(action: &KeyAction, mode: KeyMode, hold_ms: u64) -> Vec<KeyPlanStep> {
    let mut steps = Vec::new();

    match mode {
        KeyMode::PressRelease => {
            for modifier in &action.modifiers {
                steps.push(KeyPlanStep::Press(*modifier));
            }
            steps.push(KeyPlanStep::Press(action.main_key));
            if hold_ms > 0 {
                steps.push(KeyPlanStep::Hold(hold_ms));
            }
            steps.push(KeyPlanStep::Release(action.main_key));
            for modifier in action.modifiers.iter().rev() {
                steps.push(KeyPlanStep::Release(*modifier));
            }
        }
        KeyMode::Press => {
            for modifier in &action.modifiers {
                steps.push(KeyPlanStep::Press(*modifier));
            }
            steps.push(KeyPlanStep::Press(action.main_key));
        }
        KeyMode::Release => {
            steps.push(KeyPlanStep::Release(action.main_key));
            for modifier in action.modifiers.iter().rev() {
                steps.push(KeyPlanStep::Release(*modifier));
            }
        }
    }

    steps
}

fn perform_key_plan(enigo: &mut Enigo, plan: &[KeyPlanStep]) -> Result<(), enigo::InputError> {
    for step in plan {
        match step {
            KeyPlanStep::Press(key) => enigo.key(*key, Direction::Press)?,
            KeyPlanStep::Release(key) => enigo.key(*key, Direction::Release)?,
            KeyPlanStep::Hold(hold_ms) => thread::sleep(Duration::from_millis(*hold_ms)),
        }
    }

    Ok(())
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    let message = err.to_string();

    if looks_like_windows_uipi_permission_denied(&message) {
        return io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{message}. Windows UIPI 会阻止低完整性进程向更高完整性窗口注入输入。请让 daemon 与目标窗口处于相同或更高权限级别。"
            ),
        );
    }

    if looks_like_macos_accessibility_permission_denied(&message) {
        return io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{message}. macOS 需要为实际执行 `@key` / `@paste` / `@mouse-move` / `@mouse-button` / `@click` / `@drag` / `@wheel` 的进程授予辅助功能权限,并在授权后重启该进程。"
            ),
        );
    }

    io::Error::other(message)
}

fn looks_like_windows_uipi_permission_denied(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("blocked by uipi")
        || lower.contains("access is denied")
        || lower.contains("拒绝访问")
        || lower.contains("os error 5")
}

fn looks_like_macos_accessibility_permission_denied(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("does not have the permission to simulate input")
        || lower.contains("not trusted for accessibility")
}

fn from_process_output(output: Output) -> ActionExecutionResult {
    ActionExecutionResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: output.stderr,
        response_value_json: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[derive(Default)]
    struct FakeExecutor {
        calls: std::sync::Mutex<Vec<ControlCommand>>,
    }

    impl ControlActionExecutor for FakeExecutor {
        fn execute(
            &self,
            command: &ControlCommand,
            _shell: &str,
        ) -> io::Result<ActionExecutionResult> {
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
                &ControlCommand::Key(KeyRequest {
                    key: "F11".to_owned(),
                    hold_ms: 200,
                    mode: KeyMode::PressRelease,
                }),
                "/bin/sh",
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
            &[ControlCommand::Key(KeyRequest {
                key: "F11".to_owned(),
                hold_ms: 200,
                mode: KeyMode::PressRelease,
            })]
        );
    }

    #[test]
    fn execute_control_command_should_run_script_via_shell() {
        let executor = SystemControlActionExecutor::default();
        let (shell, script, expected_stdout) = script_test_case();
        let output = executor
            .execute(&ControlCommand::Script(script.to_owned()), shell)
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
        let request = KeyRequest {
            key: "F11".to_owned(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
        };
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
            &KeyRequest {
                key: "F11".to_owned(),
                hold_ms: 200,
                mode: KeyMode::PressRelease,
            },
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
            &KeyRequest {
                key: "F11".to_owned(),
                hold_ms: 200,
                mode: KeyMode::PressRelease,
            },
            |_| Ok(()),
            Some(&AlwaysFailingKeyInputEventSink),
        )
        .expect_err("publish failure should bubble up");

        assert!(err.to_string().contains("keyboard event publish failed"));
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
        let plan = build_key_execution_plan(&KeyRequest {
            key: "right-control+c".to_owned(),
            hold_ms: 200,
            mode: KeyMode::PressRelease,
        })
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
        let plan = build_key_execution_plan(&KeyRequest {
            key: "right-control+c".to_owned(),
            hold_ms: 200,
            mode: KeyMode::Press,
        })
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
        let plan = build_key_execution_plan(&KeyRequest {
            key: "right-control+c".to_owned(),
            hold_ms: 200,
            mode: KeyMode::Release,
        })
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
}
