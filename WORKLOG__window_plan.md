# 工作日志: rdog window control ralplan

## [2026-05-16 15:21:27] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: `@window-*` 被遮挡窗口控制共识规划

### 任务内容
- 将 `.omx/specs/deep-interview-rdog-occluded-window-control.md` 转成正式 ralplan handoff.
- 输出 consensus plan, PRD 和 test spec.
- 保留 Architect/Critic 的关键修订: short-lived `window_id`, terminate/kill 必填审计字段, graceful 与 force close 的 ambiguity 边界, live E2E full-loop 证据.

### 完成过程
- 复用已完成的 Planner/Architect/Critic 草案和评审结果.
- 创建 `.omx/plans/rdog-window-control-consensus-plan.md`.
- 创建 `.omx/plans/prd-rdog-window-control.md`.
- 创建 `.omx/plans/test-spec-rdog-window-control.md`.
- 用 `beautiful-mermaid-rs --ascii` 验证 Mermaid flowchart 和 sequence diagram.
- 用 `git diff --check` 验证新增计划文件和支线 task plan 没有 whitespace 问题.
- 清理 ralplan active state,确认 `omx state list-active --json` 返回空 active modes.

### 总结感悟
- 这条规划的关键不是“再加一个 AX 包装”,而是把窗口状态,窗口生命周期和 UI 元素操作拆开.
- `@window-find -> @window-activate -> @click/@key/@ax-press` 是 Phase 1 的单一交互真相源.
- terminate/kill 必须像审计事件一样可追溯,不能只返回普通成功或失败.

## [2026-05-16 17:35:45] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: `@window-*` Phase 1 实现与真实桌面验证

### 任务内容
- 落地 `@window-find`、`@window-activate`、`@window-close` 的协议、执行器接线和 macOS backend.
- 新增 live ignored E2E,证明被遮挡、最小化、hidden app 的窗口可以被真实发现、恢复并关闭.
- 将窗口控制方案同步到 repo spec、AGENTS 索引和 `rdog-control` skill 参考文档.

### 完成过程
- 新增 `src/control_window.rs` 与 `src/control_window/macos.rs`,定义 schema、payload parser、窗口状态模型、macOS activate/close 流程和 non-macOS unsupported 路径.
- 修改 `src/control_protocol.rs`、`src/control_actions.rs`、`src/control_core.rs`、`src/main.rs`,把 `@window-*` 命令接入 line-control 主路径,并让 structured invalid-input JSON 能原样透传.
- 新增 `tests/control_window_e2e.rs`,复用 live daemon harness,对 TextEdit 窗口真实执行 occluded -> activate、minimized -> activate、hidden -> activate、graceful close 全链路.
- live E2E 首轮失败后,根据动态证据修正了两处真实问题:
  - 测试夹具的 hidden AppleScript 写法不稳,改成 `System Events` 的 `visible=false`.
  - backend 的 `unhide_app` 只按 pid 走单一路径会在真实 hidden app 上失败,改成 pid -> app name -> activate fallback.
- 新增 `specs/rdog-window-control-plan.md`,并把索引同步进 repo `AGENTS.md`.
- 更新全局 `rdog-control` skill 及其 `control-workflow.md`、`protocol.md`,让 agent 学会先 `@window-find`,再 `@window-activate`,最后做输入或 close.

### 总结感悟
- 这条能力最关键的不是 parser 能过,而是 live ignored E2E 真正打到了 hidden app 这条最容易失真的路径.
- 真实桌面验证证明 `@window-*` 应该被当成窗口生命周期层,而不是 screenshot/AX 的附属糖衣.
- 安装版 `~/.cargo/bin/rdog` 与工作区当前二进制一旦漂移,live E2E 会出现“协议不存在”的假失败,后续复跑时应优先用当前工作区构建出的 `rdog`.

## [2026-05-16 17:39:39] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: `@window-*` 收尾 fresh 验证与 Ralph 清理

### 任务内容
- 在 local commit `26a7005` 上追加 fresh 验证证据.
- 清理仍处于 active 的 Ralph runtime state,避免 stop hook 将本任务判为未结束.

### 完成过程
- 在提交后的 HEAD 上再次跑了 `control_window::tests`、窗口协议解析测试、structured invalid-input test.
- 再次跑了 `RDOG_LIVE_WINDOW_E2E=1 RDOG_LIVE_WINDOW_E2E_VIA_TERMINAL=1 cargo test --package rustdog --test control_window_e2e -- --ignored --nocapture`,确认真实桌面窗口 E2E 在提交后仍通过.
- 执行 `omx cancel ralph`,随后用 `omx state list-active --json` 确认 active modes 已清空.

### 总结感悟
- 对带 runtime workflow 的任务,local commit 完成并不等于真正结束,还要把 runtime state 清到 `active_modes=[]`.
- live ignored E2E 最有价值的地方,是能把“提交前通过,提交后失效”的情况在收尾阶段再拦一遍.

## [2026-05-16 17:57:39] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 修复 commit review 暴露的窗口控制边界问题

### 任务内容
- 修正 `26a7005` review 中暴露的 3 个问题: `window_id` 被内部 limit 截断、`current_space` 被同 pid 其他窗口污染、`select:\"frontmost\"` 没有窗口级前台语义.

### 完成过程
- 将 `resolve_single_window` 改为在带 `window_id` 时直接走 locator 解析,不再依赖 `find_response.matches`.
- 保留 `ResolvedWindow.focused`,并让 `select_frontmost` 通过 resolver 优先选 focused window.
- 收紧 `match_visible_window`,只在 title/rect 有真实匹配证据时才认定当前 Space,去掉同 pid 任意窗口兜底.
- 新增 3 个 macOS backend focused 单测:
  - `visible_window_match_should_not_fallback_to_any_pid_window`
  - `select_frontmost_should_prefer_focused_window`
  - `resolve_single_window_should_use_direct_window_id_lookup_even_when_find_result_is_truncated`

### 总结感悟
- 这类窗口控制协议,最危险的不是明显崩溃,而是“状态看起来像对的,但其实引用了另一扇窗口的事实”.
- `window_id` 只要被任何内部分页/limit 路径二次解释,就会失去作为 follow-up locator 的价值.

## [2026-05-16 23:17:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 稳定 `@window-*` live E2E 并修复 current_space 误判

### 任务内容
- 将不稳定的 TextEdit 被测 fixture 改成 Finder 文件夹窗口,同时保留 TextEdit 只作为遮挡窗口.
- 修复 macOS backend 在 AX title 和 CGWindow name 不一致时误判 `current_space:false` 的问题.
- 避免 live ignored E2E 残留多个 Terminal daemon 窗口.

### 完成过程
- 通过 fresh live 失败输出确认 Finder 目标窗口真实存在,但 `@window-find` 返回 `current_space:false`.
- 用 Swift/CoreGraphics 最小探针确认 CGWindowList 里同 pid Finder 窗口 rect 与 AX rect 完全一致,只是 CGWindow name 退化为 `T`.
- 将 `match_visible_window` 从 title+rect 必须同时命中,改为同 pid 下 title 或 rect 任一真实命中即可.
- 增加单测覆盖 `CGWindow title` 与 `AX title` 不一致但 rect 精确命中的场景,同时保留“不回退到同 pid 任意窗口”的旧单测.
- E2E 改为每个状态段使用 fresh `window_id`,符合 short-lived locator 语义.
- Terminal daemon 的 Drop 增加测试窗口清理,并在启动前清理旧的 `rdog-window-e2e-*` 窗口.

### 总结感悟
- macOS 的 AX title 与 CGWindow name 不是同一个真相源,不能拿两者做强 AND 匹配.
- live E2E 里 `window_id` 必须被当成短期 follow-up locator,状态变更后应先重新 find 再操作.
- 测试打开真实 GUI 窗口时,清理逻辑本身就是验收的一部分,否则“测试通过”也会破坏用户桌面状态.

## [2026-05-16 23:36:37] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: shell fake executor 补齐窗口命令分支

### 任务内容
- 将 `src/shell.rs` 中测试 fake executor 对 `@window-find` / `@window-activate` / `@window-close` 的响应补齐.
- 该改动从 mouse E2E 脏工作树中拆出,作为 window-control follow-up 单独提交.

### 完成过程
- 复核 diff 确认它只影响 `shell::tests` 的 fake executor,不是 mouse 控制逻辑.
- 运行 focused shell tests 验证 control receiver 测试面仍通过.

### 总结感悟
- 新增 `ControlCommand` 变体后,测试 fake executor 也要同步补齐,否则后续 shell/control receiver 回归测试容易被新协议分支遗漏.
