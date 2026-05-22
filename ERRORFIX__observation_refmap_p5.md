# ERRORFIX: observation refmap P5 mouse ref 化

## [2026-05-21 22:07:10] [Session ID: aa882315-9cda-4dd8-9e29-eb4615a849c1] 问题: drag selector no-action 不能返回 Unsupported

### 现象

- Architect 初审指出 `@drag` 的 selector endpoint 在 no-action 场景下不能返回 `Unsupported` error。
- `@drag` 的 `from` / `to` 任一端如果是 selector handoff、blocked、not_found 或 auto-refind 未满足 gate,都应该返回结构化 `performed:false`。

### 原因

- mouse target 准备层早期更偏向 click / wheel 的单 target 模型。
- drag 有两个 endpoint,如果其中一个 endpoint 给出 `PreparedEndpoint::NoAction`,必须在 prepare 层统一转成 no-action response,不能继续落到 plan builder 或 Unsupported error。

### 修复

- `prepare_drag_request()` 已分别处理 `from` 和 `to` endpoint。
- 任一 endpoint 返回 no-action 时,直接返回 `PreparedMouseRequest::NoAction`。
- 新增/保留测试:
  - `drag_selector_handoff_should_return_no_action_instead_of_error`
  - `drag_selector_auto_refind_without_match_should_return_no_action`

### 验证

- `cargo test --package rustdog --bin rdog control_mouse::target_tests`: 6 passed。
- `cargo test --package rustdog --bin rdog`: 260 passed。
- `cargo test --package rustdog --test control_lanes`: 8 passed, 1 ignored。
- `cargo test --package rustdog --test control_mode`: 1 passed。
- `cargo check --package rustdog --bin rdog`: 通过。
- `git diff --check`: 通过。
