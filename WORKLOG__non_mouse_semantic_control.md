## [2026-05-17 10:50:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 1 落地与 Ralph 收尾

### 任务内容
- 落地 `@ax-action`、`@ax-set-value`、`@type-text` 的第一批非鼠标语义控制能力。
- 保持 `@ax-press` 兼容映射,并把 agent-facing 文档与 skill 更新到非鼠标优先策略。
- 在 Ralph 收尾阶段修正协议真实性缺口,确保文档、parser 和运行时能力一致。

### 完成过程
- 扩展 `src/control_ax.rs`:
  - 新增 action / set-value / type-text 请求与 report 结构。
  - 扩展 `AxBackend` 为 `perform_action()` 与 `set_value()`。
  - 保留 `@ax-press` 作为 `AXPress` 兼容入口。
- 扩展 `src/control_ax/macos.rs`:
  - 新增通用 AX action 执行与 AXValue 写入。
  - 收尾统一 `AX target id` 错误文案,避免新协议回退成旧 `@ax-press target` 口径。
- 扩展协议与执行层:
  - `src/control_protocol.rs`
  - `src/control_actions.rs`
  - `src/control_core.rs`
  - `src/shell.rs`
- 更新长期规格和 agent 使用说明:
  - `specs/rdog-non-mouse-semantic-control-plan.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `AGENTS.md`
  - `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`
- Ralph 收尾时补掉两处真实性问题:
  - `@type-text allow_clipboard:true` 不再静默接受,改为显式拒绝。
  - `AX target` / `AX target id` 相关错误文案全部泛化。

### 验证
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --tests --no-run`
- `cargo build --package rustdog --bin rdog`
- `cargo fmt -- --check`
- `git diff --check`
- 全部通过

### 总结感悟
- 非鼠标语义控制的关键不是“再增加几条命令”,而是让 agent 明确知道哪些能力真的存在,哪些还没有开放。
- `allow_clipboard` 这类未来能力字段,在未实现前必须显式拒绝,不能默默吞掉。
- `AX target` 这类共享定位语义一旦升级成通用层,错误文案也必须同步泛化,否则 agent 会被历史命名误导。

### 提交记录
- local commit: `6497ab6`
- 提交标题: `Make non-mouse control truthful for agents`

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 1 review fix

### 任务内容
- 根据对 `1d580eb` 的 code review,修复 append 语义、type-text 错误口径和 redaction report 真实性问题。

### 完成过程
- 在 `src/control_ax/macos.rs` 中把 append 行为改成:
  - 仅在当前 `AXValue` 可读时才允许 append
  - 当前值不可读时直接失败
- 在 `src/control_ax/macos.rs` 中新增 target redaction 推导,让 report 使用真实 secure 状态。
- 在 `src/control_ax.rs` 中新增 `remap_type_text_ax_value_error()`,让 `@type-text` 不再冒用 `AX set value` 协议名。
- 同步修正 `specs/rdog-non-mouse-semantic-control-plan.md` 的 append 文案。
- 补充 focused tests,锁住:
  - append 不可读时报错
  - type-text 错误口径独立
  - redaction report 不再固定 false

### 验证
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`
- 全部通过

### 总结感悟
- append 这类“看起来只是字符串处理”的能力,只要偷做一次 silent fallback,就会直接破坏 agent 对协议的信任。
- report 字段一旦对外暴露,宁可少报,也不能伪造固定值。

## [2026-05-17 12:08:33] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 2 第一轮实现

### 任务内容
- 落地 `@key delivery`、`@ax-focus`、`@ax-scroll`、`@type-text` 的 targeted-keyboard / clipboard 真实现。
- 保持旧 `@key` 兼容,同时让新协议显式汇报真实 delivery。
- 同步更新 specs / usage / skill / AGENTS 长期索引。

### 完成过程
- 扩展 `src/control_protocol.rs`
  - `KeyRequest` 新增 `delivery/pid/window_id/response_mode`
  - 新增 `@ax-focus`、`@ax-scroll` parser
  - `@type-text` 新增 `targeted-keyboard` / `clipboard`
- 扩展 `src/control_ax.rs`
  - 新增 `KeyDeliveryReport`、`AxFocusReport`、`AxScrollReport`
  - 扩展 `AxBackend` 到 focus / scroll / type_text
- 扩展 `src/control_ax/macos.rs`
  - 新增 `CGEventPostToPid` 键盘 / 滚动定向投递
  - 新增 AX focus helper
  - 新增 clipboard opt-in 文本输入和剪贴板恢复
- 扩展执行层
  - `src/control_actions.rs`
  - `src/control_core.rs`
  - `src/shell.rs`
  - `src/zenoh_control.rs`
- 同步长期文档:
  - `specs/rdog-non-mouse-semantic-control-plan.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `AGENTS.md`
  - `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`

### 验证
- `cargo fmt`
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
- `cargo test --package rustdog --bin rdog -- control_actions::tests --nocapture`
- `cargo test --package rustdog --bin rdog --no-run`
- `cargo build --package rustdog --bin rdog`
- `git diff --check`
- 全部通过

### 总结感悟
- `@key` 这种老命令做演进时,关键不是“能不能多传几个字段”,而是要把 legacy success 和 structured success 的边界说清楚。
- `@ax-scroll` 这类名字和真实后端不完全同构的能力,必须把 `delivered_via`、`line_steps` 这种“真实发生了什么”显式回给 agent。
- clipboard fallback 只要一旦开放,就必须做到显式 opt-in 和恢复剪贴板,否则很容易和“避免干扰人类操作”目标冲突。

### 提交记录
- local commit: `3725ce9`
- 提交标题: `Deliver truthful non-mouse semantic control for agent-safe GUI automation`

## [2026-05-17 13:12:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 2 提交收口

### 任务内容
- 对 Phase 2 第一轮 diff 做提交前复核。
- 重新跑 focused verification,确认这轮能力已经满足本地提交条件。
- 只提交非鼠标语义控制相关文件,不混入其他支线。

### 完成过程
- 复跑并确认以下证据仍成立:
  - `control_ax::tests`
  - `control_protocol::tests`
  - `control_actions::tests`
  - `control_core::tests`
  - `cargo build --package rustdog --bin rdog`
  - 针对本轮文件集合的 `git diff --check`
- 审阅并确认本次提交边界:
  - 协议层: `src/control_protocol.rs`
  - 语义控制层: `src/control_ax.rs`, `src/control_ax/macos.rs`
  - 执行层: `src/control_actions.rs`, `src/control_core.rs`, `src/shell.rs`, `src/zenoh_control.rs`
  - 长期文档: `specs/rdog-non-mouse-semantic-control-plan.md`, `specs/code-agent-rdog-control-usage.md`, `AGENTS.md`
  - 本支线 context 文件
- 完成 local commit:
  - `3725ce9 Deliver truthful non-mouse semantic control for agent-safe GUI automation`

### 总结感悟
- 一旦 worktree 同时跑多条线,提交前显式收窄文件集合很关键,否则很容易把未验证实验线一起带进历史。
- 对 agent-facing 协议来说,真实 response schema 比“看起来更方便的 fallback”更重要。

## [2026-05-17 14:06:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 非鼠标语义控制 Phase 2.1 live E2E 与 `@key` 单测收口

### 任务内容
- 给 `@key delivery:"global"` 的 structured success 增加独立 unit seam test。
- 为已授权的 rdog 增加一个不碰鼠标的 macOS live ignored E2E。
- 用真实桌面证明:
  - `@ax-focus activate:true`
  - `@type-text mode:"targeted-keyboard"`

### 完成过程
- 在 `src/control_actions.rs` 中抽出 `structured_global_key_success_response()`:
  - 让 `execute_key()` 的 global structured success 走可单测的纯函数缝
  - 新增 focused unit test 锁住:
    - `kind:"key"`
    - `backend:"global-input-simulation"`
    - `delivery:"global"`
- 在 `tests/control_ax_e2e.rs` 中新增 live ignored E2E:
  - 使用临时 TextEdit 文档作为真实目标
  - 先用 `@window-find` 锁定真实 `window_id`
  - 把 TextEdit app 隐藏
  - 再用 `@ax-focus activate:true` 恢复窗口
  - 用 `@ax-get(window_id)` 在单窗口树里定位编辑区 target
  - 最后用 `@type-text mode:"targeted-keyboard"` 输入文本,并用 `@ax-get(target_id)` 回读 AXValue
- 调试过程中还修正了 live fixture:
  - 避开会拖死 daemon 的重型全局 `@ax-find`
  - 改用 `@window-find -> @ax-get(window_id)` 两段式
  - 修正 TextEdit 隐藏用的 AppleScript
  - 修正 targeted-keyboard report 的真实 backend 断言

### 验证
- `cargo test --package rustdog --bin rdog -- control_actions::tests::structured_global_key_success_response_should_report_structured_global_success --exact`
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_focus_hidden_textedit_and_type_without_mouse --exact --ignored --nocapture`
- `git diff --check -- src/control_actions.rs tests/control_ax_e2e.rs task_plan__non_mouse_semantic_control.md notes__non_mouse_semantic_control.md WORKLOG__non_mouse_semantic_control.md`
- 以上全部通过

### 总结感悟
- 对真实 GUI 文本输入场景,把“找窗口”和“找元素”拆成两段,比一条全局 AX 查询稳得多。
- live E2E 的价值不只是“最后绿了”,更在于它把协议名、backend 名、桌面状态恢复路径都压成了真实证据。

## [2026-05-17 14:30:14] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: 校正 `@key` 与普通文本输入的职责边界

### 任务内容
- 根据用户新增约束,把 `@key` 和 `@type-text` 的使用场景正式拆开。
- 修正 live ignored E2E,不再把 `@key` 当作普通字符输入能力来证明。
- 同步更新 specs、agent usage 文档和全局 `rdog-control` skill。

### 完成过程
- 在 `specs/rdog-non-mouse-semantic-control-plan.md` 中明确:
  - `@key` 主要用于快捷键、功能键、导航键和 app 功能触发
  - 普通文本输入优先走 `@ax-set-value` / `@type-text`
  - `targeted-keyboard` 仍是文本输入路径,依然可能受输入法和焦点影响
- 在 `specs/code-agent-rdog-control-usage.md` 中同步 agent-facing 用法:
  - `@key` 不再被描述成通用文本输入
  - `@type-text` 被明确标成普通文本输入入口
- 在 `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md` 中补上面向 code agent 的经验口径:
  - 不要把 `@key:"1"` / `@key:"a"` 当成稳定文本输入
  - 普通文本输入优先用 `@type-text`
- 在 `tests/control_ax_e2e.rs` 中:
  - 删除输入法切换 guard 和相关 helper
  - 把 live targeted-key 测试改成热键场景
  - 改为用 `Cmd+A` + `Backspace` 验证“真实 app 状态变化”,而不是验证字符是否输入
  - 新增 `wait_for_textedit_value_exact()` 方便断言文本被整段清空

### 验证
- `cargo fmt`
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `git diff --check -- specs/rdog-non-mouse-semantic-control-plan.md specs/code-agent-rdog-control-usage.md tests/control_ax_e2e.rs task_plan__non_mouse_semantic_control.md notes__non_mouse_semantic_control.md`
- 以上全部通过

### 总结感悟
- `@key` 有意义,但它的意义不是“代替所有文本输入”,而是“把按键当成功能触发”。
- 一旦协议层同时拥有 `@key` 和 `@type-text`,live E2E 也必须跟着分工,否则测试会反过来污染产品语义。

## [2026-05-17 15:11:59] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 任务名称: Phase 2.2 targeted key 与 AX scroll live E2E 收口

### 任务内容
- 为 `@key pid-targeted` / `@key window-targeted` 补 live ignored E2E。
- 为 `@ax-scroll` 补 live ignored E2E。
- 修正 `@ax-scroll` 后端,让 response 和真实桌面行为一致。

### 完成过程
- 将 targeted key live E2E 改成:
  - 先用 `@type-text mode:"targeted-keyboard"` 输入 `"AB"`
  - 再用 `@key delivery:"pid-targeted",key:"Backspace"` 删除成 `"A"`
  - 再用 `@key delivery:"window-targeted",key:"Backspace"` 删除成空字符串
- 为 scroll live E2E 建立长文本小窗口 TextEdit fixture。
- 发现 `pid-scroll-event` 返回 success 但不改变 TextEdit 滚动状态。
- 改为 `AXScrollBar AXValue` 语义路径:
  - 在目标窗口内找同方向 `AXScrollBar`
  - 检查 `AXValue` settable
  - 按 direction/pages 写入新的 0..1 比例值
  - response 改为 `delivered_via:"ax-scrollbar-value"`
- 修正 scroll E2E 判据:
  - 优先使用 `AXValueIndicator.rect.y` 作为真实滚动位置
  - fallback 才看 `AXScrollBar.value`

### 验证
- `cargo fmt`
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
- `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_deliver_pid_and_window_targeted_hotkeys_to_real_textedit --exact --ignored --nocapture`
- `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_scroll_real_textedit_without_mouse --exact --ignored --nocapture`
- targeted key live: 1 passed
- scroll live: 1 passed, `before=109`, `after=211`

### 总结感悟
- GUI semantic control 不能只看“API 调用成功”。必须回读真实 UI 状态。
- AX 字段并不总是稳定同构。TextEdit 滚动后可能不再返回 `AXScrollBar.value`,但 indicator 位置仍是可靠的可观察状态。

## [2026-05-17 15:30:18] [Session ID: codex-20260517-clipboard-restore] 任务名称: Phase 2.3 clipboard 文本投递恢复语义收口

### 任务内容
- 收紧 `@type-text mode:"clipboard"` 的剪贴板恢复行为。
- 在 response 中暴露 clipboard 恢复策略和恢复结果。
- 同步更新 repo specs、agent usage 文档和全局 `rdog-control` skill。

### 完成过程
- 将 macOS clipboard 路径从“投递后无条件恢复旧剪贴板”改为 `restore-if-unchanged`。
- 新增 `ClipboardRestoreStatus`,让 `TypeTextReport` 能表达:
  - `clipboard_restore_policy:"restore-if-unchanged"`
  - `clipboard_restored:true|false`
  - `clipboard_restore_skipped_reason`
- 补充 focused tests:
  - response schema 暴露恢复状态
  - clipboard 恢复决策只在临时值仍存在时恢复
- 修正规格文档里的 clipboard code fence 嵌套问题。

### 验证
- `cargo fmt`
- `cargo fmt -- --check`
- `cargo test --package rustdog --bin rdog -- control_ax::tests::type_text_clipboard_report_should_expose_restore_status control_ax::macos::tests::clipboard_restore_decision_should_restore_only_when_temporary_value_survived --nocapture`
- `cargo test --package rustdog --bin rdog -- control_ax:: --nocapture`
- `cargo test --package rustdog --test control_ax_e2e --no-run`
- `cargo test --package rustdog --bin rdog -- control_core::tests::explicit_request_should_route_ax_commands_to_executor --exact --nocapture`
- `git diff --check -- src/control_ax.rs src/control_ax/macos.rs specs/rdog-non-mouse-semantic-control-plan.md specs/code-agent-rdog-control-usage.md task_plan__non_mouse_semantic_control.md`
- 以上全部通过

### 总结感悟
- clipboard fallback 不是“无副作用输入”。它必须把人类剪贴板当成共享资源。
- 对 agent 来说,仅有 `used_clipboard:true` 不够,还需要知道恢复策略和恢复结果,否则无法判断现场是否被污染。
