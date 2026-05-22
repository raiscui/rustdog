## [2026-05-22 14:37:24] [Session ID: DECD1A1F-DE7A-4689-8762-F23D9FCF9708] 错误修复: AX observation ref mouse target live timeout

### 问题

- 在 macOS live lane 上,`@observe` 能返回 `rdog.observe.v1` 和 `@eN` refs。
- `@ax-get target:{ref,observation_id}` 能解析同一个 ref。
- raw `@mouse-move {x,y,coordinate_space:"os-logical"}` 能成功。
- 但 `@mouse-move target:{ref,observation_id}` 在默认 Zenoh 3 秒 timeout 下表现为 `Zenoh session bridge subscriber 在收到结果前关闭`。

### 原因

- ref mouse 的 AX current rect 解析调用了 `resolve_current_ax_target_rect()`。
- 旧实现为了一个已经明确的 AX backend id,仍会重建 `depth:8,max_elements:5000` 的完整 AX snapshot。
- 在真实桌面窗口较多时,这条路径超过 Zenoh session client 默认 3 秒等待窗口,导致 client 看不到 terminal `@response`。

### 修复

- 在 `src/control_ax.rs` 中新增 `direct_ax_target_id()`。
- 当 target 是直接 `id` 或 observation ref 时,先解析出 backend id,然后走平台级 direct rect resolver。
- 在 `src/control_ax/macos.rs` 中新增 `resolve_current_target_rect()`。
- macOS direct resolver 使用现有 AX target id 解析和 `retain_target_element()` 路径,直接读取当前 element/window rect,不再为 mouse ref 重建完整 snapshot。
- 保留 semantic target 的 snapshot fallback,避免改变语义查询路径。

### 验证

- `cargo test --package rustdog --bin rdog control_ax::tests::direct_ax_target_id_should_resolve_ids_and_observation_refs_without_snapshot -- --exact --quiet`: 通过。
- `cargo test --package rustdog --bin rdog control_mouse::target_tests --quiet`: 通过。
- live smoke `/tmp/rdog-observe-smoke-final-summary.json`: `@mouse-move#102` 返回 `status:"ok"` 且 `target_resolution.source:"observation_ref"`。
