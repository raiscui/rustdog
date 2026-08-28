# rdog 伴生 agent 与 agent 消息通道方案 (Phase 3)

> 正式 spec 同步自 issue #71 (ready-for-agent), 实施以 issue 为准, 本文件是长期文档镜像。

## Problem Statement

跨主机 agent 协作(用户在 A2A 调研后确立的方向)缺少传输基础: agent 之间没有任何消息通道, daemon 也没有托管长驻 agent 进程的入口。rdog 刻意保持零智能(daemon 是纯被控执行端), 但这意味着"每台主机上跑一个伴生 agent、agent 之间互相委派任务"的场景没有落地路径 — 现在唯一的协作形态是"单个编排 agent 遥控多台主机", 长任务无法增量汇报, daemon 无法主动推送。

(完整四阶段演进见 specs/rdog-task-spawn-control-plan.md; 本 spec 固化 Phase 3, 收拢 A2A 调研支线散落的设计。)

## Solution

daemon 提供**纯传输**的 agent 消息通道(per-agent inbox + 卡片托管 + mailbox 缓存), 智能永远在外挂的伴生 agent 进程里。新增 `rdog agent` 子命令作为伴生 agent 的宿主入口: 托管 daemon lifecycle + headless agent loop + 通道接线。agent loop 与 LLM provider 完全解耦(与评测 runner 的 provider 抽象同构)。

## User Stories

1. As an 编排 agent(主机 A), I want 把一个自然语言任务消息投递到主机 B 上某伴生 agent 的 inbox, so that 任务可以在 B 本地被消化执行而不需要我持续遥控 B 的每个动作。
2. As a 伴生 agent, I want 在自己的 inbox 收到新任务消息, so that 我能开始规划并驱动本机 daemon 执行。
3. As a 伴生 agent, I want 用本机 localhost control 调 rdog daemon 执行任务, so that 执行能力与现有 @spawn/@computer-act 等原语完全一致。
4. As a 伴生 agent, I want 把任务进度以 Task 进度帧(Phase 2)回报给委派方, so that 委派方无需轮询。
5. As a 伴生 agent, I want 在下线重启后补拉 inbox 里未 ack 的消息, so that agent 崩溃或换 model 不丢任务。
6. As a 编排 agent, I want 发现某 namespace 里有哪些在线伴生 agent 并读取各自的能力卡片, so that 我能按能力选择委派目标。
7. As a 伴生 agent, I want 在启动时注册/更新自己的能力卡片, so that 发现方看到的能力是新鲜的。
8. As a 人类操作者, I want 用单条命令 `rdog agent` 在一台主机上把伴生 agent 跑起来(自动带起或复用本地 daemon), so that 部署成本接近零。
9. As a 人类操作者, I want 查询伴生 agent 的在线状态与最近消息, so that 我能诊断"任务为什么没被执行"。
10. As a 编排 agent, I want 消息有唯一 id 和投递时间戳, so that 我能关联回复与请求并检测重复。
11. As a 伴生 agent, I want 对处理完成的消息显式 ack, so that mailbox 缓存能释放且重复投递可被识别。
12. As a 任何 client, I want 通过 queryable 拉取 agent 卡片, so that 晚加入的订阅者不错过卡片。
13. As a 人类操作者, I want agent 进程崩溃后 daemon 的 mailbox 不丢消息, so that 重启 agent 即可恢复。
14. As a 伴生 agent, I want 多条消息按序处理, so that 任务语义不因并发到达而错乱。

## Implementation Decisions

- **keyexpr 布局**(对齐 zenoh_identity 现有身份层级):
  - `rdog/<ns>/agent/<name>/inbox` — 消息投递(pub)+ 补拉(queryable)
  - `rdog/<ns>/agent/<name>/card` — 能力卡片托管(pub + queryable)
  - `rdog/<ns>/agent/<name>/alive` — liveliness 在线状态(对齐 daemon alive 模式)
- **agent name**: 复用 daemon_name 的校验规则(DNS 风格 label, 全小写), namespace 推断同款。
- **消息 envelope**: `rdog.agentmsg.v1` JSON — {id, from, to, kind, payload, sent_at}; kind 首版: task / reply / ack / control。
- **mailbox**: daemon 侧 per-agent 有界缓存(入站消息, 上限量级 256 条, 超限丢最老并计数); agent ack 后清除; registry 不持久化与 task registry 同款纪律。
- **agent loop 形态**: 收消息 → 决策回调(trait, provider 无关)→ 驱动本机 daemon → 回复/进度。决策回调是唯一智能注入点。
- **`rdog agent` CLI 契约**: --name(必填), --daemon-config(可选, 缺省复用本地默认 daemon, 没有则带起), agent 退出时消息保留在 mailbox。
- **安全边界**: inbox 是定向消息非广播(不同于被否决的旁观 topic); 卡片 pub 默认开启但内容是 agent 自己声明的摘要; fail-closed 收紧(认证)归 Phase 4 前置工作, 本 phase 不引入新的明文暴露面超过现有 daemon 控制面。
- **分工红线**: daemon 承担确定性部分(消息存储/路由/卡片托管), agent 承担智能部分(任务消化/规划/卡片内容)。

## Testing Decisions

- 测试只验证外部行为(CLI/协议层), 不测内部结构。
- **主 seam 复用现有 e2e 模式**(tests/zenoh_router_client 的子进程模式): 起 daemon 子进程 + rdog agent 子进程(决策回调用测试 build 的 echo/benchmark 实现, 不接 LLM)+ rdog control 子进程投递消息, 断言回复到达。
- agent loop 的决策回调 trait 用单测 mock(第二个 seam, 在 loop 边界)。
- mailbox: e2e 场景"先投递后起 agent"断言补拉成功。
- 先例: control_should_wait_for_slow_session_channel_response(子进程 e2e), task_spawn_progress_frames_should_reach_control_client(Phase 2 双侧断言模式)。

## Out of Scope

- A2A 语义层(AgentCard 标准 schema, 跨主机 Task 语义, A2A gateway)— Phase 4, 需本 phase 语义稳定后 to-spec。
- LLM provider 集成(agent loop 的决策回调实现是使用方的事)。
- 认证/加密(独立工作流, 优先级高于 Phase 4)。
- 旁观 status topic / agent 状态广播(曾在 A2A 调研讨论, 明确后置)。
- @flow async 升级(留在 task-spawn spec 的预留)。

## Further Notes

- 本 spec 收拢的调研来源: A2A 支线上下文(task_plan__a2a_research.md / notes__a2a_research.md, 2026-08-26~28 五轮讨论)。
- 曾否决的方案与理由记录在调研支线: daemon 内嵌 LLM(纪律/安全/评测灵活性), agent 直连不经 rdog(重造发现与 mailbox), 单一共享 topic 装所有进度帧(可靠性语义冲突+安全暴露面)。
- 测试接缝选择: 主 seam 复用 e2e 子进程模式(不新建), agent loop 内部用决策回调 trait 做 mock 边界 — 与 skill 指引的最高接缝原则一致。
