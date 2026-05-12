## [2026-05-05 13:12:00] [Session ID: codex-20260505-zenoh-bare-shell] 任务名称: 统一 Zenoh control 裸 shell 与 line-control 能力

### 任务内容

- 让 `rcat control mac.lab` 的 Zenoh 路径支持裸 shell 行,例如直接输入 `ls` / `pwd` / `printf READY`。
- 把 Zenoh 的执行入口和 TCP / WebSocket control 对齐,避免同一类 control line 在不同 transport 下行为分裂。
- 顺手统一 `@paste` 的 profile gate 行为: Zenoh 不再提前拒绝 `@paste`,而是交给已有 executor 和系统权限处理。
- 同步 README、`cmd.md` 和 Zenoh 对接规格。

### 完成过程

- 先按六文件规则完成续档:
  - 超过 1000 行的默认 `task_plan.md` / `notes.md` / `WORKLOG.md` 已归档到 `archive/default_history/2026-05-05_pre_zenoh_bare_shell/`
  - 已完成支线 `__control_zenoh_default` 已归档到 `archive/branch_contexts/control_zenoh_default/`
  - 新归档说明写入 `archive/manifests/ARCHIVE_MANIFEST__2026-05-05_zenoh_bare_shell.md`
- 在 `src/control_core.rs` 新增统一入口:
  - `execute_literal_shell_line()`
  - `parse_and_execute_control_line()`
- 在 `src/shell.rs` 中让 TCP / WebSocket control receiver 复用统一入口。
- 在 `src/zenoh_control.rs` 中让 direct query 和 session bridge 复用统一入口,并移除 client 侧非 `@` 行拦截。
- 在 `tests/zenoh_router_client.rs` 中把旧拒绝测试改成成功测试,并补 session bridge 裸 shell 回归。
- 文档同步到:
  - `README.md`
  - `cmd.md`
  - `specs/zenoh-control-plane-plan.md`
  - `specs/zenoh-sdk-integration-playbook.md`
  - `specs/zenoh-sdk-agent-prompts.md`
  - `specs/zenoh-unity-querier-wrapper-design.md`

### 验证

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

### 总结感悟

- 这次正确修法不是给 Zenoh 单独开一个特殊分支。
- 更稳的是把 line-control 的一行输入语义收口到 `control_core`,让各个 transport 共享同一个真相源。
- 裸 shell 行仍然不能和 interactive shell 混淆。它只是"每行一个命令",需要 request id 时仍应使用 `@cmd#id:"..."`。

## [2026-05-05 16:30:53] [Session ID: codex-20260505-human-response-display] 任务名称: TTY 下 `@response` 人类可读显示

### 任务内容

- 改善 `rcat control mac.lab` 人类直接输入时的显示体验。
- 让 `@response "...\n..."` 在 TTY 中显示为真实换行正文。
- 保留 pipe / redirect / 程序 stdio 的原始协议输出,不破坏自动化解析。

### 完成过程

- 新增 `src/control_display.rs`:
  - 定义 `ControlResponseDisplay`
  - 根据 stdin / stdout 是否都是 TTY 选择显示策略
  - 人类模式解码简单字符串响应和数字响应
  - 复杂对象、错误对象、request-id 对象保持原始 `@response {...}`
- 更新 `src/shell.rs`:
  - TCP / WebSocket control client 复用共享显示 helper
- 更新 `src/zenoh_control.rs`:
  - Zenoh control client 复用同一显示 helper
- 更新 `tests/control_tty.rs`:
  - 真实 PTY 下现在要求看到 `TTY_OK`,而不是原始 `@response "TTY_OK"`
- 同步文档:
  - `README.md`
  - `cmd.md`
  - `specs/control-line-protocol.md`

### 验证

- `cargo fmt --all`: 通过
- `cargo test --bin rcat control_display::tests:: -- --nocapture`: 6 passed
- `cargo test --test control_tty control_cli_should_treat_arrow_keys_as_local_cursor_motion_in_tty -- --exact --nocapture`: 通过
- `cargo test --test control_lanes daemon_control_lane_should_execute_script_via_rcat_control -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_execute_literal_shell_line_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_use_single_positional_name_as_zenoh_target -- --exact --nocapture`: 通过
- `cargo check --quiet`: 通过
- `cargo test --bin rcat`: 114 passed
- `cargo build --quiet`: 通过

### 总结感悟

- 这次不应改 `@response` wire protocol,否则会把人类体验问题变成协议兼容问题。
- 更稳的是把 protocol output 和 local display 分层。
- 判断条件必须同时看 stdin 和 stdout。只看 stdin 会误伤重定向输出,只看 stdout 会误伤脚本输入。
## [2026-05-05 18:58:00] [Session ID: codex-20260505-pty-implementation] 任务名称: `@pty` / `rcat control --pty` 远程 PTY 落地

### 任务内容

- 实现 `@pty` / `@pty-close` 协议请求和 PTY frame。
- 增加 `rcat control TARGET --pty -- COMMAND ...` 与 `rcat control TARGET --pty-close SESSION_ID` CLI sugar。
- 打通 TCP、WebSocket、Zenoh session channel 三条 control lane 的 PTY 会话。
- 保持 PTY 输入透明,确保 `@key` / `@script` / `~.` / `Ctrl-C` / `Ctrl-D` 不被本地 control parser 截获。
- 同步 README、`cmd.md`、control-line spec、Zenoh spec、SDK 对接文档和 AGENTS 索引。

### 完成过程

- 在 `src/control_protocol.rs` 中增加 `PtyOpenRequest` / `PtyCloseRequest`,支持 `@pty:{...}` 与 `@pty-close:{...}`。
- 在 `src/control_frames.rs` 中增加 `@pty-ready`、`@pty-output`、`@pty-exit` 和 `@pty-stdin` 的 frame 编解码。
- 新增 `src/pty_control.rs`,负责:
  - CLI payload 渲染
  - 本地 raw terminal guard
  - server-side portable-pty loop
  - PTY active session registry
  - out-of-band close
- 在 TCP / WebSocket control receiver 中检测 `@pty`,并把当前 transport 切入 PTY loop。
- 在 Zenoh daemon session bridge 中检测 `@pty`,并把 `to-daemon` / `to-control` session channel 映射到 PTY frame。
- 让 daemon `inbound.mode = "control"` 每个连接独立线程处理,避免 PTY 长连接阻塞 `--pty-close` 这类 out-of-band control 请求。
- 新增 `tests/control_pty.rs`,覆盖真实 TTY probe、透明输入、`--pty-close`。
- 扩展 `tests/control_websocket.rs` 和 `tests/zenoh_router_client.rs`,分别覆盖 WebSocket PTY 和 Zenoh PTY。

### 验证

- `cargo fmt --all`: 通过
- `cargo check --quiet`: 通过
- `cargo test --test control_pty -- --nocapture`: 3 passed
- `cargo test --test control_websocket -- --nocapture`: 2 passed, 1 ignored
- `cargo test --test zenoh_router_client -- --nocapture`: 16 passed, 1 ignored
- `cargo test --bin rcat`: 121 passed
- `cargo build --quiet`: 通过
- `git diff --check`: 通过
- `beautiful-mermaid-rs --ascii`: PTY / line-control 相关 Mermaid 图已验证

### 总结感悟

- PTY 不应该被塞进 `@response`。它是 session frame 流,不是普通请求返回值。
- `@pty-exit` 必须晚于 output drain,否则快速命令会出现 exit 抢跑并丢输出。
- out-of-band close 不只是一个 parser 问题。daemon control inbound 也必须能在活动长连接期间继续 accept 新控制连接。

## [2026-05-05 20:29:09] [Session ID: codex-20260505-maclab-live-pty-smoke] 任务名称: `mac.lab` 本机实机启动与 PTY smoke

### 任务内容

- 用当前这台 macOS 主机真实启动 `mac.lab` 对应的 Zenoh router daemon。
- 验证 `rcat control mac.lab` short form、裸 shell 行、TTY 人类可读显示和 `--pty` 都能走通。
- 收尾时清理本轮测试 daemon,避免留下新的残留进程。

### 完成过程

- 先检查现有本机 `rcat daemon`:
  - 发现两个 5 月 2 日残留进程,只监听 `127.0.0.1:52878` / `127.0.0.1:52882`
  - `udp/7447` 没有占用,说明它们不是当前要测的 `mac.lab`
- 清理旧实例后,用 `target/debug/rcat daemon -c rcat_macos.toml` 启动真实 `mac.lab`
- 运行 pipe 模式 smoke:
  - `@ping` 返回 `@response "pong"`
  - 裸 shell `pwd` 返回仓库路径
- 运行 PTY smoke:
  - `target/debug/rcat control mac.lab --pty -- /bin/sh -c 'if [ -t 0 ]; then printf MACLAB_PTY_OK; else printf MACLAB_NOT_TTY; fi'`
  - 结果为 `MACLAB_PTY_OK`
- 运行真实 TTY smoke:
  - 打开 `target/debug/rcat control mac.lab`
  - 输入 `@ping`
  - 终端直接显示 `pong`,证明 TTY 显示层也在真实主机上工作
- 启动时额外发现 pid guard 的探测噪声:
  - `kill: 54255: No such process`
  - 已在 `src/zenoh_control.rs` 里把 `kill -0` 探测改成静默 stderr/stdout
  - 重新编译、回归并再次启动后,噪声消失
- 最后停止本轮启动的 daemon,确认没有新残留进程

### 总结感悟

- 真机 smoke 很有价值。它不仅证明 `mac.lab` 这条路径通了,还顺手暴露了 pid guard 的脏输出问题。
- `rcat control mac.lab` 现在在这台主机上已经具备完整的日常使用闭环:
  - pipe 下保留协议输出
  - TTY 下显示人类可读正文
  - `--pty` 下远端 stdin 的确是终端
- 同机残留 daemon 很容易让 smoke 结果变脏。先看 `udp/7447` 和 autodiscovery 结果,再决定是否清理,这个判断口径是对的。

## [2026-05-05 22:03:07] [Session ID: codex-20260505-maclab-smoke-script] 任务名称: 固化 `mac.lab` live smoke 脚本

### 任务内容

- 把上一轮已经验证过的 `mac.lab` 本机 smoke 固化成仓库内固定脚本入口。
- 覆盖两条运行路径:
  - 当前已有可用 `mac.lab` daemon 时直接复用
  - 当前不可用时临时用 `rcat_macos.toml` 启动一个本地 daemon
- 同步 `cmd.md` 与 `README.md`,避免脚本和文档脱节。

### 完成过程

- 新增 [scripts/mac_lab_live_smoke.sh](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/scripts/mac_lab_live_smoke.sh)
  - 默认先 `cargo build --quiet`
  - 默认先探测 `rcat control mac.lab` 的 `@ping` 是否可用
  - 若可用则复用现有 daemon
  - 若不可用则临时启动 `target/debug/rcat daemon -c rcat_macos.toml`
  - 固定跑 4 组 smoke:
    - `@ping`
    - 裸 shell `printf MACLAB_LITERAL_SHELL_OK`
    - `--pty` TTY probe
    - 真实 TTY 下 `@ping` 是否显示成 `pong`
  - 若 daemon 是脚本自己拉起的,退出时自动清理
- 为了把 TTY 人类显示也纳入固定 smoke,在脚本里内嵌了一个 `python3` PTY harness
- 同步文档:
  - [cmd.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/cmd.md)
  - [README.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/README.md)
- 自测时第一次发现 ready 日志匹配写窄了:
  - 误把 `service_name(daemon_name)=mac.lab` 当成 `daemon_name=mac.lab`
  - 修正匹配后再次运行,脚本通过
- 额外把复用分支也做了实机验证:
  - 手动起 daemon
  - `RCAT_SKIP_BUILD=1 ./scripts/mac_lab_live_smoke.sh`
  - 确认脚本复用现有实例,并且不误停它

### 总结感悟

- smoke 脚本如果只验证“当前无 daemon 时能自启”,其实还不够。复用已有实例的分支一样要实测,否则日常使用时最容易翻车的反而是默认路径。
- 把 TTY display 也写进固定脚本后,这个入口就不只是协议检查,而是更接近日常真实使用体验的回归。
- 这类 repo-supported runbook, 光写在聊天里不够。最好同时有:
  - 固定脚本入口
  - 文档可发现入口
  - 实机跑过的证据

## [2026-05-05 23:20:48] [Session ID: codex-20260505-pty-string-shorthand] 任务名称: 支持 `@pty:"codex"` 字符串简写

### 任务内容

- 让 line-control 协议支持 `@pty:"codex"` 这种和 `@key:"F11"` 一样的字符串简写。
- 保持现有对象写法 `@pty:{cmd:"...",argv:[...],cols:...,rows:...}` 继续可用。
- 让 `rcat control` 客户端在手打 `@pty:...` 时也会自动切进 PTY 模式,而不是只认 `--pty` CLI sugar。

### 完成过程

- 更新 [src/control_protocol.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_protocol.rs)
  - `parse_pty_payload()` 现在同时接受字符串和对象
  - 新增默认语义:
    - `@pty:"codex"` => `cmd="codex"`, `argv=["codex"]`, `cols=80`, `rows=24`
  - request id 语法自然兼容:
    - `@pty#9:"codex"`
- 更新 [src/control_client_input.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_client_input.rs)
  - 新增 `ControlStdinAction`
  - 允许 stdin 分发在识别到 PTY open 行时中途切换模式
- 更新 [src/shell.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/shell.rs)
  - TCP / WebSocket control client 现在会在手打 `@pty:...` 时切进 `run_pty_client_transport()`
- 更新 [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs)
  - Zenoh control client 现在也会在手打 `@pty:...` 时切进 `run_client_pty_over_session_bridge()`
- 扩测试:
  - `src/control_protocol.rs` 增加 `@pty:"codex"` 和 `@pty#9:"codex"` parser 覆盖
  - `src/pty_control.rs` 增加 string shorthand helper 覆盖
  - [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs) 增加“直接把 `@pty:"/usr/bin/tty"` 喂给 control stdin”集成测试
  - [tests/zenoh_router_client.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/zenoh_router_client.rs) 增加 Zenoh 端到端 string shorthand 集成测试
- 同步文档:
  - [cmd.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/cmd.md)
  - [README.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/README.md)
  - [specs/control-line-protocol.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/control-line-protocol.md)
  - [specs/pty-control-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/pty-control-plan.md)

### 总结感悟

- 这次最容易漏掉的点不是 parser,而是 client 路由。只让 daemon 认 `@pty:"..."` 还不够, 本地 `rcat control` 也必须在读到这一行时立刻切换到 PTY 会话模式。
- `@pty` 的字符串简写如果继续往下做参数切分, 很快就会长成第二套命令行解析器。把它收敛成“单命令最短入口”更稳。
- 现场验证里还要区分“当前跑着的 daemon 是不是新二进制”。不然很容易把旧进程的行为误判成新代码没生效。

## [2026-05-06 00:18:00] [Session ID: codex-20260506-pty-cmd-args-followup] 任务名称: `@pty` 对象协议收敛到 `cmd + args`

### 任务内容

- 把 `@pty` 对象 payload 从不直观的 `cmd + argv` 收敛成 `cmd + args`。
- 保持 `@pty:"codex"` 字符串简写继续可用。
- 保留对旧 `argv` payload 的兼容解析,但不再让它继续占据 canonical 口径。

### 完成过程

- 确认 [src/control_protocol.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_protocol.rs) 里的 `PtyOpenRequest` 已切成:
  - `cmd: String`
  - `args: Vec<String>`
- 确认 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs) 里的 CLI 渲染和 daemon 执行路径都已经跟随 `args` 口径:
  - `render_pty_open_line()` 输出 `@pty:{cmd:\"...\",args:[...],...}`
  - server 侧不再依赖 `argv.iter().skip(1)` 这种重复真相源写法
- 把规格残留示例改掉:
  - [specs/control-line-protocol.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/control-line-protocol.md)
  - [specs/pty-control-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/pty-control-plan.md)
- 在规格中显式写清:
  - `@pty:"codex"` => `cmd=\"codex\"`, `args=[]`
  - canonical object form => `@pty:{cmd:\"codex\",args:[\"--profile\",\"fast\"],cols:120,rows:40}`
  - parser 仍兼容 legacy `argv`,但那已经不是文档真相源

### 验证

- `cargo fmt --all`: 通过
- `beautiful-mermaid-rs --ascii`: 已验证本轮改动涉及的两个规格文件内全部 Mermaid block
- `cargo test --bin rcat control_protocol::tests::parse_should_support_pty_open_and_close_requests -- --exact --nocapture`: 通过
- `cargo test --bin rcat control_protocol::tests::parse_should_reject_unknown_or_empty_or_multiline_payloads_or_bad_request_ids -- --exact --nocapture`: 通过
- `cargo test --bin rcat pty_control::tests::render_pty_open_line_should_roundtrip_through_protocol_parser -- --exact --nocapture`: 通过
- `cargo test --test control_pty -- --nocapture`: 4 passed
- `cargo test --test zenoh_router_client control_should_run_pty_command_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --bin rcat --quiet`: 122 passed
- `cargo build --quiet`: 通过
- `git diff --check`: 通过

### 总结感悟

- `cmd + argv` 的主要问题不是“不能用”,而是它把程序名写了两遍,逼着协议、渲染和执行端一起做同步。
- `cmd + args` 更符合人类直觉,也更接近 MCP 这类常见对象约定。
- 兼容层可以暂时保留,但真相源必须只有一个。否则下一次改协议时,旧心智会顺着文档和示例重新长回来。

## [2026-05-06 10:42:12] [Session ID: codex-20260506-pty-enter-stall] 任务名称: 修复 `@pty:"codex"` 后 Enter 无效

### 任务内容

- 修复 `rcat control mac.lab` 中输入 `@pty:"codex"` 后,远端 `codex` TUI 对回车无反应的问题。
- 给 PTY 客户端补一个真实 TTY 级回归测试,证明 Enter 会按 carriage return `0d` 透传到远端 raw PTY。

### 完成过程

- 回读 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs) 的 PTY client 输入链路。
- 确认进入 PTY 后会调用 `LocalRawTerminalGuard::enter_if_tty()`。
- 补充 [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs) 中的真实 TTY 测试:
  - 用 `script` 给 `rcat control` 提供真实 TTY
  - 输入 `@pty:"临时 helper"`
  - helper 在远端 PTY 里切 raw mode 并读取一个字节
  - 修复前输出 `0a`,修复后输出 `0d`
- 将 `LocalRawTerminalGuard` 从“只关 canonical/echo/signal”改成接近 `cfmakeraw` 的完整 raw mode。
- 顺手保留 `/dev/tty` 的 `File`,避免 Drop 时用已经关闭的 fd 恢复终端。

### 验证

- `cargo test --test control_pty control_pty_string_shorthand_should_forward_enter_as_carriage_return_in_tty -- --exact --nocapture`: 通过
- `cargo fmt --all`: 通过
- `cargo test --bin rcat pty_control::tests:: -- --nocapture`: 4 passed
- `cargo test --test control_pty -- --nocapture`: 5 passed
- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --bin rcat --quiet`: 122 passed
- `cargo build --quiet`: 通过
- `git diff --check`: 通过

### 总结感悟

- PTY client 的 raw mode 必须接近 `cfmakeraw`,不能只关 `ICANON` 和 `ECHO`。
- 对 shell 还“看起来能用”的输入翻译,到 `codex` / vim / REPL 这类 TUI 上就会变成真实交互 bug。
- 回归测试要用真实 TTY,pipe 模式测不到这类本地 line discipline 问题。
## [2026-05-06 12:01:36] [Session ID: codex-20260506-pty-ctrlc-resume] 任务名称: 修复 interactive `@pty` 的 Ctrl-C 生命周期

### 任务内容

- 修复 `rcat control ...` interactive 模式里 `@pty:"codex"` 运行期间按 `Ctrl-C` 时,远端 `codex` 退出但本地 `rcat control` 也一起退出的问题。
- 保持既有契约不变:
  - `Ctrl-C` / `Ctrl-D` 先送远端 PTY
  - 不引入 in-band escape
  - `@pty:"codex"` 字符串简写继续可用
- 顺手把 Zenoh session bridge 的 PTY 生命周期也对齐到同一模型,避免 direct PTY 路径因为 exit 收尾时序不同而挂住。

### 完成过程

- 更新 [src/shell.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/shell.rs)
  - interactive control client 已经稳定在 loop 中处理 stdin
  - 手打 `@pty:...` 后进入 PTY loop,PTy 结束再回到 line-control loop,不再把 `run_pty_client_transport()` 的返回当成整个 control client 结束
- 更新 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs)
  - server 侧 `run_pty_server_loop()` 不再在 wait thread 里先卡 `output_thread.join()`
  - 改成先把子进程 exit 送回主循环,主循环显式 `drop(pty_writer)`、drain output thread 后再发 `@pty-exit`
  - 这样 client 能及时收到 PTY 退出,不会被 reader thread EOF 顺序拖住
- 更新 [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs)
  - 补齐 borrowed / owned 两套 PTY client bridge
  - interactive `run_client_control()` 手打 `@pty` 后复用现有 session bridge,PTY 结束继续留在 control loop
  - daemon `open_daemon_session_bridge()` 在 PTY 结束后不再 `return`,而是继续处理同一 session 后续 control 行
  - direct PTY client 在已经成功拿到 PTY 输出后,若 subscriber 随后自然关闭,按成功结束处理,避免 Zenoh shorthand 因 close 时序不同误报失败
- 更新测试:
  - [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs)
    - 新增的 `Ctrl-C` 回归测试现在会在验证 `REMOTE_INT` 和 `pong` 后再送本地 `Ctrl-D`,让 interactive client 自然退出
    - Enter 字节测试也同步补了本地 EOF 收尾,避免再因为 interactive client 继续存活而超时

### 总结感悟

- 这次真正的 bug 不是 `Ctrl-C` 信号本身,而是 PTY 子会话和 control 主会话的生命周期被混成了一层。
- TCP 和 Zenoh 的 PTY 行为如果要统一,关键不是 parser,而是:
  - PTY exit 发生后,transport/session 还要不要继续活着
  - reader / writer / wait thread 的收尾顺序是谁驱动谁
- 真实 TTY 测试非常值钱。第一次失败时 stdout 里已经有 `REMOTE_INT` 和 `pong`,正是这个证据帮我区分出了“功能已修好”和“测试还没优雅退出”。
## [2026-05-06 12:37:08] [Session ID: codex-20260506-pty-ctrlc-resume] 任务名称: 收紧 PTY terminal lifecycle,实现 strict terminal frames first

### 任务内容

- 先把 PTY 规格升级成显式 terminal lifecycle 模型。
- 实现第一阶段代码收口:
  - `@pty-exit` 只表示远端进程自然退出
  - `@pty-closed` 表示 out-of-band close / transport 丢失 / 策略回收
  - direct PTY 不再允许“收到过输出 + subscriber 关闭 = 成功”
- 给后续 detach / reattach / close reason / 审计日志预留正式协议位,避免以后再返工。

### 完成过程

- 更新 [specs/pty-control-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/pty-control-plan.md)
  - 新增 terminal lifecycle taxonomy:
    - `@pty-exit`
    - `@pty-closed`
    - future `@pty-detached` / `@pty-attached`
  - 明确 direct PTY mode 的完成条件:
    - 必须收到 terminal lifecycle frame
    - 不能再靠 channel close 推断成功
- 更新 [src/control_frames.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_frames.rs)
  - `PtyExitFrame` 现在带:
    - `session_id`
    - `exit_code`
    - `reason`
    - `ended_at`
  - 新增 `PtyClosedFrame`
  - parser / wire format / tests 一起收口
- 更新 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs)
  - 服务端 PTY runtime 现在区分 terminal outcome:
    - `ProcessExit`
    - `Closed`
  - `@pty-close` 不再伪装成 `@pty-exit exit_code=1`
  - 活动 PTY registry 从“只有 killer handle”升级成:
    - `close_handle`
    - `terminal_reason`
  - transport 断开时先给子进程一个短暂自然退出观察窗口
    - 快速退出命令仍优先产出 `@pty-exit`
    - 真正链路丢失才产出 `@pty-closed reason=transport_lost`
- 更新 [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs) 和 [src/shell.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/shell.rs)
  - client 侧 direct PTY 现在只认:
    - `@pty-exit`
    - `@pty-closed`
  - 删除了之前为了临时修住 interactive `Ctrl-C` 引入的 `saw_output => Ok(())` 宽松兜底
- 更新测试:
  - [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs)
    - `@pty-close` 现在期待 `@pty-closed`
  - [tests/zenoh_router_client.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/zenoh_router_client.rs)
    - strict terminal frame 路径重新跑通

### 总结感悟

- 这轮真正重要的不是多加了一个 frame,而是把“为什么结束”从隐式时序推断,改成了协议里的显式语义。
- 只有先把 `process_exit` 和 `force_close` 分开,后面的 detach / reattach / 审计日志才有干净地基。
- `transport_lost` 这种 close reason 不能太早下结论。对于 `/usr/bin/tty` 这种快退命令,如果不留一个很短的自然退出观察窗口,就会把正常结束误判成链路异常。
## [2026-05-06 18:15:27] [Session ID: codex-20260506-pty-ctrlc-resume] 任务名称: 打通 TCP/WebSocket PTY detach / attach 第一版

### 任务内容

- 把 spec 里已经预留的 `@pty-detach` / `@pty-attach` 真正推进到 runtime。
- 目标不是一次性把所有 transport 都做完,而是先拿下 TCP/WebSocket 这条最小闭环:
  - detach 后 PTY 进程继续活着
  - attach 后后续输出能回到当前 attached client
  - attach 后仍可 out-of-band close

### 完成过程

- 更新 [src/control_protocol.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_protocol.rs)
  - 新增:
    - `PtyDetachRequest`
    - `PtyAttachRequest`
  - parser 现在支持:
    - `@pty-detach:{session_id:"..."}`
    - `@pty-attach:"..."`
    - `@pty-attach:{session_id:"...",cols:...,rows:...}`
- 更新 [src/control_frames.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_frames.rs)
  - 新增:
    - `PtyDetachedFrame`
    - `PtyAttachedFrame`
- 更新 [src/input.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/input.rs) 与 [src/main.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/main.rs)
  - CLI 现在支持:
    - `--pty-detach`
    - `--pty-attach`
- 更新 [src/control_core.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/control_core.rs)
  - `@pty-detach` 已能真正落到 PTY runtime
- 更新 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs)
  - 引入后台 `AttachedPtySession`
  - PTY session registry 现在不只存 killer,还存:
    - `input_tx`
    - `attached_control_session_id`
    - `terminal_reason`
  - attach 后输出不再写死发给最初 open 的 channel,而是切到当前 attached client
  - attach 后 terminal frame 也跟随当前 attached channel 走
- 更新 [src/shell.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/shell.rs)
  - TCP/WebSocket receiver 现在会识别 `@pty-attach`
  - open / attach 都开始复用后台 `AttachedPtySession` bridge
- 更新测试:
  - [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs)
    - 新增 `control_pty_detach_should_allow_later_attach`
    - 验证:
      - 先收到 `FIRST`
      - detach
      - attach 后收到新的 `SECOND`
      - 之后还能由另一条 control 连接 `--pty-close`
      - attached client 会显示 `remote PTY closed before natural exit: force_close`

### 总结感悟

- attach 这类功能最难的点不是 parser,而是“输出路由”和“terminal frame 路由”必须一起切到当前 attached client。
- 如果只切输出不切 terminal frame,就会出现:
  - 新 client 能看到后续输出
  - 但收不到最终 `@pty-exit` / `@pty-closed`
  - 最后 CLI 一直挂着不退
- 所以 detach / attach 的真正单一真相源不是 transport 本身,而是后台 session runtime + 当前 attached sink。

## [2026-05-07 00:14:03] [Session ID: 019ded13-aa64-7043-801e-294d098c48b2] 任务名称: 修复 Zenoh PTY timeout 误判并同步生命周期文档

### 任务内容

- 修复 Zenoh PTY bridge / client loops 把 `recv_timeout()` 的 timeout 当 subscriber 关闭的问题。
- 同步 `@pty-close` / `@pty-detach` / `@pty-attach` 的文档口径,把 detach / attach 从“预留”更新成已实现生命周期能力。
- 收口 `@pty` canonical payload 为 `cmd + args`,并保留 legacy `argv` 兼容说明。

### 完成过程

- 从失败的 `control_should_accept_pty_string_shorthand_in_zenoh_profile` 入手。
- 先确认 daemon 已经发出 `@pty-ready`,并且远端 PTY 已经产生 output。
- 再查 Zenoh 1.8.0 本地源码,确认 `recv_timeout()` 超时返回 `Ok(None)`。
- 修改 [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs)
  - active PTY 下 `Ok(None)` 改成继续轮询
  - daemon session bridge 只在没有 active PTY 时把 idle timeout 当 bridge 回收
  - 删除未使用的 `ZenohPtyFrameSender`
- 修改 [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs)
  - 保留旧同步 PTY server loop,但明确标注为 dead_code 兼容保留,避免 warning 干扰验证。
- 同步文档:
  - [README.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/README.md)
  - [cmd.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/cmd.md)
  - [specs/control-line-protocol.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/control-line-protocol.md)
  - [specs/pty-control-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/pty-control-plan.md)
  - [specs/zenoh-control-plane-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/zenoh-control-plane-plan.md)
  - [specs/zenoh-sdk-integration-playbook.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/zenoh-sdk-integration-playbook.md)
  - [specs/zenoh-sdk-agent-prompts.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/zenoh-sdk-agent-prompts.md)

### 验证

- `cargo fmt --all`: 通过
- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_detach_and_attach_pty_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client -- --nocapture`: 18 passed, 1 ignored
- `cargo test --test control_pty -- --nocapture`: 7 passed
- `cargo check --quiet`: 通过
- `cargo test --bin rcat --quiet`: 123 passed
- `git diff --check`: 通过

### 总结感悟

- Zenoh `recv_timeout()` 的 `Ok(None)` 是 timeout,不是断链。
- strict terminal lifecycle 的关键是: `@pty-exit` / `@pty-closed` 才能结束 PTY 成功或失败判断。
- `@pty-detached` / `@pty-attached` 只能表达控制端所有权变化,不能被当成 session 完成。

## [2026-05-07 00:27:45] [Session ID: 019ded13-aa64-7043-801e-294d098c48b2] 任务名称: 本机 `mac.lab` live smoke 收尾验证

### 任务内容

- 用仓库固定脚本 `./scripts/mac_lab_live_smoke.sh` 验证当前这台主机上的 `mac.lab` 路径。
- 覆盖 `@ping`、裸 shell、`--pty` 和真实 TTY 人类显示。

### 完成过程

- 首次运行脚本时复用了已有旧 daemon。
- 旧 daemon 能通过 `@ping` 和裸 shell,但 `--pty` 返回旧版 `@pty-exit`,缺少新协议必填 `reason` 字段。
- 停止旧 daemon pid=3161 后重新运行脚本。
- 脚本使用当前 `target/debug/rcat` 临时启动 `mac.lab` daemon,并在结束时自动停止。

### 验证

- `./scripts/mac_lab_live_smoke.sh`: 通过
  - temporary daemon ready
  - pipe `@ping`: 通过
  - literal shell: 通过
  - `--pty`: 通过
  - TTY display: 通过
  - temporary daemon stopped

### 总结感悟

- PTY frame schema 改动后,live smoke 不能只看 `@ping` 是否通。
- `@ping` 通过只能证明控制面基础链路可用,不能证明 daemon 协议版本和当前 client 完全一致。
- 遇到 `@pty-exit` 缺字段这类错误时,优先确认是不是复用了旧 daemon。

## [2026-05-07 01:02:19] [Session ID: codex-20260507-commit] 任务名称: Zenoh PTY 生命周期改动提交

### 任务内容

- 按用户 `commit` 请求提交本轮 Zenoh/PTTY/PTY 改动。
- 提交前确认 `.omx` 运行态文件没有进入 staged 范围。
- 处理归档历史 Markdown 中阻塞 `git diff --cached --check` 的尾随空格、CRLF 行尾和 rebase conflict marker。

### 完成过程

- 先跑 `git status --short`、`git diff --cached --stat`、`git diff --cached --check` 和 `.omx` staged 检查。
- 对 `archive/default_history/2026-05-05_pre_zenoh_bare_shell/` 下的历史归档日志做纯格式清理,保留两边历史正文,只删除 conflict marker。
- 第一次提交被 OMX hook 拦截后,查明需要固定 trailer `Co-authored-by: OmX <omx@oh-my-codex.dev>`,随后重新提交成功。
- 最终 commit 为 `195d0079ca2ed95f511851e906321100d658db5c`,主题是 `Preserve PTY lifecycle semantics across Zenoh control`。

### 总结感悟

- 这类长任务的归档文件也要参与 `git diff --check`,否则历史记录里的行尾和冲突标记会在最后一步挡住提交。
- 通过 Codex/OMX 提交时,commit message 除 Lore trailers 外还需要固定 OmX co-author trailer。

## [2026-05-07 18:10:11] [Session ID: codex-20260507-zenoh-pty-idle-log] 任务名称: Zenoh PTY 空闲轮询日志降噪

### 任务内容

- 修复 `rcat control mac.lab` 远程 PTY 过程中 `Zenoh PTY bridge polling session` / `had no queued frame` 反复刷屏的问题。
- 保持 Zenoh PTY session bridge 的 25ms active poll 行为不变,只调整正常 idle poll 的日志可见性。

### 完成过程

- 读取 `src/zenoh_control.rs` 中 `open_daemon_session_bridge()` 的 active PTY loop。
- 确认刷屏日志来自存在 active PTY session 时的短轮询分支。
- 将高频诊断日志从 `log::info!` 降为 `log::debug!`:
  - 每轮 polling session
  - 每个 forwarding frame
  - 每轮 no queued frame
- 保留 PTY open / attached / failed 这些状态变化日志的 `info` / `warn` 级别。

### 验证

- `cargo fmt --all`: 通过
- `cargo check --quiet`: 通过
- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_detach_and_attach_pty_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client -- --nocapture`: 18 passed, 1 ignored
- `git diff --check`: 通过

### 总结感悟

- active PTY 下短轮询是功能需要,不应该为了降噪改慢或去掉。
- 正常 idle poll 属于 debug 诊断,不应该默认占用用户的 control 终端。
- protocol lifecycle 和 observability 要分开处理。不要为了日志噪声去碰 `@pty-exit` / `@pty-closed` 的硬完成语义。

## [2026-05-07 19:25:14] [Session ID: codex-20260507-zenoh-pty-ssh-input] 任务名称: 修复 Zenoh `@pty` 后无法像 SSH 一样输入

### 任务内容

- 修复 `rcat control mac.lab` 内手动输入 `@pty:"codex"` 后,远端 PTY 程序无法继续接收本地键盘输入的问题。
- 同步降低 daemon 端 PTY 高频日志噪声,避免默认 info 级日志刷屏。
- 增加真实 TTY 回归测试,覆盖远端输出进入 idle 后本地输入仍能送达远端 PTY。

### 完成过程

- 先确认用户日志里的 `PTY output produced` 表示远端 PTY 已经启动,问题不是 open 失败。
- 新增 `control_should_forward_tty_input_after_zenoh_pty_output_goes_idle`,用 `script` 包装真实 TTY。
- 通过 debug 证据确认 client 已 publish `@pty-stdin`,daemon bridge 已收到 frame,但没有写入 PTY runtime。
- 修复 `parse_pty_open_request()` 过宽的 `starts_with("@pty")` 判断。
- 把 Zenoh active PTY 分支前移,进入 PTY 后优先处理 PTY stream frame,再处理生命周期控制。
- 同步调整 TCP/WebSocket 共享 PTY bridge 的 `@pty-stdin` 优先级。
- 删除会泄露完整输入行的 debug 形式,只保留 frame kind / bytes 级诊断。

### 验证

- `cargo test pty_control::tests::parse_pty_open_request_should_not_claim_pty_stream_frames -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_forward_tty_input_after_zenoh_pty_output_goes_idle -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client -- --nocapture`: 19 passed, 1 ignored
- `cargo test --test control_pty -- --nocapture`: 7 passed
- `cargo test --test control_websocket -- --nocapture`: 2 passed, 1 ignored
- `cargo test --all-targets -- --nocapture`: 通过
- `cargo check --quiet`: 通过
- `git diff --check`: 通过

### 总结感悟

- `@pty` open helper 不能只靠字符串前缀判断,因为 `@pty-stdin` / `@pty-output` / `@pty-exit` 都共享同一个词根。
- active PTY 模式下,frame parser 必须先于 line-control parser。
- 一旦进入 PTY,用户输入应该默认属于远端程序,否则就很容易再次破坏类似 SSH 的透明交互体验。

## [2026-05-07 20:48:26] [Session ID: codex-20260507-zenoh-pty-tui-latency] 任务名称: Zenoh PTY TUI 输入重绘延迟修复

### 任务内容

- 修复 `@pty:"codex"` 忙输出期间后续输入不即时显示的问题。
- 覆盖 `src/zenoh_control.rs` 的 client PTY loop 和 daemon session bridge 调度公平性。
- 增加 TUI-like 回归测试,锁定 busy output 期间本地输入仍应触发远端 repaint。

### 完成过程

- 复盘上一轮 `@pty-stdin` 已能写入远端 PTY 的边界,确认本轮不是 frame parser 问题。
- 新增 `control_should_repaint_tui_input_while_zenoh_pty_output_is_busy`。
- 将 Zenoh client TTY stdin publish 从 output receive loop 中拆出,改由独立线程通过 cloned `zenoh::Session` publish。
- 将 daemon active PTY output drain 限制为每轮最多 32 帧,避免持续 output 压住 inbound stdin/control。
- 修正 raw TTY `Ok(0)` 语义和 pipe EOF 语义,避免热循环、误退出和非 TTY direct PTY 挂住。

### 验证

- `cargo test --test zenoh_router_client control_should_repaint_tui_input_while_zenoh_pty_output_is_busy -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_forward_tty_input_after_zenoh_pty_output_goes_idle -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client control_should_run_pty_command_in_zenoh_profile -- --exact --nocapture`: 通过
- `cargo test --test zenoh_router_client -- --nocapture`: 20 passed, 1 ignored
- `cargo test --test control_pty -- --nocapture`: 7 passed
- `cargo test --test control_websocket -- --nocapture`: 2 passed, 1 ignored
- `cargo check --quiet`: 通过
- `git diff --check`: 通过

### 总结感悟

- PTY TUI 体验不是只要 `stdin` 和 `stdout` 都能传就够。
- 对 `codex`、shell、vim、REPL 这类程序,input publish、daemon inbound、output forwarding 都要有公平调度。
- raw TTY 和 pipe 的 EOF / no-data 语义必须分开处理,否则一个修复会把另一条路径打坏。

## [2026-05-07 23:33:30] [Session ID: codex-20260507-pty-shell-shorthand-resize] 任务名称: 支持 `@pty:"cmd args..."` 与真实 PTY resize

### 任务内容

- 支持用户直接输入 `@pty:"codex resume 019e02de-8814-72a2-ab0c-b06263cc0fba"`。
- 补齐 `@pty-resize` frame,让远端 PTY 能收到真实本地终端尺寸和后续窗口变化。
- 同步 TCP/WebSocket/Zenoh 三条 control lane 的 PTY 能力和文档说明。

### 完成过程

- 在 `src/control_protocol.rs` 中为 `@pty` 字符串 payload 增加 shell-style split,同时保留对象写法作为 canonical 入口。
- 在 `src/control_frames.rs` 新增 `PtyResizeFrame` 的 wire 编码/解析和维度校验。
- 在 `src/pty_control.rs` 中新增 resize runtime command、真实 Unix winsize 读取、raw TTY resize publisher,并在 attach 时同步调整 master PTY 尺寸。
- 在 `src/shell.rs` 和 `src/zenoh_control.rs` 中接入 active PTY 下的 `@pty-resize` 分流。
- 在 `tests/control_pty.rs` 中用 `stty size` 证明 TCP PTY 收到 resize 后远端 kernel winsize 真的变化。
- 在 `tests/zenoh_router_client.rs` 中证明 Zenoh profile 支持带参数的 `@pty:"..."` 字符串简写,并证明 Zenoh `@pty-resize` 能改变远端 `stty size`。
- 更新 `specs/pty-control-plan.md`、`specs/control-line-protocol.md`、`README.md`、`cmd.md` 中的协议说明。

### 总结感悟

- `@pty:"..."` 可以做人类输入糖,但内部仍要归一化为 `cmd + args`,否则协议真相源会重新分裂。
- resize 不能塞进 PTY stdin,必须是 out-of-band frame,否则会污染 `codex`、vim、shell 这类 TUI 的输入流。
- 远程 PTY 是否接近 SSH,不只看输入能不能到,还要看 winsize 和 resize 能不能及时传到远端 kernel PTY。

## [2026-05-07 23:39:17] [Session ID: codex-20260507-pty-shell-shorthand-resize-followup] 任务名称: 接手核验 `@pty` 字符串简写与 resize

### 任务内容

- 接手上一轮实现后,重新确认 `@pty:"cmd args..."` 与真实 PTY resize 已经落到当前工作区。
- 用 focused tests、相关全集和全量测试证明 TCP/WebSocket/Zenoh control lane 没有能力分裂。

### 完成过程

- 检查 `src/control_protocol.rs` 中 `@pty` 字符串 payload 的 shell-style split。
- 检查 `src/control_frames.rs` 的 `PtyResizeFrame` wire contract。
- 检查 `src/pty_control.rs` 的 `ioctl(TIOCGWINSZ)` winsize 读取、resize publisher 和 runtime resize command。
- 检查 `src/shell.rs` 与 `src/zenoh_control.rs` 中 active PTY 的 `@pty-resize` 分流。
- 运行 focused tests、`control_pty` / `zenoh_router_client` / `control_websocket` 全集、全量 `cargo test --all-targets`、`cargo check` 和 `git diff --check`。

### 验证

- `cargo test control_protocol::tests::parse_should_support_pty_open_and_close_requests -- --exact --nocapture`: 通过。
- `cargo test control_frames::tests::pty_resize_frame_should -- --nocapture`: 通过。
- `cargo test --test control_pty control_pty_resize_frame_should_update_remote_winsize -- --exact --nocapture`: 通过。
- `cargo test --test zenoh_router_client control_should_accept_pty_string_shorthand_in_zenoh_profile -- --exact --nocapture`: 通过。
- `cargo test --test zenoh_router_client control_should_forward_pty_resize_frame_in_zenoh_profile -- --exact --nocapture`: 通过。
- `cargo test --test control_pty -- --nocapture`: 8 passed。
- `cargo test --test zenoh_router_client -- --nocapture`: 21 passed, 1 ignored。
- `cargo test --test control_websocket -- --nocapture`: 2 passed, 1 ignored。
- `cargo test --all-targets -- --nocapture`: 通过。
- `cargo check --quiet`: 通过。
- `git diff --check`: 通过。

### 总结感悟

- 这次需求的关键不是新增第二种 PTY 协议,而是把人类输入糖归一化到既有 `cmd + args` 真相源。
- resize 必须作为 out-of-band terminal frame 传递,这样既能靠近 SSH 体验,又不会破坏 PTY stdin 透明性。

## [2026-05-10 23:40:50] [Session ID: omx-1778425927914-vdr4af] 任务名称: 分析 rcat Zenoh daemon/control 用于 code agent 远程协调主机

### 任务内容

- 分析当前仓库里 `rcat daemon` 的 Zenoh router profile 和 `rcat control` 的 Zenoh control client 能力。
- 明确用户口中的 `rcat zenoh daemon` 在当前 CLI 中并不是实际子命令,避免后续文档或使用口径继续漂移。
- 从 code agent 操控局域网、远程主机和多主机协调的角度提炼独特价值、边界和建议。

### 完成过程

- 读取了 README、cmd、control-line、pty-control、zenoh-control-plane、zenoh-sdk-integration、screenshot-control 等文档。
- 运行 `cargo run --quiet -- --help`、`daemon --help`、`control --help` 获取真实 CLI 形态。
- 阅读 `src/main.rs`、`src/input.rs`、`src/daemon.rs`、`src/zenoh_runtime.rs`、`src/zenoh_identity.rs`、`src/zenoh_control.rs`、`src/control_core.rs` 的关键路径。
- 查阅 `tests/zenoh_router_client.rs` 中 autodiscovery、entrypoint fallback、session channel、bare shell、`@key`、keyinput event、PTY、resize、detach/attach、daemon restart re-resolve 的测试覆盖。

### 总结感悟

- `rcat control` 对 code agent 的价值不是取代 SSH,而是提供一个 stdio-friendly、可发现、可解析、可扩展的远程控制面。
- Zenoh profile 的关键分界是 control-plane first: 裸 shell 行是 one-shot,真正 TUI 必须走 `@pty`。
- 对接方如果直接使用 Zenoh SDK,要把 queryable 看成 session bootstrap,不要再按旧单 query/reply 模型写死。

## [2026-05-11 10:17:59] [Session ID: omx-1778425927914-vdr4af] 任务名称: 落地 code agent 使用 rcat control 的规格文档

### 任务内容

- 新增 `specs/code-agent-rcat-control-usage.md`。
- 更新 `AGENTS.md` 长期知识索引,让后续 agent 在解释或实现 code agent 远程控制主机前能先读这份文档。
- 保留当前仓库真实口径: 没有 `rcat zenoh daemon` 子命令,正确入口是 `rcat daemon --transport zenoh` / `rcat daemon -c <配置>` 和 `rcat control <target-name>`。

### 完成过程

- 文档写入了 CLI 事实、code agent 心智模型、能力矩阵、line-control / PTY 推荐用法、Zenoh SDK session channel 模型、局域网与可达远程网络边界、安全权限边界、和最小 smoke 命令。
- 文档内包含一个 flowchart 和一个 sequenceDiagram,并用 `beautiful-mermaid-rs --ascii` 从 stdin 验证语法。
- 对新文档做了尾随空白与代码围栏平衡检查。
- 对已跟踪文档和上下文文件运行了 `git diff --check`。

### 总结感悟

- 面向 code agent 的文档不能只描述协议字段,还要说明什么时候用 one-shot line-control,什么时候必须切到 `@pty`。
- 这份文档把 `rcat control` 的定位固定为 stdio-friendly 的远程控制面,而不是 SSH 替代品。

## [2026-05-11 22:05:00] [Session ID: omx-1778469026342-c6n34v] 任务名称: 项目从 rcat/rustcat 更名为 rdog/rustdog

### 任务内容

- 将 Cargo package 从 `rustcat` 改为 `rustdog`,二进制入口从 `rcat` 改为 `rdog`。
- 同步 CLI help、配置模板、环境变量、默认目录、日志名、脚本、测试、README/cmd/specs/AGENTS 索引。
- 将 Zenoh 默认 keyexpr root 从 `rcat` 改为 `rdog`,同时保留 legacy `rcat` root 和 legacy session sentinel 解析兼容。
- 将配置加载改为新 `rdog_*` / `RDOG_` 优先,旧 `rcat_*` / `rcat.toml` / `RCAT_` 作为升级 fallback。

### 完成过程

- 使用 `git mv` 重命名平台配置、隐藏日志、下载目录和 code agent 规格文档。
- 执行受控文本替换,避开 `.omx`、`.codex`、`archive` 和旧六文件历史记录。
- 手动修正升级兼容层,避免更名导致已有配置、旧 env、旧 Zenoh session payload 立即失效。
- 修正由更名暴露的测试问题:
  - macOS `target/debug/rdog` 缺少辅助功能权限时,`@key` 返回 code 77,测试不再错误等待成功 key event。
  - PTY Ctrl-C 测试改为等待远端 helper ready 后再发送 Ctrl-C。
  - no-router autodiscovery 测试接受当前中文错误文案。
- 验证 AUR `rustdog.git` 存在,并更新 `.gitmodules`;同时确认 GitHub `raiscui/rustdog` 当前还不存在。

### 验证

- `cargo fmt --all`: 通过。
- `cargo check --quiet`: 通过,无 warning 输出。
- `cargo test --quiet config::tests -- --nocapture`: 23 passed。
- `cargo test --quiet --bin rdog input::tests -- --nocapture`: 8 passed。
- `cargo test --quiet --bin rdog zenoh_identity::tests -- --nocapture`: 6 passed。
- `zenoh_control` liveliness/session payload focused tests: 通过。
- `cargo test --quiet --test control_websocket control_cli_should_drive_websocket_daemon_end_to_end -- --nocapture`: 通过。
- `cargo test --quiet --test zenoh_router_client control_should_execute_literal_shell_line_in_zenoh_profile -- --nocapture`: 通过。
- `cargo test --quiet --test control_pty control_pty_string_shorthand_should_switch_cli_into_pty_mode -- --nocapture`: 通过。
- `cargo test --quiet --test zenoh_router_client -- --test-threads=1 --nocapture`: 21 passed, 1 ignored。
- `cargo test --quiet --all-targets -- --test-threads=1 --nocapture`: 通过。
- `beautiful-mermaid-rs --ascii` 验证所有改动 Markdown 中的 Mermaid block: 通过。
- `git diff --check`: 通过。
- `./target/debug/rdog --help`: usage 显示 `rdog <COMMAND>`。

### 总结感悟

- 更名不是简单字符串替换。配置、env、Zenoh keyexpr、session sentinel、权限绑定和测试二进制路径都属于真实对外面。
- 新名应该成为默认真相源,但 wire/runtime 旧入口需要明确 legacy fallback,否则会破坏已有部署或混跑场景。
- macOS 辅助功能权限绑定实际可执行文件。二进制从 `rcat` 改为 `rdog` 后,`@key` 权限测试必须把 code 77 视为可验证契约,不能误等成功事件。

### 补充验证记录

- 后续又修正了 `tests/zenoh_router_client.rs::run_control_with_retry_on_missing_target`,让默认并发测试中短暂命中 stale router locator 的 `Unable to connect to any of` 能在限定窗口内重试。
- `cargo fmt --all && cargo test --quiet --all-targets -- --nocapture`: 通过。

## [2026-05-11 23:34:24] [Session ID: omx-1778469026342-c6n34v] 任务名称: rdog 更名任务续跑收尾复核

### 任务内容

- 接续上一轮 `rcat/rustcat` -> `rdog/rustdog` 更名任务。
- 对更名后的仓库做交付前复核,确认当前主命令、编译、格式和旧名残留边界。
- 处理 `task_plan.md` 超 1000 行后的续档和持续学习沉淀。

### 完成过程

- 读取当前六文件上下文、交接摘要和相关长期知识索引。
- 运行并通过:
  - `cargo fmt --all --check`
  - `cargo check --quiet`
  - `git diff --check`
  - `./target/debug/rdog --help`
- 扫描残留 `rcat/rustcat` 引用,确认剩余命中是兼容层、兼容测试、升级说明或历史记录。
- 将超长 `task_plan.md` 续档到 `archive/default_history/2026-05-11_rdog_rename_continuation/task_plan_2026-05-11_rdog_rename_continuation.md`。
- 新建 `archive/manifests/ARCHIVE_MANIFEST__2026-05-11_rdog_rename_continuation.md` 并在 `AGENTS.md` 建索引。
- 在 `EXPERIENCE.md` 追加两条经验:
  - rdog 更名与 legacy 兼容经验。
  - CLI 二进制更名会改变系统权限主体。
- 记录本轮未加引号 heredoc 错误到 `ERRORFIX.md`。

### 总结感悟

- 更名不是全局替换。新名字要成为唯一默认真相源,旧名字只作为明确标注的升级兼容层存在。
- 对带 GUI/输入/截图副作用的远程控制工具,二进制名变化会影响系统权限主体,测试必须接受权限拒绝这类一等结果。
- 追加 Markdown 时只要正文含反引号,必须使用 `cat <<'EOF'`,否则 shell 会执行命令替换并污染日志。

## 2026-05-11 23:50:49 [Session ID: omx-1778469026342-c6n34v] 任务名称: rdog 更名本地提交

### 任务内容

- 按建议继续,将 `rdog/rustdog` 更名相关改动整理为本地 commit。
- 精确排除 `.codex/**` 删除和 `.omx/**` runtime 噪音。

### 完成过程

- 检查 submodule 状态: `pkg/arch/aur` 当前未初始化,没有 submodule 工作树内容可提交;本轮只提交 `.gitmodules` 中 AUR URL 更新。
- 使用 pathspec 暂存更名相关文件,并确认暂存区没有 `.codex/**` / `.omx/**`。
- 提交前运行并通过:
  - `cargo fmt --all --check`
  - `cargo check --quiet`
  - `git diff --cached --check`
  - `git diff --check`
  - `./target/debug/rdog --help`
- 第一次 commit 被 hook 拦截,原因是缺少 `Co-authored-by: OmX <omx@oh-my-codex.dev>` trailer;补齐后提交成功。
- 成功创建 commit: `Adopt rustdog naming as the primary control surface`。最终 hash 以 `git log -1 --oneline` 为准。

### 总结感悟

- 这个仓库的提交协议不只是普通 commit message,还要满足 Lore trailers 和 OmX co-author trailer。
- 提交大范围 rename 时,先用 staged diff 和排除检查守住 `.codex/**` / `.omx/**` 边界,比事后清理更安全。

## 2026-05-12 11:10:59 [Session ID: omx-1778469026342-c6n34v] 任务名称: 创建并推送 rustdog GitHub 远端

### 任务内容

- 接续 `rdog/rustdog` 更名任务,创建新的 GitHub 远端仓库。
- 将本地 `master` 和历史 tag 推送到 `raiscui/rustdog`。

### 完成过程

- 确认旧仓库 `raiscui/rustcat` 是 public,默认分支是 `master`。
- 确认 `raiscui/rustdog` 原本不存在。
- 第一次使用 active `GITHUB_TOKEN` 创建失败,错误为 `Resource not accessible by personal access token (createRepository)`。
- 使用 `env -u GITHUB_TOKEN gh ...` 切到 keyring 中 `raiscui` token 后,成功创建 public `https://github.com/raiscui/rustdog`。
- 添加本地 remote `rustdog = git@github.com:raiscui/rustdog.git`。
- 推送 `master` 并设置 upstream 为 `rustdog/master`。
- 推送全部 14 个历史 tag。

### 验证

- `git ls-remote git@github.com:raiscui/rustdog.git HEAD refs/heads/master` 返回 `424e0ef233a2265b967dd41f82f333180659052f`。
- `git rev-parse HEAD` 返回同一 hash。
- `git ls-remote --tags rustdog` 返回 14 个 tag。
- `gh repo view raiscui/rustdog` 显示仓库为 public,默认分支为 `master`。

### 总结感悟

- 迁移 GitHub 仓库时,`gh auth status` 里 active token 和 keyring token 可能不是同一个权限来源。
- `GITHUB_TOKEN` 可用于部分读操作,但未必有 `createRepository` 权限;必要时要明确用 `env -u GITHUB_TOKEN` 切回 keyring token。

## 2026-05-12 11:21:59 [Session ID: omx-1778469026342-c6n34v] 任务名称: 更新 README 匹配 rdog 更名和新增控制能力

### 任务内容

- 根据当前项目更名和新增功能更新 `README.md`。
- 让 README 的主入口从旧的 port listener/reverse shell 介绍,更新为包含 remote control plane、Zenoh、PTY、截图和 code agent 协调能力的当前口径。

### 完成过程

- 核验真实 CLI help:
  - `./target/debug/rdog --help`
  - `./target/debug/rdog control --help`
  - `./target/debug/rdog daemon --help`
- 回读 `cmd.md` 和 `specs/code-agent-rdog-control-usage.md`,确认 README 不写不存在的命令。
- 重写 README 结构:
  - `Rename and compatibility`
  - `Modes`
  - `Quick start`
  - `Control plane at a glance`
  - `Control commands`
  - `Code agent workflow`
  - `Daemon Mode`
  - `WebSocket control endpoint`
  - `Windows hidden resident mode`
  - `Zenoh Router / Serial Control Plane`
  - `Security notes`
- 保留 legacy `rcat` 兼容说明,但把新部署路径固定为 `rdog_*`、`RDOG_`、`rdog/...`。

### 验证

- README 本地链接检查: 通过。
- README Mermaid block: `beautiful-mermaid-rs --ascii` 通过。
- README 旧名扫描: 只剩兼容说明里的旧名。
- `git diff --check`: 通过。

### 总结感悟

- README 首页要先解释今天的产品心智,不能只保留旧时代的 listen/connect 介绍。
- 对 code agent 来说,最重要的不是命令多,而是 `rdog control` 的响应协议、target-name 寻址、PTY 和截图这些可编排能力。

## [2026-05-12 11:54:07] [Session ID: codex-app-2026-05-12-rustdog-repush] 任务名称: 重新发布 rustdog 到 raiscui/rustdog

### 任务内容

- 按用户新的本地 `git init` 状态,重新发布仓库到 `raiscui/rustdog`。
- 发布前检查 `.gitignore` 和已跟踪文件边界,避免把私有、运行态、构建产物和下载缓存推到 GitHub。

### 完成过程

- 确认当前分支是 `main`,初始提交已存在但 remote 为空。
- 检查 `.gitignore` 与全局 ignore:
  - `.envrc.private` 被 `/Users/cuiluming/.gitignore_global` 拦住。
  - `.omx/`、`target/`、`archive/`、`openspec/`、`rdog_downloads/` 被项目 `.gitignore` 拦住。
- 确认 tracked 文件中没有 `secret`、`token`、`credential`、`private`、`.pem`、`.key` 等命名风险文件;唯一 `.envrc` 是公开注释示例,只 source `.envrc.private`。
- 使用 Lore protocol amend 初始提交,最终本地 baseline commit 是 `9b2c0455f0caf6d50fb172c6d96f4a5ad9615de6`。
- 使用 `env -u GITHUB_TOKEN gh repo create raiscui/rustdog --public --description "Rustdog remote control utility" --source=. --remote=origin --push` 创建 public GitHub 仓库并推送 `main`。

### 验证

- `git diff --check`: 通过。
- `cargo fmt --all --check`: 通过。
- `cargo check --quiet`: 通过。
- `git check-ignore -v .envrc.private .omx/state.json .codex/tmp target/debug/foo rdog_downloads/foo archive/foo openspec/foo`: 对应路径均被 ignore 命中。
- `gh repo view raiscui/rustdog --json nameWithOwner,visibility,isPrivate,defaultBranchRef,url,pushedAt`: 返回 public,defaultBranch 为 `main`。
- `git ls-remote origin HEAD refs/heads/main`: 远端 HEAD 和 `refs/heads/main` 都是 `9b2c0455f0caf6d50fb172c6d96f4a5ad9615de6`。

### 总结感悟

- 这次是 fresh init 后发布,不能沿用上午旧的 `master` / tag / remote 结论。
- `GITHUB_TOKEN` 和 keyring 中 `raiscui` 权限来源不同;创建仓库时继续使用 `env -u GITHUB_TOKEN` 更稳。
