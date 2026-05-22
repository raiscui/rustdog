# 任务计划: observation-scoped refmap P4 `@observe` 可落地计划

## [2026-05-21 07:23:35] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 启动 P4 Ralplan 规划

### 目标

根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P4 可执行计划,输出到 `.omx/plans/ralplan-rdog-observation-refmap-p4.md`。

### 范围边界

- 本轮只做规划,不修改 Rust 实现代码。
- P4 重点预计是新增统一 `@observe` surface,把 screenshot / AX / window observation 组织成一个 agent-friendly bundle。
- P4 必须承接 P0/P1/P2/P3 已落地的 observation header、durable selector、`@selector-resolve` 和 `@selector-refind` 契约。
- 不做 P5 mouse command 全 ref 化,不把 action by selector 混进 observation,不让旧短 ref 跨 daemon 重启复活。

### 阶段

- [x] 阶段1: 启动 ralplan,登记支线上下文和主计划索引。
- [ ] 阶段2: 回读 roadmap、P3 plan/落地证据、当前代码触点和风险记录。
- [ ] 阶段3: 提炼 P4 方案选项、ADR、scope 边界、验收标准和验证矩阵。
- [ ] 阶段4: 创建 context snapshot 与 P4 plan 草案。
- [ ] 阶段5: Architect review -> Critic review -> 必要时迭代。
- [ ] 阶段6: 最终计划落盘、验证格式、记录 worklog、更新 Ralplan state。

### 当前状态

**目前在阶段2** - 正在收集 P4 需要继承的 observation / selector / refind 事实。

## [2026-05-21 07:27:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P4 brownfield 收集动作

### 已收集事实

- `$ralplan` 需要先生成 `.omx/context/` 快照,再输出 `.omx/plans/` 计划,并按 Planner -> Architect -> Critic 顺序审查。
- roadmap 中 P4 的定位是新增 `@observe` 统一入口,但不能过早替换 `@screenshot` / `@ax-tree` / `@window-find`。
- P3 已落地 `@selector-refind`,后续 P4 要复用其 scoring / blocked / fresh_target / verify_hint 语义。
- 当前 `src/control_observation.rs` 已 1271 行,超过项目健康线,因此 P4 计划必须把结构拆分列为执行前置。

### 下一步动作

- 读取 P4 相关代码片段的 line-level 证据。
- 写入 `notes__observation_refmap_p4.md`。
- 创建 context snapshot 和 P4 plan draft。

## [2026-05-21 07:34:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P4 draft 与基础验证完成

### 阶段状态

- [x] 阶段2: 已回读 roadmap、P3 落地证据、当前代码触点和结构风险。
- [x] 阶段3: 已提炼 P4 方案选项、ADR、scope 边界、验收标准和验证矩阵。
- [x] 阶段4: 已创建 context snapshot 与 P4 plan draft。
- [ ] 阶段5: Architect review -> Critic review -> 必要时迭代。
- [ ] 阶段6: 最终计划落盘、验证格式、记录 worklog、更新 Ralplan state。

### 已落盘文件

- `notes__observation_refmap_p4.md`
- `.omx/context/observation-refmap-p4-20260520T232723Z.md`
- `.omx/drafts/ralplan-rdog-observation-refmap-p4-draft.md`

### 已验证

- `beautiful-mermaid-rs --ascii` 已验证 draft 中 flowchart。
- `beautiful-mermaid-rs --ascii` 已验证 draft 中 sequenceDiagram。
- `git diff --check -- .omx/drafts/ralplan-rdog-observation-refmap-p4-draft.md .omx/context/observation-refmap-p4-20260520T232723Z.md notes__observation_refmap_p4.md task_plan__observation_refmap_p4.md task_plan.md` 通过。

### 当前状态

**目前在阶段5** - 正在等待 Architect review,完成后再进入 Critic review。

## [2026-05-21 07:35:01] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Architect review 要求迭代

### Review 结论

- Architect verdict: `ITERATE`。
- 主要问题: draft 中 `@observe hybrid` 的 observation ownership 不够硬,`record merged observation metadata` 和 section-level observation 的说法冲突。
- 次要问题: 结构减负需要覆盖 `src/screenshot.rs`、`src/control_ax.rs`、`src/control_window.rs`; `target` 字段需要明确只过滤 window / AX summary,visual 首版仍是 desktop screenshot。

### 下一步动作

- 修改 draft,取消 "merged observation" 表述。
- 写死 hybrid 不创建合并 observation,top-level observation 选择主 observation,所有 ref sample 必须带 `observation_id`。
- 加入已超线文件不追加实质逻辑的验收条件。
- 明确 target 首版不裁剪 visual screenshot。

## [2026-05-21 07:40:22] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Critic review 通过

### Review 结论

- Critic verdict: `APPROVE`。
- Blocking Issues: None。
- Required Changes: None。

### 已采纳的非阻断建议

- 明确 `ax_required` 作为正式 request 字段。
- 明确 `refs.sample` 可跨 section,但每项必须携带 `observation_id` 和 `section`。
- 把 "行数不明显增长" 升级为已超 1000 行文件只能接线,实质逻辑必须进子模块。

### 下一步动作

- 将 draft 提升为 `.omx/plans/ralplan-rdog-observation-refmap-p4.md`。
- 重新运行 Mermaid / diff / 文本收口检查。
- 写入 `WORKLOG__observation_refmap_p4.md`。

## [2026-05-21 07:44:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P4 Ralplan 完成

### 阶段状态

- [x] 阶段1: 启动 ralplan,登记支线上下文和主计划索引。
- [x] 阶段2: 回读 roadmap、P3 plan/落地证据、当前代码触点和风险记录。
- [x] 阶段3: 提炼 P4 方案选项、ADR、scope 边界、验收标准和验证矩阵。
- [x] 阶段4: 创建 context snapshot 与 P4 plan 草案。
- [x] 阶段5: Architect review -> Critic review -> 必要时迭代。
- [x] 阶段6: 最终计划落盘、验证格式、记录 worklog、更新 Ralplan state。

### 最终产物

- `.omx/plans/ralplan-rdog-observation-refmap-p4.md`
- `.omx/context/observation-refmap-p4-20260520T232723Z.md`
- `.omx/drafts/ralplan-rdog-observation-refmap-p4-draft.md`
- `notes__observation_refmap_p4.md`
- `WORKLOG__observation_refmap_p4.md`

### 验证结果

- Mermaid flowchart: PASS。
- Mermaid sequenceDiagram: PASS。
- 收口词检查: PASS。
- `git diff --check`: PASS。

### 当前状态

**全部阶段完成** - P4 可落地计划已通过 Architect/Critic 共识并落盘。

## [2026-05-21 07:45:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 启动 P4 Ralph 实现

### 目标

按 `.omx/plans/ralplan-rdog-observation-refmap-p4.md` 落地 `@observe` 只读 facade,并通过测试、deslop 和 architect verification 收口。

### 执行阶段

- [ ] 阶段7: 复核代码现状和测试模式。
- [ ] 阶段8: 实现协议解析、observe 模块、core dispatch、side-effect guard。
- [ ] 阶段9: 复用 screenshot savefile producer,生成 visual summary。
- [ ] 阶段10: 更新 docs / skill。
- [ ] 阶段11: 运行 focused tests / fmt / diff check。
- [ ] 阶段12: Architect verification。
- [ ] 阶段13: ai-slop-cleaner changed-files pass 与回归验证。
- [ ] 阶段14: WORKLOG / state / final 交付。

### 当前约束

- 当前 worktree 已有大量前序改动,本轮只处理 P4 计划相关文件。
- 不创建 merged observation。
- 不做 P5 mouse ref 化。
- 已超 1000 行文件只做必要接线,实质逻辑进子模块。

### 当前状态

**目前在阶段7** - 正在复核现有 parser、core、screenshot、AX/window 和 docs 测试模式。

## [2026-05-21 08:11:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 继续 P4 Ralph 实现接线

### 当前继承状态

- 已有 `src/control_observation/observe.rs` 初版,但还未完成 protocol / core / screenshot / docs 接线。
- 文档侧 explorer 已返回 skill 与 `specs/code-agent-rdog-control-usage.md` 的具体插入点。
- 代码侧 explorer 仍在运行,本地先继续检查新增模块和相邻接口,避免阻塞实现。

### 下一步动作

- 复核 `observe.rs` 剩余实现和潜在编译问题。
- 对 `src/control_observation.rs`、`src/control_protocol.rs`、`src/control_core.rs`、`src/control_actions.rs`、`src/screenshot.rs` 做必要接线。
- 补最小协议解析与 observe 模块测试。

### 当前状态

**目前在阶段8/9** - 正在把 `@observe` 从模块草稿接入可执行控制面。

## [2026-05-21 08:24:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P4 代码接线初版完成

### 阶段进展

- [x] 阶段8: 已接入 `ControlCommand::Observe`、parser、core direct dispatch、action executor guard。
- [x] 阶段9: 已把 composite screenshot 拆出可复用 producer,`@observe` visual path 可直接拿到 `@savefile` frames 和 filename summary。
- [ ] 阶段10: 更新 docs / skill。
- [ ] 阶段11: 运行 focused tests / fmt / diff check。
- [ ] 阶段12: Architect verification。
- [ ] 阶段13: ai-slop-cleaner changed-files pass 与回归验证。
- [ ] 阶段14: WORKLOG / state / final 交付。

### 当前验证计划

- 先运行 `cargo fmt` 和 focused tests,处理所有编译错误和 warning。
- 再更新文档和 skill,避免文档先承诺代码尚未通过的 surface。

### 当前状态

**目前在阶段11** - 正在进行格式化和 focused 编译测试。

## [2026-05-21 08:28:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 首轮 focused tests 暴露编译错误

### 现象

- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture` 失败。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_observe_command -- --exact --nocapture` 失败。

### 已验证结论

- 编译器报 `parse_ax_depth` / `parse_ax_max_elements` 是 private function。
- 这是 P4 `observe.rs` 复用 AX parser 时的可见性接线问题,不是业务逻辑失败。

### 修复动作

- 将这两个 parser 从私有提升为 `pub(crate)`,保持单一解析逻辑。
- 修复后重新运行同一组 focused tests。

## [2026-05-21 08:39:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: focused tests 通过并完成文档同步

### 阶段进展

- [x] 阶段10: 已更新 `rdog-control` skill、protocol/workflow/zenoh-hardware reference,以及 `specs/code-agent-rdog-control-usage.md`。
- [x] 阶段11: 第一组 focused tests 通过。
- [ ] 阶段12: Architect verification。
- [ ] 阶段13: ai-slop-cleaner changed-files pass 与回归验证。
- [ ] 阶段14: WORKLOG / state / final 交付。

### 已通过验证

- `cargo fmt`
- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_observe_command -- --exact --nocapture`
- `cargo test --package rustdog --bin rdog control_core::tests::explicit_request_should_render_observe_without_action_executor -- --exact --nocapture`
- `cargo test --package rustdog --bin rdog screenshot::tests -- --nocapture`
- `cargo test --package rustdog --bin rdog control_protocol::tests -- --nocapture`
- `cargo test --package rustdog --bin rdog control_core::tests -- --nocapture`

### 当前状态

**目前在阶段11/12** - 正在补全更宽的编译检查、diff check,然后进入 Architect verification。

## [2026-05-21 08:57:00] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Architect 通过并进入 deslop

### Architect 结论

- Verdict: `APPROVED`。
- 关键确认: `@observe` 是只读 facade,parser/core/action guard/文档边界符合 P4。
- 后续建议: P5 前继续拆分 `observe.rs`,避免逼近 1000 行。

### Deslop inventory

- Fallback-like findings:
  - 文档中的 mouse fallback / entry-point fallback 属于明确工作流边界,不是 masking fallback。
  - `screenshot.rs` 的 `sck-rs -> xcap` 属于既有平台 backend fallback,有现有 screenshot tests 覆盖,本轮不改。
  - `observe.rs` 中 visual observation 记录使用 `.ok().flatten()` 会静默吞掉 observation store 失败,这是本轮需要修复的 masking fallback slop。

### 修复计划

- 将 visual-only primary observation 记录失败改为显式传播。
- 增加单测验证 visual-only observe 会产生 visual primary observation。
- 重新跑 observe / protocol / core / screenshot / full bin tests 与质量门。

## [2026-05-21 08:48:19] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: P4 Ralph 实现完成

### 阶段状态

- [x] 阶段7: 复核代码现状和测试模式。
- [x] 阶段8: 实现协议解析、observe 模块、core dispatch、side-effect guard。
- [x] 阶段9: 复用 screenshot savefile producer,生成 visual summary。
- [x] 阶段10: 更新 docs / skill。
- [x] 阶段11: 运行 focused tests / fmt / diff check。
- [x] 阶段12: Architect verification。
- [x] 阶段13: ai-slop-cleaner changed-files pass 与回归验证。
- [x] 阶段14: WORKLOG / state / final 交付。

### 完成结果

- `@observe` 已作为只读 observation facade 落地。
- visual screenshot 继续通过 `@savefile` frames 返回 image / manifest,final response 只包含 filename summary。
- hybrid 没有 merged observation namespace,ref sample 带 `section + observation_id + ref`。
- 文档和 skill 已写清 `@observe` 是推荐观察入口,旧 observation commands 仍是 stable lower-level lanes。

### 最终验证

- `cargo fmt -- --check`: PASS。
- `cargo check --package rustdog --bin rdog`: PASS,无 warning。
- `git diff --check`: PASS。
- `cargo test --package rustdog --bin rdog -- --nocapture`: PASS,252 passed。
- `cargo test --package rustdog --test control_lanes -- --nocapture`: PASS,8 passed,1 ignored(真实截图权限依赖既有 ignored 用例)。
- `cargo test --package rustdog --test control_mode -- --nocapture`: PASS,1 passed。

### 当前状态

**全部 P4 Ralph 阶段完成** - 准备写入 Ralph state complete 并交付。

## [2026-05-21 17:28:15] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 执行 P5 前 observe.rs 分层减负

### 任务目标

- 根据 `LATER_PLANS__observation_refmap_p4.md` 中的后续建议,先把 `src/control_observation/observe.rs` 拆成 request / producer / response / refs 四层。
- 保持 `@observe` 协议响应、`@savefile` frame 行为、ref sample section scope、primary observation 选择语义不变。
- 本轮只做结构减负,不进入 P5 mouse command ref 化,不新增 selector action 能力。

### 阶段计划

- [ ] 阶段15: 完整阅读 `observe.rs` 与 `observe_tests.rs`,确认拆分边界。
- [ ] 阶段16: 新增 `observe/request.rs`、`observe/producer.rs`、`observe/response.rs`、`observe/refs.rs`,并让 `observe.rs` 只保留公开入口和测试接线。
- [ ] 阶段17: 调整模块可见性与单测引用,保持现有测试覆盖的内部契约可验证。
- [ ] 阶段18: 运行 focused tests、`cargo fmt`、`cargo check`、`git diff --check`。
- [ ] 阶段19: 更新 `WORKLOG__observation_refmap_p4.md` 和 `LATER_PLANS__observation_refmap_p4.md`,交付本轮拆分结果。

### 当前状态

**目前在阶段15** - 正在读取当前 `observe.rs` 的全部代码结构,下一步开始按四层边界移动代码。

## [2026-05-21 17:34:04] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: observe.rs 四层拆分完成第一轮 focused test

### 阶段进展

- [x] 阶段15: 已完整阅读 `observe.rs` 与 `observe_tests.rs`,确认 request / producer / response / refs 的依赖方向。
- [x] 阶段16: 已新增四个 `observe/` 子模块,根 `observe.rs` 只保留模块装配、公开入口和测试接线。
- [x] 阶段17: 已把测试所需内部项改成测试期私有导入,没有把 producer / response 内部类型扩成新的公开 API。
- [ ] 阶段18: 正在补 protocol/core focused tests、编译检查和 diff check。
- [ ] 阶段19: 待更新支线 WORKLOG / LATER 记录。

### 已遇到并处理的编译错误

- 第一轮 observe focused test 在 Rust re-export 可见性检查失败:
  - `select_primary_observation`、`ProducedSections`、`render_observe_response` 不能从子模块以更宽可见性重导出。
- 处理方式:
  - 改为 `#[cfg(test)]` 私有导入给 `observe_tests.rs` 使用。
  - 保持生产态公开面仍只有 `build_observe_outcome`、`parse_observe_payload` 和 request 类型。

### 已通过验证

- `cargo fmt`
- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`: PASS,4 passed。

### 当前状态

**目前在阶段18** - 正在验证 parser 和 core dispatch 是否在拆分后仍保持原协议行为。

## [2026-05-21 17:37:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: observe.rs 分层减负完成

### 阶段进展

- [x] 阶段15: 完整阅读 `observe.rs` 与 `observe_tests.rs`,确认拆分边界。
- [x] 阶段16: 新增 `observe/request.rs`、`observe/producer.rs`、`observe/response.rs`、`observe/refs.rs`。
- [x] 阶段17: 调整模块可见性与单测引用,保持生产公开面收窄。
- [x] 阶段18: 运行 focused tests、`cargo fmt -- --check`、`cargo check`、完整 bin tests、`git diff --check`。
- [x] 阶段19: 更新支线 WORKLOG / LATER / ERRORFIX 记录。

### 结构结果

- `src/control_observation/observe.rs`: 46 行,只保留模块装配、公开入口和测试接线。
- `src/control_observation/observe/request.rs`: 379 行,承载 `ObserveMode` / `ObserveTarget` / `ObserveRequest` / parser。
- `src/control_observation/observe/producer.rs`: 199 行,承载 visual / AX / window section production。
- `src/control_observation/observe/response.rs`: 167 行,承载 response render、status 和 primary source。
- `src/control_observation/observe/refs.rs`: 134 行,承载 refs sample 与 selector count。

### 已通过验证

- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`: PASS,4 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_observe_command -- --exact --nocapture`: PASS,1 passed。
- `cargo test --package rustdog --bin rdog control_core::tests::explicit_request_should_render_observe_without_action_executor -- --exact --nocapture`: PASS,1 passed。
- `cargo fmt -- --check`: PASS。
- `cargo check --package rustdog --bin rdog`: PASS,无 warning。
- `cargo test --package rustdog --bin rdog -- --nocapture`: PASS,252 passed。
- `git diff --check`: PASS。

### 当前状态

**阶段15-19 全部完成** - `observe.rs` 已完成 request / producer / response / refs 分层,可继续进入 P5 mouse ref 化前的下一项准备。

## [2026-05-21 18:35:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Hook 要求 Ralph active 收尾复验

### 触发来源

- stop hook 提示 OMX Ralph 仍处于 active,`current_phase:"starting"`。
- Ralph state 路径: `.omx/state/sessions/019e38be-b9d9-76f0-aabc-fad94a2bcf12/ralph-state.json`。

### 补充计划

- [ ] 阶段20: 读取 Ralph state 与 goal mode 状态,确认未完成的是 workflow state 而不是代码任务。
- [ ] 阶段21: 重新运行 fresh verification evidence,至少覆盖 observe focused tests、protocol/core focused tests、fmt check、cargo check、完整 bin tests、diff check。
- [ ] 阶段22: 把 fresh verification 写回支线记录,并将 Ralph state 写为 complete。

### 当前状态

**目前在阶段20/21** - 已确认 goal mode 当前无 active goal,正在重新跑本轮 fresh verification。

## [2026-05-21 18:38:19] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: Ralph hook 复验与 state 收口完成

### 阶段进展

- [x] 阶段20: 已读取 Ralph state,确认此前仍是 `active:true,current_phase:"starting"`; goal mode 当前无 active goal。
- [x] 阶段21: 已重新运行 fresh verification evidence。
- [x] 阶段22: 已通过 `omx state write` 写入 Ralph completion audit,并读回确认 `active:false,current_phase:"complete"`。

### Fresh verification evidence

- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`: PASS,4 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_observe_command -- --exact --nocapture`: PASS,1 passed。
- `cargo test --package rustdog --bin rdog control_core::tests::explicit_request_should_render_observe_without_action_executor -- --exact --nocapture`: PASS,1 passed。
- `cargo fmt -- --check`: PASS。
- `cargo check --package rustdog --bin rdog`: PASS,无 warning。
- `cargo test --package rustdog --bin rdog -- --nocapture`: PASS,252 passed。
- `git diff --check`: PASS。

### Ralph state

- 写入命令: `omx state write --input ... --json`。
- 写入结果: `success:true`。
- 读回结果:
  - `active:false`。
  - `current_phase:"complete"`。
  - `completion_audit.passed:true`。

### 当前状态

**Ralph hook 收口完成** - 代码任务、fresh verification 和 workflow state 均已完成。
