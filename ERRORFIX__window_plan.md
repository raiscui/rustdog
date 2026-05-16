# 错误修复记录: rdog window control

## [2026-05-16 23:17:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 修复: Finder 可见窗口被误判为非当前 Space

### 问题
- live E2E 中 Finder 目标窗口已通过 AX 找到,但 state 显示 `current_space:false`,导致 occluded/interactable 验证不稳定.
- 测试还会残留多个 `rdog-window-e2e-*.command` Terminal 窗口.

### 原因
- backend 的 `match_visible_window` 在 AX title 和 CGWindow rect 同时存在时要求两者都匹配.
- macOS 上 CGWindow name 可能退化成短名,例如 Finder 目标窗口 AX title 是测试目录名,CGWindow name 是 `T`,但 rect 完全一致.
- live E2E 复用了最初的 `window_id`,违背 short-lived locator 语义.

### 修复
- 同 pid 下 title 或 rect 任一真实命中即可作为当前 Space 可见证据.
- 增加 rect 命中但 title 不同的 focused 单测.
- E2E 每个状态段重新获取 fresh `window_id`.
- Terminal/TextEdit/Finder fixture 增加清理逻辑,并在启动前清理旧测试 Terminal 窗口.

### 验证
- `cargo test --package rustdog --bin rdog -- control_window::macos::tests --nocapture` -> 6 passed.
- `cargo test --package rustdog --test control_window_e2e --no-run` -> passed.
- `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture` -> 1 passed, 132.60s.
- `cargo fmt -- --check` -> passed.
- `git diff --check` -> passed.
