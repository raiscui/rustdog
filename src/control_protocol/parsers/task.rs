//! `@spawn` / `@task-status` / `@task-output` / `@task-cancel` 的 payload 解析。
//!
//! 语法契约 (specs/rdog-task-spawn-control-plan.md Phase 1):
//! - `@spawn:cargo build --release`    raw shell 文本 (对齐 `@cmd` 的 raw 先例)
//! - `@spawn "cargo build"`            quoted 形式
//! - `@spawn {command:"cargo build",cwd:"/tmp"}`  对象形式 (cwd 只在对象形式提供,
//!   raw 前缀 `cwd=...` 方案有解析歧义被否, 见 spec 决策记录)
//! - `@task-status:t-a1b2c3d4`         裸 task id token
//! - `@task-output:t-a1b2c3d4`         尾部 80 行 (默认)
//! - `@task-output {task:"t-x",lines:40}`  对象形式自定义行数
//! - `@task-cancel:t-a1b2c3d4`         裸 task id token
//!
//! LLM 兼容: parser 上层的"命令名 + 空格 + 参数"归一化让
//! `@spawn cargo build` 自动等价于 `@spawn:cargo build`。

use std::io;

use super::{
    normalize_object_field_name, object_inner, parse_quoted_payload, require_non_empty_payload,
    split_object_field, split_object_fields,
};
use crate::control_protocol::{SpawnRequest, TaskIdRequest, TaskOutputRequest};

/// task id 的合法形状: `t-` 前缀 + 至少 4 位十六进制。
/// 宽松校验 (不锁死长度), 让未来 id 生成策略变化不破坏 parser。
fn validate_task_id(kind: &str, task_id: &str) -> io::Result<String> {
    let task_id = task_id.trim();
    let malformed = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@{kind} task id 格式非法 (期望 t-<hex>): {task_id}"),
        )
    };
    let hex = task_id.strip_prefix("t-").ok_or_else(malformed)?;
    if hex.len() < 4 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(malformed());
    }
    Ok(task_id.to_owned())
}

pub(crate) fn parse_spawn_payload(raw_payload: &str) -> io::Result<SpawnRequest> {
    let trimmed = raw_payload.trim();

    // 对象形式: 唯一提供 cwd 的入口
    if trimmed.starts_with('{') {
        let inner = object_inner(trimmed, "@spawn")?;
        if inner.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@spawn 对象 payload 不能为空",
            ));
        }
        let mut command = None::<String>;
        let mut cwd = None::<String>;
        for field in split_object_fields(inner)? {
            let (field_name, raw_value) = split_object_field(field)?;
            let field_name = normalize_object_field_name(field_name)?;
            let raw_value = raw_value.trim();
            match field_name.as_str() {
                "command" => {
                    command = Some(parse_quoted_payload(raw_value)?);
                }
                "cwd" => {
                    cwd = Some(parse_quoted_payload(raw_value)?);
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@spawn 对象 payload 包含未知字段: {field_name}"),
                    ));
                }
            }
        }
        let command = command.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "@spawn 对象 payload 缺少 command 字段",
            )
        })?;
        return Ok(SpawnRequest {
            command: require_non_empty_payload("spawn", command, |value| value)?,
            cwd,
        });
    }

    // quoted 形式
    if trimmed.starts_with('"') {
        let command = parse_quoted_payload(trimmed)?;
        return Ok(SpawnRequest {
            command: require_non_empty_payload("spawn", command, |value| value)?,
            cwd: None,
        });
    }

    // raw shell 文本 (与 @cmd 同款): 非空、单行、不带对象头
    if trimmed.is_empty() || raw_payload.contains(['\r', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("@spawn payload 必须是非空、单行的 quoted 或 raw shell 文本: {trimmed}"),
        ));
    }

    Ok(SpawnRequest {
        command: trimmed.to_owned(),
        cwd: None,
    })
}

/// `@task-status` / `@task-cancel`: 裸 task id token 或 quoted。
pub(crate) fn parse_task_id_payload(kind: &str, input: &str) -> io::Result<TaskIdRequest> {
    let trimmed = input.trim();
    let task_id = if trimmed.starts_with('"') {
        parse_quoted_payload(trimmed)?
    } else {
        trimmed.to_owned()
    };
    let task_id = validate_task_id(kind, &task_id)?;
    Ok(TaskIdRequest { task_id })
}

/// `@task-output`: 裸 task id (默认 80 行) 或对象 `{task,lines}`。
pub(crate) fn parse_task_output_payload(input: &str) -> io::Result<TaskOutputRequest> {
    let trimmed = input.trim();

    if trimmed.starts_with('{') {
        let inner = object_inner(trimmed, "@task-output")?;
        if inner.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@task-output 对象 payload 不能为空",
            ));
        }
        let mut task_id = None::<String>;
        let mut lines = None::<usize>;
        for field in split_object_fields(inner)? {
            let (field_name, raw_value) = split_object_field(field)?;
            let field_name = normalize_object_field_name(field_name)?;
            let raw_value = raw_value.trim();
            match field_name.as_str() {
                "task" => {
                    task_id = Some(validate_task_id(
                        "task-output",
                        &parse_quoted_payload(raw_value)?,
                    )?);
                }
                "lines" => {
                    lines = Some(raw_value.parse::<usize>().map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("@task-output lines 必须是非负整数: {raw_value}"),
                        )
                    })?);
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("@task-output 对象 payload 包含未知字段: {field_name}"),
                    ));
                }
            }
        }
        let task_id = task_id.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "@task-output 对象 payload 缺少 task 字段",
            )
        })?;
        return Ok(TaskOutputRequest {
            task_id,
            lines: lines.unwrap_or(crate::task_control::DEFAULT_OUTPUT_TAIL_LINES),
        });
    }

    let request = parse_task_id_payload("task-output", trimmed)?;
    Ok(TaskOutputRequest {
        task_id: request.task_id,
        lines: crate::task_control::DEFAULT_OUTPUT_TAIL_LINES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_raw_command_should_parse() {
        let request = parse_spawn_payload("cargo build --release").unwrap();
        assert_eq!(
            request,
            SpawnRequest {
                command: "cargo build --release".to_owned(),
                cwd: None,
            }
        );
    }

    #[test]
    fn spawn_quoted_command_should_parse() {
        let request = parse_spawn_payload("\"echo 'hi there'\"").unwrap();
        assert_eq!(request.command, "echo 'hi there'");
    }

    #[test]
    fn spawn_object_should_parse_command_and_cwd() {
        let request = parse_spawn_payload("{command:\"cargo build\",cwd:\"/tmp/rdog\"}").unwrap();
        assert_eq!(request.command, "cargo build");
        assert_eq!(request.cwd.as_deref(), Some("/tmp/rdog"));
    }

    #[test]
    fn spawn_object_without_command_should_fail() {
        let err = parse_spawn_payload("{cwd:\"/tmp\"}").unwrap_err();
        assert!(err.to_string().contains("缺少 command"));
    }

    #[test]
    fn spawn_unknown_object_field_should_fail() {
        let err = parse_spawn_payload("{command:\"ls\",foo:\"bar\"}").unwrap_err();
        assert!(err.to_string().contains("未知字段"));
    }

    #[test]
    fn spawn_empty_payload_should_fail() {
        assert!(parse_spawn_payload("").is_err());
        assert!(parse_spawn_payload("   ").is_err());
    }

    #[test]
    fn task_id_token_should_parse() {
        let request = parse_task_id_payload("task-status", "t-a1b2c3d4").unwrap();
        assert_eq!(request.task_id, "t-a1b2c3d4");
    }

    #[test]
    fn task_id_quoted_should_parse() {
        let request = parse_task_id_payload("task-cancel", "\"t-a1b2c3d4\"").unwrap();
        assert_eq!(request.task_id, "t-a1b2c3d4");
    }

    #[test]
    fn malformed_task_id_should_fail() {
        assert!(parse_task_id_payload("task-status", "a1b2c3d4").is_err());
        assert!(parse_task_id_payload("task-status", "t-xyz").is_err());
        assert!(parse_task_id_payload("task-status", "t-").is_err());
    }

    #[test]
    fn task_output_short_form_should_default_lines() {
        let request = parse_task_output_payload("t-a1b2c3d4").unwrap();
        assert_eq!(request.task_id, "t-a1b2c3d4");
        assert_eq!(
            request.lines,
            crate::task_control::DEFAULT_OUTPUT_TAIL_LINES
        );
    }

    #[test]
    fn task_output_object_should_parse_lines() {
        let request = parse_task_output_payload("{task:\"t-a1b2c3d4\",lines:40}").unwrap();
        assert_eq!(request.task_id, "t-a1b2c3d4");
        assert_eq!(request.lines, 40);
    }
}
