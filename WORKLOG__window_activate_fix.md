# 工作记录: `@window-activate` over Zenoh session bridge 返回路径修复

## [2026-05-25 10:25:20] [Session ID: omx-1779670884813-rnokx6] 任务名称: 修复 `@window-activate` over Zenoh session bridge 返回路径

### 任务内容
- 修复 `@window-activate` 经 Zenoh session bridge 执行时,control 端在约 3 秒后报 subscriber closed 的问题。
- 覆盖文件: `src/zenoh_control/client_pty.rs`、`tests/zenoh_router_client.rs`。
- 使用支线上下文: `task_plan__window_activate_fix.md`、`notes__window_activate_fix.md`、`ERRORFIX__window_activate_fix.md`。

### 完成过程
- 先用 live `@window-activate` 复现失败,再用慢 `@script` 做最小可证伪实验。
- 确认问题不是 window backend 独有,而是普通 line-control response 超过 3 秒后都会触发 client-side 误判。
- 阅读 `execute_remote_request()` 后确认静态路径: `recv_timeout()` 的 `Ok(None)` 被当成 `UnexpectedEof`。
- 将 `Ok(None)` 改为 idle tick,并为普通 line-control 请求引入 60 秒 final response deadline。
- 增加 focused 回归测试 `control_should_wait_for_slow_session_channel_response`。
- 用真实 Zenoh daemon 重新验证 `@window-activate` 可以在 4.241s 后返回 final `@response`。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo nextest run --package rustdog --test zenoh_router_client control_should_wait_for_slow_session_channel_response`: 通过。
- `cargo check --quiet`: 通过。
- `cargo test --package rustdog --test control_window_e2e --no-run`: 通过。
- `git diff --check`: 通过。
- live smoke: `@window-activate#2:{window_id:"pid:8231/window:0"}` 返回 `status:"ok"`,exit status 0,elapsed 4.241s。

### 总结感悟
- Zenoh FIFO 的 `Ok(None)` 必须按 timeout tick 处理,不能偷懒映射成 channel closed。
- GUI side-effect 和慢脚本都属于普通 control action,它们需要独立于网络 request timeout 的 response deadline。
- 用慢 `@script` 做 transport 回归测试更稳定,真实 `@window-activate` 作为 live smoke 验证语义路径。
