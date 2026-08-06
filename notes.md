## [2026-08-05 22:58:24] [Session ID: omx-1785926019233-oohizd] 笔记: native screenshot capture tracing 诊断

### 现象

- macOS `capture_with_timeout` 会在 native SCK 或 xcap 调用长期不返回时,以 `TimedOut` 结束控制面等待,并用单 worker gate 避免无限创建线程。
- 现有路径没有结构化事件。daemon 日志无法区分 SCK 超时、正常 fallback、fallback 失败或 Screen Recording 权限拒绝。

### 静态证据

- `capture_primary_display_image` 与 `capture_all_display_images` 都先执行 Screen Recording preflight,再走 SCK,最后按非权限错误进入 xcap。
- `capture_with_timeout` 是唯一的 native deadline 与 in-flight gate 边界。它知道 backend、timeout 和 timeout 原因,但不知道请求是 primary 还是 all-display。
- `classify_capture_error` 已保证任一 backend 的 `PermissionDenied` 会覆盖为最终权限错误。权限不应继续 fallback。
- `Cargo.toml` 只有 `log` / `fern`,没有能输出 structured fields 的 tracing subscriber。

### 当前设计

- 新增 `tracing` 与 `tracing-subscriber`,让新增事件沿用 `RDOG_LOG_LEVEL` 和已有 stderr/hidden-file 目标,不迁移既有 `log` 调用。
- 共享 SCK -> xcap policy 负责 `fallback` 与终态事件。timeout helper 负责 timeout 原因,因为只有它能识别 worker deadline 和 in-flight gate。
- 权限拒绝是终态类别,用 `screenshot_capture_permission_denied` 代替泛化 `screenshot_capture_failed`,避免同一请求重复记录两个终态错误。

### 反证与边界

- 备选方案是在 `map_capture_error` 直接记录。该函数没有 capture kind 或 fallback 上下文,会把同一次失败拆成无关联的重复日志,因此不采用。
- 备选方案是只继续用 `log::warn!` 拼接文本。这无法按字段筛选 SCK timeout、fallback 与权限,不满足此次可观测性目标。

### 外部 API 证据

- `cargo info tracing@0.1.44`: 当前 crate 提供 application-level tracing。
- `cargo info tracing-subscriber@0.3.23`: `fmt` subscriber 可输出 events。源码 `fmt::writer::BoxMakeWriter` 支持运行时选择 stderr 或 file writer。
