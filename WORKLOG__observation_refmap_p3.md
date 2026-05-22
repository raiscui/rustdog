## [2026-05-20 20:27:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: observation semantic re-find P3 Ralplan

### 任务内容

- 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P3 可落地计划。
- 范围限定为 semantic re-find、`@selector-refind`、confidence scoring、blocked/rebound/not_found/needs_disambiguation 决策、verify/audit 契约和测试矩阵。
- 明确排除 P4 `@observe`、P5 mouse ref 化、旧 `@eN` 跨重启复活和默认 action by selector。

### 完成过程

- 读取 `$ralplan` 工作流约束和 `humanizer-zh` 表达约束。
- 回读 roadmap、P2 plan、P2 落地记录、P2 reviewer 修复记录和当前 selector / durable / observation 代码触点。
- 记录 P3 brownfield 事实到 `notes__observation_refmap_p3.md`。
- 创建 `.omx/context/observation-refmap-p3-20260520T120059Z.md`。
- 创建 `.omx/drafts/ralplan-rdog-observation-refmap-p3-draft.md`。
- 进行 Architect -> Critic -> Architect -> Critic 共识审查。
- 第一轮 Critic 要求把 `blocked`、`fresh_target` verify、scoring conformance 升级成硬契约,已采纳。
- 第二轮 Architect 要求跳过 verify 时保留 audit evidence/log,已采纳。
- 第二轮 Critic `APPROVE` 后,提升为 `.omx/plans/ralplan-rdog-observation-refmap-p3.md`。

### 验证

- `beautiful-mermaid-rs --ascii` 验证 P3 architecture flowchart。
- `beautiful-mermaid-rs --ascii` 验证 P3 selector-refind sequenceDiagram。
- `rg` 检查未收口占位 / 摇摆措辞无命中。
- `git diff --check -- ...` 对本轮 P3 文件通过。
- `omx state get-status --input '{"mode":"ralplan"}' --json` 显示 `active:false,current_phase:"complete"`。

### 总结感悟

- P3 的关键不是“selector 命中后自动做动作”,而是把 stale 后的恢复决策变成可审计协议对象。
- `@selector-refind` 与 `@selector-resolve` 分开是必要的。前者承载 recovery decision,后者保持 P2 的 low-level dry-run probe。
- `fresh_target` 最容易被误读成动作成功。本轮把 verify_hint、skip audit 和 action-by-selector 延期写成硬契约,能减少后续实现滑坡。

## [2026-05-20 23:18:46] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: observation semantic re-find P3 实现落地

### 任务内容

- 按 `.omx/plans/ralplan-rdog-observation-refmap-p3.md` 落地 P3 semantic re-find。
- 新增 `@selector-refind` 协议入口,并保持 P2 `@selector-resolve` dry-run 语义不变。
- 锁定 rebound / needs_disambiguation / not_found / blocked response contract、`rdog.selector.score.v1` scoring contract 和 verify/audit 规则。
- 更新 `rdog-control` skill 与 repo 使用文档。

### 完成过程

- 在 `src/control_protocol.rs` 增加 `SelectorRefind` parser,支持 `selector_id`、`limit`、`policy`、`min_confidence`、`include_explanations`、`include_history` 和 `source`。
- 在 `src/control_core.rs` 直接 dispatch `build_selector_refind_response_json()`,并在 `src/control_actions.rs` 防止 selector recovery 误进 side-effect executor。
- 新增 `src/control_observation/refind.rs`,实现 `rdog.selector.refind.v1` response、`rdog.selector.score.v1` deterministic scoring、confidence band、hard gate、stable tie-break、blocked response、verify_hint 和 recovery_recipe。
- 升级 stale / expired durable payload,增加 `refind_available`、`refind_command`、`recovery_recipe` 和“refind 不代表动作完成”的说明。
- 新增 `tests/fixtures/observation_selectors/selector_refind_*.json` golden fixtures,覆盖 rebound、needs_disambiguation、not_found、permission/backend/schema blocked 和 verify-skip audit。
- Deslop pass 修正了 `observation:null` 不能算 fresh target 的边界,避免 `rebound` 生成空 verify target。
- 更新 `.codex/skills/rdog-control/SKILL.md`、`references/protocol.md`、`references/control-workflow.md` 和 `specs/code-agent-rdog-control-usage.md`。
- Architect reviewer 已 APPROVE,无阻断项。

### 验证

- `cargo fmt -- --check`: PASS。
- `cargo test --package rustdog --bin rdog control_observation::refind::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_selector_commands`: PASS。
- `cargo test --package rustdog --bin rdog control_observation::selector::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 4 passed。
- `cargo test --package rustdog --bin rdog control_observation::tests`: 10 passed。
- `cargo test --package rustdog --bin rdog control_core::tests`: 13 passed。
- `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- `cargo test --package rustdog --bin rdog config::tests`: 26 passed。
- `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`: 23 passed,2 ignored。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: PASS。
- `git diff --check`: PASS。

### 总结感悟

- `@selector-refind` 要保持只读恢复层定位。它可以给出 fresh ref,但不能偷偷做 AXPress、focus、set-value 或 mouse fallback。
- `blocked` 作为正常 response 很重要。权限、backend、schema 问题不应该被 action error 混淆,否则 agent 会走错恢复分支。
- `fresh_target` 必须和 `verify_hint` 绑定。没有真实 `observation_id + ref` 的候选,哪怕匹配字段得分很高,也不能自动 rebound。
