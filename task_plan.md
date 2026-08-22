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

## [2026-08-22 11:20:00] [Session ID: zcode-idle-20260822] [记录类型]: 报备 - 闲时 continuous-learning 整理

### 目标
无人值守闲时整理: 回读六文件 + EXPERIENCE + docs/solutions, 运行 Compound Gate/Refresh,
清理已落地 LATER_PLANS 条目, 核对 glossary 双载体, 同步 AGENTS 索引, 收尾交付报告。

### 已确认的事实 (证据)
- 六文件与 git HEAD 一致, 无未提交六文件改动; 工作区仅 src/control_actions.rs (非本轮产物, 不动)
- 8-13~8-20 新提交的知识大多已由当期 session 沉淀: durable-observation solution (8-19),
  gui-resource-epoch solution (8-20), upstream-pi solution (8-15), user-config-dir spec
- LATER_PLANS 有 4 条已落地未清理条目: warning 清理 (8-09 完成), admin transport event
  (8-09 已处置), guard/FIFO 清理 (8-09 完成), screenshot timeout-trace flaky (881b300 8-18 加锁)
- 仓库存在双 glossary 载体: 根级 CONTEXT.md (canonical, AGENTS 已索引) + docs/glossary.md
  (@computer-act 术语, AGENTS 未索引)
- EPIPHANY_LOG.md 999 行, 未超 1000 续档线

### 阶段
- [ ] 阶段1: 回读六文件 + EXPERIENCE + solutions (已完成大部分)
- [ ] 阶段2: 核实 trusted changes / cached progressive queries 载体, 运行 Compound Gate
- [ ] 阶段3: Scoped Refresh + glossary 双载体处置 + AGENTS 索引同步
- [ ] 阶段4: 清理 LATER_PLANS 已落地条目
- [ ] 阶段5: 验证 + WORKLOG 收尾 + 交付报告

## [2026-08-22 01:50:00] [Session ID: zcode-sess_fa3b551c] [记录类型]: 完成 - 闲时 continuous-learning 整理

### 阶段结果
- [x] 阶段1: 回读六文件 + EXPERIENCE + solutions
- [x] 阶段2: Compound Gate 判定 (cached progressive queries = capture; trusted changes = 已承接 skip; screenshot serialize = 琐碎 skip)
- [x] 阶段3: Scoped Refresh (docs/glossary.md verify_failed 漂移 -> Update) + AGENTS.md 双索引
- [x] 阶段4: LATER_PLANS 清理 4 条已落地条目
- [x] 阶段5: 验证 + WORKLOG 收尾

### 本轮产物
- 新增 solution: docs/solutions/architecture-patterns/ax-observation-cached-progressive-queries.md (双校验 0 flags)
- 更新 docs/glossary.md: verify_failed 漂移修正 + 补 outcome 三态术语
- 更新 AGENTS.md: Domain docs 节 glossary 分工 + 新 solution 与 docs/glossary.md 索引
- 清理 LATER_PLANS: warning 清理 / admin transport event / guard+FIFO / screenshot flaky (881b300 落地) 4 条

### 验证
- 干净 HEAD worktree 复跑: cargo nextest 13 passed (cached/bounded/budget)
- validate-solution-frontmatter / claims 双通过

### 备注
- 工作区 src/control_actions.rs 有用户进行中的 ax_action 重构 (未编译通过), 全程未触碰;
  测试验证改走独立 worktree
- EPIPHANY_LOG 999 行未超续档线, 本轮无新增重大风险, 不追加

## [2026-08-22 12:40:11] [Session ID: zcode-sess_fa3b551c] [记录类型]: 报备 - 用户显式调用 continuous-learning 全量整理

### 目标
清偿 2026-08-09 遗留欠账: EXPERIENCE.md 积压 27 段候选全量逐条核验分流 (七项门禁),
对通过门禁的执行 Compound Capture, 已承接的确认索引链, 失效的记录处置理由。

### 阶段
- [ ] 阶段1: 状态刷新 (已完成: 上轮产物未提交待审, ax_action 重构未落地, 无支线集)
- [ ] 阶段2: EXPERIENCE 27 段候选分组 + 定向取证 (代码静态证据核查)
- [ ] 阶段3: Compound Gate 逐段裁决 + Capture 通过项
- [ ] 阶段4: Scoped Refresh + AGENTS 索引同步
- [ ] 阶段5: 双校验脚本 + WORKLOG 收尾 + 报告

### 约束
- 不动 src/, 不提交 git, EPIPHANY_LOG 999 行慎追加 (追加即触发续档流程)

## [2026-08-22 12:47:43] [Session ID: zcode-sess_fa3b551c] [记录类型]: 完成 - continuous-learning 全量整理

### 阶段结果
- [x] 阶段1: 状态刷新 (上轮产物未提交待审, ax_action 未落地, 无支线集)
- [x] 阶段2: EXPERIENCE 27 段候选分组 + 定向取证
- [x] 阶段3: Gate 裁决 (3 capture / 11 已承接 skip / 其余代码即载体保留索引)
- [x] 阶段4: 漂移处置 (WeChat 政策抢救 + AGENTS 修复 + EPIPHANY 续档 + manifest)
- [x] 阶段5: 双校验 + WORKLOG 收尾 (locator 测试结果确认后交付报告)

### 重大发现
- WeChat no-AX 安全政策被 92a3d06 瘦身移除且双引用悬空 -> 已抢救 + 记 LATER_PLANS
  待用户决策是否恢复进 SKILL.md

## [2026-08-22 13:13:08] [Session ID: zcode-sess_fa3b551c] [记录类型]: 完成 - WeChat 政策恢复 + git 提交

- [x] 政策逐字恢复进 SKILL.md v2.28 (Native App Lane 末尾 + Safety 指针 + 来源注)
- [x] 载体同步 (solution / AGENTS / LATER_PLANS / EPIPHANY)
- [x] 验证矩阵全过 (逐字 diff / fence / grep / diff-check / 双校验)
- [x] git 提交 (排除用户进行中的 src/control_actions.rs 与 .mimosa)
