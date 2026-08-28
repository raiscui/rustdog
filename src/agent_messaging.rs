//! agent 消息 envelope: `rdog.agentmsg.v1`
//!
//! 设计口径 (specs/rdog-agent-messaging-plan.md, issue #71):
//! - envelope 是 inbox keyexpr 上传输的消息载荷, 纯 JSON (对齐 rdog.flow.v1 的
//!   schema 风格, 不是 `@帧名` 前缀 — 那是 session channel 帧族的格式)
//! - 智能永远在 agent 侧: 本模块只做确定性的序列化/校验, 不解释 payload 语义
//! - from/to 是 agent name, 复用 daemon_name 校验规则 (DNS 风格 label)
//! - id 由发送方生成 (uuid v4), 消费方用它做去重与关联回复

use std::io;

use crate::zenoh_identity::validate_daemon_name;

/// envelope schema 版本, 写死在 wire 里供未来演进识别。
pub const AGENT_MESSAGE_SCHEMA_VERSION: u64 = 1;

/// 消息种类 (spec 首版四类)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageKind {
    /// 任务委派: 委派方发给执行方 agent 的任务描述。
    Task,
    /// 任务回复: 执行方对 task 的答复 (结果/部分结果)。
    Reply,
    /// 确认: mailbox 消费确认 (daemon 收到后清除缓存条目)。
    Ack,
    /// 控制消息: ping/能力查询等非任务交互。
    Control,
}

impl AgentMessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Reply => "reply",
            Self::Ack => "ack",
            Self::Control => "control",
        }
    }

    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "task" => Ok(Self::Task),
            "reply" => Ok(Self::Reply),
            "ack" => Ok(Self::Ack),
            "control" => Ok(Self::Control),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("agent message kind 非法: {other} (期望 task/reply/ack/control)"),
            )),
        }
    }
}

/// `rdog.agentmsg.v1` 消息 envelope。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessage {
    /// 消息唯一 id (uuid v4), 去重与关联回复的依据。
    pub id: String,
    /// 发送方 agent name。
    pub from: String,
    /// 接收方 agent name。
    pub to: String,
    pub kind: AgentMessageKind,
    /// 消息体 (任务描述/回复内容等), 语义由 kind 和 agent 侧约定。
    pub payload: String,
    /// 发送时刻 (unix epoch ms)。
    pub sent_at_ms: u64,
}

impl AgentMessage {
    /// 构造新消息: 自动生成 uuid id 和当前时间戳。
    pub fn new(from: &str, to: &str, kind: AgentMessageKind, payload: &str) -> io::Result<Self> {
        validate_agent_name("from", from)?;
        validate_agent_name("to", to)?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            payload: payload.to_owned(),
            sent_at_ms: now_ms(),
        })
    }

    /// 序列化为 wire JSON (inbox keyexpr 的 pub payload)。
    pub fn to_wire_message(&self) -> String {
        format!(
            "{{\"v\":{},{}}}",
            AGENT_MESSAGE_SCHEMA_VERSION,
            self.body_fields_json()
        )
    }

    /// 解析 wire JSON。v 字段必须匹配当前 schema 版本 (fail-closed, 不猜旧格式)。
    pub fn parse_wire_message(message: &str) -> io::Result<Self> {
        let value: serde_json::Value = serde_json::from_str(message.trim()).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("agent message 不是合法 JSON: {err}"),
            )
        })?;
        let object = value.as_object().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "agent message 必须是 JSON 对象")
        })?;

        let version = object
            .get("v")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "agent message 缺少 v 版本字段")
            })?;
        if version != AGENT_MESSAGE_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "agent message 版本不匹配: {version} (期望 {AGENT_MESSAGE_SCHEMA_VERSION})"
                ),
            ));
        }

        let require_string = |field: &str| -> io::Result<String> {
            object
                .get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("agent message 缺少 {field} 字段"),
                    )
                })
        };

        let id = require_string("id")?;
        if id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "agent message id 不能为空",
            ));
        }
        let from = require_string("from")?;
        let to = require_string("to")?;
        validate_agent_name("from", &from)?;
        validate_agent_name("to", &to)?;
        let kind = AgentMessageKind::parse(&require_string("kind")?)?;
        let payload = require_string("payload")?;
        let sent_at_ms = object
            .get("sent_at")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "agent message 缺少 sent_at 数字字段",
                )
            })?;

        Ok(Self {
            id,
            from,
            to,
            kind,
            payload,
            sent_at_ms,
        })
    }

    fn body_fields_json(&self) -> String {
        // payload 走 serde 转义, 避免 task 描述里的引号/换行破坏 JSON
        let payload = serde_json::to_string(&self.payload).unwrap_or_else(|_| "\"\"".to_owned());
        format!(
            "\"id\":\"{}\",\"from\":\"{}\",\"to\":\"{}\",\"kind\":\"{}\",\"payload\":{payload},\"sent_at\":{}",
            self.id,
            self.from,
            self.to,
            self.kind.as_str(),
            self.sent_at_ms,
        )
    }
}

/// agent name 校验: 直接复用 daemon_name 规则 (spec 决策, 单一校验真相源)。
fn validate_agent_name(field: &str, name: &str) -> io::Result<()> {
    validate_daemon_name(name).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "agent message {field} 非法: {}",
                err.to_string().replace("zenoh.daemon_name", "agent name")
            ),
        )
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_should_preserve_all_fields() {
        let message = AgentMessage::new(
            "orchestrator-a.lab",
            "helper-b.lab",
            AgentMessageKind::Task,
            "在 TextEdit 里打开 notes.txt 并统计行数",
        )
        .unwrap();
        let wire = message.to_wire_message();
        let parsed = AgentMessage::parse_wire_message(&wire).unwrap();
        assert_eq!(parsed, message);
    }

    #[test]
    fn payload_with_quotes_and_newlines_should_survive_round_trip() {
        let tricky = "line1 \"quoted\"\nline2 \\backslash\t tab";
        let message = AgentMessage::new("a.lab", "b.lab", AgentMessageKind::Reply, tricky).unwrap();
        let parsed = AgentMessage::parse_wire_message(&message.to_wire_message()).unwrap();
        assert_eq!(parsed.payload, tricky);
    }

    #[test]
    fn wire_format_should_carry_version_and_all_fields() {
        let message = AgentMessage::new("a.lab", "b.lab", AgentMessageKind::Ack, "ok").unwrap();
        let wire = message.to_wire_message();
        assert!(
            wire.starts_with("{\"v\":1,"),
            "wire 应以版本字段开头: {wire}"
        );
        assert!(wire.contains("\"kind\":\"ack\""), "{wire}");
        assert!(wire.contains("\"sent_at\":"), "{wire}");
    }

    #[test]
    fn mismatched_version_should_be_rejected() {
        let message = AgentMessage::new("a.lab", "b.lab", AgentMessageKind::Task, "x").unwrap();
        let stale = message.to_wire_message().replacen("\"v\":1", "\"v\":2", 1);
        let err = AgentMessage::parse_wire_message(&stale).unwrap_err();
        assert!(err.to_string().contains("版本不匹配"), "{err}");
    }

    #[test]
    fn malformed_names_should_be_rejected() {
        assert!(AgentMessage::new("Bad Name", "b.lab", AgentMessageKind::Task, "x").is_err());
        assert!(AgentMessage::new("a.lab", "", AgentMessageKind::Task, "x").is_err());
        let err = AgentMessage::new("Bad Name", "b.lab", AgentMessageKind::Task, "x").unwrap_err();
        assert!(err.to_string().contains("from 非法"), "{err}");
    }

    #[test]
    fn unknown_kind_should_be_rejected() {
        let err = AgentMessageKind::parse("chat").unwrap_err();
        assert!(err.to_string().contains("kind 非法"), "{err}");
    }

    #[test]
    fn missing_fields_should_be_rejected() {
        assert!(AgentMessage::parse_wire_message("{}").is_err());
        assert!(AgentMessage::parse_wire_message("not json").is_err());
        assert!(AgentMessage::parse_wire_message("[]").is_err());
        // 缺 payload
        let partial = r#"{"v":1,"id":"u","from":"a.lab","to":"b.lab","kind":"task","sent_at":1}"#;
        let err = AgentMessage::parse_wire_message(partial).unwrap_err();
        assert!(err.to_string().contains("缺少 payload"), "{err}");
    }
}
