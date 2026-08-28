## [2026-08-28 09:40:00] [Session ID: current] 任务名称: A2A 协议调研与 Task/Spawn 演进方案

### 任务内容
- 五轮递进: A2A v1.0 调研 → channel 分享设计 → 伴生 agent 定位 → herdr 评估 → daemon 并发模型验证
- 最终交付 specs/rdog-task-spawn-control-plan.md(四阶段演进正式 spec)

### 完成过程
- Web 调研 A2A v1.0(LF 治理, 三 binding, AgentCard/Task/Artifact 语义)
- Explore agent 代码级梳理控制面现状(ControlFrame/session channel/PTY/安全边界)
- 逐行验证 daemon 并发模型: 主 queryable 串行 + legacy 线程 + per-session bridge,
  同 session 帧串行(daemon_bridge.rs:313), 确认用户"长命令阻塞"担忧部分成立
- 确认 rdog 无 rdog agent 命令, 无后台任务原语, PTY 是唯一长进程机制(语义错位)
- herdr 评估: 用而不集成(Apache 2.0/Rust/单机/无任务分配), 借异步原语形态

### 总结感悟
- "学语义不换传输"是本轮核心结论: A2A 的 Task/AgentCard 语义补 rdog 缺口,
  Zenoh 执行层保持优势
- 用户每个追问都推动一次架构精确化: channel 设计 → 语义主体(agent 侧) →
  并发真相(同 session 串行) → 原语缺口(@spawn)
- herdr 的 pane run 提交即返回 + wait/read 分离正是 rdog 缺的原语形态,
  不需要它的代码但值得抄它的接口形状
