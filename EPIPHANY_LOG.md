
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

## [2026-05-18 16:30:01] [Session ID: codex-phase3-20260518-160435] 主题: Zenoh queryable 兼容面可能悄悄变成第二主路径

### 发现来源
- 本轮 Phase 3 调整 `src/zenoh_control.rs` 时,新增 `control_should_reject_rich_frame_over_legacy_queryable_path` 后先暴露出 direct queryable `@screenshot#7` 会返回完整 `@savefile` / bundle。
- 继续检查后又发现旧 `__rdog_session__:<id>\n...` query payload 也可能把 rich request 送进旧 queryable 分支。

### 核心问题
- 只要兼容层还能直接执行 rich command,queryable 就会成为隐藏的第二主路径。
- 这种路径不一定体现在 `@response ...` 的表层,因为结果可能被转发到 `to-control`,更难在普通回归里一眼看出。

### 为什么重要
- 这类问题会让“session channel 主路径收紧”的口径失真。
- 用户和后续维护者会以为 queryable 只是 bootstrap / legacy,但实际仍能做 screenshot、PTY 或 GUI action。

### 未来风险
- 如果后续再加 capability、doctor 或 GUI recipe,兼容层会很容易悄悄再长出一条富能力执行面。
- 仅靠“CLI 主路径走 session”不够,必须持续保留 queryable 负向测试。

### 当前结论
- queryable 现在已经被收敛为 bootstrap / legacy / compatibility,且对 rich/session-only 命令返回 code 78。
- direct query payload 和旧 session query payload 都需要各自有负向测试。

### 后续讨论入口
- `src/zenoh_control.rs` 的 `handle_daemon_control_query()` 和 `reject_session_channel_only_legacy_query()`
- `tests/zenoh_router_client.rs::control_should_reject_rich_frame_over_legacy_queryable_path`
- `tests/zenoh_router_client.rs::control_should_reject_rich_frame_over_legacy_session_query_payload`

## [2026-06-19 01:00:00] [Session ID: CURRENT_SESSION] 主题: rdog 日志默认走 stdout 而不是 stderr,e2e 测试断言要合流

### 发现来源
- 给 rdog control 加 one-shot CLI 入口后写 e2e 测试 `tests/control_lanes.rs::control_one_shot_*`
- 测试断言 `output.status.success() && stderr.contains("错误文案")` 全部失败,stderr 一直为空
- 手动跑 daemon + rdog control 复现,错误信息确实出现在 stdout

### 核心问题
- `src/main.rs::init_logger` 用 fern + `log_target_for_command`,非 hidden 模式走 `LogTarget::Stdout`
- `run()` 出错时统一 `log::error!("{err}"); exit(1);`,`log::error!` 走 stdout
- 违反 Unix 习惯:错误/警告应当走 stderr
- 影响:所有依赖 rdog CLI 退出码 + stderr 的 agent / 脚本会拿不到错误描述

### 为什么重要
- 后续任何给 rdog CLI 加 e2e 的测试,断言 `stderr.contains("...")` 都会假阴性
- 任何把 rdog 嵌进 shell pipeline 拿错误日志的脚本会拿空
- 长尾可观察性下降

### 未来风险
- 如果不修,所有新 CLI 子命令的 e2e 都会写错断言
- 真正的 stderr 信息(skill 加载、权限降级、bootstrap 警告)被 stdout 冲掉,debug 难度上升

### 当前结论
- 已知事实:`init_logger` 默认走 stdout,exit code 仍然正确
- 仍未确认:是否所有 release profile 都走 stdout,还是仅 debug 走;以及是否存在依赖 stdout 行为的下游(目前没有发现)
- 当前缓解:e2e 断言用 `format!("{}{}", stdout, stderr)` 合流检查

### 后续讨论入口
- 看 `src/main.rs::init_logger` + `src/hidden_mode.rs::log_target_for_command`
- 决定方向:全量切 stderr,还是给 `--log-target` flag 让用户选,还是保留 stdout 但增加 `--quiet` 抑制成功路径的 stdout
- 不在 one-shot 任务范围内展开

## [2026-06-19 04:15:00] [Session ID: CURRENT_SESSION] 主题: init_logger 走 stdout 修复完成 + 测试基础设施连带修

### 解决路径
- `src/main.rs::init_logger` 非 hidden 模式改走 `stderr()`,`src/hidden_mode.rs` 给 `LogTarget::Stdout` enum variant 加注释说明实际走 stderr(保留名字做向后兼容)
- 4 个 e2e 因为 log 路径变了需要修:
  - `tests/control_lanes::listen_local_interactive_should_reach_connect_control_lane` - listener 同时 pipe stdout+stderr,合流
  - `tests/control_pty::control_pty_detach_should_allow_later_attach` - attach stdout+stderr 合流
  - `tests/shell_pty::reverse_shell_should_run_with_tty_semantics` - 同上,改用 combined_output closure + wait loop
- `tests/zenoh_router_client` 的 24+ 现有测试只 pipe stdout 等 "zenoh router daemon ready" marker,改 `start_zenoh_daemon_with_config` 用 `sh -c "exec rdog ... 2>&1"` 兼容层,`exec` 让 rdog 替换 sh 进程避免孤儿,`2>&1` 把 stderr 合成到 stdout

### 状态
- 修完了,但暴露了一个更大的事实:rdog 仓库里大量 e2e 用 `log::info!` 输出做"启动就绪" sentinel,这是隐性契约。
- 后续推进正式规格或实现时,任何"输出路径"相关的改动都要先查 e2e 是否依赖 log 输出做 polling。
- "log target" 实际语义在 2026-06-19 之前是 stdout,改完是 stderr;enum variant 名字仍叫 Stdout 是历史 API 稳定妥协。

### 后续讨论入口
- `tests/zenoh_router_client.rs::start_zenoh_daemon_with_config` 的 sh wrapper 是临时兼容层,理想做法是统一改测试用 stdout+stderr 合流,然后把 sh wrapper 退役。
- `LogTarget::Stdout` 名字可考虑改名 `Stderr`,但需要先 grep 所有使用方做兼容性 audit。

## [2026-06-19 05:10:00] [Session ID: CURRENT_SESSION] 主题: sh wrapper 退役完成,Zenoh e2e 改用合流 buffer

### 解决路径
- 加 `start_zenoh_daemon_with_combined_output` helper,内部把 stdout+stderr 合流到一个 `Arc<Mutex<String>>`
- 重构 `spawn_output_collector` → `spawn_output_collector_to(reader, buffer)`,让多个 collector 写同一个 buffer
- `start_zenoh_daemon_with_config` 回退到直接 `Command::new(rdog_binary_path())`,去掉 sh wrapper
- 24+ Zenoh e2e 改用合流 buffer,不再依赖 stdout 上的 log marker

### 状态
- 全部完成
- 历史上 2026-06-19 04:15 那条"init_logger 走 stderr 测试兼容 sh wrapper"已经退役
- 后续任何 log 路径变化,改 `start_zenoh_daemon_with_combined_output` 内部就行,不用再动 24+ 测试

### 经验教训
- **同一文件多个相似 `impl Trait` 函数会触发 rustc 解析器 bug**: 两个相邻的 `fn f(reader: impl Read + Send + 'static,)` 定义,后一个的 `'static` 被误判为 char literal。改用显式 generic `<R: Read + Send + 'static>` 解决
- **临时兼容层(sh wrapper)会变成永久债务**: 一开始为了"少改测试"用的 sh -c "exec rdog ... 2>&1",现在必须退役,改成正式 API
- **批量改测试时识别变体**: 不同测试用 `buffer` / `daemon_buffer` / `first_buffer` / `second_buffer` 等不同变量名,需要分别处理

## [2026-06-20 22:00:00] [Session ID: omx-1781788115552-szl2hn] 主题: zenoh_router_client 测试集存在 pre-existing discovery 时序 flakiness

### 发现来源
- 在 `unixpipe` fast path 实施阶段跑 `cargo test --test zenoh_router_client` 做回归。
- 单跑某一个用例都 pass,全套跑会随机有 1 个 fail,失败用例每次都不同。
- 把 Cargo.toml 改回原样 stash 后再跑,同样 fail,同样随机用例。证明与 transport_unixpipe feature 无关。

### 核心问题
- zenoh_router_client.rs 在多 test 并发跑时,session discovery 在某些 case 还没 settle 就被下一个 test 触发。
- 当前失败信息是 `未找到目标 service: namespace=lab, target_name=ok3-XXXXX.lab`,典型 discovery race。

### 为什么重要
- 不能让 `unixpipe` fast path 实施的"已有测试不回归"被这条 flakiness 干扰判定。
- 这是 pre-existing,不属于本轮范围,但要让后续维护者知道:不要每次被这条 flake 拦下就误以为是新改动回归。

### 未来风险
- 如果 unixpipe 改动实际破坏了某个用例,可能因为这条 flake 被误判为"已知 flake",掩盖真正问题。
- 因此后续每条新加的 e2e 测试,优先跑单独用例 5~10 次确认非 flake,再加入测试集。

### 当前结论
- 当前事实: zenoh_router_client.rs 的 multi-test 并发有 ~4% 概率失败,失败用例不固定。
- 仍未确认的部分: 是否某条具体用例有更严重的退化(单跑 5 次都过,但并发跑挂)。后续要单跑每条用例 5 次确认基线。

### 后续讨论入口
- 排查方向: 给 `resolve_target` 的 liveliness get 加一个 retry(已在 `acquire_daemon_name_guard` 之后的 watch),或在 test helper 里给每个 test 一个独立的 namespace 隔离,避免 cross-test liveliness 串扰。

## [2026-06-20 22:30:00] [Session ID: omx-1781788115552-szl2hn] 主题: Zenoh 1.8.0 的 `transport_unixpipe` 实际是 named pipe (FIFO),不是 Unix domain socket

### 发现来源
- Step 3 实施时查 `~/.cargo/registry/src/.../zenoh-link-unixpipe-1.8.0/src/unix/unicast.rs`。
- 看到 `PipeR::create_and_open_unique_pipe_for_read` 调用 `create(path_r, ...)`(来自 `unix_named_pipe` crate),以及 `split_pipe_path` 派生 `<base>_uplink` / `<base>_downlink` 两条 FIFO。

### 核心问题
- 我之前在 plan 里把 `unixpipe` 写成"Unix domain socket 内存连接",这是错的。
- 实际: Zenoh 1.8.0 的 `unixpipe` 是 **named pipe(FIFO)**,是文件系统上的两个 FIFO 文件 + tokio::fs::File 异步 I/O。
- 性能收益(本机避免 UDP loopback 的 Zenoh link 层开销)依然成立,因为 FIFO 比 UDP loopback 还是有可观的常数级优势,只是没有"in-memory pipe"那么夸张。

### 为什么重要
- 路径长度上限必须改:`<base>_downlink` 是 9 字节后缀,macOS `sun_path` 是 104 字节,所以 base 路径上限是 95 字节(不是之前 plan 里写的 100)。
- 已经把 `UNIXPIPE_SOCKET_PATH_MAX_BYTES` 从 100 改为 95,错误消息和单测都同步更新过。
- 不存在"in-memory IPC",plan 里"2~5x 提速"这个预估仍然合理,但理由要更新为"避免 UDP loopback 协议栈开销",而不是"避免 UDP 协议栈 + 文件系统 I/O"。

### 未来风险
- FIFO 是文件系统可见的,daemon 退出后 FIFO 文件会残留(因为它不是 socket,内核不会自动清理)。必须 stale cleanup,这点之前的 plan 已经覆盖。
- 极端并发下,FIFO 写满 buffer 会 block writer,这一点 Unix domain socket SOCK_STREAM 没这问题。本轮在 spec 层提一下,不展开。
- 后续如果对延迟还有更高要求,方向 B(直接 Unix domain socket 控制面)的价值更大了,因为它能真正 in-memory。

### 当前结论
- 当前事实: Zenoh 1.8.0 `transport_unixpipe` 实现是 FIFO,不是 Unix domain socket;性能收益依然成立(常量级)。
- 仍未确认的部分: 真实 ping round-trip 改善幅度,等 Step 5 跑 `time` 实测。

### 后续讨论入口
- 后续 plan: 方向 B(直接 Unix domain socket 控制面)10~50x 提速,把 Zenoh 完全跳过。
- 同步更新: `.omx/plans/zenoh-unixpipe-fast-path.md` 的"风险"和"ADR"两节,把"unixpipe = Unix domain socket"措辞改成"unixpipe = named pipe (FIFO)";`specs/zenoh-unixpipe-fast-path-plan.md` 同步修正。
