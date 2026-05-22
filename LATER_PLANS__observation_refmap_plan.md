## [2026-05-19 09:11:02] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 后续计划: P0 落地后再细化 P1

### 背景
- 本轮只完成 `specs/rdog-observation-scoped-refmap-plan.md` 的 P0 可落地计划。
- 用户明确要求先做 P0,后续完成 P0 落地后再做 P1 细化方案。

### 待 P0 完成后处理
- [ ] 根据 P0 实际实现和验证结果,创建 P1 durable observation state 细化计划。
- [ ] P1 再决定 daemon-owned state dir、observation metadata 持久化格式、selector index schema 和 restart recovery 策略。
- [ ] P1 不应回头修改 P0 的短期 `@eN` 语义。短期 ref 仍然是 ephemeral,selector 才负责持久恢复。

### 触发条件
- P0 plan `.omx/plans/ralplan-rdog-observation-refmap-p0.md` 被 `$ralph` / `$team` 落地。
- P0 验证完成,尤其是 AX / screenshot / window-find 的 observation header 和 stale ref 错误契约已经通过测试。

## [2026-05-19 23:50:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 后续计划状态: P1 已进入细化

### 状态
- P0 已完成,且 Ralph active state 已关闭。
- 用户已要求进入 P1 durable observation state / selector 细化方案。
- 后续执行记录切换到 `task_plan__observation_refmap_p1.md`、`notes__observation_refmap_p1.md`、`WORKLOG__observation_refmap_p1.md`、`LATER_PLANS__observation_refmap_p1.md`。

### 处理
- 上一条 P1 待办不再视为“未启动”。它已迁入 P1 支线上下文继续推进。
- P1 仍只做 durable state / selector 细化方案,不把 P2 semantic re-find、P4 `@observe`、P5 mouse ref 化提前混入本轮落地范围。
