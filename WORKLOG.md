# WORKLOG (新一档, 2026-08-09 续档)

旧 WORKLOG 超过 1000 行 (1002) 后按 continuous-learning 流程续档, 历史记录见
`archive/default_history/WORKLOG_2026-08-09_223000_before_continuous_learning_rollover.md`。

## [2026-08-09 22:35:00] [Session ID: omx-1786268168901-f711dm] 任务名称: continuous-learning 完整复盘 (2026-08-09)

### 任务内容
- Capture: docs/solutions/best-practices/macos-tcc-stable-codesign-identity.md (TCC 授权身份稳定方案)
- Capture: docs/solutions/best-practices/eval-carrier-drift-vs-model-regression.md (评测载体差异误判)
- Skill: ~/.codex/skills/self-learning.macos-codesign-stable-dr-check/ (三个 codesign 校验坑 + 可执行流程)
- AGENTS.md: 新增 docs/solutions/ 索引, EXPERIENCE.md 定位改为收件箱
- 续档: task_plan.md / WORKLOG.md 移入 archive/default_history/, 新建当前档
- Manifest: archive/manifests/ARCHIVE_MANIFEST__2026-08-09_continuous_learning_sign_identity.md

### 完成过程
- 回读六文件事实账本, 按 Session ID 区分来源
- 两份 solution 通过 frontmatter + claims 校验 (0 flags)
- EXPERIENCE.md 积压 23 段未全量逐条核验, 已承接的保留; 仅核验与本会话证据链相关的"评测载体差异"段并 Capture

### 总结感悟
- codesign canonical 输出格式随 identifier 内容变化, 断言必须归一化
- 评测载体 (runner/case/prompt/binary) 与模型能力必须分开归因

## [2026-08-09 22:48:00] [Session ID: omx-1786268168901-f711dm] 任务名称: LATER_PLANS 待办执行 (guard 清理 + warning 清理 + admin 日志定位)

### 任务内容
- zenoh guard/FIFO 清理: 5506 guard + 476 FIFO 回收, 诊断噪音消失
- warning 清理: 16 个 src 文件, 48 warning -> 0, 798 测试全过
- admin transport event: 定位 zenoh 源码触发点, 4 场景无法复现, 结论不修

### 完成过程
- FIFO 清理用 find -type p (FIFO 不满足 -f); guard 清理按内容 PID 存活判断
- warning 分类处理: unused imports (cfg(test) 隔离测试专用符号) / unused vars (_ 前缀) / recording 模块 allow(dead_code) / 2 个孤儿测试补 #[test]
- admin 调研: 静态定位 zenoh-1.8.0 admin.rs:229; 动态复现 20x unixpipe + 5x UDP + kill 均干净

### 总结感悟
- "warning 里 unused 的符号" 常常只是当前编译目标不用, 删除前必须 grep 全部 cfg 引用
- LATER_PLANS 记录的噪音源 (FIFO) 与真实匹配模式 (.pipe_uplink) 不一致时, 先读扫描代码再清理

## [2026-08-22 00:00:00] [Session ID: current] Ticket #04 完成: 通用 action 迁移

### 任务内容
将 `execute_ax_action` 从 `perform_default_ax_action` 迁移到 `ax_action::perform_action`。

### 完成过程

#### 1. 迁移调用路径
- 修改 `control_actions.rs`:
  - import: 删除 `perform_default_ax_action`，添加 `ax_action::perform_action`
  - `execute_ax_action`: `perform_default_ax_action(request)` → `perform_action(request)`

#### 2. 验证编译
- 编译通过（0 errors）
- 已启动后台测试验证

### 架构改进

#### 调用链简化
**Before**:
```
control_actions::execute_ax_action
  → control_ax::perform_default_ax_action
    → SystemAxBackend.perform_action
```

**After**:
```
control_actions::execute_ax_action
  → ax_action::perform_action
    → SystemAxBackend.perform_action
```

减少一层中间调用。

#### 职责清晰化
- `control_actions.rs`: RPC routing 层，不关心 AX 实现细节
- `ax_action::perform_action`: 统一 action 执行入口
- `SystemAxBackend`: 平台实现

### 待验证
- [ ] 后台测试通过（b5tpdy6ja）
- [ ] press_sequence 测试通过（b3uzz7d02）

### 下一步
- 等待测试结果
- 如果通过，更新 task_plan.md 标记 Ticket #04 完成
- 准备 Ticket #05: 标记 perform_default_ax_action 为 deprecated

## [2026-08-22 01:50:00] [Session ID: zcode-sess_fa3b551c] 任务名称: 闲时 continuous-learning 整理 (无人值守)

### 任务内容
- 回读默认六文件 + EXPERIENCE.md + docs/solutions 7 份 + 8-13~8-20 git 提交链, 按七项门禁评估 Capture 候选
- Capture: docs/solutions/architecture-patterns/ax-observation-cached-progressive-queries.md
  (承接 60f9e26 + 1066123 + 8af9e12 三个提交的 cached progressive queries 架构契约, 此前无文档载体)
- Scoped Refresh: docs/glossary.md 的 verify_failed 条目仍写 "ok:false error_code" 旧语义,
  已按 outcome 三态现状 Update, 并补 outcome 术语 (证据: src/control_computer_act/outcome.rs:16 与
  error_envelope.rs:63 注释)
- AGENTS.md: Domain docs 节明确 CONTEXT.md (canonical) 与 docs/glossary.md (computer-act surface)
  分工; 长期文件索引新增该 solution 与 docs/glossary.md 两条入口
- LATER_PLANS 清理 4 条已落地条目: warning 清理 (8-09 完成, 752125b)、admin transport event
  (8-09 定位不改)、guard/FIFO 清理 (8-09 完成)、screenshot timeout-trace flaky (881b300 加
  TIMEOUT_TRACE_TEST_LOCK 于 capture_trace helper, 三个调用点全部串行化)
- Gate 判定 skip 的候选: trusted changes (gui-resource-epoch solution 8-20 已承接)、
  screenshot serialize (琐细细粒度, 同族知识已在 EXPERIENCE 2026-07-18 唯一共享锁条目)

### 完成过程
- 工作区 src/control_actions.rs 有进行中的 ax_action 重构导致 bin 编译失败, 未触碰用户改动,
  改用 git worktree 在干净 HEAD (8af9e12) 复跑测试后清理
- EXPERIENCE.md 保持不动: 8-13 条目的 upstream Pi 已 Capture 说明仍准确; 8-08 ledger 条目由
  workflows/macos-ops-interaction-efficiency.md 完整承接 (计量口径/分类/认证门槛逐一核对)

### 总结感悟
- 8-13 之后的 session 大量落地功能但未维护六文件 (WORKLOG 停在 8-09), 闲时整理要靠
  git log --stat 考古补齐知识链; solution last_updated 与提交时间对齐是健康的信号
- 用户进行中的重构会让主工作区无法编译, worktree 干净态复跑是无人值守任务获取动态证据的
  可复用手段, 不碰用户工作区

## [2026-08-22 12:47:43] [Session ID: zcode-sess_fa3b551c] 任务名称: continuous-learning 全量整理 (用户显式调用)

### 任务内容
- 清偿 2026-08-09 遗留欠账: EXPERIENCE.md 积压 27 段候选全量核验分流
- Capture x3: gui-target-owner-evidence-gate (含 WeChat 政策漂移修复) /
  zenoh-hello-locator-priority / daemon-log-sentinel-e2e-contract
- 发现并处置重大漂移: WeChat Temporary No-AX Policy (fail-closed 安全政策) 于
  2026-07-28 被 92a3d06 skill 瘦身无记录移除, AGENTS.md 与 EXPERIENCE 双悬空;
  政策本体抢救进 solution, 悬空引用修复, 恢复决策记入 LATER_PLANS
- EPIPHANY_LOG 续档: 999 行 -> 归档 31 条已核查旧档 + 新档写入 token 优化删除
  安全政策的系统性风险; manifest 见
  archive/manifests/ARCHIVE_MANIFEST__2026-08-22_epiphany_rollover_wechat_policy_drift.md

### 完成过程
- 27 段候选逐段定向取证: 11 段确认既有载体承接, 3 段通过七项门禁 Capture,
  其余确认代码/spec/skill 即载体保留索引 (处置明细见 EXPERIENCE 尾部指针条目)
- 三份 solution 全部通过 frontmatter + claims 双校验 (0 flags)
- AGENTS.md: 修 skill 条目悬空声明 + 追加 3 条 solution 索引

### 总结感悟
- 积压收件箱的全量核验最大价值不是 Capture 数量, 而是抓到了 "安全政策被 token
  优化静默删除 + 双处引用悬空 25 天" 这种单 Session 视角永远看不到的漂移
- 经验类知识放 canonical skill (会被度量裁剪的载体) 等于没放; durable 载体 +
  skill 引用才是安全政策的正确分层

## [2026-08-22 13:13:08] [Session ID: zcode-sess_fa3b551c] 任务名称: WeChat no-AX 政策恢复进 SKILL.md + git 提交

### 任务内容
- 用户决策 "同意恢复": 政策原文 (2026-07-14 版) 从 git 历史 (92a3d06^) 逐字恢复进
  SKILL.md v2.28-wechat-noax-restore, 位于 Native App Lane 末尾
- Safety 节加 WeChat 指针行; 政策节尾部加来源注 (durable 载体 solution 路径 +
  "safety boundary, 禁止 token 优化移除" 声明)
- 载体同步: solution Evidence 更新恢复状态; AGENTS.md skill 注释翻转;
  LATER_PLANS 决策项清除; EPIPHANY_LOG 追加决议

### 完成过程
- 验证矩阵: 政策正文与 92a3d06^ 逐字 diff 一致; fence 14 个成对; 关键句
  (xinWeChat / 文件传输助手 / 政策标题) 全在; git diff --check 干净;
  solution 双校验复跑 0 flags
- git 分两笔提交: (1) skill 政策恢复; (2) continuous-learning 知识批次

### 总结感悟
- 恢复用 "从 git 历史逐字复原 + diff 验证" 而非凭记忆重写, 保证安全政策零语义漂移
- 来源注里显式声明 "safety boundary" 是给未来优化 pass 的护栏, 把这次教训固化进载体本身
