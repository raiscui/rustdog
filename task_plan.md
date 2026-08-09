# 任务计划: (新一档, 2026-08-09 续档)

# 目标

旧 task_plan 超过 1000 行 (1056) 后按 continuous-learning 流程续档, 历史记录见
`archive/default_history/task_plan_2026-08-09_223000_before_continuous_learning_rollover.md`,
归档说明见 `archive/manifests/ARCHIVE_MANIFEST__2026-08-09_continuous_learning_sign_identity.md`。

# 阶段

- [x] 2026-08-09: macOS 签名身份方案 (issue #40) 已完成并实施 (scripts/install-signed.sh)
- [x] 2026-08-09: continuous-learning 复盘 (2 份 solution + 1 个 skill + 续档)
- [ ] 活跃待办 (见下)

# 当前活跃事项

- 3 个有未提交工作的分支待用户决策: feature/wechat-ax-disconnected-roots (源码改动, 重要)、
  feature/display-aware-control-chain (文档)、feature/screenshot-cache-ttl (文档)
- feature/computer-act-outcome-3state 已非 main 祖先, 待用户决定是否删除
- LATER_PLANS: zenoh guard/unixpipe FIFO 清理、Rust binary 15+ warning 清理、Zenoh admin transport event 日志
- 一次性操作提醒: ~/.cargo/bin/rdog 已切换固定签名身份, 首次需重新授权 Accessibility + Screen Recording

# 状态

**2026-08-09 22:30** - continuous-learning 收尾中, 待提交本轮产物

## [2026-08-09 22:50:00] [Session ID: omx-1786268168901-f711dm] [记录类型]: 报备 - 执行 LATER_PLANS 待办

### 动作 (用户确认 "2" = LATER_PLANS 待办)
- 1. zenoh guard / unixpipe FIFO 清理 (低风险, 先做)
- 2. Rust binary 既有 warning 清理 (代码改动, 需全量测试)
- 3. Zenoh admin transport event 日志调研 (复现定位)

## [2026-08-09 22:45:00] [Session ID: omx-1786268168901-f711dm] [记录类型]: 完成 - LATER_PLANS 三个待办

### 1. zenoh guard / FIFO 清理 (完成)
- guard: 5478+28 个死 guard + 422 lease 移入回收目录; FIFO: 476 个 $TMPDIR/rdog-*.pipe_uplink/downlink 移入回收目录
- 验证: `rdog control self @ping` 不再列几百行候选
- 坑: FIFO 用 `-f` 判断不成立, 须用 `find -type p`; guard 内容是 PID

### 2. Rust warning 清理 (完成)
- 48 个 warning -> 0 (unused imports 删条目, unused vars 加 _ 前缀, recording 模块级 allow(dead_code), 2 个孤儿测试补 #[test])
- 全量测试 796 -> 798 全过
- 教训: 测试代码引用的符号不能从顶层 use 删, 用 #[cfg(test)] 单独导入

### 3. Zenoh admin transport event 日志 (已定位, 不改)
- 机制: zenoh-1.8.0 admin.rs:229 transport event 回调在 session 已关闭时 put 失败
- 复现尝试 4 场景均未出现; 不修 (统一 LevelFilter 无法按模块过滤, EnvFilter 属过度设计)
- 新观察: UDP 模式向 VPN 虚拟网卡广播 Hello 报错噪音 (LATER_PLANS 已记)
