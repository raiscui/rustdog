# 任务计划: rdog window control 共识规划

## [2026-05-16 14:34:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralplan]: 启动 @window-* 共识规划

### 目标
把 `.omx/specs/deep-interview-rdog-occluded-window-control.md` 转成可执行共识计划,覆盖跨平台 `@window-*` 协议,macOS 完整首版实现,Windows/Linux stub,skill 更新和 live ignored E2E 验收.

### 阶段
- [x] 阶段1: 读取 deep-interview spec 和现有 AX/control 代码事实.
- [ ] 阶段2: Planner 生成 RALPLAN-DR 草案.
- [ ] 阶段3: Architect 顺序审查.
- [ ] 阶段4: Critic 顺序审查.
- [ ] 阶段5: 根据反馈修订并输出 consensus plan,PRD,test-spec.

### 当前事实
- `src/control_protocol.rs` 已有 `ControlCommand::AxTree/AxFind/AxGet/AxPress`,解析入口集中在 line-control parser.
- `src/control_actions.rs` 已把 AX 命令分发到 default AX backend 并返回 structured JSON.
- `src/control_ax.rs` / `src/control_ax/query.rs` 已有 AX snapshot/find/get/press 的模型和轻量查询模式.
- `tests/control_ax_e2e.rs` 已有 macOS live ignored E2E harness,支持 Terminal 宿主授权路径.

### 状态
**目前在 Planner 草案阶段** - 先生成 draft,再按 ralplan 规则顺序审查.

## [2026-05-16 14:38:44] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [状态]: 进入 deliberate Planner 草案

### 决策
- 本规划按 deliberate 模式处理.
- 原因: `@window-activate` 会改变桌面状态,`@window-close` 允许显式 `terminate` / `kill` 升级,属于有副作用的控制能力.

### 下一步
- 生成 `.omx/drafts/rdog-window-control-ralplan-planner-draft.md`.
- 草案必须包含 RALPLAN-DR summary,pre-mortem,expanded test plan.

### 状态
**Planner 草案生成中** - 只生成规划文件,不修改源码.

## [2026-05-16 14:40:57] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Planner]: 草案完成

### 已生成
- `.omx/drafts/rdog-window-control-ralplan-planner-draft.md`.

### 草案重点
- 选择独立 `@window-*` 协议和 `control_window` backend,而不是把窗口生命周期塞进 `@ax-*`.
- deliberate pre-mortem 已覆盖误关/误杀,桌面状态不可预测,平台能力假成功.
- 测试计划覆盖 unit/integration/live E2E/observability.

### 状态
**进入 Architect 审查** - 下一步审查架构边界和最强反方观点.

## [2026-05-16 14:58:23] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Architect]: 审查完成

### 执行说明
- 原生 Architect 子任务超过等待窗口未返回,已关闭.
- 主会话按 ralplan 要求完成等价 Architect 审查,输出 `.omx/drafts/rdog-window-control-architect-review.md`.

### Verdict
- ITERATE.

### 必须修订
- `window_id` 需要明确为短期 locator,补 `snapshot_id` / freshness / stale 语义.
- Space/fullscreen 不应写成 Phase 1 自动化硬保证;schema/recipe 必须支持,自动 E2E 不稳定时要 manual-gated test 或 limitation note.
- `terminate` / `kill` 需要双重显式性,不允许 ambiguous auto-select 后升级杀进程.
- Phase 1 不应同时改普通 `@click` / `@key` 的 `activate:true`.

### 状态
**进入 Critic 审查前修订** - 先按 Architect 必须项修订 Planner 草案.

## [2026-05-16 15:01:09] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Planner]: 已按 Architect 修订

### 已修订
- `window_id` 明确为短期 locator,补 `snapshot_id` / `observed_at` / stale/code 64 语义.
- `strategy:"terminate"` / `strategy:"kill"` 需要显式 strategy 和唯一 target,不允许 ambiguous auto-select.
- Phase 1 不修改普通 `@click` / `@key`;agent 先 `@window-activate`,再执行输入动作.
- Space/fullscreen 保持 schema/recipe 支持,自动化不稳时走 manual-gated test 或 limitation note.

### 状态
**进入 Critic 审查** - 检查计划一致性,验收可测性和风险缓解.

## [2026-05-16 15:06:38] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Critic]: 第一轮审查完成

### Verdict
- ITERATE.

### 必须修订
- 补 `strategy:"terminate"` / `strategy:"kill"` 响应字段契约,包括 `process_scope` 和 `target_pid`.
- 收紧验收第 5 条,明确 graceful close 可以 select,terminate/kill 不允许 ambiguous auto-select.
- live ignored E2E 每个场景必须断言 find 前状态,activate step report,activate 后可交互证据,以及后续真实交互或 graceful close 效果.

### 状态
**进入 Planner 第二轮修订** - 补齐上述 3 项后再复核.

## [2026-05-16 15:09:26] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Critic]: 短复核修订

### 复核结果
- Critic 短复核仍为 ITERATE.
- 只剩 `strategy:"kill"` failure 示例缺 `snapshot_id` / `observed_at_unix_ms`,以及 terminate/kill 通用 MUST 字段契约需要独立写明.

### 已修订
- kill failure 示例已补 `snapshot_id` 和 `observed_at_unix_ms`.
- terminate/kill 成功或失败响应必须包含的字段已写成 MUST 条款.

### 状态
**准备最终复核** - 若无新增问题,进入 final plan/PRD/test-spec 输出.

## [2026-05-16 15:15:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 输出最终 ralplan 文件

### 即将执行
- 创建 `.omx/plans/rdog-window-control-consensus-plan.md`。
- 创建 `.omx/plans/prd-rdog-window-control.md`。
- 创建 `.omx/plans/test-spec-rdog-window-control.md`。
- 对 Mermaid、Markdown diff 和 OMX active state 做收尾验证。

### 状态
**最终文件生成中** - 只写规划文件和本支线 task plan,不进入实现。

## [2026-05-16 15:21:27] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: ralplan 输出和验证完成

### 已完成
- [x] 阶段1: 读取 deep-interview spec 和现有 AX/control 代码事实.
- [x] 阶段2: Planner 生成 RALPLAN-DR 草案.
- [x] 阶段3: Architect 顺序审查.
- [x] 阶段4: Critic 顺序审查.
- [x] 阶段5: 根据反馈修订并输出 consensus plan,PRD,test-spec.

### 输出文件
- `.omx/plans/rdog-window-control-consensus-plan.md`
- `.omx/plans/prd-rdog-window-control.md`
- `.omx/plans/test-spec-rdog-window-control.md`

### 验证
- `rg` 复核 terminate/kill 必填字段, ambiguous auto-select, live E2E full-loop 约束已在计划文件中出现.
- `beautiful-mermaid-rs --ascii` 已验证 consensus plan 中 flowchart 和 sequenceDiagram 两个 Mermaid 块.
- `git diff --check -- .omx/plans/... task_plan__window_plan.md` 通过.
- `omx state clear --input '{"mode":"ralplan","all_sessions":true}' --json` 清理了 3 个 ralplan state 文件.
- `omx state list-active --json` 返回 `{"active_modes":[]}`.

### 状态
**已完成** - 本轮只完成 planning handoff,未进入源码实现.

## [2026-05-16 16:38:37] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [Ralph]: 进入实现执行

### 目标
按 `.omx/plans/rdog-window-control-consensus-plan.md` 落地 Phase 1, 完成 `@window-find` / `@window-activate` / `@window-close` 协议骨架、macOS backend、Windows/Linux stub、skill 文档和 live ignored E2E.

### 阶段
- [ ] 阶段1: 读取实现触点和现有测试骨架,确认最小改动面.
- [ ] 阶段2: 实现 protocol/model/executor 骨架.
- [ ] 阶段3: 实现 macOS backend 与 non-macOS stub.
- [ ] 阶段4: 实现和补齐测试,包括 live ignored E2E.
- [ ] 阶段5: 更新 spec/skill/docs.
- [ ] 阶段6: 验证,architect 审核,deslop,回归复验,收尾.

### 已知约束
- Phase 1 不改普通 `@click` / `@key` 的隐式 activate 语义.
- `window_id` 必须是 short-lived locator,配套 `snapshot_id` 或 `observed_at_unix_ms`.
- terminate/kill 需要显式 strategy,唯一目标,以及完整审计字段.
- Windows/Linux 先 honest `unsupported` / `limited`,不能假成功.
- 现有 mouse E2E 脏文件不是本轮改动面,不要碰.

### 状态
**正在读取实现触点** - 先锁定协议,backend 和测试入口,再开始修改源码.

## [2026-05-16 16:55:12] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 继续实现 live E2E 与文档同步

### 当前结果
- `control_window` 协议骨架, macOS backend, `control_protocol` / `control_actions` / `control_core` 接线已完成首轮编译.
- focused tests 已证明 parser 和 structured invalid-input error path 可工作.

### 下一步
- 创建 `tests/control_window_e2e.rs`,先覆盖 occluded/minimized/hidden app + graceful close 的 live ignored 场景.
- 同步 `specs/rdog-window-control-plan.md`, `AGENTS.md` 索引和 `rdog-control` skill 参考文档.
- 然后跑更广的编译/测试验证,收敛 warnings.

### 状态
**进入 live E2E + docs 阶段** - 先补动态证据面,再做全面回归.

## [2026-05-16 17:01:59] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 继续 Phase 1 收口

### 当前判断
- 现象: `@window-find` / `@window-activate` / `@window-close` 首轮实现已在源码中,focused parser/error-path test 已通过.
- 假设: 当前最主要缺口不是协议连线,而是缺少 live ignored E2E 证明被遮挡/最小化/hidden app 窗口能被 find + activate/close,以及长期 spec/skill 索引尚未同步.
- 验证计划: 先复用 `tests/control_ax_e2e.rs` 的 live harness 增加 `tests/control_window_e2e.rs`,再补 `specs/rdog-window-control-plan.md` 与索引,最后跑 compile/test/check 收尾.

### 下一步
- 读取现有 AX live E2E harness 与窗口控制源码细节.
- 编写 window live ignored E2E,优先覆盖 hidden/minimized/occluded/graceful close.
- 同步长期规格和 skill 文档,并清理当前实现引入的 warnings.

### 状态
**正在进入实现与验证收口** - 先补真实动态证据,再做文档与回归验证.

## [2026-05-16 17:25:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: Phase 1 实现与 live E2E 收口完成

### 已完成
- [x] 阶段1: 读取实现触点和现有测试骨架,确认最小改动面.
- [x] 阶段2: 实现 protocol/model/executor 骨架.
- [x] 阶段3: 实现 macOS backend 与 non-macOS stub.
- [x] 阶段4: 实现和补齐测试,包括 live ignored E2E.
- [x] 阶段5: 更新 spec/skill/docs.
- [x] 阶段6: 验证与回归复验收尾.

### 关键结果
- 新增 `src/control_window.rs` 与 `src/control_window/macos.rs`,接入 `@window-find` / `@window-activate` / `@window-close`.
- 新增 `tests/control_window_e2e.rs`,真实覆盖 TextEdit 的 occluded / minimized / hidden / graceful close 全链路.
- 新增 `specs/rdog-window-control-plan.md`,并同步 repo `AGENTS.md` 与全局 `rdog-control` skill 文档.
- live E2E 过程中发现 macOS hidden app 的 `unhide_app` 首轮 JXA 路径不稳,已改成 pid -> app name -> activate fallback 的多路径恢复.

### 验证
- `cargo fmt -- --check`
- `cargo build --package rustdog --bin rdog`
- `cargo test --package rustdog --bin rdog -- control_window::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_window_commands --exact --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_forward_structured_invalid_input_json --exact --nocapture`
- `cargo test --package rustdog --test control_window_e2e --no-run`
- `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture`
- `cargo test --tests --no-run`
- `git diff --check`
- `beautiful-mermaid-rs --ascii < /tmp/rdog-window-control-mermaid-1.mmd`
- `beautiful-mermaid-rs --ascii < /tmp/rdog-window-control-mermaid-2.mmd`

### 状态
**已完成** - 当前实现、长期规格和真实桌面验证已经打通,下一步可进入 diff review 和 local commit.

## [2026-05-16 17:39:39] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [收尾]: fresh 验证与 Ralph runtime 清理完成

### fresh 验证
- `git rev-parse --short HEAD` -> `26a7005`
- `cargo test --package rustdog --bin rdog -- control_window::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_window_commands --exact --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_forward_structured_invalid_input_json --exact --nocapture`
- `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture`

### runtime 收尾
- `omx cancel ralph` -> `Cancelled: ralph`
- `omx state list-active --json` -> `{\"active_modes\":[]}`

### 状态
**真正完成** - 代码、真实桌面验证、local commit 与 Ralph runtime 都已收口.

## [2026-05-16 17:54:37] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [行动]: 修复 review 暴露的窗口控制边界问题

### 现象
- review 指出 `window_id` follow-up 仍依赖内部 `find(limit=20)`,可能把真实存在的窗口误报成 stale.
- `current_space` 当前会被“同 pid 的任意可见窗口”污染.
- `select:\"frontmost\"` 只按 app frontmost 选,多窗口同 app 时可能选错 window.

### 计划
- 让 `window_id` 跟进路径直接按 locator 解析,不再依赖内部截断后的 `find` 结果.
- 重新保留窗口级 `focused`,并让 `select:\"frontmost\"` 优先选 focused window.
- 收紧 `current_space` 判定,去掉同 pid 任意窗口兜底.
- 补针对性单测,再跑 focused tests 验证.

### 状态
**正在修 review 问题** - 先修协议闭环,再修多窗口语义.

## [2026-05-16 17:57:39] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: review 修复已落地并完成 focused 验证

### 已修复
- `window_id` follow-up 改为 direct locator 解析,不再依赖内部 `find(limit=20)` 截断结果.
- `current_space` 判定去掉“同 pid 任意可见窗口”兜底,避免被其他窗口污染.
- `select:\"frontmost\"` 改为优先选择 focused window,其次才退回 app frontmost.

### focused 验证
- `cargo fmt`
- `cargo test --package rustdog --bin rdog -- control_window::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_window::macos::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests::parse_should_support_window_commands --exact --nocapture`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`

### 状态
**本轮 review 问题已修完** - 当前剩余动作主要是 review 新 diff 和决定是否做 local commit/amend.

## [2026-05-16 18:20:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [继续]: 收尾 Finder 夹具 live E2E 稳定化

### 现象
- `cdcb56e` 之后的 backend focused tests 已通过, 当前未提交改动集中在 `tests/control_window_e2e.rs`.
- Finder 迁移版测试已经可以 `cargo test --package rustdog --test control_window_e2e --no-run` 编译通过.
- `cargo fmt -- --check` 失败, 当前只剩格式化差异, 还没有 fresh live E2E 证据.

### 当前假设
- 主要风险更像是 live 夹具行为稳定性,而不是 Rust 编译或 review 修复把 backend 再次打坏.
- 需要先把测试文件收口到仓库格式,再用 live ignored E2E 证明 Finder 夹具是否比 TextEdit 更稳定.

### 下一步
- 格式化并复核 `tests/control_window_e2e.rs`.
- 运行 `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture`.
- 如果 live 失败,基于 fresh 动态证据判断是 Finder 夹具问题还是 backend 真回归.

### 状态
**正在做 live E2E 稳定化收尾** - 先拿到 fresh 动态证据,再决定是否提交新 commit.

## [2026-05-16 18:27:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [修订]: 降低 live E2E 桌面窗口污染

### 现象
- fresh live E2E 失败在 `occluded` 准备阶段.
- `@window-find` 返回 Finder 目标窗口 `current_space:false`, `interactable:false`, `occluded:false`,说明激活 Terminal 没有稳定制造同一 Space 的遮挡状态.
- 用户反馈测试开启的 Terminal 太多,希望测试也顺便关掉测试关闭功能.

### 假设
- Terminal daemon 宿主窗口应当作为测试资源,和 Finder/TextEdit 夹具一样在 Drop 中清理.
- 遮挡状态不应依赖 Terminal 宿主窗口,应改为测试自己创建并清理的轻量 occluder 窗口.
- `@window-close` 对 Finder 目标窗口的真实关闭断言仍应保留,这是本测试的核心验收之一.

### 下一步
- 为 Terminal daemon 脚本设置唯一窗口标题 marker,Drop 时 kill listener 后关闭匹配 marker 的 Terminal 窗口.
- 增加可清理的 TextEdit occluder 夹具,只用于制造遮挡,不作为被测窗口.
- 重新跑格式、编译和 live ignored E2E,确认窗口资源能收干净.

### 状态
**正在修复测试资源生命周期** - 优先解决窗口污染,再复验真实 close 能力.

## [2026-05-16 23:17:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [完成]: Finder live E2E 稳定化与 backend current_space 修复

### 现象
- TextEdit 作为被测窗口时, live E2E 偶发 `match_count:0`,不适合作为稳定 fixture.
- Finder 作为被测窗口时, `@window-find` 曾返回 `current_space:false`,即使 CGWindowList 里同 pid 窗口真实存在.
- 最小探针显示 CGWindowList 中 Finder 窗口 `owner=访达`, `name=T`,但 rect 与 AX candidate 完全一致.
- live E2E 过程中还暴露了测试会残留多个 `rdog-window-e2e-*.command` Terminal 窗口的问题.

### 已验证结论
- backend 的 `match_visible_window` 不能要求 AX title 与 CGWindow name 同时匹配; 在同 pid 下, title 或 rect 任一真实命中即可证明当前 Space 可见.
- `window_id` 是 short-lived locator,测试每个状态段必须使用 fresh `@window-find` 返回的 `window_id`,不能复用最初的 id.
- Terminal daemon 宿主窗口必须按测试专用标题前缀自动清理,否则 live ignored E2E 会污染桌面.

### 已完成
- 修正 `src/control_window/macos.rs` 的 visible-window 匹配逻辑,并补充 focused 单测.
- 将 `tests/control_window_e2e.rs` 改为 Finder 被测窗口 + TextEdit occluder,并在 Drop 中清理 Finder/TextEdit/Terminal 测试资源.
- live E2E 已通过完整链路: occluded -> activate, minimized -> activate, hidden -> activate, graceful close -> `match_count == 0`.

### 验证
- `cargo test --package rustdog --bin rdog -- control_window::macos::tests --nocapture` -> 6 passed.
- `cargo test --package rustdog --test control_window_e2e --no-run` -> passed.
- `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture` -> 1 passed, 132.60s.
- `cargo fmt -- --check` -> passed.
- `git diff --check` -> passed.
- live 后检查 Terminal/TextEdit 测试资源为空; Finder 剩余 `T` 窗口不匹配测试专用长标题,未处理用户窗口.

### 状态
**window live E2E 稳定化已完成** - 下一步只提交本轮 window 相关文件,不纳入其他脏改动.
