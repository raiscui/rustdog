## [2026-05-17 10:36:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: 非鼠标语义控制第一批实现切面

### 静态证据
- `src/control_protocol.rs`
  - `ControlCommand` 只有 `AxPress`,没有 `AxAction` / `AxSetValue` / `TypeText`。
  - parser 入口只接了 `ax-press`。
- `src/control_ax.rs`
  - `AxActionReport.action` 当前是硬编码 `press`。
  - `AxBackend` trait 当前只有 `snapshot()` 和 `press()`。
- `src/control_ax/macos.rs`
  - `press_target_id()` 和 `map_ax_action_error()` 的错误文案都写死 `AXPress`。
- `src/control_actions.rs`
  - 目前只有 `execute_ax_press()`。
- `src/shell.rs`
  - fake executor 只有 `ControlCommand::AxPress` 的 `AX_PRESS:*` 输出。

### 实现策略
- 不在这一轮把 `@click` 改成 AX-first。
- 先把 AX 后端抽象升级为:
  - `perform_action(target, action)`
  - `set_value(target, value, mode)`
- 协议层新增:
  - `@ax-action`
  - `@ax-set-value`
  - `@type-text`
- 第一版 `@type-text` 只支持:
  - `mode:"ax-value"`
  - `mode:"auto"` 但只走 AXValue 分支
- `@ax-press` 保留,内部映射成 `@ax-action action:"AXPress"`。

### 风险提醒
- `src/control_protocol.rs` / `src/shell.rs` 已经偏大,如果再直接堆 parser 和 fake arms,后续会更难维护。
- 因此实现时尽量把新增结构和 parser 继续收在 `src/control_ax.rs`,避免把 AX 相关知识散落到更多地方。

## [2026-05-17 10:50:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Ralph 收尾自审结论

### 现象
- 第一轮 focused verification 已经通过,但二次文本扫描又发现 `src/control_ax/macos.rs` 里残留 `@ax-press target id ...` 错误文案。
- `parse_type_text_payload()` 在主实现完成后,仍会静默接受 `allow_clipboard:true`,这和当前运行时能力不一致。

### 假设
- 这两处都属于协议真实性问题,不是执行链路故障。
- 如果不修,agent 会被错误文案误导,以为:
  - 新命令的 target id 失败仍然是 `@ax-press` 专属错误。
  - `@type-text` 已经具备 clipboard fallback。

### 已验证修正
- `src/control_ax.rs`
  - `parse_ax_target()` 的重复字段和字段解析文案统一改为 `AX target`。
  - `@type-text allow_clipboard:true` 改为显式报错。
  - 新增 parser test,锁住:
    - 不再出现 `@ax-press target`
    - `allow_clipboard:true` 会失败
- `src/control_ax/macos.rs`
  - `parse_target_id()` 的非法 target id 错误文案统一改为 `AX target id ...`
  - 新增单测,确保不再回退到 `@ax-press target`

### 动态证据
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
  - 9 passed
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
  - 14 passed
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
  - 11 passed
- 复跑收尾验证:
  - `cargo test --tests --no-run`
  - `cargo build --package rustdog --bin rdog`
  - `cargo fmt -- --check`
  - `git diff --check`
  - 全部通过

### 结论
- 当前 Phase 1 非鼠标语义控制已经从“主链能跑”收敛到“协议口径和实际能力一致”。
- 尚未进入的 Phase 2 仍然是:
  - `@key delivery`
  - `@ax-focus`
  - `@ax-scroll`
  - `@type-text targeted-keyboard / clipboard`

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Phase 1 review 后补真修复策略

### 现象
- review 证明 `1d580eb` 仍有 3 个不够“说真话”的点:
  - append 可能静默覆盖
  - `@type-text` 会复用 `AX set value` 的 unsupported 错误口径
  - redaction report 固定写 `false`

### 修复策略
- append:
  - 只有在当前 `AXValue` 可读且可转成字符串时才允许 append
  - 否则返回结构化 invalid input,拒绝偷偷 replace
- type-text:
  - 保持当前只走 AXValue 路径
  - 但在 Unsupported / PermissionDenied / 其它错误上保留 `@type-text` 自己的协议名
- redaction:
  - 用目标元素的 `AXRole` / `AXSubrole` 推导是否是 secure element
  - secure 时把 old/new redacted 标成 `true`
  - 非 secure 才标 `false`

### 需要同步的文档口径
- `append` 不再写成“先读取再拼接”这么轻描淡写。
- 要明确“当前值不可读时 append 失败”。
- `old_value_redacted/new_value_redacted` 不再暗示固定有值,而是表达真实 redaction 状态。

## [2026-05-17 10:59:54] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: review 修复后的结论

### 已修复事实
- `src/control_ax/macos.rs`
  - `append` 现在走 `build_final_ax_value()`。
  - 当前 `AXValue` 不可读时,明确报错 `无法执行 append`,不再静默 replace。
  - `target_value_is_redacted()` 会读取目标元素的 `AXRole` / `AXSubrole`,再复用 `looks_like_secure_element()` 推导 redaction。
- `src/control_ax.rs`
  - `AxSetValueReport::success()` 不再硬编码 redaction 为 `false`,而是接收真实值。
  - `perform_default_type_text()` 经过 `remap_type_text_ax_value_error()` 包一层协议名映射。
  - 非 macOS unsupported 文案从 `AX set value` 纠正为 `type-text` 自己的路径描述。
- `specs/rdog-non-mouse-semantic-control-plan.md`
  - append 语义补成“当前值不可读即失败”。

### 动态证据
- `cargo test --package rustdog --bin rdog -- control_ax::tests --nocapture`
  - 11 passed
- `cargo test --package rustdog --bin rdog -- control_protocol::tests --nocapture`
  - 14 passed
- `cargo test --package rustdog --bin rdog -- control_core::tests --nocapture`
  - 11 passed
- `cargo build --package rustdog --bin rdog`
  - 通过
- `git diff --check`
  - 通过

### 结论
- 这轮修复把 Phase 1 从“主链可用”推进到“append / type-text / redaction 三个敏感点也说真话”。
- 现在再开 Phase 2,不用继续背着 Phase 1 的协议真实性债。

## [2026-05-17 11:14:33] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Phase 2 第一轮实施边界

### 静态证据
- `src/control_protocol.rs`
  - `KeyRequest` 仍只有 `key/hold_ms/mode`。
  - `parse_key_object_payload()` 还不认识 `delivery/pid/window_id`。
- `src/control_actions.rs`
  - `execute_key_with_dependencies()` 成功后没有结构化 report,所以现在无法诚实回报“实际投递到了哪里”。
- `src/control_ax.rs`
  - `TypeTextReport` 已经有 `delivered_via/used_clipboard`,说明 type-text 新模式不需要另起 response schema。
  - `TypeTextMode` 目前只有 `auto/ax-value`。
- `src/control_window.rs` + `src/control_window/macos.rs`
  - 已经稳定提供 `window_id = pid:<pid>/window:<index>` 解析与 direct lookup,可以给 Phase 2 复用。

### 实施策略
- `@key`
  - 兼容旧字符串 payload。
  - 只对显式 object + targeted 字段返回结构化 `kind:"key"` 报告。
- `@ax-focus`
  - 默认 `activate:false`。
  - 只在请求显式写 `activate:true` 时复用现有 `@window-activate` recipe。
- `@ax-scroll`
  - 第一轮只做 AX action / AX value 层可解释的滚动,不偷偷回退到全局 wheel。
- `@type-text`
  - `auto` 先按 `ax-value -> targeted-keyboard -> clipboard(opt-in)` 梯子尝试。
  - clipboard 必须是显式允许,并且 response 必须说明是否真的用了剪贴板。

### 主要风险
- `src/control_ax/macos.rs` 已经很大,Phase 2 实现时要尽量抽出小 helper,否则文件会继续膨胀。
- `@key` 返回形态一旦改坏,会连带影响 shell / control_core / lane tests,需要分清“旧路径兼容”和“新路径结构化成功”两类断言。

## [2026-05-17 13:10:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Phase 2 提交前复核结论

### 现象
- 当前 worktree 里混有多个别的支线文件,不能直接整体提交。
- 本轮 Phase 2 相关文件重新复跑后:
  - `control_ax::tests` 13 passed
  - `control_protocol::tests` 14 passed
  - `control_actions::tests` 14 passed
  - `control_core::tests` 11 passed
  - `cargo build --package rustdog --bin rdog` 通过
  - 针对本轮文件集合执行 `git diff --check` 通过

### 复核结论
- 当前没有发现需要在提交前继续返工的阻塞问题。
- `@key delivery`、`@ax-focus`、`@ax-scroll`、`@type-text targeted-keyboard/clipboard` 这几条线已经满足“协议说真话”的最小提交条件。
- 这次提交应只包含:
  - `src/control_protocol.rs`
  - `src/control_ax.rs`
  - `src/control_ax/macos.rs`
  - `src/control_actions.rs`
  - `src/control_core.rs`
  - `src/shell.rs`
  - `src/zenoh_control.rs`
  - `specs/rdog-non-mouse-semantic-control-plan.md`
  - `specs/code-agent-rdog-control-usage.md`
  - `AGENTS.md`
  - 本支线 context 文件

### 剩余风险
- `src/control_ax/macos.rs` 继续膨胀的问题没有在这次提交里处理,后续如果继续做 live E2E 或再扩能力,最好优先拆 helper。

## [2026-05-17 14:05:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: Phase 2.1 live ignored E2E 调试结论

### 现象
- 新增的 TextEdit live ignored E2E 首轮失败,不是一上来就通过。
- 第一轮失败现象:
  - 重型 `@ax-find` 请求会导致 `rdog control` 在收到结果前连接关闭。
  - 随后 daemon 端口也消失,说明这条请求会把 daemon 拖死。
- 第二轮失败现象:
  - 改成轻量 `@ax-find` 后 daemon 不再崩,但 `match_count = 0`。
- 第三轮失败现象:
  - 改成 `@window-find -> @ax-get(window_id)` 两段式后,live 链路已经能走到 `@type-text targeted-keyboard`。
  - 最后暴露的只是不正确的测试断言:
    - 实际 backend 是 `macos-cg-event-post-to-pid`
    - 不是 `macos-accessibility`

### 假设演进
- 假设1: TextEdit 场景只是等待条件不稳。
  - 被手工复现推翻:
    - 同样的重型 `@ax-find` 会直接让 daemon 退出。
- 假设2: 只是 AX 查找深度不够。
  - 也不完整。
  - 因为 `@ax-find` 的空结果并不等于窗口不存在,只是“窗口定位和编辑区定位混在一条全局元素查询里”不稳。
- 最终成立的假设:
  - 对真实 GUI 文本编辑场景,应该把“找窗口”和“找元素”拆开:

## [2026-05-17 13:56:17] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: 用户补充后的 `@key` / `@type-text` 职责分界

### 用户给出的新边界
- `@key` 是有意义的。
- 它最初就是给快捷键准备的。
- 主要用途是:
  - 呼叫 app
  - 在特定 app 焦点下激活特定功能
  - 发送 `F12` 这类不受输入法影响的按键
- 如果是普通输入,可能会激活输入法状态,这时 `@key` 不适合,需要改用更适合的文本输入路径。

### 调整后的结论
- `@key` 不是“文本输入 API”,而是“按键/热键/功能触发 API”。
- `@type-text` 才是普通文本输入 API。
- `@type-text` 内部再分:
  - `ax-value`: 最语义化,最不受输入法影响
  - `targeted-keyboard`: 仍可能受输入法/焦点影响,但比全局键盘更定向
  - `clipboard`: 最后 fallback,而且必须显式 opt-in

### 对测试策略的影响
- `@key` 的 live E2E 不该再验证 `"1"`、`"2"` 这种字符是否进入文本框。
- 更合适的验证方式是:
  - 用 `Cmd+A`、`Cmd+W`、`Return`、`Tab`、方向键等不依赖输入法状态的键
  - 断言真实 app 状态变化
  - 例如窗口关闭、窗口新建、内容被选择后删除、焦点跳转、滚动状态变化
- `@type-text targeted-keyboard` 可以继续保留真实文本输入 E2E,但它的结果解释要明确标成“文本输入路径”,不能再拿它去代表 `@key`。

## [2026-05-17 15:11:59] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 笔记: `@ax-scroll` live E2E 修正结论

### 现象
- 第一版 scroll live E2E 曾挂起超过 60 秒。
- 轻量化观测后,挂起消失,但 `pid-scroll-event` 后端没有改变 TextEdit 的滚动条 value。
- 改成 AXScrollBar page button 的 AXPress 后,仍然没有让 value 变化。
- 改成写入 AXScrollBar 的 AXValue 后,TextEdit 的滚动条 indicator 真实移动。

### 被推翻的假设
- 假设1: 只要给目标 pid 发 targeted scroll event 就能让 TextEdit 滚动。
  - 动态证据推翻: `@ax-scroll` 返回 success,但 scroll bar value 仍为 0。
- 假设2: 对 AXScrollBar 的 page button 执行 AXPress 就能滚动。
  - 动态证据推翻: AXPress 后 `AXValueIndicator` 没有移动。
- 假设3: 只看 `AXScrollBar.value` 就足够判断滚动。
  - 动态证据修正: 写入 AXValue 后,TextEdit 有时不再返回 scroll bar value,但 `AXValueIndicator.rect.y` 明确从 109 变到 211。

### 已验证结论
- macOS TextEdit 的这条 scroll live E2E 应使用 `AXScrollBar AXValue` 作为后端路径。
- live 成功判据应优先看真实滚动条 indicator 位置变化,而不是只看 value 字段。
- 当前动态证据:
  - `daemon_control_lane_should_scroll_real_textedit_without_mouse`
  - `before=109`
  - `after=211`
    1. `@window-find` 先锁定真实 `window_id`
    2. `@ax-get(window_id)` 再在单窗口树里找 `AXTextArea` / `AXTextField`
  - 这样既避开重型全局 AX 查询,也更符合当前协议分层。

### 动态证据
- focused unit seam:
  - `cargo test --package rustdog --bin rdog -- control_actions::tests::structured_global_key_success_response_should_report_structured_global_success --exact`
  - 1 passed
- integration compile:
  - `cargo test --package rustdog --test control_ax_e2e --no-run`
  - 通过
- live ignored E2E:
  - `RDOG_LIVE_AX_E2E=1 RDOG_LIVE_AX_E2E_VIA_TERMINAL=1 RDOG_LIVE_AX_E2E_BINARY=/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/target/debug/rdog cargo test --package rustdog --test control_ax_e2e -- daemon_control_lane_should_focus_hidden_textedit_and_type_without_mouse --exact --ignored --nocapture`
  - 最终通过,并观察到:
    - `window_id=pid:551/window:0`
    - `target_id=pid:551/window:0/path:0.0`
    - `pid=551`

### 结论
- `@ax-focus activate:true` 已能把隐藏的真实 TextEdit 窗口恢复到可交互状态。
- `@type-text mode:"targeted-keyboard"` 已能在不动鼠标的前提下,把文本输入到真实 TextEdit 编辑区。
- `@key delivery:"global"` 的 structured success 现在也有独立 unit seam 锁住了。

## [2026-05-17 15:30:18] [Session ID: codex-20260517-clipboard-restore] 笔记: clipboard 文本投递的恢复策略

### 现象
- `@type-text mode:"clipboard"` 需要临时写入系统剪贴板,再通过 `Cmd+V` 投递文本。
- 旧实现会在投递结束后无条件恢复旧剪贴板内容。

### 风险判断
- 如果人类或其他进程在 rdog 投递期间写入了新的剪贴板内容,无条件恢复会把这份新内容覆盖掉。
- 这不符合“非鼠标语义控制尽量不干扰人类现场”的目标。

### 本轮结论
- clipboard 路径应该使用 `restore-if-unchanged` 策略。
- 只有当前剪贴板仍等于 rdog 临时写入的文本时,才恢复旧剪贴板。
- 如果当前剪贴板已经变化,应跳过恢复,并在 response 中返回:
  - `clipboard_restored:false`
  - `clipboard_restore_skipped_reason:"clipboard-changed"`

### 验证边界
- 本轮用 focused unit test 锁住恢复决策和 response 字段。
- 未跑 live clipboard E2E,原因是用户正在交互,真实剪贴板测试会有额外干扰风险。
