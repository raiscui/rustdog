// ax_diff/output.rs
//
// 三种输出格式:
//   - text: 默认, 人/agent 友好, 包含 summary 一行 + windows/elements
//     改动详情, 每个 field change 含 before/after 简明值。
//   - json: 完整结构化 diff, 方便程序消费。
//   - summary: 单行计数 windows: +N -N ~N | elements: +N -N ~N。

use crate::ax_diff::types::{DiffReport, ElementDiffKind, WindowDiffKind};
use serde_json::Value;
use std::io::{self, Write};

pub fn write_summary_report(out: &mut dyn Write, r: &DiffReport) -> io::Result<()> {
    writeln!(
        out,
        "windows: +{} -{} ~{} | elements: +{} -{} ~{}",
        r.windows_added,
        r.windows_removed,
        r.windows_modified,
        r.elements_added,
        r.elements_removed,
        r.elements_modified
    )
}

pub fn write_json_report(out: &mut dyn Write, r: &DiffReport) -> io::Result<()> {
    let text = serde_json::to_string_pretty(r)
        .map_err(|err| io::Error::other(format!("序列化 diff 报告失败: {err}")))?;
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")
}

pub fn write_text_report(
    out: &mut dyn Write,
    r: &DiffReport,
    quiet: bool,
    top_changes: Option<usize>,
) -> io::Result<()> {
    write_summary_report(out, r)?;
    if quiet {
        return Ok(());
    }
    // window 维度改动先打 (数量通常远小于 element), 不受 top_changes 限制。
    for wd in &r.windows {
        match wd.kind {
            WindowDiffKind::Added => writeln!(out, "\n[window +] {} (新增窗口)", wd.id)?,
            WindowDiffKind::Removed => writeln!(out, "\n[window -] {} (整窗消失)", wd.id)?,
            WindowDiffKind::Modified => {
                writeln!(out, "\n[window ~] {} (字段改动)", wd.id)?;
                for fc in &wd.changed_fields {
                    writeln!(
                        out,
                        "    {} : {} -> {}",
                        fc.field,
                        short_value(&fc.before),
                        short_value(&fc.after)
                    )?;
                }
            }
        }
    }
    // element 维度改动按 id 字典序输出, top_changes 限制前 N 条,
    // 超出截断并提示, 让 model 在 context 紧张时只看 N 条最相关的改动。
    let total_elements = r.elements.len();
    let take = top_changes.unwrap_or(total_elements).min(total_elements);
    let mut shown: usize = 0;
    let mut truncated: usize = 0;
    for (eid, ed) in &r.elements {
        if shown >= take {
            truncated = total_elements - shown;
            break;
        }
        match ed.kind {
            ElementDiffKind::Added => {
                writeln!(out, "\n[element +] {} (window: {})", eid, ed.window_id)?;
            }
            ElementDiffKind::Removed => {
                writeln!(out, "\n[element -] {} (window: {})", eid, ed.window_id)?;
            }
            ElementDiffKind::Modified => {
                writeln!(out, "\n[element ~] {} (window: {})", eid, ed.window_id)?;
                for fc in &ed.changed_fields {
                    writeln!(
                        out,
                        "    {} : {} -> {}",
                        fc.field,
                        short_value(&fc.before),
                        short_value(&fc.after)
                    )?;
                }
            }
        }
        shown += 1;
    }
    if truncated > 0 {
        writeln!(
            out,
            "\n... 还有 {} 个 element 改动被截断 (--top-changes {})。如需完整列表, 用 --format json。",
            truncated, take
        )?;
    }
    Ok(())
}

fn short_value(v: &Value) -> String {
    let s = match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    if s.len() > 120 {
        format!("{}…", &s[..120])
    } else {
        s
    }
}
