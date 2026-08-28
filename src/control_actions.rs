use crate::{
    ax_action::{
        focus as ax_focus, perform_action, press as ax_press, press_sequence,
        press_with_postcondition, scroll as ax_scroll, set_value as ax_set_value,
    },
    ax_input::{send_key_with_config, type_text_with_config},
    ax_query::{
        ax_window_id_from_backend_id, capture_current_ax_window_snapshot,
        capture_default_ax_snapshot,
    },
    cancellation::CancellationToken,
    control_ax::{
        build_ax_find_response_json, build_ax_get_response_json, capture_ax_find_snapshot,
        window_activation_verified, AxFocusReport,
    },
    // Phase F-1: 三个 error_envelope wrapper helper (Cancelled / PlatformUnsupported /
    // PermissionDenied), 让手写 JSON payload 跟其它 error_code 走同一 envelope 形状。
    control_computer_act::error_envelope::{
        cancelled_envelope_json, permission_denied_envelope_json,
    },
    control_frames::{default_savefile_directory, SaveFileFrame},
    control_gui_bench::build_gui_bench_response_json,
    control_mouse::{
        build_click_plan, build_drag_plan, build_mouse_button_plan, build_mouse_move_plan,
        build_wheel_plan, perform_mouse_plan, prepare_click_request, prepare_drag_request,
        prepare_mouse_move_request, prepare_wheel_request, MouseExecutionPlan,
        PreparedMouseRequest,
    },
    control_observation::{resolve_observation_ref, resolve_observation_resource_epoch},
    control_protocol::{
        CancelRequest, ControlCommand, KeyMode, KeyRequest, KeyResponseMode, OpenAppRequest,
        PasteRequest, PasteRequestKind, WaitRequest, DEFAULT_KEY_HOLD_MS,
    },
    control_resource_lane::{with_resource_write, ResourceEpochSnapshot},
    control_web::{build_default_web_act_response_json, build_default_web_find_response_json},
    control_window::{
        execute_default_window_activate, execute_default_window_close, execute_default_window_find,
        execute_default_window_resize, resolve_unique_app_window_id,
    },
};
// platform_unsupported_envelope_json 只被 cfg(not(target_os = "macos")) 分支调用
// (linux 首次真实编译时暴露过缺失 import, 见 2026-08-28 CI 修复)。
#[cfg(not(target_os = "macos"))]
use crate::control_computer_act::error_envelope::platform_unsupported_envelope_json;
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
    fn execute(
        &self,
        command: &ControlCommand,
        shell: &str,
        cancel: Option<&CancellationToken>,
    ) -> io::Result<ActionExecutionResult>;
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
    cancel_registry: Arc<crate::cancellation::CancelRegistry>,
    /// `@key` 的送达后端 (2026-08-03 wayfinder #37)。
    ///
    /// daemon 启动时从 `[key] delivery_backend` 注入; 默认随平台
    /// (macOS = ax_press, 其他 = simulated)。
    key_delivery_backend: crate::config::KeyDeliveryBackend,
}

impl Default for SystemControlActionExecutor {
    fn default() -> Self {
        Self {
            key_input_event_sink: None,
            savefile_base_dir: None,
            cancel_registry: Arc::new(crate::cancellation::CancelRegistry::new()),
            key_delivery_backend: crate::config::KeyDeliveryBackend::default_for_platform(),
        }
    }
}

impl SystemControlActionExecutor {
    /// 创建一个会在 `@key` 成功后同步发布键盘事件的执行器。
    pub fn with_key_input_event_sink(key_input_event_sink: Arc<dyn KeyInputEventSink>) -> Self {
        Self {
            key_input_event_sink: Some(key_input_event_sink),
            savefile_base_dir: None,
            cancel_registry: Arc::new(crate::cancellation::CancelRegistry::new()),
            key_delivery_backend: crate::config::KeyDeliveryBackend::default_for_platform(),
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
            cancel_registry: Arc::new(crate::cancellation::CancelRegistry::new()),
            key_delivery_backend: crate::config::KeyDeliveryBackend::default_for_platform(),
        }
    }

    /// 显式设置 `@key` 的送达后端。
    ///
    /// daemon 启动时从 `[key] delivery_backend` 配置注入。
    pub fn with_key_delivery_backend(mut self, backend: crate::config::KeyDeliveryBackend) -> Self {
        self.key_delivery_backend = backend;
        self
    }

    /// 暴露内部 cancel_registry 引用, 让 dispatcher (zenoh_control / 控制平面)
    /// 跟 executor 共享同一 registry 实例, 避免 ticket 03 跨实例 bug。
    ///
    /// ticket 03 修法 (Phase F-3): 之前 zenoh_control.rs:240 每次请求新建
    /// `CancelRegistry::new()`, 跟 executor 内部的 cancel_registry 不是同一实例,
    /// 导致 `@cancel#seq` 找不到 in-flight seq (返回 `unknown_target_seq`)。
    /// 修法是 dispatcher 传引用, executor 暴露 accessor, 两边共享 Arc<CancelRegistry>。
    pub(crate) fn cancel_registry(&self) -> &Arc<crate::cancellation::CancelRegistry> {
        &self.cancel_registry
    }
}

impl Clone for SystemControlActionExecutor {
    fn clone(&self) -> Self {
        Self {
            cancel_registry: self.cancel_registry.clone(),
            key_input_event_sink: self.key_input_event_sink.as_ref().map(Arc::clone),
            savefile_base_dir: self.savefile_base_dir.clone(),
            key_delivery_backend: self.key_delivery_backend,
        }
    }
}

/// 直达 primitive 也必须消费 observation 记录的 PID 版本。
///
/// 无 ref 或无法解析 PID 的请求没有可靠资源归属,保持既有执行路径。
fn resource_epoch_for_ref(
    observation_id: Option<&str>,
    ref_id: Option<&str>,
) -> Option<ResourceEpochSnapshot> {
    resolve_observation_resource_epoch(observation_id?, ref_id?)
        .ok()
        .flatten()
}

fn resource_epoch_for_ax_target(
    target: &crate::control_ax::AxTarget,
) -> Option<ResourceEpochSnapshot> {
    resource_epoch_for_ref(target.observation_id.as_deref(), target.ref_id.as_deref())
}

fn resource_epoch_for_window_target(
    target: &crate::control_window::WindowCommandTarget,
) -> Option<ResourceEpochSnapshot> {
    resource_epoch_for_ref(target.observation_id.as_deref(), target.ref_id.as_deref())
}

fn resource_epoch_for_web_target(
    target: &crate::control_web::WebFindTarget,
) -> Option<ResourceEpochSnapshot> {
    resource_epoch_for_ref(
        target.observation_id.as_deref(),
        target.window_ref.as_deref(),
    )
}

/// 在统一 executor 入口解析所有直达 ref mutation 的 PID resource snapshot。
///
/// 这里是直达 primitive 的唯一路由表。新增 ref mutation 时只需在此登记,
/// 后面的 dispatch 会自动经过同一条 daemon-owned resource lane。
fn resource_epoch_for_command(command: &ControlCommand) -> Option<ResourceEpochSnapshot> {
    match command {
        ControlCommand::AxFocus(request) => request
            .target
            .as_ref()
            .and_then(resource_epoch_for_ax_target),
        ControlCommand::AxScroll(request) => resource_epoch_for_ax_target(&request.target),
        ControlCommand::AxAction(request) => resource_epoch_for_ax_target(&request.target),
        ControlCommand::AxPress(request) => resource_epoch_for_ax_target(&request.target),
        ControlCommand::AxSetValue(request) => resource_epoch_for_ax_target(&request.target),
        ControlCommand::TypeText(request) => resource_epoch_for_ax_target(&request.target),
        ControlCommand::WindowActivate(request) => {
            resource_epoch_for_window_target(&request.target)
        }
        ControlCommand::WindowClose(request) => resource_epoch_for_window_target(&request.target),
        ControlCommand::WindowResize(request) => resource_epoch_for_window_target(&request.target),
        ControlCommand::WebAct(request) => resource_epoch_for_web_target(&request.find.target),
        _ => None,
    }
}

fn execute_ref_mutation(
    snapshot: Option<ResourceEpochSnapshot>,
    dispatch: impl FnOnce() -> io::Result<ActionExecutionResult>,
) -> io::Result<ActionExecutionResult> {
    let Some(snapshot) = snapshot else {
        return dispatch();
    };
    match with_resource_write(&snapshot, dispatch) {
        Ok(result) => result,
        Err(stale) => Ok(ActionExecutionResult {
            exit_code: 64,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(
                serde_json::json!({
                    "ok": false,
                    "error_code": "stale_resource_epoch",
                    "error_message": format!(
                        "资源 {} 的 write epoch 已从 {} 变化为 {},拒绝陈旧 mutation",
                        stale.resource_key, stale.expected_epoch, stale.current_epoch
                    ),
                    "retry": {
                        "strategy": "re_observe_then_retry",
                        "hint": "重新调 @observe 获取该 PID 的最新状态后重试"
                    }
                })
                .to_string(),
            ),
        }),
    }
}

impl ControlActionExecutor for SystemControlActionExecutor {
    fn execute(
        &self,
        command: &ControlCommand,
        shell: &str,
        cancel: Option<&CancellationToken>,
    ) -> io::Result<ActionExecutionResult> {
        execute_ref_mutation(resource_epoch_for_command(command), || {
            match command {
            ControlCommand::Key(request) => {
                execute_key(
                    request,
                    self.key_input_event_sink.as_deref(),
                    Some(self.key_delivery_backend),
                )
            }
            ControlCommand::Cancel(request) => execute_cancel(request, &self.cancel_registry),
            ControlCommand::ComputerAct(request) => crate::control_computer_act::execute_computer_act(request, cancel),
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
            // Phase 1 后台任务四原语: 由 control_core 专门分支直接处理
            // (specs/rdog-task-spawn-control-plan.md), 不走 executor 兜底
            ControlCommand::Spawn(_)
            | ControlCommand::TaskStatus(_)
            | ControlCommand::TaskOutput(_)
            | ControlCommand::TaskCancel(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@spawn / @task-* 由 control_core 直接走 task registry,不应进入默认 executor 分支",
            )),
            // agent messaging Phase 3: mailbox 由 control_core 专门分支处理
            ControlCommand::AgentRegister(_)
            | ControlCommand::AgentInbox(_)
            | ControlCommand::AgentAck(_)
            | ControlCommand::AgentCard(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@agent-* 由 control_core 直接走 mailbox/card,不应进入默认 executor 分支",
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
            ControlCommand::AxPressSequence(request) => execute_ax_press_sequence(request),
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
            ControlCommand::Record(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@record-* 由 control_core 直接走 RecordingHandler,不应进入默认 executor 分支",
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
            ControlCommand::OpenApp(request) => execute_open_app(request, &SystemOpenAppCommand),
            ControlCommand::Wait(request) => execute_wait(request, cancel),
            // Composite 不应进入默认 executor 分支 (由 @computer-act dispatch_underlying 处理)
            ControlCommand::Composite(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "@computer-act Composite 由 dispatch_underlying 处理,不应进入默认 executor 分支",
            )),
        }
        })
    }
}

pub(crate) fn execute_wait(
    request: &WaitRequest,
    cancel: Option<&CancellationToken>,
) -> io::Result<ActionExecutionResult> {
    // `@wait` 让 dispatcher worker thread sleep 一段毫秒数,主要用于:
    // - `@computer-act` action=`wait` 的底层原语 (ticket 01)
    // - `@flow` 步骤间固定间隔
    // - 调试 / 节流场景
    //
    // 返回值带实际 elapsed_ms (用于 client 端 verify budget 统计)。
    // 当收到 cancellation token 时 sleep 立即返回并报 cancelled。
    let result = match cancel {
        Some(token) => crate::cancellation::sleep_cancellable(request.duration_ms, token),
        None => Ok(sleep_and_measure(request.duration_ms)),
    };

    match result {
        Ok(actual_ms) => Ok(ActionExecutionResult {
            exit_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(build_default_wait_response_json(request, actual_ms)),
        }),
        Err(()) => Ok(ActionExecutionResult {
            exit_code: 64, // 与 parse error / platform_unsupported 一致
            stdout: Vec::new(),
            stderr: Vec::new(),
            response_value_json: Some(build_cancelled_wait_response_json(request)),
        }),
    }
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

/// `@cancel#seq` 的 executor。
///
/// 命中 registry 时 signal 对应 token (后续 sleep check 会醒);
/// 不命中时返回 `unknown_target_seq` 但 cancel 命令本身仍 OK。
pub(crate) fn execute_cancel(
    request: &CancelRequest,
    registry: &crate::cancellation::CancelRegistry,
) -> io::Result<ActionExecutionResult> {
    let signaled = registry.signal(request.target_seq);
    let payload = if signaled {
        serde_json::json!({
            "ok": true,
            "dispatched_to": "@cancel#seq",
            "target_seq": request.target_seq,
            "signaled": true,
        })
    } else {
        serde_json::json!({
            "ok": false,
            "dispatched_to": "@cancel#seq",
            "target_seq": request.target_seq,
            "error_code": "unknown_target_seq",
            "error_message": format!(
                "没有 in-flight 命令的 seq={},可能已完成或从未存在",
                request.target_seq
            ),
            "evidence": {
                "target_seq": request.target_seq,
                "registry_state": "empty_or_completed",
            }
        })
    };
    Ok(ActionExecutionResult {
        exit_code: if signaled { 0 } else { 64 },
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(payload.to_string()),
    })
}

/// `execute_wait` 被取消时返回的 response JSON (走 ADR-0004 E2 envelope)。
///
/// Phase F-1: 改走 `cancelled_envelope_json()` helper, 自动补 `retry.strategy="never"` /
/// `retry.hint="..."`, 跟其它 error_code 形状一致。
/// `evidence.cancelled_at_step = "sleep_cancellable"` 标明取消点, 后续 client 或
/// agent loop 可基于此判断是否重试 (Cancelled retry=strategy=never, 不需要重试)。
pub(crate) fn build_cancelled_wait_response_json(request: &WaitRequest) -> String {
    cancelled_envelope_json(request.duration_ms)
}

/// `@open-app` 的 executor。
///
/// macOS 走 `open -a <app_name>`,等待 `wait_ms` 让 app 完成初次绘制。
/// 其他平台返回 `platform_unsupported` 错误码 (LP1 跟进跨平台)。
/// Phase F-3.5: `OpenAppCommand` trait 让 execute_open_app 可注入不同
/// `open` 命令实现 (production 走 SystemOpenAppCommand, 单测可注入 mock)。
///
/// 这解决 daemon PATH 隔离问题: smoke 改 client shell PATH 不影响 daemon 进程的
/// `Command::new("open")` 行为. 通过 trait 注入, 单测可模拟 spawn 失败场景
/// (`Err(NotFound)` 等), 验证 PermissionDenied envelope 真实路径.
pub(crate) trait OpenAppCommand: Send + Sync {
    /// 调 `open -a <app_name>` (或 mock), 返 `Output` 或 IO 错误.
    /// `Err` 走 permission_denied envelope, `Ok` 但 status != success 走 app_not_found.
    fn run(&self, app_name: &str) -> io::Result<std::process::Output>;
}

/// Production `OpenAppCommand` 实现: 调真实 `Command::new("open")`.
pub(crate) struct SystemOpenAppCommand;

impl OpenAppCommand for SystemOpenAppCommand {
    fn run(&self, app_name: &str) -> io::Result<std::process::Output> {
        use std::process::Command;
        Command::new("open").args(["-a", app_name]).output()
    }
}

pub(crate) fn execute_open_app(
    request: &OpenAppRequest,
    open_cmd: &dyn OpenAppCommand,
) -> io::Result<ActionExecutionResult> {
    let payload = open_app_payload_for_current_platform(request, open_cmd);

    // platform_unsupported / permission_denied / app_not_found 用非零 exit_code 标记;
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
fn open_app_payload_for_current_platform(
    request: &OpenAppRequest,
    open_cmd: &dyn OpenAppCommand,
) -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        return run_open_app_on_macos(request, open_cmd);
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Phase F-1: 走 platform_unsupported_envelope_json() helper, 自动补 retry.strategy
        // / retry.hint, 跟其它 error_code 形状一致。manual_only 策略告诉客户端这是
        // 平台不支持, 不要尝试 retry, 需要人工换替代动作。
        serde_json::from_str(&platform_unsupported_envelope_json(
            std::env::consts::OS,
            &request.app_name,
        ))
        .expect("envelope_json produces valid JSON")
    }
}

#[cfg(target_os = "macos")]
fn run_open_app_on_macos(
    request: &OpenAppRequest,
    open_cmd: &dyn OpenAppCommand,
) -> serde_json::Value {
    // `open -a <app_name>` 启动指定 app。wait_ms==0 跳过 sleep。
    // Phase F-3.5: 通过 trait 注入的 open_cmd 调, 让单测可注入 mock
    // 模拟 spawn 失败 (PATH 缺失等 PermissionDenied 场景)。
    let output = open_cmd.run(&request.app_name);

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
            // Phase F-1: 走 permission_denied_envelope_json() helper, 自动补
            // retry.strategy="never" / retry.hint="..." / evidence.missing_capability=null。
            serde_json::from_str(&permission_denied_envelope_json(
                &request.app_name,
                &e.to_string(),
            ))
            .expect("envelope_json produces valid JSON")
        }
    }
}

pub(crate) fn execute_script(shell: &str, script_text: &str) -> io::Result<ActionExecutionResult> {
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

pub(crate) fn execute_key(
    request: &KeyRequest,
    key_input_event_sink: Option<&dyn KeyInputEventSink>,
    delivery_backend: Option<crate::config::KeyDeliveryBackend>,
) -> io::Result<ActionExecutionResult> {
    if let Some(report) = send_key_with_config(request.clone())? {
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

    // 2026-08-03 (wayfinder #38): 配置为 ax_press 且是单字符按键时,
    // 先尝试在当前聚焦窗口 AX 树中找匹配按钮并按它; 找不到匹配按钮时
    // fallback 到 enigo 模拟按键 (历史行为)。
    if matches!(
        delivery_backend,
        Some(crate::config::KeyDeliveryBackend::AxPress)
    ) {
        if let Some(report) = try_ax_press_single_char(request)? {
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

/// 尝试用 AX press 执行一个单字符按键。
///
/// 2026-08-03 (wayfinder #38): 当 `@key` 配置为 ax_press 且按键是单字符
/// (数字 / 运算符 / 字母, 无修饰键组合)时, 在当前前台应用 (frontmost app)
/// 的 AX 树中查找 description/name 匹配该字符的 AXButton 并按它。
///
/// 返回 `Ok(None)` 表示没有匹配按钮或无法定位, 调用方应 fallback 到
/// enigo 模拟按键; 返回 `Ok(Some(report))` 表示 AX press 成功。
fn try_ax_press_single_char(
    request: &KeyRequest,
) -> io::Result<Option<crate::control_ax::KeyDeliveryReport>> {
    // 仅处理 Global 送达的单字符按键; 快捷键/修饰键组合不适用 AX press。
    if request.delivery != crate::control_protocol::KeyDelivery::Global {
        return Ok(None);
    }
    if !is_single_char_key(&request.key) {
        return Ok(None);
    }

    // 定位当前前台应用, 拿不到焦点时直接 fallback (不报错)。
    let Ok(pid) = crate::control_window::frontmost_pid() else {
        return Ok(None);
    };

    // 抓取前台应用的 AX 树 (含按钮 description/name)。
    let snapshot = match crate::control_ax::platform_capture_current_window(
        &format!("pid:{pid}/window:0"),
        &crate::control_ax::AxTreeRequest {
            depth: 8,
            max_elements: 2000,
            include_values: true,
            ..crate::control_ax::AxTreeRequest::default()
        },
    ) {
        Ok(snapshot) => snapshot,
        Err(_) => return Ok(None),
    };

    // 在当前窗口 AX 树中匹配 AXButton 且 description/name == key。
    let mut matched: Option<String> = None;
    for window in &snapshot.windows {
        find_button_matching_key(&window.elements, &request.key, &mut matched);
        if matched.is_some() {
            break;
        }
    }

    let Some(element_id) = matched else {
        return Ok(None);
    };

    // 找到匹配按钮, 用 AX press 按下。
    let target = crate::control_ax::AxTarget {
        id: Some(element_id),
        ..crate::control_ax::AxTarget::default()
    };
    let report = ax_press(&target)?;
    if !report.performed {
        return Ok(None);
    }

    Ok(Some(crate::control_ax::KeyDeliveryReport::success(
        "ax-press",
        request,
        Some(pid),
        None,
    )))
}

/// 判断按键是否为"单字符按键" (数字 / 字母 / 单个运算符字符)。
///
/// 排除修饰键组合 (Cmd+T, Shift+8 等含 `+` 的) 与命名键 (Esc, Return 等)。
fn is_single_char_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(_first) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    // 单字符 `+` 是加号键 (AX press 可匹配"加"按钮), 不是组合分隔符。
    // 组合键 (Cmd+T 等) 长度 > 1 已在上面的多字符分支排除。
    true
}

/// 在 AX 元素树中递归查找 description 或 name 等于按键字符的按钮。
fn find_button_matching_key(
    elements: &[crate::control_ax::AxElement],
    key: &str,
    matched: &mut Option<String>,
) {
    for element in elements {
        if element.role == "AXButton" {
            let desc_matches = element
                .description
                .as_deref()
                .map(|value| button_text_matches_key(value, key))
                .unwrap_or(false);
            let name_matches = element
                .name
                .as_deref()
                .map(|value| button_text_matches_key(value, key))
                .unwrap_or(false);
            if desc_matches || name_matches {
                *matched = Some(element.id.clone());
                return;
            }
        }
        find_button_matching_key(&element.children, key, matched);
        if matched.is_some() {
            return;
        }
    }
}

/// 判断按钮文本是否匹配目标按键字符。
///
/// 数字 / 字母走精确匹配 (`5` 匹配 "5"); 单字符运算符额外做语义别名匹配,
/// 因为本地化计算器 / 键盘类 App 的按钮描述可能是 "加" / "add" / "plus"
/// 而不是字面 "+"。只覆盖常见中英文别名, 避免误匹配其他含义的按钮。
fn button_text_matches_key(text: &str, key: &str) -> bool {
    if text == key {
        return true;
    }
    operator_alias_matches(text, key)
}

/// 单字符运算符的语义别名匹配 (中英文常见说法)。
fn operator_alias_matches(text: &str, key: &str) -> bool {
    match key {
        "+" => matches!(text, "加" | "加号" | "add" | "plus"),
        "-" => matches!(text, "减" | "减号" | "subtract" | "minus"),
        "*" | "×" => matches!(text, "乘" | "乘号" | "multiply" | "times"),
        "/" | "÷" => matches!(text, "除" | "除号" | "divide"),
        "=" => matches!(text, "等于" | "等号" | "equals" | "equal"),
        "." => matches!(text, "点" | "小数点" | "decimal" | "period"),
        "%" => matches!(text, "百分比" | "percent" | "mod"),
        _ => false,
    }
}

fn structured_global_key_success_response(request: &KeyRequest) -> io::Result<Option<String>> {
    // 清除类按键 (escape/backspace/delete) 即使 legacy 模式也返回结构化响应
    // 并带 continue hint: 模型用快捷键清除后容易"清除即停", hint 把它拉回
    // 主任务。非清除类按键保持 legacy 裸 0 不变 (wayfinder #36 决策 5)。
    if is_clear_key(&request.key) {
        let mut report = crate::control_ax::KeyDeliveryReport::success(
            "global-input-simulation",
            request,
            None,
            None,
        );
        report.hint = Some(crate::ax_action::CLEAR_ACTION_HINT.to_string());
        return report.to_value_json().map(Some);
    }

    if !matches!(request.response_mode, KeyResponseMode::Structured) {
        return Ok(None);
    }

    crate::control_ax::KeyDeliveryReport::success("global-input-simulation", request, None, None)
        .to_value_json()
        .map(Some)
}

/// 判断 @key 按键是否为"清除类" (escape / backspace / delete)。
///
/// 覆盖模型用快捷键清除的常见写法 (m27hs / qwen-plus stale 用 escape 清除)。
/// 注意: 单字符 `c` / `C` 不加 hint —— 文本输入场景 c 是普通字符,
/// rdog 无法区分前台 app 语义, 保守只覆盖明确的清除命名键。
fn is_clear_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "escape" | "esc" | "backspace" | "delete" | "del" | "clear"
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

pub(crate) fn execute_paste(request: &PasteRequest) -> io::Result<ActionExecutionResult> {
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

pub(crate) fn execute_mouse_plan(plan: MouseExecutionPlan) -> io::Result<ActionExecutionResult> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(to_io_error)?;
    let report = perform_mouse_plan(&mut enigo, &plan).map_err(to_io_error)?;

    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()),
    })
}

pub(crate) fn execute_prepared_mouse_request<T>(
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
    let snapshot = match (&request.observation_id, request.epoch) {
        (Some(observation_id), Some(epoch)) => {
            crate::control_ax::resolve_cached_ax_tree(observation_id, epoch)?
                .bounded_for_query(request)
        }
        (None, None) => capture_default_ax_snapshot(request)?.with_observation("@ax-tree")?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "缓存 @ax-tree 必须同时提供 observation_id 和 epoch",
            ))
        }
    };
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
    let snapshot = capture_ax_find_snapshot(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_ax_find_response_json(&snapshot, request)?),
    })
}

fn execute_ax_get(request: &crate::control_ax::AxGetRequest) -> io::Result<ActionExecutionResult> {
    let snapshot = match (request.target.observation_id.as_deref(), request.epoch) {
        (Some(observation_id), Some(epoch)) => {
            let snapshot = crate::control_ax::resolve_cached_ax_get(
                observation_id,
                request.target.ref_id.as_deref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "缓存 @ax-get 必须使用 observation-local ref",
                    )
                })?,
                epoch,
            )?;
            let target_id =
                crate::control_ax::resolve_target_id_in_snapshot(&snapshot, &request.target)
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            serde_json::json!({
                                "ok": false,
                                "error_code": "target_not_found",
                                "error_message": "缓存 observation 中不存在请求目标",
                                "retry": {"strategy": "re_observe_then_retry"}
                            })
                            .to_string(),
                        )
                    })?;
            snapshot.bounded_for_query_with_target(&request.tree_request(), Some(&target_id))
        }
        (None, None) => capture_ax_get_snapshot_with(
            request,
            capture_default_ax_snapshot,
            capture_current_ax_window_snapshot,
        )?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "缓存 @ax-get 必须同时提供 target.observation_id 和 epoch",
            ))
        }
    };
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(build_ax_get_response_json(&snapshot, request)?),
    })
}

fn capture_ax_get_snapshot_with(
    request: &crate::control_ax::AxGetRequest,
    capture_global: impl FnOnce(
        &crate::control_ax::AxTreeRequest,
    ) -> io::Result<crate::control_ax::AxSnapshot>,
    capture_window: impl FnOnce(
        &str,
        &crate::control_ax::AxTreeRequest,
    ) -> io::Result<crate::control_ax::AxSnapshot>,
) -> io::Result<crate::control_ax::AxSnapshot> {
    let tree_request = request.tree_request();
    match target_window_id_from_ax_target(Some(&request.target))? {
        Some(window_id) => capture_window(&window_id, &tree_request),
        None => capture_global(&tree_request),
    }
}

fn execute_ax_press(
    request: &crate::control_ax::AxPressRequest,
) -> io::Result<ActionExecutionResult> {
    let response_value_json = match request.postcondition {
        Some(_) => press_with_postcondition(request)?.to_value_json()?,
        None => ax_press(&request.target)?.to_value_json()?,
    };
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(response_value_json),
    })
}

fn execute_ax_press_sequence(
    request: &crate::control_ax::AxPressSequenceRequest,
) -> io::Result<ActionExecutionResult> {
    // resolve_app 由调用方注入: 这既是 app selector 的解析入口,
    // 也是 press_sequence 的可测试 seam (单测传入 stub 即可)。
    let report = press_sequence(request, resolve_unique_app_window_id);
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
    let report = perform_action(request)?;
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
    let report = ax_set_value(request)?;
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
    execute_ax_focus_with(request, execute_default_window_activate, ax_focus)
}

fn execute_ax_focus_with(
    request: &crate::control_ax::AxFocusRequest,
    activate_window: impl FnOnce(
        &crate::control_window::WindowActivateRequest,
    ) -> io::Result<crate::control_window::WindowActionReport>,
    focus_ax: impl FnOnce(&crate::control_ax::AxFocusRequest) -> io::Result<AxFocusReport>,
) -> io::Result<ActionExecutionResult> {
    let mut activation = None;
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
            guard: None,
            verify: crate::control_window::WindowActivateVerify::default(),
        };
        let activation_report = activate_window(&activation_request)?;
        if !window_activation_verified(&activation_report) {
            let report = AxFocusReport::activation_failed(activation_report);
            return Ok(ActionExecutionResult {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                response_value_json: Some(report.to_value_json()?),
            });
        }
        activation = Some(activation_report);
    }

    let mut report = focus_ax(request)?;
    if let Some(activation) = activation {
        report = report.with_activation(activation);
    }
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
        return Ok(ax_window_id_from_backend_id(id).map(str::to_owned));
    }

    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        let entry = resolve_observation_ref(observation_id, ref_id)?;
        return Ok(ax_window_id_from_backend_id(&entry.backend_id).map(str::to_owned));
    }

    Ok(None)
}

fn execute_ax_scroll(
    request: &crate::control_ax::AxScrollRequest,
) -> io::Result<ActionExecutionResult> {
    let report = ax_scroll(request)?;
    Ok(ActionExecutionResult {
        exit_code: 0,
        stdout: Vec::new(),
        stderr: Vec::new(),
        response_value_json: Some(report.to_value_json()?),
    })
}

pub(crate) fn execute_type_text(
    request: &crate::control_ax::TypeTextRequest,
) -> io::Result<ActionExecutionResult> {
    let report = type_text_with_config(request.clone())?;
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
    // 主键可能是字面 `+` (`@key:"+"` / `@key:"Cmd++"`)。
    // 此时 chord 以 `+` 结尾, 先把结尾的 `+` 切出来作为主键,
    // 剩余部分只解析修饰符, 避免 `split('+')` 把纯 `+` 拆成空 token。
    let (rest, literal_plus) = match chord.strip_suffix('+') {
        Some(rest) => (rest, true),
        None => (chord, false),
    };

    let mut modifiers = Vec::new();
    let mut tokens = rest
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let main_key = if literal_plus {
        // 字面 `+` 主键: 剩余 token 全部是修饰符 (`Cmd++` -> Cmd + `+`)
        for token in tokens {
            modifiers.push(parse_modifier_key(token)?);
        }
        Key::Unicode('+')
    } else {
        let Some(key) = tokens.pop() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "@key payload 不能为空",
            ));
        };

        for token in tokens {
            modifiers.push(parse_modifier_key(token)?);
        }

        parse_named_key(key)
            .or_else(|| {
                if key.chars().count() == 1 {
                    key.chars().next().map(Key::Unicode)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("首版不支持的 @key 按键: {key}"),
                )
            })?
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
