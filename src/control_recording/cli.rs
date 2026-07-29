//! `rdog record` CLI dispatcher。
//!
//! 5 个子命令 (start/status/mark/stop/cancel) 都生成对应 line-control
//! `@record-*` 文本行, 然后复用 `control_invocation` 现有 transport
//! 分发路径, 不引入新依赖或新协议。

use std::path::{Path, PathBuf};

use crate::{
    control_invocation::{
        resolve_control_invocation, send_control_lines_for_invocation,
    },
    input::Transport,
};

/// CLI 5 个子命令的 high-level 视图, 由 input.rs 直接 derive。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordCommand {
    Start { profile: String },
    Status,
    Mark { label: Option<String>, redaction_active: bool },
    Stop,
    Cancel,
}

pub type RecordCommandShared = crate::input::RecordCommandShared;


/// 把 subcommand 翻译成对应 line-control 文本行 (无末尾 `\n`)。
pub fn render_line(subcommand: &RecordCommand) -> Result<String, String> {
    let line = match subcommand {
        RecordCommand::Start { profile } => match profile.as_str() {
            "semantic" | "physical" => format!("@record-start:{{\"profile\":\"{profile}\"}}"),
            other => return Err(format!("`rdog record start` profile 必须是 semantic 或 physical,收到 {other}")),
        },
        RecordCommand::Status => "@record-status".to_owned(),
        RecordCommand::Mark { label, redaction_active } => {
            let label_field = match label {
                Some(value) => format!(",\"label\":\"{}\"", escape_json_string(value)),
                None => String::new(),
            };
            format!(
                "@record-mark:{{\"redaction_active\":{}{label_field}}}",
                if *redaction_active { "true" } else { "false" }
            )
        }
        RecordCommand::Stop => "@record-stop:{}".to_owned(),
        RecordCommand::Cancel => "@record-cancel:{}".to_owned(),
    };
    Ok(line)
}

/// 解析 invocation, 发送单行控制命令, 落盘 `@savefile` 资产到 `artifacts_dir`。
///
/// 复用 `send_control_lines_for_invocation` 的多帧 + artifact 落盘能力,
/// 把单 subcommand 当 N=1 的 one-shot。
pub fn run(
    subcommand: RecordCommand,
    shared: &RecordCommandShared,
    artifacts_dir: &Path,
) -> Result<(), String> {
    let line = render_line(&subcommand)?;
    let invocation = resolve_control_invocation(
        shared.transport,
        shared.url.as_deref().map(|s| s.to_owned()),
        shared.namespace.as_deref().map(|s| s.to_owned()),
        shared.target_name.as_deref().map(|s| s.to_owned()),
        shared.entry_point.clone(),
        shared.host.clone(),
    )?;
    let lines = vec![line];
    send_control_lines_for_invocation(&invocation, &lines, artifacts_dir).map(|_| ())
}

/// JSON string 转义, 只覆盖 line-control 协议实际需要的控制字符。
fn escape_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

/// 默认 artifacts 目录 (`rdog_downloads/`) 在 `Cargo` 仓库根或 cwd。
pub fn default_artifacts_dir() -> PathBuf {
    PathBuf::from("rdog_downloads")
}

// Suppress dead_code for UiScriptCommandShared (re-exported for future
// cross-CLI unification; this CLI currently only uses RecordCommandShared).
