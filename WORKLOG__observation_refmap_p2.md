## [2026-05-20 07:43:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: observation permanent selector P2 Ralplan

### 任务内容

- 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P2 可落地计划。
- 范围限定为 permanent selector schema、stable selector id、explicit dry-run resolve、fixtures 和验证矩阵。
- 明确排除 P3 semantic re-find、P4 `@observe`、P5 mouse ref 化和 side-effect action by selector。

### 完成过程

- 读取 `$ralplan` 工作流约束和 `humanizer-zh` 表达约束。
- 回读 P0/P1 roadmap、P1 plan、P1 branch notes 和当前 selector/durable 代码触点。
- 记录 P2 brownfield 事实到 `notes__observation_refmap_p2.md`。
- 创建 `.omx/context/observation-refmap-p2-20260519T234334Z.md`。
- 创建 `.omx/plans/ralplan-rdog-observation-refmap-p2.md`。
- 用 `beautiful-mermaid-rs --ascii` 验证 plan 内 flowchart 和 sequenceDiagram。
- 检查未收口占位关键字。
- 用 `git diff --check` 检查本轮 P2 文件空白格式。
- 更新 `.omx` ralplan state 为 complete。

### 总结感悟

- P2 的关键不是急着让 selector 自动点东西,而是先让 selector 成为稳定、可审计、可 dry-run resolve 的协议对象。
- 当前 P1 的最大 P2 缺口是 `selector_id` 仍从 `observation_id + ref` 派生。P2 必须先解决 stable identity,否则后续 P3 的 re-find 会建立在会漂移的身份上。
- `@selector-resolve` 作为 dry-run 是合适的过渡层。它让 agent 能验证候选,又不会破坏 P0/P1 的 short ref 语义。

## [2026-05-20 19:28:13] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: observation refmap P2 permanent selector 落地

### 任务内容

- 实现 `rdog.selector.v1` permanent selector schema、fingerprint 和 `sel-v1-*` stable selector id。
- 升级 durable selector index,支持 selector id lookup、last_seen 和真实 selector history。
- 新增 `@selector-get` 和 `@selector-resolve` dry-run 协议入口。
- 更新 `rdog-control` skill 和 `specs/code-agent-rdog-control-usage.md` 的 stale selector workflow。

### 完成过程

- 在 `src/control_observation/selector.rs` 中把 P1 draft 转为 P2 permanent selector,并用 AX element / AX window / window 三类 fixture 锁住 schema。
- 在 `src/control_observation/durable.rs` 中保存 stable selector metadata,并新增 `selector_history(selector_id, limit)`。
- 在 `src/control_observation.rs` 中实现 `selector-get` response、`selector-resolve` dry-run、0 候选 / 多候选 / backend 权限错误的结构化 selector error。
- 在 `src/control_protocol.rs`、`src/control_core.rs`、`src/control_actions.rs` 和相关测试中接入 selector command。
- 第一轮 Ralph reviewer 提出 3 个阻断后,补齐错误契约、fixture 覆盖和真实 history,第二轮复审 `APPROVED`。

### 验证

- `cargo fmt -- --check`
- `cargo test --package rustdog --bin rdog control_observation::selector::tests`
- `cargo test --package rustdog --bin rdog control_observation::durable::tests`
- `cargo test --package rustdog --bin rdog control_observation::tests`
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_selector_commands`
- `cargo test --package rustdog --bin rdog control_core::tests`
- `cargo test --package rustdog --bin rdog control_ax::tests`
- `cargo test --package rustdog --bin rdog control_ax::query::tests`
- `cargo test --package rustdog --bin rdog control_window::tests`
- `cargo test --package rustdog --bin rdog screenshot::tests`
- `cargo test --package rustdog --bin rdog config::tests`
- `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`
- `git diff --check`
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`

### 总结感悟

- P2 的核心验收不是“能返回 selector”,而是 stable identity、真实 history、可解释 dry-run 和明确错误边界同时成立。
- `include_history` 这种看起来只是便利字段的协议表面,如果没有真实 durable history 支撑,会误导 agent。以后类似字段要么做实,要么不要暴露。
- 多候选必须是结构化错误,不能让 agent 在 dry-run 结果里误以为已经拿到了可自动执行的唯一目标。
