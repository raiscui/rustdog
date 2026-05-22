use std::{
    collections::VecDeque,
    io::{self, Write},
    path::PathBuf,
};

use crate::{
    control_display::{write_response_for_display, ControlResponseDisplay},
    control_frames::{ControlExecutionOutcome, ControlFrame, SaveFileFrame},
    control_transport::ControlTransport,
};
use zenoh::Wait;

/// transport 无关的控制面 frame 发送器。
///
/// 这层只负责“把 frame 写出去”,不碰 socket / websocket / zenoh 的具体实现。
/// 这样 TCP、WebSocket 和 Zenoh 就可以共用同一套 frame dispatch 逻辑。
pub trait ControlPeerFrameSink {
    fn send_control_frame(&mut self, frame: &ControlFrame) -> io::Result<()>;
}

impl ControlPeerFrameSink for ControlTransport {
    fn send_control_frame(&mut self, frame: &ControlFrame) -> io::Result<()> {
        self.write_message(frame.to_wire_message().as_str())
    }
}

impl<'a> ControlPeerFrameSink for zenoh::pubsub::Publisher<'a> {
    fn send_control_frame(&mut self, frame: &ControlFrame) -> io::Result<()> {
        self.put(frame.to_wire_message())
            .wait()
            .map_err(|err| io::Error::other(err.to_string()))
    }
}

/// 把普通 `Write` 适配成 frame sink。
///
/// 这里不用 blanket impl,避免和外部 crate 未来给某个类型实现 `Write` 时产生冲突。
#[cfg(test)]
pub struct LineWriteFrameSink<'a, W: Write> {
    writer: &'a mut W,
}

#[cfg(test)]
impl<'a, W: Write> LineWriteFrameSink<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }
}

#[cfg(test)]
impl<W: Write> ControlPeerFrameSink for LineWriteFrameSink<'_, W> {
    fn send_control_frame(&mut self, frame: &ControlFrame) -> io::Result<()> {
        writeln!(self.writer, "{}", frame.to_wire_message())?;
        self.writer.flush()
    }
}

/// 控制面 peer 的最小 session core。
///
/// 这里先不碰 transport 句柄,也不碰 PTY process 本体。
/// 它只收口 frame 队列、顺序发送、结果路由和 terminal gate 判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPeerSession {
    session_id: String,
    outbound_frames: VecDeque<ControlFrame>,
}

/// 单次 dispatch 的轻量回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPeerDispatchReport {
    pub session_id: String,
    pub queued_frames: usize,
    pub sent_frames: usize,
    pub request_ids: Vec<u64>,
    pub lifecycle_decision: Option<ControlPeerLifecycleDecision>,
}

/// PTY lifecycle 的共享判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlPeerLifecycleDecision {
    Continue,
    Detached {
        session_id: String,
        reason: String,
    },
    TerminalComplete {
        frame_kind: &'static str,
        session_id: String,
        reason: String,
        exit_code: Option<i32>,
    },
}

impl ControlPeerLifecycleDecision {
    pub fn should_stop_streaming(&self) -> bool {
        !matches!(self, Self::Continue)
    }
}

/// 轻量观测记录。
///
/// 这里故意只留 frame kind、request id 和安全摘要。
/// savefile 的 base64 原文不应该进入日志摘要。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPeerFrameLogRecord {
    pub frame_kind: &'static str,
    pub session_id: Option<String>,
    pub target_name: Option<String>,
    pub request_id: Option<u64>,
    pub payload_summary: String,
}

impl ControlPeerSession {
    /// 创建一个 transport 无关的 peer session。
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            outbound_frames: VecDeque::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 把一次 execution outcome 变成按序发送的 outbound frame 队列。
    pub fn queue_outcome(&mut self, outcome: ControlExecutionOutcome) -> usize {
        let queued_frames = outcome.outbound_frames.len();
        self.outbound_frames.extend(outcome.outbound_frames);
        queued_frames
    }

    /// 只做一次 frame dispatch,不碰具体 transport 类型。
    pub fn flush_outbound_frames<S: ControlPeerFrameSink>(
        &mut self,
        sink: &mut S,
    ) -> io::Result<usize> {
        let mut sent_frames = 0;

        while let Some(frame) = self.outbound_frames.pop_front() {
            sink.send_control_frame(&frame)?;
            sent_frames += 1;
        }

        Ok(sent_frames)
    }

    /// 把 outcome 入队并立即发送。
    pub fn dispatch_outcome<S: ControlPeerFrameSink>(
        &mut self,
        outcome: ControlExecutionOutcome,
        sink: &mut S,
    ) -> io::Result<ControlPeerDispatchReport> {
        let request_ids = collect_request_ids(outcome.outbound_frames.as_slice());
        let queued_frames = self.queue_outcome(outcome);
        let sent_frames = self.flush_outbound_frames(sink)?;

        Ok(ControlPeerDispatchReport {
            session_id: self.session_id().to_owned(),
            queued_frames,
            sent_frames,
            request_ids,
            lifecycle_decision: None,
        })
    }

    /// 只读版本的 outcome dispatch。
    ///
    /// 适合已经持有 borrowed outcome 的 transport adapter。
    pub fn dispatch_outcome_ref<S: ControlPeerFrameSink>(
        &self,
        outcome: &ControlExecutionOutcome,
        sink: &mut S,
    ) -> io::Result<usize> {
        let mut sent_frames = 0;

        for frame in &outcome.outbound_frames {
            sink.send_control_frame(frame)?;
            sent_frames += 1;
        }

        Ok(sent_frames)
    }

    /// 统一的 PTY lifecycle gate。
    ///
    /// session core 只负责判断 frame 语义,不执行真正的 PTY 进程动作。
    pub fn lifecycle_decision_for_frame(frame: &ControlFrame) -> ControlPeerLifecycleDecision {
        match frame {
            ControlFrame::PtyDetached(frame) => ControlPeerLifecycleDecision::Detached {
                session_id: frame.session_id.clone(),
                reason: frame.reason.clone(),
            },
            ControlFrame::PtyExit(frame) => ControlPeerLifecycleDecision::TerminalComplete {
                frame_kind: "@pty-exit",
                session_id: frame.session_id.clone(),
                reason: frame.reason.clone(),
                exit_code: Some(frame.exit_code),
            },
            ControlFrame::PtyClosed(frame) => ControlPeerLifecycleDecision::TerminalComplete {
                frame_kind: "@pty-closed",
                session_id: frame.session_id.clone(),
                reason: frame.reason.clone(),
                exit_code: None,
            },
            _ => ControlPeerLifecycleDecision::Continue,
        }
    }

    /// 生成安全的观测摘要。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn frame_log_record(frame: &ControlFrame) -> ControlPeerFrameLogRecord {
        match frame {
            ControlFrame::ResponseLine(line) => ControlPeerFrameLogRecord {
                frame_kind: "@response",
                session_id: None,
                target_name: None,
                request_id: extract_response_request_id(line),
                payload_summary: format!("response_len={}", line.len()),
            },
            ControlFrame::SaveFile(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@savefile",
                session_id: None,
                target_name: None,
                request_id: frame.request_id,
                payload_summary: format!(
                    "filename={},mime={},encoding={},bytes={},quality={:?},width={:?},height={:?}",
                    frame.filename,
                    frame.mime,
                    frame.encoding,
                    frame.data.len(),
                    frame.quality,
                    frame.width,
                    frame.height
                ),
            },
            ControlFrame::PtyReady(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-ready",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!("cols={},rows={}", frame.cols, frame.rows),
            },
            ControlFrame::PtyOutput(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-output",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!("bytes={}", frame.data.len()),
            },
            ControlFrame::PtyExit(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-exit",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!("exit_code={},reason={}", frame.exit_code, frame.reason),
            },
            ControlFrame::PtyClosed(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-closed",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!("reason={}", frame.reason),
            },
            ControlFrame::PtyDetached(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-detached",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!("reason={}", frame.reason),
            },
            ControlFrame::PtyAttached(frame) => ControlPeerFrameLogRecord {
                frame_kind: "@pty-attached",
                session_id: Some(frame.session_id.clone()),
                target_name: None,
                request_id: None,
                payload_summary: format!(
                    "control_session_id={},cols={},rows={}",
                    frame.control_session_id, frame.cols, frame.rows
                ),
            },
        }
    }

    /// 注入 adapter 层观测字段。
    ///
    /// `target_name` 只是日志/trace 上下文,不是 session core 的路由语义。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn frame_log_record_with_adapter_target(
        frame: &ControlFrame,
        target_name: Option<&str>,
    ) -> ControlPeerFrameLogRecord {
        let mut record = Self::frame_log_record(frame);
        record.target_name = target_name.map(str::to_owned);
        record
    }
}

/// 统一路由 line-control 的结果 frame。
///
/// - `@response` 直接写回输出
/// - `@savefile` 由调用方注入 receiver 决定落盘
/// - PTY lifecycle frame 仍然是错误,因为这里处理的是普通 line-control response
pub fn route_line_control_result_frame<W, F>(
    frame: ControlFrame,
    output: &mut W,
    display: ControlResponseDisplay,
    mut savefile_receiver: F,
) -> io::Result<()>
where
    W: Write,
    F: FnMut(SaveFileFrame) -> io::Result<PathBuf>,
{
    match frame {
        ControlFrame::ResponseLine(response) => {
            write_response_for_display(output, &response, display)
        }
        ControlFrame::SaveFile(frame) => {
            let saved_path = savefile_receiver(frame)?;
            writeln!(output, "saved file: {}", saved_path.display())?;
            output.flush()
        }
        ControlFrame::PtyReady(_)
        | ControlFrame::PtyOutput(_)
        | ControlFrame::PtyExit(_)
        | ControlFrame::PtyClosed(_)
        | ControlFrame::PtyDetached(_)
        | ControlFrame::PtyAttached(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "line-control response 收到了意外 PTY frame",
        )),
    }
}

/// 把一条 wire message 解析并按 line-control 结果规则路由。
pub fn route_line_control_result_message<W, F>(
    message: &str,
    output: &mut W,
    display: ControlResponseDisplay,
    savefile_receiver: F,
) -> io::Result<()>
where
    W: Write,
    F: FnMut(SaveFileFrame) -> io::Result<PathBuf>,
{
    let frame = ControlFrame::parse_inbound_result_message(message)?;
    route_line_control_result_frame(frame, output, display, savefile_receiver)
}

fn extract_response_request_id(line: &str) -> Option<u64> {
    let trimmed = line.trim_start();
    let payload = trimmed.strip_prefix("@response ")?;
    let payload = payload.trim_start();
    let payload = payload.strip_prefix('{')?;

    let id_key = "\"id\":";
    let id_index = payload.find(id_key)?;
    let after_id = &payload[id_index + id_key.len()..];
    let digits: String = after_id
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn collect_request_ids(frames: &[ControlFrame]) -> Vec<u64> {
    let mut request_ids = Vec::<u64>::new();

    for frame in frames {
        let request_id = match frame {
            ControlFrame::ResponseLine(line) => extract_response_request_id(line),
            ControlFrame::SaveFile(frame) => frame.request_id,
            ControlFrame::PtyReady(_)
            | ControlFrame::PtyOutput(_)
            | ControlFrame::PtyExit(_)
            | ControlFrame::PtyClosed(_)
            | ControlFrame::PtyDetached(_)
            | ControlFrame::PtyAttached(_) => None,
        };

        if let Some(request_id) = request_id {
            if !request_ids.contains(&request_id) {
                request_ids.push(request_id);
            }
        }
    }

    request_ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_frames::{ControlExecutionOutcome, SaveFileFrame};

    #[test]
    fn should_emit_ordered_frames_without_owning_savefile_persistence() {
        let mut session = ControlPeerSession::new("peer-1");
        let mut sink = Vec::<u8>::new();
        let mut line_sink = LineWriteFrameSink::new(&mut sink);

        let report = session
            .dispatch_outcome(
                ControlExecutionOutcome {
                    outbound_frames: vec![
                        ControlFrame::SaveFile(SaveFileFrame {
                            request_id: Some(7),
                            filename: "shot.jpg".to_owned(),
                            mime: "image/jpeg".to_owned(),
                            encoding: "base64".to_owned(),
                            data: "QUJD".to_owned(),
                            quality: Some(75),
                            width: Some(1920),
                            height: Some(1080),
                        }),
                        ControlFrame::ResponseLine(r#"@response {"id":7,"value":0}"#.to_owned()),
                    ],
                },
                &mut line_sink,
            )
            .expect("dispatch should succeed");

        assert_eq!(report.session_id, "peer-1");
        assert_eq!(report.queued_frames, 2);
        assert_eq!(report.sent_frames, 2);
        assert_eq!(report.request_ids, vec![7]);
        let sink = String::from_utf8(sink).expect("sink should contain utf-8");
        let sink = sink.lines().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            sink,
            vec![
                r#"@savefile {"id":7,"filename":"shot.jpg","mime":"image/jpeg","encoding":"base64","quality":75,"width":1920,"height":1080,"data":"QUJD"}"#
                    .to_owned(),
                r#"@response {"id":7,"value":0}"#.to_owned(),
            ]
        );
    }

    #[test]
    fn should_dispatch_zero_one_and_many_frames_in_order() {
        let mut session = ControlPeerSession::new("peer-1");
        let mut sink = Vec::<u8>::new();

        {
            let mut line_sink = LineWriteFrameSink::new(&mut sink);
            let empty_report = session
                .dispatch_outcome(ControlExecutionOutcome::default(), &mut line_sink)
                .expect("empty dispatch should succeed");
            assert_eq!(empty_report.queued_frames, 0);
            assert_eq!(empty_report.sent_frames, 0);
            assert!(empty_report.request_ids.is_empty());

            let one_report = session
                .dispatch_outcome(
                    ControlExecutionOutcome::from_response_line(
                        r#"@response {"id":3,"value":0}"#.to_owned(),
                    ),
                    &mut line_sink,
                )
                .expect("single dispatch should succeed");
            assert_eq!(one_report.queued_frames, 1);
            assert_eq!(one_report.sent_frames, 1);
            assert_eq!(one_report.request_ids, vec![3]);

            let many_report = session
                .dispatch_outcome(
                    ControlExecutionOutcome {
                        outbound_frames: vec![
                            ControlFrame::ResponseLine("@response \"first\"".to_owned()),
                            ControlFrame::ResponseLine("@response \"second\"".to_owned()),
                        ],
                    },
                    &mut line_sink,
                )
                .expect("multi dispatch should succeed");
            assert_eq!(many_report.queued_frames, 2);
            assert_eq!(many_report.sent_frames, 2);
            assert!(many_report.request_ids.is_empty());
        }

        let text = String::from_utf8(sink).expect("sink should contain utf-8");
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            vec![
                r#"@response {"id":3,"value":0}"#,
                "@response \"first\"",
                "@response \"second\""
            ]
        );
    }

    #[test]
    fn should_not_log_savefile_base64_payload() {
        let frame = ControlFrame::SaveFile(SaveFileFrame {
            request_id: Some(7),
            filename: "shot.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: "QUJDREVGRw==".to_owned(),
            quality: Some(75),
            width: Some(1920),
            height: Some(1080),
        });

        let record = ControlPeerSession::frame_log_record(&frame);

        assert_eq!(record.frame_kind, "@savefile");
        assert_eq!(record.request_id, Some(7));
        assert!(record.payload_summary.contains("filename=shot.jpg"));
        assert!(record.payload_summary.contains("bytes=12"));
        assert!(!record.payload_summary.contains("QUJDREVGRw=="));
    }

    #[test]
    fn should_log_frame_kind_request_id_and_target_without_payload_body() {
        let frame = ControlFrame::ResponseLine(
            r#"@response {"id":42,"value":"SECRET_PAYLOAD"}"#.to_owned(),
        );

        let record =
            ControlPeerSession::frame_log_record_with_adapter_target(&frame, Some("mac.lab"));

        assert_eq!(record.frame_kind, "@response");
        assert_eq!(record.request_id, Some(42));
        assert_eq!(record.target_name.as_deref(), Some("mac.lab"));
        assert!(record.payload_summary.contains("response_len="));
        assert!(!record.payload_summary.contains("SECRET_PAYLOAD"));
    }

    #[test]
    fn should_emit_terminal_completion_only_for_terminal_frames() {
        let response = ControlFrame::ResponseLine(r#"@response {"id":7,"value":0}"#.to_owned());
        let exit = ControlFrame::PtyExit(crate::control_frames::PtyExitFrame {
            session_id: "pty-1".to_owned(),
            exit_code: 0,
            reason: "process_exit".to_owned(),
            ended_at: "2026-05-18T00:00:00Z".to_owned(),
        });
        let closed = ControlFrame::PtyClosed(crate::control_frames::PtyClosedFrame {
            session_id: "pty-1".to_owned(),
            reason: "force_close".to_owned(),
            ended_at: "2026-05-18T00:00:01Z".to_owned(),
        });
        let detached = ControlFrame::PtyDetached(crate::control_frames::PtyDetachedFrame {
            session_id: "pty-1".to_owned(),
            reason: "owner_detach".to_owned(),
            detached_at: "2026-05-18T00:00:02Z".to_owned(),
        });

        assert!(matches!(
            ControlPeerSession::lifecycle_decision_for_frame(&response),
            ControlPeerLifecycleDecision::Continue
        ));

        assert!(matches!(
            ControlPeerSession::lifecycle_decision_for_frame(&exit),
            ControlPeerLifecycleDecision::TerminalComplete {
                frame_kind: "@pty-exit",
                session_id,
                reason,
                exit_code: Some(0),
            } if session_id == "pty-1" && reason == "process_exit"
        ));

        assert!(matches!(
            ControlPeerSession::lifecycle_decision_for_frame(&closed),
            ControlPeerLifecycleDecision::TerminalComplete {
                frame_kind: "@pty-closed",
                session_id,
                reason,
                exit_code: None,
            } if session_id == "pty-1" && reason == "force_close"
        ));

        assert!(matches!(
            ControlPeerSession::lifecycle_decision_for_frame(&detached),
            ControlPeerLifecycleDecision::Detached { session_id, reason }
                if session_id == "pty-1" && reason == "owner_detach"
        ));
    }
}
