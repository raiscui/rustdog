## [2026-05-14 10:40:21] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Planner 本地复核

## 来源

### 来源1: `.omx/plans/rdog-mouse-control-implementation-plan.md`

- 计划已选择 Option A: 显式命令变体 + 鼠标计划层 + 平台能力保护。
- 原计划把 structured success metadata 写成可选项。
- 原计划已经包含 `PermissionDenied -> code 77` 和 `Unsupported -> code 78`。

### 来源2: `specs/rdog-mouse-control-coordinate-plan.md`

- 源规格示例成功响应是结构化 value object。
- 鼠标坐标必须复用 screenshot manifest 的 `os-logical`。

## Planner 结论

- Verdict: REVISE 后可继续审查。
- 必须修订:
  - structured mouse success response 从可选项提升为 Option A 第一版硬要求。
  - backend capability guard 独立成实施阶段,避免多显示器坐标静默 clamp 或主屏偏移。
  - drag failure release、wheel axis order、request id 透传都要进入 acceptance criteria。
- 已修订:
  - 增加 `Consensus Update`。
  - 增加 `MouseExecutionReport` / `MouseReleaseRecovery` 计划模型。
  - 增加 Phase 3.5 structured response。
  - 增加 Phase 3.6 backend capability guard。

## [2026-05-14 10:42:03] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Architect 本地审查

## Architect antithesis

- 最强反方观点: 把 structured response 和 capability guard 都提升为第一版硬要求,会扩大实现面。对一个鼠标控制 MVP 来说,也许 `@response 0` 加真实 smoke 已够。

## Tradeoff tension

- 快速交付 vs agent 可验证性:
  - 快速交付可以少改 `control_core` renderer。
  - 但后续 code agent 需要知道实际执行坐标、button、release 状态和 backend。纯 `@response 0` 只能表示“某事成功”,无法作为远程桌面动作证据。

## Synthesis

- 保持 structured response 为 Option A 硬要求,因为用户目标是让 code agent 控制屏幕/硬件,不是只给人类看成功码。
- 为了控制风险,计划必须新增内部 `MouseBackend` facade,让组合动作的失败恢复可以通过 fake backend 测试,不依赖真实 OS 权限。

## Verdict

- Verdict: REVISE 后进入 Critic。
- 已修订:
  - 增加 Phase 2.5 鼠标 backend facade。
  - 执行层改为依赖 `MouseBackend`,真实 `Enigo` 只做 adapter。
  - integration tests 增加 fake backend failure tests。
  - risk 增加“组合动作无法稳定测试失败恢复”。

## [2026-05-14 10:44:18] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Critic 本地验收

## Critic checks

- Principle-option consistency: 通过。Option A 已经围绕 `os-logical`、显式命令、可测试恢复和平台保护展开。
- Fair alternatives: 通过。Option B 和统一 `@mouse:{kind}` 等替代方案有明确拒绝理由。
- Acceptance criteria: 初次检查发现一处冲突,已修订。
- Verification path: 通过。包含 parser/action/core/integration/ignored smoke/skill validate。

## 修订项

- 问题: Phase 3.5 原文仍说“可选设计”,ADR Consequences 仍说“如果 structured success metadata 成为硬需求”。这和 Consensus Update 中“Option A 第一版必须做 structured metadata”冲突。
- 修订:
  - Phase 3.5 固定采用 `ActionExecutionResult` 新增 `response_value_json: Option<String>`。
  - 明确 ordinary action 仍为 `None`,mouse executor 返回 `Some(value_json)`。
  - ADR Consequences 改为 structured success metadata 已是 Option A 第一版硬要求。

## Verdict

- Verdict: APPROVE。
- 剩余风险: 真正实现时仍需真实多显示器 smoke 证明平台坐标能力;计划已经要求不能证明时返回 `Unsupported` / code 78。
