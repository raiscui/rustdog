# 任务计划: 修复 `@window-activate` over Zenoh session bridge 返回路径

## [2026-05-25 09:52:43] [Session ID: omx-1779670884813-rnokx6] [任务计划]: bugfix 启动

### 目标
- 修复 `@window-activate` 通过 Zenoh session bridge 执行时 control 端报 `Zenoh session bridge subscriber 在收到结果前关闭` 的问题, 并用测试和 live smoke 验证返回路径正确。

### 阶段
- [ ] 阶段1: 复现并记录动态现象。
- [ ] 阶段2: 静态阅读 parser / action / session bridge / tests,提出可证伪假设。
- [ ] 阶段3: 增加最小失败测试或诊断实验,确认失败路径。
- [ ] 阶段4: 实施根因修复,保持 window activation 显式语义不变。
- [ ] 阶段5: 运行 focused tests / 编译 / live smoke,记录验证和收尾。

### 现象
- 已观察事实: 上轮 `@window-activate#4:{window_id:"pid:8231/window:0"}` 单独执行和与 `@screenshot` 同 session 执行时,control 端报 `Zenoh session bridge subscriber 在收到结果前关闭`。
- 对照事实: 同一临时 daemon 下 `@ping`、`@capabilities`、`@observe`、`@screenshot`、`@click` 均可用。

### 当前假设
- 主假设: `@window-activate` 的 action outcome 没有被正确转换/发送到 `to-control`,导致 client session bridge 等不到 final result。
- 最强备选解释: action 已回传,但 client 侧对某类 frame / response 格式解析或终止条件判断错误,把正常结束误判成 subscriber closed。
- 推翻主假设的证据: daemon 日志或测试证明 `@window-activate` 已经发送完整 final `@response`,但 client 未识别。

### 验证计划
- 用当前二进制重新复现一次,同时保留 daemon 输出。
- 阅读 `src/control_protocol*`、`src/control_actions*`、`src/control_core*`、`src/zenoh_control*` 中 window activate 和 outcome dispatch 路径。
- 先写一个 focused 测试或最小诊断,验证 window command 通过 session bridge 能产生 final response。

### 遇到错误
- `omx explore` 首次调用遗漏 `--prompt`,返回 `Unknown argument`; 下一步改用正确命令。

### 状态
**目前在阶段1** - 准备重新复现,并用正确的 `omx explore --prompt` 做代码路径定位。

## [2026-05-25 09:55:31] [Session ID: omx-1779670884813-rnokx6] [行动]: 启动临时 daemon 并复现 `@window-activate`

### 目的
- 重新生成当前会话的动态证据,避免只依赖上一轮 live smoke 记忆。

### 操作计划
- 启动 `./target/debug/rdog daemon --transport zenoh --name mac.lab --namespace lab`。
- 发送 `@window-find` 获取真实当前 window id。
- 发送 `@window-activate` 复现 session bridge 返回路径问题。
- 观察 daemon 输出,判断 action 是否执行到 daemon 端。

### 状态
**阶段1进行中** - 准备启动临时 daemon。

## [2026-05-25 10:13:22] [Session ID: omx-1779670884813-rnokx6] [阶段推进]: 根因证据已收敛,准备写回归测试

### 已完成
- [x] 阶段1: 复现并记录动态现象。
- [x] 阶段2: 静态阅读 parser / action / session bridge / tests,提出可证伪假设。

### 结论
- 当前根因判断具备动态证据和静态证据:
  - 动态: `@window-activate` 和慢 `@script` 都在约 3.04 秒报 subscriber closed。
  - 静态: `execute_remote_request()` 把 `recv_timeout()` 的 `Ok(None)` 当成 subscriber closed。
- 备选解释“window backend 没有回传”被慢 `@script` 同样失败推翻。

### 下一步
- 增加 `tests/zenoh_router_client.rs` focused 回归测试,要求 Zenoh session-channel 能等待超过 3 秒的普通 response。
- 修改 client session bridge 等待逻辑。

### 状态
**进入阶段3** - 先添加失败测试,再实施修复。

## [2026-05-25 10:18:30] [Session ID: omx-1779670884813-rnokx6] [行动]: 接手收尾验证

### 当前读取结果
- 已沿用实际存在的支线上下文 `__window_activate_fix`,不再新建 `__window_activate_bridge`。
- 当前代码改动位于 `src/zenoh_control/client_pty.rs`,测试改动位于 `tests/zenoh_router_client.rs`。
- 修复方向与 notes 中的已验证结论一致: `recv_timeout()` 的 `Ok(None)` 只能表示等待 tick 超时,不能作为 subscriber closed。

### 操作计划
- 运行格式化检查,确认代码风格无漂移。
- 运行 focused 回归测试 `control_should_wait_for_slow_session_channel_response`。
- 运行相关 Zenoh window/session 测试检索或已有测试,确认没有遗漏直接针对 `@window-activate` 的测试文件。
- 如验证通过,追加 `ERRORFIX__window_activate_fix.md` 和 `WORKLOG__window_activate_fix.md`,并更新本计划为完成。

### 状态
**进入阶段5** - 进行格式化、focused test 和 diff 审查。

## [2026-05-25 10:22:40] [Session ID: omx-1779670884813-rnokx6] [行动]: 追加轻量 live smoke

### 原因
- 新增回归测试已经覆盖慢普通 response 的 Zenoh session bridge 返回路径。
- 用户点名的是 `@window-activate`,因此再用本机临时 daemon 做一次真实 `@window-find -> @window-activate` smoke,只验证返回链路,不做破坏性 UI 操作。

### 操作计划
- 先 `cargo build --quiet` 确保 `./target/debug/rdog` 是最新代码。
- 启动唯一 target name 的临时 Zenoh daemon。
- 发送 `@window-find` 找 Chrome 窗口,再用返回的 `window_id` 发送 `@window-activate`。
- 如果 live smoke 因桌面环境或权限失败,记录为环境限制,不把它混同为当前代码回归。

### 状态
**阶段5继续** - 在 focused test 通过后追加真实 GUI 返回路径验证。

## [2026-05-25 10:25:20] [Session ID: omx-1779670884813-rnokx6] [完成]: 修复和验证完成

### 阶段完成情况
- [x] 阶段1: 复现并记录动态现象。
- [x] 阶段2: 静态阅读 parser / action / session bridge / tests,提出可证伪假设。
- [x] 阶段3: 增加最小失败测试或诊断实验,确认失败路径。
- [x] 阶段4: 实施根因修复,保持 window activation 显式语义不变。
- [x] 阶段5: 运行 focused tests / 编译 / live smoke,记录验证和收尾。

### 已验证结论
- `@window-activate` 失败不是 window backend 没有 response。
- 真实根因是 client session bridge 把 `recv_timeout()` 的 `Ok(None)` 当成 subscriber closed。
- 修复后,真实 `@window-activate` over Zenoh session bridge 在 4.241s 后返回 `status:"ok"`。

### 验证命令
- `cargo fmt -- --check`
- `cargo nextest run --package rustdog --test zenoh_router_client control_should_wait_for_slow_session_channel_response`
- `cargo check --quiet`
- `cargo test --package rustdog --test control_window_e2e --no-run`
- `git diff --check`
- live smoke: `@window-find#1` -> `@window-activate#2` over `mac.windowfix.lab`

### 状态
**本支线任务已完成** - 等待最终交付说明。

## [2026-05-25 10:34:00] [Session ID: omx-1779670884813-rnokx6] [行动]: 按建议做提交前整理

### 目的
- 用户要求“按建议”,对应上一轮建议的提交前整理。
- 本轮只审查和处理本次修复相关文件,避免把工作区已有杂项改动混入同一个 commit。

### 操作计划
- 查看 `src/zenoh_control/client_pty.rs` 和 `tests/zenoh_router_client.rs` 的 diff。
- 查看全局 `git status --short`,区分本次修复文件和已有无关改动。
- 只 stage 本次修复相关代码、测试和 `__window_activate_fix` 支线记录文件。
- 再运行一次 staged diff 检查,如果范围正确则做本地 commit,不 push。

### 状态
**提交前整理中** - 先审查 diff 和待提交范围。

## [2026-05-25 10:38:00] [Session ID: omx-1779670884813-rnokx6] [错误]: commit message 被 Lore hook 拦截

### 现象
- `git commit -m "fix: wait for slow Zenoh session responses"` 被 PreToolUse hook 拦截。
- 错误提示: `git commit is blocked until the inline commit message satisfies the Lore format and includes the required OmX co-author trailer`。

### 处理计划
- 不绕过 hook。
- 先查找本地 Lore / OmX commit message 规则。
- 按规则重新提交。

### 状态
**提交前整理继续** - staged 范围仍保留,下一步修正 commit message。

## [2026-05-25 10:42:00] [Session ID: omx-1779670884813-rnokx6] [完成]: 本地 commit 已完成

### 提交结果
- 本地 commit 已创建: `70d636d`。
- 提交范围只包含本次修复相关的代码、测试和 `__window_activate_fix` 支线记录。
- 未 push。

### 已排除范围
- 未 stage / 未提交既有工作区改动: `AGENTS.md`、`EXPERIENCE.md`、默认六文件、`.DS_Store`、`agent_desktop_review` 支线文件等。

### 状态
**提交前整理已完成** - 准备做最终状态检查。
