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
## [2026-06-24 19:37:30] [Session ID: native-hook-20260624-193730] 笔记: 多显示器 display scope 设计分析

## 来源

### 来源1: `specs/rdog-multi-display-screenshot-coordinate-plan.md`
- 现有结论: `@screenshot` 默认 `display:"all",layout:"composite",coordinate_space:"os-logical"`。
- 关键约束: manifest 是截图坐标和后续鼠标坐标之间的单一真相源。
- 已有入口: `display:"primary",layout:"single"` 是显式主屏兼容入口。
- 缺口: 目前规格没有定义 `display_id` / `display_ref` / `display_filter` 进入 `@observe` 或 AX/window 查询链路。

### 来源2: `specs/rdog-mouse-control-coordinate-plan.md`
- 现有结论: 鼠标控制必须复用 screenshot manifest 的 `os-logical` 坐标语义。
- 关键约束: 坐标必须命中某个 `displays[].os_rect`,不能命中 gap。
- 缺口: 鼠标命令有坐标校验,但没有“这次 action 必须落在某个 display scope 内”的协议字段。

### 来源3: `specs/rdog-observation-scoped-refmap-plan.md`
- 现有结论: `@observe` 是统一观察入口,ref 只对当前 observation 有效,selector 负责跨 observation 恢复。
- 关键约束: mouse 已经可以 ref 化,坐标只是 fallback。
- 缺口: observation header 有 `scope`,但当前规格没有把 display scope 明确建模为 scope 的一部分。

### 来源4: `specs/rdog-ax-screenshot-manifest-control-plan.md`
- 现有结论: AX 结构和鼠标控制都复用 screenshot manifest 的 `os-logical` 坐标语义。
- 缺口: AX tree/find/get/press 没有 display filter,因此多屏下 agent 只能靠窗口标题、进程名或坐标间接过滤。

## 综合发现

### 当前候选结论
- display 指定应成为 `ObservationScope` 的一部分,由 `@observe` 首先收窄可见窗口、AX 元素、WebArea 和截图。
- `@screenshot display:"primary"` 只能解决视觉证据范围,不能解决 agent 操作目标范围。
- mouse 坐标命令应该支持可选 `display` guard,用于阻止坐标落到其他显示器或 gap。
- `@window-find` / `@ax-find` / `@web-find` 应支持从 observation 继承 display scope,也应允许显式 `display` filter。

### 推荐协议方向
- 增加 `@displays` 或把 displays summary 放进 `@observe/@bootstrap` response。
- display selector 最少支持: `primary`, `all`, `index`, `id`, `name_contains`, `contains_point`, `window_ref`。
- `@observe` 增加 `scope:{display:{...}}`。
- `@click` / `@drag` / `@wheel` 增加 `guard:{display_ref:"@d1"}` 或从 target ref 自动继承 display。

## [2026-06-26 13:01:03] [Session ID: 019f023a-e4c3-7f73-9d7b-9393ef3d38ff] 笔记: rdog UI script 设计讨论

## 来源

### 来源1: iced_emg `for_each_state_push_reverse_push_reverse_push3.json`

- 路径: `/Users/cuiluming/local_doc/l_dev/my/rust/iced_emg/ui_script/for_each_state_push_reverse_push_reverse_push3.json`
- 要点:
  - 脚本是 JSON array,每个 step 是单 key object。
  - 样例只使用 `SleepMs` / `Screenshot` / `Move` / `Click` / `Exit`。
  - `Move` / `Click` 使用绝对逻辑坐标,适合进程内单窗口回放,不等同于 rdog 的桌面 `os-logical` 坐标。

### 来源2: iced_emg `docs/ui_script_command.md` 与实现

- 路径: `/Users/cuiluming/local_doc/l_dev/my/rust/iced_emg/docs/ui_script_command.md`
- 路径: `/Users/cuiluming/local_doc/l_dev/my/rust/iced_emg/emg_bind/src/ui_script.rs`
- 要点:
  - CLI 优先 `--ui-script`,兼容 `--script`,再回退 `UI_SCRIPT_FILE`。
  - DSL 支持 `WindowSize`、`SleepMs/DelayMs`、`Move/CursorMove`、`Click`、`MouseDown/MouseUp`、`KeyDown/KeyUp/KeyPress`、`Text/TextInput`、`Screenshot`、`Barrier`、`Exit`。
  - `WindowSize` 负责稳定 logical 坐标系;`Screenshot` 负责产物证据;`Barrier` 负责等待下一帧,这些都是稳定性工具,不是单纯动作。
  - 实现是进程内 winit event loop 注入,用 `OnceLock` 保证脚本只设置一次。

### 来源3: rdog control 协议与现有控制面

- 路径: `specs/control-line-protocol.md`
- 路径: `.codex/skills/rdog-control/references/protocol.md`
- 路径: `src/control_protocol.rs`
- 路径: `src/control_core.rs`
- 路径: `src/control_actions.rs`
- 要点:
  - `@script` / `@cmd` 已经是 shell 命令执行语义,新的 UI 脚本不应复用 `@script` 命名。
  - 现有 `ControlCommand` 已覆盖 `@observe`、`@screenshot`、`@click`、`@drag`、`@wheel`、`@key`、`@paste`、AX/window/web/control frames。
  - `@cmd` 在 parser 中映射到 `ControlCommand::Script`,说明它是 shell 显式入口,不是 UI flow。
  - rdog 已经有 `ControlExecutionOutcome` 和 `@savefile` frame,脚本执行报告应复用 frame / response 模型。

### 来源4: rdog observation / density 设计

- 路径: `specs/rdog-observation-scoped-refmap-plan.md`
- 路径: `specs/rdog-computer-use-density-plan.md`
- 要点:
  - rdog GUI 自动化优先 semantic action,鼠标坐标只是 fallback。
  - `@web-act` / `@web-find` 已经走 `AXWebArea` 和 bounded verification,不应被 UI script 退化成无脑坐标点击。
  - display scope / guard、observation ref、selector refind 应进入脚本执行模型,否则脚本会绕开 rdog 最近刚收敛的安全边界。

## 综合发现

### 可复用的 iced_emg 语法

- 保留 JSON array + PascalCase step 形态,方便迁移已有脚本。
- `SleepMs`、`Move`、`Click`、`Screenshot`、`Exit` 可以保留名字。
- `MouseDown/MouseUp`、`KeyPress`、`Text` 可以作为 rdog v1/v2 的自然扩展。

### 必须适配 rdog 的地方

- `Exit` 在 rdog 中默认只能表示"结束脚本执行",不能默认关闭远端应用或 daemon。
- `WindowSize` 不能默认照搬为窗口 resize。rdog 当前没有通用 window resize 协议,第一版应拒绝或仅作为 metadata/precondition。
- `Move` / `Click` 坐标必须明确是 `os-logical`,并允许挂 `guard:{display:{...}}`。
- 需要新增脚本级 `Target` / `Scope` / `Observe` / `Expect` 这类 rdog 专属步骤,否则脚本无法表达 target window、display scope、权限和验证。
- 第一版最好先做 CLI-side runner,读取本地 JSON 并编译成现有 line-control frames。daemon-side `@ui-flow` 可作为后续优化,但必须复用同一套 IR,避免长出第二控制协议。

### 当前推荐方向

- 方案 A: `rdog ui-script run [TARGET] <file.json>` 或 `rdog control TARGET --ui-script <file.json>`。
- 解析本地 JSON,执行同一条 `rdog control` session。
- step 报告写入 `rdog_script_runs/<timestamp>/trace.jsonl`,截图和 manifest 按 `Screenshot.label` 归档。
- 坐标动作默认允许但必须标记 `target_resolution.source:"coordinate_fallback"`;推荐在脚本开头用 `Observe` / `Scope` / `Target` 收窄环境。
- 后续再考虑 daemon-side `@ui-flow#id:{...}`,但命名不要碰 `@script`。

## [2026-06-26 16:31:44] [Session ID: codex-20260626-ui-script-fixtures] 笔记: UI script fixture tests 与 WindowSize resize 规划

## 来源

### 来源1: `src/ui_script.rs`

- 新增 UI script dry-run 测试内核,只在 `#[cfg(test)]` 下编译。
- 解析形态固定为 JSON array + PascalCase single-key object。
- 当前覆盖 `Dialect`、`Target`、`Policy`、`Scope`、`SleepMs`、`DelayMs`、`Observe`、`Screenshot`、`Move`、`Click`、mouse/key/text/action/barrier/expect/window size/control line/exit。
- dry-run runner 输出 line-control 文本和 step summary,不连接 daemon。

### 来源2: `tests/fixtures/ui_script/`

- `iced_compatible_basic.json`: 覆盖 iced_emg 风格的 `SleepMs` / `Screenshot` / `Move` / `Click` / `Exit`。
- `rdog_target_scope_observe_expect.json`: 覆盖 rdog 专属 `Target` / `Scope` / `Observe` / `Expect`,并验证 scope 注入 observe 与 mouse guard。
- `window_size_precondition.json`: 覆盖 `WindowSize mode:"precondition"`。
- negative fixtures 覆盖 multi-key step、缺少 coordinate_space、WindowSize 缺 mode。

### 来源3: `specs/rdog-window-control-plan.md`

- 已新增后续 `@window-resize` 协议草案。
- `@window-resize` 以 window target resolver 为入口,请求中显式描述 target、size、origin、activate、verify。
- macOS 后端建议走 AX,读取并设置 `kAXSizeAttribute` 与可选 `kAXPositionAttribute`,再复读 rect 做后验验证。

## 综合发现

### 已验证结论

- `rtk cargo test --package rustdog --bin rdog -- ui_script`: 7 passed。
- `cargo check --package rustdog --bin rdog --quiet`: 通过,无 warning 输出。
- `rtk cargo fmt -- --check`: 通过。
- `rtk git diff --check`: 通过。
- 两个 specs 中 6 个 Mermaid 块均通过 `beautiful-mermaid-rs --ascii`。

### 设计结论

- UI script 测试底座在正式 CLI 接入前应保持 test-only,否则二进制非测试编译会出现 dead_code warning。
- `WindowSize` 当前不能偷渡成真实窗口 resize。它只表示前置条件检查语义。
- 真正 resize 应先落 `@window-resize`,再让 UI script 的 `WindowSize mode:"resize"` 编译到这个 control frame。

## [2026-06-26 18:15:52] [Session ID: codex-20260626-window-resize-default-activate] 笔记: @window-resize 默认恢复/激活语义

## 来源

### 来源1: 用户决策

- `@window-resize` 默认 `activate:true`,但请求里不用显式写出来。
- resize 的核心价值是节省步骤。既然要 resize,通常接下来就是在该窗口工作,因此默认恢复/激活窗口更符合 agent 工作流。
- `@window-activate` 在 skill 等地方弱化为备用功能。

### 来源2: 当前规格与 skill

- `specs/rdog-window-control-plan.md` 原先把 resize 前恢复窗口拆成显式 `@window-activate`。
- `rdog-control` skill 和 references 原先把 `@window-activate` 放在 GUI/window 主路径里。

## 综合发现

### 已采用结论

- `@window-resize` 是固定窗口尺寸场景的高密度动作。
- `@window-resize` 请求不出现 `activate:true` 字段,默认执行恢复/激活目标窗口的 recipe。
- `steps[]` 必须记录 unhide、unminimize、activate、raise、switch Space 等实际执行步骤。
- `@window-activate` 只用于"恢复/聚焦但不 resize"或 resize recovery limited 后的手动备用。
- resize target 统一走 `target:{...}` 入口,不再使用顶层 `window_id`。

## [2026-06-27 20:10:53] [Session ID: codex-20260627-window-resize-edge-decisions] 笔记: @window-resize 实现前边界

## 来源

### 来源1: 用户确认

- 用户确认按此前建议补齐 `@window-resize` 的 5 个边界问题。

### 来源2: `specs/rdog-window-control-plan.md`

- resize 小节已经有默认恢复/激活、统一 `target:{...}` 和基础错误边界。
- 本轮补充 verify 容差、clamped 状态、not settable、display guard 和 query 多命中严格失败。

## 综合发现

### 已采用结论

- `verify:true` 等价于 `{tolerance_px:2}`。
- 尺寸误差在 `2` logical px 内时返回 `verify.status:"ok_with_delta"`。
- app / system clamp 返回 `WINDOW_RESIZE_CLAMPED`,并在 report 里带 `requested_size`、`after_rect`、`delta` 和 `clamp_reason`。
- AX / 平台后端可读但不可写返回 `WINDOW_RESIZE_NOT_SETTABLE`。
- 默认允许 resize 后跨 display。只有显式 `guard:{display:{...}}` 时,才检查 `after_rect` 是否满足 display guard。
- `target:{query:{...}}` 必须唯一命中。多命中返回 `WINDOW_AMBIGUOUS`,不自动选第一个。

## [2026-06-28 00:45:00] [Session ID: codex-20260628-goal-window-resize] 笔记: @window-resize 实现落地

## 来源

### 来源1: `src/control_window.rs`

- 新增 `WindowResizeRequest`、`WindowResizeSize`、`WindowResizeOrigin`、`WindowResizeVerify`。
- `parse_window_resize_payload` 只接受 canonical `target:{...}`,显式拒绝顶层 `window_id`。
- `target:{query:{...}}` 已进入 `WindowCommandTarget` parser,多命中后端仍按严格唯一目标处理。
- `verify:false` 暂不支持,因为 resize 必须执行后验 rect 验证。
- 新增纯 helper `evaluate_window_resize_verification`,覆盖 `ok`、`ok_with_delta`、`WINDOW_RESIZE_CLAMPED`、`WINDOW_RESIZE_VERIFY_FAILED`。

### 来源2: `src/control_window/macos.rs`

- macOS backend 新增 `resize`。
- resize 默认复用窗口恢复 recipe: `unhide_app`、`unminimize_window`、`activate_app`、`raise_window`、必要时 `switch_to_window_space`。
- 恢复步骤失败或 limited 时,不继续伪造 resize 成功,返回 `WINDOW_RESIZE_RECOVERY_FAILED`。
- 写入前用 `AXUIElementIsAttributeSettable` 检查 `AXSize` 和可选 `AXPosition`。
- 用 `AXValueCreate` 创建 `CGSize` / `CGPoint`,再通过 `AXUIElementSetAttributeValue` 写 `AXSize` / `AXPosition`。
- 写入后重新读取 `AXPosition` / `AXSize`,生成 `before_rect`、`requested_rect`、`after_rect`、`delta`、`verify`。
- 显式 `guard:{display:{...}}` 会用当前 display summaries 检查 `after_rect` 是否仍与目标 display 相交。

### 来源3: `src/control_protocol.rs` / `src/control_actions.rs`

- 新增 `ControlCommand::WindowResize(WindowResizeRequest)`。
- `parse_control_line` 支持 `@window-resize`。
- 默认 executor 新增 `execute_window_resize`,返回 `window-action` JSON report。

## 综合发现

### 已验证结论

- `rtk cargo test --package rustdog --bin rdog -- control_window::tests`: 12 passed。
- `rtk cargo test --package rustdog --bin rdog -- control_protocol::tests`: 29 passed。
- `rtk cargo test --package rustdog --bin rdog -- shell::tests`: 14 passed。
- `rtk cargo test --package rustdog --bin rdog`: 405 passed。
- `rtk cargo test --package rustdog --bin rdog -- ui_script`: 7 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: 通过。
- `rtk cargo fmt -- --check`: 通过。
- `rtk git diff --check`: 通过。
- `beautiful-mermaid-rs --ascii`: 6 个 Mermaid 块通过。

### 当前边界

- 本轮没有跑 live `@window-resize`,因为它会真实恢复/激活并移动/缩放用户窗口,需要明确目标窗口才适合执行。
- UI script `WindowSize mode:"resize"` 仍未接入真实 runner。当前 dry-run parser 继续只接受 `mode:"precondition"`。
- `WINDOW_RESIZE_CLAMPED` 使用保守启发: 写入后尺寸朝请求方向移动,但停在容差外,才标记 clamped。完全不动或证据不足时返回 verify failed。

## [2026-06-28 13:12:00] [Session ID: codex-20260628-ui-script-window-size-resize] 笔记: WindowSize resize dry-run 编译接入

## 来源

### 来源1: `src/ui_script.rs`

- `WindowSizeStep` 已扩展 `target`、`origin`、`guard`、`box`、`verify`。
- `mode:"precondition"` 仍保持本地 dry-run effect,不发送 control line。
- `mode:"resize"` 会生成 `@window-resize` control line。
- `mode:"resize"` 必须显式提供 `target`,避免脚本依赖当前焦点或隐式窗口。
- 当前 `Scope` 会在 `WindowSize` 未显式写 `guard` 时注入为 `@window-resize.guard`。
- `verify:false` 仍被拒绝,因为底层 `@window-resize` 必须执行后验验证。

### 来源2: `tests/fixtures/ui_script/window_size_resize.json`

- 新 fixture 覆盖 `Scope`、`WindowSize mode:"resize"`、`target.query`、`origin`、`box` 和 `verify.tolerance_px`。
- 测试不只检查字符串,还会把 dry-run 生成的 control line 交给真实 `parse_control_line`,确认它解析为 `ControlCommand::WindowResize`。

## 综合发现

### 已验证结论

- `rtk cargo test --package rustdog --bin rdog -- ui_script`: 9 passed。

### 当前边界

- 正式 `rdog ui-script run` / `rdog control --ui-script` 仍未接入。
- 本轮只升级 dry-run compiler 和 fixture tests,不跑 live resize。

## [2026-06-28 13:30:07] [Session ID: codex-20260628-finder-window-resize-live] 笔记: Finder @window-resize live 验证

## 来源

### 来源1: installed live daemon 初测

- `rdog control @ping` 返回 `@response "pong"`,说明本机 daemon 可达。
- `@window-find#1:{app_contains:"Finder",limit:10,include_state:true,include_recipes:true}` 找到 2 个 Finder match。
- 选定目标窗口: `window_id:"pid:877/window:0"`,标题 `docs`,原始 rect `{x:271,y:247,width:920,height:436}`。
- 第二个 Finder match rect 为 `{x:0,y:-124,width:3390,height:1080}`,更像桌面/Space 级窗口,不作为 resize 目标。
- 初次执行 `@window-resize#2` 返回 `{"id":2,"code":64,"error":"不支持的控制指令类型: window-resize"}`。
- 已确认 installed daemon 来自 `/Users/cuiluming/.cargo/bin/rdog`,版本 `rustdog 3.0.0`,进程命令为 `rdog daemon -c ./rdog_macos.toml`。

### 来源2: 当前 workspace debug daemon 重跑

- `cargo build --package rustdog --bin rdog` 通过,生成 `target/debug/rdog`。
- 停止旧 daemon 后,用 `./target/debug/rdog daemon -c ./rdog_macos.toml` 启动当前源码 daemon。
- `./target/debug/rdog control @ping` 返回 `@response "pong"`。
- 重跑 `@window-find#3` 仍选中 Finder `docs` 窗口,rect 为 `{x:271,y:247,width:920,height:436}`。
- 执行:
  `./target/debug/rdog control '@window-resize#4:{target:{window_id:"pid:877/window:0"},size:{width:1000,height:700,unit:"os-logical",box:"outer"},origin:"keep",verify:true}'`
- resize report 返回 `status:"clamped"`、`error_code:"WINDOW_RESIZE_CLAMPED"`。
- report 中 `before_rect` 为 `{x:271,y:247,width:920,height:436}`。
- report 中 `requested_rect` 为 `{x:271,y:247,width:1000,height:700}`。
- report 中 `after_rect` 为 `{x:271,y:247,width:1000,height:652}`。
- report 中 `delta` 为 `{x:0,y:0,width:0,height:-48}`。
- 执行步骤为 `activate_app ok`、`raise_window ok`、`check_size_settable ok`、`set_size ok`,最终 `verify_rect` 因 `WINDOW_RESIZE_CLAMPED` 失败。

### 来源3: 独立后验验证

- 第一次后验验证误用了 `@window-find#5:{target:{window_id:"pid:877/window:0"},...}`,返回未知字段 `target`。这是验证命令 shape 错误,不是 resize 后端错误。
- 改用 `@window-find#6:{app_contains:"Finder",title:"docs",limit:5,include_state:true,include_recipes:true}` 后,读回 rect `{x:271,y:247,width:1000,height:652}`。
- 收尾时再次执行 `@window-find#8:{app_contains:"Finder",title:"docs",limit:5,include_state:true,include_recipes:true}`,仍读回 rect `{x:271,y:247,width:1000,height:652}`。
- 最终窗口状态为 `frontmost:true`、`interactable:true`、`minimized:false`、`current_space:true`。

## 综合发现

### 已验证结论

- `@window-resize` 在当前 workspace debug daemon 下可以真实执行 Finder 窗口 resize。
- Finder/macOS 没有接受完整 `1000x700` 外框请求,而是 clamp 到 `1000x652`。
- 这个结果符合当前协议语义: 写入动作成功,但后验 rect 与请求尺寸超出容差,所以返回 `WINDOW_RESIZE_CLAMPED`。
- 旧 installed daemon 不认识 `@window-resize`,后续 live 验证如果使用裸 `rdog control ...`,需要先确保启动的是当前二进制。

### 当前状态

- 当前源码 daemon 运行在 tmux session `rdog-debug-daemon`。
- 当前 liveness 已验证: `./target/debug/rdog control @ping` 返回 `@response "pong"`。

## [2026-06-28 13:48:00] [Session ID: codex-20260628-next-priority-analysis] 笔记: 当前最值得做的后续工作

## 来源

### 来源1: `LATER_PLANS.md`

- 最新真实未完成项集中在 UI script 正式 runner、trace/artifacts、installed daemon 更新、debug daemon 清理。
- 旧的 `@window-resize` parser / backend / smoke 待办已经被后续完成记录覆盖,不能再按旧 checklist 直接采用。
- `zenoh_router_client` flake 仍记录为等待自然复现,不适合作为当前主动推进的主线。
- 方向 B(直接 UDS 控制面)仍是独立性能路线,不是 UI script / window resize 当前链路的阻塞项。

### 来源2: `specs/rdog-ui-script-control-plan.md`

- 规格明确推荐 v1 是 CLI-side runner。
- 推荐入口是 `rdog ui-script run [TARGET] path/to/script.json` 和可选 `rdog control TARGET --ui-script path/to/script.json`。
- runner 应本机读取 JSON,解析 IR,打开真实 control session,逐步发送 line-control,并写 `trace.jsonl` 和 artifacts。
- 正式接入后,`WindowSize mode:"resize"` 应复用 dry-run 已经生成的 `@window-resize` control line。

### 来源3: `src/ui_script.rs` / `src/main.rs`

- `src/ui_script.rs` 目前仍明确说明"暂时不连接 daemon,也不暴露 CLI"。
- dry-run compiler 已经可以生成 `@window-resize` line,并有 fixture tests 验证真实 parser roundtrip。
- `src/main.rs` 已有 one-shot control 发送路径,后续 runner 应复用现有 transport / invocation 解析,不要新造第二套控制协议。

### 来源4: live Finder smoke

- `@window-resize` 在 workspace debug daemon 下已经能真实调整 Finder 窗口。
- installed `/Users/cuiluming/.cargo/bin/rdog` daemon 仍可能是旧二进制,裸 `rdog control ...` 会遇到 `window-resize` unsupported。

## 综合判断

### 推荐优先级

1. 先做一个很小的运行环境收口:更新 installed `rdog` 或至少明确停止 `rdog-debug-daemon` / 重启 release daemon。
2. 主线推进 `rdog ui-script run [TARGET] <file.json>` 的最小真实 runner。
3. 给 runner 加 trace/artifacts 和 live smoke,让 `WindowSize mode:"resize"` 的真实执行有证据链。
4. 整理 `LATER_PLANS.md` 的旧 unchecked 项,清掉已经完成但仍显示 `[ ]` 的噪音。
5. 再处理 `AGENTS.md` 里不存在的 self-learning skill 索引和 `.codex/skills/rdog-control/*.bak` 等工作区卫生。

### 暂不推荐主动推进

- `zenoh_router_client` flake: 现有结论是等自然复现后按 EPIPHANY 推荐顺序处理。
- 直接 UDS 控制面:有性能价值,但不是当前 UI script 可用性的阻塞项。
- daemon-side `@ui-flow`: 规格已明确 v1 先走 CLI-side runner,避免新造第二套协议。

## [2026-06-28 18:20:00] [Session ID: codex-20260628-installed-ui-runner] 笔记: installed daemon 收口与 UI script 最小 runner

## 来源

### 来源1: installed daemon 收口

- `cargo install --path . --bin rdog` 已替换 `/Users/cuiluming/.cargo/bin/rdog`。
- 已停止旧 tmux session `rdog-debug-daemon`。
- 已启动新 tmux session `rdog-installed-daemon`,命令为 `rdog daemon -c ./rdog_macos.toml`。
- `rdog control @ping` 返回 `@response "pong"`。
- `rdog control '@window-resize#901:{}'` 返回 `@window-resize 对象 payload 不能为空`,证明 installed daemon 已识别 `@window-resize`,不是旧版 unsupported。

### 来源2: `src/input.rs`

- 新增 `Command::UiScript` 和 `UiScriptCommand::Run`。
- `rdog ui-script run` 支持:
  - `--dry-run`
  - `--url`
  - `--transport`
  - `--namespace`
  - `--target-name`
  - `--entry-point`
  - 1 到 3 个 positional,最后一个始终是 script path,前面 0 到 2 个作为 control target / TCP host-port。

### 来源3: `src/main.rs`

- `ui_script` 模块从 test-only 进入生产编译。
- 新增 `split_ui_script_run_positionals`。
- 新增 `apply_ui_script_target`,支持脚本内 `Target.name` / `Target.namespace` 参与 control invocation。
- 新增 `run_ui_script`,复用 `parse_script_file` / `compile_dry_run`。
- 新增 `execute_ui_script_plan`,按 dry-run step 顺序执行:
  - control line step 进入 pending batch。
  - `SleepMs` / `DelayMs` 编译出的 `sleep_ms:*` 在本地 sleep。
  - `Exit` 会先 flush pending lines,再停止脚本。
  - `Expect` 和 `Barrier observe` 暂不假装实现,真实 runner 返回清晰错误。
- 新增 `send_control_lines_for_invocation`,让 UI runner 和 one-shot control 共用 TCP / WebSocket / Zenoh / ZenohLocal 发送入口。

### 来源4: fixtures / docs

- 新增 `tests/fixtures/ui_script/ping_control_line.json`,用于非破坏性 live smoke。
- 更新 `specs/rdog-ui-script-control-plan.md`,把状态改为最小 runner 已落地,trace/artifacts/Expect 仍待做。

## 综合发现

### 已验证结论

- `rtk cargo test --package rustdog --bin rdog -- ui_script`: 16 passed。
- `rtk cargo test --package rustdog --bin rdog -- ui_script_run`: 4 passed。
- `rtk cargo test --package rustdog --bin rdog -- ui_script_target`: 2 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: 通过。
- `cargo fmt -- --check`: 通过。
- `rtk cargo test --package rustdog --bin rdog`: 414 passed。
- `rtk git diff --check`: 通过。
- `rdog ui-script run --help`: 显示新入口和参数。
- `rdog ui-script run --dry-run tests/fixtures/ui_script/ping_control_line.json`: 输出 `@ping` control line。
- `rdog ui-script run tests/fixtures/ui_script/ping_control_line.json`: 通过 local-default unixpipe 返回 `@response "pong"`。

### 当前边界

- 最小 runner 尚未写 `trace.jsonl` 和 artifacts。
- `Expect` 真实验证尚未实现,当前会明确失败,避免伪造验证成功。
- `rdog control --ui-script` 尚未接入。
- `--compat iced-emg`、`--trace-dir`、`--allow-coordinate-fallback` 等规格中提到的 flags 尚未实现。

## [2026-06-28 18:45:00] [Session ID: codex-20260628-next-priority-after-runner] 笔记: 最小 runner 之后最值得做什么

## 综合判断

- 当前最重要的不是继续新增协议,而是先把已经完成的最小 runner 改动收口。`git diff --stat` 显示源码、规格和六文件仍有 8 个文件改动,另有 `.codex/skills/rdog-control/*.bak` 与 `test-prompts.json` 未跟踪。
- 功能主线下一步是 `trace.jsonl` / run directory / artifacts。原因很直接: runner 已经能发 control lines,但还没有把每一步的请求、响应、失败、产物写成可审计证据链。
- `Expect` 验证排在 trace 后面。因为没有 trace,`Expect` 的失败证据也无处稳定落盘。
- `rdog control --ui-script`、`--compat iced-emg` 和直接 UDS 控制面都不是当前阻塞项。
- `task_plan.md` 已经 934 行。下一次大任务如果继续追加很多记录,可能需要按规则续档并触发 continuous-learning。

## [2026-06-28 19:48:19] [Session ID: codex-20260628-goal-ui-script-runner-1234] 笔记: UI script trace/artifacts 与最小 Expect 收口

## 来源

### 来源1: `src/main.rs`

- `rdog ui-script run` 现在会创建 run directory。
- 默认目录是 `rdog_script_runs/<run_id>/`,也可以通过 `--trace-dir` 指定。
- runner 会写入 `trace.jsonl`、`summary.json`、`script.normalized.json` 和 `artifacts/`。
- control line 执行会收集真实 `ControlFrame`,把 `@response` 和 `@savefile` 记录进 trace。
- `Expect` 已支持 `response_status`、`response_contains`、`control_status`、`window_rect`、`screenshot_exists`。

### 来源2: `src/zenoh_control.rs` / `src/shell.rs`

- Zenoh 多 line 发送新增 `send_control_lines_collect_frames`,让调用方可以决定输出和 artifact 目录。
- 旧 `shell::run_line_control_lines` wrapper 已删除,避免 dead_code warning。
- one-shot `rdog control ... @line` 现在同样走 collect frames + print 的路径。

### 来源3: live smoke

- `rdog ui-script run --trace-dir /tmp/rdog-ui-script-installed-ping-expect-1782647263 tests/fixtures/ui_script/ping_expect_response.json` 通过。
- trace 中包含 `@response "pong"`、`response_contains` 和 `response_status` 两条 Expect 记录。
- `summary.json` 中 `status:"complete"` 且 `verification_passed:true`。

## 综合发现

- 本轮没有新增 daemon-side UI 协议,仍然复用现有 line-control transport。
- `.codex/skills/rdog-control/*.bak` 与 `test-prompts.json` 保持未跟踪,未纳入本轮交付。
- `LATER_PLANS.md` 已清掉 UI script/window-resize 旧 unchecked 噪音,只保留真实 future items。

## [2026-06-28 20:52:27] [Session ID: codex-20260628-plan-daemon-ui-flow] 笔记: daemon-side @ui-flow 规划结论

## 综合发现

- 当前最稳的 `@ui-flow` 方案是新增普通 `ControlCommand::UiFlow`,而不是新建第二套 daemon 协议。
- daemon-side v1 应复用 UI script parser/IR/compiler,但拒绝 `Target`、shell、PTY 和 nested `@ui-flow`。
- daemon 内部执行 control lines 时,内层 `ResponseLine` 应进入 flow state/trace,不能直接发给 client。
- 内层 `SaveFile` 应透传到外层 `ControlExecutionOutcome`,最终只发一条 outer `@response` summary。
- 计划已落地到 `.omx/plans/rdog-daemon-side-ui-flow-plan.md`。

## [2026-06-28 23:04:11] [Session ID: codex-20260628-plan-daemon-flow] 笔记: daemon-side @flow 规划结论

## 综合发现

- 用户确认需要的是 daemon 侧 full script flow,不是 GUI-only `@ui-flow`。
- `@flow` 应作为新的 full runtime 入口,支持 `Cmd`、`Script`、`ControlLine`、`SleepMs`、`Expect`、`SaveArtifact`、`Exit`。
- `@cmd/@script` 当前已经能解析到 `ControlCommand::Script`,并由 `SystemControlActionExecutor` 执行 shell。`@flow` 应复用这条能力,但补上 policy、cwd/env、timeout、capture 和 trace。
- 前一版 `.omx/plans/rdog-daemon-side-ui-flow-plan.md` 现在应视为过窄草案。新的主计划是 `.omx/plans/rdog-daemon-flow-plan.md`。
- `@flow` 必须明确 daemon 文件系统语义: cwd/env/path 都属于 daemon 所在机器,controller 本机文件需要先 upload/inline。
## [2026-06-29 11:00:33] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: Ultragoal G001 @flow parser/schema

## 来源

### 来源1: `.omx/plans/rdog-daemon-flow-plan.md`

- 要点:
  - `@flow` 是 daemon-side full script runtime,不是 `@ui-flow`。
  - v1 要支持 `Cmd`、`Script`、`ControlLine`、`SleepMs`、`Expect`、`SaveArtifact`、`Exit` 这些有限顺序 step。
  - shell step 必须显式 `policy.allow_shell:true`。
  - v1 不做 PTY、后台任务、变量、循环或条件。

### 来源2: `src/control_protocol.rs` 与 CodeGraph

- 要点:
  - `ControlCommand` 是协议 parser 的中心枚举。
  - 现有 `@cmd` / `@script` 解析成 `ControlCommand::Script`。
  - 复杂 request 通常放在独立模块解析,例如 `control_bootstrap`。

## 综合发现

- 本轮新增 `src/control_flow.rs`,只承载 `FlowRequest` / `FlowPolicy` / `FlowOptions` / `FlowStep` 的 schema 和 parser validation,没有引入执行副作用。
- `@flow` 使用严格 JSON object,不同于部分旧 control object parser 的宽松字段写法。
- `ControlLine` step 现在会拒绝 nested `@flow`、`@pty` 系列、以及 `@cmd` / `@script` shell policy 绕过。
- `SystemControlActionExecutor` 对 `ControlCommand::Flow` 暂时返回 `Unsupported`;后续 story 应由 `control_core` 直接接入 flow runtime。

## 验证

- `rtk cargo test --package rustdog --bin rdog control_protocol::tests::flow --quiet`: 6 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk git diff --check`: passed。
## [2026-06-29 11:10:44] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: Ultragoal G002 @flow shell lane runtime

## 来源

### 来源1: `src/control_actions.rs::build_shell_command`

- 要点:
  - 现有 `@cmd` / `@script` 的 shell 参数选择已经集中在 `build_shell_command`。
  - G002 复用这一路径,没有重新发明 shell 参数拼装。

### 来源2: `self-learning.rust-command-timeout-kill-process-group`

- 要点:
  - timeout 后只杀父进程可能留下子进程。
  - 更稳的策略是让命令进入独立进程组,timeout 时杀进程组。
  - 本仓库当前没有 `nix` / `libc` 主依赖,因此 G002 采用不新增依赖的 best-effort 方案: Unix 下 `process_group(0)` + 系统 `kill -TERM/-KILL -<pid>`,非 Unix 走 `child.kill()`。

## 综合发现

- 新增 `execute_flow_shell_lane`,支持有限顺序执行 `Cmd`、`Script`、`SleepMs`、`Expect`、`Exit`。
- `Cmd` / `Script` 支持 daemon-local `cwd`、`env`、per-step `timeout_ms`、`capture`、`max_output_bytes` 截断。
- `FlowRunReport` 返回 `schema`、`status`、`completed_steps`、`failed_step` 和 captures summary。
- `ControlCommand::Flow` 已经接到 `SystemControlActionExecutor`,能执行最小 shell lane flow。`ControlLine` 和 `SaveArtifact` 在 G002 阶段仍返回明确未接入,留给 G003。

## 验证

- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog explicit_request_should_execute_minimal_flow_shell_lane --quiet`: 1 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed,无 warning。
- `rtk git diff --check`: passed。
## [2026-06-29 11:18:36] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: Ultragoal G003 @flow ControlLine/artifact/trace lane

## 来源

### 来源1: `src/control_frames.rs`

- 要点:
  - `ControlExecutionOutcome` 已经能承载多 frame。
  - `ControlFrame::SaveFile` 的 wire 形态是 `@savefile {...}`。
  - `to_multiline_wire_payload` 已经能把多 frame 按换行拼起来。

### 来源2: `src/control_core.rs`

- 要点:
  - `parse_and_execute_control_line` 是 transport 无关的 line-control 执行入口。
  - G003 通过 closure 把 inner `ControlLine` 交回 control_core 执行,避免 flow runtime 重新发明 parser/executor。

## 综合发现

- 新增 `execute_flow_request`,返回 `ControlExecutionOutcome`,能保留外层多 frame。
- `ControlLine` step 会消费 inner `ResponseLine` 到 flow state,不直接透传给 client。
- inner `SaveFile` 和 `SaveArtifact` 都进入 outer frames,并保持在 final `@response` 之前。
- `options.trace:"savefile"` 会生成 `flow-trace-<id>.jsonl` 的 `@savefile` frame。
- final `@response` 的 summary 包含 `status`、`response_count`、`artifacts`、`trace_record_count`。

## 验证

- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `cargo fmt -- --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk git diff --check`: passed。
## [2026-06-29 11:21:43] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: Ultragoal G004 control-core/ui_script regression

## 综合发现

- G003 已经把 `ControlCommand::Flow` 通过 `execute_explicit_control_request` 接入,且没有新增 transport-specific 分支。
- G004 主要做验证收口: 确认 `@flow` parser/runtime/control-core 通过,同时确认 `rdog ui-script run` 相关测试仍通过。

## 验证

- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 35 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script --quiet`: 20 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。

## [2026-06-29 12:01:26] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: Ultragoal G005 review fix 和 final verification

## 来源

### 来源1: independent `code-reviewer`

- 要点:
  - 没有发现 `@flow` 绕过 shell policy、另开 transport、吞 inner response 或 SaveFile 顺序错误。
  - 初审给 `COMMENT`,原因是 `src/control_flow.rs` 超过 1000 行,以及 `response_status` / `control_status` 缺少 parse-time `code` 校验。

### 来源2: independent `architect`

- 要点:
  - 架构初审是 `WATCH`,不是 `BLOCK`。
  - `@flow` / `@ui-flow` 边界、shell policy、ControlLine 复用 control core、daemon-local 语义和 outer frame contract 都成立。
  - WATCH 集中在 `src/control_flow.rs` 过大,以及 `response_status` / `control_status` 名字和实现语义未收口。

## 综合发现

- 已把 `src/control_flow.rs` 内联测试拆到 `src/control_flow/tests.rs`。
- 已把 shell process helper 拆到 `src/control_flow/process.rs`。
- `src/control_flow.rs` 当前 993 行,低于项目 1000 行阈值。
- `response_status` / `control_status` 缺少 `code` 现在在 parser validation 阶段失败。
- `specs/rdog-flow-control-plan.md` 明确两者是 v1 alias,都检查最新 inner `@response` code。

## 验证

- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- focused `control_flow::tests` / `control_core::tests` / `control_protocol::tests` / `ui_script`: 5 / 22 / 36 / 20 passed。
- `specs/rdog-flow-control-plan.md` 的 Mermaid 正文通过 `beautiful-mermaid-rs --ascii`。

## [2026-06-29 14:04:01] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: rdog-control skill 文案瘦身

## 来源

### 来源1: `.codex/skills/rdog-control/SKILL.md`

- 当前文件 274 行,约 2532 词。
- 主要问题不是内容错误,而是重复较多:
  - local fast path、tool-use rules、decision flow、unattended mode 多次表达"不要臆造,要验证"。
  - Local Key Chords 示例偏长,可保留少量代表命令,更多细节交给 protocol reference。
  - retry table 和 Do NOT 有重叠,可以改成更短的固定规则。

### 来源2: 记忆与近期实现

- 需要保留 agent-agnostic wording,不要退回 Codex-only。
- 不要继续通过堆 prompt 解决模型行为问题;skill 应短而可执行。
- 新增 `@flow` 已落地,skill 入口应告诉 agent 什么时候用 daemon-side full script flow。

## 改写目标

- frontmatter description 更短,覆盖当前能力但不堆枚举。
- 主体按使用路径组织:
  - Contract
  - Start Here
  - Choose Lane
  - GUI Targeting
  - Keyboard / PTY / Flow
  - Validation / Retry / Safety
  - References
- 保留关键禁止项:
  - 不 invent stdout。
  - 不 pipe rdog 输出给 `jq/grep/head/tail`。
  - GUI action 后必须验证。
  - `@window-resize` 默认恢复/激活,不要写 `activate:true`。
  - display scope 使用 `scope.display` / `guard.display`,不要生成顶层 `display_id`。

## 完成结果

- `.codex/skills/rdog-control/SKILL.md` 从 274 行 / 2532 词压缩到 205 行 / 1209 词。
- frontmatter 升到 `version: "1.5"`,description 改为更短的触发描述。
- 主体改为按 agent 执行路径组织:
  - Contract
  - Start Here
  - Choose The Lane
  - Daemon-Side Flow
  - GUI Targeting
  - Keyboard And PTY
  - Validate And Retry
  - Safety Boundaries
  - References
- 保留 `@flow`、`@window-resize`、display scope、AX diff、PTY、permission 和 destructive-action 边界。

## 验证

- `awk '/^```/{c++} END{print c}' .codex/skills/rdog-control/SKILL.md`: 20。
- `rtk git diff --check -- .codex/skills/rdog-control/SKILL.md`: passed。
- `rtk git diff --numstat -- .codex/skills/rdog-control/SKILL.md`: 119 insertions / 188 deletions。

## [2026-06-29 14:18:00] [Session ID: 019f0356-e933-7e02-808d-12c495a89f09] 笔记: skill 文案瘦身 continuous-learning

## 六文件摘要

- 涉及上下文集: 默认六文件。`rg --files` 检查后,根目录没有支线六文件。
- 任务目标: 检查并更新 `.codex/skills/rdog-control/SKILL.md` 描述叙述,降低 token 和重复叙述。
- 关键决定: 不改协议语义;保留 agent-agnostic;把低频协议细节交给 references / specs。
- 实际变更: skill 从 274 行 / 2532 词压缩到 205 行 / 1209 词,主体按执行路径重排。
- 归档动作: 旧 `WORKLOG.md` 移到 `archive/default_history/WORKLOG_2026-06-29_rdog_control_skill_compaction.md`。
- 沉淀位置: `EXPERIENCE.md` 记录 skill token 纪律;`AGENTS.md` 更新 skill 和 manifest 索引;新增 archive manifest。

## 可复用结论

- agent-facing skill 的主文件要短而可执行。
- 高频路径、硬边界和验证规则留在 skill 主体。
- 低频命令细节、完整协议和历史背景放到 references / specs。
- 文案压缩后要用关键词检查确认关键语义没有被删掉。

## [2026-06-29 14:40:00] [Session ID: codex-20260629-progress-analysis] 笔记: 当前项目进展与下一步价值判断

## 来源

### 来源1: 当前六文件

- `WORKLOG.md` 显示最近已完成 `rdog-control` skill 文案瘦身和 WORKLOG 续档。
- `LATER_PLANS.md` 当前保留的真实未完成项包括:
  - `rdog control --ui-script <file.json>` 兼容入口。
  - `--compat iced-emg` 和更细 policy flags。
  - 一个安全 GUI live smoke。
  - `src/main.rs` 中 UI script runner 职责拆分。
  - Zenoh flake 等待再次自然复现。
  - 方向 B 直接 UDS 控制面作为独立性能路线。
- `EPIPHANY_LOG.md` 里最需要持续记住的风险是 Zenoh 并发 flake 和 unixpipe 是 FIFO 而不是 UDS。

### 来源2: `specs/rdog-ui-script-control-plan.md`

- 当前状态已经更新为:
  - `rdog ui-script run [TARGET] path/to/script.json` 最小真实 runner 已落地。
  - run directory、`trace.jsonl`、`summary.json`、`script.normalized.json`、`artifacts/` 已落地。
  - 最小真实 `Expect` 已落地。
  - `rdog control --ui-script`、`--compat iced-emg`、完整 web/AX `Expect` 和 GUI-only `@ui-flow` 仍是计划。
- Phase 3 剩余项主要是 CLI 兼容入口和 compat/policy。
- Phase 4 是 focused live smoke。

### 来源3: 当前 git 状态

- 工作区仍有较大 diff,包含 `@flow`、UI script runner、skill 文案、上下文文件和未跟踪的新模块/fixture。
- 这说明继续新增大功能前,最好先收口当前 diff 边界,避免不同主线互相污染。

## 综合判断

- 当前最值得做的第一件事不是新增协议,而是把已完成的大块工作拆清楚、验证清楚、提交或至少形成可 review 边界。
- 功能主线下一步最有价值的是 UI script runner 结构拆分和 `rdog control --ui-script` 兼容入口。
- 之后才是 `--compat iced-emg`、更完整的 web/AX Expect 和 focused GUI live smoke。
- 方向 B 直接 UDS 控制面价值高,但不是当前主线阻塞项。
- Zenoh flake 已有诊断,应等待自然复现后按 EPIPHANY 推荐顺序做最小可证伪修复。

## [2026-06-29 15:02:00] [Session ID: codex-20260629-big-diff-closeout] 笔记: 当前大 diff 收口

## 分组结果

- `@flow` 主线:
  - `src/control_flow.rs`
  - `src/control_flow/process.rs`
  - `src/control_flow/tests.rs`
  - `src/control_protocol/tests/flow.rs`
  - `src/control_core.rs`
  - `src/control_protocol.rs`
  - `specs/rdog-flow-control-plan.md`
- UI script runner 主线:
  - `src/main.rs`
  - `src/ui_script.rs`
  - `src/input.rs`
  - `src/zenoh_control.rs`
  - `tests/fixtures/ui_script/ping_control_line.json`
  - `tests/fixtures/ui_script/ping_expect_response.json`
  - `specs/rdog-ui-script-control-plan.md`
- skill/docs/context 主线:
  - `.codex/skills/rdog-control/SKILL.md`
  - `AGENTS.md`
  - `EXPERIENCE.md`
  - `task_plan.md`
  - `notes.md`
  - `WORKLOG.md`
  - `ERRORFIX.md`
  - `LATER_PLANS.md`

## 清理动作

- 删除未跟踪临时噪音:
  - `.codex/skills/rdog-control/SKILL.md.v1.1.bak`
  - `.codex/skills/rdog-control/SKILL.md.v1.2.bak`
  - `.codex/skills/rdog-control/test-prompts.json`
- 删除前已读取确认:两个 `.bak` 是旧 skill 备份,`test-prompts.json` 是无人值守 prompt 实验输入,不属于当前产品/测试矩阵。

## 验证

- `rtk cargo fmt -- --check`: passed。
- `rtk git diff --check`: passed。
- `rtk cargo check --package rustdog --bin rdog --quiet`: passed。
- `rtk cargo test --package rustdog --bin rdog control_flow::tests --quiet`: 5 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests::flow --quiet`: 7 passed。
- `rtk cargo test --package rustdog --bin rdog control_core::tests --quiet`: 22 passed。
- `rtk cargo test --package rustdog --bin rdog ui_script --quiet`: 20 passed。
- `rtk cargo test --package rustdog --bin rdog control_protocol::tests --quiet`: 36 passed。
- `rtk cargo test --package rustdog --bin rdog --quiet`: 434 passed。
- `rtk cargo test --package rustdog --test control_lanes --quiet`: 15 passed,1 ignored。
- `./target/debug/rdog ui-script run --dry-run tests/fixtures/ui_script/ping_control_line.json`: dry-run 输出 `@ping` control line。
- 8 个 Mermaid block 通过 `beautiful-mermaid-rs --ascii`。

## 发现与修正

- `tests/control_lanes.rs::control_one_shot_should_reject_at_line_without_target` 仍假设 `rdog control @ping` 必须失败。
- 当前产品语义已经改变:空 target one-shot 是本机 local-default fast path。
- 测试已改为使用唯一不存在 namespace,验证找不到本机 daemon 时返回清晰错误。

## 剩余结构风险

- `src/main.rs`: 2461 行。
- `src/ui_script.rs`: 1013 行。
- `src/control_core.rs`: 1080 行。
- 当前收口没有继续扩大成重构。下一步最值得先拆 `src/main.rs` 中 UI script runner 职责。
