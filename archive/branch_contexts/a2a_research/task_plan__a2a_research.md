# 任务计划: A2A 协议调研与 rustdog 跨主机 agent 通讯架构咨询

## [2026-08-26 18:24:18] [Session ID: current] 咨询任务: A2A 支持远程主机通讯吗, rustdog 如何做跨主机 a2a

### 目标

回答三个问题:
1. A2A 协议是否支持远程主机之间的通讯
2. rustdog 实现跨主机 agent-to-agent 通讯怎么做比较好
3. 要不要学习 A2A, 还是 rustdog 现有架构更好

### 阶段

- [x] 阶段1: 调研 rustdog 控制面现状(Explore agent, 引用代码证据)
- [x] 阶段2: 调研 A2A 2026 最新状态(v1.0, Linux Foundation, 三种 binding)
- [x] 阶段3: 对比分析与结论
- [x] 阶段4: 落盘与交付

### 核心发现

- A2A v1.0 (2026) 天然支持远程主机通讯, 这是它的主场: HTTP/gRPC binding,
  AgentCard well-known 发现, OAuth2/mTLS 企业安全, Linux Foundation 治理, 150+ 组织
- A2A 是"异构 agent 框架互操作"的语义层协议(能力发现/任务委派/生命周期/产物)
- rustdog 是"主机控制"执行层: Zenoh router/client, session channel 已落地,
  多 transport(TCP/UDP/unixpipe/serial), LAN liveliness 发现
- rustdog 三大真实缺口(与 A2A 语义正好对应):
  (a) 双向对等发起未完成(ControlFrame 无 Request 变体, daemon 只能被动应答)
  (b) 无通用任务生命周期(request-id 逐命令 + PTY 专用 session, @flow 是阻塞 RPC)
  (c) 跨主机零认证加密(明文 Zenoh, 信任=网络可达性)

### 做出的决定(咨询结论)

- 结论: 学 A2A 的语义模型, 不照搬它的传输绑定。分层设计:
  执行层保持 Zenoh control plane, 语义层借鉴 AgentCard/Task/Artifact,
  对外互操作可选加 A2A gateway(等内部 task 语义稳定后)
- [理由]: A2A 传输(HTTP/DNS/PKI)对 LAN 低延迟 GUI 控制、serial、unixpipe 场景是负担;
  Zenoh 多 transport + 组播发现是已验证优势。但 A2A 的任务语义和 AgentCard
  正是 rustdog 双向控制面 Phase 3 需要的抽象参考

### 曾考虑的替代方案(细节在 notes__a2a_research.md)

- 全面转向 A2A (HTTP 为主传输): 放弃 Zenoh 优势, serial/unixpipe/组播发现全丢, 不采用
- 不引入任何 agent 语义, 只让外挂编排 agent 跨 target 编排: 现状已支持
  (code-agent-rdog-control-usage.md 模式), 但 daemon 主动推送/长任务协作缺失, 天花板低

### 状态

**已完成, 结论已在会话中交付**

## [2026-08-27 10:30:00] [Session ID: current] 追问: 用 channel 分享卡片与 Task 进度是否合适

### 问题

用户希望有一个 channel 分享 "AgentCard式能力卡片 → Task状态机+进度帧", 是否合适。

### 结论

- 合适, 且这正是 Zenoh 相对 A2A/HTTP 的结构性优势(liveliness/pub/sub 原生)。
- 但 "一个 channel" 要精确化为 "一套 channel 体系, 两种分发语义":
  - 卡片 = 发现层 channel: alive token 携带卡片版本 + rdog/ns/daemon/name/card 完整卡片
  - Task 进度 = 按消费者分两路:
    - 发起方: 现有 session channel to-control, Task 帧族(仿 PTY 帧族模式), 有序不丢
    - 旁观者: namespace topic rdog/ns/task/id/status, opt-in, 只发摘要不发原始输出
    - 兜底: queryable task-get, 重连/晚加入补拉终态
- 真相源纪律: daemon Task Registry 是唯一真相源, channel 只分发视图

### 否掉的简化方案

- 单一共享 topic 装下所有进度帧: 可靠性语义冲突(发起方不能丢终止帧)、
  安全边界冲突(stdout 等敏感输出不应对 namespace 全员可见)、
  违反现有 session channel 已有的 frame ordering + request-id 关联复用

## [2026-08-27 11:20:00] [Session ID: current] 追问: rdog 无 agent 能力, 需外挂 agent 调用, 现状与做法

### 现状确认(证据)

- 调用路径 = bash/CLI: agent 通过 `rdog control` 每次调用, SKILL.md 明确
  "Every bash call must begin with rdog control", agent-agnostic 设计
- LLM 适配在协议层不在 runtime 层: skill 文档(Tight Loop 纪律) +
  48KB 响应预算 + compact 短格式 + parser 容错
- 评测已验证外挂模式: runner/ 起 managed daemon + 外部 LLM(Pi),
  5 model x 8 case, agent 是外部进程, daemon 是被驱动方
- daemon 零智能: 只知协议能力(@capabilities), 不知"能干什么智能任务"

### 对前两轮结论的精确化

- AgentCard / Task 语义主体不是 daemon: 卡片内容必须 agent 侧生成,
  任务消化(自然语言→命令序列)也是 agent 侧的事
- 正确分工: daemon 承担确定性部分(Task 状态机/seq/进度帧路由/消息通道),
  agent 承担智能部分(任务消化/规划/卡片内容) — 状态机是确定性逻辑, 属基础设施

### 推荐做法: 伴生 agent 模式

- 每台主机: 外挂 agent 进程 + rdog daemon 并存, agent localhost 驱动本机 daemon
- daemon 新增 agent 消息通道(纯传输), A2A 语义消息在 Zenoh 上跑
- agent-side runtime 打包(起 daemon + agent loop + 通道接线),
  复用 runner/daemon_manager.py 验证过的 lifecycle 模式
- 前两轮 channel 设计全部保持有效, 仅收发端点从 daemon 改为伴生 agent

### 否掉的方案

- daemon 内嵌 LLM runtime: 违背最简单实现纪律; LLM 依赖拖垮 daemon 轻量性;
  把"手"和"脑"锁死, 换 model 评测要动 daemon; runner 已验证外挂更优
- agent 之间直连不经 rdog: 重新发明发现/通道(Zenoh 已有);
  agent 可下线而主机在线, 需要 daemon 提供 mailbox 语义

## [2026-08-27 14:00:00] [Session ID: current] 追问: herdr 用于进程管理/任务分配, 用还是集成进 rdog

### herdr 事实 (README + herdr.dev/docs/agent-automation 0.8.2 确认)

- 定位: "tmux 之于编程 agent", Rust, Apache 2.0, 33k stars, v0.4-0.8.x 未稳定
- 架构: 单 server 多客户端 attach, SSH 桥接是人类接入方式, 非原生分布式
- 三原语: Layout(workspace/tab/pane) / Pane(run/send-text/wait-output/read) /
  Agent(start/prompt/send-keys/wait/read), 20 种 agent CLI 检测
- 状态: 读 pane 内容启发式分类 working/blocked/idle/done/unknown;
  blocked=识别到审批/提问 UI; unknown 不代表失败
- 明确无任务分配/调度/队列: 纯协调原语, "agent 为 agent 创造工作"要编排方自己拼
- 备用屏分页读取: agent idle 时用鼠标滚动接口收集历史 — 实用脏活

### 与 rdog 的职责矩阵

- rdog 管"身体"(GUI/键盘/截图/跨主机), herdr 管"住所"(agent 进程的终端+状态感知)
- 重叠区: pane run/wait-output/read ≈ rdog PTY + @flow Expect; herdr 多出
  agent 状态启发式+备用屏读取, rdog 多出跨主机+GUI+daemon 控制面

### 结论

- 用 (选项A): 成立, 但价值取决于伴生 agent 形态:
  - 交互式 coding agent CLI 当伴生 → herdr 价值大 (pane 持久/blocked 检测/审批响应)
  - 自研 headless agent loop → herdr 价值小 (不需要终端, blocked 用 Task 状态机表达更可靠)
- 集成进 rdog (选项B): 否。功能重叠(PTY)+拆解成本高(完整产品, API 未稳定)+
  daemon 高权限进程职责膨胀+违背"改良胜过新增"纪律
- 任务分配: herdr 和 rdog 都不做也不该做, 分配智能在 orchestrator agent /
  A2A 语义层(Task 委派), herdr 只是被驱动的一个执行住所

### 推荐路径

1. 短期: 部署 herdr 试运行, 每台主机 herdr(跑 coding agent)+rdog daemon 并存,
   agent localhost 调 rdog control, 零代码验证多 agent 协作需求
2. 中期: 按伴生 agent 形态分化, 交互式协作需求成立则 herdr 常驻;
   收敛到 headless 则 herdr 退出, rdog agent loop + Task 状态机接管
3. 只借思想不搬代码: blocked 启发式, 备用屏分页读取

### 曾考虑的替代方案

- rdog 自己补进程管理 (上轮 rdog agent 子命令): 适用于 headless 形态,
  但交互式 agent 场景要重新发明 pane 持久化/状态分类/备用屏读取, 不如先用现成的
- fork herdr 融合: Apache 2.0 法律上可行, 但 v0.8 API 未稳定, 上游快速迭代,
  fork 即掉队, 否

## [2026-08-27 15:30:00] [Session ID: current] 追问: rdog 有 agent 命令吗, daemon 是否只能阻塞跑

### 事实核 (代码证据)

- `rdog agent` 命令不存在: CLI 只有 daemon/control/config/record, 上轮是建议不是现状
- daemon 并发模型三条 lane:
  1. 主 queryable (zenoh_control.rs:212): 串行 loop, recv 一条同步处理完才收下一条
  2. legacy queryable (zenoh_control.rs:195): 独立线程, 同样串行, 与主线程并行
  3. session bridge (daemon_bridge.rs:52): 每个 control 连接一个 thread::spawn
- 同 session 内串行: daemon_bridge.rs:313 parse_and_execute_control_line 是同步函数,
  长命令(@cmd sleep 300 / @flow)阻塞该 session 的后续帧
- CancelRegistry 共享注释(310-312)证明 in-flight 阻塞真实存在, cancel 从别的 lane 进
- PTY attach 期间 session 变专线: 输入直接转发 PTY stdin(239-242), 不走命令解析
- PTY 是唯一"后台长进程"机制: 进程独立跑, 流式转发, detach 继续活

### 用户担忧的精确判定

- 成立: 同一 control 连接内串行, 长命令阻塞同 session 后续命令;
  无 spawn-and-forget 任务原语(@cmd/@flow 都是同步等终态)
- 不成立: daemon 不会全局死锁 — 跨 session 天然并发(每连接一线程),
  PTY 期间开新 control 连接即可继续操作

### 真缺口与依赖链

- 缺后台任务原语: spawn → 立即返回 task id → status/output/cancel 查询
- 这正是 Task registry(A2A 语义层确定性部分)的第一个用例, 两者合流
- 伴生 agent 是长驻进程, 现在唯一承载方式是 PTY(终端语义错位),
  所以 spawn 原语是伴生 agent 模式的前置条件
- herdr 的异步模型(pane run 提交即返回 + wait-output 单独等)正是 rdog 缺的原语形态,
  上轮"herdr 只服务交互式 agent"结论补充: 它也示范了任务原语的正确形态

### 建议演进顺序

1. @spawn/@task-status/@task-output/@task-cancel 原语(泛化 PTY 进程管理内核)
2. Task registry 状态机(确定性) — 与 A2A 语义层设计合流
3. 伴生 agent 托管(长进程的第一个用户)

## [2026-08-28 09:40:00] [Session ID: current] 演进正式化: rdog-task-spawn-control-plan.md 落盘

### 完成内容

- [x] 正式 spec 落盘: specs/rdog-task-spawn-control-plan.md
  - Phase 1 @spawn 四原语(协议/帧流/语义边界/状态机/验收矩阵)
  - Phase 2 Task registry + 进度帧(走 session channel, 旁观 topic 后置)
  - Phase 3 伴生 agent 托管(rdog agent, mailbox 语义)
  - Phase 4 A2A 语义层索引(学语义不换传输, 认证层优先)
  - 决策记录 5 条, 每条含替代方案否决理由
- [x] 三张 mermaid 图(演进/时序/状态机)经 beautiful-mermaid-rs 验证
- [x] AGENTS.md 长期文件索引登记

### 状态

**咨询支线收尾: 调研 -> 讨论 -> 并发模型验证 -> 演进方案正式化, 全链路完成。
后续实施(Phase 1 ticket 化)是独立新任务, 待主线 ax-split 完成后启动。**
