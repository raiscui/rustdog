## [2026-05-27 14:22:30] [Session ID: codex-native-20260527-133652] 错误修复: @screenshot 可疑旧帧不再静默成功

### 问题
- `@screenshot` 曾保存出不同文件名、不同 `screenshot_id` 的新文件,但 JPEG 内容 MD5 完全相同。
- 用户看到的截图不是当前左侧显示器内容,导致 GUI 点击任务使用了错误视觉证据。

### 原因
- 动态验证显示系统 `screencapture` 能抓到当前画面,但 long-running `rdog daemon` 的 `sck-rs` 截图链路返回了旧帧。
- 重启 daemon 后同一 `@screenshot` 路径恢复新鲜画面,说明保存层不是主要问题。
- 在代码层面,旧实现没有对“新文件名但旧像素”做 freshness/stale 检查。

### 修复
- composite screenshot 捕获后先计算 display fingerprint。
- 如果连续两次 fingerprint 完全相同,立即返回 `SCREENSHOT_STALE_FRAME` 结构化错误。
- stale 错误在 AX capture 和 `@savefile` 输出之前发生,避免下游继续使用旧视觉证据。
- `@observe include_screenshot` 复用同一 guard。

### 验证
- `cargo test --package rustdog --bin rdog -- screenshot::tests --quiet`: 19 passed。
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_forward_structured_other_error_json --exact --quiet`: 1 passed。
- `cargo test --package rustdog --bin rdog --no-run --quiet`: 通过。
- `cargo test --package rustdog --test control_lanes --no-run --quiet`: 通过。
- `cargo test --package rustdog --test zenoh_router_client --no-run --quiet`: 通过。
- `git diff --check`: 通过。
- live smoke: 新 daemon 下 `@ping#1` 返回 `pong`; 当前连续真实 `@screenshot` 没误报 stale,因为屏幕像素确实在变化。

### 后续注意
- 当前 guard 是 suspicious hard-stop,策略是 `reject-consecutive-identical-composite-fingerprint`。
- 如果未来发现静态屏幕场景误报,应在保留 hard-stop 语义的前提下增加更强证据,例如捕获时间戳、后端 frame id、主动刷新探针或多 backend 交叉检查。

## [2026-06-01 15:01:15] [Session ID: 019e6d92-4146-7361-aae9-3e05a41b8c52] 错误修复: 首页点击不能只看 click ok

### 问题
- 初始坐标点击 `@click#10:{x:74,y:284,...}` 和重试 `@click#13:{x:48,y:255,...}` 都返回 `status:"ok"`。
- 但点击后截图显示打开了笔记详情层,不是“首页点击后瀑布流刷新”。

### 原因
- 当页面处于详情层或浏览器窗口状态复杂时,坐标 fallback 容易命中可见内容层,不能用动作响应判断业务成功。
- 当前 `@web-find target:{browser:"active"}` 在多 Chrome 窗口下会 `BROWSER_WINDOW_AMBIGUOUS`,导致语义路径没有直接落地。

### 修复
- 先用 `@key#16:"Escape"` 关闭详情层,恢复到瀑布流。
- 使用 fresh before 截图 `screenshot-1780297129609-virtual-desktop.jpg` 作为视觉基线。
- 再执行 `@click#19:{x:48,y:255,...}`。
- 点击后重新截图 `screenshot-1780297167647-virtual-desktop.jpg`。
- 对 feed 区域裁剪图做 `imgdiff`,以瀑布流内容变化作为成功证据。

### 验证
- `/tmp/xhs-feed-before.jpg` 与 `/tmp/xhs-feed-after.jpg` 的 MD5 不同。
- `imgdiff --threshold 0.1 --diff-image` 报 `Images are different`,不同像素 `304233`。
- 人眼复核 before/after feed 卡片明显不是同一批内容。

### 后续注意
- 真实 GUI e2e 必须区分动作成功和业务成功。
- 点击“首页”这类任务以后默认要保留 before/after screenshot diff 证据。
- 产品化应补 window-scoped `@web-find` / `@gui-probe`,避免多浏览器窗口时退回坐标猜测。
