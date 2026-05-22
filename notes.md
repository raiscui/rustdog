## [2026-05-05 12:45:02] [Session ID: codex-20260505-zenoh-bare-shell] 笔记: 续档和当前任务事实底座

## 来源

### 来源1: 默认六文件续档前摘要

- `task_plan.md`、`notes.md`、`WORKLOG.md` 均已超过 1000 行,本轮续档到:
  - `archive/default_history/2026-05-05_pre_zenoh_bare_shell/task_plan_2026-05-05_pre_zenoh_bare_shell.md`
  - `archive/default_history/2026-05-05_pre_zenoh_bare_shell/notes_2026-05-05_pre_zenoh_bare_shell.md`
  - `archive/default_history/2026-05-05_pre_zenoh_bare_shell/WORKLOG_2026-05-05_pre_zenoh_bare_shell.md`
- 旧默认组里最相关的可复用事实:
  - line-control 长期保留显式协议请求与裸 shell 行双轨。
  - Zenoh v1 当时为了收窄 router/client + serial 迁移范围,明确拒绝裸 shell 行。
  - 现在 Zenoh 已经有 query bootstrap + session bridge,不再只是单条 query/reply。

### 来源2: 已完成支线 `__control_zenoh_default`

- 支线已归档到 `archive/branch_contexts/control_zenoh_default/`。
- 该支线已完成:
  - `rcat control mac.lab` 推断为 Zenoh target-name。
  - 旧 TCP 位置参数入口继续保留。
  - TCP / WebSocket control 的 JSON-agent raw response 兼容已恢复。

### 来源3: 当前代码事实

- `src/control_core.rs::parse_and_execute_explicit_control_line()` 当前会拒绝 `LiteralShellLine`。
- `src/zenoh_control.rs::execute_remote_request()` 当前在 client 侧用 `!line.starts_with('@')` 拒绝裸 shell 行。
- TCP / WebSocket control receiver 已经在 `src/shell.rs` 里支持 `LiteralShellLine` 并调用远端 shell 执行。

## 综合发现

### 当前实现方向

- 新增或改造一个 control core 入口,让它同时支持:
  - 显式控制请求
  - 裸 shell 行
- 保留旧的 explicit-only 入口给确实只允许显式请求的调用方使用,或者改名后只在需要的地方使用。
- Zenoh direct query 和 session bridge 都应切到支持裸 shell 的入口。

### 文档同步方向

- `specs/zenoh-control-plane-plan.md`、`README.md`、`cmd.md` 里旧的 "no bare shell lines over Zenoh" 口径需要更新。
- `specs/control-line-protocol.md` 的双轨边界仍然有效,不用改成 request-id 模型。

## [2026-05-05 13:12:00] [Session ID: codex-20260505-zenoh-bare-shell] 笔记: 实现与验证结果

## 来源

### 来源1: 代码改动

- `src/control_core.rs`
  - 新增 `execute_literal_shell_line()`
  - 新增 `parse_and_execute_control_line()`
  - 显式请求和裸 shell 行现在共用一个 transport 无关执行入口
- `src/shell.rs`
  - TCP / WebSocket control receiver 改为复用 `parse_and_execute_control_line()`
- `src/zenoh_control.rs`
  - direct query 与 session bridge 改为复用 `parse_and_execute_control_line()`
  - client 侧移除非 `@` 行拦截
- `tests/zenoh_router_client.rs`
  - 旧的"Zenoh 拒绝裸 shell"测试改成"Zenoh 执行裸 shell"
  - session bridge 增加裸 shell line 回归
  - `@paste` 测试改成确保不再被 profile gate 拒绝

### 来源2: 文档同步

- `README.md` / `cmd.md`
  - Zenoh 支持列表加入 bare shell lines 和 `@paste`
  - 不支持列表保留 interactive shell / PTY / cwd 状态保持 / streaming data plane
- `specs/zenoh-control-plane-plan.md`
  - canonical 行为从 explicit-only 更新为 line-control plane
- `specs/zenoh-sdk-integration-playbook.md`
  - 对接方现在可以发送裸 shell line 和 `@paste`
  - 仍不能假设 bare shell line 保留 cwd / PTY 状态
- `specs/zenoh-sdk-agent-prompts.md` / `specs/zenoh-unity-querier-wrapper-design.md`
  - 给智能体的实现提示同步到当前能力面

## 验证

- `cargo fmt --all`: 通过
- `cargo test --bin rcat control_core::tests:: -- --nocapture`: 8 passed
- `cargo test --test zenoh_router_client control_should_execute_literal_shell_line_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client external_peer_should_send_control_request_via_zenoh_to_daemon_channel -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_route_paste_request_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test control_lanes daemon_control_lane_should_execute_script_via_rcat_control -- --exact --nocapture`: 通过
- `cargo check --quiet`: 通过
- `cargo nextest run --test zenoh_router_client -E 'test(control_should_execute_literal_shell_line_in_zenoh_profile) or test(external_peer_should_send_control_request_via_zenoh_to_daemon_channel) or test(control_should_route_paste_request_in_zenoh_profile)'`: 3 passed
- `cargo test --bin rcat`: 108 passed
- `cargo build --quiet`: 通过
- `cargo test --test zenoh_router_client control_should_use_single_positional_name_as_zenoh_target -- --exact --nocapture`: 通过

## 综合发现

- Zenoh 现在和 TCP / WebSocket control 一样接受裸 shell 行。
- 裸 shell 行仍然是顺序流命令,不是 interactive shell。
- `@paste` 现在不再被 Zenoh profile gate 拒绝。真实执行能否成功仍取决于系统输入权限。

## [2026-05-05 16:30:53] [Session ID: codex-20260505-human-response-display] 笔记: TTY 下 `@response` 人类可读显示

## 来源

### 来源1: 用户现场输出

- `rcat control mac.lab` 已能直接执行 `ls` / `cat Cargo.toml`。
- 现场问题变成可读性:
  - 远端返回仍显示为 `@response "...\n..."`
  - `\n` 以 JSON 转义文本出现,不是真实换行
  - 这适合程序 stdio,不适合人类直接交互

### 来源2: 当前代码输出点

- `src/shell.rs::receive_control_result_frames()` 负责 TCP / WebSocket control client 的结果显示。
- `src/zenoh_control.rs::handle_reply_payload()` 负责 Zenoh control client 的结果显示。
- 两处原来都直接 `writeln!(stdout, "{response}")`,所以会把 wire protocol 原样展示给 TTY 用户。

## 综合发现

### 正确边界

- 不应修改 daemon 返回格式,否则会破坏 SDK / pipe / redirect / 自动化解析。
- 本次只应修改本地 client display layer。
- 只有 stdin 和 stdout 都是 TTY 时启用人类可读显示。
- 成功字符串响应可以安全解码为正文。
- 数字响应可以去掉 `@response` 前缀显示。
- 错误对象、复杂对象、request-id 对象需要保留原始协议形态,避免隐藏诊断信息和 correlation id。

## 验证

- `cargo fmt --all`: 通过
- `cargo test --bin rcat control_display::tests:: -- --nocapture`: 6 passed
- `cargo test --test control_tty control_cli_should_treat_arrow_keys_as_local_cursor_motion_in_tty -- --exact --nocapture`: 通过
- `cargo test --test control_lanes daemon_control_lane_should_execute_script_via_rcat_control -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_execute_literal_shell_line_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_use_single_positional_name_as_zenoh_target -- --exact --nocapture`: 通过
- `cargo check --quiet`: 通过
- `cargo test --bin rcat`: 114 passed
- `cargo build --quiet`: 通过
## [2026-05-05 18:31:14] [Session ID: codex-20260505-pty-implementation] 笔记: PTY WIP 对账

## 来源

### 来源1: 当前 WIP 代码

- 文件:
  - `src/pty_control.rs`
  - `src/shell.rs`
  - `src/zenoh_control.rs`
  - `src/control_core.rs`
- 要点:
  - TCP / WebSocket 已经能在收到 `@pty` 时切入 `run_pty_server_loop`。
  - `src/zenoh_control.rs` 的 daemon session bridge 仍然只是调用 `parse_and_execute_control_line()`,所以 `@pty` 会被当成普通 action,不会真的进入 PTY。
  - `--pty-close` 已经有 CLI sugar,但如果它通过新连接发给 daemon,当前没有全局活动 PTY session 注册表,所以无法关闭旧 session。

## 综合发现

### 必补项

- Zenoh session channel 必须检测 `@pty`,并把同一条 `to-daemon` subscriber 交给 PTY loop。
- PTY session 需要一个按 `session_id` 索引的全局 close handle。这样 `@pty-close:{session_id:"..."}` 才能作为真正 out-of-band control request 工作。
- 普通 line-control 收到 PTY frame 时应该报协议错误,不能默默吞掉,否则 transport 混线会难排查。

## [2026-05-06 22:06:53] [Session ID: 019ded13-aa64-7043-801e-294d098c48b2] 笔记: 本轮文档收口前的残留点

## 来源

### 来源1: 现有规格文档

- `specs/control-line-protocol.md` 已经把 `@pty` 的 canonical 示例切到 `cmd + args`,但仍保留 legacy `argv` 兼容说明。
- `specs/pty-control-plan.md` 里的 detach / attach 段落还留着“本轮先写入规格,暂不要求代码完全实现”“本轮只实现 attached PTY 的 strict terminal semantics”这类旧口径。

### 来源2: 当前仓库代码

- `src/control_protocol.rs` 已经把 `PtyOpenRequest` 收敛成 `cmd + args`。
- `src/pty_control.rs`、`src/zenoh_control.rs`、`tests/control_pty.rs` 和 `tests/zenoh_router_client.rs` 里,detach / attach 已经不是纯预留,而是有实际实现和回归测试。

## 综合发现

- 这轮 stage4 的核心不是再补协议,而是把规格文档从“历史预留”改成“现状描述”。
- 最容易出 drift 的地方是 `specs/pty-control-plan.md` 的策略段,因为它还在讲未实现状态,但代码已经进入 detach / attach 生命周期了。
- `README.md` 和 `cmd.md` 目前已经更接近 canonical 口径,所以优先级更低,先修规格文档就够了。

## [2026-05-07 00:14:03] [Session ID: 019ded13-aa64-7043-801e-294d098c48b2] 笔记: Zenoh PTY bridge timeout 根因

## 来源

### 来源1: 失败测试

- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture` 首次失败。
- daemon 日志显示:
  - `Zenoh PTY bridge forwarding frame ... @pty-ready`
  - `PTY output produced ... bytes=14`
- client 端却报:
  - `Zenoh PTY subscriber 在收到 terminal lifecycle frame 前关闭`

### 来源2: Zenoh 1.8.0 本地依赖源码

- `zenoh-1.8.0/src/api/handlers/fifo.rs::recv_timeout()` 注释明确说明:
  - timeout expired 时返回 `None`
- 也就是说 `Ok(None)` 是 timeout,不是 subscriber 关闭。

## 综合发现

- 根因是 `src/zenoh_control.rs` 的 PTY bridge 和 client PTY loops 把 `Ok(None)` 当成 closed。
- active PTY 的轮询间隔只有 25ms,所以 bridge 很容易在没有 stdin/control sample 的瞬间退出。
- 正确语义是:
  - active PTY 下 `Ok(None)` 继续轮询
  - 没有 active PTY 的 daemon session bridge 可以把 idle timeout 作为 session bridge 回收条件
  - terminal success / close 仍然只能由 `@pty-exit` 或 `@pty-closed` 判定

## [2026-05-07 01:08:33] [Session ID: codex-20260507-continuous-learning] 笔记: 六文件摘要与沉淀决策

## 六文件摘要

- 涉及的上下文集:
  - 默认六文件。
  - 本轮未发现新的 `__suffix` 支线六文件。
- 任务目标:
  - 完成 Zenoh 裸 shell、TTY 下人类可读 `@response`、远程 PTY、strict terminal lifecycle、detach / attach、`mac.lab` live smoke 和 commit 收尾。
- 关键决定:
  - `@pty` 是协议层单一真相源,CLI `--pty` 只是语法糖。
  - PTY 输入流保持字节透明,不塞 in-band escape。
  - `@pty-exit` / `@pty-closed` 才是 terminal completion。
  - `.omx/*` 运行态文件不进入 commit。
- 关键发现:
  - Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 是 timeout,不是 subscriber closed。
  - attach / detach 不能只切输出路由,terminal frame 也必须跟随当前 attached client。
  - PTY frame schema 更新后,live smoke 必须确保 daemon 是当前新二进制。
  - Codex/OMX commit message 需要固定 `Co-authored-by: OmX <omx@oh-my-codex.dev>` trailer。
- 实际变更:
  - 上一轮 commit 已完成: `8f0c0d1e1da23b941fed7f5c689348dfd292e9fa`。
  - 本轮持续学习续档了超 1000 行的 `task_plan.md` 与 `ERRORFIX.md`。
  - 新增项目级 skill: `.codex/skills/self-learning.zenoh-fifo-recv-timeout-timeout-not-closed/SKILL.md`。
  - 更新 `EXPERIENCE.md`、`AGENTS.md`、`cmd.md`、`EPIPHANY_LOG.md`。
- 暂缓事项 / 后续方向:
  - `LATER_PLANS.md` 中原 `@pty` 事项已有完成清理记录。
  - Windows live PTY smoke 仍未验证,后续如果有 Windows 主机应补。
- 错误与根因:
  - `recv_timeout()` timeout 被误判成 closed。
  - live smoke 复用旧 daemon 导致 `@pty-exit` 缺 `reason`。
  - commit 首次被 OMX hook 拦截,原因是缺固定 co-author trailer。
- 重大风险 / 重要规律:
  - PTY 生命周期不能用 transport close 推断。
  - 后续任何 Zenoh session frame 短轮询都要先确认 `recv_timeout()` 语义。
- 可复用点候选:
  - Zenoh FIFO timeout 处理 skill。
  - PTY lifecycle 项目经验。
  - Codex/OMX commit hook 与 `.omx` 噪音处理经验。
- 最适合写到哪里:
  - 项目经验写入 `EXPERIENCE.md`。
  - Zenoh FIFO timeout 提取为项目级 skill。
  - 新增长期文件和 manifest 写入 `AGENTS.md` 索引。
- 需要同步的文档:
  - `cmd.md` 中 `@pty-exit` 示例缺 `reason`,已修。
  - `specs/pty-control-plan.md` 已是较新的 strict lifecycle 口径,无需再改。
- 是否提取 / 更新 skill:
  - 是。新增 `self-learning.zenoh-fifo-recv-timeout-timeout-not-closed`。

## [2026-05-07 19:25:14] [Session ID: codex-20260507-zenoh-pty-ssh-input] 笔记: Zenoh PTY stdin 到达 daemon 后未写入远端 PTY

## 来源

### 来源1: 用户现场日志

- daemon 已出现 `Zenoh PTY open received on session bridge`。
- daemon 已出现 `Zenoh PTY session attached to bridge`。
- daemon 已连续输出 `PTY output produced`,说明远端 PTY 程序已经启动并在产出输出。

### 来源2: 新增真实 TTY 回归测试

- `tests/zenoh_router_client.rs::control_should_forward_tty_input_after_zenoh_pty_output_goes_idle` 用 `script` 包住 `rcat control`。
- 测试先通过 TTY 输入 `@pty:{cmd:"/bin/sh",args:[...],cols:80,rows:24}`。
- 远端命令输出 `READY` 后等待 6 个原始字节。
- 本地继续写入 `hello\r`,断言远端 `od` 输出包含 `68 65 6c 6c 6f 0d`。

### 来源3: debug 证据

- client 端出现 `Zenoh PTY client stdin produced bytes ... bytes=6`。
- client 端出现 `Zenoh PTY client stdin frame published ... bytes=6`。
- daemon bridge 收到 `@pty-stdin {"session_id":"...","encoding":"base64","data":"aGVsbG8N"}`。
- 修复前没有出现 `Zenoh PTY bridge forwarding stdin frame` 或 `PTY stdin received`。

## 综合发现

- 问题不在远端 PTY 启动,也不在 client 读取本地 TTY。
- 真正断点在 daemon active PTY session bridge 的分流顺序。
- `parse_pty_open_request()` 原先只用 `starts_with("@pty")` 判断,会把 `@pty-stdin` 误送进 `@pty` open parser。
- parser 报错后 bridge 直接 `continue`,导致真正的 `PtyStdinFrame::parse_wire_message()` 永远执行不到。
- 正确模型是: active PTY 期间先处理 PTY stream frame,再处理 `@pty-close` / `@pty-detach`,最后才把其他 line 按字面输入写入远端 PTY。

## [2026-05-07 20:48:26] [Session ID: codex-20260507-zenoh-pty-tui-latency] 笔记: Zenoh PTY TUI 输入重绘延迟

## 来源

### 来源1: 用户现场现象

- `rcat control mac.lab` 后输入 `@pty:"codex"`。
- codex 在等待最终回答期间,用户继续输入内容时,输入框不即时显示。
- 最终回答到达后,输入内容才像被补显示出来。

### 来源2: 当前代码路径

- interactive `@pty` 走 `run_client_pty_over_session_bridge_tty()`。
- 原实现把 `subscriber.recv_timeout(25ms)`、stdout 写入、`stdin.read()`、`publisher.put()` 放在一个 loop 里。
- daemon session bridge 原实现会在 active PTY 下连续 drain `try_recv_frame()` 到空,远端持续输出时会长时间不回到 `subscriber.recv_timeout()` 读取 `@pty-stdin`。

### 来源3: 新增回归测试

- `tests/zenoh_router_client.rs::control_should_repaint_tui_input_while_zenoh_pty_output_is_busy` 用 `script` 包真实 TTY。
- 远端 helper 一边持续输出 `FRAME...`,一边前台读取 stdin 并输出 `REPAINT:<char>`。
- 修复后,本地输入 `b!` 能在 busy output 期间触发 `REPAINT:b`,不需要等 `DONE`。

## 综合发现

- 这不是 `@pty-stdin` 帧完全不到 daemon 的问题。上一轮已经修过这一层。
- 真实根因是 PTY 泵送公平性不足:
  - client 侧 TTY input publish 和 output receive 被绑在单线程调度里。
  - daemon 侧 active PTY output drain 没有每轮上限,持续 output 会压住 inbound stdin/control 读取。
- raw TTY 下 `stdin.read()` 的 `Ok(0)` 不是 EOF,而是当前没有可读输入。独立线程里必须短 sleep 后继续,否则会热循环或者误退出。
- pipe 模式下 stdin EOF 只表示本地不再输入,不能表示远端 PTY session 完成。远端完成仍只能看 `@pty-exit` / `@pty-closed`。

## [2026-05-10 23:40:50] [Session ID: omx-1778425927914-vdr4af] 笔记: rcat Zenoh daemon/control 作为 code agent 远程协调底座

## 来源

### 来源1: CLI help 与入口源码

- `cargo run --quiet -- --help`: 当前没有 `rcat zenoh daemon` 子命令,顶层子命令是 `control`、`daemon` 等。
- `cargo run --quiet -- daemon --help`: Zenoh daemon 入口是 `rcat daemon --transport zenoh` 或带 `[zenoh].enabled = true` 的 config-driven daemon。
- `cargo run --quiet -- control --help`: Zenoh control 入口支持 `rcat control <target-name>`、`--transport zenoh`、`--target-name`、`--entry-point`、`--pty`、`--pty-close`、`--pty-detach`、`--pty-attach`。
- `src/main.rs:65-160`: 单个非端口位置参数会推断成 Zenoh target-name; 单个端口仍是 TCP shorthand。
- `src/main.rs:440-462`: daemon Zenoh profile 运行 `daemon::run_zenoh_router(...)`。
- `src/main.rs:490-501`: 显式 config path 且 `[zenoh].enabled=true` 时 daemon 默认推断 Zenoh transport。

### 来源2: README/cmd/specs

- `README.md:311-390`: 当前主路径是 `daemon = router`, `control = client`, control 默认 autodiscovery, `--entry-point` 是 fallback, `rcat control <target-name>` 是 Zenoh 短入口。
- `README.md:364-379`: Zenoh profile 支持 `@ping`、`@cmd#id`、bare shell lines、`@key`、`@paste`、`@savefile`、`@screenshot`、`@pty` 系列; 不支持不经 `@pty` 的传统 interactive shell,也不把裸 shell 行变成带 cwd 状态的长期 shell。
- `cmd.md:343-402`: `rcat control` 不是暴露 shell,而是把本地 stdio 变成远端 control lane 文本控制桥。
- `cmd.md:473-498`: `@pty` 是 frame 流,支持 ready/output/exit/closed/detached/attached,并支持 close/detach/attach。
- `specs/zenoh-sdk-integration-playbook.md:47-66`: 面向编程智能体的当前支持能力和非支持能力清单。
- `specs/zenoh-sdk-integration-playbook.md:100-139`: session channel keyexpr 和 bootstrap payload。
- `specs/zenoh-sdk-integration-playbook.md:179-230`: 编程智能体接入时序: join client session, liveliness discovery, session open, to-daemon publish, to-control receive。
- `specs/pty-control-plan.md:15-24`: `@pty` 是协议单一真相源,PTY completion 必须由 lifecycle frame 判断。

### 来源3: 实现与测试证据

- `src/zenoh_identity.rs:129-160`: alive/control/keyinput/session keyexpr 构造。
- `src/zenoh_control.rs:59-151`: daemon 声明 liveliness token、control queryable、keyinput publisher,queryable 主要处理 session open 和兼容请求。
- `src/zenoh_control.rs:568-867`: daemon session bridge 订阅 `to-daemon`,发布 `to-control`,处理 active PTY、stdin、resize、detach/close 和普通控制行。
- `src/zenoh_control.rs:916-951`: client 构建 session bridge,向 control key 发 `__rcat_session_open__:<id>`,再使用 `to-daemon` / `to-control`。
- `src/zenoh_control.rs:1406-1450`: 请求超时时重新 open client session、重新 resolve target、重建 session bridge 后 retry。
- `src/zenoh_control.rs:1513-1564`: daemon 启动前用 liveliness 检查重复 service_name。
- `src/zenoh_control.rs:1552-1586`: 本机 PID guard 防止同机重复 daemon_name 竞争窗口。
- `tests/zenoh_router_client.rs:486-578`: autodiscovery、target shorthand、entry-point fallback、`@cmd#id` 有集成测试。
- `tests/zenoh_router_client.rs:641-773`: bare shell、`@key` 错误回传、keyinput event 有集成测试。
- `tests/zenoh_router_client.rs:776-913`: 外部 Zenoh SDK 风格 client 通过 session channel 发请求、收响应、close/reopen session 的测试。
- `tests/zenoh_router_client.rs:962-1163`: Zenoh `--pty`、PTY 字符串简写、`@pty-resize` 有测试。
- `tests/zenoh_router_client.rs:1356-1540`: detach/reattach/close PTY session 有测试。
- `tests/zenoh_router_client.rs:1664-1750`: daemon 重启后 control session 能 re-resolve 并继续 `@ping`。

## 综合发现

### 已验证事实

- 当前没有 `rcat zenoh daemon` 命令。正确口径是 `rcat daemon --transport zenoh`、`rcat daemon -c <配置>` 启动 Zenoh router profile,再用 `rcat control <target-name>` 连接。
- `rcat control <target-name>` 对 code agent 是一个 stdio-friendly 的远程控制桥: agent 可以往 stdin 写 line-control,从 stdout 读 `@response` / `@savefile` / PTY frame 语义化结果。
- Zenoh 让目标选择从 IP/端口转为 daemon_name + namespace。普通情况下 control 通过 autodiscovery 加入 router,在不稳定网络或跨网段时可用 `--entry-point`。
- 当前架构已经从单 query/reply 演进到 session channel: queryable 主要 bootstrap,后续请求/结果走 `rcat/<ns>/session/<id>/to-daemon` 和 `to-control`。
- 对 code agent 最关键的能力组合是: one-shot 命令、request id、GUI 输入模拟、截图回文件、真实 PTY/TUI、detach/reattach、daemon 重启后 re-resolve。

### 推论

- 相比 SSH,`rcat control` 更像“控制面协议 + 目标发现 + 远端 GUI/桌面副作用执行器”,不是通用登录 shell 替代品。
- 对多主机 code agent 协调,它的独特价值不在“又一个远程命令执行”,而在统一寻址、统一响应格式、跨 TCP/WebSocket/Zenoh 的同一 line-control 语义,以及 PTY + screenshot + key/paste 这类 agent 驱动操作。
- 如果要让编程智能体直接写 Zenoh SDK client,应优先复用 playbook 的 session channel 模型,不要只对 control queryable 发送单条请求后假设它永远是单 query -> 单 reply。

### 未确认/边界

- 远程跨互联网 NAT 场景不是当前证据里的默认完成能力。当前文档更明确的是局域网 autodiscovery 和 entry-point fallback; 真正公网穿透/VPN/relay 需要额外网络层。
- `@screenshot` 有实现和 ignored real-backend 测试,但真实屏幕权限仍受系统权限控制。
- `@key` / `@paste` 受系统输入权限影响,不能承诺一定绕过 macOS Accessibility、Windows UIPI 或远端桌面焦点问题。

## [2026-05-11 23:32:08] [Session ID: omx-1778469026342-c6n34v] 笔记: rdog 更名收尾时的持续学习摘要

## 来源

### 来源1: 默认六文件

- `task_plan.md` 超过 1000 行后已续档到 `archive/default_history/2026-05-11_rdog_rename_continuation/task_plan_2026-05-11_rdog_rename_continuation.md`。
- 当前根目录只剩默认六文件,没有额外 `__suffix` 支线上下文集。
- `WORKLOG.md`、`ERRORFIX.md`、`EPIPHANY_LOG.md` 已记录 rdog 更名、macOS Accessibility 权限变化、默认并发 Zenoh stale locator 处理等事实。

### 来源2: 当前验证

- `cargo check --quiet`: 通过。
- `git diff --check`: 通过。
- `./target/debug/rdog --help`: 显示 `Usage: rdog <COMMAND>`。
- 残留旧名扫描显示源码层旧 `rcat` 只保留在 legacy config fallback、legacy Zenoh keyexpr / session sentinel 和兼容测试中。

## 综合发现

### 可复用经验

- 产品更名不是简单全局替换。新默认路径要统一切到新名字,但升级兼容入口必须保留并明确标注。
- CLI 二进制改名会改变系统权限主体。macOS Accessibility 这类权限绑定实际可执行文件路径/身份,测试不能假设旧二进制权限自动迁移给新二进制。
- 向 append-only Markdown 写入含反引号内容时,必须使用 `cat <<'EOF'`,否则 shell 会执行 command substitution,造成记录缺字段甚至误执行命令。

### 沉淀位置

- `EXPERIENCE.md`: 追加更名兼容和二进制权限主体经验。
- `AGENTS.md`: 追加 `archive/manifests/ARCHIVE_MANIFEST__2026-05-11_rdog_rename_continuation.md` 索引。
- `ERRORFIX.md`: 追加本次未加引号 heredoc 错误记录。
## [2026-05-12 17:28:58] [Session ID: codex-native-unknown] 笔记: rdog-control skill 资料依据

## 来源

### 来源1: 当前 CLI help

- 路径: `./target/debug/rdog --help`
- 路径: `./target/debug/rdog control --help`
- 路径: `./target/debug/rdog daemon --help`
- 要点:
  - 当前主二进制是 `rdog`。
  - `rdog control [HOST_OR_TARGET]... [-- COMMAND...]` 支持 `--url`、`--transport`、`--namespace`、`--target-name`、`--entry-point`、`--pty`、`--pty-close`、`--pty-detach`、`--pty-attach`。
  - 单个非端口位置参数会推断为 Zenoh target-name,所以 `rdog control mac.lab` 是现行主路径。
  - `rdog daemon` 支持 `--transport zenoh`、`--namespace`、`--name` 和 `--config`。

### 来源2: 仓库规格文档

- 路径: `specs/code-agent-rdog-control-usage.md`
- 路径: `specs/control-line-protocol.md`
- 路径: `specs/pty-control-plan.md`
- 路径: `specs/zenoh-sdk-integration-playbook.md`
- 路径: `specs/zenoh-screenshot-control-plan.md`
- 路径: `README.md`
- 路径: `cmd.md`
- 要点:
  - `rdog control` 是 stdio 到远端 control lane 的桥,不是 SSH 的同义词。
  - code agent 应优先 `@ping`,再按需求选择 `@cmd#id`、裸 shell、`@key`、`@paste`、`@screenshot` 或 `--pty`。
  - `@response` 是请求结果,不是 `rdog control` 退出信号。
  - 文件型结果使用 `@savefile`。接收端应保存文件,不要把 base64 直接展示给用户。
  - PTY 完成必须看 `@pty-exit` 或 `@pty-closed`; `@pty-detached` / `@pty-attached` 只是所有权变化。
  - 硬件和单片机控制通常通过桥接主机间接完成,比如 bridge host 上的 serial/JTAG/SDK/vendor CLI。

## 综合发现

### skill 结构

- 主 `SKILL.md` 只放触发条件、最小决策流和安全边界。
- 详细命令放 `references/control-workflow.md`。
- 协议和响应解析放 `references/protocol.md`。
- Zenoh、硬件桥接和单片机边界放 `references/zenoh-hardware.md`。

### 验证结论

- `quick_validate.py` 输出 `Skill is valid!`。
- 当前二进制 help 中确认存在 skill 引用的关键 flag。
- skill 内容扫描未发现 `TODO`、旧 `rcat` 主路径、`zenoh-peer` 或 `target/debug/rcat`。
## [2026-05-13 14:06:58] [Session ID: codex-native-unknown] 笔记: `@screenshot` 只有桌面没有窗口

## 来源

### 来源1: `src/screenshot.rs`

- `execute_screenshot_request()` 直接调用 `capture_primary_display_image()`。
- macOS 路径先执行 `capture_with_sck_rs()`,失败后执行 `capture_with_xcap()`。
- `capture_with_sck_rs()` 只调用 `sck_rs::Monitor::primary().capture_image()`,没有传入窗口排除参数。
- `capture_with_xcap()` 只找 primary monitor 并调用 `monitor.capture_image()`。
- 结论: 当前仓库实现没有主动过滤窗口,也没有只截桌面的业务开关。

### 来源2: `sck-rs` 本地依赖源码

- `Monitor::capture_image()` 调用 `capture_monitor_sync(..., &[])`。
- `sck-rs` 注释说明空的 excluded window list 表示 capture everything。
- 结论: 如果 SCK 主路径成功,从代码意图看应该包含屏幕上的窗口。

### 来源3: `xcap` 本地依赖源码

- macOS monitor capture 最终走 `CGWindowListCreateImage(cg_rect, CGWindowListOption::OptionAll, 0, ...)`。
- 这个路径只要拿到 `CGImage` 就会组装 `RgbaImage`,没有额外判断窗口是否真的进入了图像。
- `xcap` 的 window listing 路径有 Screen Recording preflight,但 monitor capture 路径这里没有显式 preflight。
- 结论: 如果系统隐私权限导致窗口内容被裁掉,当前 fallback 有机会把“只有桌面背景”的图当成成功截图。

## 综合发现

- 已验证现象:
  - `@ping` 和 `@screenshot#7` 协议层成功。
  - `@savefile` 落盘 JPEG 成功。
  - 用户观察到内容只有桌面,没有可见窗口。
- 主假设:
  - macOS Screen Recording 权限没有授给实际执行截图的进程身份,或者 `sck-rs` 主路径失败后,`xcap` fallback 返回了被系统隐私裁剪后的桌面图。
- 最强备选解释:
  - daemon 运行在不同 GUI session / Space / 用户上下文中,实际能看到的就是空桌面。
  - 但本轮 daemon 是从当前用户 PTY 临时启动,所以这个备选解释弱于权限/fallback 假阳性。
- 需要修复时的方向:
  - macOS 上先做权限 preflight 或窗口可见性 probe。
  - 如果无法枚举窗口或权限不足,返回 `PermissionDenied`,不要继续保存桌面-only 图并返回成功。

## [2026-05-13 17:45:06] [Session ID: codex-native-unknown] 笔记: 多显示器截图与鼠标坐标方案事实

## 来源

### 来源1: 当前协议和截图实现

- `src/control_protocol.rs:54-60`: `ScreenshotRequest` 当前只有 `quality`。
- `src/control_protocol.rs:686-710`: 当前对象 payload 只允许 `target="display"`、`format="jpeg"`、`display="primary"`。
- `src/control_protocol.rs:1461-1463`: 当前测试明确拒绝 `display="secondary"`。
- `src/screenshot.rs:13-18`: 截图入口只调用 primary capture。
- `src/screenshot.rs:32-56`: 当前一个 screenshot request 只产出一个 JPEG `@savefile` 和一个最终 `@response`。
- `src/screenshot.rs:77-118`: macOS 和非 macOS 当前都以 primary monitor 为主路径。

### 来源2: 当前 `@savefile` 能力

- `src/control_frames.rs:31-40`: `SaveFileFrame` 字段可以承载任意 filename/mime/base64 data,但没有通用 metadata map。
- `src/control_frames.rs:540-555`: `SaveFileFrame` 直接序列化为 `@savefile {...}`。
- `src/shell.rs:604-610`: control client 以循环方式接收 result frames,一个请求返回多个 `@savefile` 在传输模型上可行。

### 来源3: 显示器枚举能力

- `sck-rs::Monitor::all()` 已能提供多显示器的 id、x/y、logical size、scale factor 和 primary 信息。
- `xcap::Monitor::all()` 已能提供 id、name、x/y、width/height、scale factor、primary 和 capture_image。

## 综合发现

- 方案应选择“完整虚拟桌面大图 + manifest”作为默认主路径。
- manifest 应成为截图坐标和未来鼠标坐标之间的单一真相源。
- 默认 composite 建议使用 `os-logical` 分辨率,让 `image_x + virtual_bounds.x = os_x` 成立,降低后续点击/拖拽换算复杂度。
- 每屏单独文件可以作为 future debug 模式,不应成为默认 code-agent 视觉入口。

## [2026-05-13 19:57:11] [Session ID: omx-1778661154642-agn8qc] 验证记录: 多显示器 screenshot bundle 动态证据

### 已验证
- `cargo test --package rustdog --bin rdog`: 140 passed。
- `cargo test --tests --no-run`: integration tests 全部预编译通过,无 warning。
- `git diff --check`: 在上一轮批处理通过。
- `beautiful-mermaid-rs --ascii`: `specs/rdog-multi-display-screenshot-coordinate-plan.md` 两个 Mermaid block 通过。
- 手写真实 TCP control smoke:
  - stdout 出现两个 `saved file:`。
  - 保存 `screenshot-...-virtual-desktop.jpg`。
  - 保存 `screenshot-...-manifest.json`。
  - final response 为 `kind:"screenshot-bundle"`。
  - manifest `display_count = 2`。
  - manifest `image_size = {"width":3390,"height":1080}`。
- `cargo test --package rustdog --test zenoh_router_client -- control_should_execute_screenshot_and_save_file_in_zenoh_profile --exact --ignored --nocapture`: passed。
- `cargo test --package rustdog --test control_lanes -- daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control --exact --ignored --nocapture`: passed。
- `cargo test --package rustdog --test control_websocket -- control_cli_should_execute_screenshot_and_save_file_over_websocket --exact --ignored --nocapture`: passed。

### 结论
- 默认 `@screenshot#id` 已在真实双显示器环境返回 composite JPEG + manifest JSON。
- TCP、WebSocket、Zenoh control 路径均能保存两个 `@savefile` 并收到 `screenshot-bundle` final response。

## [2026-05-13 20:13:46] [Session ID: omx-1778661154642-agn8qc] 笔记: 多显示器 screenshot bundle post-deslop 验证

## 来源

### 来源1: post-deslop 回归命令

- 命令: cargo fmt
- 结果: 退出码 0。
- 命令: cargo test --package rustdog --bin rdog -- screenshot::tests --nocapture
- 结果: 11 passed, 0 failed。
- 命令: cargo test --package rustdog --bin rdog
- 结果: 142 passed, 0 failed。
- 命令: cargo test --tests --no-run
- 结果: integration tests 全部编译为可执行测试目标,未出现 warning/error。
- 命令: git diff --check
- 结果: 退出码 0,无输出。
- 命令: cargo test --package rustdog --test zenoh_router_client -- control_should_execute_screenshot_and_save_file_in_zenoh_profile --exact --ignored --nocapture
- 结果: 1 passed, 0 failed。
- 命令: python3 /Users/cuiluming/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control
- 结果: Skill is valid!

## 综合发现

- Architect 子智能体结论为 APPROVE,架构状态 WATCH。
- WATCH 点是内部 API 构造非法 ScreenshotRequest 时可能先 capture 再校验。
- 本轮已把 primary/composite request 校验前置到 capture closure 之前,并新增两个测试证明非法 request 不会触发 capture。
- 多显示器默认 screenshot bundle、manifest 坐标契约、primary 兼容入口和 Zenoh ignored smoke 仍然通过验证。

## [2026-05-13 22:50:12] [Session ID: codex-app-2026-05-13-mouse-control-plan] 笔记: mouse control 执行计划生成

## 来源

### 来源1: 源规格

- 文件: `specs/rdog-mouse-control-coordinate-plan.md`
- 要点:
  - 鼠标控制必须复用 `@screenshot` manifest 的 `os-logical` 坐标语义。
  - 默认命令面覆盖 `@mouse-move`、`@mouse-button`、`@click`、`@drag`、`@wheel`。
  - image pixel 与 OS logical 的换算公式是 `os_x = image_x + virtual_bounds.x` 和 `os_y = image_y + virtual_bounds.y`。

### 来源2: 当前源码入口

- 文件: `src/control_protocol.rs`
- 要点:
  - 当前 `ControlCommand` 还没有鼠标变体。
  - parser dispatch 已经按 command kind 集中分发。
  - `@key` 对象 payload 已有字段去重、默认值和未知字段拒绝模式,鼠标 parser 应沿用。

### 来源3: 当前执行层

- 文件: `src/control_actions.rs`
- 要点:
  - `SystemControlActionExecutor` 已经负责 `@key` / `@paste` / shell action。
  - key 路径已经把 plan builder 和 enigo performer 拆开,鼠标动作适合复用这一思路。
  - 权限文案当前还偏向 `@key` / `@paste`,后续要扩展成通用输入模拟权限说明。

## 综合发现

- 已生成 `.omx/plans/rdog-mouse-control-implementation-plan.md`。
- 计划推荐 Option A: 显式 `ControlCommand` 变体 + 纯鼠标计划层 + enigo performer + 平台能力保护。
- Option B 只做 `@click` / `@wheel` 被拒绝,因为不满足用户明确要求的 move 和 button press/release。
- `@drag` 是最高风险组合命令,实现时必须覆盖 press 后失败时尝试 release 的动态测试。
- 当前计划文件本身没有 Mermaid 块;源规格中的两个 Mermaid 块已用 `beautiful-mermaid-rs --ascii` 验证通过。

## [2026-05-18 10:47:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: rdog-control skill 项目内迁移

## 来源

### 来源1: 全局 skill 目录

- 路径: `/Users/cuiluming/.codex/skills/rdog-control`
- 要点:
  - 全局目录包含 `SKILL.md`、`references/`、`agents/openai.yaml` 和 `.vscode/`。
  - 本轮只把 skill 实质内容复制进仓库,不复制 `.vscode` 这类本机编辑器配置。

### 来源2: 项目长期索引

- 路径: `AGENTS.md`
- 要点:
  - 原索引里存在用户级绝对路径 `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`。
  - 本轮改为项目内相对路径 `.codex/skills/rdog-control/SKILL.md`,避免后续项目维护和全局 skill 维护分裂。

## 综合发现

### 迁移结论

- 项目内维护落点确定为 `.codex/skills/rdog-control`。
- 这个位置和仓库现有知识索引中 `.codex/skills/...` 的表达一致。
- 迁移后通过 `quick_validate.py` 验证 skill 结构有效。
- 通过 `diff -ru --exclude='.vscode'` 证明项目内 copy 和全局来源在实质内容上没有差异。

## [2026-05-18 10:57:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: 全局 skill 目录改为连接目录

## 来源

### 来源1: 文件系统替换结果

- 路径: `/Users/cuiluming/.codex/skills/rdog-control`
- 要点:
  - 该路径已从普通目录替换为符号链接。
  - 链接目标是仓库内 `.codex/skills/rdog-control`。
  - 这让全局入口和项目内入口共享同一份 skill 内容。

### 来源2: 验证命令

- 命令: `ls -ld /Users/cuiluming/.codex/skills/rdog-control && readlink ... && realpath ...`
- 命令: `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`
- 命令: `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`

## 综合发现

### 结论

- 全局 skill 现在只是项目内 skill 的连接入口。
- 后续维护只需要改仓库内 `.codex/skills/rdog-control`。
- 旧全局目录已保留到 `/tmp/rdog-control-global-backup-20260518-104751` 作为回退点。

## [2026-05-18 13:00:51] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: autoresearch rustdog 能力演进建议

## 来源

### 来源1: README 和项目经验

- 路径: `README.md`
- 要点:
  - 当前项目定位已经是 remote control plane,不是单纯 reverse shell。
  - `rdog control` 的 agent flow 是写 line-control、收 `@response` / `@savefile` / `@pty-*` frame。
  - README 已列出 Zenoh target-name、PTY、GUI actions、structured responses。

- 路径: `EXPERIENCE.md`
- 要点:
  - line-control 要保持显式协议请求和裸 shell 行分层。
  - Zenoh autodiscovery 是主路径,`--entry-point` 是 fallback。
  - PTY 完成条件必须由 `@pty-exit` / `@pty-closed` lifecycle frame 决定。
  - macOS/Windows 权限要当作一等错误。
  - `rdog-control` skill 已经是长期 agent 使用入口。

### 来源2: 当前源码

- 路径: `src/control_frames.rs`
- 要点:
  - `ControlFrame`、`ControlExecutionOutcome`、`SaveFileFrame` 和 PTY lifecycle frame 已经存在。
  - 这推翻了“下一步先实现 ControlFrame”的旧候选判断。

- 路径: `src/control_core.rs`
- 要点:
  - `execute_explicit_control_request` 已返回 `ControlExecutionOutcome`。
  - `@screenshot` 已直接走 screenshot producer,可返回多 frame outcome。

- 路径: `src/zenoh_control.rs`
- 要点:
  - 仍保留 control queryable。
  - 已有 session bridge,通过 `to-daemon` / `to-control` channel 转发 frame。
  - 当前缺口更像是缺少 transport-agnostic `ControlPeerSession`,不是缺少单个 Zenoh frame helper。

- 路径: `src/screenshot.rs`
- 要点:
  - 默认 composite screenshot 会生成 image savefile、manifest savefile 和 final `screenshot-bundle` response。

- 路径: `src/control_actions.rs`
- 要点:
  - mouse、AX、window、type-text 已接入 `SystemControlActionExecutor`。
  - GUI 原子能力已经比较丰富,下一步重点应是 agent workflow 产品化和验证矩阵。

### 来源3: 规格和测试

- 路径: `specs/bidirectional-control-plane-plan.md`
- 要点:
  - 长期目标是 control 和 daemon 都是 control peers。
  - `@savefile` 应作为普通双向结果/控制指令。

- 路径: `specs/code-agent-rdog-control-usage.md`
- 要点:
  - agent 使用心智是 stdio bridge + Zenoh + daemon + host。
  - 能力矩阵已经覆盖 shell、PTY、screenshot、AX、window、mouse。

- 路径: `tests/zenoh_router_client.rs`
- 要点:
  - 已有 autodiscovery 测试和 session keyexpr 测试。
  - PTY detach/attach/close 在 session channel 上已有覆盖。

## 综合发现

- 正式推荐的第一优先级是完成 `ControlPeerSession` 一等抽象,让 TCP / WebSocket / Zenoh 共享 frame dispatch、request id、savefile receiver 和 terminal lifecycle gate。
- 第二优先级是把 Zenoh queryable 降级为 bootstrap / legacy,富能力默认走 session channel。
- 第三优先级是把 GUI agent 能力从原子命令升级成 `observe -> locate -> act -> verify` recipe 和真实场景回归。
- 第四优先级是做 `@capabilities` / `rdog doctor` 级别的结构化权限与平台诊断。
- 第五优先级是把 SDK 对接文档升级成 conformance surface。
- 第六优先级是结构性减负,因为多个核心文件已经超过项目建议线。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: ralplan Architect 第一轮反馈

## 来源

### 来源1: Architect review

- 结论: `ITERATE`
- 要点:
  - 支持 Option A 的方向,但要求把边界钉得更细。
  - `ControlPeerSession` 不应成为更大的 wrapper。
  - savefile receiver / 落盘策略已经分散在 client adapter 中,不能被粗暴塞进 core。
  - queryable 保留 bootstrap / legacy fallback 会形成真实 tradeoff,但不能直接删掉。
  - PTY close / session close 语义必须先讲清楚。

## 综合发现

- draft 已补 `Boundary Inventory`。
- `ControlPeerSession should own` 已收窄为 frame ordering、request correlation、lifecycle gating、outbound fan-out 和 terminal completion detection。
- `ControlPeerSession should not own` 明确排除 savefile on-disk policy、screenshot backend、PTY process spawn、transport construction 和 permission probing。
- Phase 3 已改成迁移 / 硬化既有 Zenoh session channel。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: ralplan Architect 第二轮反馈

## 来源

### 来源1: Architect review

- 结论: `ITERATE`
- 关键点:
  - `savefile receiver policy` 这个词还会把 core 往回拉,应改成 `savefile routing / persistence policy` 并明确留在 adapter/policy。
  - `@pty-close` 的单一语义已可冻结,需要直接把 `@pty-detach` 和 disconnect / transport lost 的行为写成固定结果。
  - `outbound frame fan-out` 这个说法也偏宽,应收窄为 `ordered outbound frame queue`。

## 综合发现

- draft 已更新为单义边界。
- Phase 1 不再把 savefile policy 放进 core。
- Phase 0 冻结了 PTY close / detach / disconnect / transport lost 的默认动作。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: ralplan Architect 第三轮通过

## 来源

### 来源1: Architect review

- 结论: `APPROVE`
- 仍需保留的实现提醒:
  - `ControlPeerSession` 的 `terminal completion detection` 和 `session close / detach / attach state hooks` 只能是 wire-level gating。
  - PTY process、savefile persistence、transport plumbing 都必须留给 adapter / backend / policy。

## 综合发现

- 计划的架构方向已经通过。
- 下一步进入 Critic,重点检查 testable acceptance criteria、风险缓解和 execution handoff 是否完整。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: ralplan Critic 第一轮反馈

## 来源

### 来源1: Critic review

- 结论: `ITERATE`
- 必须修改:
  - `ControlPeerSession` 不能隐性拥有 PTY lifecycle owner 角色。
  - PTY close 语义必须写清“谁执行进程动作”。
  - TCP screenshot、WebSocket screenshot、Zenoh rich control、PTY lifecycle、capability failure 必须给具体命令或测试名。
  - Observability 必须有测试计划,不能只列日志字段。
  - Acceptance Criteria 必须有可观察信号。

## 综合发现

- draft 已补具体验证命令。
- draft 已补新增测试名:
  - `control_session::tests::should_emit_ordered_frames_without_owning_savefile_persistence`
  - `control_session::tests::should_not_log_savefile_base64_payload`
  - `control_session::tests::should_emit_terminal_completion_only_for_terminal_frames`
  - `zenoh_control::tests::should_distinguish_session_timeout_transport_close_and_terminal_frames`
  - `tests/zenoh_router_client::control_should_reject_rich_frame_over_legacy_queryable_path`
- draft 已把首个结构性减负目标固定为 `src/control_protocol.rs`。

## [2026-05-18 13:11:09] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: post-Critic Architect 通过

## 来源

### 来源1: Architect review

- 结论: `APPROVE`
- 合成建议:
  - `session_id`、`frame_kind`、`request_id` 可以是 core invariant。
  - `transport`、`target_name`、`savefile_path` 应由 adapter / policy 注入或在 adapter-level 测试验证。
  - 不要让 observability 测试把 core 拉回 transport owner。

## 综合发现

- draft 已将该合成建议写入 Observability 执行约束和 Risks。

## [2026-05-18 13:45:05] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: ralplan 最终通过

## 来源

### 来源1: Critic review

- 结论: `APPROVE`
- 非阻塞建议:
  - `control_session::tests::should_log_frame_kind_request_id_and_target_without_payload_body` 中的 `target` 明确成 adapter 注入观测字段。
  - Phase 1 的 parser 表述收紧为“接收已解析请求”或“委派现有 parser”。

## 综合发现

- final plan 已写入 `.omx/plans/ralplan-rustdog-control-peer-session-evolution.md`。
- draft 与 final plan 内容一致,并吸收了 Critic 的非阻塞修订。

## [2026-05-18 14:25:21] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Ralph Phase 0-2 初版实现调查

## 来源

### 来源1: 源码只读调查

- `src/control_frames.rs` 已有 `ControlFrame` / `ControlExecutionOutcome`,但没有 `ControlPeerSession`。
- `src/shell.rs` 已在 TCP / WebSocket receiver 侧逐 frame 写 transport,但逻辑仍是 adapter 本地循环。
- `src/zenoh_control.rs` 已有 session channel 和 `publish_outcome_to_session_channel()`,但 frame dispatch 也仍在 Zenoh adapter 内部。
- `src/pty_control.rs` 已把 PTY terminal completion 固定在 `@pty-exit` / `@pty-closed`,这和 plan 的 session lifecycle gate 一致。

### 来源2: 编译反馈

- 初版 `ControlPeerFrameSink` 使用 `impl<W: Write>` blanket impl。
- 编译报错 `E0119 conflicting implementations`,因为上游 crate 将来可能给 `zenoh::pubsub::Publisher` 实现 `Write`。
- 修正为显式 `LineWriteFrameSink` wrapper,避免 blanket impl 和外部类型冲突。
- 初版测试把 `@savefile` wire 字段写成 `request_id`,实际稳定字段是 `id`。
- 修正测试期望,以真实 `SaveFileFrame::to_wire_message()` 语义为准。

## 综合发现

- 当前最稳的 Phase 0-2 落点是新增薄 `src/control_session.rs`,不要移动 PTY process ownership 或 savefile persistence。
- `ControlPeerSession` 现在只拥有 ordering / dispatch / lifecycle decision / observability summary。
- TCP / WebSocket 和 Zenoh 已开始通过同一个 session core dispatch frame,但 Phase 3 的 Zenoh rich-control 主路径迁移还没有提前展开。

## [2026-05-18 15:13:16] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: screenshot live smoke 与显示器休眠

## 来源

### 来源1: Architect review

- Verdict: `ITERATE`
- 阻断项: Phase 2 缺真实运行态 `@screenshot -> @savefile` smoke 证据。

### 来源2: 直接运行 ignored tests

- TCP smoke 失败:
  - `cargo test --package rustdog --test control_lanes daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control -- --exact --ignored --nocapture`
  - 输出包含 `@response {"id":7,"code":70,"error":"没有可截图的显示器"}`
- WebSocket smoke 失败:
  - `cargo test --package rustdog --test control_websocket control_cli_should_execute_screenshot_and_save_file_over_websocket -- --exact --ignored --nocapture`
  - 输出同样包含 `没有可截图的显示器`

### 来源3: 环境状态

- `system_profiler SPDisplaysDataType` 显示内置屏和外接屏都是 `Display Asleep: Yes`。
- `caffeinate -u -t 5` 后,系统报告仍显示 display asleep。

### 来源4: 最小验证

- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_lanes daemon_control_lane_should_execute_screenshot_and_save_file_via_rdog_control -- --exact --ignored --nocapture`: 通过。
- `caffeinate -d -u -t 30 cargo test --package rustdog --test control_websocket control_cli_should_execute_screenshot_and_save_file_over_websocket -- --exact --ignored --nocapture`: 通过。

## 综合发现

- 当前失败不是 TCP/WebSocket session dispatch 差异造成的,两条路径在显示器休眠时同样拿不到截图显示器。
- live screenshot smoke 需要让 display awake assertion 覆盖整个测试窗口,单独提前执行一次 `caffeinate -u` 不够稳。
- Phase 2 的 live smoke 证据现在已经补齐: TCP 和 WebSocket 都在同一环境处理方式下通过。

## [2026-05-18 16:30:01] [Session ID: codex-phase3-20260518-160435] 笔记: Ralph Phase 3 Zenoh queryable 降级

## 来源

### 来源1: 最小红测

- 新增 `tests/zenoh_router_client.rs::control_should_reject_rich_frame_over_legacy_queryable_path` 后,未改运行时时失败。
- 失败输出证明直接对 Zenoh queryable 发送 `@screenshot#7` 会返回 image `@savefile`、manifest `@savefile` 和 final `screenshot-bundle`。
- 这说明 queryable 当时仍是富能力执行路径之一,不只是 bootstrap / legacy compatibility。

### 来源2: 边界复查

- CLI `rdog control TARGET` 已经通过 `build_client_session_bridge()` 打开 session,再用 `to-daemon` publisher 发送普通 line-control。
- `open_daemon_session_bridge()` 旧实现里普通 line-control outcome 手写循环发送 frame,这次改成复用 `ControlPeerSession::dispatch_outcome_ref()`。
- 直接 queryable payload 和旧 `__rdog_session__:<id>\n...` query payload 都需要拦截 session-only 富命令,否则 queryable 仍有隐藏富能力路径。

### 来源3: 外部 review 尝试

- `omx ask claude --agent-prompt architect ...` 失败: 本机没有 `architect` prompt role。
- `omx ask claude -p ...` 失败: provider 返回 402 insufficient balance。
- `omx ask gemini -p ...` 30 秒无输出,已手动清理进程。
- 因此本轮采用本地静态 review + focused tests 作为降级审查证据。

## 综合发现

- `reject_session_channel_only_legacy_query()` 在执行前拦截 screenshot、PTY lifecycle、mouse、AX、window、type-text 和 `@savefile`。
- `@ping`、`@cmd` 和裸 shell 仍允许走 queryable compatibility。
- 旧 `__rdog_session__` query payload 遇到富命令时只把 code 78 发到 `to-control`,query reply 只返回 ack,不会再执行 screenshot 或发送 `@savefile`。
- Phase 3 的关键不是删除 queryable,而是让它无法继续成为富能力主执行通道。

## [2026-05-18 17:00:55] [Session ID: codex-phase4-20260518-163845] 笔记: Phase 4 capabilities 单一真相源

## 来源

### 来源1: 当前代码入口

- `src/control_protocol.rs` 原本没有 `@capabilities` kind。
- `src/control_core.rs` 已有 `render_structured_success_response()`,可以直接返回 JSON object,不需要发明新的 frame。
- `src/control_actions.rs` 已经把 `PermissionDenied` 和 `Unsupported` 映射到上层 code 语义,对应 code `77` / `78`。

### 来源2: Phase 4 目标

- GUI agent workflow 不能继续靠平台名猜能力。
- 权限诊断必须结构化暴露 macOS Accessibility / Screen Recording、Windows UIPI、Linux backend。
- `rdog doctor` 暂不作为第一入口,后续应复用同一份 capability model。

## 综合发现

- `@capabilities` 适合作为协议层单一真相源,返回 `rdog.capabilities.v1`。
- report 中每个能力都应有 `status`,并且把 `permission_denied` 和 `unsupported` 明确区分。
- GUI agent recipe 固定为 `@capabilities -> observe -> locate -> activate_or_focus -> semantic_action -> verify -> fallback_recipe`。
- 这条命令只产生单条 `@response`,所以可以保留在 Zenoh legacy queryable 的 bootstrap / diagnosis 能力范围内,不必强制走 session channel。
## [2026-05-18 17:55:18] [Session ID: codex-phase5-20260518-173716] 笔记: control_protocol 结构减负

## 综合发现

- 结构性减负第一刀选 `src/control_protocol.rs` 是正确的,因为它同时承载类型、payload parser 和测试,继续加 GUI / capability 命令会扩大修改半径。
- 只把所有 parser 搬到单个 `parsers.rs` 不够,该文件会变成 1286 行的新大文件。
- 最终拆分为父模块、common parser registry、`pty` / `screenshot` / `key` 三个 payload parser 子模块和独立测试模块。
- 旧的 `crate::control_protocol::{normalize_object_field_name, object_inner, parse_quoted_payload, split_object_field, split_object_fields}` 导入路径保持不变。

## 验证结论

- `control_protocol::tests` 14 个用例通过,说明 line-control wire syntax、request id、错误路径和默认值未被拆分改动。
- `rdog` bin、`zenoh_router_client`、`control_lanes`、`control_websocket` 的 no-run 编译通过,说明外层 transport / session 入口没有被 re-export 破坏。
## [2026-05-18 18:12:42] [Session ID: codex-phase5-20260518-173716] 笔记: control_actions 测试拆分

## 综合发现

- `src/control_actions.rs` 的主执行路径本身不算最重,真正撑高行数的是内联测试块。
- 把测试移到 `src/control_actions/tests.rs` 后,主文件回到 802 行,终于落回项目健康线。
- 这类拆分对行为风险很低,因为测试文件仍然通过 `use super::*;` 直接访问同一模块的私有函数。

## 验证结论

- `control_actions::tests` 17 个用例通过。
- `cargo test --package rustdog --bin rdog --no-run`、`cargo test --package rustdog --test zenoh_router_client --no-run`、`cargo test --package rustdog --test control_lanes --no-run`、`cargo test --package rustdog --test control_websocket --no-run` 都通过。
