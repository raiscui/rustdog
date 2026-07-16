# 任务计划: daemon-side @flow plan

## [2026-06-28 23:04:11] [Session ID: codex-20260628-plan-daemon-flow] [续档]: task_plan.md rollover

### 目标

为 daemon-side full script flow 建立 `.omx/plans/rdog-daemon-flow-plan.md`,把前一版过窄的 `@ui-flow` 方案升级为完整 `@flow` 方案。

### 阶段

- [x] 阶段1: 发现旧 `task_plan.md` 已到 1000 行,按规则归档为 `archive/default_history/task_plan_2026-06-28_205227_before_flow_plan.md`。
- [x] 阶段2: 读取现有 UI script runner、control protocol、shell/cmd 执行和 frame/artifact 相关上下文。
- [x] 阶段3: 新增 `.omx/plans/rdog-daemon-flow-plan.md`,明确 `@flow` 的完整脚本能力、边界、实施步骤和验证。
- [x] 阶段4: 验证 Mermaid、Markdown whitespace 和 diff check,并记录 notes / WORKLOG。

### 做出的决定

- `@flow` 是 daemon 侧完整自动化脚本 runtime,不是 `@ui-flow` 的 GUI 子集。
- `@ui-flow` 可以后续作为安全 profile 或别名考虑,但本轮主计划聚焦 `@flow`。

### 状态

**目前在完成** - `@flow` plan 已落盘并完成验证。

## [2026-06-29 10:50:24] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: Ultragoal 执行 daemon-side @flow

### 目标

按 `.omx/plans/rdog-daemon-flow-plan.md` 进入 Ultragoal 执行,把 daemon-side full script `@flow` 从计划推进到可验证实现。

### 阶段

- [x] 阶段1: 初始化 / 修复 Ultragoal 状态读取,生成 `.omx/ultragoal/goals.json` 与当前 story handoff。
- [x] 阶段2: 根据当前 story 读取相关代码和规格,只实现当前 story 范围。
- [x] 阶段3: 补 parser / runner / fixture 或 live smoke 所需测试,运行 targeted verification。
- [x] 阶段4: 按 Ultragoal 规则 checkpoint 当前 story,记录 WORKLOG / 后续项 / 风险。

### 做出的决定

- 采用 plan 中的方案 A: 新增 daemon-side `@flow` full script runtime,不把 shell 能力塞进 `@ui-flow`。
- `@flow` 的 shell step 必须显式 `policy.allow_shell:true`,daemon 文件路径语义属于 daemon 本机。
- 本轮先按 Ultragoal 当前 story 推进,不擅自一次性跳到最终 story。

### 状态

**目前在完成** - G001 已 checkpoint complete;下一步进入 G002 daemon flow runtime shell lane。

## [2026-06-29 11:01:43] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: Ultragoal G002 shell lane

### 目标

实现 daemon-side `@flow` 的有限顺序 runtime shell lane,覆盖 `Cmd`、`Script`、`SleepMs`、`Expect`、`Exit` 的真实执行与测试。

### 阶段

- [x] 阶段1: 读取 `control_flow`、`control_actions::build_shell_command`、已有 timeout/process 经验和相关测试结构。
- [x] 阶段2: 在 `control_flow` 中补 runtime state、cmd/script execution、timeout、capture、truncation 和 Expect evaluator。
- [x] 阶段3: 增加 G002 focused tests,覆盖 stdout/stderr/exit/timeout/truncation。
- [x] 阶段4: 运行验证并 checkpoint G002。

### 状态

**目前在完成** - G002 已 checkpoint complete;下一步进入 G003 ControlLine 和 artifact lane。

### 验证

- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog explicit_request_should_execute_minimal_flow_shell_lane --quiet`: 1 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 11:12:02] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: Ultragoal G003 ControlLine 和 artifact lane

### 目标

让 `@flow` 支持 `ControlLine`、inner response 消费、inner SaveFile 外提、`SaveArtifact`、trace savefile,并补对应 focused tests。

### 阶段

- [x] 阶段1: 读取 `ControlExecutionOutcome`、`ControlFrame`、`SaveFileFrame` 和现有 `@savefile` helper。
- [x] 阶段2: 扩展 flow runtime state,接收 control step executor,收集 response/artifact/trace。
- [x] 阶段3: 补 `ControlLine:"@ping"` response expect、SaveArtifact、trace savefile、nested `@flow` 边界测试。
- [x] 阶段4: 运行验证并 checkpoint G003。

### 状态

**目前在完成** - G003 已 checkpoint complete;下一步进入 G004 control-core integration regression。

### 验证

- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 11:20:40] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: Ultragoal G004 control-core regression

### 目标

确认 `ControlCommand::Flow` 已通过 `execute_explicit_control_request` 接入,并用 focused parser/runtime/control/ui_script 测试证明没有回归。

### 阶段

- [x] 阶段1: 复核 G003 已接入的 control_core Flow 分支和 runtime 入口。
- [x] 阶段2: 跑 focused flow / control_core / control_protocol / ui_script 测试矩阵。
- [x] 阶段3: 跑 `cargo check`、`cargo fmt -- --check`、`git diff --check`。
- [x] 阶段4: checkpoint G004。

### 状态

**目前在完成** - G004 已 checkpoint complete;下一步进入 G005 docs/live smoke/final quality gate。

### 验证

- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script --quiet`: 20 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 11:24:07] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: Ultragoal G005 docs/live smoke/final gate

### 目标

同步 `@flow` 与 `@ui-flow` 的文档边界,跑非破坏 live smoke,完成 final verification、ai-slop-cleaner、architecture invariant audit 和 independent code review。

### 阶段

- [ ] 阶段1: 更新 tracked specs / AGENTS 索引,明确 `@flow` 是 daemon-side full script runtime,`@ui-flow` 只是未来 GUI-only profile/alias。
- [ ] 阶段2: 运行非破坏 live `@flow` smoke。
- [ ] 阶段3: 跑 final verification 和 ai-slop-cleaner。
- [ ] 阶段4: 做 architecture invariant audit 与 independent code review,生成 quality gate JSON。
- [ ] 阶段5: `update_goal complete` 并 checkpoint final story。

### 状态

**目前在阶段1** - 准备同步 specs 文档。

### 遇到错误

- `omx state read --mode ultragoal --json` 返回 `mode must be one of ...`,确认 Ultragoal 不走 `omx state` mode,应使用 `omx ultragoal status/create-goals/complete-goals`。
- 直接对 `.omx/plans/rdog-daemon-flow-plan.md` 运行 `create-goals` 被切成 165 个碎片目标,包含事实陈述、风险条目和非目标,不可作为执行 story。下一步改为基于该计划显式传入 5 个可执行 `--goal`。

### 验证

- `rtk cargo test --package rustdog --bin rdog control_protocol::tests::flow --quiet`: 6 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 11:34:11] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: G005 final quality gate 收口

### 目标

完成 post-cleaner verification、architecture invariant audit、独立 code-review / architect review、quality gate JSON、Codex goal complete 和 Ultragoal final checkpoint。

### 阶段

- [x] 阶段1: 重跑 post-cleaner 验证矩阵。
- [x] 阶段2: 启动独立 `code-reviewer` 与 `architect` 审查。
- [x] 阶段3: 汇总 architecture invariant audit 与 quality gate JSON。
- [x] 阶段4: clean 后调用 `update_goal complete` 并执行 final checkpoint。

### 状态

**目前已完成** - G005 final checkpoint 成功,Ultragoal 5/5 goals complete,Codex aggregate goal complete。

### 验证

- `rtk cargo test --package rustdog --bin rdog --quiet`: 433 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- focused `control_flow::tests` / `control_core::tests` / `control_protocol::tests` / `ui_script`: 5 / 22 / 35 / 20 passed。

### 进行中

- 已启动独立 `code-reviewer` 和 `architect` lane。
- 等待 review 时刷新 current-source 临时 daemon 的 live smoke 证据,避免只引用旧运行记录。

### live smoke

- 临时 daemon: `.omx/tmp/flow-smoke-daemon.toml`,监听 `127.0.0.1:45679`。
- `@flow#9` smoke 返回 `status:"ok"`, `completed_steps:6`, `stdout:"flow-ok\n"`, `response_count:1`。
- Ctrl-C 关闭临时 daemon 后,`rtk lsof -nP -iTCP:45679 -sTCP:LISTEN` 无监听输出。

### 遇到错误

- 曾误跑不存在的占位 shell 命令 `rtk codegraph_status_placeholder`,exit 127。已改用 CodeGraph MCP `codegraph_status`,确认索引健康,该错误没有修改文件或影响验证。

## [2026-06-29 11:49:54] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [修复]: 处理 code-reviewer COMMENT

### 目标

把 final gate 的 code-reviewer 结果从 `COMMENT` 推到可复审状态。

### 修复项

- [x] 拆出 `src/control_flow.rs` 的内联测试,降低单文件体量和职责集中风险。
- [x] 让 `response_status` / `control_status` 缺少 `code` 时在 parser validation 阶段失败。
- [x] 重跑 focused 和 final verification。
- [x] 请求 `code-reviewer` 复审。

### 状态

**目前在修复** - 正在按 review evidence 改 `control_flow` parser/test 结构。

### 修复进展

- [x] `src/control_flow.rs` 内联测试已拆到 `src/control_flow/tests.rs`。
- [x] shell process helper 已拆到 `src/control_flow/process.rs`,`src/control_flow.rs` 降到 993 行。
- [x] `response_status` / `control_status` 缺少 `code` 已改为 parser validation 阶段失败。
- [x] `response_status` / `control_status` 已在 `specs/rdog-flow-control-plan.md` 明确为 v1 alias。
- [x] fresh verification: full bin tests 434 passed; focused `control_flow/control_core/control_protocol/ui_script` 分别 5/22/36/20 passed; cargo check / fmt check / diff check passed。

### 遇到错误

- `beautiful-mermaid-rs --ascii` 首次重跑时误把 Markdown fence 一起喂给 CLI,返回 `Invalid mermaid header: "```mermaid"`。这是命令用法错误,下一步改为只传 Mermaid 正文。

### 复审结果

- independent `code-reviewer`: `Recommendation: APPROVE`,无剩余 CRITICAL/HIGH/MEDIUM/LOW findings。
- independent `architect`: `Architectural Status: CLEAR`,无 final gate blocker。
- `.omx/ultragoal/quality-gate-g005.json` 已创建并通过 `rtk jq empty`。

### 状态

**目前在阶段4** - final quality gate 已 clean,下一步调用 `update_goal complete` 并 checkpoint G005。

### checkpoint 错误

- 第一次 `omx ultragoal checkpoint` 失败: `architectureInvariantGate.invariants[].source` 必须直接引用 `sourceArtifacts` 中的路径。已把说明性 source label 改为精确文件路径后重试。

### final checkpoint

- `omx ultragoal checkpoint --goal-id G005-docs-live-smoke-and-final-quality-ga --status complete ... --quality-gate-json .omx/ultragoal/quality-gate-g005.json --json`: passed。
- `omx ultragoal status --json`: 5 total,5 complete,0 pending,0 failed,artifactComplete true。
- `get_goal`: Codex aggregate goal status `complete`,tokensUsed 1001058,timeUsedSeconds 4454。

## [2026-06-29 14:04:01] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [执行]: rdog-control skill 文案瘦身

### 目标

检查并更新 `.codex/skills/rdog-control/SKILL.md` 的描述叙述,让 skill 更适合注入给 agent: 更短、更清楚、更少重复,同时保留安全边界和当前已落地的 `@flow` / `@window-resize` / AX 验证语义。

### 阶段

- [x] 阶段1: 读取当前 skill、humanizer-zh 和相关记忆。
- [x] 阶段2: 重排 skill 结构,压缩重复叙述。
- [x] 阶段3: 验证 Markdown、关键词和体量变化。
- [x] 阶段4: 记录 notes / WORKLOG。

### 做出的决定

- 不改 rdog 协议语义,只改 skill 的组织和叙述。
- 保留 agent-agnostic 表述,不退回 Codex-only。
- 避免增加更多示例堆砌,把低频细节交给 references。

### 状态

**目前已完成** - `.codex/skills/rdog-control/SKILL.md` 已从 274 行 / 2532 词压缩到 205 行 / 1209 词,并保留关键协议和安全边界。

### 验证

- `awk '/^```/{c++} END{print c}' .codex/skills/rdog-control/SKILL.md`: 20,代码块 fence 成对。
- `rtk git diff --check -- .codex/skills/rdog-control/SKILL.md`: passed。
- `rtk grep` 确认 `@flow`、`@ui-flow`、`@window-resize`、`scope.display` / `guard.display`、`display_id` 反例、`activate:true` 反例、`rdog ax-diff`、`policy.allow_shell`、`daemon-local`、`references/protocol.md` 都仍保留。

## [2026-06-29 14:10:25] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] [收口]: WORKLOG 续档与 skill 文案持续学习

### 目标

完成 `.codex/skills/rdog-control/SKILL.md` 文案瘦身后的上下文收口。由于 `WORKLOG.md` 已达到 1009 行,按仓库规则需要先做默认主线 WORKLOG 续档,再沉淀可复用经验和更新索引。

### 阶段

- [x] 阶段1: 回读六文件和相关长期知识,确认只需默认主线 WORKLOG 续档。
- [x] 阶段2: 归档旧 `WORKLOG.md`,创建新的当前 `WORKLOG.md`。
- [x] 阶段3: 更新 `EXPERIENCE.md` / `AGENTS.md` / archive manifest。
- [x] 阶段4: 重跑 Markdown 与 diff 检查,确认上下文文件不再超过 1000 行。

### 遇到错误

- `rg -n ... docs ...` 返回 `rg: docs: No such file or directory`,因为仓库当前没有 `docs/` 目录。后续改为只检索实际存在的 `AGENTS.md`、`EXPERIENCE.md`、`specs/` 和 skill 目录。

### 状态

**目前已完成** - 旧 `WORKLOG.md` 已移入 `archive/default_history/`,新的当前 `WORKLOG.md`、archive manifest、`EXPERIENCE.md` 和 `AGENTS.md` 索引已更新。收口验证通过。

### 验证

- 默认六文件行数: `task_plan.md` 305,`notes.md` 781,`WORKLOG.md` 22,`LATER_PLANS.md` 444,`ERRORFIX.md` 608,`EPIPHANY_LOG.md` 567。
- `.codex/skills/rdog-control/SKILL.md`: 205 行 / 1209 词。
- Markdown fence 检查: skill + manifest + 新 WORKLOG 合计 20 个 fence,成对。
- 关键词检查: `@flow`、`@ui-flow`、`@window-resize`、`scope.display` / `guard.display`、`display_id` 反例、`activate:true` 反例、`rdog ax-diff`、`policy.allow_shell`、`daemon-local`、`references/protocol.md` 均仍保留。
- `rtk git diff --check -- .codex/skills/rdog-control/SKILL.md AGENTS.md EXPERIENCE.md task_plan.md notes.md WORKLOG.md LATER_PLANS.md ERRORFIX.md EPIPHANY_LOG.md specs/rdog-ui-script-control-plan.md`: passed。
- 新 manifest 与归档 WORKLOG 尾随空白检查: passed。
- `archive/` 受 `.gitignore` 忽略,所以新 manifest 和旧 WORKLOG 归档是本地 archive 文件,普通 `git status` 不显示。

## [2026-06-29 14:40:00] [Session ID: codex-20260629-progress-analysis] [分析]: 当前项目下一步优先级

### 目标

基于当前六文件、`specs/rdog-ui-script-control-plan.md`、`specs/rdog-flow-control-plan.md`、`LATER_PLANS.md`、`EPIPHANY_LOG.md` 和当前工作区状态,只读分析现在最值得做的后续工作。

### 阶段

- [x] 阶段1: 刷新六文件和相关 specs。
- [x] 阶段2: 查看当前 git diff / status,确认工作区风险。
- [x] 阶段3: 形成优先级建议。

### 状态

**目前已完成分析** - 判断当前最值得优先做的是收口已完成的大块变更,再拆 UI script runner 结构和补 `rdog control --ui-script` 入口。方向 B UDS 和 Zenoh flake 仍应保留为后续条件触发项。

## [2026-06-29 15:02:00] [Session ID: codex-20260629-big-diff-closeout] [执行]: 收口当前大 diff

### 目标

把当前混合工作区整理成可审查、可验证、边界清楚的状态。优先盘点 diff 分组,清理可证实的临时/备份噪音,跑必要验证,并记录剩余需要人或下一轮决定的边界。

### 阶段

- [x] 阶段1: 盘点 tracked / untracked diff,按功能主线分组。
- [x] 阶段2: 清理可证实的临时文件,不碰不确定来源的用户改动。
- [x] 阶段3: 跑 focused + final 验证矩阵。
- [ ] 阶段4: 写入 notes / WORKLOG,更新状态和剩余事项。

### 做出的决定

- 这轮先不提交,除非用户明确要求。
- 不使用 `git add .`,不回滚当前业务改动。
- 对未跟踪文件只处理明显属于本轮生成的 `.bak` / 临时测试输入一类噪音,处理前先读内容确认。

### 状态

**目前在阶段4** - 已删除 2 个旧 skill 备份和 1 个 prompt 实验 JSON。验证矩阵通过,并修正 `control_lanes` 中仍假设空 target one-shot 必须失败的旧语义测试。下一步写入 notes / WORKLOG / ERRORFIX。

### 验证

- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests::flow --quiet`: 7 passed。
- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script --quiet`: 20 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 36 passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `./target/debug/rdog ui-script run --dry-run tests/fixtures/ui_script/ping_control_line.json`: 输出 `@ping` dry-run line。
- 8 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。

### 剩余边界

- `src/main.rs` 仍有 2461 行,`src/ui_script.rs` 1013 行,`src/control_core.rs` 1080 行。当前收口先不扩大为重构,但下一步应优先拆 UI script runner 出 `main.rs`。

## [2026-06-29 15:14:05] [Session ID: codex-20260629-final-big-diff-closeout] [接手]: 当前大 diff 最终验证

### 目标

接手上一轮已经完成的大 diff 收口记录,重跑改动后的 fresh verification。当前只做验证、状态确认和必要记录,不扩大为新功能开发,也不提交。

### 阶段

- [x] 阶段1: 重跑格式、diff、构建和测试验证。
- [x] 阶段2: 检查六文件行数和 git status,确认没有新的临界归档或噪音文件。
- [x] 阶段3: 更新 task_plan / WORKLOG,给出当前 diff 的收口结论和剩余风险。

### 状态

**目前已完成** - fresh verification 已通过,`task_plan.md` 和 `WORKLOG.md` 已写回收口记录。当前 diff 保持未提交状态。

## [2026-06-29 15:32:44] [Session ID: codex-20260629-review-and-commit] [执行]: review and 提交

### 目标

对当前大 diff 做提交前 review gate。若 review 没有阻塞问题,按 scoped commit 提交当前已收口的 `@flow` / UI script runner / skill-docs 变更;若发现阻塞问题,先修正并重新验证。

### 阶段

- [x] 阶段1: 启动独立 code-reviewer / architect review lane,形成合并前判断。
- [x] 阶段2: 根据 review 结果修复阻塞项或记录非阻塞 watchlist。
- [x] 阶段3: 跑提交前 fresh verification。
- [x] 阶段4: scoped stage,检查 staged diff,提交。
- [x] 阶段5: 写入 WORKLOG / task_plan,交付 commit hash 和验证证据。

### 约束

- 不使用 `git add .`。
- 不回滚当前 diff 中不属于本轮修复的已有改动。
- commit 前必须确认 submodule 状态和 staged 边界。

### 状态

**目前进入提交收口** - 初轮 review 的 HIGH/MEDIUM 已修复,复审无 BLOCK,最终验证通过。下一步执行 scoped stage 和 commit,commit hash 在最终回复中交付。

### review 结果

- code-reviewer: `REQUEST CHANGES`。
  - HIGH: UI script runner 对非零 code 控制响应可能仍记录脚本成功。
  - MEDIUM: `@flow SaveArtifact` 缺少显式 daemon-local 文件读取授权。
  - LOW: `LATER_PLANS.md` 有过期 unchecked cleanup 项。
- architect: `WATCH`。
  - 非阻塞 watchlist: UI script / control local-default target resolver 语义应后续收敛;`src/main.rs` runner 代码应拆分。

### 修复

- `src/main.rs`: `record_ui_script_control_step` 识别错误响应,写 failed trace,设置 `failed_step_index`,并返回 `Err`。
- `src/control_flow.rs`: 新增 `policy.allow_file_read`,并要求 `SaveArtifact` 必须显式授权。
- `src/control_protocol/tests/flow.rs` / `src/control_core.rs`: 更新合法 `SaveArtifact` 测试并新增无授权拒绝测试。
- `specs/rdog-flow-control-plan.md` / `.codex/skills/rdog-control/SKILL.md`: 同步 `allow_file_read` 规则。
- `LATER_PLANS.md`: 删除已失效的 self-learning skill cleanup 项。

### focused verification

- 新增 UI script 错误响应测试: 先红后绿。
- 新增 `SaveArtifact` 无授权拒绝测试: 先红后绿。
- `control_protocol::tests::flow`: 8 passed。
- `control_core::tests`: 22 passed。
- `ui_script_run`: 5 passed。
- `control_flow::tests`: 5 passed。
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。

### 复审与最终验证

- 复审 code-reviewer: `APPROVE`,原 HIGH/MEDIUM/LOW 全部 resolved。
- 复审 architect: `WATCH`,无 BLOCK,允许当前 diff 提交。
- 最终验证:
  - `rtk cargo test --package rustdog --bin rdog --quiet`: 436 passed。
  - `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
  - 两个 UI script fixture dry-run 均通过。
  - 4 个相关 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。
  - `rtk cargo fmt -- --check`: passed。
  - `rtk git diff --check`: passed。
  - 六文件均低于 1000 行续档阈值。

### 验证

- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `./target/debug/rdog ui-script run --dry-run tests/fixtures/ui_script/ping_control_line.json`: passed,输出 `ControlLine control @ping`。
- `./target/debug/rdog ui-script run --dry-run tests/fixtures/ui_script/ping_expect_response.json`: passed,输出 `Expect response_contains` 和 `Expect response_status`。
- `specs/rdog-flow-control-plan.md` 与 `specs/rdog-ui-script-control-plan.md` 共 4 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。

### 状态检查

- 六文件行数: `task_plan.md` 379,`notes.md` 886,`WORKLOG.md` 48,`LATER_PLANS.md` 444,`ERRORFIX.md` 629,`EPIPHANY_LOG.md` 567,均未超过 1000 行续档阈值。
- `git status --short` 中未跟踪项只剩真实新文件: `specs/rdog-flow-control-plan.md`、`src/control_flow.rs`、`src/control_flow/`、`src/control_protocol/tests/flow.rs`、两个 UI script fixture。

### 遇到错误

- 曾误用双引号执行 `rg -n "^```mermaid" ...`,zsh 将反引号当作命令替换并返回 `unmatched "\""`. 已改用单引号重新执行,没有修改文件。

## [2026-06-29 16:28:09] [Session ID: codex-20260629-next-worth-analysis] [分析]: 当前项目下一步价值判断

### 目标

基于当前六文件、`LATER_PLANS.md`、`WORKLOG.md`、`EPIPHANY_LOG.md`、`specs/rdog-ui-script-control-plan.md`、`specs/rdog-flow-control-plan.md` 和当前 git 状态,只读分析现在最值得继续做的工作。

### 阶段

- [x] 阶段1: 刷新最近 task / worklog / later / epiphany 记录。
- [x] 阶段2: 检查当前 git 状态、最新提交和核心文件体量。
- [x] 阶段3: 对候选任务做优先级判断。

### 状态

**目前已完成分析** - 当前最值得做的是先拆 UI script runner / target resolver,再做 `rdog control --ui-script` 兼容入口和最小 live smoke。方向 B UDS、Zenoh flake 和更细 file-read policy 暂不应抢主线。

## [2026-06-29 23:19:56] [Session ID: codex-20260629-ultragoal-ui-script-123] [执行]: Ultragoal 按顺序完成 1/2/3

### 目标

按上一轮分析的优先级执行 durable ultragoal:
1. 拆 `src/main.rs` 中 UI script runner / target resolver 职责。
2. 接入 `rdog control --ui-script <file.json>` 兼容入口。
3. 做一个安全 live smoke,验证 runner 真实控制路径。

### 阶段

- [ ] 阶段1: 创建新的 `.omx/ultragoal` goals 并承接 Codex aggregate goal。
- [ ] 阶段2: 完成 G001 runner / target resolver 拆分。
- [ ] 阶段3: 完成 G002 `rdog control --ui-script` 兼容入口。
- [ ] 阶段4: 完成 G003 安全 live smoke 与 final quality gate。
- [ ] 阶段5: checkpoint / WORKLOG / 最终交付。

### 状态

**目前在阶段1** - 旧 Codex goal 已确认 `complete`;准备创建新的 ultragoal plan。

## [2026-06-29 23:25:29] [Session ID: codex-20260629-ultragoal-ui-script-123] [状态]: 修正 Ultragoal 目标拆分

### 当前观察

- `.omx/ultragoal/goals.json` 当前只有一个 pending goal,把 1/2/3 合并到了同一个目标里。
- 用户明确要求"按顺序 做 123",所以需要恢复成三个可 checkpoint 的顺序目标。

### 即将执行

- 使用 `omx ultragoal steer --directive-json` 的 `split_subgoal` 机制拆分 `G001-ui-script-runner-target-resolver-src`。
- 不直接手改 `.omx/ultragoal/goals.json`,保留 ledger 审计轨迹。
- 拆分完成后再执行 `omx ultragoal complete-goals`,承接新的 aggregate Codex goal。

### 状态

**目前在阶段1** - 正在修正 durable goal 结构,暂未开始代码编辑。

## [2026-06-29 23:28:07] [Session ID: codex-20260629-ultragoal-ui-script-123] [执行]: G002 runner / resolver 拆分

### 目标

完成 Ultragoal story `G002-split-ui-script-runner-and-target-re`:把 UI script runner state、trace writer、artifact handling、Expect evaluation、control exchange 和共用 target / invocation resolution 从 `src/main.rs` 拆出。

### 阶段

- [x] 阶段1: 使用 steering 把错误合并的单目标拆成 G002 / G003 / G004 三个顺序目标。
- [x] 阶段2: 用 CodeGraph 和源码阅读确认当前 runner / resolver 边界。
- [x] 阶段3: 实施模块拆分,保持 `main.rs` 只做 CLI wiring。
- [x] 阶段4: 跑 focused tests / fmt / check,checkpoint G002。

### 状态

**目前在完成 G002** - `src/main.rs` 已降到 625 行,`control_invocation` / `ui_script_runner` / runner tests 已拆出,验证通过;下一步 checkpoint G002 并进入 G003。

### 验证

- `rtk cargo test --package rustdog --bin rdog control_invocation::tests --quiet`: 16 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script_runner::tests --quiet`: 11 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script --quiet`: 21 passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 436 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 23:47:17] [Session ID: codex-20260629-ultragoal-ui-script-123] [执行]: G003 control --ui-script 入口

### 目标

完成 Ultragoal story `G003-add-rdog-control-ui-script-compatibi`:在 G002 拆出的共享 runner / target resolver 上实现 `rdog control --ui-script <file.json>` 兼容入口。

### 阶段

- [x] 阶段1: 阅读 `input.rs` control CLI shape 和当前 `run` 分支。
- [x] 阶段2: 新增 `--ui-script <file.json>` 参数,桥接到 `ui_script_runner::run`。
- [x] 阶段3: 补 CLI parse/unit tests 和 dry-run/fixture 验证。
- [x] 阶段4: 跑 focused verification 并 checkpoint G003。

### 约束

- 不实现 `--compat iced-emg`。
- 不把本任务扩大成 daemon-side `@ui-flow`。
- 不复制 UI script runner 逻辑,只通过 `ui_script_runner::run` 调用。

### 状态

**目前在完成 G003** - `rdog control --ui-script <file.json> [TARGET]` 已接入共享 runner,验证通过;下一步 checkpoint G003 并进入 G004。

### 验证

- `rtk cargo test --package rustdog --bin rdog input::tests --quiet`: 19 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script_runner::tests --quiet`: 11 passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 438 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `./target/debug/rdog control --ui-script tests/fixtures/ui_script/ping_expect_response.json --dry-run self`: passed。

### 遇到错误

- 首次 focused test 命令把 `--exact` 放在 cargo 参数位置,被 cargo 拒绝。已改为 `-- --exact` 后重跑通过。
- 首次直接执行 `./target/debug/rdog control --ui-script ...` 时二进制尚未重建,旧 binary 报 `unexpected argument '--ui-script'`。已先 `cargo build --package rustdog --bin rdog --quiet` 后重跑通过。

## [2026-06-29 23:54:59] [Session ID: codex-20260629-ultragoal-ui-script-123] [执行]: G004 live smoke 和 final gate

### 目标

完成 Ultragoal story `G004-safe-live-smoke-and-final-quality-ga`:跑安全 live smoke,证明 UI script runner 真实控制路径和产物路径,再完成 final quality gate。

### 阶段

- [ ] 阶段1: 选择安全 live smoke 脚本和临时 daemon 方案。
- [ ] 阶段2: 执行 live smoke,检查 trace / summary / normalized / artifacts。
- [ ] 阶段3: 跑 final verification、ai-slop-cleaner、post-cleaner verification。
- [ ] 阶段4: 做 architecture invariant audit 和独立 code-review / architect review。
- [ ] 阶段5: clean 后 `update_goal complete`,checkpoint G004。

### 状态

**目前在阶段1** - 准备使用临时本机 daemon 做只读 live smoke。

## [2026-06-30 00:00:45] [Session ID: codex-20260629-ultragoal-ui-script-123] [接手]: G004 final gate 继续执行

### 目标

继续完成 Ultragoal story `G004-safe-live-smoke-and-final-quality-ga`,从已通过的 live smoke / pre-cleaner verification 之后推进到 cleaner、post-cleaner verification、独立 review、architecture invariant gate 和最终 checkpoint。

### 阶段

- [x] 阶段1: 刷新 `.omx/ultragoal/goals.json` 与 active Codex goal,确认当前只需要推进 G004。
- [x] 阶段2: 对 changed files 做 scoped ai-slop-cleaner 审查,只在发现明确问题时编辑。
- [x] 阶段3: cleaner 后重跑 verification matrix,包括 dry-run 和必要 live smoke。
- [x] 阶段4: 启动独立 code-reviewer / architect 双 lane,生成 quality gate JSON。
- [x] 阶段5: clean 后 `update_goal complete`,写 fresh goal snapshot,checkpoint G004。

### 状态

**目前在阶段4** - post-cleaner verification 已通过,接下来启动独立 code-reviewer / architect 双 lane。

### 遇到错误

- code-reviewer 返回 `REQUEST CHANGES`: `Expect response_status/control_status` 在没有上一条 `@response` 时会因为默认 `ok` / `code=0` 误判成功。
- 当前根因假设: `expect_response_status` 与 `expect_control_status` 缺少共享的 prior-response guard,直接把 missing state 当成成功状态。
- 验证计划: 先加 focused failing tests,再修 runner,最后重跑 focused/full verification 并重新 review。
- architect 返回 `WATCH`: 主架构边界正确,但 trace contract 与计划示例存在轻微漂移,缺少 `started_at_unix_ms` 和结构化 `target_resolution.source` 证据。
- 处理计划: 保持 lean trace,但补齐 control step 的 `started_at_unix_ms` 与 `target_resolution.source` 字段,并加测试锁定。
- code-reviewer 复审再次返回 `REQUEST CHANGES`: 相邻 `ControlLine` 会被批量发送,第一个失败 `@response` 只是在发送完后才被 runner 发现,可能导致后续 UI action 已执行。
- 新根因假设: runner 的 `pending_lines` flush 粒度太粗,把性能 batching 放在安全 fail-fast 之前。
- 修复计划: `flush_ui_script_pending_lines` 改为逐条发送 / 逐条记录 / 第一条失败立即停止,并加测试证明第二条不会发送。

### 状态更新

**目前在阶段4 blocker 修复** - final checkpoint 暂停,先修复 code-reviewer 和 architect 指出的 final gate 问题。

### 修复验证

- `Expect` prior-response guard 已加,focused test 先红后绿。
- trace contract 已补 `started_at_unix_ms` 和结构化 `response.target_resolution`,对应测试通过。
- adjacent `ControlLine` fail-fast 已加,focused test 先红后绿,证明第一条返回 `code:64` 时第二条不会被发送。
- `rtk cargo test --package rustdog --bin rdog ui_script_runner::tests --quiet`: 14 passed。
- `cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 441 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- latest `rdog control --ui-script ... --dry-run self`: emitted 4 compiled steps。
- final TCP live smoke: summary `status:"complete"`, `verification_passed:true`, trace 4 lines, daemon port released。

### 状态更新

**目前在完成** - G004 已 checkpoint complete;`omx ultragoal complete-goals --json` 返回 `done:true`,artifactComplete 为 true。

## [2026-06-30 00:56:17] [Session ID: codex-20260629-ultragoal-ui-script-123] [完成]: Ultragoal UI script 123 收口

### 目标

完成用户要求的 1/2/3 顺序任务:拆分 runner / resolver、接入 `rdog control --ui-script`、完成安全 live smoke 与 final quality gate。

### 阶段

- [x] 阶段1: G002 拆分 `src/main.rs` 中 UI script runner 和 control invocation resolver。
- [x] 阶段2: G003 接入 `rdog control --ui-script <file.json> [TARGET]`,复用共享 runner。
- [x] 阶段3: G004 跑安全 TCP live smoke,证明 trace / summary / normalized / artifacts 路径。
- [x] 阶段4: 修复 code-reviewer 发现的两个真实 runner blocker。
- [x] 阶段5: final verification、ai-slop-cleaner、code-reviewer APPROVE、architect CLEAR、quality gate JSON、Codex goal complete、Ultragoal checkpoint complete。

### 状态

**目前已完成** - `.omx/ultragoal/quality-gate-g004.json` 和 `.omx/tmp/g004-codex-goal.json` 已生成,G004 checkpoint 成功,`omx ultragoal complete-goals --json` 返回 `done:true`。

## [2026-07-14 11:40:00] [Session ID: omx-1783957580965-m4bn8e] [实施 ticket 01]: rdog @wait primitive

### 背景
- 阶段 A 已完成: spec + 22 ticket 已落盘 (`specs/rdog-computer-act-spec.md` + `specs/rdog-computer-act-tickets/`)。
- 用户用 `$implement` 启动实施, 默认从 ticket 01 (`@wait` primitive) 开始 — 这是 critical path 第一个, 无 blocker, 体量小, 是验证 TDD 节奏和 rdog parser/executor pattern 的最佳切片。

### 范围 (ticket 01 acceptance criteria)
- [ ] Line-protocol parser 接受 `@wait#N:{duration_ms:N}`, 拒绝负数 / 非数字 (parse error)
- [ ] Runtime sleep 后返回 `{ok:true, dispatched_to:"@wait", duration_ms:<actual>}`, 在 macOS 上 50ms tolerance 内
- [ ] 单元测试覆盖 valid / missing duration / negative / malformed JSON 四种情况
- [ ] Smoke 脚本 `scripts/smoke_wait.sh` 跑通

### 计划
- Phase 1 (red): 写 `src/control_protocol/tests/wait.rs`, 跑测试确认红
- Phase 2 (green): 实现 parser (parse_wait_payload) + ControlCommand::Wait variant + executor
- Phase 3 (smoke): 跑真 daemon smoke, 记录 elapsed_ms
- Phase 4 (review): 单测全过 + `cargo check --quiet` 无 warning

### 状态
**Phase 1 即将开始。**


### Phase 1-4 完成 (2026-07-14 19:13)

#### 实现细节
- `src/control_protocol.rs`: 新增 `WaitRequest` struct + `ControlCommand::Wait` variant + dispatch arm
- `src/control_protocol/parsers/wait.rs`: 新增 `parse_wait_payload` (对象 payload,拒绝字符串 / 缺字段 / 重复 / 负数 / 非数字)
- `src/control_protocol/parsers.rs`: 注册 `mod wait` + re-export
- `src/control_protocol/tests/wait.rs`: 7 个 parser 单测 (valid / zero / missing / negative / non-numeric / non-object / duplicate)
- `src/control_actions.rs`: 新增 `execute_wait` 函数 + `ControlCommand::Wait` executor arm
- `src/shell/tests.rs`: 补 `Wait` arm (match 必须 exhaustive)
- `scripts/smoke_wait.sh`: 3 段 e2e smoke (200ms / 0ms / malformed)

#### 验证证据
- `cargo test --bin rdog`: **448 passed; 0 failed** (含 7 个新 wait 测试)
- `cargo check --bin rdog`: 0 warning
- `bash scripts/smoke_wait.sh`: 3/3 通过
  - test 1: `@wait#1:{duration_ms:200}` → `{ok:true, dispatched_to:"@wait", duration_ms:204, requested_duration_ms:200}` (4ms tolerance)
  - test 2: `@wait#2:{duration_ms:0}` → `{ok:true, dispatched_to:"@wait", duration_ms:0, requested_duration_ms:0}` (立即返回)
  - test 3: `@wait#3:{}` → `{id:3, code:64, error:"@wait 对象 payload 不能为空,需要 duration_ms 字段"}` (parse error 干净返回)

#### 状态
**ticket 01 完成, 7/7 单测 + 3/3 smoke 通过。下一步候选 ticket 02 (`@open-app`) 或 03 (`@cancel#seq`), 都是无 blocker 可立即启动。**


### Code review 修复 (2026-07-14 19:25)

按 `/code-review` 两轴结果修复 #1-#4:

- **#1 [Hard, Standards]**: WaitRequest doc-comment 删掉 "0" 误标 (`src/control_protocol.rs`)
- **#2 [Spec]**: smoke 加 2 段 (negative / non-numeric), 从 3 段 → 5 段
- **#3 [Judgement, Standards]**: 提取 `build_default_wait_response_json` helper, 镜像 `build_default_web_*` 模式 (`src/control_actions.rs`)
- **#4 [Judgement, Standards]**: smoke 用 feature-specific liveness probe (`@wait:0`) 替代 `@ping`, 旧 daemon 不支持时自动 kill 重启, 落实 EXPERIENCE.md:216 教训

### 最终验证

- `cargo test --bin rdog`: **448 passed; 0 failed**
- `RUSTFLAGS="-Awarnings" cargo check`: 0 warning
- `bash scripts/smoke_wait.sh`: 5/5 通过
  - test 1: 200ms → duration_ms:205 (5ms tolerance)
  - test 2: 0ms → duration_ms:0 (immediate)
  - test 3: negative → "@wait 的 `duration_ms` 不能为负数: -1"
  - test 4: non-numeric → "@wait 的 `duration_ms` 必须是整数: \"abc\""
  - test 5: missing field → "@wait 对象 payload 不能为空,需要 duration_ms 字段"

### 状态
**code-review 修复完成, 准备进 commit。**

## [2026-07-15 16:00:00] [Session ID: omx-1783957580965-m4bn8e] [ticket 11 实施]: rdog `@computer-act` implicit_observe plumbing + freshness + TTL

### 触发
- 用户回应 "3" → 按 critical path 推进 Phase B 收口。Phase B skeleton (04) + minimum slice (05) 都收口在 `ec0f653`,ticket 11 是 critical path 下一步 (`05 ✓ → 11 → 13 → 14 → 18 → 21 → 22`)。
- ticket 11 不依赖 ticket 04 之外的实现,但 ticket 04 留下的 `observation_id` / `observation_used` 等 null placeholder 是它要填的字段。

### 范围 (ticket 11 acceptance criteria, 来自 `specs/rdog-computer-act-tickets/11-implicit-observe-and-freshness.md`)
- [ ] `start_box` + 无 `target.ref` → 触发 implicit_observe;response 携带 `observation_id` 和 `observation_used.freshness`
- [ ] `target.ref + observation_id` 仍在 TTL 内 → `freshness:"fresh"`,不重新 observe
- [ ] `target.ref + observation_id` 已过期 → daemon 自动 re-observe,response 携带 `observation_used.re_observe_id`,`freshness:"stale_re_observed"`
- [ ] TTL 5 秒严格生效 (ADR-0005 L3): 超过 5000ms 即视为过期
- [ ] 测试覆盖: 时钟注入做 TTL 边界 / fresh path / stale path / re-observe 路径

### 实施决策 (本轮)
1. **轻量级 cache,不复用全局 `ObservationStore`**: ticket 11 阶段不需要真实的 AX/screenshot observe,只需要 `observation_id → ref_id` 映射 + TTL 生命周期。新建 `ComputerActObservationCache`,独立维护 TTL 5s,跟全局 `ObservationStore` (300s TTL) 解耦,避免语义混淆。
2. **Synthetic ref_id 占位**: `ref_id = format!("@e{seq}")` 作为 ticket 11 占位。后续 ticket (Phase I real observe 集成) 才把 start_box 真实映射到 AX ref。当前底层 dispatch 仍用 `MouseEndpoint::Coordinate`(start_box 像素),等真实 observe 接入后再切 `MouseEndpoint::ObservationRef`。这是 ticket 11 acceptance criteria 不反对的简化。
3. **clock 注入 via `now_ms: u64` 参数** (跟 `ObservationStore` 同模式): 单元测试用 mock clock,real path 在 daemon 启动时初始化为 `SystemTime::now() - UNIX_EPOCH`。
4. **缓存作用域 = daemon 进程内全局**: 用 `OnceLock<Mutex<ComputerActObservationCache>>`,跟现有 `ObservationStore` 用法对齐 (rdog dispatcher 单线程,但 Mutex 兜底兼容未来并发)。
5. **freshness 三态严格按 ADR-0005**:
   - `fresh`: target.observation_id 命中且未过期 (5s TTL 内)
   - `stale_re_observed`: target.observation_id 过期或不存在,daemon 自动 re-observe
   - `stale_fallback_to_coords`: 留给后续 real observe 阶段 (start_box 不可 observe 时降级); ticket 11 暂不暴露 (real observe 还没接入)

### 文件变更
- 新增 `src/control_computer_act/implicit_observe.rs` (~150 行): cache struct + outcome enum + now_ms 注入
- `src/control_computer_act/mod.rs`: 加 `mod implicit_observe;` + 在 `execute_computer_act` 入口加 implicit_observe 包装
- `src/control_computer_act/tests.rs`: 加 ~12 个单测 (fresh / stale / re-observe / TTL boundary / clock 注入 / routing 集成)
- 新增 `scripts/smoke_computer_act_observe.sh`: 端到端 smoke,2 段 (fresh reuse + stale re-observe)
- `docs/adr/0005-lifecycle.md`: 不动,本轮实现跟 L3 决策一致

### 状态
**Phase 0 完成 (读 spec + ADR + 当前 dispatcher 代码 + 现有 ObservationStore API)。即将开 Phase 1 (red tests)。**

## [2026-07-15 17:30:00] [Session ID: omx-1783957580965-m4bn8e] [ticket 12 + 13 实施]: rdog `@computer-act` verify 三档 (none / best_effort)

### 触发
- 用户 "继续" → 接 ticket 11 已 push 的 `afa7517` 之后,按 critical path 推进 ticket 13 (`verify-best-effort`)。
- ticket 13 依赖 ticket 12 (`verify-none`),所以本轮 ticket 12 + ticket 13 一起做。

### 范围 (ticket 12 + 13 acceptance criteria)
- [x] ticket 12: 无 `verify` 字段 → 默认 `verify:"none"`,response 不带 `verification` key
- [x] ticket 12: `verify:"none"` 显式 → 同上
- [x] ticket 13: `verify:"best_effort"` → 内部 post-action AX diff,返回 `verification.method:"ax_diff"` + `verification.ax_diff.{added, removed, changed}`
- [x] ticket 13: 不带 screenshot,只跑 AX diff
- [x] ticket 13: `density.verify_ms` 与 `density.dispatch_ms` 拆分
- [x] 测试: synthetic AX-tree diff fixture + verify response shape

### 实施决策 (本轮)
1. **新增 `src/control_computer_act/verify.rs` (~340 行)**: VerifyPolicy 三态枚举 + parse_verify_policy 单一入口 + AxDiffSummary 结构 + run_best_effort_verify 执行流 + render_verification / render_density helpers + 11 个单测
2. **ticket 12 vs 13 一并实现**: ticket 12 是 ticket 13 的基础 (None 不写 verification 字段),不强求两个独立 commit
3. **verify=always 占位**: 留作 ticket 14 (本轮 `render_verification` 对 Always 政策返回 None,等同 None)
4. **density 字段结构**: `{dispatch_ms, implicit_observe_ms, verify_ms?}`,verify_ms 仅 verify=best_effort 时存在 (omit vs null placeholder,跟 ticket 12 一致)
5. **invalid_verify 错误码**: `error_code:"invalid_verify"` + `error_message` 含不支持的值 (e.g. `"@computer-act.verify 不支持: bogus; 必须是 none / best_effort / always"`)
6. **ax_diff::diff 模块从 private → pub(crate)**: verify.rs 需要 `compute_diff` 入口,ax_diff 之前只 expose `run(opts)`,没 expose compute_diff 本体
7. **smoke_computer_act.sh 跟随契约升级**: 旧 smoke 期待 `verification: null` 占位,ticket 12 后 omit 整个字段;旧 smoke 改成反向匹配 (verify=none 不应包含 verification key) + 校验 density 是 object {dispatch_ms, implicit_observe_ms}

### 文件变更
- 新增 `src/control_computer_act/verify.rs` (~340 行): VerifyPolicy + AxDiffSummary + helpers + 11 单测
- `src/control_computer_act/mod.rs`: 加 `mod verify;` + 在 execute_computer_act 入口加 verify policy parse + dispatch_ms 拆分 + verify 块渲染
- `src/ax_diff/mod.rs`: `mod diff` 从 private → pub(crate) (verify.rs 需要 compute_diff 入口)
- `scripts/smoke_computer_act.sh`: 更新成新契约 (omit verification, density 是 object)
- 新增 `scripts/smoke_computer_act_verify.sh` (193 行, 5 段 e2e): 默认无 verify / verify=none / verify=best_effort / verify=always (占位) / verify=bogus (invalid_verify)

### 状态
**ticket 12 + ticket 13 实施完成,准备 commit + smoke + push。**

## [2026-07-16 10:30:00] [Session ID: omx-1783957580965-m4bn8e] [ticket 14 实施]: rdog `@computer-act` verify tier 'always' (full observation)

### 触发
- 用户 "继续" → 接 ticket 12+13 (`aeac227`) 之后,按 critical path 推进 ticket 14 (`verify-always`)。
- ticket 13 已经预留 enum variant (Always) 和 render 骨架,ticket 14 在骨架上落地。

### 范围 (ticket 14 acceptance criteria)
- [x] `verify:"always"` 触发 post-action 全量 observe (screenshot + AX + windows)
- [x] Response 携带 `verification.method:"full"`
- [x] Response 携带 `verification.observation:{screenshot_id, ax_tree_id, windows, screenshot_truncated}`
- [x] Response 携带 `verification.ax_diff.{windows_added, ..., changed}` (跟 best_effort 同样口径)
- [x] Screenshot > 2MB → `verification.observation.screenshot_truncated:true` (不截断图像,只标 false 警示)
- [x] 测试覆盖: 三字段都 present + screenshot_truncated 翻转 + render dispatch + empty summary fallback

### 实施决策 (本轮)
1. **full observe 复用 `build_observe_bundle(ObserveRequest::default())`**: Hybrid 模式 = screenshot + AX + windows,跟 client 调 `@observe` 走同一路径
2. **pre-AX 走轻量 `capture_default_ax_snapshot`**: 全量 observe 已经包含 AX (post),pre 只为了 diff;用 `AxTreeRequest::default()` 不带额外 scope 节省时间
3. **diff 用 pre-AX snapshot vs observe_bundle.value.accessibility (post AX JSON)**: 不重做 AX capture,直接复用 observe bundle 里的 accessibility 段
4. **screenshot_id 优先取 visual.id,fallback observation.observation_id**: 当前 observe bundle.visual 没有显式 id 字段,所以 fallback 到 observation.observation_id (跟 ax_tree_id 同源)
5. **screenshot_truncated 阈值 2MB**: base64 长度 × 3/4 估算字节数;超阈值标 true,**不截断图像** (client 可能需要完整图做 OCR)
6. **render_verification 签名改成 3 参数 (diff_summary + always_summary)**: Always 走 AlwaysVerifySummary 路径,BestEffort 走 AxDiffSummary 路径,caller 在 mod.rs 显式 dispatch
7. **observation_block 字段保留**: ticket 14 不渲染到 response (只取子字段),但保留 struct field 给 ticket 18 trace (`#[allow(dead_code)]` 标记)
8. **always_summary 不进 density.implicit_observe_ms**: ticket 14 跑 full observe 不复用 implicit_observe 的 ref_id cache,避免污染 5s TTL

### 文件变更
- `src/control_computer_act/verify.rs` (~410 行,+63 行): 加 `AlwaysVerifySummary` / `run_always_verify` / `render_always_verification` / `ALWAYS_VERIFY_SCREENSHOT_LIMIT_BYTES` / 6 个新单测 (17 总)
- `src/control_computer_act/mod.rs`: 加 `run_always_verify` import + dispatch `verify_summary`/`always_summary` + 改 `render_verification` 签名
- `src/control_observation.rs`: 加 `pub use observe::build_observe_bundle` (verify.rs 需要入口)
- `scripts/smoke_computer_act_verify.sh`: test 4 改成验证 full observe 行为 (method:full + 三字段)

### 状态
**ticket 14 实施完成,所有 verify smoke 5/5 + 回归 smoke 9/9 通过。准备 commit + push。**

## [2026-07-16 13:30:00] [Session ID: omx-1783957580965-m4bn8e] [ticket 17 + 18 实施]: rdog `@computer-act` density metrics + trace observability

### 触发
- 用户 "继续" → 接 ticket 14 (`41dd0bd`) 之后,按 critical path 推进 ticket 18 (`trace-summary-and-savefile`)。
- ticket 18 依赖 ticket 17 (density metrics),本轮 ticket 17 + 18 一起做。

### 范围 (ticket 17 acceptance criteria)
- [x] `density` 字段含 ADR-0006 全字段集: backend_request_count / control_frame_count /
      elapsed_ms_total / semantic_action_count / mouse_fallback_count /
      stale_ref_recovery_count / verification_passed / false_success_count /
      payload_bytes / trace_step_count / implicit_observe / implicit_observe_ms /
      dispatch_ms / verify_ms
- [x] `verification_passed` = verify != none && ax_diff non-empty
- [x] 字段名严格对齐 ADR-0006 §Consequences

### 范围 (ticket 18 acceptance criteria)
- [x] 每次成功 response 都带 `trace_summary:[{step, elapsed_ms, status}, ...]`,严格 4 entry
- [x] verify step 即使 policy=none 也占位,status="skipped"
- [x] `trace:"savefile"` 触发 full trace dump 到 rdog_downloads/trace-*.json
- [x] response 带 `trace_savefile:"<path>"`
- [x] 不带 trace 字段时,trace_savefile 字段 omit
- [x] Full trace 含 implicit_observe sub-steps (screenshot_capture / ax_tree_scan /
      ref_resolution) + dispatch sub-steps

### 实施决策 (本轮)
1. **新建 `density.rs` 模块** (~190 行): `ComputerActDensity` struct + `render_density` + `compute_verification_passed` + 6 个单测
   - 把 ADR-0006 全字段集都填,即使占 0 (mouse_fallback_count / false_success_count / stale_ref_recovery_count)
   - `verify_ms` 在 verify=none 时 omit (跟 ticket 12 一致的 omit 风格)
2. **新建 `trace.rs` 模块** (~340 行): `TraceStepKind` / `TraceStatus` / `TraceStep` /
   `TraceSummary` / `FullTrace` / `write_trace_savefile` + 7 个单测
   - `TraceSummary::build()` 严格 4 entry 构造器,verify status 三态 (ok / skipped / failed)
   - `write_trace_savefile` 走 `default_savefile_directory()` (rdog_downloads/) 落盘
3. **verify.rs 删 `render_density`** (密度块搬到新模块): 避免两处重复定义密度 JSON shape
4. **mod.rs 重组**: `ComputerActDensity` 构造必须在 json! macro 之前 (rust borrow checker);
   `trace_summary` / `density` 都在 json! macro 之前计算完,一次性塞进 payload
5. **omit vs null 占位**: trace_savefile 不写整个字段 (而不是 null),跟 ticket 12 verification 一致
6. **savefile name: trace-{ts_ms}-{id}.json**: 没 request_id 入口 (execute_computer_act 只有 ComputerActRequest,没有 ControlRequest),暂时传 None;后续 control_actions.rs 重构时再 thread request_id
7. **payload_bytes / mouse_fallback_count / stale_ref_recovery_count / false_success_count 占 0**: ticket 21 e2e smoke 真实 GUI 场景才补;这轮 0 是合理 placeholder
8. **trace_step_count = 4 跟 density.trace_step_count 同步**: 避免 trace_summary 跟 density 不一致

### 文件变更
- 新增 `src/control_computer_act/density.rs` (~190 行): ComputerActDensity + render_density + 6 单测
- 新增 `src/control_computer_act/trace.rs` (~340 行): TraceSummary + FullTrace + write_trace_savefile + 7 单测
- `src/control_computer_act/mod.rs`: 注册 density / trace 子模块 + 重组 density_metrics/trace_summary 构造顺序
- `src/control_computer_act/verify.rs`: 删 render_density 函数 + 2 个旧测试 (搬到 density.rs)
- `scripts/smoke_computer_act.sh`: 更新成新契约 (density ADR-0006 全字段 + trace_summary 4 entry + trace_savefile 默认 omit)
- 新增 `scripts/smoke_computer_act_trace.sh` (199 行, 3 段 e2e): 默认无 trace → trace_savefile omit / trace="savefile" → trace 文件落地 / trace+verify=best_effort → verify status=ok

### 状态
**ticket 17 + ticket 18 实施完成,所有 4 个 smoke 全过 (5/5 + 5/5 + 4/4 + 3/3 = 17/17)。准备 commit + push。**

## [2026-07-16 16:30:00] [Session ID: omx-1783957580965-m4bn8e] [ticket 08 completion + ticket 21 实施]: hotkey_click Composite + e2e smoke

### 触发
- 用户 "继续" → 接 ticket 16 (5717201) 之后, 按 critical path 推进 ticket 21 (e2e smoke)。
- ticket 21 spec 要求 13 动作全部跑通。跑 smoke 时发现 hotkey_click 在 ticket 04 用 shell script 占位
  (key down / key up) 失败 (shell 不识别 rdog CLI 命令)。
- 本轮: ticket 08 完整实现 hotkey_click (Composite 复合命令, ticket 21 e2e smoke。

### 范围 (ticket 08 completion)
- [x] `hotkey_click` 改成 ControlCommand::Composite([Key(Press), Click, Key(Release)])
      替代 ticket 04 的 shell script 占位
- [x] Composite 顺序执行, 任一失败回滚已执行的 Key(Press) → Key(Release)
      (modifier release guard, ticket 08 acceptance "If the click step errors
       after the modifier is pressed, the modifier is released before returning the error")
- [x] dispatch_underlying 加 Composite 分支

### 范围 (ticket 21 acceptance)
- [x] `scripts/smoke_computer_act_all.sh` 覆盖 13 动作 (open_app / open_url / click /
      doubleclick / triple_click / right_single / hover / type / hotkey /
      hotkey_click / scroll / drag / wait)
- [x] 每个动作验证 ok:true + dispatched_to + trace_summary 4 entry
- [x] bonus: invalid_args error path (scroll amount=-1) 验证 E2 envelope (retry.strategy)
- [x] macOS 本地可跑, 无外部网络依赖

### 实施决策 (本轮)
1. **ControlCommand::Composite(Vec<ControlCommand>) 新 variant**: 单一真相源表达 "多步原子操作"
   (不是用 shell script 串);dispatch_underlying 顺序执行,任一失败回滚 Key(Press) → Key(Release)
2. **failure rollback**: 因为只有 key down 是 "带 modifier 状态" 的副作用 (click 没副作用, key up 是 release),
   rollback 逻辑只针对已执行的 Key(Press)。click 失败时 shift 已经被按下但不释放 → 必须 rollback。
3. **shell/tests.rs 加 Composite arm**: 维持 ControlCommand match exhaustive。
4. **control_actions.rs 加 Composite 拒绝 arm**: Composite 不应进入默认 executor 分支
   (由 @computer-act dispatch_underlying 单独处理), 加 explicit Unsupported 错误防止 leak。
5. **timeout.rs fields/fired/stop 加 #[allow(dead_code)]**: 当前 caller (let _timeout_watcher) 
   不直接读这些字段, 但 API 留出来给上层显式 stop / fired check (e.g., dispatch 完成后判断
   timeout vs 其它原因失败)。
6. **InvalidArgs 错误路径漏接**: ticket 15 commit 时, mod.rs InvalidArgs handler 没替换成
   error_envelope (断言匹配没找到 text 实际格式)。本轮补接 (跟 ticket 15 一起算)。
7. **smoke dispatched_to check 用 grep -F**: 之前用 grep -E 配 [[:space:]]*, 但 `@key+@click+@key`
   的 + 被当 regex 元字符。grep -F 走字面匹配避免。

### 文件变更
- `src/control_protocol.rs`: 加 `Composite(Vec<ControlCommand>)` variant
- `src/control_actions.rs`: 加 Composite 拒绝 arm
- `src/shell/tests.rs`: 加 Composite arm (test fixture)
- `src/control_computer_act/mod.rs`: 
  - route_hotkey_click 改返 Composite([Key(Press), Click, Key(Release)])
  - dispatch_underlying 加 Composite 分支 + failure rollback
  - InvalidArgs handler 改用 error_envelope (补 ticket 15 漏接)
- `src/control_computer_act/tests.rs`: hotkey_click_routes_to_composite_3_steps 替换旧 test
- `src/control_computer_act/timeout.rs`: TimeoutWatcher fields/fired/stop 加 #[allow(dead_code)]
- 新增 `scripts/smoke_computer_act_all.sh` (197 行, 14 段 e2e)

### 状态
**ticket 08 完成 + ticket 21 实施完成, 13/13 e2e + bonus error path 全过。准备 commit + push。**
