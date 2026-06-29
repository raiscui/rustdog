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
