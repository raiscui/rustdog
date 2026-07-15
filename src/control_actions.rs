use crate::{
    control_ax::{
        build_ax_find_response_json, build_ax_get_response_json, capture_default_ax_snapshot,
        perform_default_ax_action, perform_default_ax_focus, perform_default_ax_press,
        perform_default_ax_scroll, perform_default_ax_set_value, perform_default_key_delivery,
        perform_default_type_text,
    },
    control_frames::{default_savefile_directory, SaveFileFrame},
    control_gui_bench::build_gui_bench_response_json,
    control_mouse::{
        build_click_plan, build_drag_plan, build_mouse_button_plan, build_mouse_move_plan,
        build_wheel_plan, perform_mouse_plan, prepare_click_request, prepare_drag_request,
        prepare_mouse_move_request, prepare_wheel_request, MouseExecutionPlan,
        PreparedMouseRequest,
    },
    control_observation::resolve_observation_ref,
    control_protocol::{
        ControlCommand, KeyMode, KeyRequest, KeyResponseMode, PasteRequest, PasteRequestKind,
    OpenAppRequest, WaitRequest,
        DEFAULT_KEY_HOLD_MS,
    },
    control_web::{build_default_web_act_response_json, build_default_web_find_response_json},
    control_window::{
        execute_default_window_activate, execute_default_window_close, execute_default_window_find,
        execute_default_window_resize,
    },
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
            ControlCommand::Paste(request) => execute_paste(request),
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
            ControlCommand::MouseMove(request) => execute_prepared_mouse_request(
                prepare_mouse_move_request(request)?,
                build_mouse_move_plan,
            ),
            ControlCommand::MouseButton(request) => {
                execute_mouse_plan(build_mouse_button_plan(request)?)
            }
            ControlCommand::Click(request) => {
                execute_prepared_mouse_request(prepare_click_request(request)?, build_click_plan)
            }
            ControlCommand::Drag(request) => {
                execute_prepared_mouse_request(prepare_drag_request(request)?, build_drag_plan)
            }
            ControlCommand::Wheel(request) => {
                execute_prepared_mouse_request(prepare_wheel_request(request)?, build_wheel_plan)
            }
            ControlCommand::AxTree(request) => execute_ax_tree(request),
            ControlCommand::AxFind(request) => execute_ax_find(request),
            ControlCommand::AxGet(request) => execute_ax_get(request),
            ControlCommand::AxFocus(request) => execute_ax_focus(request),
            ControlCommand::AxScroll(request) => execute_ax_scroll(request),
            ControlCommand::AxAction(request) => execute_ax_action(request),
            ControlCommand::AxPress(request) => execute_ax_press(request),
            ControlCommand::AxSetValue(request) => execute_ax_set_value(request),
            ControlCommand::TypeText(request) => execute_type_text(request),
            ControlCommand::WindowFind(request) => execute_window_find(request),
            ControlCommand::WindowActivate(request) => execute_window_activate(request),
            ControlCommand::WindowClose(request) => execute_window_close(request),
            ControlCommand::WindowResize(request) => execute_window_resize(request),
            ControlCommand::WebFind(request) => execute_web_find(request),
            ControlCommand::WebAct(request) => execute_web_act(request),
            ControlCommand::GuiBench(request) => execute_gui_bench(request),
            ControlCommand::Bootstrap(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@bootstrap 是只读 preflight facade,由 control_core 直接组合 capabilities / observe,不应进入默认 executor 分支",
            )),
            ControlCommand::Flow(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@flow 由 control_core 直接返回多 frame outcome,不应进入默认 executor 分支",
            )),
            ControlCommand::Capabilities => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@capabilities 由 control_core 直接生成能力报告,不应进入默认 executor 分支",
            )),
            ControlCommand::Observe(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@observe 是只读 observation facade,由 control_core 直接生成 bundle,不应进入默认 executor 分支",
            )),
            ControlCommand::SelectorGet(_)
            | ControlCommand::SelectorResolve(_)
            | ControlCommand::SelectorRefind(_) => {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "@selector-get / @selector-resolve / @selector-refind 由 control_core 直接读取 observation selector state,不应进入默认 executor 分支",
                ))
            }
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
            ControlCommand::OpenApp(request) => execute_open_app(request),
            ControlCommand::Wait(request) => execute_wait(request),
        }
    }
}

fn execute_wait(request: &WaitRequest) -> io::Result<ActionExecutionResult> {
    // `@wait` 让 dispatcher worker thread sleep 一段毫秒数,主要用于:
    // - `@computer-act` action=`wait` 的底层原语 (ticket 01)
    // - `@flow` 步骤间固定间隔
    // - 调试 / 节流场景
    //
    // 返回值带实际 elapsed_ms (用于 client 端 verify budget 统计)。
    let actual_ms = sleep_and_measure(request.duration_ms);
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_default_wait_response_json(request, actual_ms)),
    })
}

/// 让 dispatcher worker thread 真正 sleep 的辅助函数。
///
/// 拆出来是为了让 `build_default_wait_response_json` 保持纯函数形态,
/// 方便后续在测试里独立验证 elapsed_ms 的换算语义 (u64 ms 截断)。
fn sleep_and_measure(duration_ms: u64) -> u64 {
    use std::time::Instant;
    let start = Instant::now();
    if duration_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(duration_ms));
    }
    start.elapsed().as_millis() as u64
}

/// `wait` 的默认 response JSON 形状。
///
/// 跟 `control_web::build_default_web_act_response_json` 的 `pub fn` 模式对齐,
/// 后续 ticket 18 (density/trace) 扩展 envelope 时只改这里一处即可。
pub(crate) fn build_default_wait_response_json(request: &WaitRequest, actual_ms: u64) -> String {
    serde_json::json!({
        "ok": true,
        "dispatched_to": "@wait",
        "requested_duration_ms": request.duration_ms,
        "duration_ms": actual_ms,
    })
    .to_string()
}

/// `@open-app` 的 executor。
///
/// macOS 走 `open -a <app_name>`,等待 `wait_ms` 让 app 完成初次绘制。
/// 其他平台返回 `platform_unsupported` 错误码 (LP1 跟进跨平台)。
fn execute_open_app(request: &OpenAppRequest) -> io::Result<ActionExecutionResult> {
    let payload = open_app_payload_for_current_platform(request);

    // platform_unsupported / permission_denied 用非零 exit_code 标记;
    // 成功路径用 0。
    let exit_code = if payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        0
    } else {
        64 // 与现有 parse error 同 code
    };

    Ok(ActionExecutionResult {
        exit_code,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(payload.to_string()),
    })
}

/// 根据当前平台返回对应的 `@open-app` 响应 JSON。
///
/// 拆出来便于单测 (未来 ticket 02 的 smoke 已经在 daemon 跑过,这里纯函数
/// 保证 macOS / 非 macOS 两个分支都被覆盖)。
fn open_app_payload_for_current_platform(request: &OpenAppRequest) -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        return run_open_app_on_macos(request);
    }

    #[cfg(not(target_os = "macos"))]
    {
        serde_json::json!({
            "ok": false,
            "error_code": "platform_unsupported",
            "error_message": "@open-app 是 macOS-only 的本轮实现;Linux/Windows 见 LATER_PLANS LP1",
            "evidence": {
                "target_os": std::env::consts::OS,
                "app_name": request.app_name,
            }
        })
    }
}

#[cfg(target_os = "macos")]
fn run_open_app_on_macos(request: &OpenAppRequest) -> serde_json::Value {
    use std::process::Command;

    // `open -a <app_name>` 启动指定 app。wait_ms==0 跳过 sleep。
    let output = Command::new("open").args(["-a", &request.app_name]).output();

    match output {
        Ok(out) if out.status.success() => {
            if request.wait_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(request.wait_ms));
            }
            serde_json::json!({
                "ok": true,
                "dispatched_to": "@open-app",
                "app_name": request.app_name,
                "wait_ms": request.wait_ms,
            })
        }
        Ok(out) => {
            // `open` 自己退出非 0 (典型: app not found)
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            serde_json::json!({
                "ok": false,
                "error_code": "app_not_found",
                "error_message": format!("`open -a {}` 退出码 {:?}", request.app_name, out.status.code()),
                "evidence": {
                    "app_name": request.app_name,
                    "exit_code": out.status.code(),
                    "stderr": stderr,
                }
            })
        }
        Err(e) => {
            // 启动 `open` 命令本身失败 (PATH 缺失等)
            serde_json::json!({
                "ok": false,
                "error_code": "permission_denied",
                "error_message": format!("无法执行 `open` 命令: {e}"),
                "evidence": {
                    "app_name": request.app_name,
                    "io_error": e.to_string(),
                }
            })
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
    if let Some(report) = perform_default_key_delivery(request)? {
        if let Some(key_input_event_sink) = key_input_event_sink {
            key_input_event_sink.publish_key_event(request)?;
        }
        return Ok(ActionExecutionResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(report.to_value_json()?),
        });
    }

    let mut result = execute_key_with_dependencies(
        request,
        |request| {
            let key_plan = build_key_execution_plan(request)?;
            let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
            perform_key_plan(&mut enigo, &key_plan).map_err(to_io_error)
        },
        key_input_event_sink,
    )?;

    result.response_value_json = structured_global_key_success_response(request)?;

    Ok(result)
}

fn structured_global_key_success_response(request: &KeyRequest) -> io::Result<Option<String>> {
    if !matches!(request.response_mode, KeyResponseMode::Structured) {
        return Ok(None);
    }

    crate::control_ax::KeyDeliveryReport::success("global-input-simulation", request, None, None)
        .to_value_json()
        .map(Some)
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct PasteReport {
    kind: &'static str,
    delivery: &'static str,
    delivered_via: &'static str,
    used_hotkey: bool,
    used_keyboard: bool,
    requires_focus: bool,
    performed: bool,
    status: &'static str,
}

impl PasteReport {
    fn hotkey_success(delivered_via: &'static str) -> Self {
        Self {
            kind: "paste",
            delivery: "global-hotkey",
            delivered_via,
            used_hotkey: true,
            used_keyboard: true,
            requires_focus: true,
            performed: true,
            status: "ok",
        }
    }

    fn to_value_json(&self) -> io::Result<String> {
        serde_json::to_string(self)
            .map_err(|err| io::Error::other(format!("paste response 序列化失败: {err}")))
    }
}

fn execute_paste(request: &PasteRequest) -> io::Result<ActionExecutionResult> {
    execute_paste_with_dependencies(request, perform_paste_hotkey, perform_legacy_paste_text)
}

fn execute_paste_with_dependencies<FH, FT>(
    request: &PasteRequest,
    perform_hotkey: FH,
    perform_text: FT,
) -> io::Result<ActionExecutionResult>
where
    FH: FnOnce(&KeyRequest) -> io::Result<()>,
    FT: FnOnce(&str) -> io::Result<()>,
{
    match &request.kind {
        PasteRequestKind::GlobalHotkey => {
            let key_request = KeyRequest::legacy(
                platform_paste_shortcut(),
                DEFAULT_KEY_HOLD_MS,
                KeyMode::PressRelease,
            );
            perform_hotkey(&key_request)?;

            Ok(ActionExecutionResult {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: Some(
                    PasteReport::hotkey_success(platform_paste_delivered_via()).to_value_json()?,
                ),
            })
        }
        PasteRequestKind::LegacyTextInjection(text) => {
            perform_text(text)?;

            Ok(ActionExecutionResult {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: None,
            })
        }
    }
}

fn perform_paste_hotkey(key_request: &KeyRequest) -> io::Result<()> {
    let key_plan = build_key_execution_plan(key_request)?;
    let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
    perform_key_plan(&mut enigo, &key_plan).map_err(to_io_error)
}

fn perform_legacy_paste_text(text: &str) -> io::Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
    enigo.text(text).map_err(to_io_error)
}

#[cfg(target_os = "macos")]
fn platform_paste_shortcut() -> &'static str {
    "cmd+v"
}

#[cfg(not(target_os = "macos"))]
fn platform_paste_shortcut() -> &'static str {
    "ctrl+v"
}

#[cfg(target_os = "macos")]
fn platform_paste_delivered_via() -> &'static str {
    "cmd-v"
}

#[cfg(not(target_os = "macos"))]
fn platform_paste_delivered_via() -> &'static str {
    "ctrl-v"
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

fn execute_prepared_mouse_request<T>(
    prepared: PreparedMouseRequest<T>,
    build_plan: impl FnOnce(&T) -> io::Result<MouseExecutionPlan>,
) -> io::Result<ActionExecutionResult> {
    match prepared {
        PreparedMouseRequest::Ready {
            request,
            target_resolution,
        } => {
            let plan = build_plan(&request)?;
            execute_mouse_plan_with_target_resolution(plan, target_resolution)
        }
        PreparedMouseRequest::NoAction {
            response_value_json,
        } => Ok(ActionExecutionResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(response_value_json),
        }),
    }
}

fn execute_mouse_plan_with_target_resolution(
    plan: MouseExecutionPlan,
    target_resolution: Option<serde_json::Value>,
) -> io::Result<ActionExecutionResult> {
    let mut result = execute_mouse_plan(plan)?;
    if let Some(target_resolution) = target_resolution {
        let Some(response_json) = result.response_value_json.take() else {
            return Ok(result);
        };
        let mut value = serde_json::from_str::<serde_json::Value>(&response_json)
            .map_err(|err| io::Error::other(format!("mouse response JSON 解析失败: {err}")))?;
        value["target_resolution"] = target_resolution;
        result.response_value_json = Some(value.to_string());
    }
    Ok(result)
}

fn execute_ax_tree(
    request: &crate::control_ax::AxTreeRequest,
) -> io::Result<ActionExecutionResult> {
    let snapshot = capture_default_ax_snapshot(request)?.with_observation("@ax-tree")?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(snapshot.to_tree_value_json()?),
    })
}

fn execute_ax_find(
    request: &crate::control_ax::AxFindRequest,
) -> io::Result<ActionExecutionResult> {
    let snapshot = capture_default_ax_snapshot(&request.tree)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_ax_find_response_json(&snapshot, request)?),
    })
}

fn execute_ax_get(request: &crate::control_ax::AxGetRequest) -> io::Result<ActionExecutionResult> {
    let snapshot = capture_default_ax_snapshot(&request.tree_request())?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_ax_get_response_json(&snapshot, request)?),
    })
}

fn execute_ax_press(
    request: &crate::control_ax::AxPressRequest,
) -> io::Result<ActionExecutionResult> {
    let report = perform_default_ax_press(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn execute_ax_action(
    request: &crate::control_ax::AxActionRequest,
) -> io::Result<ActionExecutionResult> {
    let report = perform_default_ax_action(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn execute_ax_set_value(
    request: &crate::control_ax::AxSetValueRequest,
) -> io::Result<ActionExecutionResult> {
    let report = perform_default_ax_set_value(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn execute_ax_focus(
    request: &crate::control_ax::AxFocusRequest,
) -> io::Result<ActionExecutionResult> {
    if request.activate {
        let window_id = match &request.window_id {
            Some(window_id) => Some(window_id.clone()),
            None => target_window_id_from_ax_target(request.target.as_ref())?,
        };
        let Some(window_id) = window_id else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "@ax-focus activate:true 目前需要 `window_id` 或可回推出 window_id 的 target.id",
            ));
        };
        let activation_request = crate::control_window::WindowActivateRequest {
            target: crate::control_window::WindowCommandTarget {
                window_id: Some(window_id),
                ..crate::control_window::WindowCommandTarget::default()
            },
            recipe: Some("to_interact".to_owned()),
            steps: Vec::new(),
            allow_ambiguous: false,
            select: None,
        };
        execute_default_window_activate(&activation_request)?;
    }

    let report = perform_default_ax_focus(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn target_window_id_from_ax_target(
    target: Option<&crate::control_ax::AxTarget>,
) -> io::Result<Option<String>> {
    let Some(target) = target else {
        return Ok(None);
    };

    if let Some(id) = target.id.as_deref() {
        return Ok(id.split("/path:").next().map(str::to_owned));
    }

    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        let entry = resolve_observation_ref(observation_id, ref_id)?;
        return Ok(entry.backend_id.split("/path:").next().map(str::to_owned));
    }

    Ok(None)
}

fn execute_ax_scroll(
    request: &crate::control_ax::AxScrollRequest,
) -> io::Result<ActionExecutionResult> {
    let report = perform_default_ax_scroll(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn execute_type_text(
    request: &crate::control_ax::TypeTextRequest,
) -> io::Result<ActionExecutionResult> {
    let report = perform_default_type_text(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

fn execute_window_find(
    request: &crate::control_window::WindowFindRequest,
) -> io::Result<ActionExecutionResult> {
    let response = execute_default_window_find(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(response.to_value_json()?),
    })
}

fn execute_window_activate(
    request: &crate::control_window::WindowActivateRequest,
) -> io::Result<ActionExecutionResult> {
    let response = execute_default_window_activate(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(response.to_value_json()?),
    })
}

fn execute_window_close(
    request: &crate::control_window::WindowCloseRequest,
) -> io::Result<ActionExecutionResult> {
    let response = execute_default_window_close(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(response.to_value_json()?),
    })
}

fn execute_window_resize(
    request: &crate::control_window::WindowResizeRequest,
) -> io::Result<ActionExecutionResult> {
    let response = execute_default_window_resize(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(response.to_value_json()?),
    })
}

fn execute_web_find(
    request: &crate::control_web::WebFindRequest,
) -> io::Result<ActionExecutionResult> {
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_default_web_find_response_json(request)?),
    })
}

fn execute_web_act(
    request: &crate::control_web::WebActRequest,
) -> io::Result<ActionExecutionResult> {
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_default_web_act_response_json(request)?),
    })
}

fn execute_gui_bench(
    request: &crate::control_gui_bench::GuiBenchRequest,
) -> io::Result<ActionExecutionResult> {
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_gui_bench_response_json(request)?),
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
mod tests;
