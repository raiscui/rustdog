# 任务计划: observation-scoped refmap P3 semantic re-find 可落地计划

## [2026-05-20 19:57:24] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 启动 P3 Ralplan 规划

### 目标

根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P3 可执行计划,输出到 `.omx/plans/ralplan-rdog-observation-refmap-p3.md`。

### 范围边界

- 本轮只做规划,不修改 Rust 实现代码。
- P3 重点是 semantic re-find: stale/expired ref 后基于 permanent selector 和当前 UI 状态尝试恢复。
- P3 必须承接 P2 的 `@selector-get` / `@selector-resolve` dry-run surface,不能另起旁路。
- P3 不做完整 `@observe` 总入口,不做 mouse command ref 化,不让旧 `@eN` 在 daemon 重启后复活。
- 自动 re-find 只能在明确 gate 下发生: 单候选、高置信度、可解释、无副作用歧义。

### 阶段

- [x] 阶段1: 读取 `$ralplan` / 表达约束 / memory 摘要
- [ ] 阶段2: 回读 roadmap、P2 plan、P2 落地证据和当前代码触点
- [ ] 阶段3: 提炼 P3 方案选项、置信度模型、错误契约和 ADR
- [ ] 阶段4: 创建 context snapshot 与 P3 plan
- [ ] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### 当前状态

**目前在阶段2** - 正在收集 P3 需要继承的 P2 selector resolve 事实,并确认哪些能力应延期到 P4/P5。

## [2026-05-20 20:05:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: P3 draft 与 Architect 审查完成

### 已完成

- [x] 阶段1: 读取 `$ralplan` / 表达约束 / memory 摘要
- [x] 阶段2: 回读 roadmap、P2 plan、P2 落地证据和当前代码触点
- [x] 阶段3: 提炼 P3 方案选项、置信度模型、错误契约和 ADR 初稿
- [ ] 阶段4: 创建最终 P3 plan
- [ ] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### Architect 审查结论

- Verdict: APPROVE,带非阻塞补强建议。
- 主要张力: 协议语义干净 vs 命令面复杂度增加。
- 采纳方向: 保留新增 `@selector-refind`,但最终计划必须补强:
  - `fresh_target` 不代表动作已验证成功。
  - scoring 权重和 hard gate 必须成为 conformance surface。
  - P3 wire surface 对权限/backend 阻断统一返回 `decision:"blocked"`,parse / invalid payload 仍走协议错误。

### 当前状态

**目前在 Critic 审查阶段** - 准备让 Critic 检查方案是否满足 testable acceptance、风险缓解和执行可落地性。

## [2026-05-20 20:09:31] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [迭代]: Critic 要求补硬契约

### Critic 结论

- Verdict: ITERATE。
- 方向认可,但要求把 Architect 三条建议从文档提醒升级成 protocol + fixture + acceptance 硬约束。

### 必须修改

- [ ] `decision:"blocked"` 必须成为 P3 wire surface 的强制规则,permission/backend/schema/capability 阻断不能一半 error 一半 response。
- [ ] `fresh_target` 不得暗示动作已完成,`decision:"rebound"` 必须包含 required `verify_hint`。
- [ ] scoring table 必须版本化,每个 score source 有固定 reason code,每个 hard gate 有 fixture/golden。
- [ ] 验收标准补 blocked 无 `fresh_target`、rebound 必有 `verify_hint`、low/medium/multiple high 不自动 action。

### 当前状态

**目前在修订阶段** - 正在把 Critic 必须项合入 draft,随后重新走 Architect -> Critic。

## [2026-05-20 20:13:47] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: 第二轮 Architect 通过

### 审查结论

- Verdict: APPROVE。
- 架构方向继续选择 Option B: 新增显式 `@selector-refind`。
- 非阻塞补强: 如果调用方跳过 `verify_hint`,必须产出可审计 evidence/log 字段,避免 verify 前置规则被口头跳过。

### 当前状态

**目前在最终 Critic gate 前** - 采纳跳过 verify 的 evidence 要求,然后提交 Critic 二审。

## [2026-05-20 20:27:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: P3 Ralplan 计划已落盘并验证

### 已完成

- [x] 阶段1: 读取 `$ralplan` / 表达约束 / memory 摘要
- [x] 阶段2: 回读 roadmap、P2 plan、P2 落地证据和当前代码触点
- [x] 阶段3: 提炼 P3 方案选项、置信度模型、错误契约和 ADR
- [x] 阶段4: 创建 context snapshot 与 P3 plan
- [x] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### 交付文件

- `.omx/plans/ralplan-rdog-observation-refmap-p3.md`
- `.omx/context/observation-refmap-p3-20260520T120059Z.md`
- `.omx/drafts/ralplan-rdog-observation-refmap-p3-draft.md`
- `notes__observation_refmap_p3.md`
- `task_plan__observation_refmap_p3.md`

### 验证证据

- `beautiful-mermaid-rs --ascii` 已验证 P3 architecture flowchart。
- `beautiful-mermaid-rs --ascii` 已验证 P3 selector-refind sequenceDiagram。
- 未收口占位 / 摇摆措辞检查无命中。
- `git diff --check -- ...` 对本轮 P3 文件通过。
- `omx state write --input '{"mode":"ralplan",...}' --json` 返回 `success:true`。
- `.omx/state/sessions/019e38be-b9d9-76f0-aabc-fad94a2bcf12/ralplan-state.json` 与 `omx state get-status` 均显示 `active:false,current_phase:"complete"`。

### 当前状态

**完成** - P3 semantic re-find 计划已可交给 `$ralph .omx/plans/ralplan-rdog-observation-refmap-p3.md` 执行。

## [2026-05-20 22:41:22] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [执行]: Ralph 开始落地 P3 semantic re-find

### 行动目的

- 用户已用 `$ralph .omx/plans/ralplan-rdog-observation-refmap-p3.md` 要求执行 P3。
- 本轮从规划切换为代码、测试、文档和验证落地。

### 将要做什么

- [x] 复用 P3 context snapshot 并写入 Ralph state。
- [ ] 实现 `@selector-refind` request / parser / dispatch。
- [ ] 抽出 selector candidate collection,保持 P2 `@selector-resolve` 行为不变。
- [ ] 实现 `rdog.selector.score.v1` deterministic scoring、reason codes、confidence band、hard gate 和 stable tie-break。
- [ ] 实现 rebound / needs_disambiguation / not_found / blocked response,以及 required `verify_hint` 和 verify-skip audit evidence contract。
- [ ] 升级 stale / expired durable payload 的 refind hint。
- [ ] 新增 fixtures / focused tests。
- [ ] 更新 `rdog-control` skill 和 repo specs。
- [ ] 运行验证矩阵、Architect gate、deslop、post-deslop 回归。

### 当前状态

**目前在实现阶段** - 先接协议模型和 control_core dispatch,再拆 scoring / response 细节。

## [2026-05-20 22:49:55] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [恢复]: 继续 P3 semantic re-find 落地

### 恢复点

- 上一轮已经开始新增 `src/control_observation/refind.rs`,但协议接线、fixtures、文档和验证还没有完成。
- 当前 Ralph goal mode 没有 active goal,本轮以 `.omx/plans/ralplan-rdog-observation-refmap-p3.md` 的验收矩阵作为停止条件。

### 下一步行动

- [ ] 检查当前 dirty diff,确认只接续 P3 相关文件,不碰无关改动。
- [ ] 修正 `refind.rs` 初版里的编译问题。
- [ ] 接入 `ControlCommand::SelectorRefind` parser、dispatch 和 action guard。
- [ ] 补齐 golden fixtures 与 focused tests。
- [ ] 更新 stale / expired durable payload、docs / skill,最后跑验证矩阵。

### 当前状态

**目前在代码接线阶段** - 先读取当前 diff 和协议文件,再用小步补丁接入 `@selector-refind`。

## [2026-05-20 23:08:24] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: `@selector-refind` 协议接线和初版 fixture 已补

### 已完成

- [x] 检查当前 dirty diff,确认本轮只接续 P3 相关文件。
- [x] 接入 `ControlCommand::SelectorRefind` parser、`control_core` dispatch 和 default executor guard。
- [x] 修正 `refind.rs` 初版里的生命周期问题,并补强 blocked / recovery_recipe / 多候选不自动 rebound 的字段。
- [x] 升级 stale / expired durable payload,增加 `refind_available`、`refind_command`、`recovery_recipe` 和 refind 不代表动作完成的说明。
- [x] 新增 P3 `selector_refind_*` golden fixtures。

### 下一步行动

- [ ] 运行 `control_observation::refind::tests` 和 `control_protocol::tests::parse_should_support_selector_commands`。
- [ ] 根据编译/断言输出修复 contract 或 fixture 偏差。
- [ ] 再扩展到 P2 selector / durable / core focused tests。

### 当前状态

**目前在 focused verification 阶段** - 先让最窄测试告诉我们哪里还没闭合。

## [2026-05-20 23:09:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [错误记录]: `candidate_kind` 类型比较错误

### 现象

- `cargo test --package rustdog --bin rdog control_observation::refind::tests` 编译失败。
- Rust 报 `expected Option<&str>, found &str`。

### 处理

- 已把 `candidate_kind(&candidate) != expected_candidate_kind(selector)` 改成显式比较 `Some(expected_candidate_kind(selector))`。

### 当前状态

**继续 focused verification** - 立刻重跑同一条 refind 单测。

## [2026-05-20 23:18:46] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: P3 semantic re-find 已落地并通过 Ralph gate

### 已完成

- [x] 实现 `@selector-refind` request / parser / dispatch。
- [x] 抽出 selector candidate collection,保持 P2 `@selector-resolve` 行为不变。
- [x] 实现 `rdog.selector.score.v1` deterministic scoring、reason codes、confidence band、hard gate 和 stable tie-break。
- [x] 实现 rebound / needs_disambiguation / not_found / blocked response,以及 required `verify_hint` 和 verify-skip audit evidence contract。
- [x] 升级 stale / expired durable payload 的 refind hint。
- [x] 新增 fixtures / focused tests。
- [x] 更新 `rdog-control` skill 和 repo specs。
- [x] 运行验证矩阵。
- [x] 执行 deslop pass,修正 `observation:null` 不能算 fresh target 的边界。
- [x] post-deslop 回归通过。
- [x] Architect review APPROVE。

### 验证摘要

- `cargo fmt -- --check`: PASS。
- `control_observation::refind::tests`: 6 passed。
- `control_protocol::tests::parse_should_support_selector_commands`: PASS。
- P2 selector / durable / observation / core 回归: PASS。
- AX / window / screenshot / config 回归: PASS。
- `zenoh_router_client`: 23 passed,2 ignored。
- `quick_validate.py .codex/skills/rdog-control`: PASS。
- `git diff --check`: PASS。

### 当前状态

**完成** - P3 主契约已实现并验证。后续 scoring conformance 细化已记录到 `LATER_PLANS__observation_refmap_p3.md`。
