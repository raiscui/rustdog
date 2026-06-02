## [2026-05-25 09:14:22] [Session ID: omx-1779670884813-rnokx6] 笔记: notes.md 续档后的当前入口

## 来源

### 来源1: continuous-learning 触发

- 触发条件: 默认 `notes.md` 达到 1009 行,超过六文件 1000 行阈值。
- 已归档旧文件: `archive/default_history/notes_2026-05-25_0910_rdog_control_live_click.md`。
- 已创建 manifest: `archive/manifests/ARCHIVE_MANIFEST__2026-05-25_rdog_control_notes.md`。

## 综合发现

### 当前任务摘要

- 本轮通过 `./target/debug/rdog daemon --transport zenoh --name mac.lab --namespace lab` 临时启动本机 daemon。
- `@ping#1` 成功返回 `pong`。
- `@capabilities#2` 返回 `rdog.capabilities.v1`, screenshot / accessibility / window_control / mouse_input 均为 `available`。
- `@observe#3` 找到 Chrome 小红书窗口 `pid:8231/window:0`,但网页内容没有暴露可直接 AXPress 的“首页”按钮。
- `@screenshot#5` 返回 composite JPEG 和 manifest,manifest 说明 `image_to_os` 为 `os_x=image_x+virtual_bounds.x; os_y=image_y+virtual_bounds.y`。
- 根据截图裁剪定位“首页”按钮中心,用 `@click#6:{x:78,y:219,...}` 完成点击,响应 `status:"ok"` 且 `target_resolution.source:"coordinate_fallback"`。
- `@screenshot#7` 生成点击后验证截图,左侧“首页”仍可见且高亮。

### 可复用点

- Chrome 网页内容 AX 不足时,不要硬编 AXPress。应读取 screenshot manifest,明确坐标空间后再用 coordinate fallback。
- request id 必须是无符号整数,例如 `@ping#1`,不要用 `@ping#ping`。
- 如果 `rdog control mac.lab` 未发现 router,先确认 daemon 是否运行。临时 daemon 完成后应清理。
