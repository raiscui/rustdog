# 任务计划: observation refmap P0 可落地计划

## [2026-05-19 08:55:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: ralplan P0 实施计划

### 目标
基于 `specs/rdog-observation-scoped-refmap-plan.md` 产出一份可执行的 P0 plan,范围只覆盖 observation-scoped refmap,并明确 P1 仅在 P0 落地后再细化。

### 阶段
- [x] 阶段1: 清理 OMX workflow 冲突并完成 pre-context intake
- [x] 阶段2: 收集 brownfield 代码事实和约束
- [x] 阶段3: 草拟 P0 可落地计划
- [x] 阶段4: Architect / Critic 评审并修订
- [x] 阶段5: 落盘 `.omx/plans/` 最终计划并交付

### 关键问题
1. P0 需要改哪些协议和模块,才能不破坏现有 `@ax-*` / `@screenshot` 行为?
2. `ObservationStore` 第一版应该放在哪个层级,避免提前进入 P1 durable state?
3. P1 应如何被明确推迟,但不被遗忘?

### 状态
**目前在阶段1** - 已清理 active `ralph` workflow state,准备创建 `.omx/context` 快照并读取实现边界。

## [2026-05-19 09:00:25] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: pre-context intake 与 brownfield 阅读完成

### 已完成
- [x] 阶段1: 清理 OMX workflow 冲突并完成 pre-context intake
- [x] 阶段2: 收集 brownfield 代码事实和约束

### 当前证据
- `specs/rdog-observation-scoped-refmap-plan.md` 明确 P0 是 observation 内 `@eN`、stale ref 和 observation header, P1 以后才做 durable state / selector / re-find / `@observe` / mouse ref 化。
- `src/control_ax.rs` 的 `AxTarget`、`AxSnapshot`、`AxWindow`、`AxElement` 当前只有 `id` / semantic locator,没有 `ref` 或 `observation_id`。
- `src/control_ax/query.rs` 的 `@ax-find` / `@ax-get` 响应没有 observation header,`@ax-get` 仍通过 `resolve_target_id_in_snapshot()` 解析目标。
- `src/screenshot.rs` 已经能把 `AxSnapshot` 放进 screenshot manifest 的 `accessibility` 字段,所以 P0 应该把 screenshot include_ax 作为 staged P0b,避免 AX-only 和 screenshot manifest 语义分裂。
- `ControlPeerSession` 当前只负责 frame ordering、dispatch report 和 lifecycle gate,不直接拥有 UI state。P0 的 ObservationStore 应该是 control/observation 层进程内 store,不要塞进 transport 私有实现。

### 下一步
- [x] 阶段3: 草拟 P0 可落地计划,先写 planner draft。
- [x] 阶段4: 使用 Architect / Critic 评审 draft,只修计划,不改实现。
- [x] 阶段5: 落盘 `.omx/plans/` 最终计划并交付。

### 当前状态
**目前在阶段3** - 开始把证据收束成 `.omx/plans/` 可执行计划。

## [2026-05-19 09:06:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: planner draft 已完成,外部评审通道不可用

### 已完成
- [x] 阶段3: 草拟 P0 可落地计划

### 评审尝试
- `omx ask claude --agent-prompt architect` 失败: 本地 prompt catalog 没有 `architect` role。
- `omx ask claude --prompt ...` 失败: provider 返回 402 insufficient balance,artifact 已写入 `.omx/artifacts/claude-rustdog-omx-drafts-ralplan-rdog-observation-refmap-p0-planne-2026-05-19T01-03-38-816Z.md`。
- `omx ask gemini --prompt ...` 运行 120 秒超时,无评审内容返回。

### 当前决定
- 继续执行本地 Architect / Critic 检查,并在最终计划 changelog 中明确外部评审不可用。
- 计划交付物仍需包含 RALPLAN-DR、ADR、验收、验证、handoff 和 P1 deferred scope。

### 当前状态
**目前在阶段4** - 进入本地评审和最终计划修订。

## [2026-05-19 09:11:02] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: P0 可落地计划已交付

### 已完成
- [x] 阶段1: 清理 OMX workflow 冲突并完成 pre-context intake
- [x] 阶段2: 收集 brownfield 代码事实和约束
- [x] 阶段3: 草拟 P0 可落地计划
- [x] 阶段4: Architect / Critic 评审并修订
- [x] 阶段5: 落盘 `.omx/plans/` 最终计划并交付

### 交付文件
- `.omx/plans/ralplan-rdog-observation-refmap-p0.md`
- `.omx/drafts/ralplan-rdog-observation-refmap-p0-planner-draft.md`
- `notes__observation_refmap_plan.md`
- `LATER_PLANS__observation_refmap_plan.md`
- `WORKLOG__observation_refmap_plan.md`

### 验证
- `git diff --check -- .omx/plans/ralplan-rdog-observation-refmap-p0.md .omx/drafts/ralplan-rdog-observation-refmap-p0-planner-draft.md task_plan__observation_refmap_plan.md notes__observation_refmap_plan.md`: 通过。
- `rg -n '```mermaid|Phase 0 到 Phase 5|RALPLAN Draft|TODO|TBD' ...`: 最终 plan 无命中; draft 仍保留 Draft 标题是预期。
- `wc -l`: 支线上下文和 plan 文件均未超过 1000 行。

### 当前状态
**本轮规划任务已完成** - P1 durable observation state 已记录为 P0 落地后的后续细化项。

## [2026-05-19 09:12:12] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [最终验证]: 支线文件检查完成

### 验证结果
- `git diff --check -- task_plan__observation_refmap_plan.md notes__observation_refmap_plan.md LATER_PLANS__observation_refmap_plan.md WORKLOG__observation_refmap_plan.md .omx/plans/ralplan-rdog-observation-refmap-p0.md .omx/drafts/ralplan-rdog-observation-refmap-p0-planner-draft.md`: 通过。
- `wc -l`: 支线 context 和 plan/draft 均未超过 1000 行。
- `.omx/plans/ralplan-rdog-observation-refmap-p0.md` 不含 Mermaid 代码块,无需 `beautiful-mermaid-rs`。

### 当前状态
**完成** - 可以把 `.omx/plans/ralplan-rdog-observation-refmap-p0.md` 交给 `$ralph` 执行 P0,并在 P0 验证后再启动 P1 细化方案。

## [2026-05-19 09:43:57] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [执行]: Ralph 开始落地 P0

### 行动目的
- 用户已用 `$ralph .omx/plans/ralplan-rdog-observation-refmap-p0.md` 要求执行 P0。
- 本轮目标从计划交付切换为代码、测试和文档落地。

### 将要做什么
- [x] 新增 `src/control_observation.rs`,实现进程内 TTL-bound observation/ref store 和结构化 stale/expired 错误。
- [x] 扩展 AX snapshot / window / element 响应,给 `@ax-tree` / `@ax-find` / `@ax-get` 输出 observation header 和 `ref`。
- [x] 扩展 `AxTarget` parser / resolver,让 AX action-family command 支持 `target:{ref,observation_id}`。
- [x] 接入 `@screenshot include_ax` 的 accessibility manifest。
- [x] 接入 `@window-find` observation header 和 window ref,视复杂度处理 `@window-activate` / `@window-close` ref follow-on。
- [x] 更新 skill/docs,跑 focused tests 和格式检查。

### 当前状态
**目前在实现阶段** - 先写 `control_observation` 底座,再接 AX 路径。

## [2026-05-19 10:14:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [继续执行]: Ralph P0 从中断点恢复

### 行动目的
- 用户说"继续",当前 Ralph state 仍是 active executing。
- 本轮从已写到一半的 `control_observation` / AX ref 实现继续,不重做规划。

### 将要做什么
- [x] 先跑 focused compile,用真实编译错误定位剩余结构体字段、导入和 lifetime 问题。
- [x] 补齐 AX tree/find/get/screenshot/window 的 observation/ref 输出和 ref target 输入。
- [x] 更新 docs/skill,再跑 focused tests、格式检查和 diff 检查。

### 当前状态
**目前在实现阶段** - 先用编译器和现有测试把半成品实现拉回可验证状态。

## [2026-05-19 11:55:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: P0 实现与 focused regression 已完成

### 已完成
- [x] 新增 `src/control_observation.rs`,实现进程内 TTL-bound observation/ref store 和结构化 stale/expired 错误。
- [x] 扩展 AX snapshot / window / element 响应,让 `@ax-tree` / `@ax-find` / `@ax-get` 输出 observation header 和 `ref`。
- [x] 扩展 `AxTarget` parser / resolver,让 AX action-family command 支持 `target:{ref,observation_id}`。
- [x] 接入 `@screenshot include_ax` 的 accessibility manifest。
- [x] 接入 `@window-find` observation header 和 window ref,并支持 `@window-activate` / `@window-close` 的 ref follow-on。
- [x] 更新 `rdog-control` skill 与 `specs/code-agent-rdog-control-usage.md` 的 observation ref 使用规则。

### 验证结果
- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_observation::tests`: 4 passed。
- `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_actions::tests`: 18 passed。
- `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_window::macos::tests`: 7 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests`: 14 passed。
- `git diff --check`: 通过。

### 当前状态
**目前在收尾阶段** - 还需要记录 worklog,执行 Ralph 要求的 deslop/复验/审查收口。

## [2026-05-19 12:03:22] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成收口]: P0 收尾记录已补齐

### 已完成
- [x] 补写 `WORKLOG__observation_refmap_plan.md`,把 P0 实现结果和收尾验证记录落盘。
- [x] 复核 `LATER_PLANS__observation_refmap_plan.md`,确认 P1 仍然只作为后续细化入口。
- [x] 维持 P0 边界不扩展到 durable selector / restart recovery / semantic re-find。

### 当前状态
**完成** - P0 observation-scoped refmap 线已收口,后续只在 P1 细化时再往 durable 方向推进。

## [2026-05-19 12:06:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [复核记录]: fresh verification 收口前复核

### 行动目的
- 当前 Ralph state 仍显示 `executing`,需要补一轮新的验证证据,不能只依赖上一轮已经通过的结果。
- 先确认 P0 实现仍然可编译、可测试,再决定是否可以真正停下。

### 将要做什么
- [x] 重新跑与 observation refmap 相关的 focused tests。
- [x] 重新跑格式和 diff 检查,确认当前 worktree 没有引入新问题。
- [x] 将新验证结果补写到 worklog,再判断是否可以结束 Ralph。

### 当前状态
**目前在复核阶段** - 先补 fresh verification evidence,不提前宣告结束。

## [2026-05-19 12:12:53] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成复核]: fresh verification 已补齐

### 已完成
- [x] 重新跑 `cargo fmt -- --check`。
- [x] 重新跑 `cargo test --package rustdog --bin rdog control_observation::tests`。
- [x] 重新跑 `cargo test --package rustdog --test zenoh_router_client`。
- [x] 修掉 `ObservationStore::with_limits` 的整合测试 dead_code warning。
- [x] 重新验证 warning 已消失,测试结果保持通过。

### 当前状态
**完成** - 这轮 fresh verification evidence 已补齐,可以停止本轮执行。

## [2026-05-19 12:17:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [收尾检查]: deslop / audit / Ralph state cleanup

### 行动目的
- stop hook 仍看到 Ralph active,说明 `.omx` lifecycle 还没有真正收干净。
- 在清理 state 前,按 Ralph 要求补齐 deslop 检查、计划 TODO 清零、completion audit 和 fresh verification evidence。

### 将要做什么
- [x] 清理 `task_plan__observation_refmap_plan.md` 中已经完成但仍未勾掉的历史复选框。
- [x] 运行 `cargo build --package rustdog --bin rdog`,补充 fresh build evidence。
- [x] 对 P0 相关文件做 bounded deslop / fallback-like 检查。
- [x] 复验 `git diff --check` 和计划 TODO 清零。
- [x] 写入 Ralph completion audit,再清理 active state。

### 当前状态
**完成** - P0 实现、post-deslop regression、completion audit 和 Ralph state cleanup 已进入最终关闭步骤。

## [2026-05-19 23:50:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: 启用 P1 durable observation state 细化上下文

### 启用原因
- 用户要求进入 P1 durable observation state / selector 细化方案。
- P1 已经不是 P0 实现收尾,需要独立记录 durable store、selector schema、restart recovery 和验证矩阵。

### 新上下文集
- `task_plan__observation_refmap_p1.md`
- `notes__observation_refmap_p1.md`
- `WORKLOG__observation_refmap_p1.md`
- `LATER_PLANS__observation_refmap_p1.md`

### 当前状态
**P0 支线保持完成** - 后续 P1 方案只写入 `__observation_refmap_p1` 后缀文件。
