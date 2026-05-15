## [2026-05-14 15:12:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: AX 能力规划代码事实

### 已确认代码入口
- `src/screenshot.rs:24` 的 `execute_screenshot_request` 是 `@screenshot` 执行入口.
- `src/screenshot.rs:99` 的 `build_composite_screenshot_outcome_with_id` 生成 virtual desktop JPEG 和 manifest JSON 两个 `@savefile`.
- `src/screenshot.rs:368` 附近构造 `ScreenshotManifest`,当前 schema 是 `rdog.screenshot.v1`,坐标空间是 `os-logical`.
- `src/control_protocol.rs:22` 的 `ControlCommand` 是显式控制命令枚举.
- `src/control_protocol.rs:157` 的 `parse_control_line` 负责 `@...` line-control 分发.
- `src/control_mouse.rs:16` 和 `src/control_mouse.rs:584` 说明鼠标已使用独立 request/plan/performer 结构,AX 控制也应采用同类分层.

### 规划原则
- 截图 manifest 增加 AX 信息时,不要改变 JPEG 与显示器坐标契约.
- AX 元素引用不能只用按钮名字 `b`,必须有稳定 locator.建议 `ax_path` + `role/title` + 可选 `pid/window_id`.
- AXPress,AXSetValue,AXFocus 等命令必须清楚表达权限错误和平台不支持.

## [2026-05-14 15:20:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: 默认 task_plan 续档

### 六文件摘要
- 默认 `task_plan.md` 超过 1000 行,已复制进 `archive/default_history/2026-05-14_ax_plan_context_rollover/` 并创建新的默认入口.
- 本轮 AX plan 使用 `__ax_plan` 支线上下文,活跃文件保留在根目录.
- 当前无日期后缀但当天仍活跃的支线包括 `__ax_plan`, `__mouse_e2e`, `__mouse_ralph`, `__mouse_ralplan`;本轮不归档这些支线.

### 可复用点
- AX manifest/AX control 的核心经验已进入 `.omx/plans/rdog-ax-screenshot-manifest-control-plan.md`,暂不写入 `EXPERIENCE.md`,因为这还是未实施计划而非已验证经验.
