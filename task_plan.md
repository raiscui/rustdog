# 任务计划: 默认主线上下文续档后入口

## [2026-05-14 15:20:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [续档]: 默认 task_plan 超过 1000 行

### 续档原因
- 默认 `task_plan.md` 在 AX plan 索引写入后达到 1003 行,超过仓库六文件 1000 行上限.
- 旧文件已复制到 `archive/default_history/2026-05-14_ax_plan_context_rollover/task_plan_2026-05-14_before_ax_plan_rollover.md`.

### 当前活跃支线
- `__ax_plan`: 正在生成 `@screenshot include_ax` 与 `@ax-*` 控制能力计划.
- `__mouse_e2e`: 仍有未提交的真实 GUI E2E 修改现场,本轮 AX plan 不继续处理该支线.

### 当前状态
**默认主线已续档** - 后续默认任务从本文件继续记录;AX plan 状态继续写入 `task_plan__ax_plan.md`.

## [2026-05-17 00:21:14] [Session ID: codex-20260517-non-mouse-control-research] [索引]: 启用非鼠标控制调研支线

### 启用原因
- 用户要求调研 `https://github.com/iFurySt/open-codex-computer-use`。
- 目标是寻找“完整能力的非鼠标类控制”,避免 live 鼠标测试干扰人类当前操作。

### 支线文件
- `task_plan__non_mouse_control_research.md`: 调研计划和状态。
- `notes__non_mouse_control_research.md`: 仓库与方案调研笔记。
- `WORKLOG__non_mouse_control_research.md`: 本轮调研交付记录。

### 当前状态
**非鼠标控制调研支线已启用** - 本轮不运行任何真实鼠标点击、拖拽或滚轮测试。

## [2026-05-17 10:27:25] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [索引]: 启用非鼠标语义控制实现支线

### 启用原因
- 用户执行 `$ralph .omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
- 目标是把调研方案推进到代码实现,优先完成非鼠标语义控制协议,避免干扰用户正在操作的电脑。

### 支线文件
- `task_plan__non_mouse_semantic_control.md`: Ralph 执行计划和状态。
- `notes__non_mouse_semantic_control.md`: 实现调研和关键决策。
- `WORKLOG__non_mouse_semantic_control.md`: 本轮交付记录。

### 当前状态
**非鼠标语义控制实现支线已启用** - 本轮禁止运行 live 鼠标移动、点击、拖拽或滚轮测试。
## [2026-05-18 10:36:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: 迁移 rdog-control skill 到仓库内

### 行动目的
- 把全局 `/Users/cuiluming/.codex/skills/rdog-control` 复制进 rustdog 仓库,作为项目长期资产维护。

### 为什么现在做
- 仓库已经把 `rdog control` 作为核心入口,skill 应该跟着仓库一起版本化,而不是只留在用户级目录。
- 这样后续改协议、改 README、改 skill references 时,可以在同一个仓库里对齐。

### 将要做什么
- 先确认仓库里 skill 的落点和索引方式。
- 再把 `rdog-control` skill 复制到项目目录中,清理本机编辑器噪音。
- 最后更新项目索引,让它成为长期维护入口。

### 当前阶段
**目前在阶段1** - 先确认项目内 skill 目录和长期索引约定。

## [2026-05-18 10:46:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: 项目内 skill 复制和索引已完成

### 已完成
- [x] 确认全局 skill 来源: `/Users/cuiluming/.codex/skills/rdog-control`
- [x] 复制到项目内: `.codex/skills/rdog-control`
- [x] 排除本机编辑器噪音: 未复制 `.vscode`
- [x] 更新 `AGENTS.md`,将维护入口改为项目内相对路径
- [x] 移除旧的用户级绝对路径索引,避免双入口漂移

### 当前验证
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 已通过
- `git diff --check`: 已通过
- `diff -ru --exclude='.vscode' /Users/cuiluming/.codex/skills/rdog-control .codex/skills/rdog-control`: 无差异

### 当前状态
**目前在阶段4** - 收尾记录 WORKLOG,并最终复查工作区 diff。

## [2026-05-18 10:50:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: rdog-control skill 已迁入项目目录

### 已完成结果
- `rdog-control` skill 已复制到 `.codex/skills/rdog-control`
- `AGENTS.md` 已改为索引项目内相对路径
- 旧的用户级绝对路径索引已清除
- `notes.md`、`WORKLOG.md` 已补齐本轮记录

### 最终验证
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过
- `diff -ru --exclude='.vscode' /Users/cuiluming/.codex/skills/rdog-control .codex/skills/rdog-control`: 无差异
- `git diff --check`: 通过

### 当前状态
**本轮任务已完成** - skill 已成为仓库长期维护入口。

## [2026-05-18 10:55:40] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: 将全局 rdog-control 改成项目内连接目录

### 行动目的
- 让 `/Users/cuiluming/.codex/skills/rdog-control` 指向项目内 `.codex/skills/rdog-control`。
- 后续只维护项目内 skill,全局入口通过连接目录同步使用同一份内容。

### 已确认现状
- 全局路径目前是普通目录,不是连接目录。
- 项目内 `.codex/skills/rdog-control` 已存在。
- 两边实质内容在排除 `.vscode` 后无差异。

### 将要做什么
- 先备份/移走当前全局普通目录。
- 创建 `/Users/cuiluming/.codex/skills/rdog-control -> /Users/cuiluming/local_doc/l_dev/my/rust/rustdog/.codex/skills/rdog-control` 的符号链接。
- 通过 `ls -ld`、`readlink` 和 `quick_validate.py` 验证连接目录可用。

### 当前状态
**目前在阶段2** - 准备替换全局目录为连接目录。

## [2026-05-18 10:57:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: 全局 rdog-control 已改为连接目录

### 已完成结果
- `/Users/cuiluming/.codex/skills/rdog-control` 已替换为符号链接。
- 链接目标指向项目内 `.codex/skills/rdog-control`。
- 全局路径和项目路径现在共享同一份 skill 内容。

### 验证结果
- `ls -ld /Users/cuiluming/.codex/skills/rdog-control`: 显示为 `lrwxr-xr-x`。
- `readlink /Users/cuiluming/.codex/skills/rdog-control`: 指向仓库内 skill 目录。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`: 通过。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过。
- `git diff --check`: 通过。

### 备份位置
- 旧的全局普通目录已移到 `/tmp/rdog-control-global-backup-20260518-104751`。

### 当前状态
**本轮任务已完成** - 全局入口已切成项目内连接目录。

## [2026-05-18 12:56:05] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: autoresearch 项目能力演进建议

### 行动目的
- 按 `$oh-my-codex:autoresearch` 工作流,给出 rustdog 项目能力继续演进建议。
- 研究结果不只停留在聊天,还要落到 `.omx/specs/autoresearch-rustdog-evolution/` 产物中。

### 验证模式
- 使用 `prompt-architect-artifact`。
- 完成 artifact: `.omx/specs/autoresearch-rustdog-evolution/result.json`。
- 输出 artifact: `.omx/specs/autoresearch-rustdog-evolution/report.md`。

### 将要做什么
- [x] 读取当前 README、AGENTS、EXPERIENCE 和核心 specs,确认已有能力边界。
- [x] 按能力层次整理候选演进方向。
- [x] 输出优先级建议、取舍理由和可验证下一步。
- [x] 写入 autoresearch result artifact,包含 architect approval verdict。

### 当前状态
**目前在阶段4** - 已生成 autoresearch report/result artifact,准备做 JSON 与 diff 验证。

## [2026-05-18 13:00:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: autoresearch 研究结论已落盘

### 已完成结果
- 已生成 `.omx/specs/autoresearch-rustdog-evolution/mission.md`。
- 已生成 `.omx/specs/autoresearch-rustdog-evolution/sandbox.md`。
- 已生成 `.omx/specs/autoresearch-rustdog-evolution/report.md`。
- 已生成 `.omx/specs/autoresearch-rustdog-evolution/result.json`。

### 关键修正
- 一开始的候选判断是“先实现 ControlFrame/ControlExecutionOutcome”。
- 源码核验后发现这部分已经存在,所以正式建议改成“完成 ControlPeerSession 一等抽象,并让 TCP/WebSocket/Zenoh 复用同一套 frame dispatch”。

### 当前状态
**目前在阶段4** - 准备验证 artifact JSON、git diff 和最终工作区状态。

## [2026-05-18 13:00:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: autoresearch artifact 已通过验证

### 验证结果
- `jq -e '.architect_review.verdict == "approved" and (.output_artifact_path | length > 0)' .omx/specs/autoresearch-rustdog-evolution/result.json`: 通过。
- `test -s .omx/specs/autoresearch-rustdog-evolution/report.md && test -s .omx/specs/autoresearch-rustdog-evolution/result.json`: 通过。
- `git diff --check`: 通过。

### 待办状态
- [x] 读取当前 README、AGENTS、EXPERIENCE 和核心 specs,确认已有能力边界。
- [x] 按能力层次整理候选演进方向。
- [x] 输出优先级建议、取舍理由和可验证下一步。
- [x] 写入 autoresearch result artifact,包含 architect approval verdict。

### 当前状态
**本轮任务已完成** - autoresearch completion artifact 满足 `prompt-architect-artifact` gate。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: ralplan ControlPeerSession 能力演进实施规划

### 行动目的
- 用户触发 `$ralplan`,要求把 P0-P5 能力演进建议转成 consensus plan。
- 系统提示当前 autoresearch workflow 仍 active,必须先清理 incompatible workflow state,再进入 ralplan。

### 将要做什么
- [x] 清理 active autoresearch state。
- [x] 创建或复用 `.omx/context/` 下的 grounded context snapshot。
- [x] 读取相关源码和 specs,刷新计划依据。
- [x] 输出 Planner 初稿和 RALPLAN-DR summary。
- [ ] 顺序完成 Architect review 和 Critic review。
- [ ] 保存最终 plan 到 `.omx/plans/`,包含 ADR、agent roster、handoff guidance 和 verification path。

### 当前状态
**目前在阶段3** - Planner 初稿已落盘,正在等待 Architect review。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: 收到 Architect ITERATE 反馈并修订 draft

### 反馈要点
- `ControlPeerSession` 不能写成另一个更大 wrapper,必须收窄成 frame ordering / request correlation / lifecycle gating。
- savefile 落盘策略不应进入 session core,应留给 transport adapter / policy 层。
- PTY close 语义还不够明确,需要先决定 drain / detach / force-close 的单一规则。
- Phase 3 的措辞要改成对既有 Zenoh session channel 的迁移和硬化,不是从零引入。
- queryable 仍要保留 stateless fallback / legacy compatibility,不能只剩 bootstrap ack。

### 当前状态
**目前在阶段3** - 已按 Architect 反馈收窄 plan 边界,准备重新送审。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: second Architect ITERATE 反馈已消化

### 新修订点
- 将 `ControlPeerSession` 的职责再收窄为 ordered outbound frame queue,不再写成 fan-out/writer 形态。
- 明确 `@savefile` 路由 / 落盘 / 冲突策略只在 adapter / policy 层。
- PTY close 语义已冻结为三条单一规则:
  - `@pty-close` = force-close
  - `@pty-detach` = 保留进程
  - owner/control disconnect / transport lost = force-close policy cleanup

### 当前状态
**目前在阶段3** - revised draft 已更新,准备再次送审 Architect。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: Architect 第三轮通过

### 审查结论
- Architect verdict: `APPROVE`
- 保留提醒: `terminal completion detection` 和 `session close / detach / attach state hooks` 必须限制在 wire-level gating。
- PTY process、savefile persistence、transport plumbing 继续归 adapter / backend / policy 层。

### 当前状态
**目前在阶段4** - Architect 已通过,准备进入 Critic review。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: Critic ITERATE 反馈已消化

### Critic 要求
- Phase 级别明确 `ControlPeerSession` 不拥有 PTY process、savefile persistence、transport plumbing。
- PTY close 只由 session core 判定 terminal frame / close reason,真正进程动作由 PTY manager 或 adapter 执行。
- Phase 2/3/4/5 必须补具体测试命令或新增测试名。
- Observability 不能只列 log 字段,必须说明如何验证字段存在、如何验证不记录 base64、如何区分 timeout / terminal / transport close。
- Acceptance Criteria 要从结论改成可观察信号。

### 已完成修订
- Phase 0/1 增加了 PTY process ownership 非目标和 gate decision 表述。
- Phase 2/3/4/5/6 增加了具体 `cargo test` 命令或新增测试名。
- Observability 增加了 tracing capture 类测试名。
- Acceptance Criteria 增加了 legacy queryable negative test、first split target 和 core ownership 断言。

### 当前状态
**目前在阶段3** - 因 Critic 要求迭代,已修订 Planner draft,准备重新进入 Architect review。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: post-Critic Architect review 通过

### 审查结论
- Architect verdict: `APPROVE`
- 已确认 revised draft 满足具体命令、observability 测试、measurable acceptance criteria 和非所有权边界。
- 合成提醒: observability 测试不能反向迫使 `ControlPeerSession` 拥有 transport plumbing。

### 当前状态
**目前在阶段4** - Architect 再次通过,准备重新进入 Critic review。

## [2026-05-18 13:45:05] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: ralplan consensus plan 已落盘

### 已完成结果
- 最终 plan 已保存到 `.omx/plans/ralplan-rustdog-control-peer-session-evolution.md`。
- planner draft 仍保留在 `.omx/drafts/ralplan-rustdog-control-peer-session-evolution-planner-draft.md` 作为草稿轨迹。
- final plan 已吸收 Critic 非阻塞建议,包括更明确的 observability / ownership / phase wording。

### 审查结论
- Architect verdict: `APPROVE`
- Critic verdict: `APPROVE`

### 当前状态
**本轮任务已完成** - consensus plan 已通过 Architect 和 Critic,并已落到 `.omx/plans/`。

## [2026-05-18 13:48:36] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [收尾验证]: 最终产物与工作区状态复核

### 复核结果
- `git diff --check` 通过,没有额外的 whitespace / patch 格式问题。
- `.omx/plans/ralplan-rustdog-control-peer-session-evolution.md` 非空。
- `.omx/drafts/ralplan-rustdog-control-peer-session-evolution-planner-draft.md` 与 final plan 内容一致。
- 当前工作区仍保留这轮计划、笔记和工作记录的改动,供后续继续沿用。

### 当前状态
**本轮任务已完成并复核完毕** - 交付物可直接作为后续 `ralph` / `team` / `ultragoal` 的输入。

## [2026-05-18 14:04:11] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [执行]: Ralph 执行 ControlPeerSession Phase 0-2

### 行动目的
- 用户触发 `$ralph .omx/plans/ralplan-rustdog-control-peer-session-evolution.md`。
- 这次按已批准计划中的 Ralph path 先执行 Phase 0-2,目标是把 `ControlPeerSession` core 和 TCP / WebSocket 适配打稳。

### 将要做什么
- [x] 读取 Ralph state、context snapshot 和 consensus plan。
- [x] 记录执行范围为 Phase 0-2,并写入 Ralph state。
- [ ] 只读梳理 `ControlFrame` / `ControlExecutionOutcome` 的定义、消费点和现有测试。
- [ ] 新增或调整 `ControlPeerSession` core,保持它只拥有 ordering / correlation / lifecycle gating。
- [ ] 接入 TCP / WebSocket frame dispatch,保持旧 stdout / savefile 行为不变。
- [ ] 运行 focused tests、`cargo fmt -- --check` 和 `git diff --check`。
- [ ] 完成 Ralph architect/deslop/regression 收尾。

### 当前状态
**目前在执行阶段** - 正在做 Phase 0-2 代码与测试入口调查。

## [2026-05-18 14:25:21] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [进展]: ControlPeerSession 初版已接入并通过 focused tests

### 已完成内容
- 新增 `src/control_session.rs`,提供 transport-agnostic `ControlPeerSession`、`ControlPeerFrameSink`、`LineWriteFrameSink` 和 PTY lifecycle gate。
- `src/main.rs` 已注册新模块。
- `src/shell.rs` 和 `src/zenoh_control.rs` 已开始复用 `control_session` 的 frame dispatch / result routing。
- `control_session::tests` 已覆盖 0/1/N frame dispatch、savefile base64 摘要不泄漏、adapter target observability、PTY terminal gate。

### 验证结果
- `cargo test --package rustdog --bin rdog -- control_session::tests` 通过。

### 当前状态
**目前在执行阶段** - 正在继续补 Phase 0-2 的 TCP / WebSocket / Zenoh 回归验证。

## [2026-05-18 15:00:27] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [修正]: 补齐 TCP/WebSocket screenshot savefile 运行态 smoke

### 行动目的
- Architect review 要求补一条真实运行态的 `@screenshot -> @savefile` smoke 证据。
- 当前单测和 `--no-run` 已过,但还缺 live 证据闭环。

### 将要做什么
- [x] 跑至少一条 ignored 的 TCP 或 WebSocket screenshot savefile smoke。
- [x] 记录实际输出和失败原因,如果是权限问题就按权限契约记录。
- [x] 根据 smoke 结果决定是继续修复还是进入下一轮 review。

### 当前状态
**目前在修正阶段** - TCP 和 WebSocket screenshot savefile smoke 已在 `caffeinate -d -u` 包裹下通过,准备进入下一轮 review。

## [2026-05-18 15:13:16] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [验证]: live screenshot savefile smoke 已补齐

### 现象
- 直接运行 TCP / WebSocket screenshot savefile ignored tests 时,两条都失败为 `@response {"id":7,"code":70,"error":"没有可截图的显示器"}`。
- `system_profiler SPDisplaysDataType` 同时显示内置屏和外接屏均为 `Display Asleep: Yes`。

### 验证处理
- 使用 `caffeinate -d -u -t 30` 包住 live smoke 测试窗口后,TCP 和 WebSocket 均通过。

### 通过命令
- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_lanes daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control -- --exact --ignored --nocapture`: 通过。
- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_websocket control_cli_should_execute_screenshot_and_save_file_over_websocket -- --exact --ignored --nocapture`: 通过。

### 当前状态
**目前在验证阶段** - Architect 阻断项已补证,准备重新发起 review。

## [2026-05-18 15:30:28] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: Ralph Phase 0-2 通过 review / deslop / regression

### 完成情况
- [x] `ControlPeerSession` 薄抽象已新增。
- [x] TCP / WebSocket outcome dispatch 已复用 `ControlPeerSession`。
- [x] Zenoh session channel outcome publish 已复用 `ControlPeerSession`。
- [x] `specs/control-frame-refactor-plan.md` 已同步当前 baseline。
- [x] Architect 第二轮 verdict: `APPROVE`。
- [x] ai-slop-cleaner changed-files scoped pass 已完成。
- [x] post-deslop regression 已完成。

### post-deslop 验证
- `cargo test --package rustdog --bin rdog -- control_session::tests control_frames::tests control_core::tests shell::tests`: 34 passed。
- `cargo test --package rustdog --bin rdog --no-run`: 通过。
- `cargo test --package rustdog --test control_lanes --no-run`: 通过。
- `cargo test --package rustdog --test control_websocket --no-run`: 通过。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_lanes daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control -- --exact --ignored --nocapture`: 通过。
- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_websocket control_cli_should_execute_screenshot_and_save_file_over_websocket -- --exact --ignored --nocapture`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。

### 当前状态
**本轮 Ralph Phase 0-2 已完成** - 等待清理 Ralph state 并交付结果。

## [2026-05-18 15:40:23] [Session ID: codex-resume-20260518-154023] [收尾复核]: resume 后重新验证 Ralph Phase 0-2

### 行动目的
- 接续上一轮 Ralph 执行结果,在当前回合内重新跑关键验证,避免最终交付只依赖上下文摘要。
- 确认 Ralph state 已清理,并确认 live screenshot smoke 在当前显示器状态下仍可通过。

### 重新验证结果
- [x] `cargo test --package rustdog --bin rdog -- control_session::tests control_frames::tests control_core::tests shell::tests`: 34 passed。
- [x] `cargo test --package rustdog --bin rdog --no-run`: 通过。
- [x] `cargo test --package rustdog --test control_lanes --no-run`: 通过。
- [x] `cargo test --package rustdog --test control_websocket --no-run`: 通过。
- [x] `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- [x] `caffeinate -d -u -t 30 cargo test --package rustdog --test control_lanes daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control -- --exact --ignored --nocapture`: 1 passed。
- [x] `caffeinate -d -u -t 30 cargo test --package rustdog --test control_websocket control_cli_should_execute_screenshot_and_save_file_over_websocket -- --exact --ignored --nocapture`: 1 passed。
- [x] `cargo fmt -- --check`: 通过。
- [x] `git diff --check`: 通过。
- [x] `omx state read --input '{"mode":"ralph"}' --json`: 返回 `{"exists":false,"mode":"ralph"}`。

### 当前状态
**本轮 Ralph Phase 0-2 已完成并重新复核** - 可以交付结果,后续如继续演进,下一阶段是 Phase 3 的 Zenoh session channel 主路径收紧。

## [2026-05-18 16:04:35] [Session ID: codex-phase3-20260518-160435] [执行]: Ralph Phase 3 Zenoh session channel 主路径收紧

### 行动目的
- 继续 `.omx/plans/ralplan-rustdog-control-peer-session-evolution.md` 的 Phase 3。
- 目标是让 Zenoh rich control 的默认主路径明确走 session channel,并让 queryable 只承担 session open ack、legacy request 和 compatibility。

### 为什么现在做
- Phase 0-2 已经让 `ControlPeerSession` 成为 TCP / WebSocket / Zenoh outcome dispatch 的共享薄 core。
- Phase 3 需要把 Zenoh 的富能力验收从“可能通过 query reply”收紧到“必须经过 `to-daemon` / `to-control` session channel”。

### 将要做什么
- [ ] 只读梳理 `src/zenoh_control.rs` 和 `tests/zenoh_router_client.rs` 的 session open、to-daemon、to-control、queryable fallback 路径。
- [ ] 先做最小可证伪测试,确认当前 rich control 是否仍能直接通过 queryable multiline reply 成功。
- [ ] 根据证据修改 Zenoh control path,让 rich frame over legacy queryable 返回明确 unsupported / legacy error。
- [ ] 补 `control_should_reject_rich_frame_over_legacy_queryable_path` 或等价测试。
- [ ] 跑 Phase 3 focused tests、fmt 和 diff 检查。
- [ ] 做 Ralph review / deslop / post-deslop regression 收尾。

### 当前状态
**目前在执行阶段** - 正在刷新 Zenoh session/queryable 路径和现有测试入口。

## [2026-05-18 16:12:11] [Session ID: codex-phase3-20260518-160435] [进展]: Phase 3 legacy queryable 负向测试已红转绿

### 已观察现象
- 新增 `control_should_reject_rich_frame_over_legacy_queryable_path` 后,未改运行时时测试失败。
- 失败输出证明直接 queryable `@screenshot#7` 会返回 `@savefile` image、`@savefile` manifest 和 final `screenshot-bundle`。

### 已完成修正
- `src/zenoh_control.rs` 增加 `reject_session_channel_only_legacy_query()`。
- 无 `session_id` 的 legacy queryable 分支在执行前拒绝 session-channel-only 命令。
- 被拒绝命令包括 screenshot、PTY lifecycle、mouse、AX、window、type-text 和 `@savefile`。
- `@ping`、`@cmd` 和裸 shell 仍作为 bootstrap / legacy compatibility 保留。
- `open_daemon_session_bridge()` 的普通 line-control outcome dispatch 已改为复用 `ControlPeerSession::dispatch_outcome_ref()`。

### 当前验证
- `cargo test --package rustdog --test zenoh_router_client control_should_reject_rich_frame_over_legacy_queryable_path -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog -- zenoh_control::tests::legacy_queryable_should_reject_rich_screenshot_requests zenoh_control::tests::legacy_queryable_should_allow_bootstrap_and_compatibility_requests`: 2 passed。

### 当前状态
**目前在验证阶段** - 正在运行 Phase 3 focused integration tests。

## [2026-05-18 16:30:01] [Session ID: codex-phase3-20260518-160435] [完成]: Ralph Phase 3 Zenoh session channel 主路径已收紧

### 完成情况
- [x] 只读梳理 `src/zenoh_control.rs` 和 `tests/zenoh_router_client.rs` 的 session open、to-daemon、to-control、queryable fallback 路径。
- [x] 最小红测确认 direct queryable `@screenshot#7` 会返回 `@savefile` / bundle,主假设成立。
- [x] legacy queryable 对 screenshot、PTY、mouse、AX、window、type-text、`@savefile` 返回 code 78。
- [x] 旧 `__rdog_session__` query payload 对 rich command 同样返回 code 78 到 `to-control`,不执行 rich producer。
- [x] `open_daemon_session_bridge()` 普通 line-control outcome dispatch 已复用 `ControlPeerSession::dispatch_outcome_ref()`。
- [x] `specs/control-frame-refactor-plan.md` 已同步 Phase 3 baseline。

### 验证结果
- `cargo test --package rustdog --test zenoh_router_client control_should_execute_literal_shell_line_in_zenoh_profile -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_should_find_daemon_by_target_name_without_explicit_entrypoint -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_should_reach_daemon_via_explicit_entrypoint_fallback -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client external_peer_should_send_control_request_via_zenoh_to_daemon_channel -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_should_reject_rich_frame_over_legacy_queryable_path -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_should_reject_rich_frame_over_legacy_session_query_payload -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_should_detach_and_attach_pty_in_zenoh_profile -- --exact`: 通过。
- `cargo test --package rustdog --test zenoh_router_client control_session_should_reresolve_after_daemon_restart -- --exact`: 通过。
- `caffeinate -d -u -t 30 cargo test --package rustdog --test zenoh_router_client control_should_execute_screenshot_and_save_file_in_zenoh_profile -- --exact --ignored --nocapture`: 通过。
- `cargo test --package rustdog --bin rdog -- zenoh_control::tests::legacy_queryable_should_reject_rich_screenshot_requests zenoh_control::tests::legacy_queryable_should_allow_bootstrap_and_compatibility_requests`: 通过。
- `cargo test --package rustdog --bin rdog --no-run`: 通过。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。

### 审查和降级
- `omx ask claude --agent-prompt architect ...`: 本机没有 `architect` prompt role。
- `omx ask claude -p ...`: provider 返回 402 insufficient balance。
- `omx ask gemini -p ...`: 30 秒无输出后已清理进程。
- 本轮 architect 外部复核不可用,以本地静态 review、red-green 测试和 focused regression 作为降级证据。

### 当前状态
**本轮 Ralph Phase 3 已完成** - 等待清理 Ralph state 并交付结果。下一阶段可进入 Phase 4 的 GUI recipe / capability diagnosis。

## [2026-05-18 16:38:45] [Session ID: codex-phase4-20260518-163845] [执行]: Ralph Phase 4 GUI recipe 与能力诊断

### 行动目的
- 进入 `.omx/plans/ralplan-rustdog-control-peer-session-evolution.md` 的 Phase 4。
- 目标是把 GUI agent 工作流和平台权限能力诊断产品化,先落一份可被 protocol / CLI 复用的 capability model。

### 为什么现在做
- Phase 3 已收紧 Zenoh rich control 主路径,后续 GUI agent 能力不能继续靠猜 transport 或猜平台权限。
- 权限失败是项目里的一等契约,需要让 agent 能结构化知道 macOS Accessibility / Screen Recording、Windows UIPI、Linux backend 能力边界。

### 方向选择
- 选择先实现 `@capabilities`,作为远程 daemon 能力诊断的单一真相源。
- `rdog doctor` 暂不作为第一落点,避免 CLI 和 protocol 同时定义两套模型;后续如果需要,应复用同一份 capability report。

### 将要做什么
- [ ] 梳理当前 control protocol / action executor / screenshot / AX / mouse / window 能力与权限错误映射。
- [ ] 新增 `control_capabilities` 模块,返回结构化 report,并将 `@capabilities` 接入 line-control。
- [ ] 补 unit tests,至少区分 permission_denied 和 unsupported。
- [ ] 更新 `.codex/skills/rdog-control` 的 observe / locate / semantic action / verify / fallback recipe,让 agent 先做能力探测。
- [ ] 同步 specs / AGENTS 长期索引。
- [ ] 跑 focused tests、`cargo fmt -- --check` 和 `git diff --check`。

### 当前状态
**目前在执行阶段** - 正在调查已有能力入口和文档/skill 结构。

## [2026-05-18 16:43:15] [Session ID: codex-phase4-20260518-163845] [进展]: 准备实现 @capabilities

### 已确认
- `@capabilities` 目前还没有协议入口。
- 现有 `control_core` 已支持结构化 JSON 成功响应,适合作为能力报告返回通道。
- 权限错误已有统一 code 语义: `PermissionDenied` -> `77`, `Unsupported` -> `78`。

### 将要做什么
- 新增 `src/control_capabilities.rs`,把能力报告模型和平台探测集中到一个模块。
- 在 `src/control_protocol.rs` 增加 `@capabilities` 解析。
- 在 `src/control_core.rs` 增加执行分支,返回 `rdog.capabilities.v1` JSON。
- 补解析、执行和报告 builder 的 focused tests。

### 当前状态
**目前在执行阶段** - 进入代码实现,先落最小可验证的 `@capabilities` 单一真相源。

## [2026-05-18 17:00:55] [Session ID: codex-phase4-20260518-163845] [进展]: @capabilities 最小链路已通过 focused tests

### 已完成
- [x] 新增 `src/control_capabilities.rs`,包含 `rdog.capabilities.v1` report builder。
- [x] `@capabilities` 和 `@capabilities#id` 已接入 `src/control_protocol.rs`。
- [x] `src/control_core.rs` 已直接返回结构化 capabilities report,不经过 action executor。
- [x] `.codex/skills/rdog-control` 和相关 specs 已开始同步 GUI agent recipe。

### 验证
- `cargo test --package rustdog --bin rdog -- control_capabilities::tests control_protocol::tests::parse_should_support_key_paste_script_cmd_and_screenshot control_protocol::tests::parse_should_support_optional_request_ids control_core::tests::explicit_request_should_render_capabilities_report`: 6 passed。

### 当前状态
**目前在文档同步阶段** - 继续补协议和 code-agent 使用文档,然后跑格式化、focused regression 和 diff 检查。

## [2026-05-18 17:09:25] [Session ID: codex-phase4-20260518-163845] [完成]: Ralph Phase 4 @capabilities 与 GUI recipe 已落地

### 完成情况
- [x] 梳理当前 control protocol / action executor / screenshot / AX / mouse / window 能力与权限错误映射。
- [x] 新增 `control_capabilities` 模块,返回结构化 `rdog.capabilities.v1` report。
- [x] 将 `@capabilities` / `@capabilities#id` 接入 line-control。
- [x] 补 unit tests,区分 `permission_denied` 和 `unsupported`。
- [x] 更新 `.codex/skills/rdog-control` 的 GUI recipe,固定 `@capabilities -> observe -> locate -> activate_or_focus -> semantic_action -> verify -> fallback_recipe`。
- [x] 同步 `specs/control-line-protocol.md`、`specs/code-agent-rdog-control-usage.md` 和 `specs/rdog-non-mouse-semantic-control-plan.md`。
- [x] `rdog doctor` 后续复用 `@capabilities` model 的事项已记录到 `LATER_PLANS.md`。

### 验证结果
- `cargo test --package rustdog --bin rdog -- control_capabilities::tests control_protocol::tests::parse_should_support_key_paste_script_cmd_and_screenshot control_protocol::tests::parse_should_support_optional_request_ids control_protocol::tests::parse_should_reject_unknown_or_empty_or_multiline_payloads_or_bad_request_ids control_core::tests::explicit_request_should_render_capabilities_report zenoh_control::tests::legacy_queryable_should_allow_bootstrap_and_compatibility_requests`: 8 passed。
- `cargo test --package rustdog --bin rdog --no-run`: 通过。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `cargo test --package rustdog --all-targets --no-run`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过。
- `beautiful-mermaid-rs --ascii` 验证 `specs/rdog-non-mouse-semantic-control-plan.md` 的 Mermaid 决策流: 通过。

### 当前状态
**本轮 Ralph Phase 4 已完成** - 等待清理 Ralph state 并交付结果。

## [2026-05-18 17:37:16] [Session ID: codex-phase5-20260518-173716] [计划]: 进入结构性减负支线

### 行动目的
- 用户要求继续进入结构性减负。
- 当前最重的核心文件是 `src/control_protocol.rs`、`src/zenoh_control.rs`、`src/shell.rs` 和 `src/control_actions.rs`。

### 这次先做什么
- 先从 `src/control_protocol.rs` 的测试和解析辅助开始拆分,降低单文件长度。
- 先做低风险、可回滚的结构拆分,确保协议输出完全不变。
- 同步记录一个明确的首个拆分目标,给后续更深的拆文件留边界。

### 当前阶段
**目前在执行阶段** - 先把 `control_protocol.rs` 的测试区和辅助函数拆开,再继续决定下一块拆哪里。

## [2026-05-18 17:40:47] [Session ID: codex-phase5-20260518-173716] [执行]: 拆分 control_protocol 解析器和测试

### 行动目的
- 先降低 `src/control_protocol.rs` 的单文件长度,并保留原有 `crate::control_protocol::*` 导入边界。
- 本轮只做结构迁移,不改变 line-control 的 wire syntax、request id、错误语义或默认值。

### 将要做什么
- [x] 新建 `src/control_protocol/parsers.rs`,承接 PTY / screenshot / key / 对象字段解析辅助。
- [x] 新建 `src/control_protocol/tests.rs`,承接原 `#[cfg(test)] mod tests`。
- [x] 在 `src/control_protocol.rs` 只保留协议类型、`parse_control_line()` 入口和 helper re-export。
- [ ] 跑 focused parser tests、相关 compile checks、format 和 diff check。

### 当前状态
**目前在验证阶段** - `cargo test --package rustdog --bin rdog -- control_protocol::tests` 已通过,继续跑相关 no-run 编译和 diff check。

## [2026-05-18 17:48:59] [Session ID: codex-phase5-20260518-173716] [执行]: 继续拆分 parser 子模块

### 行动目的
- 初次拆分后 `src/control_protocol/parsers.rs` 仍有 1286 行,不符合 Rust 文件健康线。
- 继续把 payload parser 按语义拆成 PTY / screenshot / key 三块,避免把结构性负担只是换位置。

### 将要做什么
- [x] 新建 `src/control_protocol/parsers/pty.rs`。
- [x] 新建 `src/control_protocol/parsers/screenshot.rs`。
- [x] 新建 `src/control_protocol/parsers/key.rs`。
- [x] 保留 `src/control_protocol/parsers.rs` 作为 common helper 与 re-export registry。
- [x] 重新运行 focused tests、相关 compile checks、format 和 diff check。

### 当前状态
**本轮结构减负首个目标已完成** - `control_protocol` 已拆成父模块、common parser registry、PTY / screenshot / key parser 子模块和测试模块; 相关验证通过。

## [2026-05-18 18:12:42] [Session ID: codex-phase5-20260518-173716] [执行]: 拆分 control_actions 测试模块

### 行动目的
- `src/control_actions.rs` 仍有 1213 行,超过 Rust 文件健康线。
- 这次只把内联 `#[cfg(test)] mod tests` 拆到 `src/control_actions/tests.rs`,保留主执行路径不变。

### 将要做什么
- [x] 新建 `src/control_actions/tests.rs`,迁移原内联单测。
- [x] 在 `src/control_actions.rs` 末尾改为 `#[cfg(test)] mod tests;`。
- [x] 运行 `control_actions::tests`、相关 no-run 编译、fmt 和 diff check。

### 当前状态
**本轮控制执行层测试拆分已完成** - `control_actions.rs` 已回到健康线内,测试已迁到同名子模块,验证通过。

## [2026-05-18 18:25:24] [Session ID: codex-phase5-20260518-173716] [执行]: 拆分 shell 测试模块

### 行动目的
- `src/shell.rs` 仍有 1632 行,也是结构性减负剩余重点。
- 优先迁移末尾 `#[cfg(test)] mod tests`,让 shell 主文件先回到健康线内。

### 将要做什么
- [x] 新建 `src/shell/tests.rs`,迁移原内联测试。
- [x] 在 `src/shell.rs` 末尾改为 `#[cfg(test)] mod tests;`。
- [x] 运行 `shell::tests`、相关 no-run 编译、fmt 和 diff check。

### 当前状态
**本轮 shell 测试拆分已完成** - `shell.rs` 已回到健康线内,测试已迁到同名子模块,验证通过。

## [2026-05-18 18:48:01] [Session ID: codex-phase5-20260518-173716] [执行]: 按 LATER_PLANS 拆分 zenoh_control session payload

### 行动目的
- 用户要求按 `LATER_PLANS.md` 继续。
- `zenoh_control/session_payload.rs` 是最低风险切口,只承接 session open / close / bridge payload 的 render / parse。
- 本轮不改变 `to-daemon` / `to-control` keyexpr,不改变 legacy `__rcat_session__` 兼容语义。

### 将要做什么
- [ ] 新建 `src/zenoh_control/session_payload.rs`。
- [ ] 将 `SessionBridgeRequest` 和 session payload render / parse 函数迁入子模块。
- [ ] 迁移对应 unit tests,保留 `zenoh_control::tests` 里其余测试不动。
- [ ] 运行 `zenoh_control::tests`、`zenoh_router_client --no-run`、`cargo fmt -- --check` 和 `git diff --check`。

### 当前状态
**目前在验证与延伸阶段** - `session_payload` 已拆出并验证通过,下一步继续拆 `target_resolve`。

## [2026-05-18 18:52:16] [Session ID: codex-phase5-20260518-173716] [执行]: 拆分 zenoh_control target resolve

### 行动目的
- `src/zenoh_control.rs` 里最适合第二刀的边界是 target resolve 和 daemon guard。
- 这部分可以单独承接 liveliness parse、target resolve、pid guard 和平台 guard 目录,不碰 session bridge loop。

### 将要做什么
- [ ] 新建 `src/zenoh_control/target_resolve.rs`。
- [ ] 将 `ResolvedTarget`、liveliness parse、target resolve、daemon-name guard 迁出。
- [ ] 保留现有调用点只通过 `use self::target_resolve::*` 访问。
- [ ] 跑 `zenoh_control::tests`、`zenoh_router_client --no-run`、`cargo fmt -- --check` 和 `git diff --check`。

### 当前状态
**目前在执行阶段** - 准备迁移 target resolve 子模块。

## [2026-05-18 19:00:05] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [执行]: 继续落地 zenoh_control target_resolve wiring

### 行动目的
- 用户要求按 `LATER_PLANS.md` 继续,当前最直接的减负切口就是把 `src/zenoh_control.rs` 里还残留的 target resolve / daemon guard 代码真正切到子模块。
- 这次只收拢纯解析和 guard 边界,不碰 session bridge 主循环。

### 将要做什么
- [ ] 给 `src/zenoh_control.rs` 加上 `mod target_resolve;` 和对应 `use`。
- [ ] 删除父文件里残留的 `ResolvedTarget`、liveliness parse、daemon guard、target resolve、平台 guard helper 和对应测试。
- [ ] 让 `src/zenoh_control/target_resolve.rs` 承接这些实现,并补齐可编译所需的 import / visibility。
- [ ] 跑 `zenoh_control::tests`、`cargo test --package rustdog --test zenoh_router_client --no-run`、`cargo fmt -- --check` 和 `git diff --check`。

### 当前状态
**目前在执行阶段** - 先把 wiring 接通,再清理父文件残留。

## [2026-05-18 19:09:36] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: target_resolve 已完成,继续 daemon_bridge

### 已完成
- [x] `src/zenoh_control.rs` 已接入 `mod target_resolve;`。
- [x] target resolve / daemon guard / liveliness parse 已从父文件移入 `src/zenoh_control/target_resolve.rs`。
- [x] liveliness parse 单测已移动到 `target_resolve` 子模块。

### 验证结果
- `cargo test --package rustdog --bin rdog -- zenoh_control::target_resolve::tests`: 2 passed。
- `cargo test --package rustdog --bin rdog -- zenoh_control::tests`: 6 passed。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。

### 下一步
- [ ] 继续按 `LATER_PLANS.md` 切 `src/zenoh_control/daemon_bridge.rs`。
- [ ] 只迁 `open_daemon_session_bridge()` 和 daemon bridge 私有 helper,暂时不动 client PTY loop。

### 当前状态
**目前在执行阶段** - 进入 `daemon_bridge` 子模块拆分。

## [2026-05-18 19:16:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: daemon_bridge 已完成,继续 client_pty

### 已完成
- [x] 新建 `src/zenoh_control/daemon_bridge.rs`。
- [x] `open_daemon_session_bridge()`、daemon bridge publish helper 和 PTY frame 描述 helper 已迁出父模块。
- [x] `src/zenoh_control.rs` 从 1799 行降到 1464 行。

### 验证结果
- `cargo test --package rustdog --bin rdog -- zenoh_control::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog -- zenoh_control::target_resolve::tests`: 2 passed。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。

### 下一步
- [ ] 继续按 `LATER_PLANS.md` 切 `src/zenoh_control/client_pty.rs`。
- [ ] 迁出 client session bridge、PTY ready/attach、stdin/resize pump、session request helper。
- [ ] 保持 `run_client_control()` / `run_client_pty_control()` / `run_client_pty_attach()` 的外部行为不变。

### 当前状态
**目前在执行阶段** - 进入 `client_pty` 子模块拆分。

## [2026-05-18 19:23:56] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: LATER_PLANS 的 zenoh_control 深层拆分已落地

### 已完成
- [x] `src/zenoh_control/session_payload.rs` 承接 session open / close / bridge payload。
- [x] `src/zenoh_control/target_resolve.rs` 承接 liveliness parse、target resolve、daemon-name guard。
- [x] `src/zenoh_control/daemon_bridge.rs` 承接 daemon session bridge loop。
- [x] `src/zenoh_control/client_pty.rs` 承接 client session bridge、PTY ready/attach、stdin/resize pump 和 session request helper。
- [x] `src/zenoh_control.rs` 已降到 906 行,回到项目 Rust 文件健康线内。
- [x] `LATER_PLANS.md` 里的“zenoh_control 深层拆分”完成项已清除。

### 验证结果
- `cargo test --package rustdog --bin rdog -- zenoh_control::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog -- zenoh_control::target_resolve::tests`: 2 passed。
- `cargo test --package rustdog --test zenoh_router_client --no-run`: 通过。
- `cargo fmt -- --check`: 通过。
- `git diff --check`: 通过。

### 当前状态
**本轮按 LATER_PLANS 继续的结构减负已完成** - 进入最终检查与交付。

## [2026-05-18 23:01:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: agent-desktop 对标研究

### 启用原因
- 用户要求查看外部仓库 `https://github.com/lahfir/agent-desktop`,并判断对 rustdog 有哪些可借鉴与补充。
- 这是架构研究支线,不直接修改当前 Phase 5 主线代码。
- 默认 `notes.md` 已接近 1000 行,本轮避免继续向默认 notes 灌入研究材料。

### 支线上下文集
- `task_plan__agent_desktop_review.md`
- `notes__agent_desktop_review.md`

### 当前状态
**目前在研究阶段** - 先读取外部仓库 README / 代码结构 / 配置与能力面,再映射回 rustdog 的 control plane、GUI agent workflow 和能力诊断。

## [2026-05-19 08:55:23] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: observation refmap P0 可落地计划

### 启用原因
- 用户要求按 `$ralplan` 基于 `specs/rdog-observation-scoped-refmap-plan.md` 创建可落地计划。
- 范围明确为先做 P0,等 P0 落地后再细化 P1。
- 这是规划支线,不直接改实现代码。

### 支线上下文集
- `task_plan__observation_refmap_plan.md`
- `notes__observation_refmap_plan.md`

### 当前状态
**目前在规划阶段** - 先完成 ralplan pre-context intake 和 brownfield 代码事实收集,再输出 `.omx/plans/` 可执行计划。

## [2026-05-20 19:57:24] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: observation refmap P3 semantic re-find 可落地计划

### 启用原因
- 用户要求用 `$ralplan` 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P3 可落地计划。
- P0/P1/P2 已经各自有独立计划和执行记录,本轮只做 P3 semantic re-find 的规划,不修改 Rust 实现代码。
- 默认 `notes.md` 已接近 1000 行,本轮避免把 P3 研究材料写入默认上下文。

### 支线上下文集
- `task_plan__observation_refmap_p3.md`
- `notes__observation_refmap_p3.md`
- `WORKLOG__observation_refmap_p3.md`

### 当前状态
**目前在规划阶段** - 先回读 roadmap、P2 计划/落地证据和当前 selector/observation 代码面,再输出 `.omx/plans/ralplan-rdog-observation-refmap-p3.md`。

## [2026-05-21 07:23:35] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: observation refmap P4 `@observe` 可落地计划

### 启用原因
- 用户要求用 `$ralplan` 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P4 可落地计划。
- P3 semantic re-find 已落地,本轮只做 P4 规划,不修改 Rust 实现代码。
- 默认 `notes.md` 接近 1000 行,本轮继续使用支线上下文避免污染默认上下文。

### 支线上下文集
- `task_plan__observation_refmap_p4.md`
- `notes__observation_refmap_p4.md`
- `WORKLOG__observation_refmap_p4.md`

### 当前状态
**目前在规划阶段** - 先回读 roadmap、P3 落地证据和当前 observation/selector/refind 代码面,再输出 `.omx/plans/ralplan-rdog-observation-refmap-p4.md`。

## [2026-05-21 19:48:04] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [支线索引]: observation refmap P5 mouse ref 化可落地计划

### 启用原因

- 用户要求用 `$ralplan` 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P5 可落地计划。
- P4 `@observe` 已落地并完成 `observe.rs` request / producer / response / refs 分层,本轮只做 P5 规划,不修改 Rust 实现代码。
- P5 是 observation roadmap 的最后一层,需要把 mouse command 接入 ref / selector,但必须保持 mouse 是显式 fallback,不是隐藏主路径。

### 支线上下文集

- `task_plan__observation_refmap_p5.md`
- `notes__observation_refmap_p5.md`
- `WORKLOG__observation_refmap_p5.md`

### 当前状态

**目前在规划阶段** - 先回读 roadmap、P4 落地证据和当前 mouse / observation / selector 代码面,再输出 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。

## [2026-05-22 11:43:43] [Session ID: db93c592-07b8-4e82-af49-37791f2a5c8b] [支线索引]: observation refmap P0-P5 commit 与 macOS live smoke

### 启用原因

- 用户要求先把 observation refmap P0-P5 按主题整理提交,再补一轮真实 macOS GUI smoke。
- 默认 `notes.md` 已 989 行,本轮使用独立支线上下文,避免把默认 notes 推过 1000 行。
- 这是提交与动态证据支线,需要精确区分 observation refmap 相关改动和明显无关改动。

### 支线上下文集

- `task_plan__observation_refmap_commit_smoke.md`
- `notes__observation_refmap_commit_smoke.md`
- `WORKLOG__observation_refmap_commit_smoke.md`
- `ERRORFIX__observation_refmap_commit_smoke.md`

### 当前状态

**目前在阶段A** - 正在盘点 git diff,准备按主题拆分 staging 和 commit。
