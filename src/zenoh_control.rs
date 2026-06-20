use std::{
    collections::HashSet,
    io::{self, IsTerminal, Stdin},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(unix)]
use termios::{tcflush, TCIFLUSH};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::Console::{FlushConsoleInputBuffer, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE},
};

use zenoh::Wait;

mod client_pty;
mod daemon_bridge;
mod session_payload;
mod target_resolve;

use self::client_pty::{
    build_client_session_bridge, close_client_session_bridge, execute_remote_request,
    run_client_pty_attach_over_session_bridge_owned, run_client_pty_over_session_bridge_owned,
    run_client_pty_over_session_bridge_tty, ZenohClientSessionBridge,
};
use self::daemon_bridge::open_daemon_session_bridge;
use self::session_payload::{parse_session_bridge_request, parse_session_open_payload};
use self::target_resolve::{
    acquire_daemon_name_guard, ensure_unique_daemon_name, log_target_if_changed, resolve_target,
    ResolvedTarget,
};

use crate::{
    config::{KeyInputEventsConfig, ObservationConfig},
    control_actions::{KeyInputEventSink, SystemControlActionExecutor},
    control_client_input::ControlStdinAction,
    control_core::{parse_and_execute_control_line, render_protocol_error_response},
    control_display::ControlResponseDisplay,
    control_frames::{default_savefile_directory, ControlFrame},
    control_observation::initialize_durable_observation_state,
    control_protocol::{
        parse_control_line, ControlCommand, ControlParseResult, KeyMode, KeyRequest,
    },
    control_session::{route_line_control_result_frame, ControlPeerSession},
    zenoh_identity::{
        build_alive_key, build_alive_key_with_root, build_control_key, build_control_key_with_root,
        build_key_input_key, build_session_to_control_key_with_root, KEYEXPR_ROOT,
        LEGACY_KEYEXPR_ROOT,
    },
    zenoh_runtime,
};

/// daemon 侧运行时所需的最小参数。
#[derive(Debug, Clone)]
pub struct ZenohDaemonRuntimeConfig {
    pub namespace: String,
    pub daemon_name: String,
    pub listen_endpoints: Vec<String>,
    pub request_timeout_ms: u64,
    pub startup_guard_window_ms: u64,
    pub key_input_events: KeyInputEventsConfig,
    pub observation: ObservationConfig,
}

pub fn run_router_daemon(config: ZenohDaemonRuntimeConfig, shell: &str) -> io::Result<()> {
    let alive_key = build_alive_key(&config.namespace, &config.daemon_name);
    let control_key = build_control_key(&config.namespace, &config.daemon_name);
    let legacy_alive_key =
        build_alive_key_with_root(LEGACY_KEYEXPR_ROOT, &config.namespace, &config.daemon_name);
    let legacy_control_key =
        build_control_key_with_root(LEGACY_KEYEXPR_ROOT, &config.namespace, &config.daemon_name);
    let member_id = crate::zenoh_identity::member_id_from_daemon_name(&config.daemon_name);
    initialize_durable_observation_state(
        &config.observation,
        Some(&config.namespace),
        &config.daemon_name,
    )?;
    let _name_guard = acquire_daemon_name_guard(&config.namespace, &config.daemon_name)?;

    let session = zenoh_runtime::open_router_session(&config.listen_endpoints)?;
    ensure_unique_daemon_name(
        &session,
        &config.namespace,
        &config.daemon_name,
        Duration::from_millis(config.startup_guard_window_ms),
    )?;

    let _token = session
        .liveliness()
        .declare_token(&alive_key)
        .wait()
        .map_err(to_io_error)?;
    let _legacy_token = session
        .liveliness()
        .declare_token(&legacy_alive_key)
        .wait()
        .map_err(to_io_error)?;
    let key_input_event_publisher = declare_key_input_event_publisher(&session, &config)?;
    let queryable = session
        .declare_queryable(&control_key)
        .complete(true)
        .wait()
        .map_err(to_io_error)?;
    let legacy_queryable = session
        .declare_queryable(&legacy_control_key)
        .complete(true)
        .wait()
        .map_err(to_io_error)?;
    let key_input_event_key = key_input_event_publisher
        .as_ref()
        .map(|publisher| publisher.keyexpr.clone())
        .unwrap_or_else(|| "<disabled>".to_owned());
    let executor = build_router_control_executor(key_input_event_publisher);
    let active_session_bridges = Arc::new(Mutex::new(HashSet::new()));

    log::info!(
        "zenoh router daemon ready: namespace={}, service_name(daemon_name)={}, member_id={}, alive_key={}, control_key={}, key_input_event_key={}, listen_endpoints={:?}, request_timeout_ms={}",
        config.namespace,
        config.daemon_name,
        member_id,
        alive_key,
        control_key,
        key_input_event_key,
        config.listen_endpoints,
        config.request_timeout_ms
    );

    let legacy_session = session.clone();
    let legacy_namespace = config.namespace.clone();
    let legacy_shell = shell.to_owned();
    let legacy_executor = executor.clone();
    let legacy_active_session_bridges = Arc::clone(&active_session_bridges);
    let legacy_control_key_for_reply = legacy_control_key.clone();
    thread::spawn(move || {
        while let Ok(query) = legacy_queryable.recv() {
            if let Err(err) = handle_daemon_control_query(
                &legacy_session,
                LEGACY_KEYEXPR_ROOT,
                &legacy_namespace,
                &legacy_shell,
                &legacy_control_key_for_reply,
                &legacy_executor,
                &legacy_active_session_bridges,
                query,
            ) {
                log::warn!("legacy Zenoh control query failed: {err}");
            }
        }
    });

    while let Ok(query) = queryable.recv() {
        handle_daemon_control_query(
            &session,
            KEYEXPR_ROOT,
            &config.namespace,
            shell,
            &control_key,
            &executor,
            &active_session_bridges,
            query,
        )?;
    }

    Err(io::Error::other("Zenoh control queryable channel closed"))
}

fn handle_daemon_control_query(
    session: &zenoh::Session,
    keyexpr_root: &str,
    namespace: &str,
    shell: &str,
    control_key: &str,
    executor: &SystemControlActionExecutor,
    active_session_bridges: &Arc<Mutex<HashSet<String>>>,
    query: zenoh::query::Query,
) -> io::Result<()> {
    let payload = query.payload().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Zenoh control query 缺少 payload",
        )
    })?;
    let payload = payload.try_to_string().map_err(to_io_error)?;
    if let Some(session_id) = parse_session_open_payload(payload.as_ref())? {
        let should_open = {
            let mut bridges = active_session_bridges
                .lock()
                .expect("active_session_bridges lock should work");
            bridges.insert(session_id.clone())
        };
        if should_open {
            open_daemon_session_bridge(
                session,
                keyexpr_root,
                namespace,
                &session_id,
                shell,
                executor.clone(),
                Arc::clone(active_session_bridges),
            )?;
        }
        query
            .reply(control_key.to_owned(), "@response 0")
            .wait()
            .map_err(to_io_error)?;
        return Ok(());
    }
    let request = parse_session_bridge_request(payload.as_ref())?;
    if let Some(response) = reject_session_channel_only_legacy_query(&request.line) {
        if let Some(session_id) = request.session_id.as_deref() {
            let outcome =
                crate::control_frames::ControlExecutionOutcome::from_response_line(response);
            publish_outcome_to_session_channel(
                session,
                keyexpr_root,
                namespace,
                session_id,
                &outcome,
            )?;
            query
                .reply(control_key.to_owned(), "@response 0")
                .wait()
                .map_err(to_io_error)?;
            return Ok(());
        }

        query
            .reply(control_key.to_owned(), response)
            .wait()
            .map_err(to_io_error)?;
        return Ok(());
    }
    let outcome = parse_and_execute_control_line(request.line.as_str(), shell, executor);

    if let Some(session_id) = request.session_id.as_deref() {
        publish_outcome_to_session_channel(session, keyexpr_root, namespace, session_id, &outcome)?;
        query
            .reply(control_key.to_owned(), "@response 0")
            .wait()
            .map_err(to_io_error)?;
    } else {
        query
            .reply(control_key.to_owned(), outcome.to_multiline_wire_payload())
            .wait()
            .map_err(to_io_error)?;
    }

    Ok(())
}

struct ZenohKeyInputEventPublisher {
    publisher: zenoh::pubsub::Publisher<'static>,
    keyexpr: String,
    namespace: String,
    daemon_name: String,
}

impl KeyInputEventSink for ZenohKeyInputEventPublisher {
    fn publish_key_event(&self, request: &KeyRequest) -> io::Result<()> {
        let payload = render_key_input_event_payload(&self.namespace, &self.daemon_name, request);
        self.publisher.put(payload).wait().map_err(to_io_error)?;
        Ok(())
    }
}

fn declare_key_input_event_publisher(
    session: &zenoh::Session,
    config: &ZenohDaemonRuntimeConfig,
) -> io::Result<Option<ZenohKeyInputEventPublisher>> {
    if !config.key_input_events.enabled {
        return Ok(None);
    }

    let keyexpr = resolve_key_input_event_keyexpr(
        &config.namespace,
        &config.daemon_name,
        &config.key_input_events,
    );
    let publisher = session
        .declare_publisher(keyexpr.clone())
        .wait()
        .map_err(to_io_error)?;

    Ok(Some(ZenohKeyInputEventPublisher {
        publisher,
        keyexpr,
        namespace: config.namespace.clone(),
        daemon_name: config.daemon_name.clone(),
    }))
}

fn resolve_key_input_event_keyexpr(
    namespace: &str,
    daemon_name: &str,
    config: &KeyInputEventsConfig,
) -> String {
    let configured = config.keyexpr.trim();
    if configured.is_empty() {
        build_key_input_key(namespace, daemon_name)
    } else {
        configured.to_owned()
    }
}

fn build_router_control_executor(
    key_input_event_publisher: Option<ZenohKeyInputEventPublisher>,
) -> SystemControlActionExecutor {
    match key_input_event_publisher {
        Some(key_input_event_publisher) => SystemControlActionExecutor::with_key_input_event_sink(
            Arc::new(key_input_event_publisher),
        ),
        None => SystemControlActionExecutor::default(),
    }
}

fn render_key_input_event_payload(
    namespace: &str,
    daemon_name: &str,
    request: &KeyRequest,
) -> String {
    let member_id = crate::zenoh_identity::member_id_from_daemon_name(daemon_name);
    let key = escape_json_string(&request.key);
    let mode = render_key_mode_name(request.mode);
    let executed_at_ms = current_unix_epoch_millis();

    format!(
        "{{\"event\":\"key_input\",\"namespace\":\"{}\",\"daemon_name\":\"{}\",\"member_id\":\"{}\",\"key\":\"{key}\",\"hold_ms\":{},\"mode\":\"{mode}\",\"executed_at_ms\":{executed_at_ms}}}",
        escape_json_string(namespace),
        escape_json_string(daemon_name),
        escape_json_string(member_id),
        request.hold_ms,
    )
}

fn render_key_mode_name(mode: KeyMode) -> &'static str {
    match mode {
        KeyMode::PressRelease => "press_release",
        KeyMode::Press => "press",
        KeyMode::Release => "release",
    }
}

fn current_unix_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
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

pub fn run_client_control(
    namespace: String,
    target_name: Option<String>,
    router_entrypoints: Vec<String>,
    request_timeout_ms: u64,
) -> io::Result<()> {
    let router_entrypoints = zenoh_runtime::resolve_client_connect_endpoints(
        &router_entrypoints,
        Duration::from_millis(request_timeout_ms),
    )?;
    let mut session = zenoh_runtime::open_client_session(&router_entrypoints)?;
    let stdin = std::io::stdin();
    let display = ControlResponseDisplay::from_stdio();
    let save_dir = default_savefile_directory()?;
    let mut current_target = resolve_target(
        &session,
        &namespace,
        target_name.as_deref(),
        Duration::from_millis(request_timeout_ms),
    )?;
    log::info!(
        "zenoh client control target selected: service_name(daemon_name)={}, member_id={}, control_key={}, entrypoints={:?}",
        current_target.daemon_name,
        crate::zenoh_identity::member_id_from_daemon_name(&current_target.daemon_name),
        current_target.control_key,
        router_entrypoints
    );
    let mut session_bridge = build_client_session_bridge(
        &session,
        &current_target.keyexpr_root,
        &namespace,
        &current_target.control_key,
        Duration::from_millis(request_timeout_ms),
    )?;
    loop {
        let mut pty_open_line = None::<String>;

        crate::control_client_input::for_each_control_stdin_line(|line| {
            if line.trim().is_empty() {
                return Ok(ControlStdinAction::Continue);
            }

            if crate::pty_control::parse_pty_open_request(&line)?.is_some() {
                pty_open_line = Some(line);
                return Ok(ControlStdinAction::Break);
            }

            execute_remote_request_with_retry_on_timeout(
                &save_dir,
                &mut session,
                &router_entrypoints,
                &namespace,
                target_name.as_deref(),
                &line,
                Duration::from_millis(request_timeout_ms),
                &mut current_target,
                &mut session_bridge,
                display,
            )?;

            flush_tty_input_if_needed(&stdin, &line)?;
            Ok(ControlStdinAction::Continue)
        })?;

        let Some(open_line) = pty_open_line else {
            break;
        };

        if stdin.is_terminal() {
            run_client_pty_over_session_bridge_tty(&mut session_bridge, open_line, false)?;
        } else {
            return run_client_pty_over_session_bridge_owned(session_bridge, open_line, false);
        }
    }

    close_client_session_bridge(&mut session_bridge)?;

    Ok(())
}

pub fn run_client_pty_control(
    namespace: String,
    target_name: Option<String>,
    router_entrypoints: Vec<String>,
    request_timeout_ms: u64,
    open_line: String,
) -> io::Result<()> {
    let router_entrypoints = zenoh_runtime::resolve_client_connect_endpoints(
        &router_entrypoints,
        Duration::from_millis(request_timeout_ms),
    )?;
    let session = zenoh_runtime::open_client_session(&router_entrypoints)?;
    let current_target = resolve_target(
        &session,
        &namespace,
        target_name.as_deref(),
        Duration::from_millis(request_timeout_ms),
    )?;
    log::info!(
        "zenoh client PTY target selected: service_name(daemon_name)={}, member_id={}, control_key={}, entrypoints={:?}",
        current_target.daemon_name,
        crate::zenoh_identity::member_id_from_daemon_name(&current_target.daemon_name),
        current_target.control_key,
        router_entrypoints
    );
    let session_bridge = build_client_session_bridge(
        &session,
        &current_target.keyexpr_root,
        &namespace,
        &current_target.control_key,
        Duration::from_millis(request_timeout_ms),
    )?;

    run_client_pty_over_session_bridge_owned(session_bridge, open_line, true)
}

pub fn run_client_pty_attach(
    namespace: String,
    target_name: Option<String>,
    router_entrypoints: Vec<String>,
    request_timeout_ms: u64,
    attach_line: String,
) -> io::Result<()> {
    let router_entrypoints = zenoh_runtime::resolve_client_connect_endpoints(
        &router_entrypoints,
        Duration::from_millis(request_timeout_ms),
    )?;
    let session = zenoh_runtime::open_client_session(&router_entrypoints)?;
    let current_target = resolve_target(
        &session,
        &namespace,
        target_name.as_deref(),
        Duration::from_millis(request_timeout_ms),
    )?;
    let session_bridge = build_client_session_bridge(
        &session,
        &current_target.keyexpr_root,
        &namespace,
        &current_target.control_key,
        Duration::from_millis(request_timeout_ms),
    )?;
    run_client_pty_attach_over_session_bridge_owned(session_bridge, attach_line, true)
}

/// 一组 line-control 请求复用同一条 Zenoh session bridge 串行执行。
///
/// 用法: `send_control_lines(namespace, target_name, router_entrypoints, timeout, &lines)`
/// 一次性发一组 line,共享同一条 session,任一行失败整组退出。
/// 这是 `rdog control <target> @<line> [@<line> ...]` one-shot 入口的主路径;
/// N=1 也走这条,N=1 / N>1 完全等价,不再有 N=1 / N>1 的分叉。
///
/// `send_single_control_line` 是**独立**的单帧入口,只给 `--pty-close` / `--pty-detach` 用,
/// 保留 retry-on-timeout 旧契约。两条管线不能合并:retry 在多 line 批量里会导致
/// 前面已成功的 line 被重复执行,产生半成功半失败状态对 agent 不友好。
pub fn send_control_lines(
    namespace: Option<String>,
    target_name: Option<String>,
    router_entrypoints: Vec<String>,
    request_timeout_ms: u64,
    lines: &[String],
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let namespace =
        crate::zenoh_identity::resolve_namespace(namespace.as_deref(), target_name.as_deref())?;
    let router_entrypoints = zenoh_runtime::resolve_client_connect_endpoints(
        &router_entrypoints,
        Duration::from_millis(request_timeout_ms),
    )?;
    let session = zenoh_runtime::open_client_session(&router_entrypoints)?;
    let current_target = resolve_target(
        &session,
        &namespace,
        target_name.as_deref(),
        Duration::from_millis(request_timeout_ms),
    )?;
    let mut session_bridge = build_client_session_bridge(
        &session,
        &current_target.keyexpr_root,
        &namespace,
        &current_target.control_key,
        Duration::from_millis(request_timeout_ms),
    )?;
    let save_dir = default_savefile_directory()?;
    let display = ControlResponseDisplay::from_stdio();
    let timeout = Duration::from_millis(request_timeout_ms);

    let mut result: io::Result<()> = Ok(());
    for (idx, line) in lines.iter().enumerate() {
        match execute_remote_request(
            &session,
            &current_target.control_key,
            line,
            timeout,
            &mut session_bridge,
        ) {
            Ok(response) => {
                if let Err(err) = handle_reply_payload(response.as_str(), &save_dir, display) {
                    result = Err(err);
                    break;
                }
            }
            Err(err) => {
                log::warn!(
                    "zenoh control multi-line request failed at line index {idx} (line={line}): {err}"
                );
                result = Err(err);
                break;
            }
        }
    }

    // best-effort close: 上面 `result` 已经是请求侧结果,关闭失败不掩盖它
    let _ = close_client_session_bridge(&mut session_bridge);

    result
}

/// 单行 line-control 入口,保留 retry-on-timeout 行为不变(用于 `--pty-close` 风格调用)。
///
/// 内部委托给 `send_control_lines` + `[line.to_string()]`,但额外保留 `execute_remote_request_with_retry_on_timeout`
/// 旧的 retry-on-timeout 行为,避免改动既有 PTY 关闭/分离的稳定契约。
pub fn send_single_control_line(
    namespace: Option<String>,
    target_name: Option<String>,
    router_entrypoints: Vec<String>,
    request_timeout_ms: u64,
    line: &str,
) -> io::Result<()> {
    let namespace =
        crate::zenoh_identity::resolve_namespace(namespace.as_deref(), target_name.as_deref())?;
    let router_entrypoints = zenoh_runtime::resolve_client_connect_endpoints(
        &router_entrypoints,
        Duration::from_millis(request_timeout_ms),
    )?;
    let mut session = zenoh_runtime::open_client_session(&router_entrypoints)?;
    let mut current_target = resolve_target(
        &session,
        &namespace,
        target_name.as_deref(),
        Duration::from_millis(request_timeout_ms),
    )?;
    let mut session_bridge = build_client_session_bridge(
        &session,
        &current_target.keyexpr_root,
        &namespace,
        &current_target.control_key,
        Duration::from_millis(request_timeout_ms),
    )?;
    execute_remote_request_with_retry_on_timeout(
        &default_savefile_directory()?,
        &mut session,
        &router_entrypoints,
        &namespace,
        target_name.as_deref(),
        line,
        Duration::from_millis(request_timeout_ms),
        &mut current_target,
        &mut session_bridge,
        ControlResponseDisplay::from_stdio(),
    )
}

/// legacy queryable 只保留简单兼容请求。
///
/// `@screenshot`、PTY、GUI 语义控制和 `@savefile` 都可能产生多 frame、
/// lifecycle frame 或文件传输语义,必须走 session channel。
/// 这里在执行前拦截,避免 legacy query/reply 继续成为第二条富能力主路径。
fn reject_session_channel_only_legacy_query(line: &str) -> Option<String> {
    let Ok(ControlParseResult::Control(request)) = parse_control_line(line) else {
        return None;
    };

    if !is_session_channel_only_command(&request.command) {
        return None;
    }

    Some(render_protocol_error_response(
        request.request_id,
        78,
        "Zenoh legacy queryable path only supports simple compatibility requests; @bootstrap and rich control must use session channel to-daemon/to-control",
    ))
}

fn is_session_channel_only_command(command: &ControlCommand) -> bool {
    matches!(
        command,
        ControlCommand::PtyOpen(_)
            | ControlCommand::PtyClose(_)
            | ControlCommand::PtyDetach(_)
            | ControlCommand::PtyAttach(_)
            | ControlCommand::Screenshot(_)
            | ControlCommand::Observe(_)
            | ControlCommand::MouseMove(_)
            | ControlCommand::MouseButton(_)
            | ControlCommand::Click(_)
            | ControlCommand::Drag(_)
            | ControlCommand::Wheel(_)
            | ControlCommand::AxTree(_)
            | ControlCommand::AxFind(_)
            | ControlCommand::AxGet(_)
            | ControlCommand::AxFocus(_)
            | ControlCommand::AxScroll(_)
            | ControlCommand::AxAction(_)
            | ControlCommand::AxPress(_)
            | ControlCommand::AxSetValue(_)
            | ControlCommand::TypeText(_)
            | ControlCommand::WindowFind(_)
            | ControlCommand::WindowActivate(_)
            | ControlCommand::WindowClose(_)
            | ControlCommand::WebFind(_)
            | ControlCommand::WebAct(_)
            | ControlCommand::GuiBench(_)
            | ControlCommand::Bootstrap(_)
            | ControlCommand::SaveFile(_)
    )
}

fn publish_outcome_to_session_channel(
    session: &zenoh::Session,
    keyexpr_root: &str,
    namespace: &str,
    session_id: &str,
    outcome: &crate::control_frames::ControlExecutionOutcome,
) -> io::Result<()> {
    let keyexpr = build_session_to_control_key_with_root(keyexpr_root, namespace, session_id);
    let publisher = session
        .declare_publisher(keyexpr)
        .wait()
        .map_err(to_io_error)?;
    let session_core = ControlPeerSession::new(session_id);
    let mut publisher = publisher;
    session_core.dispatch_outcome_ref(outcome, &mut publisher)?;

    Ok(())
}

fn execute_remote_request_with_retry_on_timeout(
    save_dir: &std::path::Path,
    session: &mut zenoh::Session,
    router_entrypoints: &[String],
    namespace: &str,
    target_name: Option<&str>,
    line: &str,
    timeout: Duration,
    current_target: &mut ResolvedTarget,
    session_bridge: &mut ZenohClientSessionBridge,
    display: ControlResponseDisplay,
) -> io::Result<()> {
    match execute_remote_request(
        session,
        &current_target.control_key,
        line,
        timeout,
        session_bridge,
    ) {
        Ok(response) => handle_reply_payload(response.as_str(), save_dir, display),
        Err(err) if err.kind() == io::ErrorKind::TimedOut => {
            log::warn!(
                "zenoh control request timed out for service_name(daemon_name)={}, retrying after re-resolve",
                current_target.daemon_name
            );

            *session = zenoh_runtime::open_client_session(router_entrypoints)?;
            let refreshed_target = resolve_target(session, namespace, target_name, timeout)?;
            log_target_if_changed(current_target, &refreshed_target);
            *session_bridge = build_client_session_bridge(
                session,
                &refreshed_target.keyexpr_root,
                namespace,
                &refreshed_target.control_key,
                timeout,
            )?;
            let response = execute_remote_request(
                session,
                &refreshed_target.control_key,
                line,
                timeout,
                session_bridge,
            )?;
            *current_target = refreshed_target;
            handle_reply_payload(response.as_str(), save_dir, display)
        }
        Err(err) => Err(err),
    }
}

fn handle_reply_payload(
    payload: &str,
    save_dir: &std::path::Path,
    display: ControlResponseDisplay,
) -> io::Result<()> {
    let frames = ControlFrame::parse_inbound_result_payload(payload)?;
    let mut stdout = std::io::stdout();

    for frame in frames {
        route_line_control_result_frame(frame, &mut stdout, display, |frame| {
            frame.save_to_directory(save_dir)
        })?;
    }

    Ok(())
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(windows)]
fn flush_windows_console_input_buffer() -> io::Result<()> {
    // ------------------------------------------------------------
    // Windows 没有 Unix `tcflush(TCIFLUSH)` 这条 API。
    // 这里改为直接清掉当前控制台输入队列,避免同机 `@key`
    // 把注入字符再次回灌进 `rdog control` 自己的下一次读行。
    // ------------------------------------------------------------
    let stdin_handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };

    flush_windows_console_input_buffer_with_handle(
        stdin_handle,
        |handle| unsafe {
            let mut mode = 0;
            GetConsoleMode(handle, &mut mode) != 0
        },
        |handle| unsafe { FlushConsoleInputBuffer(handle) != 0 },
    )
    .map(|_| ())
}

#[cfg(windows)]
fn flush_windows_console_input_buffer_with_handle<FMode, FFlush>(
    stdin_handle: HANDLE,
    is_console_handle: FMode,
    flush_input_buffer: FFlush,
) -> io::Result<bool>
where
    FMode: FnOnce(HANDLE) -> bool,
    FFlush: FnOnce(HANDLE) -> bool,
{
    // ------------------------------------------------------------
    // 先把“根本不是控制台输入句柄”的路径排除掉。
    // 这样不会误伤被 pipe/重定向接入的 stdin 场景。
    // ------------------------------------------------------------
    if stdin_handle.is_null() || stdin_handle == INVALID_HANDLE_VALUE {
        return Ok(false);
    }

    if !is_console_handle(stdin_handle) {
        return Ok(false);
    }

    if flush_input_buffer(stdin_handle) {
        return Ok(true);
    }

    let err = io::Error::last_os_error();
    if err.raw_os_error().is_some() {
        Err(err)
    } else {
        Err(io::Error::other(
            "failed to flush Windows console input buffer",
        ))
    }
}

fn flush_tty_input_if_needed(stdin: &Stdin, line: &str) -> io::Result<()> {
    if !stdin.is_terminal() || !should_flush_tty_input_after_request(line) {
        return Ok(());
    }

    #[cfg(unix)]
    {
        tcflush(stdin.as_raw_fd(), TCIFLUSH)?;
    }

    #[cfg(windows)]
    {
        flush_windows_console_input_buffer()?;
    }

    Ok(())
}

fn should_flush_tty_input_after_request(line: &str) -> bool {
    matches!(
        crate::control_protocol::parse_control_line(line),
        Ok(crate::control_protocol::ControlParseResult::Control(
            crate::control_protocol::ControlRequest {
                command: crate::control_protocol::ControlCommand::Key(_),
                ..
            }
        ))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_request_should_trigger_tty_flush_guard() {
        assert!(should_flush_tty_input_after_request(r#"@key:"2""#));
        assert!(should_flush_tty_input_after_request(
            r#"@key#7:{key:"right-option",hold_ms:200,mode:"press_release"}"#
        ));
    }

    #[test]
    fn non_key_request_should_not_trigger_tty_flush_guard() {
        assert!(!should_flush_tty_input_after_request("@ping"));
        assert!(!should_flush_tty_input_after_request(
            r#"@cmd:"printf READY""#
        ));
        assert!(!should_flush_tty_input_after_request("printf READY"));
    }

    #[test]
    fn key_input_event_key_should_default_to_identity_hierarchy() {
        let keyexpr = resolve_key_input_event_keyexpr(
            "lab",
            "mini-a.lab",
            &KeyInputEventsConfig {
                enabled: true,
                keyexpr: String::new(),
            },
        );

        assert_eq!(
            keyexpr,
            "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput"
        );
    }

    #[test]
    fn key_input_event_payload_should_include_request_and_source_fields() {
        let payload = render_key_input_event_payload(
            "lab",
            "mini-a.lab",
            &KeyRequest::legacy("F11", 200, KeyMode::PressRelease),
        );

        assert!(payload.contains(r#""event":"key_input""#));
        assert!(payload.contains(r#""namespace":"lab""#));
        assert!(payload.contains(r#""daemon_name":"mini-a.lab""#));
        assert!(payload.contains(r#""member_id":"mini-a.lab""#));
        assert!(payload.contains(r#""key":"F11""#));
        assert!(payload.contains(r#""hold_ms":200"#));
        assert!(payload.contains(r#""mode":"press_release""#));
        assert!(
            payload.contains(r#""executed_at_ms":"#) || payload.contains(r#""executed_at_ms":0"#)
        );
    }

    #[test]
    fn legacy_queryable_should_reject_rich_screenshot_requests() {
        let response = reject_session_channel_only_legacy_query(r#"@screenshot#7:{display:"all"}"#)
            .expect("rich screenshot should be rejected");

        assert!(response.contains(r#""id":7"#));
        assert!(response.contains(r#""code":78"#));
        assert!(response.contains("session channel"));
    }

    #[test]
    fn legacy_queryable_should_reject_bootstrap_requests() {
        for line in [
            "@bootstrap#9",
            r#"@bootstrap#10:{mode:"basic"}"#,
            r#"@bootstrap#11:{mode:"gui",observe:{mode:"window"}}"#,
        ] {
            let response = reject_session_channel_only_legacy_query(line)
                .expect("bootstrap should be session-channel-only");

            assert!(response.contains(r#""code":78"#));
            assert!(response.contains("session channel"));
        }
    }

    #[test]
    fn legacy_queryable_should_allow_simple_compatibility_requests() {
        assert!(reject_session_channel_only_legacy_query("@ping").is_none());
        assert!(reject_session_channel_only_legacy_query("@capabilities#1").is_none());
        assert!(reject_session_channel_only_legacy_query(r#"@cmd#42:"printf READY""#).is_none());
        assert!(reject_session_channel_only_legacy_query("printf READY").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_console_flush_helper_should_skip_invalid_handle() {
        let flushed = flush_windows_console_input_buffer_with_handle(
            INVALID_HANDLE_VALUE,
            |_| panic!("invalid handle should not probe console mode"),
            |_| panic!("invalid handle should not flush input buffer"),
        )
        .expect("invalid handle should be ignored");

        assert!(!flushed);
    }

    #[cfg(windows)]
    #[test]
    fn windows_console_flush_helper_should_skip_non_console_handle() {
        let fake_handle = 1 as HANDLE;
        let flushed = flush_windows_console_input_buffer_with_handle(
            fake_handle,
            |_| false,
            |_| panic!("non-console handle should not flush input buffer"),
        )
        .expect("non-console handle should be ignored");

        assert!(!flushed);
    }

    #[cfg(windows)]
    #[test]
    fn windows_console_flush_helper_should_flush_console_handle() {
        let fake_handle = 1 as HANDLE;
        let flushed = flush_windows_console_input_buffer_with_handle(
            fake_handle,
            |observed| {
                assert_eq!(observed, fake_handle);
                true
            },
            |observed| {
                assert_eq!(observed, fake_handle);
                true
            },
        )
        .expect("console handle should flush successfully");

        assert!(flushed);
    }
}
