## [2026-08-05 23:38:00] [Session ID: omx-1785926019233-oohizd] 任务名称: Worklog 续档

### 任务内容
- 旧 `WORKLOG.md` 达到 1001 行,已完成持续学习后续档。

### 完成过程
- 旧记录归档为 `archive/default_history/WORKLOG_2026-08-05_233200_app_menu_window_screenshot.md`。
- 归档范围、学习结论和验证见 `archive/manifests/ARCHIVE_MANIFEST__2026-08-05_worklog_rollover_app_menu_window_screenshot.md`。

### 总结感悟
- App 菜单 capture selector 与 screenshot backend gate 已同步到长期经验;本轮没有新增待办或重大风险。

## [2026-08-05 23:15:33] [Session ID: omx-1785926019233-oohizd] 任务名称: Native capture tracing 诊断

### 任务内容
- 为 macOS SCK / xcap screenshot 链路新增结构化 tracing event,区分 timeout、fallback、权限拒绝和非权限终态失败。
- 保持已有 timeout、单 worker gate、SCK -> xcap fallback 与 control error code 行为不变。

### 完成过程
- 新增 `tracing 0.1.44` 和关闭 `tracing-log` feature 的 `tracing-subscriber 0.3.23`;它与既有 `fern` 并行使用同一 `RDOG_LOG_LEVEL` 和 stderr/hidden-file 目标。
- 在 timeout helper 记录 `screenshot_capture_timeout`;在共享 policy 记录 `screenshot_capture_fallback`、`screenshot_capture_permission_denied` 或 `screenshot_capture_failed`。
- 新增事件捕获测试,覆盖 SCK timeout 后 xcap 成功、两个 backend 都失败、权限拒绝不 fallback。
- 已同步 `specs/zenoh-screenshot-control-plan.md` 和 canonical `rdog-control` skill,并完成 `notes.md` 阈值续档。

### 验证
- `cargo fmt -- --check`: 通过。
- `cargo nextest run --package rustdog --bin rdog screenshot::tests`: 30 passed。
- `cargo nextest run --package rustdog --bin rdog`: 683 passed,1 skipped。
- `cargo build --package rustdog --bin rdog`: 成功;17 条现有 warning 位于本任务未触碰模块。
- `RDOG_LOG_LEVEL=info target/debug/rdog --version`: 输出 `rustdog 3.0.0`,确认 logger 与 tracing subscriber 可共同初始化。
- `git diff --check`: 通过。

### 总结感悟
- 对无法取消的 native API,控制面 timeout 只说明等待结束,不代表底层 worker 已停止。日志必须分别记录 deadline、fallback 和单一终态,现场才能避免错误重试。
