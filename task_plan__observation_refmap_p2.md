# 任务计划: observation-scoped refmap P2 可落地计划

## [2026-05-20 07:43:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [记录类型]: 启动 P2 Ralplan 规划

### 目标

根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P2 可执行计划, 输出到 `.omx/plans/ralplan-rdog-observation-refmap-p2.md`。

### 范围边界

- 本轮只做规划, 不修改 Rust 实现代码。
- P2 计划必须承接 P0/P1: `@eN` 仍然是 observation-scoped ephemeral ref, durable state 只保存 observation 元数据和 selector draft/hint。
- P2 重点应落在 permanent selector 的稳定 schema、显式 resolve/inspect surface、fixtures 与验证矩阵。
- 不把 P3/P4/P5 内容混进 P2: 不做自动语义重找、不默认对 selector 发起 side-effect action、不新增完整 `@observe` bundle、不把所有 mouse command ref 化。

### 阶段

- [x] 阶段1: 读取工作流和表达约束
- [x] 阶段2: 回读 P0/P1 规格、计划和当前实现触点
- [x] 阶段3: 提炼 P2 方案选项与 ADR
- [x] 阶段4: 创建 context snapshot 与 P2 plan
- [x] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### 已观察到的错误

- `omx state read --input '{"session_id":"019e38be-b9d9-76f0-aabc-fad94a2bcf12","workflow":"ralplan"}' --json` 返回 `{"error":"mode must be a string"}`。
- 当前处理: 暂不依赖错误输入继续推进。若需要写回 Ralplan 状态, 先查 `omx state --help` 后再使用正确 schema。

### 状态

**目前在阶段2** - 准备回读 P0/P1 规格、P1 计划和当前 selector/durable 代码触点, 用这些证据限定 P2 计划边界。

## [2026-05-20 07:43:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: P2 brownfield 事实已记录

### 已完成

- [x] 阶段1: 读取工作流和表达约束
- [x] 阶段2: 回读 P0/P1 规格、计划和当前实现触点
- [ ] 阶段3: 提炼 P2 方案选项与 ADR
- [ ] 阶段4: 创建 context snapshot 与 P2 plan
- [ ] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### 当前结论

- P2 的主问题不是重新做 durable state,而是把 P1 的 `rdog.selector.draft.v1` 提升为 stable `rdog.selector.v1`。
- 当前 `selector_id` 由 `observation_id + ref` 派生,这是 P2 必须修正的核心点。
- P2 应先提供 explicit inspect / resolve dry-run surface,不把 side-effect action by selector 提前做成默认路径。

### 状态

**目前在阶段3** - 准备生成 RALPLAN-DR 选项、ADR 和可执行分阶段计划。

## [2026-05-20 07:43:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: P2 Ralplan 计划已落盘并验证

### 已完成

- [x] 阶段1: 读取工作流和表达约束
- [x] 阶段2: 回读 P0/P1 规格、计划和当前实现触点
- [x] 阶段3: 提炼 P2 方案选项与 ADR
- [x] 阶段4: 创建 context snapshot 与 P2 plan
- [x] 阶段5: 验证计划文件、记录 worklog、更新 Ralplan 状态

### 交付文件

- `.omx/plans/ralplan-rdog-observation-refmap-p2.md`
- `.omx/context/observation-refmap-p2-20260519T234334Z.md`
- `notes__observation_refmap_p2.md`
- `task_plan__observation_refmap_p2.md`

### 验证证据

- `beautiful-mermaid-rs --ascii` 已验证 P2 architecture flowchart。
- `beautiful-mermaid-rs --ascii` 已验证 P2 selector-resolve sequenceDiagram。
- 未收口占位关键字检查对本轮 P2 文件无命中。
- `git diff --check -- ...` 对本轮 P2 文件通过。
- `omx state write --input '{"mode":"ralplan",...}' --json` 返回 `success:true`。

### 当前状态

**完成** - P2 计划已可交给 `$ralph .omx/plans/ralplan-rdog-observation-refmap-p2.md` 或 `$team` 执行。

## [2026-05-20 16:33:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [执行]: Ralph 开始落地 P2 permanent selector

### 行动目的

- 用户已用 `$ralph .omx/plans/ralplan-rdog-observation-refmap-p2.md` 要求执行 P2。
- 本轮从规划切换为代码、测试、文档和验证落地。

### 将要做什么

- [x] 创建 P2 执行 context snapshot。
- [ ] 实现 `rdog.selector.v1` stable schema、canonicalization、fingerprint 和 stable selector id。
- [ ] 升级 durable selector index,支持 selector id lookup,并让 stale/expired hint 返回 stable selector id。
- [ ] 接入 `@selector-get`。
- [ ] 接入 `@selector-resolve` dry-run,返回候选和解释,不执行动作。
- [ ] 更新 docs / skill。
- [ ] 运行 focused tests、format、diff check、Ralph completion audit。

### 当前状态

**目前在实现阶段** - 先改 selector model 和测试,锁住 stable identity。

## [2026-05-20 16:33:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [错误]: Cargo focused test 过滤器一次只能传一个

### 已观察现象

- 执行 `cargo test --package rustdog --bin rdog control_observation::durable::tests control_observation::selector::tests` 返回 `unexpected argument 'control_observation::selector::tests'`。

### 结论

- 这是命令使用错误,不是代码编译错误。
- 后续改为逐条运行 focused test。

## [2026-05-20 16:33:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: stable selector schema 与协议入口已接入

### 已完成

- [x] `rdog.selector.v1` permanent selector schema。
- [x] constraints / hints / source 分层。
- [x] `sha256` fingerprint 和 `sel-v1-*` stable selector id。
- [x] P1 `DurableSelectorDraft` -> P2 `PermanentSelector` conversion。
- [x] durable index 写入 stable selector id / fingerprint / permanent selector。
- [x] `@selector-get` / `@selector-resolve` parser 和 control_core dispatch。
- [x] `@selector-resolve dry_run:false` 明确返回 `SELECTOR_ACTION_DEFERRED`。
- [x] AX element selector golden fixture。

### 验证证据

- `cargo test --package rustdog --bin rdog control_observation::selector::tests`: 4 passed。
- `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 3 passed。
- `cargo test --package rustdog --bin rdog control_observation::tests`: 7 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_selector_commands`: 1 passed。
- `cargo test --package rustdog --bin rdog control_core::tests`: 13 passed。

### 当前状态

**目前在文档与回归阶段** - 继续更新 `rdog-control` skill 和 repo specs,然后跑完整 P2 验证矩阵。

## [2026-05-20 18:55:45] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [继续]: 从 P2 验证与收口阶段恢复

### 行动目的

- 用户要求继续,本轮承接上次 Ralph P2 执行状态。
- 已落地的核心实现需要重新用 fresh evidence 验证,避免只沿用上一轮口头结论。

### 将要做什么

- [ ] 复核本轮 P2 文档和协议说明是否已经覆盖 permanent selector workflow。
- [ ] 运行 focused tests、扩展回归矩阵、格式检查和 diff 空白检查。
- [ ] 验证 `rdog-control` skill 结构。
- [ ] 进行 Ralph 要求的 deslop pass,限定在本轮 P2 已触碰文件。
- [ ] deslop 后重新跑关键验证。
- [ ] 写入 WORKLOG 和 Ralph completion audit。

### 当前状态

**目前在验证阶段** - 先做文档和 diff 复核,然后跑测试矩阵。

## [2026-05-20 18:57:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [错误]: rustfmt 检查发现格式漂移

### 已观察现象

- `cargo fmt -- --check` 返回 code 1。
- 输出只包含 rustfmt diff,集中在 `control_core.rs`、`control_observation.rs`、`control_observation/durable.rs` 和 `control_observation/selector.rs` 的换行排版。

### 当前处理

- 先运行 `cargo fmt` 做机械格式化。
- 随后重新运行 `cargo fmt -- --check`,再进入测试矩阵。

## [2026-05-20 18:58:50] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: P2 focused tests 已重新验证通过

### 已完成验证

- [x] `cargo fmt -- --check`: 通过。
- [x] `cargo test --package rustdog --bin rdog control_observation::selector::tests`: 4 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 3 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::tests`: 7 passed。
- [x] `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_selector_commands`: 1 passed。
- [x] `cargo test --package rustdog --bin rdog control_core::tests`: 13 passed。

### 当前状态

**目前在扩展回归阶段** - 继续验证 AX/window/screenshot/config/Zenoh 邻接面。

## [2026-05-20 19:00:55] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: 扩展验证矩阵与 skill 校验通过

### 已完成验证

- [x] `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- [x] `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- [x] `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- [x] `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- [x] `cargo test --package rustdog --bin rdog config::tests`: 26 passed。
- [x] `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`: 23 passed, 2 ignored。
- [x] `git diff --check`: 通过。
- [x] `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: `Skill is valid!`。

### 当前状态

**目前在 deslop 和审查阶段** - 限定 P2 文件做反 slop 检查,没有证据不做额外重构。

## [2026-05-20 19:02:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: deslop 检查完成且无需代码改动

### 检查结果

- [x] 已按 Ralph 要求对 P2 触碰文件做反 slop 检查。
- [x] fallback-like 命中已分类到 `notes__observation_refmap_p2.md`。
- [x] 未发现 masking fallback、吞错、绕过验证或临时 workaround。
- [x] 不做额外重构,避免把 P2 收口阶段扩大成 P3/P5 结构重写。

### 当前状态

**目前在 post-deslop 复验阶段** - 因 deslop 没有改代码,仍然重新跑关键验证矩阵作为收口证据。

## [2026-05-20 19:05:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: post-deslop 回归验证通过

### 复验证据

- [x] `cargo fmt -- --check`: 通过。
- [x] `control_observation::selector::tests`: 4 passed。
- [x] `control_observation::durable::tests`: 3 passed。
- [x] `control_observation::tests`: 7 passed。
- [x] `control_protocol::tests::parse_should_support_selector_commands`: 1 passed。
- [x] `control_core::tests`: 13 passed。
- [x] `control_ax::tests`: 14 passed。
- [x] `control_ax::query::tests`: 5 passed。
- [x] `control_window::tests`: 6 passed。
- [x] `screenshot::tests`: 17 passed。
- [x] `config::tests`: 26 passed。
- [x] `zenoh_router_client`: 23 passed, 2 ignored。
- [x] `git diff --check`: 通过。
- [x] `quick_validate.py .codex/skills/rdog-control`: `Skill is valid!`。

### 当前状态

**目前在最终审查阶段** - 做 prompt-to-artifact audit 和 Ralph architect review。

## [2026-05-20 19:12:56] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [审查结果]: Ralph reviewer 要求继续迭代

### Reviewer 结论

- `CHANGES_REQUESTED`

### 必须修的问题

- [ ] `@selector-resolve` 的错误/解释契约未收紧: 零候选、多候选、权限/unsupported 要有结构化 selector error,解释字段不能只是把 selector 约束字段名原样回显。
- [ ] fixture 覆盖不足: 目前只有 AX element golden,缺 AX window 和 window selector golden fixture。
- [ ] `include_history` 是表面实现: 现在只是返回 `[last_seen]`,需要 durable history API 或移除误导表面。

### 当前状态

**目前回到修复阶段** - 先补 durable history 和 fixture,再收紧 selector-resolve 结果契约。

## [2026-05-20 19:18:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: reviewer 三条阻断已做第一轮修复

### 已完成修复

- [x] `@selector-resolve` 增加 finalize gate: 0 候选返回 `SELECTOR_NOT_FOUND`,多候选返回 `AMBIGUOUS_SELECTOR`,backend 权限/unsupported 映射为 selector error。
- [x] `matched_fields` / `missing_fields` 改为基于候选实际字段比较,不再只是回显 selector 约束字段。
- [x] durable store 增加真实 `selector_history(selector_id, limit)`。
- [x] `@selector-get include_history:true` 返回真实 durable history,不再伪造 `[last_seen]`。
- [x] 新增 AX window 和 window 两类 selector golden fixture。

### 已完成验证

- [x] `cargo test --package rustdog --bin rdog control_observation::selector::tests`: 6 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 4 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::tests`: 10 passed。

### 当前状态

**目前在阻断修复后的回归阶段** - 继续跑 protocol/core/AX/window/screenshot/config/Zenoh 矩阵。

## [2026-05-20 19:20:30] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: 阻断修复后的完整回归矩阵通过

### 验证证据

- [x] `cargo fmt -- --check`: 通过。
- [x] `control_protocol::tests::parse_should_support_selector_commands`: 1 passed。
- [x] `control_core::tests`: 13 passed。
- [x] `control_ax::tests`: 14 passed。
- [x] `control_ax::query::tests`: 5 passed。
- [x] `control_window::tests`: 6 passed。
- [x] `screenshot::tests`: 17 passed。
- [x] `config::tests`: 26 passed。
- [x] `zenoh_router_client`: 23 passed, 2 ignored。
- [x] `git diff --check`: 通过。
- [x] `quick_validate.py .codex/skills/rdog-control`: `Skill is valid!`。

### 当前状态

**目前在二次 deslop 和复审阶段** - 重新检查新增修复有没有引入假历史、吞错或过度范围。

## [2026-05-20 19:28:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: P2 permanent selector 落地并通过复审

### 最终完成项

- [x] `rdog.selector.v1` stable schema 已落地。
- [x] stable selector id 来自 selector fingerprint,不依赖 `observation_id` / `@eN`。
- [x] P1 selector draft 可转换为 P2 permanent selector,旧 draft id 保留在 `source.draft_selector_id`。
- [x] durable index 支持 selector id lookup、last_seen 和真实 history。
- [x] `@selector-get` 已接入 parser / dispatch / durable response。
- [x] `@selector-resolve` 已接入 dry-run parser / dispatch / resolver,并对 0 候选、多候选、side-effect 请求、backend 权限/unsupported 返回结构化错误。
- [x] AX element / AX window / window 三类 golden fixtures 已覆盖。
- [x] docs / skill 已写明 stale hint -> selector-get -> selector-resolve dry-run -> fresh ref workflow。
- [x] Ralph reviewer 第二轮复审 `APPROVED`,阻断项 0。

### 最终验证证据

- [x] `cargo fmt -- --check`: 通过。
- [x] `cargo test --package rustdog --bin rdog control_observation::selector::tests`: 6 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 4 passed。
- [x] `cargo test --package rustdog --bin rdog control_observation::tests`: 10 passed。
- [x] `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_selector_commands`: 1 passed。
- [x] `cargo test --package rustdog --bin rdog control_core::tests`: 13 passed。
- [x] `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- [x] `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- [x] `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- [x] `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- [x] `cargo test --package rustdog --bin rdog config::tests`: 26 passed。
- [x] `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`: 23 passed, 2 ignored。
- [x] `git diff --check`: 通过。
- [x] `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: `Skill is valid!`。

### 当前状态

**完成** - P2 已通过实现、验证、deslop 检查和 Ralph reviewer 复审。
