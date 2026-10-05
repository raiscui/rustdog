use rustyline::{error::ReadlineError, DefaultEditor};
use std::io::{self, IsTerminal};

use crate::control_line_read::{
    read_line_limited, LineOutcome, MAX_CONTROL_MESSAGE_BYTES,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ControlStdinAction {
    Continue,
    Break,
}

/// 按 `rdog control` 的本地输入语义逐行读取 stdin。
///
/// 设计口径:
/// - 真实 TTY: 接入 `rustyline`,让左右方向键、历史、基础行编辑都留在本地
/// - pipe / 重定向: 继续保留原来的逐行读取行为,避免破坏脚本调用
pub fn for_each_control_stdin_line(
    on_line: impl FnMut(String) -> io::Result<ControlStdinAction>,
) -> io::Result<()> {
    let stdin = io::stdin();

    if stdin.is_terminal() {
        return for_each_terminal_line(on_line);
    }

    for_each_buffered_line(stdin, on_line)
}

fn for_each_terminal_line(
    mut on_line: impl FnMut(String) -> io::Result<ControlStdinAction>,
) -> io::Result<()> {
    let mut editor = DefaultEditor::new().map_err(to_io_error)?;

    loop {
        match editor.readline("") {
            Ok(line) => {
                if !line.is_empty() {
                    editor
                        .add_history_entry(line.as_str())
                        .map_err(to_io_error)?;
                }
                if on_line(line)? == ControlStdinAction::Break {
                    return Ok(());
                }
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => return Ok(()),
            Err(err) => return Err(to_io_error(err)),
        }
    }
}

fn for_each_buffered_line(
    stdin: io::Stdin,
    mut on_line: impl FnMut(String) -> io::Result<ControlStdinAction>,
) -> io::Result<()> {
    let mut reader = io::BufReader::new(stdin.lock());
    let mut buf = Vec::new();
    let mut sink = Vec::new();

    loop {
        match read_line_limited(&mut reader, &mut buf, &mut sink, MAX_CONTROL_MESSAGE_BYTES)? {
            // 超限行: 已消费丢弃, 提示后继续读 —— 控制端敲了一行超长文本
            // 不应该导致整个会话退出。
            LineOutcome::TooLong { consumed_bytes } => {
                eprintln!(
                    "[rdog] 输入行超过 {} 字节上限, 已丢弃 ({} 字节)",
                    MAX_CONTROL_MESSAGE_BYTES,
                    consumed_bytes
                );
                continue;
            }
            LineOutcome::Eof => return Ok(()),
            LineOutcome::Line(line) => {
                if on_line(line)? == ControlStdinAction::Break {
                    return Ok(());
                }
            }
        }
    }
}

fn to_io_error(err: ReadlineError) -> io::Error {
    io::Error::other(err.to_string())
}
