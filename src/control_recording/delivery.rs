//! 录制产物的客户端校验与不覆盖提交。

use crate::{control_frames::ControlFrame, control_line_read::MAX_CONTROL_MESSAGE_BYTES};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn command_failure(value: &serde_json::Value) -> io::Error {
    let code = value
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("RECORD_COMMAND_FAILED");
    let message = match value
        .get("cleanup_residual_path")
        .and_then(serde_json::Value::as_str)
    {
        Some(path) => format!("录制命令失败 ({code:?}), 服务端私有暂存未清理 ({path:?})"),
        None => format!("录制命令失败 ({code:?})"),
    };
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn response_value(line: &str) -> io::Result<serde_json::Value> {
    let payload = line
        .strip_prefix("@response ")
        .ok_or_else(|| invalid("缺少 response"))?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| invalid("无效 response JSON"))?;
    if value.get("error").is_some()
        || value
            .get("code")
            .is_some_and(|code| code.as_i64() != Some(0))
    {
        return Err(command_failure(&value));
    }
    let value = value.get("value").cloned().unwrap_or(value);
    if value.get("error").is_some() {
        return Err(command_failure(&value));
    }
    Ok(value)
}

pub(crate) fn save_recording(
    frames: &[ControlFrame],
    recording_id: &str,
    directory: &Path,
) -> io::Result<PathBuf> {
    if let [ControlFrame::ResponseLine(response)] = frames {
        response_value(response)?;
    }
    let [ControlFrame::SaveFile(file), ControlFrame::ResponseLine(response)] = frames else {
        return Err(invalid("录制交付必须包含一个文件和一个响应"));
    };
    if file.to_wire_message().len() > MAX_CONTROL_MESSAGE_BYTES {
        return Err(invalid("录制文件帧超过传输上限"));
    }
    let envelope: serde_json::Value = serde_json::from_str(
        response
            .strip_prefix("@response ")
            .ok_or_else(|| invalid("缺少 response"))?,
    )
    .map_err(|_| invalid("无效 response JSON"))?;
    if envelope.get("id").and_then(|v| v.as_u64()) != file.request_id {
        return Err(invalid("录制文件与响应的请求编号不一致"));
    }
    let metadata = response_value(response)?;
    let filename = format!("{recording_id}.rdogrec.jsonl");
    if recording_id.is_empty()
        || !recording_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || metadata.get("schema").and_then(|v| v.as_str()) != Some("rdog.record-control.v2")
        || metadata.get("kind").and_then(|v| v.as_str()) != Some("record-stop")
        || metadata.get("phase").and_then(|v| v.as_str()) != Some("recorded")
        || metadata.get("recording_id").and_then(|v| v.as_str()) != Some(recording_id)
        || metadata.get("artifact_filename").and_then(|v| v.as_str()) != Some(filename.as_str())
        || file.filename != filename
        || file.mime != "application/x-ndjson"
        || file.encoding != "base64"
    {
        return Err(invalid("录制交付元数据不匹配"));
    }
    let bytes = STANDARD
        .decode(&file.data)
        .map_err(|_| invalid("录制文件 base64 无效"))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if metadata.get("artifact_size_bytes").and_then(|v| v.as_u64()) != Some(bytes.len() as u64)
        || metadata.get("artifact_sha256").and_then(|v| v.as_str()) != Some(digest.as_str())
    {
        return Err(invalid("录制文件长度或摘要不匹配"));
    }
    super::private_files::create_directory(directory)?;
    let temporary = directory.join(format!(".rdog-recording-{}.tmp", uuid::Uuid::new_v4()));
    let target = directory.join(filename);
    let mut output = super::private_files::create_new(&temporary)?;
    let result = (|| {
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        fs::hard_link(&temporary, &target)?;
        Ok(target)
    })();
    match (result, fs::remove_file(&temporary)) {
        (Ok(path), Ok(())) => Ok(path),
        (Err(err), Ok(())) => Err(err),
        (Ok(path), Err(err)) => Err(io::Error::new(
            err.kind(),
            format!("已保存 {}, 但临时文件清理失败: {err}", path.display()),
        )),
        (Err(err), Err(cleanup)) => Err(io::Error::new(
            err.kind(),
            format!("{err}; 临时文件清理失败: {cleanup}"),
        )),
    }
}
