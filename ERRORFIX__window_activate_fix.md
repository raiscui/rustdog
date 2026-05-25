# 错误修复记录: `@window-activate` over Zenoh session bridge 返回路径

## [2026-05-25 10:25:20] [Session ID: omx-1779670884813-rnokx6] 修复: Zenoh session bridge 慢响应被误判为 subscriber closed

### 现象
- live `@window-activate#3:{"window_id":"pid:8231/window:0"}` 在约 3.047s 后失败。
- 慢 `@script#9:"sleep 4; printf SLOW_READY"` 也在约 3.037s 后失败。
- control 端输出为 `Zenoh session bridge subscriber 在收到结果前关闭`。

### 已验证原因
- `src/zenoh_control/client_pty.rs::execute_remote_request()` 将 `subscriber.recv_timeout(timeout)` 的 `Ok(None)` 映射为 `UnexpectedEof`。
- Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 表示本轮等待超时 tick,不是 subscriber closed。
- 因此任何普通 line-control 请求只要 final response 超过默认 3000ms,就可能被误判为返回通道关闭。

### 修复
- 在 `src/zenoh_control/client_pty.rs` 新增 `LINE_CONTROL_RESPONSE_TIMEOUT = 60s`。
- `execute_remote_request()` 现在为普通 line-control 请求设置明确 response deadline。
- `Ok(None)` 只作为 idle tick 继续等待,不再作为 `UnexpectedEof`。
- session open / target resolve 仍继续使用传入的短 request timeout,避免把网络控制面等待无限放大。

### 回归测试
- 在 `tests/zenoh_router_client.rs` 增加 `control_should_wait_for_slow_session_channel_response`。
- 测试通过 Zenoh session bridge 执行 `@script#21:"sleep 4; printf SLOW_READY"`,断言最终 `@response` 返回到 control stdout。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo nextest run --package rustdog --test zenoh_router_client control_should_wait_for_slow_session_channel_response`: 通过,1 passed。
- `cargo check --quiet`: 通过。
- `cargo test --package rustdog --test control_window_e2e --no-run`: 通过。
- `git diff --check`: 通过。
- live smoke: `@window-activate#2:{window_id:"pid:8231/window:0"}` over `mac.windowfix.lab` 返回 `status:"ok"`,exit status 0,elapsed 4.241s。
