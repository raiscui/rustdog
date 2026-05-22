# 任务计划: observation refmap P5 mouse ref 化可落地计划

## 目标

根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建可执行的 P5 计划,让后续实现能把 mouse command 接入 observation ref / selector,并保持坐标为显式 fallback。

## [2026-05-21 19:48:04] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P5 ralplan 启动

### 阶段

- [ ] 阶段1: Pre-context intake,读取 roadmap、P4 记录和当前代码触点。
- [ ] 阶段2: 生成 `.omx/context/observation-refmap-p5-20260521T114804Z.md`。
- [ ] 阶段3: 输出 draft plan 到 `.omx/drafts/ralplan-rdog-observation-refmap-p5-draft.md`。
- [ ] 阶段4: 进行 Architect review。
- [ ] 阶段5: 进行 Critic review。
- [ ] 阶段6: 根据 review 修订并落地 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。
- [ ] 阶段7: 验证 Mermaid / diff check / 记录 WORKLOG / 写 complete state。

### 关键约束

- 本轮只规划,不修改 runtime Rust 源码。
- P5 不能把 mouse 变成默认主路径。语义 action 可用时仍优先语义 action。
- P5 不能混淆短期 `ref` 和永久 `selector`。
- `@click` / `@drag` / `@wheel` 坐标输入必须继续兼容,且应明确标记为 coordinate fallback。
- `src/control_mouse.rs` 当前超过 1000 行,计划必须包含结构减负边界,不能建议继续堆大文件。

### 当前状态

**目前在阶段1** - 正在收集 roadmap、P4 和 mouse/ref/selector 代码证据。

## [2026-05-21 19:48:04] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: context intake 完成

### 阶段进展

- [x] 阶段1: 已读取 roadmap、P4 记录、mouse coordinate spec、non-mouse semantic spec、mouse/observation/selector 代码触点。
- [x] 阶段2: 已生成 `.omx/context/observation-refmap-p5-20260521T114804Z.md`。
- [ ] 阶段3: 正在输出 draft plan。
- [ ] 阶段4: 待 Architect review。
- [ ] 阶段5: 待 Critic review。
- [ ] 阶段6: 待最终 plan。
- [ ] 阶段7: 待验证和收口。

### 当前状态

**目前在阶段3** - 开始起草 P5 可落地计划。

## [2026-05-21 19:55:30] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Architect review ITERATE

### Architect verdict

- Verdict: `ITERATE`。
- 主方案 Option A 方向正确。
- 必须收紧 selector gate,防止 selector target 直接把恢复和 mouse action 压进一条命令后变成隐藏主路径。

### 必须修改项

- selector 部分拆成:
  - `Phase 4A`: selector target 默认 no-action,只返回 `@selector-refind` recovery command。
  - `Phase 4B`: gated selector action 作为显式 opt-in,必须具备 typed refind decision、audit response 和 spy no-action tests。
- selector gate 必须包含 `policy`、`min_confidence`、`candidate_count`、`gate_decision`、`gate_reason`、`verify_hint`、`verify_result`、`performed`、request correlation 字段。
- parser 必须明确拒绝同一 target 内 `ref`、`selector_id`、`id` 混用。
- 验证计划必须加入 fake/spy MouseBackend no-action 断言,覆盖 stale ref、rect missing、ambiguous、blocked、not_found、low confidence。
- 如果 Phase 4B 无法满足审计和测试条件,实现必须停在 selector-refind handoff,不能半成品接 mouse action。

### 当前状态

**目前在阶段3/4** - 正在按 Architect 意见修订 draft,再进入 Critic review。

## [2026-05-21 20:05:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P5 ralplan 完成

### 阶段进展

- [x] 阶段1: Pre-context intake,读取 roadmap、P4 记录和当前代码触点。
- [x] 阶段2: 生成 `.omx/context/observation-refmap-p5-20260521T114804Z.md`。
- [x] 阶段3: 输出 draft plan 到 `.omx/drafts/ralplan-rdog-observation-refmap-p5-draft.md`。
- [x] 阶段4: 完成 Architect review,结论为 `ITERATE`,已采纳 selector gate 收紧要求。
- [x] 阶段5: 完成 Critic review,结论为 `APPROVE`。
- [x] 阶段6: 已落地 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。
- [x] 阶段7: 已验证 Mermaid / diff check,并记录 WORKLOG。

### 验证证据

- `beautiful-mermaid-rs --ascii` 验证最终计划中的 2 个 Mermaid block,结果均为 exit=0。
- `git diff --check` 验证最终计划与支线上下文文件,无空白错误。
- `rg` 复查最终计划已包含 `TARGET_RECT_UNAVAILABLE`、`performed:false`、spy backend no-action、`@hover` 不新增、selector gate stop rule。

### 当前状态

**已完成** - P5 可落地计划已经固化为 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`,本轮没有修改 Rust runtime 源码。

## [2026-05-21 20:17:45] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Ralph P5 实现启动

### 本轮目标

- 按 `.omx/plans/ralplan-rdog-observation-refmap-p5.md` 实现 P5。
- 先完成 `src/control_mouse.rs` 结构拆分,再接入 observation ref endpoint。
- selector 默认先做 no-action handoff;只有 typed decision、audit response 和 spy no-action tests 满足后才进入 gated action。

### 执行阶段

- [x] 阶段A: 刷新 Ralph state、读取代码和测试触点。
- [x] 阶段B: Phase 0 拆分 `control_mouse`。
- [x] 阶段C: Phase 1-3 接入 `target.ref + observation_id` endpoint。
- [x] 阶段D: Phase 4A/4B 接入 selector no-action / gated 边界。
- [x] 阶段E: 更新 docs / skill / conformance。
- [x] 阶段F: 完整验证、Architect 审核、deslop、回归复验和收口。

### 当前状态

**目前在阶段A** - 正在读取 `control_mouse`、`control_actions`、observation/AX/window/refind 触点,并准备 Ralph 并行调查。

## [2026-05-21 20:20:12] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Phase 0 baseline

### 已完成

- 已读取 `control_mouse.rs` 的 request / parser / plan / backend / tests 结构。
- 已跑拆分前 focused baseline: `cargo test --package rustdog --bin rdog control_mouse::tests`,结果 8 passed。

### 当前状态

**进入阶段B** - 准备把 `control_mouse.rs` 机械拆分成 `src/control_mouse/` 子模块,要求 public API 不变。

## [2026-05-21 20:25:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Phase 0 完成

### 已完成

- `src/control_mouse.rs` 已拆成:
  - `src/control_mouse/request.rs`
  - `src/control_mouse/parser.rs`
  - `src/control_mouse/plan.rs`
  - `src/control_mouse/report.rs`
  - `src/control_mouse/backend.rs`
  - `src/control_mouse/tests.rs`
- 根 `src/control_mouse.rs` 现在只保留 re-export 和测试模块声明。
- 文件行数:
  - root: 29 行。
  - 最大子文件 `parser.rs`: 427 行。

### 验证

- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::tests`: 8 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_mouse_requests -- --exact`: 通过。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_reject_invalid_mouse_payloads -- --exact`: 通过。

### 当前状态

**阶段B已完成,进入阶段C** - 准备接入 `target:{ref,observation_id}` endpoint,同时保持旧坐标 payload 兼容。

## [2026-05-21 20:34:46] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Phase C 编译断点验证

### 即将执行

- 先运行 `cargo fmt -- --check` 和 `cargo test --package rustdog --bin rdog control_mouse::tests`,确认交接中未验证的 request/parser 改动具体断在哪里。
- 再根据编译错误调整 `plan.rs`、`report.rs`、`tests.rs` 和 protocol parser tests,保持旧坐标 payload 兼容。

### 当前状态

**目前在阶段C** - 正在把 endpoint model 接回既有 mouse plan / report / tests,目标是先恢复 focused mouse 测试绿色。

## [2026-05-21 20:41:28] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Phase C 坐标兼容层恢复

### 已完成

- `ClickRequest` / `DragRequest` / `WheelRequest` / `MouseMoveRequest` 已接回旧坐标执行路径。
- 坐标 endpoint 会生成原有 Enigo plan。
- ref / selector endpoint 在 resolver 接入前会返回 `InvalidInput`,不会误执行 mouse backend。

### 验证

- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::tests`: 8 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_mouse_requests -- --exact`: 通过。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_reject_invalid_mouse_payloads -- --exact`: 通过。

### 当前状态

**继续阶段C** - 开始实现 observation ref -> current rect -> os-logical point resolver,并把执行层从 endpoint request 过渡到 resolved coordinate plan。

## [2026-05-21 21:04:18] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Phase C/D resolver 与 selector gate 初步落地

### 已完成

- observation store 新增 `resolve_observation_ref_with_header`,用 `ObservationHeader.scope` 区分 `ax` 与 `window`,避免只靠 `kind:"window"` 猜 backend。
- `control_ax` 新增 current rect helper,会重新抓当前 AX snapshot 后解析 target id。
- `control_window` 新增 current window rect helper,macOS 复用现有 `resolve_single_window` / `resolve_window_id_direct`。
- `control_mouse::target` 新增 prepare 层:
  - coordinate payload -> `coordinate_fallback` target_resolution。
  - `target.ref + observation_id` -> current rect -> anchor point -> coordinate plan。
  - selector `auto_refind:false` -> no-action handoff。
  - selector `auto_refind:true` -> typed `SelectorRefindDecision`,只有 rebound + fresh target + rect verify 才继续 mouse action。

### 验证

- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::target::tests`: 4 passed。
- `cargo test --package rustdog --bin rdog control_actions::tests::selector_mouse_target_without_auto_refind_should_return_no_action_before_backend -- --exact`: 通过。
- `cargo test --package rustdog --bin rdog control_observation::refind::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_mouse_ref_and_selector_targets -- --exact`: 通过。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_reject_invalid_mouse_payloads -- --exact`: 通过。

### 当前状态

**阶段C/D 核心代码已落地,进入阶段E** - 需要补文档 / skill / conformance 说明,再跑完整回归、Architect 审核和 deslop。

## [2026-05-21 21:30:07] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Architect ITERATE 修复项

### Architect verdict

- `ITERATE`。

### 必须修复

- `@drag` 的 selector endpoint 不能在 no-action 情况下返回 `Unsupported` error,必须和 click/wheel 一样返回结构化 `performed:false` no-action。
- `auto_refind:true` 的 selector 非 rebound / missing rect 也必须保持 no-action。
- `specs/control-line-protocol.md` 与 `README.md` 仍只描述坐标式 mouse,需要补 P5 target/ref/selector gate 和 `target_resolution.source:"coordinate_fallback"`。

### 当前状态

**回到阶段D/E 修复** - 先修 drag no-action 契约并补 focused tests,再同步 formal docs。

## [2026-05-21 21:41:30] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: Ralph P5 交接续跑

### 已确认

- 交接摘要显示 P5 核心实现、完整回归和 Architect 复审都已经通过。
- 当前剩余结构问题是 `src/control_mouse/target.rs` 约 816 行,超过子文件低于 800 行的健康线。
- 本轮只做机械拆分:把 `target.rs` 内联测试迁到 `src/control_mouse/target_tests.rs`,不改变运行时代码路径。

### 即将执行

- 在 `src/control_mouse.rs` 注册 `#[cfg(test)] mod target_tests;`。
- 从 `src/control_mouse/target.rs` 删除内联 `#[cfg(test)] mod tests`。
- 新建 `src/control_mouse/target_tests.rs`,保留原有测试断言语义。
- 拆分后重跑格式化、focused tests、完整 bin tests、integration tests、check、diff check。

### 当前状态

**阶段F 收尾中** - 正在处理 Architect APPROVE 后的结构健康线备注,目标是让所有 `src/control_mouse/*.rs` 子文件保持在 800 行以内。

## [2026-05-21 21:45:02] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: target 测试拆分完成

### 已完成

- 新增 `src/control_mouse/target_tests.rs`,承载原 `target.rs` 内联测试。
- `src/control_mouse.rs` 已注册 `#[cfg(test)] mod target_tests;`。
- `src/control_mouse/target.rs` 已删除内联测试模块,运行时代码未改语义。

### 验证

- `cargo fmt -- --check`: 通过。
- `wc -l src/control_mouse.rs src/control_mouse/*.rs`: 最大子文件为 `src/control_mouse/parser.rs` 669 行,`src/control_mouse/target.rs` 降为 595 行。

### 当前状态

**继续阶段F** - 准备运行 focused target tests 和完整回归。

## [2026-05-21 21:47:08] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: focused tests 通过

### 验证

- `cargo test --package rustdog --bin rdog control_mouse::target_tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_actions::tests::selector_mouse_target_without_auto_refind_should_return_no_action_before_backend -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_mouse_ref_and_selector_targets -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_reject_invalid_mouse_payloads -- --exact`: 1 passed。

### 当前状态

**继续阶段F** - focused mouse/ref/selector 边界已通过,准备跑完整 bin tests、integration tests、check 和 diff check。

## [2026-05-21 21:52:16] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: 完整回归通过

### 验证

- `cargo test --package rustdog --bin rdog`: 260 passed。
- `cargo test --package rustdog --test control_lanes`: 8 passed, 1 ignored。
- `cargo test --package rustdog --test control_mode`: 1 passed。
- `cargo check --package rustdog --bin rdog`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。
- `wc -l src/control_mouse.rs src/control_mouse/*.rs`: 所有 `control_mouse` 文件均低于 800 行。

### 当前状态

**继续阶段F** - 回归验证已通过,准备做 Ralph 的 architect/deslop 收尾门槛。

## [2026-05-21 21:57:42] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: scoped deslop 检查

### 检查范围

- `src/control_mouse*`
- `src/control_actions.rs`
- `src/control_observation.rs`
- `src/control_observation/refind.rs`
- `src/control_ax.rs`
- `src/control_window.rs`
- `src/control_window/macos.rs`
- `src/control_protocol.rs`
- P5 相关 README / specs / skill 文档。

### 发现

- fallback-like 关键词命中主要是协议要求的显式 `coordinate_fallback` / mouse fallback lane / Zenoh entry-point fallback,不是掩盖错误的静默 fallback。
- `src/control_window/macos.rs` 的 activate fallback 是已有窗口激活降级路径,本轮未改,且保留 primary/fallback 两段错误证据。
- `src/control_mouse/target.rs` 里 `prepare_click_request` 的坐标分支先校验 `x/y`,又用 `expect` 重新取值,可以清理为一个局部 `point`,减少重复状态读取。

### 即将执行

- 清理 `prepare_click_request` 的坐标分支,保持行为不变。
- 清理后重跑 focused test 和完整回归。

### 当前状态

**阶段F deslop 中** - 正在做 scoped 小清理,不扩大到非 P5 改动。

## [2026-05-21 22:05:19] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: post-deslop 回归通过

### 已完成

- `src/control_mouse/target.rs` 坐标点击分支改为复用单一 `point` 局部值,移除重复读取和 `expect`。
- 既有 Ralph Architect `APPROVE` 的唯一备注是 `target.rs` 816 行;本轮已通过测试拆分降到 592 行。
- 新开 architect 代理失败,原因是当前线程子智能体数量已达上限;已读取已有 Ralph 子智能体状态,其中最终复审 verdict 为 `APPROVE`。

### post-deslop 验证

- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::target_tests`: 6 passed。
- `cargo test --package rustdog --bin rdog`: 260 passed。
- `cargo test --package rustdog --test control_lanes`: 8 passed, 1 ignored。
- `cargo test --package rustdog --test control_mode`: 1 passed。
- `cargo check --package rustdog --bin rdog`: 通过。
- `git diff --check`: 通过。
- `wc -l src/control_mouse.rs src/control_mouse/*.rs`: 最大子文件为 `parser.rs` 669 行,`target.rs` 592 行。

### 当前状态

**阶段F 验证完成** - 准备写 WORKLOG / ERRORFIX,更新 Ralph state 并交付。

## [2026-05-21 22:09:18] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: P5 完成

### 已完成

- 阶段A-F 均已完成并标记。
- `WORKLOG__observation_refmap_p5.md` 已追加本轮交付记录。
- `ERRORFIX__observation_refmap_p5.md` 已记录 drag selector no-action 修复。

### 最终验证

- pre-deslop 与 post-deslop 都已跑完整验证面。
- Architect 复审已 `APPROVE`,本轮修复了其结构备注。

### 当前状态

**P5 已完成** - 准备更新 Ralph state 并交付最终结果。

## [2026-05-21 22:12:02] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] [记录类型]: Ralph state 清理完成

### 已完成

- `omx state write` 已写入 completion audit。
- `omx state clear --input '{"mode":"ralph","session_id":"019e38be-b9d9-76f0-aabc-fad94a2bcf12"}' --json` 已清理 Ralph state。
- `omx state read` 返回 `{"exists":false,"mode":"ralph"}`。
- 已关闭本轮遗留子智能体,避免继续占用线程 agent 上限。

### 当前状态

**已交付前检查完成** - 最后确认 diff check 通过,准备回复用户。
