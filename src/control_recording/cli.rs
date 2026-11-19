//! 持久连接上的录制交互入口。

use super::{
    delivery::{response_value, save_recording},
    humantime::parse_humantime,
};
use crate::{
    control_frames::ControlFrame,
    control_invocation::{parse_port, resolve_control_invocation, ControlInvocation},
    control_transport::ControlTransport,
    zenoh_control::RecordingZenohBridge,
};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

pub type RecordCommandShared = crate::input::RecordCommandShared;
pub fn default_artifacts_dir() -> PathBuf {
    PathBuf::from("rdog_downloads")
}

pub(crate) enum RecordingConnection {
    Socket(ControlTransport),
    Zenoh(RecordingZenohBridge),
}
impl RecordingConnection {
    fn open(invocation: ControlInvocation) -> Result<Self, String> {
        match invocation {
            ControlInvocation::Tcp { host, port } => {
                ControlTransport::connect_tcp_with_retry(&host, parse_port(&port)?)
                    .map(Self::Socket)
                    .map_err(|e| e.to_string())
            }
            ControlInvocation::WebSocket { url } => ControlTransport::connect_websocket(&url)
                .map(Self::Socket)
                .map_err(|e| e.to_string()),
            ControlInvocation::Zenoh {
                namespace,
                target_name,
                entry_point,
            } => RecordingZenohBridge::open(namespace, target_name, entry_point)
                .map(Self::Zenoh)
                .map_err(|e| e.to_string()),
            ControlInvocation::ZenohLocal { namespace } => {
                let target = crate::zenoh_runtime::find_local_daemon_name(namespace.as_deref())
                    .map_err(|e| e.to_string())?;
                let namespace = namespace
                    .or_else(|| crate::zenoh_identity::infer_namespace_from_daemon_name(&target));
                RecordingZenohBridge::open(namespace, Some(target), Vec::new())
                    .map(Self::Zenoh)
                    .map_err(|e| e.to_string())
            }
        }
    }
    fn exchange(&mut self, line: &str) -> io::Result<Vec<ControlFrame>> {
        match self {
            Self::Zenoh(bridge) => bridge.send_recording_line(line),
            Self::Socket(transport) => {
                transport.write_message(line)?;
                let mut frames = Vec::new();
                loop {
                    let message = transport.read_message()?.ok_or_else(|| {
                        io::Error::new(io::ErrorKind::UnexpectedEof, "录制连接已关闭")
                    })?;
                    if message.len() > crate::control_line_read::MAX_CONTROL_MESSAGE_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "录制响应超过传输上限",
                        ));
                    }
                    let frame = ControlFrame::parse_inbound_result_message(&message)?;
                    let done = matches!(frame, ControlFrame::ResponseLine(_));
                    frames.push(frame);
                    if done {
                        return Ok(frames);
                    }
                }
            }
        }
    }
}

fn response(frames: &[ControlFrame]) -> Result<Value, String> {
    let [ControlFrame::ResponseLine(line)] = frames else {
        return Err("录制响应帧无效".into());
    };
    let value = response_value(line).map_err(|e| e.to_string())?;
    if value.get("schema").and_then(Value::as_str)
        != Some(super::control_handler::RECORD_CONTROL_SCHEMA)
    {
        return Err("不支持的录制响应版本".into());
    }
    Ok(value)
}

fn command_line(command: &str, id: Option<&str>) -> Result<String, String> {
    let mut words = command.split_whitespace();
    let name = words.next().ok_or("请输入命令")?;
    let payload = match name {
        "start" => {
            let mut profile = "semantic";
            let mut duration = None;
            let mut targets = Vec::new();
            while let Some(option) = words.next() {
                let value = words.next().ok_or("选项缺少值")?;
                match option {
                    "--target" | "--window" => {
                        let (pid, hwnd) = value.split_once(':').ok_or("目标格式为 PID:HWND")?;
                        let pid: u32 = pid.parse().map_err(|_| "无效 PID")?;
                        let hwnd = if let Some(hex) = value_after_hex(hwnd) {
                            u64::from_str_radix(hex, 16)
                        } else {
                            hwnd.parse()
                        }
                        .map_err(|_| "无效 HWND")?;
                        targets.push(json!({"pid":pid,"hwnd":hwnd}));
                    }
                    "--duration" => {
                        duration = Some(parse_humantime(value).map_err(|e| e.to_string())?)
                    }
                    "--profile" if value == "semantic" || value == "physical" => profile = value,
                    _ => return Err(format!("未知选项 {option}")),
                }
            }
            if targets.is_empty() {
                return Err("start 必须指定 --target PID:HWND".into());
            }
            let mut value = json!({"profile":profile,"targets":targets});
            if let Some(duration) = duration {
                value["duration_ms"] = json!(duration);
            }
            value
        }
        "status" | "stop" | "cancel" => {
            if words.next().is_some() {
                return Err("命令不接受参数".into());
            }
            match id {
                Some(id) => json!({"recording_id":id}),
                None if name == "status" => json!({}),
                None => return Err("没有活动录制".into()),
            }
        }
        "mark" => {
            let kind = words
                .next()
                .ok_or("mark 需要 checkpoint 或 parameter_boundary")?;
            if !matches!(kind, "checkpoint" | "parameter_boundary") || words.next().is_some() {
                return Err("mark 仅接受 checkpoint 或 parameter_boundary".into());
            }
            json!({"recording_id":id.ok_or("没有活动录制")?,"mark":kind})
        }
        _ => return Err("支持 start, status, mark, stop, cancel".into()),
    };
    Ok(format!("@record-{name}:{payload}"))
}
fn value_after_hex(value: &str) -> Option<&str> {
    value.strip_prefix("0x")
}

pub fn run(shared: &RecordCommandShared, artifacts_dir: &Path) -> Result<(), String> {
    let invocation = resolve_control_invocation(
        shared.transport,
        shared.url.clone(),
        shared.namespace.clone(),
        shared.target_name.clone(),
        shared.entry_point.clone(),
        shared.host.clone(),
    )?;
    let mut connection = RecordingConnection::open(invocation)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let prompt = if io::stdin().is_terminal() && io::stdout().is_terminal() {
            "record> "
        } else {
            ""
        };
        let mut editor = match rustyline::DefaultEditor::new() {
            Ok(editor) => editor,
            Err(_) => {
                let _ = sender.send(None);
                return;
            }
        };
        loop {
            match editor.readline(prompt) {
                Ok(line) => {
                    if sender.send(Some(line)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.send(None);
                    break;
                }
            }
        }
    });
    let mut request_id = 0_u64;
    run_session(
        &mut |line| {
            request_id = request_id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("录制请求编号耗尽"))?;
            let (header, payload) = line
                .split_once(':')
                .ok_or_else(|| io::Error::other("无效录制请求"))?;
            let frames = connection.exchange(&format!("{header}#{request_id}:{payload}"))?;
            let response = frames
                .last()
                .and_then(|frame| match frame {
                    ControlFrame::ResponseLine(line) => line.strip_prefix("@response "),
                    _ => None,
                })
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "录制请求缺少响应"))?;
            let envelope: Value = serde_json::from_str(response)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "录制响应无效"))?;
            if envelope.get("id").and_then(Value::as_u64) != Some(request_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "录制响应编号不匹配",
                ));
            }
            Ok(frames)
        },
        receiver,
        artifacts_dir,
    )
}

pub(super) fn run_session(
    exchange: &mut impl FnMut(&str) -> io::Result<Vec<ControlFrame>>,
    receiver: mpsc::Receiver<Option<String>>,
    artifacts_dir: &Path,
) -> Result<(), String> {
    let mut id: Option<String> = None;
    let mut interval = Duration::from_secs(15);
    let mut renewal = Instant::now() + interval;
    loop {
        let timeout = if id.is_some() {
            renewal.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(86400)
        };
        let next = if id.is_some() && Instant::now() >= renewal {
            Err(mpsc::RecvTimeoutError::Timeout)
        } else {
            receiver.recv_timeout(timeout)
        };
        let input = match next {
            Ok(Some(line)) => line,
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(id) = id.as_deref() {
                    let line = command_line("cancel", Some(id))?;
                    let frames = exchange(&line).map_err(|e| e.to_string())?;
                    response(&frames)?;
                }
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) if id.is_none() => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => "status".to_owned(),
        };
        let name = input.split_whitespace().next().unwrap_or("");
        if name == "start" && id.is_some() {
            eprintln!("已有活动录制");
            continue;
        }
        let line = match command_line(&input, id.as_deref()) {
            Ok(line) => line,
            Err(err) => {
                eprintln!("{err}");
                continue;
            }
        };
        let frames = exchange(&line).map_err(|e| e.to_string())?;
        if name == "stop" {
            let path = save_recording(&frames, id.as_deref().ok_or("没有活动录制")?, artifacts_dir)
                .map_err(|e| e.to_string())?;
            println!("local_verified: {}", path.display());
            return Ok(());
        }
        let value = response(&frames)?;
        println!("{value}");
        if name == "start" {
            id = Some(
                value
                    .get("recording_id")
                    .and_then(Value::as_str)
                    .ok_or("start 响应缺少 recording_id")?
                    .to_owned(),
            );
            let millis = value
                .pointer("/policy/renew_interval_ms")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .ok_or("start 响应缺少续租周期")?;
            interval = Duration::from_millis(millis);
            renewal = Instant::now() + interval;
        } else if name == "cancel" {
            return Ok(());
        }
        if name == "status" && id.is_some() {
            if value.get("recording_id").and_then(Value::as_str) != id.as_deref() {
                return Err("status 响应的 recording_id 不匹配".into());
            }
            if !matches!(
                value.get("phase").and_then(Value::as_str),
                Some("recording" | "recorded")
            ) {
                return Err("录制已结束或状态不可识别, 无可交付文件".into());
            }
        }
        if name == "status" && value.get("phase").and_then(Value::as_str) == Some("recorded") {
            let line = command_line("stop", id.as_deref())?;
            let frames = exchange(&line).map_err(|e| e.to_string())?;
            let path = save_recording(&frames, id.as_deref().ok_or("没有活动录制")?, artifacts_dir)
                .map_err(|e| e.to_string())?;
            println!("local_verified: {}", path.display());
            return Ok(());
        }
        if name == "status" && id.is_some() {
            renewal = Instant::now() + interval;
        }
    }
}
