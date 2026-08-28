//! 伴生 agent runtime (`rdog agent` 子命令, issue #74)。
//!
//! 设计口径 (specs/rdog-agent-messaging-plan.md):
//! - 决策回调 trait 是唯一智能注入点: rdog 不接 LLM, 测试用 echo 决策,
//!   真实 provider 由使用方在 trait 上实现
//! - agent 与 daemon 的 control 交互走 legacy query (即答命令, 不需要
//!   session channel): @agent-register / @agent-inbox / @agent-ack
//! - alive liveliness token 对齐 daemon alive 模式; agent 退出不清理 mailbox
//!   (消息保留, 重启后补拉恢复 — spec 的 mailbox 语义)
//!
//! Phase 3 实现偏差 (记录于 issue #74): "没有 daemon 则带起" 简化为
//! "连接现有 daemon, 缺失时报错" — 内嵌 spawn daemon 涉及配置生成与
//! lifecycle 管理, 留待真实使用反馈后再决定是否内嵌。

use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use zenoh::Wait;

use crate::agent_messaging::{mailbox_ack, AgentMessage, AgentMessageKind};
use crate::zenoh_identity::{build_agent_alive_key, build_agent_inbox_key, build_control_key};

/// 决策回调: 唯一智能注入点 (provider 无关)。
pub trait AgentDecision: Send + Sync {
    /// 处理一条 task 消息, 返回回复 payload 文本。
    fn decide(&self, message: &AgentMessage) -> String;

    /// 本 agent 的能力卡片 JSON (issue #75): 内容由决策侧生成,
    /// rdog 只托管分发。version 由 run_agent 包装递增。默认空能力卡片。
    fn card_json(&self) -> String {
        r#"{"capabilities":[]}"#.to_owned()
    }
}

/// 内置 echo 决策: 测试 / 冒烟 / 协议联调用, 不接任何 LLM。
pub struct EchoDecision;

impl AgentDecision for EchoDecision {
    fn decide(&self, message: &AgentMessage) -> String {
        format!("echo:{}", message.payload)
    }

    fn card_json(&self) -> String {
        // 内置 echo 卡片: 描述能力 + version 起始为 1 (由 run_agent 包装递增)
        r#"{"decision":"echo","capabilities":["task-reply"],"description":"回显任务 payload 的测试/联调 agent"}"#.to_owned()
    }
}

/// 单条消息的处理结果 (纯函数, 单测锚点)。
pub struct AgentReply {
    /// 回复给委派方的 envelope。
    pub message: AgentMessage,
    /// 本机 mailbox 的 ack 目标 id。
    pub ack_id: String,
}

/// 处理一条 task 消息: 决策 -> 构造回复 envelope (reply kind, to=from)。
///
/// 非 task 消息 (control 等) 返回 None: 首版只自动处理任务委派,
/// control 类交互留给未来的 agent 间协商协议。
pub fn handle_message(
    self_name: &str,
    message: &AgentMessage,
    decision: &dyn AgentDecision,
) -> Option<AgentReply> {
    if message.kind != AgentMessageKind::Task {
        return None;
    }
    let reply_payload = decision.decide(message);
    let reply = AgentMessage::new(
        self_name,
        &message.from,
        AgentMessageKind::Reply,
        &reply_payload,
    )
    .ok()?;
    Some(AgentReply {
        message: reply,
        ack_id: message.id.clone(),
    })
}

/// agent 的轻量 control client: legacy query 一问一答 (即答命令专用)。
fn control_query(
    session: &zenoh::Session,
    control_key: &str,
    line: &str,
    timeout: Duration,
) -> io::Result<String> {
    let replies = session
        .get(control_key)
        .payload(line.to_owned())
        .timeout(timeout)
        .wait()
        .map_err(|err| io::Error::other(format!("agent control query 失败: {err}")))?;
    let reply = replies
        .recv()
        .map_err(|_| io::Error::other("agent control query 无响应"))?;
    let sample = reply
        .result()
        .map_err(|err| io::Error::other(format!("agent control query 被拒: {err}")))?;
    Ok(sample
        .payload()
        .try_to_string()
        .map_err(|err| io::Error::other(format!("agent control query 响应解码失败: {err}")))?
        .into_owned())
}

/// agent 运行配置。
pub struct AgentRuntimeConfig {
    pub namespace: String,
    pub agent_name: String,
    /// daemon 的 control key (legacy query 目标)。
    pub daemon_control_key: String,
    pub decision: Arc<dyn AgentDecision>,
    /// 补拉轮询间隔 (测试可调小)。
    pub poll_interval: Duration,
}

/// agent 主循环: 注册 -> sub inbox -> (实时 + 补拉) -> 决策 -> 回复 -> ack。
///
/// 退出条件: control query 失联 (daemon 不可达) 或 inbox subscriber 关闭。
/// 本地 seen 去重: sub 直收与补拉可能重复投递同一条, ack 后 mailbox 不再给,
/// 但 sub 侧的重复由本地窗口兜底。
pub fn run_agent(session: &zenoh::Session, config: &AgentRuntimeConfig) -> io::Result<()> {
    // 1. alive token: 在线状态声明 (agent 退出自动撤销, 对齐 daemon alive)
    let alive_key = build_agent_alive_key(&config.namespace, &config.agent_name);
    let _alive_token = session
        .liveliness()
        .declare_token(&alive_key)
        .wait()
        .map_err(|err| io::Error::other(format!("agent alive token 声明失败: {err}")))?;

    // 1.5 能力卡片发布 (issue #75): daemon 通配 sub 缓存最新版,
    // @agent-card 查询。version 由 agent 侧递增 (Phase 3: 启动时 v1)。
    {
        let card_key =
            crate::zenoh_identity::build_agent_card_key(&config.namespace, &config.agent_name);
        let card_body = format!(
            "{{\"agent\":\"{}\",\"version\":1,\"card\":{}}}",
            config.agent_name,
            config.decision.card_json()
        );
        let publisher = session
            .declare_publisher(card_key)
            .wait()
            .map_err(|err| io::Error::other(format!("card publisher 声明失败: {err}")))?;
        publisher
            .put(card_body.as_str())
            .wait()
            .map_err(|err| io::Error::other(format!("card 发布失败: {err}")))?;
    }

    // 2. mailbox 注册 (daemon 侧开始缓存投给本 agent 的消息)
    let register_line = format!("@agent-register:{}", config.agent_name);
    let response = control_query(
        session,
        &config.daemon_control_key,
        &register_line,
        Duration::from_secs(5),
    )?;
    if !response.contains("\"registered\":true") {
        return Err(io::Error::other(format!(
            "agent mailbox 注册失败: {response}"
        )));
    }

    // 3. sub 自己的 inbox (实时路径; 补拉是兜底路径)
    let inbox_key = build_agent_inbox_key(&config.namespace, &config.agent_name);
    let subscriber = session
        .declare_subscriber(&inbox_key)
        .wait()
        .map_err(|err| io::Error::other(format!("agent inbox sub 声明失败: {err}")))?;

    // 本地去重窗口: 处理过的消息 id (daemon mailbox 的 seen 是跨重启窗口,
    // agent 本地的只需覆盖 sub 与补拉的竞态)
    let mut handled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_poll = Instant::now() - config.poll_interval;

    loop {
        // 实时路径: 50ms 有界等待一条消息 (兼作 loop 节拍, 不空转 CPU)。
        // recv_timeout(0) 的语义在部分 backend 上是"立即超时不检查", 不可靠。
        if let Ok(Some(sample)) = subscriber.recv_timeout(Duration::from_millis(50)) {
            if let Ok(payload) = sample.payload().try_to_string() {
                if let Ok(message) = AgentMessage::parse_wire_message(&payload) {
                    process_one(session, config, &mut handled, &message)?;
                }
            }
        }

        // 兜底路径: 周期补拉 daemon mailbox (sub 断续期间错过的消息)
        if last_poll.elapsed() >= config.poll_interval {
            last_poll = Instant::now();
            let inbox_line = format!("@agent-inbox:{}", config.agent_name);
            let response = control_query(
                session,
                &config.daemon_control_key,
                &inbox_line,
                Duration::from_secs(5),
            )?;
            for message in parse_pending_messages(&response) {
                process_one(session, config, &mut handled, &message)?;
            }
        }
    }
}

/// 处理一条消息: 去重 -> 决策 -> 回复 pub -> ack。
fn process_one(
    session: &zenoh::Session,
    config: &AgentRuntimeConfig,
    handled: &mut std::collections::HashSet<String>,
    message: &AgentMessage,
) -> io::Result<()> {
    if !handled.insert(message.id.clone()) {
        return Ok(());
    }
    if let Some(reply) = handle_message(&config.agent_name, message, config.decision.as_ref()) {
        // 回复 pub 到委派方的 inbox: envelope 的 to 才是委派方
        // (from 是本 agent 自己 — 曾误用 from 导致回复发回自己)
        let reply_inbox = build_agent_inbox_key(&config.namespace, &reply.message.to);
        let publisher = session
            .declare_publisher(reply_inbox)
            .wait()
            .map_err(|err| io::Error::other(format!("reply publisher 声明失败: {err}")))?;
        publisher
            .put(reply.message.to_wire_message())
            .wait()
            .map_err(|err| io::Error::other(format!("reply 发送失败: {err}")))?;
        // ack 本机 mailbox (消息不在 pending 也幂等)
        let ack_line = format!("@agent-ack:{}:{}", config.agent_name, reply.ack_id);
        let _ = control_query(
            session,
            &config.daemon_control_key,
            &ack_line,
            Duration::from_secs(5),
        );
        log::info!(
            "agent handled task: agent={}, message_id={}, replied_to={}",
            config.agent_name,
            reply.ack_id,
            reply.message.to
        );
    }
    Ok(())
}

/// 从 @agent-inbox 的响应 JSON 里解析 pending envelope 列表。
///
/// 响应形状: {"agent":"..","registered":true,"pending":[{envelope},..],..}
/// envelope 本身是合法 JSON, 逐段提取后走标准 parse。
fn parse_pending_messages(inbox_response: &str) -> Vec<AgentMessage> {
    // control 响应是完整 `@response {json}` 行 (legacy query 原样返回),
    // 先剥前缀再解析 — 漏剥会让 serde 静默失败, 补拉路径空转
    let json_text = inbox_response
        .trim()
        .strip_prefix("@response ")
        .unwrap_or(inbox_response.trim());
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return Vec::new();
    };
    let Some(pending) = value.get("pending").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    pending
        .iter()
        .filter_map(|entry| {
            serde_json::to_string(entry)
                .ok()
                .and_then(|wire| AgentMessage::parse_wire_message(&wire).ok())
        })
        .collect()
}

/// 解析 agent CLI 参数为 runtime 配置 (连接参数 -> control key)。
pub fn resolve_agent_config(
    namespace: &str,
    agent_name: &str,
    daemon_name: &str,
    decision: Arc<dyn AgentDecision>,
) -> AgentRuntimeConfig {
    AgentRuntimeConfig {
        namespace: namespace.to_owned(),
        agent_name: agent_name.to_owned(),
        daemon_control_key: build_control_key(namespace, daemon_name),
        decision,
        poll_interval: Duration::from_secs(2),
    }
}

// mailbox_ack 被 process_one 的 control 路径替代, 但保留直接调用入口给测试。
#[allow(dead_code)]
fn _unused_mailbox_ack_guard(agent: &str, id: &str) -> bool {
    mailbox_ack(agent, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UpperDecision;
    impl AgentDecision for UpperDecision {
        fn decide(&self, message: &AgentMessage) -> String {
            message.payload.to_uppercase()
        }
    }

    fn sample_task(payload: &str) -> AgentMessage {
        AgentMessage {
            id: "msg-1".to_owned(),
            from: "orch.lab".to_owned(),
            to: "helper.lab".to_owned(),
            kind: AgentMessageKind::Task,
            payload: payload.to_owned(),
            sent_at_ms: 1,
        }
    }

    #[test]
    fn handle_message_should_reply_to_sender_with_decision_output() {
        let reply = handle_message("helper.lab", &sample_task("算 1+1"), &UpperDecision)
            .expect("task should be handled");
        assert_eq!(reply.message.kind, AgentMessageKind::Reply);
        assert_eq!(reply.message.to, "orch.lab");
        assert_eq!(reply.message.from, "helper.lab");
        assert_eq!(reply.message.payload, "算 1+1".to_uppercase());
        assert_eq!(reply.ack_id, "msg-1");
    }

    #[test]
    fn echo_decision_should_echo_payload() {
        let reply = handle_message("h.lab", &sample_task("ping"), &EchoDecision).unwrap();
        assert_eq!(reply.message.payload, "echo:ping");
    }

    #[test]
    fn non_task_message_should_not_be_handled() {
        let control = AgentMessage {
            kind: AgentMessageKind::Control,
            ..sample_task("x")
        };
        assert!(handle_message("h.lab", &control, &EchoDecision).is_none());
    }

    #[test]
    fn echo_decision_card_should_be_valid_json_with_capabilities() {
        let card = EchoDecision.card_json();
        let value: serde_json::Value = serde_json::from_str(&card).unwrap();
        assert_eq!(value["decision"], "echo");
        assert!(value["capabilities"].is_array());
    }

    #[test]
    fn parse_pending_messages_should_extract_envelopes_from_inbox_response() {
        let task = AgentMessage::new("a.lab", "b.lab", AgentMessageKind::Task, "干活").unwrap();
        let envelope = task.to_wire_message();
        let response = format!(
            "{{\"agent\":\"b.lab\",\"registered\":true,\"pending\":[{envelope}],\"dropped\":0,\"duplicate\":0}}"
        );
        let parsed = parse_pending_messages(&response);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, task.id);
        assert_eq!(parsed[0].payload, "干活");
    }

    #[test]
    fn parse_pending_messages_should_tolerate_malformed_response() {
        assert!(parse_pending_messages("not json").is_empty());
        assert!(parse_pending_messages("{\"registered\":false}").is_empty());
    }
}
