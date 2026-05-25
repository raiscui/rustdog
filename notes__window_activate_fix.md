## [2026-05-25 09:58:43] [Session ID: omx-1779670884813-rnokx6] 笔记: `@window-activate` 失败复现证据

## 来源

### 来源1: live `rdog control mac.lab`

- 临时 daemon: `./target/debug/rdog daemon --transport zenoh --name mac.lab --namespace lab`。
- `@window-find#1:{"app_contains":"Chrome","limit":5,"include_state":true,"include_recipes":true}` 成功返回:
  - `kind:"window-find"`
  - `status:"complete"`
  - `window_id:"pid:8231/window:0"`
  - app `Google Chrome`
  - `state.interactable:true`
- `@window-activate#2:{"window_id":"pid:8231/window:0"}` 返回:
  - process exit code `1`
  - stdout error: `Zenoh session bridge subscriber 在收到结果前关闭`

## 综合发现

### 现象

- 同一 daemon、同一 target 下,`@window-find` 的 session-channel result 能正常回到 control 端。
- `@window-activate` 没有返回 `window-action` step report,而是 control 端在收到结果前认为 subscriber closed。

### 当前假设

- 主假设: `@window-activate` 进入了 daemon 侧执行,但执行路径在发送 final response 前返回/崩溃/关闭了 session bridge。
- 备选解释: final response 已发送,但 client 侧路由函数无法识别该 frame,把 session 结束解释成没有结果。

### 下一步

- 对比 `@window-find` 和 `@window-activate` 的 `ControlExecutionOutcome` 构造与 Zenoh `to-control` dispatch。

## [2026-05-25 10:13:22] [Session ID: omx-1779670884813-rnokx6] 笔记: 根因证据收敛

## 来源

### 来源1: `@window-activate` 计时复现

- 命令: `@window-activate#3:{"window_id":"pid:8231/window:0"}`
- 耗时: `3.047s`
- 结果: exit code `1`, stdout 为 `Zenoh session bridge subscriber 在收到结果前关闭`。

### 来源2: 慢 `@script` 最小可证伪实验

- 命令: `@script#9:"sleep 4; printf SLOW_READY"`
- 耗时: `3.037s`
- 结果: 同样 exit code `1`, stdout 为 `Zenoh session bridge subscriber 在收到结果前关闭`。

### 来源3: 静态代码路径

- `src/zenoh_control/client_pty.rs::execute_remote_request()` 中,`subscriber.recv_timeout(timeout)` 的 `Ok(None)` 被映射为 `io::ErrorKind::UnexpectedEof` 和 “subscriber 在收到结果前关闭”。
- 项目历史经验已经确认 Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 是 timeout,不是 subscriber closed。

## 综合发现

### 已验证结论

- 失败不是 `window-activate` 独有的平台 backend 崩溃。
- 任何超过默认 3000ms 的 session-channel 普通 line-control 响应,都会被 client 误报为 subscriber closed。
- `@window-activate` 触发该问题,是因为其 macOS JXA / AX side-effect 路径可能超过 3 秒。

### 修复方向

- `execute_remote_request()` 应把 `Ok(None)` 当成一次 idle/timeout tick,而不是 subscriber closed。
- 普通 line-control 请求应允许等待更长的 response 窗口,直到收到 final `@response` 或达到明确的 response deadline。
- 自动化回归测试用慢 `@script` 复现这个 transport bug,live smoke 再覆盖真实 `@window-activate`。

## [2026-05-25 10:25:20] [Session ID: omx-1779670884813-rnokx6] 笔记: 修复后验证证据

## 来源

### 来源1: focused nextest

- 命令: `cargo nextest run --package rustdog --test zenoh_router_client control_should_wait_for_slow_session_channel_response`
- 结果: 1 test run, 1 passed。
- 关键证据: 新增测试中的 `@script#21:"sleep 4; printf SLOW_READY"` 经 Zenoh session channel 返回 `@response {"id":21,"value":"SLOW_READY"}`。
- 意义: 证明普通 line-control response 超过旧 3 秒等待 tick 后,client 不再把 `recv_timeout()` 的 `Ok(None)` 误判成 subscriber closed。

### 来源2: 编译和格式化

- `cargo fmt -- --check`: 通过。
- `cargo check --quiet`: 通过。
- `cargo test --package rustdog --test control_window_e2e --no-run`: 通过。
- `git diff --check`: 通过。

### 来源3: live `@window-activate` over Zenoh session bridge

- 构建: `cargo build --quiet`。
- 临时 daemon: `./target/debug/rdog daemon --transport zenoh --name mac.windowfix.lab --namespace lab`。
- 查找窗口: `@window-find#1:{app_contains:"Chrome",limit:3,include_state:true,include_recipes:true}`。
- 找到窗口: `pid:8231/window:0`, `state.occluded:true`, `state.interactable:false`。
- 激活命令: `@window-activate#2:{window_id:"pid:8231/window:0"}`。
- 返回结果: exit status 0, elapsed 4.241s。
- 关键响应: `@response {"id":2,"value":{"kind":"window-action","schema":"rdog.window.v1","platform":"macos","action":"activate","status":"ok","window_id":"pid:8231/window:0",...}}`。
- 意义: 真实 `@window-activate` 超过旧 3 秒 tick 仍能走 Zenoh session bridge 返回 final `@response`。

## 综合发现

### 已验证结论

- 根因是 client-side Zenoh session bridge 把 `Subscriber::recv_timeout()` 的 `Ok(None)` 错当成 subscriber closed。
- 修复后,`Ok(None)` 被作为 idle tick 继续等待,直到收到 final `@response` 或达到明确的 line-control response deadline。
- 旧的 3 秒 request timeout 仍保留给 session open / target resolve 等网络控制面操作,没有被扩大成全局超时。
