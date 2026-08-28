use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    collections::HashSet,
    io,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use zenoh::Wait;

use super::session_payload::parse_session_close_payload;
use crate::{
    control_actions::SystemControlActionExecutor,
    control_core::{parse_and_execute_control_line, render_protocol_error_response},
    control_frames::ControlFrame,
    control_session::ControlPeerSession,
    zenoh_identity::{
        build_session_to_control_key_with_root, build_session_to_daemon_key_with_root,
    },
};

/// 打开 daemon 侧 session bridge。
///
/// 这里负责把 `to-daemon` 的文本 frame 翻译为 PTY lifecycle 或普通 control outcome,
/// 并把结果统一发回 `to-control`。
pub(super) fn open_daemon_session_bridge(
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
                    if ControlPeerSession::lifecycle_decision_for_frame(&frame)
                        .should_stop_streaming()
                    {
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

            // Task/Spawn Phase 2 (spec §6.4): 绑定本 session 且仍有 running 任务
            // (或帧待 drain) 时加快到 25ms, 对齐 PTY active 节奏; 否则维持 idle。
            let has_live_task = crate::task_control::session_has_live_tasks(session_id.as_str());
            let recv_timeout = if active_pty_session.is_some() || has_live_task {
                session_active_poll
            } else {
                session_idle_timeout
            };

            // drain 绑定本 session 的 task 进度帧并转发到 to-control (at-most-once)
            for frame in crate::task_control::drain_session_frames(session_id.as_str()) {
                let wire_message = frame.to_wire_message();
                let _ = publish_zenoh_text(&publisher, &wire_message);
            }

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

                    // Phase F-3 (ticket 03 fix): session bridge dispatcher 必须跟 executor 共享
                    // 同一 CancelRegistry, 否则 `@cancel#seq` signal 找不到 in-flight seq。
                    // 跟 zenoh_control.rs:240 一样的修法, 这是 ticket 03 跨实例 bug 的第二个 instance。
                    let outcome = parse_and_execute_control_line(
                        line.as_ref(),
                        &shell,
                        &executor,
                        executor.cancel_registry(),
                    );
                    let session_core = ControlPeerSession::new(session_id.as_str());
                    let dispatch_result = publisher
                        .lock()
                        .map_err(|_| io::Error::other("Zenoh publisher lock poisoned"))
                        .and_then(|mut publisher| {
                            session_core.dispatch_outcome_ref(&outcome, &mut *publisher)
                        });
                    // Task/Spawn Phase 2 (spec §6.4): spawn 请求的推送 lane 跟随来路。
                    // 用请求行前缀识别 (比逐条解析响应更省), bind 后下一轮 drain
                    // 会把 started 帧一并带给本 session。
                    if line.trim_start().starts_with("@spawn") {
                        for frame in &outcome.outbound_frames {
                            if let ControlFrame::ResponseLine(response) = frame {
                                crate::task_control::bind_task_lane_from_spawn_response(
                                    response,
                                    session_id.as_str(),
                                );
                            }
                        }
                    }
                    if let Err(err) = dispatch_result {
                        log::warn!(
                            "Zenoh session bridge failed to dispatch outcome: bridge_session_id={}, error={}",
                            session_id,
                            err
                        );
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
        ControlFrame::TaskStarted(_) => ("task-started", 0),
        ControlFrame::TaskProgress(frame) => ("task-progress", frame.step.len()),
        ControlFrame::TaskCompleted(_) => ("task-completed", 0),
        ControlFrame::TaskFailed(_) => ("task-failed", 0),
    }
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}
