## [2026-05-21 08:48:19] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 问题: visual-only `@observe` 静默吞掉 observation 记录失败

### 现象

- Deslop inventory 发现 `src/control_observation/observe.rs` 中 visual-only primary observation 使用 `.ok().flatten()`。
- 如果 `record_visual_observation()` 因 observation store lock 或 durable state 写入失败而返回 error,原代码会把错误吞掉,最终 response 可能没有 primary observation。

### 原因

- 代码为了在 `Option::or_else()` 链里接入 `io::Result<Option<ObservationHeader>>`,临时把 Result 压成 Option。
- 这破坏了 observation facade 的单一真相源: visual path 明明需要记录 observation header,但失败不会传到调用方。

### 修复

- 新增 `select_primary_observation()` helper。
- 按 AX observation -> window observation -> visual observation 的优先级显式选择 primary observation。
- visual-only 需要记录 observation 时,直接传播 `record_visual_observation()` 的 error。

### 验证

- 新增 `select_primary_observation_should_record_visual_when_it_is_the_only_section`。
- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`: PASS,4 passed。
- Post-deslop 完整验证:
  - `cargo fmt -- --check`: PASS。
  - `cargo check --package rustdog --bin rdog`: PASS。
  - `git diff --check`: PASS。
  - `cargo test --package rustdog --bin rdog -- --nocapture`: PASS,252 passed。
- `cargo test --package rustdog --test control_lanes -- --nocapture`: PASS,8 passed,1 ignored。
- `cargo test --package rustdog --test control_mode -- --nocapture`: PASS,1 passed。

## [2026-05-21 17:37:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 问题: observe.rs 拆分后的测试可见性编译错误

### 现象

- 第一轮 `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture` 编译失败。
- 错误为 `select_primary_observation`、`ProducedSections`、`render_observe_response` 不能从子模块以更宽可见性 re-export。
- 后续完整 bin test 又发现 `control_protocol/tests.rs` 仍从 `control_observation::observe` 构造 `ObserveMode` / `ObserveTarget` 期望值。

### 原因

- 拆分时把测试便利入口写成了 `pub(super) use`,实际比子模块内部 `pub(super)` 项对外暴露得更宽。
- 为了消除生产态 `unused_imports` warning,一度收窄了 `ObserveMode` / `ObserveTarget` re-export,但既有 protocol tests 仍依赖这个测试构造入口。

### 修复

- producer / response 的内部项改为 `#[cfg(test)]` 私有 `use`,只给同模块测试可见,不扩大生产 API。
- `ObserveMode` / `ObserveTarget` 改为 `#[cfg(test)] pub(crate) use`,只在测试构建里维持 protocol tests 的既有入口。
- 生产态根模块仍只公开 `build_observe_outcome`、`parse_observe_payload` 和 `ObserveRequest`。

### 验证

- `cargo test --package rustdog --bin rdog control_observation::observe::tests -- --nocapture`: PASS,4 passed。
- `cargo test --package rustdog --bin rdog control_protocol::tests::parse_should_support_observe_command -- --exact --nocapture`: PASS,1 passed。
- `cargo test --package rustdog --bin rdog control_core::tests::explicit_request_should_render_observe_without_action_executor -- --exact --nocapture`: PASS,1 passed。
- `cargo check --package rustdog --bin rdog`: PASS,无 warning。
- `cargo test --package rustdog --bin rdog -- --nocapture`: PASS,252 passed。
