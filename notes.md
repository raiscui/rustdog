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
