use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

/// 控制面向外发送的统一 frame。
///
/// Phase 2 开始加入 `SaveFile`:
/// - `ResponseLine` 继续承载当前稳定的 `@response ...`
/// - `SaveFile` 作为第一个真实多 frame 能力,用于通知接收端直接落文件
/// - 后续再逐步加入 `Request` 等真正的双向主动帧
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFrame {
    ResponseLine(String),
    SaveFile(SaveFileFrame),
    PtyReady(PtyReadyFrame),
    PtyOutput(PtyOutputFrame),
    PtyExit(PtyExitFrame),
    PtyClosed(PtyClosedFrame),
    PtyDetached(PtyDetachedFrame),
    PtyAttached(PtyAttachedFrame),
}

/// `@savefile` frame 的最小稳定载荷。
///
/// 这里先只收口当前 screenshot 计划已经明确的字段。
/// 以后若需要更丰富的元信息,再继续往这个结构上长。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFileFrame {
    pub request_id: Option<u64>,
    pub filename: String,
    pub mime: String,
    pub encoding: String,
    pub data: String,
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyReadyFrame {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputFrame {
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExitFrame {
    pub session_id: String,
    pub exit_code: i32,
    pub reason: String,
    pub ended_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyClosedFrame {
    pub session_id: String,
    pub reason: String,
    pub ended_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyDetachedFrame {
    pub session_id: String,
    pub reason: String,
    pub detached_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyAttachedFrame {
    pub session_id: String,
    pub control_session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub attached_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyStdinFrame {
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyResizeFrame {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

/// 单次控制执行产出的统一结果。
///
/// 这里不再假设“永远只会回一条字符串”。
/// Phase 1 虽然仍只会塞入一条 `ResponseLine`,
/// 但调用方已经开始依赖更通用的 outcome 容器。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlExecutionOutcome {
    pub outbound_frames: Vec<ControlFrame>,
}

impl ControlExecutionOutcome {
    /// 构造一个只包含单条响应文本的 outcome。
    pub fn from_response_line(line: String) -> Self {
        Self {
            outbound_frames: vec![ControlFrame::ResponseLine(line)],
        }
    }

    /// Phase 1 兼容层:
    /// 当前大多数调用方仍然只会消费一条 `@response ...` 文本。
    ///
    /// 这里故意在“不符合 Phase 1 假设”时显式失败,
    /// 避免后面新增多 frame 后被静默吞掉。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn into_single_response_line(self) -> String {
        match self.outbound_frames.as_slice() {
            [ControlFrame::ResponseLine(line)] => line.clone(),
            [] => panic!("control execution outcome unexpectedly had no outbound frames"),
            _ => panic!("control execution outcome unexpectedly had multiple outbound frames"),
        }
    }

    /// 把 outcome 序列化成单个文本 payload。
    ///
    /// 当前主要给 Zenoh 现有 query/reply 兼容桥使用:
    /// - reply payload 仍然是一段 UTF-8 文本
    /// - 多个 frame 用换行拼接
    pub fn to_multiline_wire_payload(&self) -> String {
        self.outbound_frames
            .iter()
            .map(ControlFrame::to_wire_message)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl ControlFrame {
    /// 序列化成当前 transport 可直接发送的单条文本消息。
    pub fn to_wire_message(&self) -> String {
        match self {
            Self::ResponseLine(line) => line.clone(),
            Self::SaveFile(frame) => frame.to_wire_message(),
            Self::PtyReady(frame) => frame.to_wire_message(),
            Self::PtyOutput(frame) => frame.to_wire_message(),
            Self::PtyExit(frame) => frame.to_wire_message(),
            Self::PtyClosed(frame) => frame.to_wire_message(),
            Self::PtyDetached(frame) => frame.to_wire_message(),
            Self::PtyAttached(frame) => frame.to_wire_message(),
        }
    }

    /// 解析 client 侧收到的结果 frame。
    ///
    /// 当前只识别:
    /// - `@response ...`
    /// - `@savefile {...}`
    pub fn parse_inbound_result_message(message: &str) -> io::Result<Self> {
        let trimmed = message.trim_end_matches(['\r', '\n']);

        if let Some(payload) = trimmed.strip_prefix("@savefile ") {
            return Ok(Self::SaveFile(SaveFileFrame::parse_object_payload(
                payload,
            )?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-ready ") {
            return Ok(Self::PtyReady(PtyReadyFrame::parse_object_payload(
                payload,
            )?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-output ") {
            return Ok(Self::PtyOutput(PtyOutputFrame::parse_object_payload(
                payload,
            )?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-exit ") {
            return Ok(Self::PtyExit(PtyExitFrame::parse_object_payload(payload)?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-closed ") {
            return Ok(Self::PtyClosed(PtyClosedFrame::parse_object_payload(
                payload,
            )?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-detached ") {
            return Ok(Self::PtyDetached(PtyDetachedFrame::parse_object_payload(
                payload,
            )?));
        }

        if let Some(payload) = trimmed.strip_prefix("@pty-attached ") {
            return Ok(Self::PtyAttached(PtyAttachedFrame::parse_object_payload(
                payload,
            )?));
        }

        if trimmed.starts_with("@response ") {
            return Ok(Self::ResponseLine(trimmed.to_owned()));
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "当前只支持接收 @response / @savefile / @pty-ready / @pty-output / @pty-exit / @pty-closed / @pty-detached / @pty-attached,实际收到: {trimmed}"
            ),
        ))
    }

    /// 把一个多行文本 payload 拆成多条结果 frame。
    ///
    /// 主要给当前 Zenoh query/reply 兼容桥使用。
    pub fn parse_inbound_result_payload(payload: &str) -> io::Result<Vec<Self>> {
        let mut frames = Vec::new();

        for line in payload.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            frames.push(Self::parse_inbound_result_message(trimmed)?);
        }

        if frames.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "控制结果 payload 为空,未包含任何 frame",
            ));
        }

        Ok(frames)
    }
}

impl PtyReadyFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-ready {{\"session_id\":\"{}\",\"cols\":{},\"rows\":{}}}",
            escape_json_string(&self.session_id),
            self.cols,
            self.rows
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut cols = None::<u16>;
        let mut rows = None::<u16>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "cols" => cols = Some(parse_u16(raw_value, "cols")?),
                "rows" => rows = Some(parse_u16(raw_value, "rows")?),
                _ => return Err(unknown_pty_field("@pty-ready", &name)),
            }
        }

        Ok(Self {
            session_id: require_string_field("@pty-ready", "session_id", session_id)?,
            cols: cols.unwrap_or(80),
            rows: rows.unwrap_or(24),
        })
    }
}

impl PtyOutputFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-output {{\"session_id\":\"{}\",\"encoding\":\"base64\",\"data\":\"{}\"}}",
            escape_json_string(&self.session_id),
            escape_json_string(&self.data)
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut encoding = None::<String>;
        let mut data = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "encoding" => encoding = Some(parse_json_string(raw_value)?),
                "data" => data = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-output", &name)),
            }
        }

        let encoding = encoding.unwrap_or_else(|| "base64".to_owned());
        if !encoding.eq_ignore_ascii_case("base64") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@pty-output 只支持 base64 编码,实际是 {encoding}"),
            ));
        }

        Ok(Self {
            session_id: require_string_field("@pty-output", "session_id", session_id)?,
            data: require_string_field("@pty-output", "data", data)?,
        })
    }
}

impl PtyExitFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-exit {{\"session_id\":\"{}\",\"exit_code\":{},\"reason\":\"{}\",\"ended_at\":\"{}\"}}",
            escape_json_string(&self.session_id),
            self.exit_code,
            escape_json_string(&self.reason),
            escape_json_string(&self.ended_at),
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut exit_code = None::<i32>;
        let mut reason = None::<String>;
        let mut ended_at = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "exit_code" => exit_code = Some(parse_i32(raw_value, "exit_code")?),
                "reason" => reason = Some(parse_json_string(raw_value)?),
                "ended_at" => ended_at = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-exit", &name)),
            }
        }

        Ok(Self {
            session_id: require_string_field("@pty-exit", "session_id", session_id)?,
            exit_code: exit_code.unwrap_or(-1),
            reason: require_string_field("@pty-exit", "reason", reason)?,
            ended_at: require_string_field("@pty-exit", "ended_at", ended_at)?,
        })
    }
}

impl PtyClosedFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-closed {{\"session_id\":\"{}\",\"reason\":\"{}\",\"ended_at\":\"{}\"}}",
            escape_json_string(&self.session_id),
            escape_json_string(&self.reason),
            escape_json_string(&self.ended_at),
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut reason = None::<String>;
        let mut ended_at = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "reason" => reason = Some(parse_json_string(raw_value)?),
                "ended_at" => ended_at = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-closed", &name)),
            }
        }

        Ok(Self {
            session_id: require_string_field("@pty-closed", "session_id", session_id)?,
            reason: require_string_field("@pty-closed", "reason", reason)?,
            ended_at: require_string_field("@pty-closed", "ended_at", ended_at)?,
        })
    }
}

impl PtyStdinFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-stdin {{\"session_id\":\"{}\",\"encoding\":\"base64\",\"data\":\"{}\"}}",
            escape_json_string(&self.session_id),
            escape_json_string(&self.data)
        )
    }

    pub fn parse_wire_message(message: &str) -> io::Result<Option<Self>> {
        let trimmed = message.trim_end_matches(['\r', '\n']);
        let Some(payload) = trimmed.strip_prefix("@pty-stdin ") else {
            return Ok(None);
        };

        let mut session_id = None::<String>;
        let mut encoding = None::<String>;
        let mut data = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "encoding" => encoding = Some(parse_json_string(raw_value)?),
                "data" => data = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-stdin", &name)),
            }
        }

        let encoding = encoding.unwrap_or_else(|| "base64".to_owned());
        if !encoding.eq_ignore_ascii_case("base64") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@pty-stdin 只支持 base64 编码,实际是 {encoding}"),
            ));
        }

        Ok(Some(Self {
            session_id: require_string_field("@pty-stdin", "session_id", session_id)?,
            data: require_string_field("@pty-stdin", "data", data)?,
        }))
    }
}

impl PtyResizeFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-resize {{\"session_id\":\"{}\",\"cols\":{},\"rows\":{}}}",
            escape_json_string(&self.session_id),
            self.cols,
            self.rows
        )
    }

    pub fn parse_wire_message(message: &str) -> io::Result<Option<Self>> {
        let trimmed = message.trim_end_matches(['\r', '\n']);
        let Some(payload) = trimmed.strip_prefix("@pty-resize ") else {
            return Ok(None);
        };

        let mut session_id = None::<String>;
        let mut cols = None::<u16>;
        let mut rows = None::<u16>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "cols" => cols = Some(parse_u16(raw_value, "cols")?),
                "rows" => rows = Some(parse_u16(raw_value, "rows")?),
                _ => return Err(unknown_pty_field("@pty-resize", &name)),
            }
        }

        Ok(Some(Self {
            session_id: require_string_field("@pty-resize", "session_id", session_id)?,
            cols: require_pty_dimension("@pty-resize", "cols", cols)?,
            rows: require_pty_dimension("@pty-resize", "rows", rows)?,
        }))
    }
}

impl PtyDetachedFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-detached {{\"session_id\":\"{}\",\"reason\":\"{}\",\"detached_at\":\"{}\"}}",
            escape_json_string(&self.session_id),
            escape_json_string(&self.reason),
            escape_json_string(&self.detached_at),
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut reason = None::<String>;
        let mut detached_at = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "reason" => reason = Some(parse_json_string(raw_value)?),
                "detached_at" => detached_at = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-detached", &name)),
            }
        }

        Ok(Self {
            session_id: require_string_field("@pty-detached", "session_id", session_id)?,
            reason: require_string_field("@pty-detached", "reason", reason)?,
            detached_at: require_string_field("@pty-detached", "detached_at", detached_at)?,
        })
    }
}

impl PtyAttachedFrame {
    pub fn to_wire_message(&self) -> String {
        format!(
            "@pty-attached {{\"session_id\":\"{}\",\"control_session_id\":\"{}\",\"cols\":{},\"rows\":{},\"attached_at\":\"{}\"}}",
            escape_json_string(&self.session_id),
            escape_json_string(&self.control_session_id),
            self.cols,
            self.rows,
            escape_json_string(&self.attached_at),
        )
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let mut session_id = None::<String>;
        let mut control_session_id = None::<String>;
        let mut cols = None::<u16>;
        let mut rows = None::<u16>;
        let mut attached_at = None::<String>;

        for (name, raw_value) in parse_object_fields(payload)? {
            match name.as_str() {
                "session_id" => session_id = Some(parse_json_string(raw_value)?),
                "control_session_id" => control_session_id = Some(parse_json_string(raw_value)?),
                "cols" => cols = Some(parse_u16(raw_value, "cols")?),
                "rows" => rows = Some(parse_u16(raw_value, "rows")?),
                "attached_at" => attached_at = Some(parse_json_string(raw_value)?),
                _ => return Err(unknown_pty_field("@pty-attached", &name)),
            }
        }

        Ok(Self {
            session_id: require_string_field("@pty-attached", "session_id", session_id)?,
            control_session_id: require_string_field(
                "@pty-attached",
                "control_session_id",
                control_session_id,
            )?,
            cols: cols.unwrap_or(80),
            rows: rows.unwrap_or(24),
            attached_at: require_string_field("@pty-attached", "attached_at", attached_at)?,
        })
    }
}

impl SaveFileFrame {
    pub fn to_wire_message(&self) -> String {
        let mut payload = String::new();
        payload.push('{');

        let mut first = true;
        append_json_field_number(&mut payload, &mut first, "id", self.request_id);
        append_json_field_string(&mut payload, &mut first, "filename", &self.filename);
        append_json_field_string(&mut payload, &mut first, "mime", &self.mime);
        append_json_field_string(&mut payload, &mut first, "encoding", &self.encoding);
        append_json_field_number(&mut payload, &mut first, "quality", self.quality);
        append_json_field_number(&mut payload, &mut first, "width", self.width);
        append_json_field_number(&mut payload, &mut first, "height", self.height);
        append_json_field_string(&mut payload, &mut first, "data", &self.data);
        payload.push('}');

        format!("@savefile {payload}")
    }

    pub fn save_to_directory(&self, base_dir: &Path) -> io::Result<PathBuf> {
        if !self.encoding.eq_ignore_ascii_case("base64") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("当前 @savefile 只支持 base64 编码,实际是 {}", self.encoding),
            ));
        }

        let sanitized_name = sanitize_filename(&self.filename)?;
        fs::create_dir_all(base_dir)?;
        let target_path = allocate_save_path(base_dir, &sanitized_name);
        let bytes = BASE64_STANDARD
            .decode(self.data.as_bytes())
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@savefile base64 数据无法解码: {err}"),
                )
            })?;
        fs::write(&target_path, bytes)?;
        Ok(target_path)
    }

    pub fn parse_object_payload(payload: &str) -> io::Result<Self> {
        let trimmed = payload.trim();
        let inner = trimmed
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "@savefile payload 必须是 JSON object 形态",
                )
            })?
            .trim();

        if inner.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@savefile payload 不能为空",
            ));
        }

        let mut request_id = None;
        let mut filename = None::<String>;
        let mut mime = None::<String>;
        let mut encoding = None::<String>;
        let mut data = None::<String>;
        let mut quality = None::<u8>;
        let mut width = None::<u32>;
        let mut height = None::<u32>;

        for field in split_object_fields(inner)? {
            let (name, raw_value) = split_object_field(field)?;
            let name = normalize_field_name(name)?;
            let raw_value = raw_value.trim();

            match name.as_str() {
                "id" => request_id = Some(parse_optional_u64(raw_value)?),
                "filename" => filename = Some(parse_json_string(raw_value)?),
                "mime" => mime = Some(parse_json_string(raw_value)?),
                "encoding" => encoding = Some(parse_json_string(raw_value)?),
                "data" => data = Some(parse_json_string(raw_value)?),
                "quality" => quality = Some(parse_u8(raw_value, "quality")?),
                "width" => width = Some(parse_u32(raw_value, "width")?),
                "height" => height = Some(parse_u32(raw_value, "height")?),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@savefile payload 包含未知字段: {name}"),
                    ))
                }
            }
        }

        Ok(Self {
            request_id: request_id.flatten(),
            filename: filename.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "@savefile 缺少 filename")
            })?,
            mime: mime
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "@savefile 缺少 mime"))?,
            encoding: encoding.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "@savefile 缺少 encoding")
            })?,
            data: data
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "@savefile 缺少 data"))?,
            quality,
            width,
            height,
        })
    }
}

fn parse_object_fields(payload: &str) -> io::Result<Vec<(String, &str)>> {
    let trimmed = payload.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("PTY frame payload 必须是 JSON object 形态: {payload}"),
            )
        })?
        .trim();

    if inner.is_empty() {
        return Ok(Vec::new());
    }

    split_object_fields(inner)?
        .into_iter()
        .map(|field| {
            let (name, raw_value) = split_object_field(field)?;
            Ok((normalize_field_name(name)?, raw_value.trim()))
        })
        .collect()
}

fn require_string_field(kind: &str, field_name: &str, value: Option<String>) -> io::Result<String> {
    let value = value.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 缺少必填字段 `{field_name}`"),
        )
    })?;

    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 的 `{field_name}` 不能为空"),
        ));
    }

    Ok(value)
}

fn require_pty_dimension(kind: &str, field_name: &str, value: Option<u16>) -> io::Result<u16> {
    let value = value.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 缺少必填字段 `{field_name}`"),
        )
    })?;

    if value == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{kind} 的 `{field_name}` 必须大于 0"),
        ));
    }

    Ok(value)
}

fn unknown_pty_field(kind: &str, field_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{kind} payload 包含未知字段: {field_name}"),
    )
}

/// 默认保存目录。
///
/// 当前统一收敛到工作目录下的 `rdog_downloads/`。
/// 后续若要做可配置路径,以这里为单一收口点继续扩展。
pub fn default_savefile_directory() -> io::Result<PathBuf> {
    Ok(std::env::current_dir()?.join("rdog_downloads"))
}

fn append_json_field_string(buffer: &mut String, first: &mut bool, key: &str, value: &str) {
    append_field_prefix(buffer, first, key);
    let escaped = escape_json_string(value);
    let _ = write!(buffer, "\"{escaped}\"");
}

fn append_json_field_number<T>(buffer: &mut String, first: &mut bool, key: &str, value: Option<T>)
where
    T: std::fmt::Display,
{
    let Some(value) = value else {
        return;
    };
    append_field_prefix(buffer, first, key);
    let _ = write!(buffer, "{value}");
}

fn append_field_prefix(buffer: &mut String, first: &mut bool, key: &str) {
    if !*first {
        buffer.push(',');
    }
    *first = false;
    let _ = write!(buffer, "\"{key}\":");
}

fn sanitize_filename(input: &str) -> io::Result<String> {
    let candidate = Path::new(input)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("非法保存文件名: {input}"),
            )
        })?;

    Ok(candidate.to_owned())
}

fn allocate_save_path(base_dir: &Path, filename: &str) -> PathBuf {
    let initial = base_dir.join(filename);
    if !initial.exists() {
        return initial;
    }

    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|ext| ext.to_str());

    for index in 2.. {
        let candidate_name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = base_dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded filename allocation loop should always return");
}

fn split_object_fields(input: &str) -> io::Result<Vec<&str>> {
    let mut fields = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in input.as_bytes().iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b',' if !in_string => {
                let field = input[start..index].trim();
                if field.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@savefile payload 存在空字段: {input}"),
                    ));
                }
                fields.push(field);
                start = index + 1;
            }
            _ => {}
        }
    }

    if in_string {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile payload 存在未闭合字符串: {input}"),
        ));
    }

    let tail = input[start..].trim();
    if tail.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile payload 末尾存在空字段: {input}"),
        ));
    }
    fields.push(tail);
    Ok(fields)
}

fn split_object_field(field: &str) -> io::Result<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in field.as_bytes().iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b':' if !in_string => {
                let field_name = field[..index].trim();
                let field_value = field[index + 1..].trim();
                if field_name.is_empty() || field_value.is_empty() {
                    break;
                }
                return Ok((field_name, field_value));
            }
            _ => {}
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("@savefile 字段格式非法: {field}"),
    ))
}

fn normalize_field_name(field_name: &str) -> io::Result<String> {
    let trimmed = field_name.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@savefile 字段名不能为空",
        ));
    }
    Ok(trimmed.trim_matches('"').to_ascii_lowercase())
}

fn parse_optional_u64(input: &str) -> io::Result<Option<u64>> {
    if input.eq_ignore_ascii_case("null") {
        return Ok(None);
    }

    input.parse::<u64>().map(Some).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile 的 id 必须是无符号整数或 null: {input}"),
        )
    })
}

fn parse_u8(input: &str, field_name: &str) -> io::Result<u8> {
    input.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile 的 {field_name} 必须是无符号整数: {input}"),
        )
    })
}

fn parse_u16(input: &str, field_name: &str) -> io::Result<u16> {
    input.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("PTY frame 的 {field_name} 必须是无符号整数: {input}"),
        )
    })
}

fn parse_u32(input: &str, field_name: &str) -> io::Result<u32> {
    input.parse::<u32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile 的 {field_name} 必须是无符号整数: {input}"),
        )
    })
}

fn parse_i32(input: &str, field_name: &str) -> io::Result<i32> {
    input.parse::<i32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("PTY frame 的 {field_name} 必须是整数: {input}"),
        )
    })
}

fn parse_json_string(input: &str) -> io::Result<String> {
    let bytes = input.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@savefile 的字符串字段必须使用双引号包裹: {input}"),
        ));
    }

    let mut decoded = String::with_capacity(input.len().saturating_sub(2));
    let mut chars = input[1..input.len() - 1].chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        let escaped = chars.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@savefile 字符串转义不完整: {input}"),
            )
        })?;

        match escaped {
            '"' => decoded.push('"'),
            '\\' => decoded.push('\\'),
            '/' => decoded.push('/'),
            'b' => decoded.push('\u{08}'),
            'f' => decoded.push('\u{0C}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'u' => {
                let hex = chars.by_ref().take(4).collect::<String>();
                if hex.len() != 4 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@savefile unicode 转义不完整: {input}"),
                    ));
                }
                let code = u32::from_str_radix(&hex, 16).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@savefile unicode 转义非法: {input}"),
                    )
                })?;
                let Some(decoded_char) = char::from_u32(code) else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@savefile unicode 转义非法: {input}"),
                    ));
                };
                decoded.push(decoded_char);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@savefile 字符串转义不支持该值: \\{other}"),
                ));
            }
        }
    }

    Ok(decoded)
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
                let _ = write!(escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_millis();
        std::env::temp_dir().join(format!("rdog-{name}-{millis}-{}", std::process::id()))
    }

    #[test]
    fn save_file_frame_should_roundtrip_wire_message() {
        let frame = SaveFileFrame {
            request_id: Some(7),
            filename: "shot.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: "QUJD".to_owned(),
            quality: Some(75),
            width: Some(1920),
            height: Some(1080),
        };

        let wire = frame.to_wire_message();
        let parsed =
            ControlFrame::parse_inbound_result_message(&wire).expect("wire message should parse");

        assert_eq!(parsed, ControlFrame::SaveFile(frame));
    }

    #[test]
    fn outcome_should_serialize_and_parse_multiline_wire_payload() {
        let outcome = ControlExecutionOutcome {
            outbound_frames: vec![
                ControlFrame::SaveFile(SaveFileFrame {
                    request_id: Some(7),
                    filename: "shot.jpg".to_owned(),
                    mime: "image/jpeg".to_owned(),
                    encoding: "base64".to_owned(),
                    data: "QUJD".to_owned(),
                    quality: Some(75),
                    width: Some(100),
                    height: Some(60),
                }),
                ControlFrame::ResponseLine(r#"@response {"id":7,"value":0}"#.to_owned()),
            ],
        };

        let payload = outcome.to_multiline_wire_payload();
        let parsed =
            ControlFrame::parse_inbound_result_payload(&payload).expect("payload should parse");

        assert_eq!(parsed, outcome.outbound_frames);
    }

    #[test]
    fn pty_frames_should_roundtrip_wire_messages() {
        let frames = vec![
            ControlFrame::PtyReady(PtyReadyFrame {
                session_id: "session-1".to_owned(),
                cols: 120,
                rows: 40,
            }),
            ControlFrame::PtyOutput(PtyOutputFrame {
                session_id: "session-1".to_owned(),
                data: "QUJD".to_owned(),
            }),
            ControlFrame::PtyExit(PtyExitFrame {
                session_id: "session-1".to_owned(),
                exit_code: 0,
                reason: "process_exit".to_owned(),
                ended_at: "1778042000".to_owned(),
            }),
            ControlFrame::PtyClosed(PtyClosedFrame {
                session_id: "session-1".to_owned(),
                reason: "force_close".to_owned(),
                ended_at: "1778042001".to_owned(),
            }),
        ];

        for frame in frames {
            let wire = frame.to_wire_message();
            let parsed =
                ControlFrame::parse_inbound_result_message(&wire).expect("PTY frame should parse");
            assert_eq!(parsed, frame);
        }
    }

    #[test]
    fn pty_stdin_frame_should_roundtrip_wire_message() {
        let frame = PtyStdinFrame {
            session_id: "session-1".to_owned(),
            data: "QUJD".to_owned(),
        };

        let parsed = PtyStdinFrame::parse_wire_message(&frame.to_wire_message())
            .expect("PTY stdin parse should succeed")
            .expect("PTY stdin frame should be present");

        assert_eq!(parsed, frame);
    }

    #[test]
    fn pty_resize_frame_should_roundtrip_wire_message() {
        let frame = PtyResizeFrame {
            session_id: "session-1".to_owned(),
            cols: 132,
            rows: 43,
        };

        let parsed = PtyResizeFrame::parse_wire_message(&frame.to_wire_message())
            .expect("PTY resize parse should succeed")
            .expect("PTY resize frame should be present");

        assert_eq!(parsed, frame);
    }

    #[test]
    fn pty_resize_frame_should_reject_zero_dimensions() {
        let err = PtyResizeFrame::parse_wire_message(
            r#"@pty-resize {"session_id":"session-1","cols":0,"rows":43}"#,
        )
        .expect_err("zero cols should be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_file_frame_should_save_decoded_bytes_to_directory() {
        let dir = unique_temp_dir("savefile");
        let frame = SaveFileFrame {
            request_id: None,
            filename: "shot.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: BASE64_STANDARD.encode(b"ABC"),
            quality: Some(75),
            width: None,
            height: None,
        };

        let saved = frame
            .save_to_directory(&dir)
            .expect("@savefile should save successfully");
        let bytes = fs::read(&saved).expect("saved file should exist");

        assert_eq!(bytes, b"ABC");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_file_frame_should_auto_rename_when_target_exists() {
        let dir = unique_temp_dir("savefile-rename");
        fs::create_dir_all(&dir).expect("temp dir should create");
        fs::write(dir.join("shot.jpg"), b"OLD").expect("seed file should write");

        let frame = SaveFileFrame {
            request_id: None,
            filename: "shot.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            encoding: "base64".to_owned(),
            data: BASE64_STANDARD.encode(b"NEW"),
            quality: None,
            width: None,
            height: None,
        };

        let saved = frame
            .save_to_directory(&dir)
            .expect("@savefile should save with renamed file");

        assert_eq!(
            saved.file_name().and_then(|name| name.to_str()),
            Some("shot-2.jpg")
        );
        assert_eq!(fs::read(&saved).expect("renamed file should exist"), b"NEW");

        let _ = fs::remove_dir_all(&dir);
    }
}
