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

## [2026-06-20 18:40:00] [Session ID: omx-1781934324141-q2nzhz] 笔记: continuous-learning 六文件摘要与归档决策

## 来源

### 来源1: 根目录六文件与支线六文件清单

- 命令: `rg --files -g 'task_plan*.md' -g 'notes*.md' -g 'WORKLOG*.md' -g 'LATER_PLANS*.md' -g 'ERRORFIX*.md' -g 'EPIPHANY_LOG*.md' -g '!archive/**'`
- 发现: 默认六文件仍在根目录; 另有 `agent_desktop_review`、`ax_plan`、`bootstrap`、`computer_use_density`、`mouse_e2e`、`mouse_ralph`、`mouse_ralplan`、`non_mouse_control_research`、`non_mouse_semantic_control`、`observation_refmap_*`、`rdog_*`、`window_*`、`xhs_*` 等旧支线文件。

## 综合发现

### 默认上下文集

- 当前默认组仍活跃,最新当前会话任务是 scoped commit `rdog-control` skill version + `$continuous-learning`。
- `WORKLOG.md` 已在 2026-06-20 续档,当前只有 50 行左右,不需要再次续档。
- `LATER_PLANS.md` 里 2026-06-20 12:10 已登记"完整整理根目录旧支线六文件",本轮 continuous-learning 正在执行这件事。

### 支线组活跃度判定

- 所有带 `__suffix` 的支线文件最后标准时间戳都早于 2026-06-20,或只有 2026-06-18 mtime 但没有当天活跃证据。
- 因此本轮把这些支线组判定为"未轮转旧支线文件",按 skill 规则在摘要后归档到 `archive/branch_contexts/<suffix>/`。
- 默认六文件不归档; 当前会话刚追加的 `task_plan.md` / `notes.md` 继续作为活跃入口。

### 可复用点候选

1. mixed worktree 里同一个文件已有非本轮改动时,不要 `git add file`。可以从 HEAD 内容生成临时版本,只把本轮目标行写入 index,再提交 scoped commit。
2. `rdog-control` skill 这类 agent-facing 文档需要显式版本字段,便于后续跨 agent / MCP / human operator 判断 skill 兼容边界。
3. 根目录旧支线六文件太多会污染每次检索。执行 `$continuous-learning` 时应按后缀整体归档,不是逐个零散删除。

### 沉淀去向

- `EXPERIENCE.md`: 记录 scoped index-only staging 与旧支线归档经验。
- `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_branch_context_cleanup.md`: 记录本批归档范围和摘要。
- `AGENTS.md`: 增加新 archive manifest 索引。
- `LATER_PLANS.md`: 追加完成记录,说明 2026-06-20 12:10 的根目录旧支线整理已执行。

### 是否提取新 skill

- 否。scoped mixed-worktree commit 已有用户记忆与相关 skill 线索,本轮只是项目内一次具体应用。
- 更适合沉淀到项目 `EXPERIENCE.md`,避免重复创建新 `self-learning.*` skill。
