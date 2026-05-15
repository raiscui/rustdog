## [2026-05-15 12:44:16] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 错误修复: live AX tree 读取和 E2E pipe timeout

### 问题
- Terminal.app 宿主启动的 `rdog daemon` 已经越过 Accessibility trust 检查,但 `@ax-tree` 在真实桌面上曾返回 code 70,错误分别包括 `读取 AX actions 失败: AXError -25200` 和 `读取 AX attribute `AXSubrole` 失败: AXError -25200`.
- 新增 live E2E 在 stdout 已出现完整 AX tree 的情况下仍超时,错误为 `rdog control command timed out`.

### 原因
- 真实 macOS AX 元素可能对单个 attribute/action 返回 `kAXErrorFailure` 或 `kAXErrorNotImplemented`. 这些属于 snapshot 阶段的单元素可选字段读取失败,不应破坏整棵 tree.
- 测试 harness 之前在等待 control 子进程退出时没有并发读取 stdout/stderr. 大 AX tree JSON 会填满 pipe,导致 control 子进程无法继续写完并退出.

### 修复
- `src/control_ax/macos.rs` 新增 `snapshot_optional_ax_error()`,把 snapshot 中的 `kAXErrorFailure`,`kAXErrorNotImplemented`,`kAXErrorAttributeUnsupported`,`kAXErrorNoValue`,`kAXErrorCannotComplete`,`kAXErrorInvalidUIElement` 统一降级为缺失字段或空 actions.
- 保持 `kAXErrorAPIDisabled` 为权限硬错误;`AXPress` action 执行错误仍不降级.
- `tests/control_ax_e2e.rs` 的 `wait_with_output_timeout()` 改为先取出 stdout/stderr 并在线程里 `read_to_end`,等待期间持续 drain pipe.

### 验证
- `cargo fmt -- --check`: 通过.
- `cargo test --package rustdog --bin rdog -- control_ax:: --nocapture`: 7 passed.
- `cargo test --package rustdog --test control_ax_e2e --no-run`: 通过.
- `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/.cargo/bin/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_read_real_terminal_window_and_press_real_button --exact --ignored --nocapture`: 1 passed.
- `cargo test --tests --no-run`: 通过.
- `git diff --check`: 通过.

## [2026-05-15 22:27:36] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 问题: live AX find/get E2E 使用旧 installed rdog

### 现象
- 新增 live ignored E2E 首次运行失败.
- 命令: `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/.cargo/bin/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_find_and_get_real_terminal_button --exact --ignored --nocapture`.
- 返回: `@ax-find returned protocol error code Some(64): 不支持的控制指令类型: ax-find`.

### 原因
- live 测试显式使用 `/Users/cuiluming/.cargo/bin/rdog`.
- 该 installed binary 还没有包含上一轮 `@ax-find` / `@ax-get` 新命令,所以真实 daemon 路径返回 code 64.

### 修复
- 执行 `cargo install --path .`,把 installed rdog 更新到当前源码.
- 更新后重新运行同一 live ignored E2E.

### 验证
- 待重新运行后补充.

### 复验结果
- `cargo install --path .`: 通过,已替换 `/Users/cuiluming/.cargo/bin/rdog`.
- 重新运行同一 live ignored E2E: 1 passed.
- 关键输出: `live AX find/get observed Terminal close button: target_id=pid:556/window:0/path:2`.
- 结论: 首次失败由 installed rdog 版本旧导致,不是 `@ax-find` / `@ax-get` live E2E 设计失败.
