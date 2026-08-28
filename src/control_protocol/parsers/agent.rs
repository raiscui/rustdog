//! `@agent-register` / `@agent-inbox` / `@agent-ack` 的 payload 解析
//! (agent messaging Phase 3, issue #71/#73)。

use std::io;

use super::{
    normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field,
    split_object_fields,
};
use crate::control_protocol::{AgentAckRequest, AgentNameRequest};
use crate::zenoh_identity::validate_daemon_name;

/// agent name 校验复用 daemon_name 规则 (spec: 单一校验真相源)。
pub(crate) fn parse_agent_name_payload(kind: &str, input: &str) -> io::Result<AgentNameRequest> {
    let trimmed = input.trim();
    let agent_name = if trimmed.starts_with('"') {
        parse_quoted_payload(trimmed)?
    } else {
        trimmed.to_owned()
    };
    validate_daemon_name(&agent_name).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "@{kind} {}",
                err.to_string().replace("zenoh.daemon_name", "agent name")
            ),
        )
    })?;
    Ok(AgentNameRequest { agent_name })
}

/// `@agent-ack:agent-name:msg-id` 短格式或 `{agent:"...",id:"..."}` 对象。
pub(crate) fn parse_agent_ack_payload(input: &str) -> io::Result<AgentAckRequest> {
    let trimmed = input.trim();
    // 冒号短格式: 两段都不含空格, agent name 与 uuid 均满足
    if !trimmed.starts_with('{') {
        let (agent_name, message_id) = trimmed.split_once(':').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("@agent-ack 短格式需要 agent-name:msg-id: {trimmed}"),
            )
        })?;
        validate_daemon_name(agent_name.trim())?;
        let message_id = message_id.trim();
        if message_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "@agent-ack msg-id 不能为空",
            ));
        }
        return Ok(AgentAckRequest {
            agent_name: agent_name.trim().to_owned(),
            message_id: message_id.to_owned(),
        });
    }
    // 对象格式: 手写字段循环 (与 pty parser 同款, 支持项目协议的无引号 key 风格)
    let inner = object_inner(trimmed, "@agent-ack")?;
    if inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@agent-ack 对象 payload 不能为空",
        ));
    }
    let mut agent_name = None::<String>;
    let mut message_id = None::<String>;
    for field in split_object_fields(inner)? {
        let (field_name, raw_value) = split_object_field(field)?;
        let field_name = normalize_object_field_name(field_name)?;
        let raw_value = raw_value.trim();
        match field_name.as_str() {
            "agent" => agent_name = Some(parse_quoted_payload(raw_value)?),
            "id" => message_id = Some(parse_quoted_payload(raw_value)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("@agent-ack 对象 payload 包含未知字段: {field_name}"),
                ));
            }
        }
    }
    let agent_name = agent_name.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "@agent-ack 对象缺少 agent 字段")
    })?;
    let message_id = message_id
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "@agent-ack 对象缺少 id 字段"))?;
    validate_daemon_name(&agent_name)?;
    if message_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "@agent-ack id 不能为空",
        ));
    }
    Ok(AgentAckRequest {
        agent_name,
        message_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_name_short_and_quoted_should_parse() {
        let request = parse_agent_name_payload("agent-inbox", "helper-a.lab").unwrap();
        assert_eq!(request.agent_name, "helper-a.lab");
        let request = parse_agent_name_payload("agent-inbox", "\"helper-a.lab\"").unwrap();
        assert_eq!(request.agent_name, "helper-a.lab");
    }

    #[test]
    fn invalid_agent_name_should_be_rejected() {
        assert!(parse_agent_name_payload("agent-register", "Bad Name").is_err());
        assert!(parse_agent_name_payload("agent-register", "").is_err());
    }

    #[test]
    fn ack_short_format_should_parse() {
        let request =
            parse_agent_ack_payload("helper-a.lab:11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(request.agent_name, "helper-a.lab");
        assert_eq!(request.message_id, "11111111-2222-3333-4444-555555555555");
    }

    #[test]
    fn ack_object_format_should_parse() {
        let request = parse_agent_ack_payload("{agent:\"a.lab\",id:\"u-1\"}").unwrap();
        assert_eq!(request.agent_name, "a.lab");
        assert_eq!(request.message_id, "u-1");
    }

    #[test]
    fn ack_malformed_should_be_rejected() {
        assert!(parse_agent_ack_payload("only-name").is_err());
        assert!(parse_agent_ack_payload("a.lab:").is_err());
        assert!(parse_agent_ack_payload("{agent:\"Bad Name\",id:\"u\"}").is_err());
    }
}
