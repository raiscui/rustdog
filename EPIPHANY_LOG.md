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
