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
