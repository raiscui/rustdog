use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Read, Stdin, Write},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
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

use crate::{
    config::KeyInputEventsConfig,
    control_actions::{KeyInputEventSink, SystemControlActionExecutor},
    control_client_input::ControlStdinAction,
    control_core::{parse_and_execute_control_line, render_protocol_error_response},
    control_display::{write_response_for_display, ControlResponseDisplay},
    control_frames::{
        default_savefile_directory, ControlFrame, PtyAttachedFrame, PtyReadyFrame, PtyResizeFrame,
        PtyStdinFrame,
    },
    control_protocol::{KeyMode, KeyRequest},
    zenoh_identity::{
        build_alive_key, build_alive_key_with_root, build_control_key, build_control_key_with_root,
        build_key_input_key, build_session_to_control_key_with_root,
        build_session_to_daemon_key_with_root, KEYEXPR_ROOT, LEGACY_KEYEXPR_ROOT,
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
}

pub fn run_router_daemon(config: ZenohDaemonRuntimeConfig, shell: &str) -> io::Result<()> {
    let alive_key = build_alive_key(&config.namespace, &config.daemon_name);
    let control_key = build_control_key(&config.namespace, &config.daemon_name);
    let legacy_alive_key =
        build_alive_key_with_root(LEGACY_KEYEXPR_ROOT, &config.namespace, &config.daemon_name);
    let legacy_control_key =
        build_control_key_with_root(LEGACY_KEYEXPR_ROOT, &config.namespace, &config.daemon_name);
    let member_id = crate::zenoh_identity::member_id_from_daemon_name(&config.daemon_name);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionBridgeRequest {
    session_id: Option<String>,
    line: String,
}

fn render_session_open_payload(session_id: &str) -> String {
    format!("__rdog_session_open__:{session_id}")
}

fn render_session_close_payload(session_id: &str) -> String {
    format!("__rdog_session_close__:{session_id}")
}

fn parse_session_open_payload(payload: &str) -> io::Result<Option<String>> {
    const PREFIX: &str = "__rdog_session_open__:";
    const LEGACY_PREFIX: &str = "__rcat_session_open__:";
    let trimmed = payload.trim();

    let Some(rest) = trimmed
        .strip_prefix(PREFIX)
        .or_else(|| trimmed.strip_prefix(LEGACY_PREFIX))
    else {
        return Ok(None);
    };

    let session_id = rest.trim();
    if session_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Zenoh session open payload 的 session_id 不能为空",
        ));
    }

    Ok(Some(session_id.to_owned()))
}

fn parse_session_close_payload(payload: &str) -> io::Result<Option<String>> {
    const PREFIX: &str = "__rdog_session_close__:";
    const LEGACY_PREFIX: &str = "__rcat_session_close__:";
    let trimmed = payload.trim();

    let Some(rest) = trimmed
        .strip_prefix(PREFIX)
        .or_else(|| trimmed.strip_prefix(LEGACY_PREFIX))
    else {
        return Ok(None);
    };

    let session_id = rest.trim();
    if session_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Zenoh session close payload 的 session_id 不能为空",
        ));
    }

    Ok(Some(session_id.to_owned()))
}

#[cfg_attr(not(test), allow(dead_code))]
fn render_session_bridge_payload(session_id: &str, line: &str) -> String {
    format!("__rdog_session__:{session_id}\n{line}")
}

fn parse_session_bridge_request(payload: &str) -> io::Result<SessionBridgeRequest> {
    const PREFIX: &str = "__rdog_session__:";
    const LEGACY_PREFIX: &str = "__rcat_session__:";

    if let Some(rest) = payload
        .strip_prefix(PREFIX)
        .or_else(|| payload.strip_prefix(LEGACY_PREFIX))
    {
        let Some((session_id, line)) = rest.split_once('\n') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Zenoh session bridge payload 缺少换行分隔的控制指令",
            ));
        };

        let session_id = session_id.trim();
        let line = line.trim();
        if session_id.is_empty() || line.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Zenoh session bridge payload 的 session_id 或 line 不能为空",
            ));
        }

        return Ok(SessionBridgeRequest {
            session_id: Some(session_id.to_owned()),
            line: line.to_owned(),
        });
    }

    Ok(SessionBridgeRequest {
        session_id: None,
        line: payload.trim().to_owned(),
    })
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

    for frame in &outcome.outbound_frames {
        publisher
            .put(frame.to_wire_message())
            .wait()
            .map_err(to_io_error)?;
    }

    Ok(())
}

fn open_daemon_session_bridge(
    session: &zenoh::Session,
    keyexpr_root: &str,
    namespace: &str,
    session_id: &str,
    shell: &str,
    executor: SystemControlActionExecutor,
    active_session_bridges: Arc<Mutex<HashSet<String>>>,
) -> io::Result<()> {
    let to_daemon_key = build_session_to_daemon_key_with_root(keyexpr_root, namespace, session_id);
    let to_control_key =
        build_session_to_control_key_with_root(keyexpr_root, namespace, session_id);
    let subscriber = session
        .declare_subscriber(to_daemon_key)
        .wait()
        .map_err(to_io_error)?;
    let publisher = Arc::new(Mutex::new(
        session
            .declare_publisher(to_control_key)
            .wait()
            .map_err(to_io_error)?,
    ));
    let shell = shell.to_owned();
    let session_id = session_id.to_owned();

    thread::spawn(move || {
        let session_idle_timeout = Duration::from_secs(60);
        let session_active_poll = Duration::from_millis(25);
        let mut active_pty_session = None::<crate::pty_control::AttachedPtySession>;

        loop {
            if let Some(session) = active_pty_session.as_mut() {
                log::debug!(
                    "Zenoh PTY bridge polling session: bridge_session_id={}, pty_session_id={}",
                    session_id,
                    session.session_id()
                );
                let mut saw_frame = false;
                let mut should_clear_active_session = false;
                for _ in 0..32 {
                    let Ok(frame) = session.try_recv_frame() else {
                        break;
                    };
                    saw_frame = true;
                    let (frame_kind, payload_len) = describe_pty_bridge_frame(&frame);
                    log::debug!(
                        "Zenoh PTY bridge forwarding frame: bridge_session_id={}, pty_session_id={}, frame_kind={}, payload_bytes={}",
                        session_id,
                        session.session_id(),
                        frame_kind,
                        payload_len
                    );
                    let wire_message = frame.to_wire_message();
                    let _ = publish_zenoh_text(&publisher, &wire_message);
                    if matches!(
                        frame,
                        ControlFrame::PtyExit(_)
                            | ControlFrame::PtyClosed(_)
                            | ControlFrame::PtyDetached(_)
                    ) {
                        should_clear_active_session = true;
                        break;
                    }
                }
                if !saw_frame {
                    log::debug!(
                        "Zenoh PTY bridge had no queued frame: bridge_session_id={}, pty_session_id={}",
                        session_id,
                        session.session_id()
                    );
                }
                if should_clear_active_session {
                    active_pty_session = None;
                }
            }

            let recv_timeout = if active_pty_session.is_some() {
                session_active_poll
            } else {
                session_idle_timeout
            };

            match subscriber.recv_timeout(recv_timeout) {
                Ok(Some(sample)) => {
                    let Ok(line) = sample.payload().try_to_string().map_err(to_io_error) else {
                        continue;
                    };
                    log::debug!(
                        "Zenoh session bridge received inbound line: bridge_session_id={}, active_pty={}, bytes={}",
                        session_id,
                        active_pty_session
                            .as_ref()
                            .map(|session| session.session_id())
                            .unwrap_or("<none>"),
                        line.len()
                    );
                    if let Ok(Some(close_session_id)) = parse_session_close_payload(line.as_ref()) {
                        if close_session_id == session_id {
                            if let Ok(mut bridges) = active_session_bridges.lock() {
                                bridges.remove(&session_id);
                            }
                            let _ = publish_zenoh_text(&publisher, "@response 0");
                            return;
                        }
                    }
                    if matches!(parse_session_close_payload(line.as_ref()), Ok(Some(_))) {
                        break;
                    }
                    if let Some(session) = active_pty_session.as_ref() {
                        match crate::control_frames::PtyStdinFrame::parse_wire_message(
                            line.as_ref(),
                        ) {
                            Ok(Some(frame)) => {
                                if frame.session_id == session.session_id() {
                                    match BASE64_STANDARD.decode(frame.data.as_bytes()) {
                                        Ok(bytes) => {
                                            log::debug!(
                                                "Zenoh PTY bridge forwarding stdin frame: bridge_session_id={}, pty_session_id={}, bytes={}",
                                                session_id,
                                                session.session_id(),
                                                bytes.len()
                                            );
                                            let _ = session.send_stdin_bytes(bytes);
                                        }
                                        Err(err) => {
                                            let response = render_protocol_error_response(
                                                None,
                                                64,
                                                &format!("@pty-stdin base64 数据无法解码: {err}"),
                                            );
                                            let _ = publish_zenoh_text(&publisher, &response);
                                        }
                                    }
                                    continue;
                                }

                                let response = render_protocol_error_response(
                                    None,
                                    64,
                                    "PTY stdin frame 的 session_id 与当前 attached PTY 不匹配",
                                );
                                let _ = publish_zenoh_text(&publisher, &response);
                                continue;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                let response =
                                    render_protocol_error_response(None, 64, &err.to_string());
                                let _ = publish_zenoh_text(&publisher, &response);
                                continue;
                            }
                        }

                        match crate::control_frames::PtyResizeFrame::parse_wire_message(
                            line.as_ref(),
                        ) {
                            Ok(Some(frame)) => {
                                if frame.session_id == session.session_id() {
                                    log::debug!(
                                        "Zenoh PTY bridge forwarding resize frame: bridge_session_id={}, pty_session_id={}, cols={}, rows={}",
                                        session_id,
                                        session.session_id(),
                                        frame.cols,
                                        frame.rows
                                    );
                                    let _ = session.resize(frame.cols, frame.rows);
                                    continue;
                                }

                                let response = render_protocol_error_response(
                                    None,
                                    64,
                                    "PTY resize frame 的 session_id 与当前 attached PTY 不匹配",
                                );
                                let _ = publish_zenoh_text(&publisher, &response);
                                continue;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                let response =
                                    render_protocol_error_response(None, 64, &err.to_string());
                                let _ = publish_zenoh_text(&publisher, &response);
                                continue;
                            }
                        }

                        match crate::pty_control::should_close_pty_session(
                            line.as_ref(),
                            session.session_id(),
                        ) {
                            Ok(true) => {
                                let _ = session.close("force_close");
                                continue;
                            }
                            Ok(false) => {}
                            Err(err) => {
                                let response =
                                    render_protocol_error_response(None, 64, &err.to_string());
                                let _ = publish_zenoh_text(&publisher, &response);
                                continue;
                            }
                        }
                        if matches!(
                            crate::control_protocol::parse_control_line(line.as_ref()),
                            Ok(crate::control_protocol::ControlParseResult::Control(
                                crate::control_protocol::ControlRequest {
                                    command: crate::control_protocol::ControlCommand::PtyDetach(_),
                                    ..
                                }
                            ))
                        ) {
                            let _ = session.detach("owner_detach");
                            continue;
                        }

                        let mut bytes = line.as_bytes().to_vec();
                        bytes.push(b'\n');
                        let _ = session.send_stdin_bytes(bytes);
                        continue;
                    }
                    match crate::pty_control::parse_pty_open_request(line.as_ref()) {
                        Ok(Some(request)) => {
                            log::info!(
                                "Zenoh PTY open received on session bridge: bridge_session_id={}",
                                session_id
                            );
                            match crate::pty_control::open_attached_pty_session(request) {
                                Ok(session) => {
                                    log::info!(
                                        "Zenoh PTY session attached to bridge: bridge_session_id={}, pty_session_id={}",
                                        session_id,
                                        session.session_id()
                                    );
                                    active_pty_session = Some(session)
                                }
                                Err(err) => {
                                    log::warn!(
                                        "Zenoh PTY open failed on session bridge: bridge_session_id={}, error={}",
                                        session_id,
                                        err
                                    );
                                    let response =
                                        render_protocol_error_response(None, 70, &err.to_string());
                                    let _ = publish_zenoh_text(&publisher, &response);
                                }
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let response =
                                render_protocol_error_response(None, 64, &err.to_string());
                            let _ = publish_zenoh_text(&publisher, &response);
                            continue;
                        }
                    }

                    match crate::pty_control::parse_pty_attach_request(line.as_ref()) {
                        Ok(Some(request)) => {
                            match crate::pty_control::attach_active_pty_session(request) {
                                Ok(Some(session)) => active_pty_session = Some(session),
                                Ok(None) => {
                                    let response = render_protocol_error_response(
                                        None,
                                        64,
                                        "PTY attach 目标 session 不存在",
                                    );
                                    let _ = publish_zenoh_text(&publisher, &response);
                                }
                                Err(err) => {
                                    let response =
                                        render_protocol_error_response(None, 70, &err.to_string());
                                    let _ = publish_zenoh_text(&publisher, &response);
                                }
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let response =
                                render_protocol_error_response(None, 64, &err.to_string());
                            let _ = publish_zenoh_text(&publisher, &response);
                            continue;
                        }
                    }

                    let outcome = parse_and_execute_control_line(line.as_ref(), &shell, &executor);
                    for frame in outcome.outbound_frames {
                        let _ = publish_zenoh_text(&publisher, &frame.to_wire_message());
                    }
                }
                Ok(None) if active_pty_session.is_some() => continue,
                Ok(None) => break,
                Err(_) if active_pty_session.is_some() => continue,
                Err(_) => break,
            }
        }

        if let Ok(mut bridges) = active_session_bridges.lock() {
            bridges.remove(&session_id);
        }
    });

    Ok(())
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    daemon_name: String,
    control_key: String,
    keyexpr_root: String,
}

struct ZenohClientSessionBridge {
    session_id: String,
    publisher: zenoh::pubsub::Publisher<'static>,
    subscriber:
        zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
    session: zenoh::Session,
    to_daemon_key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ZenohPtyStdinMode {
    RawTty,
    Pipe,
}

fn publish_zenoh_text(
    publisher: &Arc<Mutex<zenoh::pubsub::Publisher<'static>>>,
    payload: &str,
) -> io::Result<()> {
    let publisher = publisher
        .lock()
        .map_err(|_| io::Error::other("Zenoh publisher lock poisoned"))?;
    publisher.put(payload).wait().map_err(to_io_error)
}

fn describe_pty_bridge_frame(frame: &ControlFrame) -> (&'static str, usize) {
    match frame {
        ControlFrame::PtyReady(_) => ("pty-ready", 0),
        ControlFrame::PtyOutput(frame) => ("pty-output", frame.data.len()),
        ControlFrame::PtyExit(_) => ("pty-exit", 0),
        ControlFrame::PtyClosed(_) => ("pty-closed", 0),
        ControlFrame::PtyDetached(_) => ("pty-detached", 0),
        ControlFrame::PtyAttached(_) => ("pty-attached", 0),
        ControlFrame::ResponseLine(line) => ("response-line", line.len()),
        ControlFrame::SaveFile(frame) => ("savefile", frame.data.len()),
    }
}

fn build_client_session_bridge(
    session: &zenoh::Session,
    keyexpr_root: &str,
    namespace: &str,
    control_key: &str,
    timeout: Duration,
) -> io::Result<ZenohClientSessionBridge> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_key = build_session_to_control_key_with_root(keyexpr_root, namespace, &session_id);
    let subscriber = session
        .declare_subscriber(session_key)
        .wait()
        .map_err(to_io_error)?;
    let to_daemon_key = build_session_to_daemon_key_with_root(keyexpr_root, namespace, &session_id);
    let publisher = session
        .declare_publisher(to_daemon_key.clone())
        .wait()
        .map_err(to_io_error)?;

    ensure_daemon_session_bridge_open(session, control_key, &session_id, timeout)?;

    Ok(ZenohClientSessionBridge {
        session_id,
        publisher,
        subscriber,
        session: session.clone(),
        to_daemon_key,
    })
}

fn close_client_session_bridge(session_bridge: &mut ZenohClientSessionBridge) -> io::Result<()> {
    session_bridge
        .publisher
        .put(render_session_close_payload(&session_bridge.session_id))
        .wait()
        .map_err(to_io_error)
}

fn wait_for_pty_ready_over_session_bridge(
    session_bridge: &mut ZenohClientSessionBridge,
    open_line: String,
) -> io::Result<PtyReadyFrame> {
    session_bridge
        .publisher
        .put(open_line)
        .wait()
        .map_err(to_io_error)?;

    loop {
        let sample = session_bridge
            .subscriber
            .recv_timeout(Duration::from_secs(3600))
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Zenoh PTY subscriber 在 ready 前关闭",
                )
            })?;
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
            ControlFrame::PtyReady(frame) => return Ok(frame),
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY open 返回了普通响应而不是 @pty-ready: {response}"),
                ))
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY open 收到意外 frame: {}", frame.to_wire_message()),
                ))
            }
        }
    }
}

fn wait_for_pty_attached_over_session_bridge(
    session_bridge: &mut ZenohClientSessionBridge,
    attach_line: String,
) -> io::Result<PtyAttachedFrame> {
    session_bridge
        .publisher
        .put(attach_line)
        .wait()
        .map_err(to_io_error)?;

    loop {
        let sample = session_bridge
            .subscriber
            .recv_timeout(Duration::from_secs(3600))
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Zenoh PTY subscriber 在 attach 前关闭",
                )
            })?;
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
            ControlFrame::PtyAttached(frame) => return Ok(frame),
            ControlFrame::ResponseLine(response) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Zenoh PTY attach 返回了普通响应而不是 @pty-attached: {response}"),
                ))
            }
            frame => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Zenoh PTY attach 收到意外 frame: {}",
                        frame.to_wire_message()
                    ),
                ))
            }
        }
    }
}

fn run_client_pty_over_session_bridge_threaded_stdin(
    subscriber: &mut zenoh::pubsub::Subscriber<
        zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
    >,
    input_session: zenoh::Session,
    input_key: String,
    ready: PtyReadyFrame,
    fail_on_nonzero_exit: bool,
    stdin_mode: ZenohPtyStdinMode,
) -> io::Result<()> {
    let input_session_id = ready.session_id.clone();
    let stop_input = Arc::new(AtomicBool::new(false));
    let input_stop_signal = Arc::clone(&stop_input);
    let stdin_session = input_session.clone();
    let stdin_key = input_key.clone();
    let mut input_thread = Some(thread::spawn(move || -> io::Result<()> {
        let mut stdin = io::stdin();
        let mut buffer = [0_u8; 4096];
        while !input_stop_signal.load(Ordering::Relaxed) {
            match stdin.read(&mut buffer) {
                Ok(0) if stdin_mode == ZenohPtyStdinMode::RawTty => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Ok(0) => return Ok(()),
                Ok(len) => {
                    log::debug!(
                        "Zenoh PTY client stdin produced bytes: pty_session_id={}, bytes={}",
                        input_session_id,
                        len
                    );
                    let frame = PtyStdinFrame {
                        session_id: input_session_id.clone(),
                        data: BASE64_STANDARD.encode(&buffer[..len]),
                    };
                    stdin_session
                        .put(stdin_key.as_str(), frame.to_wire_message())
                        .wait()
                        .map_err(to_io_error)?;
                    log::debug!(
                        "Zenoh PTY client stdin frame published: pty_session_id={}, bytes={}",
                        input_session_id,
                        len
                    );
                }
                Err(err)
                    if stdin_mode == ZenohPtyStdinMode::RawTty
                        && matches!(
                            err.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        ) =>
                {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }));
    let resize_thread = if stdin_mode == ZenohPtyStdinMode::RawTty {
        Some(spawn_zenoh_pty_resize_thread(
            input_session.clone(),
            input_key.clone(),
            ready.session_id.clone(),
            Arc::clone(&stop_input),
        ))
    } else {
        None
    };
    let mut resize_thread = resize_thread;

    let mut stdout = io::stdout();
    loop {
        match subscriber.recv_timeout(Duration::from_millis(25)) {
            Ok(Some(sample)) => {
                let payload = sample.payload().try_to_string().map_err(to_io_error)?;

                match ControlFrame::parse_inbound_result_message(payload.as_ref())? {
                    ControlFrame::PtyOutput(frame) if frame.session_id == ready.session_id => {
                        let bytes =
                            BASE64_STANDARD
                                .decode(frame.data.as_bytes())
                                .map_err(|err| {
                                    io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!("@pty-output base64 数据无法解码: {err}"),
                                    )
                                })?;
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                    }
                    ControlFrame::PtyExit(frame) if frame.session_id == ready.session_id => {
                        stop_input.store(true, Ordering::Relaxed);
                        join_zenoh_pty_stdin_thread_for_terminal(&mut input_thread, stdin_mode)?;
                        join_zenoh_pty_resize_thread(&mut resize_thread)?;
                        if !fail_on_nonzero_exit || frame.exit_code == 0 {
                            return Ok(());
                        }
                        return Err(io::Error::other(format!(
                            "remote PTY exited with code {}",
                            frame.exit_code
                        )));
                    }
                    ControlFrame::PtyClosed(frame) if frame.session_id == ready.session_id => {
                        stop_input.store(true, Ordering::Relaxed);
                        join_zenoh_pty_stdin_thread_for_terminal(&mut input_thread, stdin_mode)?;
                        join_zenoh_pty_resize_thread(&mut resize_thread)?;
                        return Err(io::Error::other(format!(
                            "remote PTY closed before natural exit: {}",
                            frame.reason
                        )));
                    }
                    _ => {}
                }
            }
            Ok(None) => {}
            Err(err) => {
                return Err(zenoh_pty_subscriber_closed_before_terminal(
                    err,
                    &ready.session_id,
                ));
            }
        }

        join_finished_zenoh_pty_stdin_thread(&mut input_thread)?;
        join_finished_zenoh_pty_resize_thread(&mut resize_thread)?;
    }
}

fn spawn_zenoh_pty_resize_thread(
    session: zenoh::Session,
    input_key: String,
    pty_session_id: String,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut last_size = crate::pty_control::default_terminal_size();
        let (cols, rows) = last_size;
        let initial_frame = PtyResizeFrame {
            session_id: pty_session_id.clone(),
            cols,
            rows,
        };
        session
            .put(input_key.as_str(), initial_frame.to_wire_message())
            .wait()
            .map_err(to_io_error)?;

        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            let size = crate::pty_control::default_terminal_size();
            if size == last_size {
                continue;
            }

            last_size = size;
            let (cols, rows) = size;
            let frame = PtyResizeFrame {
                session_id: pty_session_id.clone(),
                cols,
                rows,
            };
            session
                .put(input_key.as_str(), frame.to_wire_message())
                .wait()
                .map_err(to_io_error)?;
            log::debug!(
                "Zenoh PTY client resize frame published: pty_session_id={}, cols={}, rows={}",
                pty_session_id,
                cols,
                rows
            );
        }

        Ok(())
    })
}

fn join_finished_zenoh_pty_resize_thread(
    resize_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    if resize_thread
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        return join_zenoh_pty_resize_thread(resize_thread);
    }

    Ok(())
}

fn join_zenoh_pty_resize_thread(
    resize_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let Some(handle) = resize_thread.take() else {
        return Ok(());
    };

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("Zenoh PTY resize thread panicked")),
    }
}

fn join_zenoh_pty_stdin_thread_for_terminal(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
    stdin_mode: ZenohPtyStdinMode,
) -> io::Result<()> {
    if stdin_mode == ZenohPtyStdinMode::RawTty {
        return join_zenoh_pty_stdin_thread(input_thread);
    }

    join_finished_zenoh_pty_stdin_thread(input_thread)
}

fn join_finished_zenoh_pty_stdin_thread(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    if input_thread
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        return join_zenoh_pty_stdin_thread(input_thread);
    }

    Ok(())
}

fn join_zenoh_pty_stdin_thread(
    input_thread: &mut Option<thread::JoinHandle<io::Result<()>>>,
) -> io::Result<()> {
    let Some(handle) = input_thread.take() else {
        return Ok(());
    };
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("Zenoh PTY stdin thread panicked")),
    }
}

fn run_client_pty_over_session_bridge_tty(
    session_bridge: &mut ZenohClientSessionBridge,
    open_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let ready = wait_for_pty_ready_over_session_bridge(session_bridge, open_line)?;
    let _raw_guard = crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?;
    run_client_pty_over_session_bridge_threaded_stdin(
        &mut session_bridge.subscriber,
        session_bridge.session.clone(),
        session_bridge.to_daemon_key.clone(),
        ready,
        fail_on_nonzero_exit,
        ZenohPtyStdinMode::RawTty,
    )
}

fn run_client_pty_over_session_bridge_owned(
    mut session_bridge: ZenohClientSessionBridge,
    open_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let ready = wait_for_pty_ready_over_session_bridge(&mut session_bridge, open_line)?;
    let session_id = session_bridge.session_id.clone();
    let raw_guard = if std::io::stdin().is_terminal() {
        crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?
    } else {
        None
    };
    let stdin_mode = if raw_guard.is_some() {
        ZenohPtyStdinMode::RawTty
    } else {
        ZenohPtyStdinMode::Pipe
    };
    let result = {
        let _raw_guard = raw_guard;
        run_client_pty_over_session_bridge_threaded_stdin(
            &mut session_bridge.subscriber,
            session_bridge.session.clone(),
            session_bridge.to_daemon_key.clone(),
            ready,
            fail_on_nonzero_exit,
            stdin_mode,
        )
    };
    let close_result = session_bridge
        .publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .map_err(to_io_error);

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

fn run_client_pty_attach_over_session_bridge_owned(
    mut session_bridge: ZenohClientSessionBridge,
    attach_line: String,
    fail_on_nonzero_exit: bool,
) -> io::Result<()> {
    let attached = wait_for_pty_attached_over_session_bridge(&mut session_bridge, attach_line)?;
    let ready = PtyReadyFrame {
        session_id: attached.session_id,
        cols: attached.cols,
        rows: attached.rows,
    };
    let session_id = session_bridge.session_id.clone();
    let raw_guard = if std::io::stdin().is_terminal() {
        crate::pty_control::LocalRawTerminalGuard::enter_if_tty()?
    } else {
        None
    };
    let stdin_mode = if raw_guard.is_some() {
        ZenohPtyStdinMode::RawTty
    } else {
        ZenohPtyStdinMode::Pipe
    };
    let result = {
        let _raw_guard = raw_guard;
        run_client_pty_over_session_bridge_threaded_stdin(
            &mut session_bridge.subscriber,
            session_bridge.session.clone(),
            session_bridge.to_daemon_key.clone(),
            ready,
            fail_on_nonzero_exit,
            stdin_mode,
        )
    };
    let close_result = session_bridge
        .publisher
        .put(render_session_close_payload(&session_id))
        .wait()
        .map_err(to_io_error);

    match (result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

fn ensure_daemon_session_bridge_open(
    session: &zenoh::Session,
    control_key: &str,
    session_id: &str,
    timeout: Duration,
) -> io::Result<()> {
    let replies = session
        .get(control_key)
        .payload(render_session_open_payload(session_id))
        .timeout(timeout)
        .wait()
        .map_err(to_io_error)?;

    while let Ok(reply) = replies.recv() {
        if reply.result().is_ok() {
            return Ok(());
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("Zenoh session open 超时,未收到 control_key={control_key} 的 ack"),
    ))
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
        match frame {
            ControlFrame::ResponseLine(response) => {
                write_response_for_display(&mut stdout, &response, display)?;
            }
            ControlFrame::SaveFile(frame) => {
                let saved_path = frame.save_to_directory(save_dir)?;
                writeln!(stdout, "saved file: {}", saved_path.display())?;
                stdout.flush()?;
            }
            ControlFrame::PtyReady(_)
            | ControlFrame::PtyOutput(_)
            | ControlFrame::PtyExit(_)
            | ControlFrame::PtyClosed(_)
            | ControlFrame::PtyDetached(_)
            | ControlFrame::PtyAttached(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "line-control response 收到了意外 PTY frame",
                ));
            }
        }
    }

    Ok(())
}

fn zenoh_pty_subscriber_closed_before_terminal(
    err: impl std::fmt::Display,
    session_id: &str,
) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!(
            "Zenoh PTY subscriber 在收到 terminal lifecycle frame 前关闭: session_id={session_id}, error={err}"
        ),
    )
}

fn log_target_if_changed(current_target: &ResolvedTarget, refreshed_target: &ResolvedTarget) {
    if current_target.control_key != refreshed_target.control_key {
        log::info!(
            "zenoh control target selected: service_name(daemon_name)={}, member_id={}, control_key={}",
            refreshed_target.daemon_name,
            crate::zenoh_identity::member_id_from_daemon_name(&refreshed_target.daemon_name),
            refreshed_target.control_key
        );
    }
}

fn ensure_unique_daemon_name(
    session: &zenoh::Session,
    namespace: &str,
    daemon_name: &str,
    timeout: Duration,
) -> io::Result<()> {
    let selector = build_alive_key(namespace, daemon_name);
    let replies = session
        .liveliness()
        .get(&selector)
        .timeout(timeout)
        .wait()
        .map_err(to_io_error)?;

    if let Ok(reply) = replies.recv() {
        if let Ok(sample) = reply.result() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "发现重复 service_name 活跃 member: namespace={namespace}, service_name={daemon_name}, remote_key={}",
                    sample.key_expr()
                ),
            ));
        }
    }

    Ok(())
}

struct DaemonNameGuard {
    path: PathBuf,
}

impl Drop for DaemonNameGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_daemon_name_guard(namespace: &str, daemon_name: &str) -> io::Result<DaemonNameGuard> {
    let lock_dir = zenoh_guard_dir()?;
    fs::create_dir_all(&lock_dir)?;
    let path = lock_dir.join(format!("{namespace}__{daemon_name}.pid"));
    let pid = std::process::id().to_string();

    loop {
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(pid.as_bytes())?;
                return Ok(DaemonNameGuard { path });
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let existing = fs::read_to_string(&path).unwrap_or_default();
                let existing_pid = existing.trim().parse::<u32>().ok();
                if existing_pid.is_some_and(process_exists) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "发现重复 service_name 活跃 member: namespace={namespace}, service_name={daemon_name}, local_guard={}",
                            path.display()
                        ),
                    ));
                }

                match fs::remove_file(&path) {
                    Ok(()) => continue,
                    Err(remove_err) if remove_err.kind() == io::ErrorKind::NotFound => continue,
                    Err(remove_err) => return Err(remove_err),
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn zenoh_guard_dir() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data)
                .join("rustdog")
                .join("zenoh-guards"));
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(state_home)
                .join("rustdog")
                .join("zenoh-guards"));
        }

        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("rustdog")
                .join("zenoh-guards"));
        }
    }

    Ok(std::env::temp_dir().join("rustdog").join("zenoh-guards"))
}

fn process_exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        if pid == 0 {
            return false;
        }

        let filter = format!("PID eq {pid}");
        return std::process::Command::new("tasklist")
            .args(["/FI", &filter])
            .output()
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
            });
    }

    #[cfg(not(windows))]
    {
        if pid == 0 {
            return false;
        }

        // 这里只是做本地 pid 存活探测。
        // stale pid 是正常清理路径的一部分,不应该把 `kill -0` 的 stderr
        // 直接泄漏到 daemon 启动输出里,否则用户会误以为启动失败了。
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .is_some_and(|status| status.success())
    }
}

fn resolve_target(
    session: &zenoh::Session,
    namespace: &str,
    target_name: Option<&str>,
    timeout: Duration,
) -> io::Result<ResolvedTarget> {
    let mut candidates = Vec::new();
    for keyexpr_root in [KEYEXPR_ROOT, LEGACY_KEYEXPR_ROOT] {
        let selector = match target_name {
            Some(target_name) => build_alive_key_with_root(keyexpr_root, namespace, target_name),
            None => format!("{keyexpr_root}/{namespace}/daemon/*/alive"),
        };
        let replies = session
            .liveliness()
            .get(&selector)
            .timeout(timeout)
            .wait()
            .map_err(to_io_error)?;

        while let Ok(reply) = replies.recv() {
            let Ok(sample) = reply.result() else {
                continue;
            };
            if let Some(candidate) = parse_liveliness_candidate(sample.key_expr().as_str()) {
                candidates.push(candidate);
            }
        }

        // 新 root 是默认真相源。
        // 只有新 root 完全没有命中时,才继续尝试 legacy root。
        if !candidates.is_empty() {
            break;
        }
    }

    if candidates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "未找到目标 service: namespace={}, target_name={}",
                namespace,
                target_name.unwrap_or("<auto>")
            ),
        ));
    }

    if target_name.is_none() {
        candidates.sort_by(|left, right| left.daemon_name.cmp(&right.daemon_name));
        candidates.dedup_by(|left, right| left.daemon_name == right.daemon_name);
    }

    if candidates.len() > 1 {
        let instances = candidates
            .iter()
            .map(|candidate| candidate.daemon_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("目标 service 冲突,命中的 member_id: {instances}"),
        ));
    }

    Ok(candidates.remove(0))
}

fn execute_remote_request(
    session: &zenoh::Session,
    control_key: &str,
    line: &str,
    timeout: Duration,
    session_bridge: &mut ZenohClientSessionBridge,
) -> io::Result<String> {
    ensure_daemon_session_bridge_open(session, control_key, &session_bridge.session_id, timeout)?;

    session_bridge
        .publisher
        .put(line.to_owned())
        .wait()
        .map_err(to_io_error)?;

    let mut frames = Vec::new();
    loop {
        let sample = session_bridge
            .subscriber
            .recv_timeout(timeout)
            .map_err(|err| io::Error::new(io::ErrorKind::TimedOut, err.to_string()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Zenoh session bridge subscriber 在收到结果前关闭",
                )
            })?;
        let payload = sample.payload().try_to_string().map_err(to_io_error)?;
        frames.push(payload.to_string());

        if payload.starts_with("@response ") {
            return Ok(frames.join("\n"));
        }
    }
}

fn parse_liveliness_candidate(key: &str) -> Option<ResolvedTarget> {
    let parts = key.split('/').collect::<Vec<_>>();
    if parts.len() != 7 {
        return None;
    }
    if !matches!(parts[0], KEYEXPR_ROOT | LEGACY_KEYEXPR_ROOT)
        || parts[2] != "daemon"
        || parts[4] != "member"
        || parts[6] != "alive"
    {
        return None;
    }

    let namespace = parts[1];
    let daemon_name = parts[3];
    let member_id = parts[5];
    if member_id != daemon_name {
        return None;
    }
    Some(ResolvedTarget {
        daemon_name: daemon_name.to_string(),
        control_key: build_control_key_with_root(parts[0], namespace, daemon_name),
        keyexpr_root: parts[0].to_string(),
    })
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
    fn liveliness_key_should_parse_back_to_control_target() {
        let target =
            parse_liveliness_candidate("rdog/lab/daemon/mini-a.lab/member/mini-a.lab/alive")
                .expect("candidate should parse");

        assert_eq!(target.daemon_name, "mini-a.lab");
        assert_eq!(
            target.control_key,
            "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/control"
        );
        assert_eq!(target.keyexpr_root, "rdog");
    }

    #[test]
    fn legacy_liveliness_key_should_parse_back_to_legacy_control_target() {
        let target =
            parse_liveliness_candidate("rcat/lab/daemon/mini-a.lab/member/mini-a.lab/alive")
                .expect("legacy candidate should parse");

        assert_eq!(target.daemon_name, "mini-a.lab");
        assert_eq!(
            target.control_key,
            "rcat/lab/daemon/mini-a.lab/member/mini-a.lab/control"
        );
        assert_eq!(target.keyexpr_root, "rcat");
    }

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
    fn session_bridge_payload_should_roundtrip() {
        let payload = render_session_bridge_payload("sess-42", "@ping");
        let parsed = parse_session_bridge_request(&payload).expect("payload should parse");

        assert_eq!(
            parsed,
            SessionBridgeRequest {
                session_id: Some("sess-42".to_owned()),
                line: "@ping".to_owned(),
            }
        );
    }

    #[test]
    fn session_open_payload_should_roundtrip() {
        let payload = render_session_open_payload("sess-42");
        let parsed = parse_session_open_payload(&payload).expect("payload should parse");

        assert_eq!(parsed, Some("sess-42".to_owned()));
    }

    #[test]
    fn legacy_session_payloads_should_still_parse() {
        let open =
            parse_session_open_payload("__rcat_session_open__:sess-42").expect("payload parses");
        let close =
            parse_session_close_payload("__rcat_session_close__:sess-42").expect("payload parses");
        let bridge = parse_session_bridge_request("__rcat_session__:sess-42\n@ping")
            .expect("payload parses");

        assert_eq!(open, Some("sess-42".to_owned()));
        assert_eq!(close, Some("sess-42".to_owned()));
        assert_eq!(
            bridge,
            SessionBridgeRequest {
                session_id: Some("sess-42".to_owned()),
                line: "@ping".to_owned(),
            }
        );
    }

    #[test]
    fn session_close_payload_should_roundtrip() {
        let payload = render_session_close_payload("sess-42");
        let parsed = parse_session_close_payload(&payload).expect("payload should parse");

        assert_eq!(parsed, Some("sess-42".to_owned()));
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
