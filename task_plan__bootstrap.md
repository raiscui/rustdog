# 任务计划: bootstrap / gui-probe 产品化规划

## [2026-05-29 23:40:34] [Session ID: codex-20260529-bootstrap-plan] [计划]: `$plan 产品化功能 bootstrap`

### 目标
- 产出一个落盘到 `.omx/plans/` 的产品化计划,说明如何把初始 `@ping`、`@capabilities`、`@observe` 从 skill 批处理升级为一等只读协议能力。

### 阶段
- [x] 阶段1: 读取 `$plan`、`rdog-control`、humanizer-zh skill,确认本轮只规划不实现。
- [x] 阶段2: 读取 computer-use density 主线记录,确认已有 live E2E 和 bootstrap 批处理经验。
- [ ] 阶段3: 收集当前代码与规格事实,定位 parser / executor / observe / capabilities / shell 测试入口。
- [ ] 阶段4: 生成 `.omx/plans/rdog-bootstrap-gui-probe-productization.md`。
- [ ] 阶段5: 记录本轮 WORKLOG,检查是否需要 EPIPHANY_LOG / LATER_PLANS,最后交付计划路径和推荐方案。

### 关键约束
- 本轮是规划任务,不改 Rust 业务代码。
- 默认 bootstrap 必须 read-only,不得把真实 GUI 副作用塞入默认 fixture runner。
- 权限结果必须是一等结构化 lane,不能把 Screen Recording / Accessibility denied 当作普通成功。
- 计划至少比较"改良现有 observe/capabilities 聚合"和"新增 @bootstrap / @gui-probe 一等命令"两类方向。

### 当前状态
**目前在阶段3** - 准备用 CodeGraph 和定向文件读取收集架构事实,然后写入计划文件。

## [2026-05-29 23:46:12] [Session ID: codex-20260529-bootstrap-plan] [状态变更]: 代码与规格事实收集完成

### 已完成
- [x] 阶段3: 已确认 `ControlCommand`、`parse_control_line`、`execute_explicit_control_request`、`@observe`、`@capabilities`、`@gui-bench` 和 Zenoh session channel 边界。

### 关键事实
- `@capabilities` 和 `@observe` 当前都在 `control_core` 直接执行,不经过 action executor。
- `@observe` 已经支持 savefile frame 先发,final response 后发,适合被 bootstrap 复用。
- `@gui-bench` 默认 fixture/read-only,live replay 必须 `runner:"live",allow_side_effects:true`。

### 下一步
- [ ] 阶段4: 生成 `.omx/plans/rdog-bootstrap-gui-probe-productization.md`,只写计划,不改业务代码。

### 当前状态
**目前在阶段4** - 正在落盘产品化计划。

## [2026-05-29 23:54:42] [Session ID: codex-20260529-bootstrap-plan] [状态变更]: 产品化计划已落盘并验证图表

### 已完成
- [x] 阶段4: 已生成 `.omx/plans/rdog-bootstrap-gui-probe-productization.md`。
- [x] 已用 `beautiful-mermaid-rs --ascii` 验证计划内 flowchart 和 sequenceDiagram。
- [x] 已运行 `git diff --check -- .omx/plans/rdog-bootstrap-gui-probe-productization.md task_plan__bootstrap.md task_plan__computer_use_density.md`。

### 下一步
- [ ] 阶段5: 写入 notes / WORKLOG,检查是否需要 EPIPHANY_LOG / LATER_PLANS,然后交付计划路径。

### 当前状态
**目前在阶段5** - 正在收尾记录。

## [2026-05-29 23:58:18] [Session ID: codex-20260529-bootstrap-plan] [完成]: bootstrap 产品化 planning 完成

### 已完成
- [x] 阶段1: 读取 `$plan`、`rdog-control`、humanizer-zh skill,确认本轮只规划不实现。
- [x] 阶段2: 读取 computer-use density 主线记录,确认已有 live E2E 和 bootstrap 批处理经验。
- [x] 阶段3: 收集当前代码与规格事实,定位 parser / executor / observe / capabilities / shell 测试入口。
- [x] 阶段4: 生成 `.omx/plans/rdog-bootstrap-gui-probe-productization.md`。
- [x] 阶段5: 记录本轮 WORKLOG,检查是否需要 EPIPHANY_LOG / LATER_PLANS,最后交付计划路径和推荐方案。

### 验证
- `beautiful-mermaid-rs --ascii`: plan 内 flowchart 通过。
- `beautiful-mermaid-rs --ascii`: plan 内 sequenceDiagram 通过。
- `git diff --check -- .omx/plans/rdog-bootstrap-gui-probe-productization.md task_plan__bootstrap.md notes__bootstrap.md WORKLOG__bootstrap.md task_plan__computer_use_density.md`: 通过。

### 收尾判断
- 本轮没有新增需要单独写入 `EPIPHANY_LOG__bootstrap.md` 的灾难点。
- 后续事项已经写入计划文件 Follow-ups,本轮不另建 `LATER_PLANS__bootstrap.md`。
- `task_plan__computer_use_density.md` 当前 992 行,未超过 1000 行,但后续再写主线前应优先处理续档风险。

### 当前状态
**本轮 `$plan 产品化功能 bootstrap` 已完成** - 停在规划交付,未实施业务代码。

## [2026-05-30 11:10:40] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: `$ralplan .omx/plans/rdog-bootstrap-gui-probe-productization.md`

### 目标
- 对现有 bootstrap 产品化计划执行 ralplan consensus planning。
- 补齐 RALPLAN-DR、ADR、Architect / Critic 反馈、执行 staffing guidance 和 goal-mode follow-up。
- 输出最终仍停在计划交付,不直接实施业务代码。

### 阶段
- [x] 阶段1: 读取 `ralplan` skill、现有计划、bootstrap 支线上下文和 OMX state。
- [ ] 阶段2: 创建或复用 `.omx/context/` 快照。
- [ ] 阶段3: Planner 生成 RALPLAN-DR 和计划修订建议。
- [ ] 阶段4: Architect 顺序评审。
- [ ] 阶段5: Critic 顺序评审,必要时循环修订。
- [ ] 阶段6: 将 consensus 改进合并回计划文件并验证。
- [ ] 阶段7: 写入 WORKLOG / notes,收口交付。

### 当前状态
**目前在阶段2** - 正在创建 context snapshot。

## [2026-05-30 11:24:00] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: Architect ITERATE 后已合并 required revisions

### 已完成
- [x] 阶段2: 已创建 `.omx/context/rdog-bootstrap-gui-probe-productization-20260530T031040Z.md`。
- [x] 阶段3: Planner 已产出 RALPLAN-DR round 文件。
- [x] 阶段4: Architect 已完成顺序评审,verdict 为 ITERATE。

### Architect 要求并已合并
- `capability_policy:"cached"` 第一版拒绝,不伪装成 fresh。
- 所有 `@bootstrap` 都走 Zenoh session channel。
- observe reusable API 采用 typed-first bundle 外壳。
- `@gui-probe` 明确 deferred,不进入本计划完成条件。
- 文档区分 line-control `@bootstrap` 与 Zenoh session-open handshake。

### 下一步
- [ ] 阶段5: 对修订后的计划启动 Critic 顺序评审。

### 当前状态
**目前在阶段5** - 准备 Critic 评审。

## [2026-05-30 11:34:00] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: ralplan consensus APPROVED

### 已完成
- [x] 阶段1: 读取 `ralplan` skill、现有计划、bootstrap 支线上下文和 OMX state。
- [x] 阶段2: 创建或复用 `.omx/context/` 快照。
- [x] 阶段3: Planner 生成 RALPLAN-DR 和计划修订建议。
- [x] 阶段4: Architect 顺序评审。
- [x] 阶段5: Critic 顺序评审,必要时循环修订。
- [x] 阶段6: 将 consensus 改进合并回计划文件并验证。
- [x] 阶段7: 写入 WORKLOG / notes,收口交付。

### Consensus 结果
- Planner verdict: APPROVE WITH REQUIRED PLAN REVISIONS。
- Architect verdict: ITERATE,required revisions 已合并。
- Critic verdict: APPROVE。
- Final consensus status: APPROVED。

### 验证
- `beautiful-mermaid-rs --ascii`: plan 内第一张 Mermaid 图通过。
- `beautiful-mermaid-rs --ascii`: plan 内第二张 Mermaid 图通过。
- `git diff --check`: 通过。

### 当前状态
**本轮 `$ralplan .omx/plans/rdog-bootstrap-gui-probe-productization.md` 已完成** - 已停在 consensus-approved 计划交付,未实施业务代码。

## [2026-05-30 11:42:00] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: `$ultragoal .omx/plans/rdog-bootstrap-gui-probe-productization.md`

### 目标
- 将 consensus-approved bootstrap 计划转成 durable ultragoal artifacts,并开始按 story 顺序实现。
- 通过 Codex goal tool 持有 aggregate goal,通过 `.omx/ultragoal` ledger 跟踪每个 story。

### 阶段
- [ ] 阶段1: 创建 / 读取 `.omx/ultragoal` goals 与 handoff。
- [ ] 阶段2: 创建 aggregate Codex goal,或确认当前 active goal 匹配。
- [ ] 阶段3: 实施当前 ultragoal story。
- [ ] 阶段4: 按 story 做聚焦验证并 checkpoint。
- [ ] 阶段5: 所有 story 完成后执行 final quality gate,再完成 aggregate goal。

### 当前状态
**目前在阶段1** - 准备从 consensus plan 创建 ultragoal goals。

## [2026-05-30 11:46:19] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [继续执行]: `$ultragoal` G001 protocol parser and tests

### 目标
- 继续执行 `.omx/ultragoal/goals.json` 当前 active goal: `G001-protocol-parser-and-tests`。
- 只完成协议解析层: `BootstrapRequest` 类型、`ControlCommand::Bootstrap`、`parse_control_line` 接入、parser 拒绝用例和 `src/control_protocol/tests/bootstrap.rs`。

### 当前约束
- `@bootstrap` 第一版只允许 read-only 字段。
- `capability_policy:"cached"` 必须被拒绝,错误内容需要包含 `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`。
- 本阶段不实现执行器 response,不实现 `@gui-probe`,不把真实 GUI 副作用塞进默认测试。

### 阶段
- [ ] 阶段1: 复核 ultragoal active goal、Codex aggregate goal 和相关源码入口。
- [ ] 阶段2: 添加 `control_bootstrap` request/parser 与 `ControlCommand::Bootstrap` 接线。
- [ ] 阶段3: 添加 G001 parser 测试和拒绝测试。
- [ ] 阶段4: 运行聚焦格式化/测试/diff 检查。
- [ ] 阶段5: checkpoint G001 并进入下一个 ultragoal story。

### 状态变更
- [x] 阶段1: 已确认 active goal 是 G001,Codex aggregate goal 仍为 active,并读取 `control_protocol` / `control_actions` / `control_core` / `zenoh_control` / `observe` parser 入口。
- [ ] 阶段2: 添加 `control_bootstrap` request/parser 与 `ControlCommand::Bootstrap` 接线。

### 当前状态
**目前在阶段2** - 正在进行 parser 和测试接线编辑。

## [2026-05-30 11:51:10] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: G001 parser 初步实现完成

### 已完成
- [x] 阶段2: 已新增 `src/control_bootstrap.rs`,接入 `ControlCommand::Bootstrap`、裸 `@bootstrap` 和 object payload parser。
- [x] 阶段3: 已新增 `src/control_protocol/tests/bootstrap.rs`,覆盖默认值、gui 模式、observe override、cached 拒绝、basic+observe 拒绝、unknown/duplicate/side-effect 字段拒绝。

### 初步验证
- `cargo test --package rustdog --bin rdog control_protocol::tests::bootstrap --quiet`: 7 passed。

### 当前状态
**目前在阶段4** - 正在运行更宽的 `control_protocol::tests` 回归和 diff 检查。

## [2026-05-30 11:55:04] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: G001 checkpoint 完成

### 已完成
- [x] 阶段4: 聚焦验证已完成。
- [x] 阶段5: 已通过 `omx ultragoal checkpoint` 将 `G001-protocol-parser-and-tests` 标记为 complete。

### 验证证据
- `cargo test --package rustdog --bin rdog control_protocol::tests::bootstrap --quiet`: 7 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 27 passed。
- `git diff --check -- src/main.rs src/control_protocol.rs src/control_bootstrap.rs src/control_actions.rs src/control_protocol/tests.rs src/control_protocol/tests/bootstrap.rs src/shell/tests.rs task_plan__bootstrap.md`: 通过。

### 当前状态
**G001 已完成** - 下一步按 ultragoal 调度进入 G002 reusable observe and capabilities builders。

## [2026-05-30 12:00:03] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: G002 reusable observe and capabilities builders

### 目标
- 暴露 capabilities 的 `serde_json::Value` 出口,让 bootstrap 后续直接组合结构化报告。
- 为 observe 新增 typed-first `ObserveBundle { savefile_frames, value }` 外壳,保留原 `@response` 渲染作为兼容边界。

### 阶段
- [ ] 阶段1: 阅读 capabilities / observe producer / response / tests 当前出口。
- [ ] 阶段2: capabilities JSON 出口改为委托 Value 出口。
- [ ] 阶段3: observe 增加 bundle API,原 outcome 走 bundle 再渲染 response line。
- [ ] 阶段4: 增加聚焦测试并验证。
- [ ] 阶段5: checkpoint G002。

### 当前状态
**目前在阶段1** - 已确认 G002 active,正在编辑 capabilities / observe API。

## [2026-05-30 12:08:22] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: G002 实现与验证完成

### 已完成
- [x] 阶段1: 已阅读 capabilities / observe producer / response / tests 当前出口。
- [x] 阶段2: `current_capabilities_report_json()` 已改为委托新的 `current_capabilities_report_value()`。
- [x] 阶段3: `@observe` 已新增 `ObserveBundle { savefile_frames, value }` 和 `build_observe_bundle()`。
- [x] 阶段4: 已补充 capabilities value 测试和 observe bundle 测试。

### 验证证据
- `cargo test --package rustdog --bin rdog control_capabilities::tests --quiet`: 4 passed。
- `cargo test --package rustdog --bin rdog control_observation::observe::tests --quiet`: 5 passed。
- `cargo test --package rustdog --bin rdog control_core::tests --quiet`: 15 passed。
- `git diff --check -- src/control_capabilities.rs src/control_observation.rs src/control_observation/observe.rs src/control_observation/observe/response.rs src/control_observation/observe_tests.rs task_plan__bootstrap.md`: 通过。

### 当前状态
**G002 已实现并验证** - 准备 checkpoint G002,然后进入 G003 bootstrap execution and response。

## [2026-05-30 12:18:16] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: G003 bootstrap execution and response

### 已完成
- 新增 `build_bootstrap_outcome()`,直接组合 liveness、`current_capabilities_report_value()` 和 `build_observe_bundle()`。
- `control_core` 已直接处理 `ControlCommand::Bootstrap`,不进入默认 action executor。
- `@bootstrap` response 使用 `rdog.bootstrap.v1`,包含 `liveness`、`capability_policy`、`capabilities`、`observation`、`lanes`、`errors`、`frames` 和可选 `trace`。
- `capability_policy:"cached"` 的 parser error 已保留结构化 `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED` value,`parse_and_execute_control_line` 会把它渲染成结构化 `@response`。

### 验证证据
- `cargo test --package rustdog --bin rdog control_core::tests --quiet`: 18 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::bootstrap --quiet`: 7 passed。
- `cargo test --package rustdog --bin rdog control_capabilities::tests --quiet`: 4 passed。
- `cargo test --package rustdog --bin rdog control_observation::observe::tests --quiet`: 5 passed。
- `git diff --check -- src/control_bootstrap.rs src/control_core.rs src/control_capabilities.rs src/control_observation.rs src/control_observation/observe.rs src/control_observation/observe/response.rs src/control_observation/observe_tests.rs task_plan__bootstrap.md`: 通过。

### 当前状态
**G003 已实现并验证** - 准备 checkpoint G003,然后进入 G004 transport and receiver coverage。

## [2026-05-30 12:24:05] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: G004 transport and receiver coverage

### 目标
- 将所有 `@bootstrap` 命令纳入 Zenoh session-channel-only 分类,包括 `mode:"basic"`。
- 增加 shell receiver 的 basic/gui bootstrap 覆盖,确认真实 receiver 输出结构化 `rdog.bootstrap.v1`。
- 增加 Zenoh legacy queryable 分类测试,确认 legacy path 拒绝 `@bootstrap` 并提示 session channel。

### 阶段
- [ ] 阶段1: 修改 Zenoh session-channel-only 分类和测试。
- [ ] 阶段2: 增加 shell receiver basic/gui bootstrap 测试。
- [ ] 阶段3: 跑聚焦验证并 checkpoint G004。

### 当前状态
**目前在阶段1** - 正在编辑 Zenoh 分类和 shell receiver 测试。

## [2026-05-30 12:31:02] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: G004 transport and receiver coverage

### 已完成
- [x] 阶段1: `ControlCommand::Bootstrap(_)` 已加入 Zenoh session-channel-only 分类。
- [x] 阶段2: 已新增 shell receiver basic/gui bootstrap 测试,确认真实 receiver 返回 `rdog.bootstrap.v1`。
- [x] 阶段3: 已完成聚焦验证。

### 验证证据
- `cargo test --package rustdog --bin rdog zenoh_control::tests::legacy_queryable_should_reject_bootstrap_requests --quiet`: 1 passed。
- `cargo test --package rustdog --bin rdog shell::tests::control_receiver_should_execute_basic_bootstrap_preflight --quiet`: 1 passed。
- `cargo test --package rustdog --bin rdog shell::tests::control_receiver_should_execute_gui_bootstrap_window_probe --quiet`: 1 passed。
- `cargo test --package rustdog --bin rdog zenoh_control::tests --quiet`: 7 passed。
- `cargo test --package rustdog --bin rdog control_core::tests --quiet`: 18 passed。
- `cargo test --package rustdog --bin rdog shell::tests --quiet`: 14 passed。
- `git diff --check -- src/zenoh_control.rs src/shell/tests.rs src/control_bootstrap.rs src/control_core.rs task_plan__bootstrap.md`: 通过。

### 当前状态
**G004 已实现并验证** - 准备 checkpoint G004,然后进入 G005 docs and skill update。

## [2026-05-30 12:44:11] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: G005 docs and skill update

### 已完成
- 更新 `README.md`: 增加 `@bootstrap` 能力矩阵、read-only 语义、cached 拒绝和旧 daemon fallback。
- 更新 `.codex/skills/rdog-control/SKILL.md`: GUI 起手改为优先 `@bootstrap`,旧三行 preflight 作为 fallback。
- 更新 `.codex/skills/rdog-control/references/control-workflow.md`、`protocol.md`、`cookbook-web-content.md`: 统一 `rdog.bootstrap.v1`、session-channel-only、cached 拒绝和 fallback 口径。
- 更新 `specs/control-line-protocol.md`: 增加 `@bootstrap` 正式协议段、字段、拒绝字段、response shape 和 Zenoh session-channel-only 边界。
- 更新 `specs/code-agent-rdog-control-usage.md`: 能力矩阵和 GUI workflow 改为 bootstrap-first。
- 更新 `specs/rdog-computer-use-density-plan.md`: 记录 `@bootstrap` fresh-only 当前交付、`@gui-probe` deferred、fixture/live side-effect 边界。

### 验证证据
- `git diff --check -- README.md .codex/skills/rdog-control/SKILL.md .codex/skills/rdog-control/references/control-workflow.md .codex/skills/rdog-control/references/protocol.md .codex/skills/rdog-control/references/cookbook-web-content.md specs/control-line-protocol.md specs/code-agent-rdog-control-usage.md specs/rdog-computer-use-density-plan.md task_plan__bootstrap.md`: 通过。
- `rg` 关键词验证已覆盖 `@bootstrap#`、`rdog.bootstrap.v1`、`BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`、旧 daemon fallback、`@gui-probe` deferred、`runner:"fixture"` 和 `allow_side_effects:true`。

### 当前状态
**G005 已完成** - 准备 checkpoint G005,然后进入最终 G006 verification and quality gate。

## [2026-05-30 12:51:04] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: G006 verification and final quality gate

### 目标
- 运行最终 targeted verification。
- 运行 scoped `ai-slop-cleaner`。
- cleaner 后复跑验证。
- 运行独立 `$code-review`,只有 APPROVE + CLEAR + 双 lane evidence 才完成 aggregate Codex goal。

### 阶段
- [ ] 阶段1: cargo fmt/check/test + docs grep + Mermaid validation。
- [ ] 阶段2: scoped ai-slop-cleaner。
- [ ] 阶段3: cleaner 后复跑验证。
- [ ] 阶段4: 独立 code-review。
- [ ] 阶段5: update_goal complete + checkpoint final quality gate。

### 当前状态
**目前在阶段1** - 正在运行最终 targeted verification。

## [2026-05-30 13:01:12] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: G006 pre-cleaner verification 通过

### 已完成
- [x] 阶段1: cargo fmt/check/test + docs grep + Mermaid validation。

### 验证证据
- `cargo fmt --check`: 通过。
- `cargo test --package rustdog --bin rdog --quiet`: 308 passed。
- `git diff --check` scoped changed files: 通过。
- docs/skill `rg`: 找到 `@bootstrap#`、`rdog.bootstrap.v1`、`BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`、`@gui-probe` deferred、session-channel-only、fixture/live side-effect 边界。
- `beautiful-mermaid-rs --ascii`: `.omx/plans/rdog-bootstrap-gui-probe-productization.md` 前两个 Mermaid 图均通过。

### 当前状态
**目前在阶段2** - 准备运行 scoped ai-slop-cleaner。

## [2026-05-30 13:09:40] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: ai-slop-cleaner no-op/pass

### 已完成
- [x] 阶段2: scoped ai-slop-cleaner 已完成。

### 结论
- 未发现 masking fallback slop。
- 旧 daemon fallback、`not_requested` lane、`capability_policy:"cached"` blocked error 都是显式契约,不是静默兜底。
- 清理报告已写入 `.omx/ultragoal/ai-slop-cleaner-g006.md`。

### 当前状态
**目前在阶段3** - 正在 cleaner 后复跑验证。

## [2026-05-30 13:14:12] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: post-cleaner verification 通过

### 已完成
- [x] 阶段3: cleaner 后复跑验证通过。

### 验证证据
- `cargo fmt --check`: 通过。
- `cargo test --package rustdog --bin rdog --quiet`: 308 passed。
- scoped `git diff --check`: 通过。
- `beautiful-mermaid-rs --ascii`: plan 中两个 Mermaid 图均通过。

### 当前状态
**目前在阶段4** - 准备运行独立 code-review。

## [2026-05-30 12:27:13] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: final review blockers 已记录并进入 G007

### Review 结果
- code-reviewer recommendation: COMMENT。
- architect status: WATCH。
- 按 ultragoal final gate,已执行 `record-review-blockers`,追加 `G007-resolve-final-code-review-blockers`。

### 需要修复
- `@bootstrap#id:{capability_policy:"cached"}` parse error 需要保留 request id。
- Zenoh legacy queryable 拒绝文案不能再说支持 bootstrap。
- README 中 "after bootstrap" 需要明确是 Zenoh session-open bootstrap,不是 line-control `@bootstrap`。
- `specs/control-line-protocol.md` 的 "当前支持的 kind" 列表需要同步新命令。

### 当前状态
**目前在 G007** - 正在修复 review blockers。

## [2026-05-30 12:36:42] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: G007 review blockers 已修复并进入复审

### 已修复
- `@bootstrap#42:{capability_policy:"cached"}` 现在返回结构化 blocked error 时保留 `id:42`。
- Zenoh legacy queryable 拒绝文案已改成只支持 simple compatibility requests,明确 `@bootstrap` 必须走 session channel。
- README 已把 Zenoh session-open bootstrap 与 line-control `@bootstrap` 区分开。
- `specs/control-line-protocol.md` 的 "当前支持的 kind" 列表已同步 bootstrap / observe / web / gui-bench / selector / window / savefile 等命令。

### 验证证据
- `cargo test --package rustdog --bin rdog control_core::tests::parse_error_should_preserve_bootstrap_cached_policy_structure --quiet`: passed。
- `cargo test --package rustdog --bin rdog zenoh_control::tests::legacy_queryable_should_reject_bootstrap_requests --quiet`: passed。
- `cargo fmt --check`: passed。
- `cargo test --package rustdog --bin rdog --quiet`: 308 passed。
- scoped `git diff --check`: passed。
- Mermaid validation: passed。
- scoped ai-slop-cleaner report: `.omx/ultragoal/ai-slop-cleaner-g007.md`。

### 当前状态
**目前在 G007 final review rerun** - 已启动 code-reviewer 和 architect 两条独立复审 lane。

## [2026-05-30 13:45:20] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: final quality gate clean

### Review 复审结果
- code-reviewer recommendation: APPROVE。
- architect status: CLEAR。
- independent review evidence: code-reviewer agent `019e772c-2f31-7e82-854c-056556bf6731`; architect agent `019e7726-6feb-7133-b1e5-5eb5854ea76c`。

### Quality gate 文件
- `.omx/ultragoal/quality-gate-g007.json`

### 当前状态
**目前在 G007 checkpoint** - 准备 `update_goal complete`,再用 complete snapshot checkpoint ultragoal。

## [2026-05-30 12:47:12] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: `$ultragoal` bootstrap productization artifact complete

### 完成状态
- G001-G005 已完成。
- G006 保留为 `review_blocked` 审计记录。
- G007 已完成,用于解决 G006 final review blockers。
- `omx ultragoal status --json`: `artifactComplete: true`,无 pending / in_progress / failed / needs-user-decision。
- Codex aggregate goal 已 `update_goal complete`。

### 最终质量门
- `.omx/ultragoal/quality-gate-g007.json`
- ai-slop-cleaner: passed。
- verification: passed。
- independent code review: code-reviewer APPROVE, architect CLEAR。

### 最终验证证据
- `cargo fmt --check`: passed。
- `cargo test --package rustdog --bin rdog --quiet`: 308 passed。
- targeted blocker tests: passed。
- scoped `git diff --check`: passed。
- docs grep: passed。
- Mermaid validation: passed。

### 当前状态
**本轮 `$ultragoal .omx/plans/rdog-bootstrap-gui-probe-productization.md` 已完成** - 后续只剩人工决定是否提交/整理 dirty workspace。

## [2026-05-30 12:50:19] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [收尾]: 当前会话复核 ultragoal 完成状态

### 目标
- 重新确认 `.omx/ultragoal` artifact 状态和 Codex aggregate goal 状态。
- 用当前会话刚运行的验证证据补齐最终汇报口径。
- 将 `$ultragoal` 实际交付追加到 `WORKLOG__bootstrap.md`。

### 已确认
- `omx ultragoal status --json`: `artifactComplete:true`,无 pending / in_progress / failed / needs-user-decision。
- `get_goal`: aggregate Codex goal 状态为 `complete`。
- `cargo fmt --check`: 通过。
- `cargo test --package rustdog --bin rdog --quiet`: 308 passed。
- scoped `git diff --check`: 通过。
- docs / skill `rg`: bootstrap、cached 拒绝、session-channel-only、fixture/live opt-in 和 fallback 口径均可检索。
- `.omx/plans/rdog-bootstrap-gui-probe-productization.md` 前两个 Mermaid 图通过 `beautiful-mermaid-rs --ascii` 验证。

### 当前状态
**收尾验证已完成** - 正在补写 `WORKLOG__bootstrap.md`。

## [2026-05-30 12:51:09] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [完成]: 当前会话收尾记录已补齐

### 已完成
- `WORKLOG__bootstrap.md` 已追加 `$ultragoal` bootstrap 产品化实现记录。
- 当前会话已完成 fresh verification。
- 未创建 `EPIPHANY_LOG__bootstrap.md` 或 `LATER_PLANS__bootstrap.md`,因为本轮没有新增灾难点或延期项。

### 当前状态
**`$ultragoal .omx/plans/rdog-bootstrap-gui-probe-productization.md` 收尾完成** - 可以向用户交付最终摘要。

## [2026-06-01 14:32:36] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [计划]: 拆分 bootstrap diff 并提交

### 目标
- 按主题拆当前混合工作区 diff。
- 只提交 bootstrap 产品化相关变更。
- 提交后继续下一阶段真实 e2e 闭环,因为这条线已有明确的“首页点击后瀑布流变更才算成功”验收口径。

### 阶段
- [ ] 阶段1: 盘点 modified / untracked 文件,判断 bootstrap 边界。
- [ ] 阶段2: 只暂存 bootstrap 相关文件或必要 hunk。
- [ ] 阶段3: 运行提交前验证。
- [ ] 阶段4: 创建 bootstrap commit。
- [ ] 阶段5: 转入真实 e2e 闭环准备。

### 当前状态
**目前在阶段1** - 正在按主题检查 diff,避免把 screenshot / AX / computer-use bench 等其它支线混入 bootstrap commit。

## [2026-06-01 14:46:18] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] [状态变更]: bootstrap 暂存区已拆出并通过临时树验证

### 已完成
- [x] 阶段1: 盘点 modified / untracked 文件,判断 bootstrap 边界。
- [x] 阶段2: 只暂存 bootstrap 相关文件或必要 hunk。
- [x] 阶段3: 运行提交前验证。

### 验证证据
- `git diff --cached --check`: 通过。
- `cargo fmt --check`: 在暂存区导出的临时树 `/tmp/rdog-bootstrap-index.HMG95s` 中通过。
- `cargo test --package rustdog --bin rdog control_protocol::tests::bootstrap --quiet`: 暂存区临时树 7 passed。
- `cargo test --package rustdog --bin rdog control_core::tests::parse_error_should_preserve_bootstrap_cached_policy_structure --quiet`: 暂存区临时树 1 passed。
- `cargo test --package rustdog --bin rdog zenoh_control::tests::legacy_queryable_should_reject_bootstrap_requests --quiet`: 暂存区临时树 1 passed。
- `cargo test --package rustdog --bin rdog --quiet`: 暂存区临时树 276 passed。

### 当前状态
**目前在阶段4** - 准备创建 bootstrap commit。
