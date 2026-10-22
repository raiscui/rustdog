# EPIPHANY_LOG (新一档, 2026-08-22 续档)

旧 EPIPHANY_LOG 达到 999 行, 因本轮追加 WeChat 政策漂移发现而触发续档。历史记录见
`archive/default_history/EPIPHANY_LOG_2026-08-22_124616_before_wechat_policy_drift_rollover.md`,
归档说明见 `archive/manifests/ARCHIVE_MANIFEST__2026-08-22_epiphany_rollover_wechat_policy_drift.md`。
旧档 31 条已完成载体核查, 全部由 specs / EXPERIENCE / docs/solutions / 用户级 skill 承接。

## [2026-08-22 12:46:16] [Session ID: zcode-sess_fa3b551c] 主题: skill token 优化会静默删除安全政策, 且引用悬空无人发现

### 发现来源
- 本轮 continuous-learning 对 EXPERIENCE [2026-07-14] 坐标 AX hit-test 条目做载体核查:
  条目声明 "WeChat Temporary No-AX Policy 运行时真相源在 SKILL.md", 但当前 SKILL.md
  零命中; `git log -S 'WeChat'` 定位到 92a3d06 (2026-07-28 "optimize rdog-control:
  add Failure Handling 3-tier table (dim3)", -209/+131 行)。

### 核心问题
- skill 瘦身 pass 把安全政策章节 (fail-closed WeChat no-AX, 基于已验证的 foreign-tree
  归属失败) 与普通内容放在同一裁剪平面上整体删除, 提交正文无任何移除意图记录。
- AGENTS.md 的 skill 索引 ("覆盖 ... WeChat no-AX override") 与 EXPERIENCE 条目双双
  悬空 25 天, 没有任何机制发现 "被引用章节已不存在"。

### 为什么重要
- canonical skill 是 agent 运行时安全边界的执行点, 但它同时是被 token 度量和优化的
  对象; 两个目标天然冲突, 目前没有 protected-section 或引用完整性校验兜底。
- 政策失效的后果不是报错而是静默回退到危险行为: agent 重新对 WeChat 用 AX, 重新
  暴露 2026-07-14 已验证的 foreign-tree 误归属 (可能点到别的应用)。

### 未来风险
- 任何未来的 dim4/dim5 优化 pass 都可能再次删除安全内容; 其它长章节 (destructive
  action safety, permission 边界) 面临同样风险。
- "索引指向已删除内容" 这类悬空在 AGENTS.md / EXPERIENCE / specs 间还有多少, 未盘点。

### 当前结论
- 已确认: 政策本体已从 SKILL.md 消失, 全仓库 (排除 archive) 仅剩 AGENTS.md 与
  EXPERIENCE.md 两处悬空引用, 政策全文只在 git 历史 (92a3d06^)。
- 已处置: 政策与 owner-evidence 门禁抢救进
  docs/solutions/best-practices/gui-target-owner-evidence-gate.md (已过双校验);
  AGENTS.md 悬空引用本轮修复。
- 未决: 是否把政策恢复进 SKILL.md (影响 token 指标), 待用户决策, LATER_PLANS 已记。

### 后续讨论入口
- git show 92a3d06 -- .codex/skills/rdog-control/SKILL.md (被删全文)
- .codex/skills/rdog-control/results.tsv (优化度量记录, 无删除意图注记)

## [2026-08-22 13:12:32] [Session ID: zcode-sess_fa3b551c] 决议: WeChat no-AX 政策恢复进 SKILL.md

- 用户已决策 "同意恢复"。政策原文 (2026-07-14 版) 逐字恢复进
  .codex/skills/rdog-control/SKILL.md v2.28, 位于 Native App Lane 末尾;
  Safety 节加指针; 附来源注标明这是安全边界, 禁止后续 token 优化 pass 移除。
- 上一条目中 "未决: 是否把政策恢复进 SKILL.md" 已解决, LATER_PLANS 决策项已清除。

## [2026-08-28 16:45:00] [Session ID: current] 主题: macos flake 双族未修前, 每个 PR 都需人工判定放行

### 发现来源
- 本轮处理 PR #62/#63/#68: 三个 PR 的 macos job 全部挂已知 flake 族放行

### 核心问题
- ubuntu 修复后 (PR #62), macos 成为唯一常态红源; screenshot 锁毒化族
  接近每轮必挂, 流程上每个 PR 合并都要重跑+比对+人工放行一次

### 为什么重要
- flake 判定依赖与 main 同轮比对, 成本随 PR 频率线性增长; 新会话若不读
  记忆/LATER_PLANS 可能把 screenshot 4 件套误判为回归, 浪费排查时间

### 未来风险
- 若有人"顺手"重试到 screenshot 恰好全过的轮次, 可能误以为已自愈;
  锁毒化根源 (10ms 计时窗口 + TIMEOUT_TRACE_TEST_LOCK) 不修不会消失

### 当前结论
- 修复方向已定且成本低 (计时窗口自适应 + 锁毒化恢复), 记录于 LATER_PLANS

### 后续讨论入口
- src/screenshot/tests.rs:54-120 + LATER_PLANS.md screenshot 条目

## [2026-08-28 17:40:00] [Session ID: current] 主题: main 的 macos recording CI 已从抽签 flake 恶化为稳定红

### 发现来源
- PR #69 rebase 到 main 后 CI: macos 挂 recording_manual_cancel_before_deadline (两次重跑同测再挂);
  main 最近 3 个 run 全红 (66685ce 起), 同族甚至更多测试挂 (manual_stop 也挂)

### 核心问题
- 该族不再是计时抽签: 重跑不翻 = 环境决定性稳定红
- 指纹: 本地 992/992 全绿 (含此测试) + CI 稳定红
- panic 点: tests/recording_e2e.rs:395 parse_response_value().unwrap() on None
  = @record-start 的响应在 CI 5s 窗口内没带回 @response 标记
- main 已合入两笔 recording flake 修复 (435be72 daemon 就绪探活 / c3267df owner 重连轮询),
  未覆盖 cancel 族的这个路径

### 为什么重要
- 红的 main 会污染所有后续 PR 的 macos 信号, "与 main 同轮比对"口径会越来越难执行
- recording 是 recorder/replay 主线的验收地基, CI 上跑不了会侵蚀信任

### 未来风险
- 后续 PR 全部带红 merge, 真回归被淹没
- CI runner 与本地环境差异 (TCC 权限) 无诊断日志, 排查盲飞

### 当前结论
- 已确认: 与 task-spawn Phase 1/2 改动无关 (零接触 + ubuntu 全绿 + 本地全绿)
- 未确认: CI 上 @record-start 的真实响应形状 (需加诊断日志)

### 后续讨论入口
- 入口: tests/recording_e2e.rs read_response_line 加超时时的原始行日志,
  在 CI 复现一轮看 @record-start 到底回了什么 (error envelope? 超时空?)

## [2026-10-22 14:30:00] [Session ID: mvs_04f490f87a924c96ac1cb0adcbd30806] 主题: "未核实"是知识库里最贵的三个字 —— 错的版本还住在 agent 真正会读的文件里

### 发现来源
- 本轮 continuous-learning 回读 `remote-control-blocked-command-pitfalls.md` 的落地表,
  发现 P1-1 行 (2026-10-06 写下) 记着"文档/skill 里的换算口诀 —— **未核实**"
- 核实动作: 打开 skill `.codex/skills/rdog-control/references/protocol.md`, 对照
  `src/control_mouse/screenshot_frame.rs:17` 的权威公式

### 核心问题
- skill 给的是 `os_x = image_x + virtual_bounds.x`; 代码的权威公式是
  `os_x = os_logical_bounds.x + (image_rect.x + image_x) * image_to_click_scale.x`
- 差三处: **原点** (`virtual_bounds` 物理像素 vs `os_logical_bounds` 逻辑像素)、
  **标尺** (缺 `image_to_click_scale`, Windows 4K@200% 下差 2 倍)、
  **平移** (缺 `image_rect`, 窗口裁剪图整体偏移一个窗口原点)
- 后果: 这正是 2026-10-05 那场实战"连续三次点空、浪费约 20 分钟"的同一个坑 ——
  **代码里早就有正确公式, 但错的那份留在了 agent 实际会读的地方**
- 该行挂了 16 天没人核: 因为它标的是"未核实" —— 既不提醒、也不负责
- **不是孤例**: 顺藤清点后, 同一份单段式公式共出现在 **5 处载体** ——
  skill `references/protocol.md` (2 处)、`README.md`、`specs/code-agent-rdog-control-usage.md`、
  `specs/rdog-multi-display-screenshot-coordinate-plan.md`、
  `specs/rdog-mouse-control-coordinate-plan.md`。最早写死在 2026-05/07 的两份
  coordinate plan 里, 后面的载体是**照抄**传播的
- 为什么一直没被发现: 原始设计面向 macOS (scale_factor = 1,
  `virtual_bounds == os_logical_bounds`), 单段式在 macOS 上**逐字成立**;
  HiDPI 层 (2026-10-06) 才让它失效, 而 macOS-only 的验证看不见 —— 与本仓
  `env-deterministic-ci-red-vs-local-green` 那族"本地绿/别的环境红"同源

### 为什么重要
- 知识库里的"未核实"如果只是个待办标签, 它会静默过期; 本例过期后还带着一个
  **主动误导**的公式, 比没有更贵
- 载体之间没有对账机制时, **错的那份更容易被信**: 代码里的正确公式要先被找到
  才会被用, 而 skill 里的错误公式是 agent 的默认路径
- 同类风险面不止公式: 坐标换算、协议常量、状态机语义、超时默认值, 只要是
  手写进 skill / references / README 的断言, 就有机会与代码真相源分叉

### 未来风险
- 下一轮 skill 瘦身 / 文案改写若再动这几段, 可能把"公式"换成"描述"而丢掉精度
- `docs/solutions/` 的落地表是"进度真相源", 但它同样会过期 (本轮发现 P3-2 / P4
  在表里写了 16 天"未落地", 而代码里早已落地) —— **表里的"未落地"也可能只是过期,
  不代表事实**

### 当前结论
- 已修: skill `references/protocol.md` 的换算公式 (2 处) 改为代码权威公式并指向
  manifest 自带的 `transforms` 字段; `README.md`、
  `specs/code-agent-rdog-control-usage.md`、
  `specs/rdog-multi-display-screenshot-coordinate-plan.md`、
  `specs/rdog-mouse-control-coordinate-plan.md` 同步校正 (前两份 spec 保留"校正"注记);
  `remote-control-blocked-command-pitfalls.md` 落地表按代码复核重判
- 已开票: 三条延期项见 LATER_PLANS (issue #121 / #122 / #123)
- 未决: **载体公式类断言没有自动对账机制** —— 本轮是**人工清点**出 5 处分叉后逐个
  修的, 下一个载体再抄一次仍然会错。最小可行做法: 把 skill / references /
  README / specs 里的换算与协议常量列成清单, 在 solution 校验时人工对照 `src/`
  核一遍; 更彻底是从代码生成 (成本高, 暂不做)
- 顺带发现: 载体应优先写"**去 manifest 读 transforms**"这类**指向真相源的指针**,
  而不是**手抄公式** —— 指针不会过期, 手抄的会。本轮已把 5 处改成指针 + 公式并存

### 后续讨论入口
- `src/control_mouse/screenshot_frame.rs:15-23` — 权威公式与边界注释的所在地
- `.codex/skills/rdog-control/references/protocol.md` — 本轮被修的错公式载体
- `docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md`
  落地表 — 本轮重判的进度真相源
