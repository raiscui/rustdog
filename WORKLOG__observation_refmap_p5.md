# WORKLOG: observation refmap P5 mouse ref 化计划

## [2026-05-21 20:05:17] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 任务名称: 创建 P5 mouse ref 化可落地计划

### 任务内容

- 根据 `specs/rdog-observation-scoped-refmap-plan.md` 创建 P5 可执行计划。
- 计划落地到 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。
- 本轮只做规划,没有修改 Rust runtime 源码。

### 完成过程

- 已建立 P5 context snapshot: `.omx/context/observation-refmap-p5-20260521T114804Z.md`。
- 已基于 roadmap、P4、mouse coordinate contract、non-mouse semantic contract 和当前代码触点形成 draft。
- Architect review 给出 `ITERATE`,要求 selector target 默认 no-action,并补 audit 字段、parser 互斥、spy no-action tests 和 stop rule。
- 已修订 draft v2。
- Critic review 给出 `APPROVE`。
- 已把最终计划写入 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。

### 验证证据

- `beautiful-mermaid-rs --ascii` 验证最终计划里的 2 个 Mermaid block,结果均为 exit=0。
- `git diff --check` 验证最终计划和支线上下文文件,无空白错误。
- `rg` 复查最终计划包含 `TARGET_RECT_UNAVAILABLE` no-action 验收、`@hover` 不新增说明、selector gated stop rule、spy backend no-action 测试项。

### 总结感悟

- P5 的关键不是让 mouse 更方便,而是让 mouse fallback 进入可审计的 observation/ref/selector 链路。
- selector target 必须先保持 no-action handoff,否则容易把恢复过程偷换成隐藏点击主路径。
- `src/control_mouse.rs` 已经超过项目健康线,下一轮实现必须先做结构拆分。

## [2026-05-21 22:07:10] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] 任务名称: P5 mouse ref 化 Ralph 收尾

### 任务内容

- 继续执行 `.omx/plans/ralplan-rdog-observation-refmap-p5.md`。
- 收尾 P5 mouse/ref/selector 实现,重点处理 Architect APPROVE 后的结构健康线备注。
- 只碰 P5 相关文件,没有回滚其他未提交改动。

### 完成过程

- 把 `src/control_mouse/target.rs` 的内联测试拆到 `src/control_mouse/target_tests.rs`。
- 在 `src/control_mouse.rs` 注册 `#[cfg(test)] mod target_tests;`。
- `src/control_mouse/target.rs` 从 816 行降到 592 行,`src/control_mouse/*.rs` 最大子文件为 669 行。
- 按 `ai-slop-cleaner` scope 做了 P5 文件清理检查,确认 fallback 命中主要是显式协议语义,不是吞错式静默 fallback。
- 清理了 `prepare_click_request` 坐标分支的重复读取,改成复用单一 `MousePoint` 局部值。
- 读取已有 Ralph 子智能体复审状态,最终 Architect verdict 为 `APPROVE`;其唯一备注的 800 行结构问题已修正。

### 验证证据

- `cargo fmt -- --check`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::target_tests`: 6 passed。
- `cargo test --package rustdog --bin rdog control_actions::tests::selector_mouse_target_without_auto_refind_should_return_no_action_before_backend -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_mouse_ref_and_selector_targets -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_reject_invalid_mouse_payloads -- --exact`: 1 passed。
- `cargo test --package rustdog --bin rdog`: 260 passed。
- `cargo test --package rustdog --test control_lanes`: 8 passed, 1 ignored。
- `cargo test --package rustdog --test control_mode`: 1 passed。
- `cargo check --package rustdog --bin rdog`: 通过。
- `git diff --check`: 通过。
- post-deslop 重新跑同一套关键回归后仍然通过。

### 总结感悟

- P5 最容易滑向“鼠标默认主路径”,所以 response 必须一直暴露 `target_resolution.source` 和 selector gate。
- `target.ref` 是执行前重新定位 current rect,不是复用旧观察里的静态 rect。
- selector `auto_refind:true` 的价值在于可审计恢复,不是绕过 verify 直接点击。
