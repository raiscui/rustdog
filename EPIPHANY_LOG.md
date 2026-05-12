
## [2026-04-04 02:53:54] [Session ID: omx-1775241597665-yxkw7z] 主题: Zenoh 首次接入不能同时接管 control plane 和 shell data plane

### 发现来源
- 本轮为 `rustcat` 规划 Zenoh discovery / connection 与 daemon/control 互联方案时,对当前 TCP-only + PTY 实现和 Zenoh Rust API 形态做了对照。

### 核心问题
- 如果第一次引入 Zenoh 时,同时想解决节点发现、命令控制、interactive shell 字节流三件事,边界会立刻失控。
- 当前 control protocol 已经成熟,真正没抽开的只是 transport/session 边界。

### 为什么重要
- 这决定首版改造是“可验证的架构演进”,还是“一次性把网络层、runtime、shell 模型全部推倒”。
- 一旦首版把 PTY data plane 也卷进去,后续出现问题时很难判断到底是 discovery、query-reply、backpressure 还是 shell 桥接出了错。

### 未来风险
- 如果未来直接走“all-in Zenoh shell streaming”,很可能在 async runtime、半关闭语义、PTY 桥接和 CLI 心智四个维度同时踩坑。
- 这种混改会让 `rcat control` 的职责从桥接器膨胀成总线客户端,损伤产品边界。

### 当前结论
- 已知事实: line-control 非常适合映射到 Zenoh query-reply。
- 已知事实: interactive shell/PTY 仍然天然更像流式 data plane,不是首版最合适的映射对象。
- 当前推荐: Zenoh 先只进入 control plane,interactive shell 继续保持 TCP。

### 后续讨论入口
- 下次若真的要把 shell data plane 迁到 Zenoh,应先单独写一份 stream/pty 适配设计,不要和 control-plane discovery 规划混在一起。

## [2026-04-04 03:14:08] [Session ID: omx-1775241597665-yxkw7z] 主题: Zenoh 首版若不锁死边界,会演化成长期双平面泥团

### 发现来源
- 本轮 Zenoh 接入共识规划
- Planner / Architect / Critic 对 `daemon` 与 `rcat control` 互联方式的反复收敛

### 核心问题
- 如果 Zenoh 首版只是“再加一个 adapter”,但不写死 Phase 1 的命令范围、非目标、identity/keyexpr 和 runtime 边界,项目会进入长期的 TCP shell 面 + Zenoh control 面双平面泥团。

### 为什么重要
- 这类问题短期看像“灵活”,长期会把配置、日志、排障、测试矩阵和用户心智全部翻倍。
- 比起“能不能连上”,更重要的是 discovery 和 execution 是否留在同一平面。

### 未来风险
- 若采用“Zenoh 发现 + TCP 执行”的长期方案,后续会持续背着 split-plane 复杂度。
- 若 Zenoh Phase 1 不明确禁止 bare shell lines 和 interactive shell,用户会误判 Zenoh 能力与 TCP 完全等价。

### 当前结论
- 当前最稳的方向是: Zenoh 先承接 control plane,而且 discovery + execution 都留在 Zenoh control plane 内。
- 当前仍未验证的部分是: 具体 POC 下 tokio runtime 与现有阻塞线程模型的实际耦合成本。

### 后续讨论入口
- `specs/zenoh-control-plane-plan.md`
- `notes.md` 中本轮 Zenoh 事实底座记录

## [2026-04-04 03:18:40] [Session ID: omx-1775241597665-yxkw7z] 主题: Zenoh target 身份不能只用 `daemon_id`

### 发现来源
- 在规划 `rustcat` 的 Zenoh control plane 时,Architect/ Critic 多轮审视后暴露出的关键风险。

### 核心问题
- 如果只用稳定的 `daemon_id` 作为唯一寻址键,两个 daemon 一旦撞名,就会争用同一个 liveliness / control keyexpr。
- 这种问题不能只靠客户端“看见冲突后报错”补救,因为服务端已经先把冲突带进网络平面了。

### 为什么重要
- 这会直接破坏 Zenoh discovery、target 选择和 queryable control service 的可信性。
- 它属于架构级约束,不是文档提示能兜住的问题。

### 未来风险
- 如果后续实现时忘了把实例级身份和服务端 fail-fast 约束写死,Zenoh 模式会在多实例场景下出现隐蔽歧义。

### 当前结论
- 稳定身份和运行时实例身份必须分层:
  - `daemon_id`: 人类可读、可 targeting 的稳定身份
  - `instance_id`: 单次进程生命周期的实例身份
- daemon 在暴露 control queryable 前,必须先做同 `daemon_id` 冲突检查,冲突时 fail-fast。

### 后续讨论入口
- 下一次推进正式规格或实现时,优先先看这条日志和 Zenoh identity / keyexpr 设计草案。

## [2026-04-04 03:21:14] [Session ID: omx-1775241597665-yxkw7z] 主题: Zenoh 接入时不能只靠客户端处理重复 `daemon_id`

### 发现来源
- 在为 `rustcat` 规划 Zenoh control plane 接入时,经过 Architect / Critic 多轮收敛后暴露出的架构级风险。

### 核心问题
- 如果多个 daemon 使用相同的 `namespace + daemon_id`,它们会争用同一类逻辑身份。
- 这不是 `rcat control` 端“列出冲突实例让用户自己选”就能彻底解决的问题。
- 如果 daemon 侧仍然继续暴露 control queryable,系统就已经进入脏状态。

### 为什么重要
- 这会直接破坏 Zenoh discovery 与 target selection 的可信度。
- 一旦 identity 规则不稳,后续 queryable keyexpr、日志、定位、权限边界都会跟着变脆。

### 未来风险
- 同名 daemon 同时在线时,control 侧可能出现不稳定路由、错误目标命中或难以解释的冲突。
- 如果不做 daemon 侧 fail-fast / 唯一性约束,后续越往上层封装,问题越隐蔽。

### 当前结论
- 当前已确定的推荐口径是:
  - 使用 `daemon_id` + `instance_id` 双层身份模型
  - control 侧做冲突诊断和展示
  - daemon 侧在发现重复 `daemon_id` 时 fail-fast,不要继续暴露 control queryable
- 具体实现是“纯 liveliness 预检”还是更强的租约 / fencing 机制,还可以在正式规格阶段继续细化。

### 后续讨论入口
- 下次继续时优先先写正式规格草案,先把:
  - `daemon_id` / `instance_id`
  - keyexpr 命名
  - duplicate `daemon_id` fail-fast 契约
  写死,再谈实现。

## [2026-05-01 14:20:00] [Session ID: 019de364-f2af-7432-ad6a-40552af185c8] 主题: `rustcat` 后续能力扩展要守住“请求走 control,事件走 pub/sub”的双层边界

### 发现来源
- 本轮评估“远程截图能力到底该做成新 bin + 订阅频道,还是集成进 `@screenshot` 控制指令”时,对现有 line-control、Zenoh queryable 和 `key_input_events` 做了并读。

### 核心问题
- 如果后续一看到“需要远程触发本地动作”就直接新开一个订阅频道或新 bin,项目会慢慢长出第二套控制协议。
- 那样 request correlation、错误回传、权限语义、目标寻址、日志和测试矩阵都会重复。

### 为什么重要
- `rustcat` 当前最值钱的不是某个具体动作,而是已经形成了一条统一控制面:
  - 显式动作请求走 query/reply
  - 成功后的观察事件再走 pub/sub
- 这个边界一旦松掉,后续 `screenshot`、`clipboard`、`window-focus` 之类能力都会越加越散。

### 未来风险
- 如果把 screenshot request 做成订阅频道主入口,以后每个新能力都可能要求“再来一个 topic / 再来一个 helper bin”。
- 长期看会把 `rcat control` 变成半套协议,而真正复杂的能力全跑到旁路里,最后很难维护。

### 当前结论
- 当前更稳的扩展规律应是:
  - 请求入口优先挂进显式 control 协议,例如 `@screenshot`
  - 如果结果天然适合旁观订阅,再额外开放对应 event/result keyexpr
- 也就是说,先保住单一控制面,再按需要增加观察面,不要倒过来。

### 后续讨论入口
- `notes.md` 中本轮“远程截图能力应挂在哪个控制面”分析
- `specs/zenoh-control-plane-plan.md`
- `specs/zenoh-sdk-integration-playbook.md`

## [2026-05-02 10:14:30] [Session ID: 019de364-f2af-7432-ad6a-40552af185c8] 主题: 一旦要求 daemon 主动下发控制指令,Zenoh query/reply 就不再是长期控制面模型

### 发现来源
- 本轮用户把需求从 screenshot 返回技巧,上提成“真正双向控制面”。
- 对照了 `src/shell.rs` 的 socket control receiver 和 `src/zenoh_control.rs` 的 query/reply sender 模型。

### 核心问题
- 当前 Zenoh path 的根假设是:
  - control 发 query
  - daemon 回 single reply
- 这个假设一旦遇到“daemon 主动发 `@script` / `@savefile` / `@key`”,就会直接失效。

### 为什么重要
- 如果现在还沿着 query/reply 继续给 screenshot 打补丁,后面每个主动下发能力都会重复撞墙。
- 这不是某个命令要不要加字段的问题,而是 Zenoh 控制模型本身需要升级。

### 未来风险
- 如果继续把 Zenoh 留在单向 RPC,而 TCP/WebSocket 提前做成双向,仓库会长期分裂成两套 control 心智。
- 文档、测试和行为都会逐步漂移。

### 当前结论
- TCP / WebSocket 可以作为双向控制的先行验证面。
- 但长期架构上,Zenoh 也必须从 query/reply 升级到真正双向的 session/channel 模型。

### 后续讨论入口
- `specs/bidirectional-control-plane-plan.md`
- `specs/zenoh-screenshot-control-plan.md` 顶部提示

## [2026-05-05 18:58:00] [Session ID: codex-20260505-pty-implementation] 主题: out-of-band control 要求 daemon control inbound 具备并发 accept

### 发现来源

- 本轮实现 `@pty` / `@pty-close` 时,`control_pty_close_should_kill_active_session_by_id` 起初超时。

### 核心问题

- 如果 daemon `inbound.mode = "control"` 串行处理连接,一个 PTY 长会话会占住 accept loop。
- 这会导致另一个 control 请求,例如 `--pty-close`,无法进入 daemon。

### 为什么重要

- out-of-band control 不是只加一个协议指令就够了。
- transport accept 模型也必须允许第二个控制端在第一个长会话还活着时进入。

### 未来风险

- 后续如果实现 `@pty-resize`、远程 detach、daemon 主动下发控制等长会话能力,都要检查 accept/session 并发边界。
- 如果回退成串行 accept,这些 out-of-band 能力会表现为“命令发了但永远没响应”。

### 当前结论

- control mode inbound 应按连接分线程处理。
- interactive bind shell 可以继续保持原来的单连接阻塞语义。

### 后续讨论入口

- `src/daemon.rs` 的 `EndpointMode::Control` accept 分支
- `src/pty_control.rs` 的 active PTY session registry
- `tests/control_pty.rs` 的 `control_pty_close_should_kill_active_session_by_id`

## [2026-05-06 10:42:12] [Session ID: codex-20260506-pty-enter-stall] 主题: PTY client raw mode 不能只关闭 canonical/echo

### 发现来源

- 本轮修复 `@pty:"codex"` 后 Enter 无效的问题。
- 新增真实 TTY 级测试后,修复前远端 raw PTY 收到 `0a`,不是 `0d`。

### 核心问题

- 只关闭 `ICANON` / `ECHO` / `ISIG` 不等于真正 raw mode。
- 如果 `ICRNL` 仍然开启,本地终端会继续把 Enter 的 `\r` 翻译成 `\n`。

### 为什么重要

- 对 shell 或普通 `read` 流程,这个问题可能不明显。
- 对 `codex`、vim、REPL、全屏 TUI 这类远端 raw-key 程序,这个字节差异会变成真实交互失败。

### 未来风险

- 后续如果再改 PTY、terminal restore、resize、detach,不能把 raw mode 退回到“只关 canonical/echo”的简化版本。
- 真实交互问题必须用真实 TTY 测试,pipe 输入测不出本地 tty line discipline 的翻译。

### 当前结论

- PTY client 的本地终端模式应接近 `cfmakeraw`。
- Enter 应透传为 carriage return `0d`,不能被本地翻译成 newline `0a`。

### 后续讨论入口

- [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs) 的 `LocalRawTerminalGuard`
- [tests/control_pty.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/control_pty.rs) 的 `control_pty_string_shorthand_should_forward_enter_as_carriage_return_in_tty`
## [2026-05-06 12:38:54] [Session ID: codex-20260506-pty-ctrlc-resume] 主题: PTY completion 不能再用 transport close 推断

### 发现来源

- 本轮把 Zenoh PTY 从“见过输出 + subscriber close 即成功”收紧到 strict terminal lifecycle frame 时,`control_should_accept_pty_string_shorthand_in_zenoh_profile` 先后暴露出两类错判:
  - `@pty-close` 被伪装成 `@pty-exit exit_code=1`
  - `/usr/bin/tty` 这类快退命令被抢先判成 `transport_lost`

### 核心问题

- PTY 结束原因如果不进入显式协议对象,transport close 只能提供“链路断了”这一层事实,不能说明:
  - 是自然退出
  - 是 out-of-band force close
  - 还是链路先掉,进程还活着或刚刚退出

### 为什么重要

- 这不只是 Zenoh 的一个小 race。
- 后续 detach / reattach / close reason / 审计日志 都要求 terminal completion 有单一真相源,否则每条 transport 都会长出自己的猜测逻辑。

### 未来风险

- 如果以后有人为了“先让 direct PTY 不报错”再次把 subscriber close 当成功,退出码保真和 close reason 会再次被打散。
- detach / reattach 一旦上线,这种模糊完成条件会让“session 还活着”与“session 已完成”难以审计。

### 当前结论

- direct PTY mode 的完成条件必须是 terminal lifecycle frame:
  - `@pty-exit`
  - `@pty-closed`
- transport close 只能作为异常线索或 cleanup 触发器,不能再直接等价于 PTY 完成。
- 对快退命令,transport 丢失前要给一个很短的自然退出观察窗口,避免把真实 `process_exit` 抢成 `transport_lost`。

### 后续讨论入口

- [specs/pty-control-plan.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/specs/pty-control-plan.md) 的 terminal lifecycle 章节
- [src/pty_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/pty_control.rs) 的 `PtyTerminalOutcome`
- [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs) 的 direct PTY client terminal frame handling

## [2026-05-07 00:14:03] [Session ID: 019ded13-aa64-7043-801e-294d098c48b2] 主题: Zenoh `recv_timeout()` 的 `Ok(None)` 是 timeout,不是 closed

### 发现来源

- 修复 `control_should_accept_pty_string_shorthand_in_zenoh_profile` 时,daemon 已经产出 PTY output,但 client 误报 subscriber closed。
- 回查 Zenoh 1.8.0 本地源码 `api/handlers/fifo.rs::recv_timeout()` 后确认 API 语义。

### 核心问题

- Zenoh FIFO handler 的 `recv_timeout()` 在超时时返回 `Ok(None)`。
- 如果把 `Ok(None)` 当成 subscriber closed,所有短轮询 bridge 都可能在正常空闲瞬间提前退出。

### 为什么重要

- PTY bridge 正常运行时经常 25ms 内没有 stdin / control sample。
- 这种空档不是错误,而是轮询模型的常态。
- 一旦提前退出 bridge,后续 `@pty-output` / `@pty-exit` / `@pty-closed` 都会丢失,strict terminal lifecycle 就会被破坏。

### 未来风险

- 后续凡是用 Zenoh `recv_timeout()` 做 request、event、session frame 或 liveliness 辅助轮询,都可能重复这个错误。
- 这个 bug 表面看像 transport/subscriber 不稳定,但根因其实是本地对 API timeout 语义理解错了。

### 当前结论

- active loop 中 `Ok(None)` 应按 timeout 处理并继续。
- 真正的 PTY completion 仍然只能由 `@pty-exit` / `@pty-closed` 决定。
- session idle 回收只能在明确没有 active PTY 的 bridge 状态下发生。

### 后续讨论入口

- [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs) 的 `open_daemon_session_bridge()` 和 PTY client loops
- [ERRORFIX_2026-05-07_continuous-learning.md](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/archive/default_history/2026-05-07_continuous-learning/ERRORFIX_2026-05-07_continuous-learning.md) 中同时间戳的 Zenoh `recv_timeout()` 修复记录

## [2026-05-07 20:48:26] [Session ID: codex-20260507-zenoh-pty-tui-latency] 主题: PTY TUI 需要端到端泵送公平性

### 发现来源

- 修复 `@pty:"codex"` 忙输出期间输入框不即时重绘的问题。
- 新增 `control_should_repaint_tui_input_while_zenoh_pty_output_is_busy` 后确认 input/output 调度公平性是独立风险。

### 核心问题

- 远程 PTY 不只是 frame schema 问题。
- 即便 `@pty-stdin` 能到 daemon,如果 client output loop、client stdin publish、daemon output drain、daemon inbound read 之间没有公平调度,TUI 输入也会表现得不像本机终端。

### 为什么重要

- `codex`、vim、shell、REPL 这类 TUI 都依赖输入后立即 repaint。
- 如果持续 output 能压住 stdin,用户就会看到输入框冻结,误以为程序不接收输入。

### 未来风险

- 后续实现 resize、detach/reattach、更强 audit log 时,如果又引入单线程 drain-until-empty 的 loop,会复发同类延迟。
- raw TTY 的 `Ok(0)` 和 pipe EOF 语义若再次混淆,会导致热循环、误退出或 direct `--pty` 挂住。

### 当前结论

- Zenoh PTY 侧已经把 client stdin publish 解耦到独立线程。
- daemon active PTY output drain 每轮有上限,每轮之后必须回到 inbound session polling。
- PTY completion 仍然只能由 `@pty-exit` / `@pty-closed` 表达。

### 后续讨论入口

- [src/zenoh_control.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/src/zenoh_control.rs) 的 `run_client_pty_over_session_bridge_threaded_stdin()` 和 `open_daemon_session_bridge()`。
- [tests/zenoh_router_client.rs](/Users/cuiluming/local_doc/l_dev/my/rust/rustcat/tests/zenoh_router_client.rs) 的 `control_should_repaint_tui_input_while_zenoh_pty_output_is_busy`。

## [2026-05-11 22:08:00] [Session ID: omx-1778469026342-c6n34v] 主题: CLI 更名会改变 macOS 辅助功能权限主体

### 发现来源

- 项目从 `rcat` 更名为 `rdog` 后,`tests/zenoh_router_client.rs::control_should_publish_key_event_after_successful_key_request` 等待不到 key event。
- 手工复现 `@key#7:"F11"` 后,control 返回 code 77,daemon 输出 `The application does not have the permission to simulate input!`。

### 核心问题

- macOS 辅助功能权限不是抽象授予“项目”或“终端会话”,而是和实际执行输入模拟的进程身份绑定。
- 二进制从 `target/debug/rcat` 变成 `target/debug/rdog` 后,即使旧二进制曾经有权限,新二进制也可能没有。

### 为什么重要

- 后续任何重命名、codesign、bundle id、路径迁移,都可能让 `@key` / `@paste` 从成功变成权限拒绝。
- 测试不能把 key event 作为无条件成功路径,必须把 PermissionDenied / code 77 当成一等契约验证。

### 未来风险

- 如果 release 产物、ad-hoc signed binary 或 app bundle 路径变化,用户现场可能需要重新授权。
- 如果测试只等待 key event,权限拒绝时会表现成“事件丢失”,误导排查方向。

### 当前结论

- `@key` 成功后才发布 key event 是正确语义。
- 权限拒绝时应返回 code 77,并给出可执行恢复说明。
- 更名任务的验证必须包含新二进制名下的权限行为检查。

### 后续讨论入口

- `tests/zenoh_router_client.rs::control_should_publish_key_event_after_successful_key_request`
- `src/control_actions.rs::execute_key_with_dependencies`
- `src/control_protocol.rs` / `src/control_core.rs` 中 code 77 权限错误契约
