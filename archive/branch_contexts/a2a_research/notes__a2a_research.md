## [2026-08-26 18:24:18] [Session ID: current] 笔记: A2A 协议 vs rustdog Zenoh 控制面

## 来源

### 来源1: A2A 官方与新闻(2026-08 检索)

- A2A v1.0 已发布: 首个稳定 spec, Apache 2.0, 官方 SDK 覆盖 Python/JS/Java/C#/Go
- Linux Foundation 治理, 2026-04 官宣 150+ 组织采用, 主要云平台落地, 有企业生产使用
- 三种标准 binding: JSON-RPC 2.0 over HTTP(最常见), gRPC, HTTP+JSON/REST
- a2a.proto 成为 canonical 定义源; AgentCard 的 `transport` 字段改名为 `protocolBinding`
- spec 明确允许 custom protocol bindings(自定义传输绑定的扩展点)
- 核心语义对象: AgentCard(发现/能力), Task(submitted/working/input-required/
  completed/failed/canceled 状态机), Message(role), Part(Text/File/Data), Artifact
- 安全依赖标准 Web 安全: HTTPS + OAuth 2.0 / API key / mTLS
- 与 MCP 关系: MCP 是 agent-to-tools, A2A 是 agent-to-agent, 互补不竞争

### 来源2: rustdog 控制面现状(本仓库代码证据)

- src/control_frames.rs: ControlFrame 枚举只有结果/生命周期帧
  (ResponseLine/SaveFile/PtyReady/PtyOutput/PtyExit/PtyClosed/PtyDetached/PtyAttached),
  规划中的 Request(ControlRequestFrame) 主动指令帧不存在
- src/control_session.rs: ControlPeerSession 是薄 session core(frame ordering,
  request-id 关联, PTY lifecycle gate), TCP/WS receiver 已走 dispatch
- Zenoh session channel 已落地: rdog/<ns>/session/<id>/to-daemon 与 to-control 双 key
  (src/zenoh_identity.rs), client ZenohClientSessionBridge + daemon open_daemon_session_bridge
- daemon 只能被动应答, 无本地主动发起入口; client execute_remote_request 阻塞等 final
  @response(60s 超时) — request/response 模型
- 拓扑: daemon=内嵌 router(mode="router"), control=client; 发现顺序: 显式 endpoints ->
  unixpipe FIFO -> UDP multicast scout; transport: tcp/udp/unixpipe/serial (zenoh 1.8.0)
- 跨主机寻址: target-name 即 daemon_name(DNS 风格), liveliness token 唯一性 guard
- 无通用 task 抽象: @cmd#id 逐命令 + CancelRegistry 逐命令取消; PTY 有完整 uuid session
  生命周期帧族; @flow 是单次阻塞 RPC, 执行中无增量进度帧
- daemon 无 agent/LLM runtime(纯被控执行端); 评测 harness 在外部 runner/
- 安全: Cargo.toml zenoh features 无 auth/TLS 插件, 明文网络, 连上即可下发任意 shell

## 综合发现

### A2A 与 rustdog 是两个层次

- A2A: 语义层(互操作协议) — 能力发现, 任务委派, 生命周期, 产物交付, 面向互联网/跨组织
- rustdog: 执行层(控制面) — 低延迟 GUI/shell/PTY 主机控制, 面向 LAN/边缘/本机
- 两者不是替代关系; A2A 的 custom binding 扩展点理论上也允许非 HTTP 绑定

### rustdog 离跨主机 a2a 的真实距离

- 已具备: 传输/会话通道雏形, request-id, PTY 生命周期帧族, router/client 拓扑, LAN 寻址
- 缺: 对等发起(双向 Phase 3), 通用任务生命周期, agent 语义(能力声明), 认证加密

### 值得借鉴的 A2A 设计

- AgentCard 式声明式能力发现(rdog 已有 @capabilities + liveliness, 缺标准化的能力卡片)
- Task 状态机 + 长任务流式进度 + Artifact 产物模型(@savefile 已接近, 可统一语义)
- "每个 agent 既是 client 又是 server" 的对称角色观 — 正对应双向控制面 Phase 3

### 不值得照搬的部分

- HTTP/JSON-RPC 作为主传输(LAN 低延迟/serial/unixpipe 场景的负担)
- well-known URI + DNS/Web PKI 发现体系(LAN liveliness 更合适)
- OAuth 等企业安全套件(rdog 需要的是轻量网络内信任层, 而非 Web PKI)

## [2026-08-27 10:30:00] [Session ID: current] 笔记: channel 分享卡片与 Task 进度的设计分析

### 现有 keyexpr 权威布局 (src/zenoh_identity.rs 佐证)

- rdog/<ns>/daemon/<name>/member/<name>/alive     (liveliness token)
- rdog/<ns>/daemon/<name>/member/<name>/control    (control queryable)
- rdog/<ns>/daemon/<name>/member/<name>/keyinput
- rdog/<ns>/session/<session_id>/to-daemon|to-control (session channel)
- 身份层级: daemon_name == member_id, namespace 从名字末 label 推断

### 设计结论

1. AgentCard 用 channel 分发比 A2A 的 well-known URI pull 更优:
   - Zenoh liveliness token 有持久性: 晚加入者查询立即得到当前存活 token, 解决"错过广播"
   - token payload 适合小数据: alive token 带卡片 version/哈希, 完整卡片放 /card keyexpr
   - 卡片变化 → version 递增 → 重新 pub, 订阅者可对比版本感知能力漂移
2. Task 进度必须区分消费者:
   - 发起方要求有序/不丢/id 关联/终止语义 → session channel 是现成答案(PTY 帧族已验证)
   - 旁观者容忍丢帧只要最新状态 → namespace topic + best-effort
   - Zenoh Reliable/BestEffort 是 per-subscriber 的, 但不该为省一个 keyexpr
     把权威流降级到共享 topic
3. 安全边界(fail-closed):
   - 旁观 topic 默认关闭, opt-in 开启
   - 旁观 topic 只发状态机摘要(state+seq+简述), 不发 stdout/AX dump 等原始输出
   - 认证层落地前, namespace topic 是新增暴露面, 必须最小化
4. 与 A2A 对照:
   - A2A: 卡片 pull(HTTP GET) + 任务进度 per-task SSE stream + webhook push
   - rdog/Zenoh: 卡片 push+持久 token + 任务进度 session unicast + 旁观 topic + queryable 补拉
   - 语义对齐, 传输形态各取所长

## [2026-08-27 14:00:00] [Session ID: current] 笔记: herdr 调研细节

### agent-automation 0.8.2 文档要点

- pane id 形如 w1:p2; agent 名 [a-z][a-z0-9_-]{0,31} 存活期唯一, 退出即清除
- agent start 需已存在 shell pane, 不自行分割布局; 检测 blocked 时返回
  agent_not_ready, idle 后可 prompt
- agent prompt 遵循 bracketed-paste; 对 blocked agent 返回 agent_blocked 且
  不发输入, 需 send-keys 响应对话框(esc/up/enter/ctrl+c)
- --until idle/done/blocked 可组合; done=后台工作结束直到 tab 聚焦
- 读取: recent/recent-unwrapped(默认 80 行, UTF-8 剥 ANSI)/visible/detection;
  备用屏历史: idle 且在转录底部才走滚动分页, 否则 agent_not_idle
- wait-output 轮询快照, 已存在文本可命中, Rust 正则逐行
- 无默认超时可无限等待; 错误 stderr JSON + 退出码 1

### 关键判断依据

- herdr 的 blocked 检测是"读屏幕"启发式, 对无人值守多 agent 主机是真痛点解法,
  但有 unknown 误判, 不能当强语义 — 可靠的任务状态还是要 Task 状态机(确定性)
- rdog PTY(detach/re-attach/输出帧)与 herdr pane 的持久会话是同构能力,
  差异在 herdr 面向交互 agent UI, rdog 面向跨主机控制
- 两者组合的接口点非常干净: agent 在 herdr pane 里跑, 内部用 bash 调
  rdog control localhost — 和现有 SKILL.md 的调用方式完全一致, 零适配
