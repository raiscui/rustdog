## [2026-05-19 23:50:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: P1 durable observation state / selector 细化方案

### 任务内容
- 基于 P0 已落地实现,创建 P1 durable observation state / selector 细化方案。
- 明确 P1 只负责 daemon-owned durable state、observation metadata、selector draft / selector envelope 和 restart recovery hint。
- 明确 P1 不复活短期 `@eN`,不做自动 semantic re-find,不新增 `@observe`,不做 mouse ref 化。

### 完成过程
- 阅读 P0 支线 LATER_PLANS,确认 P1 触发条件已经满足。
- 阅读 roadmap、P0 plan、`src/control_observation.rs`、AX/window/screenshot 相关实现和 config 现状。
- 记录 brownfield 事实到 `notes__observation_refmap_p1.md`。
- 设计并落盘 `.omx/plans/ralplan-rdog-observation-refmap-p1.md`。
- 将 P2/P3/P4/P5 等非 P1 范围写入 `LATER_PLANS__observation_refmap_p1.md`。

### 验证
- `rg -n '```mermaid|TODO|TBD|\\[ \\] 阶段' ...`: 无命中。
- `git diff --check -- .omx/plans/ralplan-rdog-observation-refmap-p1.md task_plan__observation_refmap_p1.md notes__observation_refmap_p1.md WORKLOG__observation_refmap_p1.md LATER_PLANS__observation_refmap_p1.md task_plan__observation_refmap_plan.md LATER_PLANS__observation_refmap_plan.md`: 通过。
- `wc -l`: P1 plan 和支线文件均未超过 1000 行。
- 方案文件不含 Mermaid 代码块,不需要 `beautiful-mermaid-rs`。

### 总结感悟
- P1 的核心不是“把 ref 存下来”,而是“把恢复线索存下来”。旧 ref 不能跨重启复活,否则 P0 刚钉住的短期 ref 语义会被破坏。
- JSONL + index 是当前最合适的第一步。sqlite 更像 P2/P3 需要复杂查询和候选排序后的选择。

## [2026-05-20 01:09:12] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: P1 durable observation state / selector 实现落地

### 任务内容
- 基于 `.omx/plans/ralplan-rdog-observation-refmap-p1.md` 实现 P1 durable observation state。
- 覆盖 daemon-owned state config、JSONL durable backend、selector draft / selector envelope、hint-only ref cache、真实 `selector_count`、restart hint、docs/skill 更新和验证矩阵。

### 完成过程
- 新增 `[observation]` config 并更新 `rdog_macos.toml`、`rdog_linux.toml`、`rdog_win.toml`。
- 新增 `src/control_observation/durable.rs` 和 `src/control_observation/selector.rs`。
- 修改 `src/control_observation.rs`,让 selector draft 在 header 生成后带上 `observation_id` 再落 durable store。
- 修改 `src/control_ax.rs` 和 `src/control_window.rs`,让 AX/window observation 生成 selector drafts。
- 修改 `src/daemon.rs` 和 `src/zenoh_control.rs`,让 TCP daemon 与 Zenoh router daemon 初始化 durable observation state。
- 修改 screenshot/AX/window/config/Zenoh 相关测试,覆盖 selector_count、restart 不复活旧 ref、JSONL reload/replay/retention、config validation。
- 更新 `.codex/skills/rdog-control/*` 和 `specs/code-agent-rdog-control-usage.md`,明确 durable hint 只是恢复线索。
- 执行 bounded deslop,收窄 index replay fallback,并让 reobserve hint 基于 selector draft 生成。

### 验证
- `cargo fmt -- --check`: passed。
- `cargo test --package rustdog --bin rdog control_observation::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_observation::durable::tests`: 3 passed。
- `cargo test --package rustdog --bin rdog control_ax::tests`: 14 passed。
- `cargo test --package rustdog --bin rdog control_ax::query::tests`: 5 passed。
- `cargo test --package rustdog --bin rdog control_window::tests`: 6 passed。
- `cargo test --package rustdog --bin rdog screenshot::tests`: 17 passed。
- `cargo test --package rustdog --bin rdog config::tests`: 26 passed。
- `cargo test --package rustdog --test zenoh_router_client -- --test-threads=1`: 23 passed,2 ignored。
- `git diff --check`: passed。

### 总结感悟
- durable state 的正确定位是“审计与恢复提示”,不是“把短期 backend id 存成永久定位”。
- selector draft 必须晚于 header finalization 转成 durable record,否则会诱导第二套 observation id 生成路径。
- fallback 要区分可恢复数据损坏和真实 IO/权限错误。损坏 index 可以 replay,权限错误必须暴露。
