# 通过 rdog 向 Herdr 中的 OMP 下发 coding 任务

## 跨机下发方案与结论

2026-11-03 已通过一次受限端到端实测: macOS Terminal 发起 rdog 请求, Windows 本机 Herdr CLI 向已有 OMP 提交任务, 提交调用结束后再用新连接查询状态和实际回复. 环境、一次性授权例外和验证边界见文末实测记录. rdog 本身仍没有直接向 Herdr agent 发送 prompt 的命令. `rdog agent` 提供的是消息邮箱, 当前 CLI 使用 `EchoDecision`, 并没有 OMP 或 Herdr 适配器. [控制协议的 agent 命令类型](../src/control_protocol.rs#L132-L136), [agent 命令解析](../src/control_protocol.rs#L628-L635), [agent CLI](../src/main.rs#L598-L631).

上述实测没有使用 OMP 原生 RPC 或 ACP. 用户后续提出统一通讯入口的可能性, 并明确不要求把会话控制和任务执行拆成两套操作. 当前协议与 ACP 的评估见文末.

用户已实测 Herdr 自带的 SSH 远程入口无法连接目标 Windows SSH. 因此这条路线对当前目标不可用, 不建议继续重试. 这只排除了 Herdr SSH 远程入口, 不排除通过 rdog 在 Windows 本机调用 Herdr CLI.

本次实测复用了 Windows 已运行的 Herdr 0.9.5 和 OMP, 没有新增接收服务或启动独立 `omp -p`. 控制端使用 macOS Terminal, 避开已证实被系统拒绝局域网访问的 Herdr core 调用上下文. Windows 端显式指定真实 session `rais-dev`, 不使用默认焦点选择目标. 远端 Herdr skill 仍要求 pane 内调用, 用户为本次限定验证明确批准了外部调用例外. 该批准不是后续操作的默认授权, 实测没有伪造 `HERDR_ENV=1`.

如果上述 Herdr session 环境无法成立, 或不要求复用现有 Herdr OMP, 可以用 rdog `@spawn` 在目标机器直接运行 `omp -p`. 这会启动独立的非交互 OMP 进程, 不会把任务送进 Herdr 中已有的 OMP. `omp --help` 列出 `-p`, `--cwd`, `--profile`, `--mode` 和 `--max-time`. rdog 可用 `@task-status` 和 `@task-output` 查询运行状态与输出, 但任务 registry 只在 daemon 内存中. daemon 重启后旧 task ID 不再有效, 输出 buffer 上限为 1 MiB. 进程状态、退出码、task ID 和输出都不能单独证明代码任务通过验收. [任务控制实现](../src/task_control.rs#L1-L14), [spawn/status/output/cancel 分派](../src/control_core.rs#L87-L138).

`@pty` 适合需要终端交互的场景, 不等于任务队列, 且当前只在 Unix 主机实现. `@run` 的 `wait:false` 只返回 PID, 没有 task registry 查询或取消接口. [PTY 平台能力](../src/control_capabilities.rs#L209-L220), [@run 实现](../src/control_actions.rs#L2294-L2365).

## 安全与验收边界

Zenoh 认证与传输加密分别配置. [配置实现](../src/config.rs#L74-L104) 的认证默认开启, TLS 默认关闭. [TLS 规格](../specs/rdog-tls-plan.md#L7-L14) 说明 TCP TLS 的范围. 独立原始 TCP 控制入口走单独的监听路径, 不能自动继承 Zenoh 认证及 TLS. [TCP 控制监听](../src/daemon.rs#L297-L355). 当前 WebSocket 客户端只接受 `ws://`, 明确拒绝 `wss://`. [WebSocket scheme 检查](../src/control_transport.rs#L429-L451). 跨机部署必须核对实际启用的入口、认证、加密和授权, 不能把网络可达当作操作授权.

一次有效验收应分别确认目标机器和 Herdr session, OMP 对任务的实际响应, 以及任务要求的验证结果. `blocked` 需要保留给授权人处理. `unknown`、调用超时、进程启动成功或 Herdr 显示空闲均不能替代这些证据. 初始静态调查没有连接 Windows, 后续受限实测已验证提交和重新连接读取结果. 本次没有执行代码修改任务, 不代表 coding 任务的代码质量验收通过.

初始静态调研时, 本机只读核对得到 Herdr 0.8.0, rdog 3.0.0 和 OMP 18.15.0. 当时运行了 `herdr --version`, `herdr --help`, `herdr agent`, `herdr pane`, `herdr session`, `herdr api`, `rdog --version`, `rdog control --help` 和 `omp --version`, `omp --help`. 下文引用的 Herdr 源码固定到 `v0.8.0`, 仅保留最初调查依据, 不代表 Windows 后续实测版本.

以下部分保留 Herdr 0.8.0 的初始接口调查和当时的候选路线. 当前已验证结果以文末的 Windows 0.9.5 实测记录为准. 本机新版 `herdr-app` 与 `hc` 是另一套接口, 不能用于推定 Windows 独立版的命令.

## Herdr 0.8.0 接口细节

Herdr 将 session、workspace、tab、pane 和 agent 分层。远端任务需要先确定目标机器和目标 session，再在该 session 中发现 agent 名称或 pane ID。不能把本机 pane ID、UI 当前焦点或 `omp` 这个种类名当作可靠的远端目标。[Herdr 0.8.0 skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L46-L76)

本机 Herdr 0.8.0 的顶层帮助列出 `--session <name>` 和交互式 `--remote <target>`，没有列出 `--machine`；本机 `herdr agent` 帮助则列出 `omp`。当前机器上另有一份较新的 Herdr skill，包含 `--machine`，但不能据此推断已安装的 0.8.0 支持该参数。`--remote` 是否能与具体非交互式 agent 命令组合，本次未验证。不要将交互式远程 attach 等同于已验证的跨机 CLI 转发。[本机 Herdr skill](/Users/cuiluming/.agents/skills/herdr/SKILL.md#L91-L103)

## 会话选择与外部访问边界

上游将 session 定义为独立的持久服务器命名空间。默认 session 与命名 session 不共享运行时命名空间。[上游 agent guide](https://github.com/herdrdev/herdr/blob/v0.8.0/website/agent-guide.md#L19-L25)

源码中显式 `--session` 优先于 socket 覆盖。没有显式 session 时，`HERDR_SOCKET_PATH` 优先于由 `HERDR_SESSION` 得出的路径。默认 API socket 位于配置目录下的 `herdr.sock`，命名 session 位于 `sessions/<name>/herdr.sock`。[session.rs](https://github.com/herdrdev/herdr/blob/v0.8.0/src/session.rs#L80-L100)、[socket 路径](https://github.com/herdrdev/herdr/blob/v0.8.0/src/session.rs#L157-L180)。API 客户端通过本地 socket 发送换行分隔的 JSON。[api/client.rs](https://github.com/herdrdev/herdr/blob/v0.8.0/src/api/client.rs#L13-L28)、[请求编码](https://github.com/herdrdev/herdr/blob/v0.8.0/src/api/client.rs#L148-L154)

Herdr 0.8.0 的 agent 操作 skill 要求先确认 `HERDR_ENV=1`。检查失败时，指示 agent 停止，不从 pane 外检查或控制聚焦的 Herdr session。[版本固定的 skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L8-L18)。这是该 skill 的操作限制，不等同于证明 socket API 在技术上拒绝所有 pane 外客户端。源码显示 API 客户端可选择本地 session 或显式 socket 路径，但本次没有运行外部访问实验。[api/client.rs](https://github.com/herdrdev/herdr/blob/v0.8.0/src/api/client.rs#L13-L28)

## OMP 识别与目标

Herdr 0.8.0 的检测枚举包含 `Omp`，标准标签和启动程序名均为 `omp`。源码也包含 Bun 包装的 OMP 前台进程识别测试，以及 `herdr:omp` 生命周期 hook 的识别分支。这些是源码与测试用例证据，并非本次运行时观察。[detect/mod.rs](https://github.com/herdrdev/herdr/blob/v0.8.0/src/detect/mod.rs#L41-L53)、[标签与程序名](https://github.com/herdrdev/herdr/blob/v0.8.0/src/detect/mod.rs#L115-L151)、[hook](https://github.com/herdrdev/herdr/blob/v0.8.0/src/detect/mod.rs#L283-L290)、[Bun 测试](https://github.com/herdrdev/herdr/blob/v0.8.0/src/detect/mod.rs#L905-L918)

`herdr agent` 的实际帮助说明，agent 目标是唯一的活动 agent 名称或当前承载 agent 的 pane ID。它不是 terminal ID，也不是 `omp` 等裸 agent kind。名字在 agent 退出、释放或被替换后清除。pane 跨 workspace 移动会获得新的 pane ID，旧 ID 不应作为通用目标继续使用。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L54-L68)。调用者在 pane 内可继承 workspace、tab、pane 环境变量；缺省目标可能指向 UI 焦点，因此不能依赖缺省焦点确定其他人的目标。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L70-L85)

## 启动、提交、读取与等待

以下是 0.8.0 本机 `herdr agent` 的只读帮助所列语法，不是在本次调研中执行的操作：

```text
herdr agent list
herdr agent get <target>
herdr agent start <name> --kind KIND --pane ID [--timeout MS] [-- <agent-args...>]
herdr agent prompt <target> <text> [--wait] [--until STATUS]... [--timeout MS]
herdr agent wait <target> [--until STATUS]... [--timeout MS]
herdr agent read <target> [--source visible|recent|recent-unwrapped|detection] [--lines N] [--format text|ansi] [--ansi]
herdr agent explain <target> [--json|--format text|json] [--verbose]
```

`agent start` 需要已有的空闲交互 shell pane，不创建布局。成功返回需要检测到预期 agent 并达到可输入状态，默认启动超时为 30 秒。原生 agent 参数放在 `--` 之后。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L108-L120)、[CLI 实现](https://github.com/herdrdev/herdr/blob/v0.8.0/src/cli/agent.rs#L272-L350)

`prompt --wait` 默认等待 `idle`、`done` 或 `blocked`。`--until` 和 `--timeout` 在 prompt 上要求同时使用 `--wait`。等待追踪的是生命周期状态而非单个任务轮次，agent 已在工作时，已有轮次的结束可能满足等待。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L122-L138)、[CLI 参数检查](https://github.com/herdrdev/herdr/blob/v0.8.0/src/cli/agent.rs#L645-L711)。`idle` 和 `done` 与是否已在 UI 中查看有关，`blocked` 表示检测到审批或提问，`unknown` 不证明完成。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L54-L58)

上游 0.8.0 skill 描述提交时会根据 bracketed-paste 模式发送文本及 Enter，并称非工作状态的提交需要在五秒内观察到生命周期变化，否则返回 `agent_prompt_stalled`。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L122-L130)。但 0.8.0 服务端实现先尝试发送文本，再延迟安排 Enter，之后返回结果。因此不应把成功响应解释为已确认 agent 接收任务或完成任务。[agents.rs](https://github.com/herdrdev/herdr/blob/v0.8.0/src/app/api/agents.rs#L62-L110)。`timeout` 或连接中断后的任务是否已送达属于未验证状态，不应直接重复提交。[推断]

读取时，`visible` 为当前视口，`recent` 包含软换行，`recent-unwrapped` 合并软换行，`detection` 是检测用底部文本。备用屏幕中移出视口的行不会进入普通宿主 scrollback，增加 `--lines` 不一定能恢复完整回复。[skill](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L147-L154)、[读取来源与限制](https://github.com/herdrdev/herdr/blob/v0.8.0/skills/herdr/SKILL.md#L175-L185)
## 初始 Windows 候选链路及源码依据

Herdr 0.8.0 的 API client 连接本机 session socket; Windows IPC 实现使用 named pipe. Herdr agent prompt 会先解析目标 agent, 确认它仍是 pane 前台进程且已就绪, 然后向该 pane 提交文本并延迟发送 Enter. 因此可让 rdog 在 Windows 执行 Herdr CLI, 不必让 Herdr 自己通过 SSH 连接 Windows. [Herdr Windows IPC](https://github.com/herdrdev/herdr/blob/v0.8.0/src/ipc.rs), [本机 API client](https://github.com/herdrdev/herdr/blob/v0.8.0/src/api/client.rs#L13-L28), [prompt handler](https://github.com/herdrdev/herdr/blob/v0.8.0/src/app/api/agents.rs#L62-L110)

rdog Windows TCP daemon 通过 `Command::new` 启动隐藏常驻子进程; `@spawn` 使用配置的 shell 再创建任务进程. 这些路径未调用 `env_clear`, 也没有显式切换 Windows 用户或 session. [源码事实] 如果从 Herdr 管理的 Windows shell pane 启动 daemon, 子进程预期继承该 pane 的实际 Herdr 环境; 这一继承链和 named pipe 可达性尚未在目标机器验证. 从 Windows service、SSH 或 pane 外的计划任务启动 daemon 时, 不应假设它有 `HERDR_ENV=1` 或指向正确的 Herdr session. [rdog daemon 分支](../src/main.rs#L633-L690), [隐藏 daemon spawn](../src/hidden_mode.rs#L103-L136), [shell 创建](../src/control_actions.rs#L1047-L1087), [@spawn 任务执行](../src/task_control.rs#L164-L243)

确认调用方满足 Herdr 的 pane 内操作要求后, 可先只读调用 `herdr agent list` 和 `herdr agent get <unique-agent-name>`, 确认预期 session 与 OMP. 再考虑通过获授权的调用方执行 `herdr agent prompt <unique-agent-name> <task> --wait --timeout <ms>`, 并查询状态和输出. `@spawn` 追踪的是它启动的子进程, 不是 OMP 任务本身; 超时后先检查状态及 pane 输出, 不要盲目重复提交.

这是比新建自定义接收服务更小的候选路径, 但取决于目标 Windows 上 Herdr 版本、pane 环境、daemon 启动位置、同一用户/session 和 named pipe 权限. 上述内容是静态调查阶段的判断. 后续实测没有沿用 pane 环境继承方案, 而是在用户明确批准的一次性外部调用例外下完成验证.

## 初始调查阶段的未验证事项

以下条目保留当时的证据缺口. 已关闭的实测缺口和仍未覆盖的场景见下一节.

- 用户报告 Herdr 自带 SSH 远程入口无法连接 Windows SSH; 本次将此作为已确认的用户观察, 未重复尝试.
- 目标机器的 Herdr 版本、运行中的 server 版本、session 名、socket/pipe 路径、OMP 安装及实际检测状态均未检查.
- rdog 从 Herdr 管理 pane 启动后, Windows daemon 与 `@spawn` 子进程是否保留可用的 session 环境并获准连接 named pipe, 尚未在目标机器验证.
- 未验证跨机 rdog 传输、prompt 提交、agent 状态变化、响应完整性或故障恢复. 源码推断不能代替端到端结果.


## 2026-11-03 Windows 受限端到端实测

### 环境与授权

- 控制端为独立 macOS Terminal, 执行已安装的 rdog, 连接 `192.168.50.61:5555`. 同一 rdog 从 Herdr core 调用时仍被 macOS 拒绝局域网访问, 本次没有修改权限或修复 core.
- Windows 回执确认主机为 `RAIS-RAZER`, 执行账户为 `RAIS-RAZER\vdcoo`, 命令运行在交互 Session 1.
- 现有 Herdr 为 0.9.5, 服务器回执为 `running`, `endpoint_compatible: yes`. 进程启动参数指向命名会话 `rais-dev`. 默认 session 没有服务器, 没有按错误提示启动或附着新服务.
- 唯一已有 OMP 的名称为 `omp4`, pane 为 `w1:p1`, terminal ID 为 `term_65f485a8158731`, cwd 为 `C:\Users\vdcoo\workspaces\rais-smoke`. 投递前状态为 `idle`, `interactive_ready=true`, 可见界面也显示上一项工作已结束.
- 用户明确批准本次从外部调用 Windows Herdr CLI, 作为旧版 skill 的 pane 内调用要求的一次性例外. 没有伪造环境变量、代答审批或中断已有任务. 后续控制不能直接复用本次例外授权.

### 实际命令与结果

Windows daemon 的 PATH 未解析到 Herdr. 实测使用现有进程的绝对路径, 并通过 rdog `@cmd` 执行下列 CLI 操作:

```powershell
$h = 'C:\Users\vdcoo\.herdr\packages\standalone\releases\0.9.5-x86_64-pc-windows-msvc\herdr.exe'
& $h --session rais-dev agent get omp4
& $h --session rais-dev agent prompt omp4 '这是一次经用户授权的只读链路验证. 不要调用任何工具, 不要运行命令、读写文件或启动子代理. 只心算 2387 + 4619, 按 RDOG_CHECK_1e54c42cf1=<计算结果> 的格式只回复一行. 不执行其他任务.'
& $h --session rais-dev agent get omp4
& $h --session rais-dev agent read omp4 --source visible --lines 30
```

提交脚本在 `prompt` 前重新检查 agent 类型、terminal ID、agent session 身份、空闲状态和 `interactive_ready`. 检查不满足即停止, 不提交. 本次提交前 `completion_seq=5`, 状态为 `idle`.

任务只提交一次, 没有使用 `--wait`. 收到 `agent_prompted` 后, 提交用的 rdog 进程以退出码 0 结束. 随后用另一个 Terminal 请求重新建立控制连接, 查询同一目标并读取可见输出.

OMP 的实际回复为:

```text
RDOG_CHECK_1e54c42cf1=7006
```

提交文本没有包含数值答案. 重连查询确认 `completion_seq=7`, 状态回到 `idle`, `interactive_ready=true`. agent session 与 terminal ID 没有变化. 这里以实际回复和状态变化共同判断执行完成, 不把 `agent_prompted` 或空闲状态单独当作完成证明.

### 会话保护与覆盖边界

Windows Herdr PID `109648`、OMP PID `109356` 和 Session 1 在验证后保持不变. 本机原 OMP `t1b` 的前台 PID `91577` 仍为 `live/working`. 未重启 app、core 或 daemon, 未创建新的 Windows OMP, 未执行代码修改任务. 本次只向已空闲的目标会话增加了一轮经授权的验证对话.

该轮通过范围是短任务提交、提交控制连接退出、重新连接后的状态和实际输出读取. 当时没有验证长任务或代码改动验收, 后续 coding 实测见下一节. 断网恢复、进程或机器重启恢复、完整历史输出提取和重复提交幂等仍未验证. `completion_seq` 不是本次任务专属 ID, 不能单独用它判断任意后续任务的完成状态.

## 2026-11-03 长 coding 任务验证

- 用户追加目标: 沿用 Windows 上已有 OMP, 提交真实 coding 任务后结束 rdog 调用, 再重连查询状态和输出. 本次请求明确续授了这一限定验证的外部 CLI 调用, 不扩大为其他会话或常设控制权限.
- 范围与停止条件: 只在独占临时目录构建 Node.js 标准库 CLI, 不安装依赖或修改已有项目. 观察同一 OMP 在投递连接退出后继续工作, 最终独立运行产物和测试, 核对身份及已有进程保持不变后收尾. 不通过 sleep、重试循环或故意放慢程序制造长任务.
- 预检: `rais-dev / omp4` 为 `idle`, `interactive_ready=true`, `completion_seq=7`. terminal ID 与 agent session 延续上轮. Node 路径为 `C:\nvm4w\nodejs\node.exe`. 独占目录 `C:\Users\vdcoo\AppData\Local\Temp\rdog-coding-04110bcb08` 尚不存在. Herdr PID `109648` 和 OMP PID `109356` 仍在 Session 1.
- 当前状态: 已完成真实 coding 投递、断开后重连观察、最终产物独立验收和原进程身份核对. throughput checkpoint: 串行单次投递到已有 OMP, 独立控制调用采样状态, 最终验收读取和运行真实文件, 没有新增服务或另建 OMP.

### 任务与断开证据

任务为 Node.js ESM JSONL 事件汇总 CLI. 输入由 `eventId`, `taskId`, `kind`, `timestampMs` 构成, 按任务维护 `queued -> started -> succeeded/failed` 状态, 按事件 ID 去重. 要求实际实现、串行测试、CLI 正反例和中文 README, 不能用 sleep 或空转延长执行.

投递前重新检查同一 agent session、terminal ID、空闲状态及目录不存在后, 只执行一次 `agent prompt`, 不带 `--wait`. 本机记录投递调用在 `2026-11-03T05:21:16Z` 已退出, 退出码 0, 回执为 `agent_prompted` 和 `SUBMISSION_RETURNED`. 此后每次查询均为新 rdog 调用, 没有保持原提交连接.

- Windows `05:22:25Z` 快照为 `working`, 已有实现和测试文件, 可见输出显示正在运行串行测试和 CLI 示例. agent session 与 terminal ID 均延续投递前的身份.
- Windows `05:24:37Z` 快照为 `idle`, `completion_seq=9`, 可见输出包含 `rdog-coding-04110bcb08:DONE` 和测试摘要.
- 随后 agent 继续修正了无效 JSON 测试的临时目录隔离. 最终测试文件修改时间为 `05:25:06Z`, 控制端独立验收报告写入时间为 `05:27:12Z`, 因而独立验收覆盖修正后的文件. 最后可见输出再次包含同一完成标记, 收尾状态为 `idle`, `completion_seq=11`.

以上时间是采样证据, 不作为精确执行耗时或性能基准. 尤其不能把第一次 `idle` 或完成标记单独当作代码验收通过. `completion_seq` 的增加也不能解读为提交了多项任务.

### 独立验收

控制端通过新的 rdog 调用直接启动目标机 `C:\nvm4w\nodejs\node.exe`, 不要求 OMP 再次执行或自行确认结果. 实际 Node 版本为 `v24.4.1`.

- 实际重跑 `journal.test.mjs`, TAP 回执为 27 tests, 27 pass, 0 fail, 0 skipped, 退出码 0.
- 控制端另建输入, 用真实 CLI 进程检查 8 个场景. 正常交错任务与字段顺序不同的重复事件得到 6 个唯一事件, 两项任务分别为 `succeeded` 和 `failed`. 空输入正常返回零事件.
- 非法 JSON、额外字段、冲突事件、时间倒退、非法状态迁移均返回退出码 2, stdout 为空, error code 与物理行号符合契约. 缺失文件返回退出码 1, `IO_ERROR`, `line=null`, stdout 为空. 这些是故障输入的预期结果, 不是未处理失败.
- 独立输入及实际结果保留在 `C:\Users\vdcoo\AppData\Local\Temp\rdog-coding-04110bcb08\controller-acceptance-iuBmRE\`, 报告为 `result.json`. 实现、CLI、测试和 README 的最后修改时间均早于该报告, 未发现使本轮验收失效的后续文件修改.

可在 Windows 复核:

```powershell
$node = 'C:\nvm4w\nodejs\node.exe'
$root = 'C:\Users\vdcoo\AppData\Local\Temp\rdog-coding-04110bcb08'
& $node --test --test-reporter=tap --test-concurrency=1 (Join-Path $root 'journal.test.mjs')
& $node (Join-Path $root 'cli.mjs') (Join-Path $root 'controller-acceptance-iuBmRE\interleaved-duplicate.jsonl')
```

### 保护与限制

收尾查询确认 Windows rdog PID `63640`、Herdr PID `109648` 和 OMP PID `109356` 保持不变, 均在 Session 1. 同一 agent session、terminal ID 和 ready 状态保持有效. 本机原 OMP `t1b` 的前台 PID `91577` 仍为 `live/working`. 没有重启或替换已有会话.

一次状态查询因可见输出较多触发 `@run` 回执截断, 另一次可见文本存在编码失真. 后续显式设置 PowerShell UTF-8 输出, 并对状态字段和可见尾部设定预算后成功读取. 这两项是观察通道问题, 没有据此重发 coding 任务. 长回复不能仅依赖当前视口或无界 JSON 回执.

本轮通过的是分钟级真实 coding 工作在提交控制进程退出后继续, 以及重新连接查询和独立运行最终产物. 没有模拟网络中断或关闭 Windows Herdr, 也没有证明小时级任务稳定性、机器重启恢复或完整终端历史可恢复. 测试产物保留在上述独占临时目录, 不作为生产工具发布.

## 2026-11-03 当前协议与 ACP 统一入口评估

### 调查目标与证据范围

用户要先检查实现与协议边界, 再判断是否应采用 ACP. "rdog 控制 Herdr 管会话, ACP 执行任务"只是用户提出的候选方案, 不是约束. 本轮不修改产品代码, 不启用协作分享, 不生成控制链接, 不启动代理会话或投递任务. throughput checkpoint: n/a, read-only investigation.

Windows 只读运行 `C:\Users\vdcoo\AppData\Local\omp\omp.exe --help` 与 `acp --help`, 实际版本为 `18.15.0`, ACP 帮助退出码为 0. 本机帮助和安装包源码版本为 `18.16.0`. Windows 的 ACP 运行时行为没有做端到端测试; 以下明确区分帮助实测, 同版本公开源码和设计判断.

### 当前链路不是 OMP RPC 或 ACP

1. 控制端到 Windows daemon 使用 rdog 自有 line-control. 本次目标是 TCP `192.168.50.61:5555`, 消息为 `@run` 等请求与 `@response` 回执. rdog 也支持 Zenoh 等传输, 但传输不改变 Agent 语义. [协议规格](../specs/control-line-protocol.md), [命令类型](../src/control_protocol.rs#L67-L140), [传输抽象](../src/control_transport.rs#L49-L57).
2. Windows daemon 创建 PowerShell 或 Herdr CLI 子进程. `execute_run` 使用 `Command::new` 和 argv 执行程序, 返回退出后的 stdout/stderr 尾部, 不是 ACP 双向会话. [实际执行器](../src/control_actions.rs#L2334-L2408).
3. Herdr CLI 使用其本地 JSONL IPC API, Windows IPC 为 named pipe. Herdr server 将 prompt 编码为终端输入与 Enter, 通过 `queue_user_input_submission` 交给 pane; 读取操作使用终端快照. 它不是调用 OMP 的结构化任务 API. [Herdr 0.9.5 IPC client](https://github.com/herdrdev/herdr/blob/v0.9.5/src/api/client.rs), [Herdr prompt/read 实现](https://github.com/herdrdev/herdr/blob/v0.9.5/src/app/api/agents.rs).

因此"本地 API 是 RPC 风格"与"使用了 OMP 的 RPC 模式"是两件事. rdog 现有 `rdog agent` 绑定 `EchoDecision`, 并非 ACP adapter 或完整 LLM agent. [CLI 绑定](../src/main.rs#L598-L631), [消息类型](../src/agent_messaging.rs#L23-L71). 对 `src` 与 `Cargo.toml` 的 ACP, JSON-RPC 相关文字检索未发现现有内建 ACP 接线; 该负结果不代表外部脚本不能调用 ACP.

### OMP 已有两种结构化协议

- `--mode rpc` 是 OMP 自定义的 stdin/stdout JSONL, 请求使用 `{id, type}`, 例如 `type:"prompt"`. 它不是 JSON-RPC 2.0. `rpc-ui` 仍属该协议, 额外把工具 UI 交互转交客户端, 不等于远程附着已有 TUI. [18.15.0 RPC 类型](https://github.com/can1357/oh-my-pi/blob/v18.15.0/packages/coding-agent/src/modes/rpc/rpc-types.ts), 本机 `18.16.0` 的 `src/modes/rpc/rpc-mode.ts` 与类型声明提供 rpc-ui 边界.
- `omp acp` 是独立子命令, 不是必须出现在 `--mode` 帮助枚举中的选项. Windows 帮助明确说明它作为 ACP server 经 stdio 服务. 同版本源码的 `runAcpMode` 在连接关闭后进入 postmortem 清理和退出. [18.15.0 ACP stdio 实现](https://github.com/can1357/oh-my-pi/blob/v18.15.0/packages/coding-agent/src/modes/acp/acp-mode.ts).
- ACP 指 Agent Client Protocol, 其消息层使用 JSON-RPC 2.0. 因而 ACP 与广义 RPC 不是互斥选项, 但与 OMP 私有 `--mode rpc` 的 wire format 不同. 官方 v1 包括 `session/new`, `session/load`, `session/prompt`, `session/update` 与 `session/cancel`, 部分能力必须先协商. [官方 overview](https://agentclientprotocol.com/protocol/v1/overview), [session setup](https://agentclientprotocol.com/protocol/v1/session-setup).

### 两个不能靠更换协议名称解决的边界

**已有进程与存储会话不同.** ACP load/resume 恢复 Agent 管理的会话, 不是按 PID 或 Herdr pane 附着任意 TUI. OMP 本机源码中, ACP 首先查连接自己的 managed-session Map; 不存在时创建 AgentSession 再打开存储会话. `--resume` 的文件恢复同样不是原进程 attach. session lease 会阻止另一个进程混写原 journal, 非 owner 写入可能产生新 ID 的 continuation. 不能另起一个 `omp acp` 读同一文件后宣称仍在操作原来的 `omp4`. [18.15.0 lease 所有权规则](https://github.com/can1357/oh-my-pi/blob/v18.15.0/packages/coding-agent/src/session/session-manager.ts), 本机 `18.16.0 src/modes/acp/acp-agent.ts:1621-1657,1709-1743`.

OMP 另有 Collab, 源码显示它能让 guest 经 relay prompt/abort 同一个 host 会话. 这是值得利用的现有接缝, 但不是 ACP. 本轮没有检查当前分享状态, 创建控制链接或连接任何 Collab host. 因而不能将其描述为当前已可直接使用的 ACP 端点. 本机 `18.16.0 src/collab/host.ts:1-9,396-405`.

**操作者断开与 ACP peer 断开不同.** 现有 `omp acp` stdio peer 断开会进入退出流程; 本机 `18.16.0` Adapter 的 dispose 还会取消未结束的 prompt 并释放所管会话. 若要维持"rdog CLI 退出后继续工作", ACP 的持有者必须是远端长驻宿主, 不能是退出即关闭 stdio 的一次性调用. 现有 `@run` 不是此类宿主, `@spawn` 也没有对外的子进程 stdin/ACP 回调接线. [ACP stdio 源码](https://github.com/can1357/oh-my-pi/blob/v18.15.0/packages/coding-agent/src/modes/acp/acp-mode.ts), [rdog task spawn](../src/task_control.rs#L175-L237), 本机 `18.16.0 src/modes/acp/acp-agent.ts:3270-3340`.

### 建议统一入口, 不把整个主机控制面改写成 ACP

以下是基于源码的设计判断, 尚未实现或验证.

建议由 rdog 提供一个统一的远端 Agent 入口和身份映射, 让操作者不必在"控制 Herdr"与"向 OMP 提交任务"之间手工切换. Agent 的通用交互以 ACP 为目标, 复用现有 rdog 跨机传输, 认证和执行能力, 不将全部主机, GUI, 文件传输和终端管理改写为 ACP. ACP 的标准方法并没有覆盖这些通用主机管理语义; 用自定义扩展硬塞进去也不等于标准互操作.

保留当前"使用已有 OMP"的目标时, 统一入口必须绑定同一个活跃 AgentSession, 而不是在 Herdr 和 ACP 两边维护两个同名会话. OMP 已有内部 ACP Adapter, SDK 和 Collab 接缝可研究复用, 但尚未证实现有 TUI 有可直接附着的 ACP listener. 正式切换需要补足 OMP 侧的同进程接入, TUI 与 ACP 共同访问及会话所有权. 只在 rdog 加 `omp acp` 子进程不能满足这一点.

远端长驻宿主负责维持 ACP peer, 关联 session 与提交, 记录事件和处理重连查询. rdog 的短连接退出不应被直接转译为 ACP peer 关闭或 `session/cancel`. 若审批时操作者不在线, 应明确呈现待审批, 不能由 bridge 自动代答. 这些都是当前异步使用目标要求的宿主职责, 不是 ACP 自动提供的保证. [ACP security](https://agentclientprotocol.com/protocol/v1/security), [prompt/session 生命周期](https://agentclientprotocol.com/protocol/v1/overview).

当前官方稳定传输文档以 stdio 为标准, 自定义双向传输被允许. HTTP/WebSocket transport 仍在 RFD 阶段, 虽然官方 Rust SDK 已提供 HTTP crate. 不需要为引入 ACP 先替换 rdog 的跨机网络层; 真正需要验证的是双向请求, 权限回调, 断开后的所有权和输出补查. ACP v2 也仍为 Draft, 不能将 v2 的 prompt acceptance 与完成通知语义直接套用到 OMP 当前实现. [transports](https://agentclientprotocol.com/protocol/v1/transports), [HTTP/WS RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport), [v2 Draft 公告](https://agentclientprotocol.com/announcements/acp-v2-draft), [官方 Rust HTTP crate](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol-http/README.md).

下一步若批准实施, 首个验收应是"同一 OMP 实例和 session 通过 ACP 收到一项任务, 操作者连接退出后继续运行, 重连可查事件和实际输出, 期间 TUI 不被替换". 然后才做正式任务入口的切换和调用方迁移. 本轮没有实施该改造, 也没有把先前 Herdr 投递通过误算为 ACP 验收通过.

## 2026-11-03 herdr-core 集成与 ACP 的比较

### 结论与调查范围

用户追加了 rdog 集成 herdr-core 的方向, 要与 ACP 比较, 并未授权安装, 重启或迁移现有会话. 本轮只读运行 `hc --version`, `hc --help`, `hc --skill`, `hc schema`, `hc connect --help` 和 `hc server status --json`, 对照官方 `v0.1.17` 文档及源码. 本机安装版本和现有 core 均为 `0.1.17`; 服务 PID 仍为 `58295`, holder 包括 `99220` 和 `11697`. 没有创建终端, 发送输入或改变服务.

设计判断: 对当前优先目标, 即复用可观察, 可接管的交互式 OMP 并在控制端退出后继续执行, 优先对接 herdr-core 的进程外 API, 比先改造 OMP 的同进程 ACP 接入更直接. 这调整的是实施优先级, 不是推翻 ACP 作为结构化 Agent 协议的价值. core 和 ACP 不是同一层的替代品.

### 新 core 提供的真实能力

herdr-core 不负责绘制 Herdr 窗口. 其 API 是 JSON-RPC 2.0, 普通模式为逐行 JSON, Unix 使用本地 socket, Windows 使用 named pipe. `hc connect` 可在 stdio 原样承载该协议; 完整 attach 使用另外协商的 framed mode, 包含流控和尺寸通知. 本机 `hc schema` 实际列出了 `terminal.run/list/get/send/read/wait/attach/interrupt`, `events.subscribe` 和 `rpc.discover`, 没有 ACP 的 `initialize` 或 `session/prompt` 方法. [v0.1.17 API](https://github.com/herdrdev/herdr-core/blob/v0.1.17/schema/README.md).

core, holder 与 viewer 是不同进程. holder 托管终端程序, 关闭 viewer 不结束程序, 支持稍后接管同一终端. 子终端与 parent terminal 的关系是另一项生命周期规则, 需按 `outlive_parent` 明确配置, 不能把它等同于网络客户端连接的寿命. 计划内 core handover 可保留程序; 程序真正丢失后的 `keep` 恢复只启动新 shell, 不重跑原命令, 更不自动恢复 OMP 任务. [同版 README](https://github.com/herdrdev/herdr-core/blob/v0.1.17/README.md), [API 的 attach 与 keep 说明](https://github.com/herdrdev/herdr-core/blob/v0.1.17/schema/README.md).

官方发布清单有 Windows x86_64 构建. Windows 上的模板与 quoted shell command 需要 PATH 中有 `sh`, 不等于所有 Windows 本地 API 都要求 SSH 可用. rdog 可在目标机访问本地 core API, 不必采用 core 自带的 SSH 机器管理. 这是根据接口作出的集成可行性判断, 尚未做 rdog 到 Windows 新 core 的端到端实测. [官方发布清单](https://releases.herdr.dev/core/latest.json), [同版平台说明](https://github.com/herdrdev/herdr-core/blob/v0.1.17/README.md).

### 与现有 rdog 能力的重叠

rdog 已有 PTY detach/attach, 不能因发现 core 就宣称必须重写全部终端能力. 但当前 PTY 后端受 `cfg(unix)` 限制, capability 明确报告 Unix-only, 活动会话保存在进程内 Map. core 提供的 Windows 终端支持, 终端屏幕模型, 状态报告, 多 viewer 和 holder 分离, 对当前 Windows 交互式 Agent 场景有实际新增价值. [rdog PTY 能力](../src/control_capabilities.rs#L209-L220), [会话结构](../src/pty_control.rs#L93-L132), [现有 PTY 契约](../specs/pty-control-plan.md).

旧决策曾拒绝引入整体 herdr 代码, 原因是 PTY 重叠, API 未稳定和高权限 daemon 职责膨胀. 新 core 已是无窗口 runtime, 有文档化 API 和 Windows 支持, 不应把旧决定直接当作否决它的结论. 进程外 API 对接也不同于把源码嵌入 daemon. 这仍是新的方案评估, 不是未经批准修改原决策或自动扩大实现范围. [历史决定](../specs/rdog-task-spawn-control-plan.md#L296-L299).

### 三种集成形式不能混称

| 形式 | 获得的能力 | 主要代价与边界 | 当前判断 |
|---|---|---|---|
| rdog 调用已有 core API | 持久终端, 同一终端的人机接管, 状态和事件 | 需要 core 后端接线, 身份及权限映射; 仍是终端交互 | 当前首选 |
| 随 rdog 分发 core binary, 保持独立进程 | 可以统一安装入口, 不把 terminal engine 链接进 rdog | 负责版本选择, 更新, 平台资产, 签名与 notices; 不能擅自管理用户已有 core | API 路线验证后再决定 |
| 将 core 源码作为 Rust 库嵌入或 vendoring | 对内部运行时有更多控制 | 同时承担内部 API, Ghostty, Zig, submodule 和构建维护; 要避免两套进程所有权 | 当前不建议作为起点 |

core 有 `src/lib.rs` 和公开模块, 不能说它无法作为源码依赖. 但 workspace 为 `publish=false`, 未承诺稳定的外部 Rust library API. 文档化 JSON-RPC API 目前为 preview, generation 1, 并通过已发布 schema 检查破坏兼容的变动, 其边界比直接依赖内部 Rust 类型清楚. 源码构建包含 Ghostty submodule 和 Zig 0.16, 不是纯 Rust 单包依赖. Apache-2.0 本身不是否定集成的理由, 分发仍需核对第三方通知. [workspace](https://github.com/herdrdev/herdr-core/blob/master/Cargo.toml), [core manifest](https://github.com/herdrdev/herdr-core/blob/v0.1.17/crates/herdr-core/Cargo.toml), [公开模块](https://github.com/herdrdev/herdr-core/blob/v0.1.17/crates/herdr-core/src/lib.rs), [Zig 构建](https://github.com/herdrdev/herdr-core/blob/v0.1.17/crates/ghostty-vt/build.rs), [API 兼容约束](https://github.com/herdrdev/herdr-core/blob/v0.1.17/schema/README.md).

### 与 ACP 的职责比较

core 解决的是终端和进程宿主问题; ACP 解决的是 Agent 的结构化 session, prompt, tool update, permission 和 cancellation 协议问题. 两者虽都使用 JSON-RPC 2.0, 方法集合与会话语义并不相同. 调用 `terminal.send` 仍然是终端输入, 不会自动变成 `session/prompt`. core 的状态来自程序报告的 OSC 7501, `done` 不是代码质量验收, 也不是独立 task ID 对应结果的证明. [core status/API](https://github.com/herdrdev/herdr-core/blob/v0.1.17/schema/README.md), [ACP overview](https://agentclientprotocol.com/protocol/v1/overview).

如果当前任务只要求交互式 OMP 的发送, 等待和读取, core API 可以先形成完整闭环, 不必为了接 ACP 先修改 OMP. 若后续明确要求标准化工具事件, 精确的权限往返, 结构化增量或多 ACP Agent 互操作, 再把 ACP 加到同一个 Agent 入口, 并解决上一节指出的活跃 AgentSession 绑定与连接所有权. 不能把 `omp acp` 放入普通 PTY 就当作这两个协议已经合并.

若最终选择 core 作为交互式会话后端, 对应 PTY 和程序生命周期应只由 core/holder 托管, rdog 管跨机入口与授权. 不要同时让 rdog 的本地 PTY registry 和 core 管同一个进程. 普通短命令或非终端任务是否受影响, 按其实际使用链路判断, 不顺带重写.

### 当前 Windows 会话与下一步边界

已验证的 Windows `rais-dev / omp4` 属于旧 herdr 0.9.5, 不是新 core 的 terminal address. core 的 `terminal.attach` 解析自己的终端记录, 公开接口没有导入外部旧 pane, PID 或 PTY 的参数. 因而不能承诺不动现有会话就把它转成新 core 管理. 当前只读调查没有尝试迁移, 旧会话不应因选型被关闭或重启.

若批准进一步原型, 应先在隔离的新 core 终端中验证 rdog 的 run/send/wait/read 闭环, 客户端断开后同一 OMP PID 存活, 重连后可读状态和实际产物. 这只证明新 core 后端, 不等于完成旧会话迁移. 迁移窗口与现有会话处置需另行明确. core 集成也不会自动修复此前 macOS core 调用链的局域网权限拒绝.

## 2026-11-03 最小 Core 原型方案与风险

### 本轮交付与新增预检

用户要求继续调查, 先给方案和风险, 不是授权实施原型. 本轮没有下载或安装 core, 没有启动新服务或 OMP, 没有向已有会话投递任务. throughput checkpoint: n/a, read-only investigation.

Windows 只读预检返回 `RAIS-RAZER`: rdog daemon 的 PATH 未解析到 `hc.exe` 或 `herdr-core.exe`; 官方默认路径 `C:\Users\vdcoo\AppData\Local\Programs\herdr-core\herdr-core.exe` 不存在; 未发现同名运行进程. 这不排除自定义目录的安装, 但目前没有已确认可用的新 core. 默认安装路径依据[官方安装脚本](https://releases.herdr.dev/core/install.ps1), 本轮只读取脚本, 未执行.

本机 `hc 0.1.17` 的 help 与实际 OpenRPC schema 补齐了以下契约. `run --bg` 不等待整个任务, `--no-parent` 取消父终端归属, `send --if idle` 在状态不匹配时拒绝输入. `SendResult` 提供 `turn`, `started`, `status`, `wait`; `WaitResult` 提供 `ended`, `reason`, `turn`, `status`. `send` 即使不带 `--wait`, 也会在程序报告状态时短暂检测是否开始 working; `started=null` 表示无法由状态报告确认启动, 不能当作成功证明. [同版 input 实现](https://github.com/herdrdev/herdr-core/blob/v0.1.17/crates/herdr-core/src/input.rs), [API handler](https://github.com/herdrdev/herdr-core/blob/v0.1.17/crates/herdr-core/src/api.rs).

### 要验证的单一问题

只验证这条组合是否成立: 通过现有 rdog 远程执行入口, 让 Windows 本地 core 托管一个新的交互式 OMP; 原提交调用退出后, 同一 OMP 仍执行真实 coding 工作; 新 rdog 调用可读取状态和输出, 最后独立运行实际产物.

原型用一个独占临时目录, 一个隔离 core session, 一个 OMP 终端和一项 coding 任务. 不修改 rdog Rust 代码, 不新增协议 verb, 不嵌入 core, 不接 ACP, 不迁移旧 Herdr, 不实现二进制 framed attach 或全量事件订阅. 这验证进程外后端的可行性, 不冒充正式集成完成.

### 最小执行形态

复用已验证的 macOS Terminal 发起 rdog 的路径. Windows 上只放一个临时 Node.js smoke runner, 由 rdog `@run` 以结构化 argv 调用; runner 再以 `shell:false` 调用本地 `herdr-core.exe`. 不增加常驻 bridge. 真正长驻的是 core/holder 和 OMP, runner 与每次 rdog 调用都应退出.

选择 CLI 作为一次性试验载体, 是为了复用其本地 JSON-RPC 接线, 避免先实现原生 pipe client. 成功只证明这一调用组合, 不证明 `hc connect` 的透明双向隧道或 framed attach 已接入 rdog. 后续产品化才选择内建 Core API client, 不能把临时 runner 当成正式长期接口.

runner 的记录形状为一份 `run.json`, 保存 runId, host, coreSession, coreId, terminalAddress, foregroundPid, 测试 cwd, labels, sendTurn, 提交阶段, 原始回执路径和产物路径. 终端运行状态与代码验收结果分开记录. runId 是本次验证的关联标记, 不是 core 提供的幂等任务 ID.

每次 core CLI 的原始 stdout, stderr 和真实退出码先完整保存在测试目录, 再返回有界摘要. 不把 core 的完整 JSON 或屏幕长文本直接塞进 `@run` 的尾部回执. 同一 runner 只接受固定的准备, 查询, 单次提交, 等待, 读取, 验收和清理操作, 不提供任意 RPC 透传服务.

### 按三个可验收单元执行

1. **准备隔离 core, 不碰默认实例.** 得到实施授权后, 若仍无现成可用 binary, 从官方版本化发布物取 `0.1.17` Windows archive, 按 manifest 校验 SHA-256, 完整解压到独占临时目录, 保留 `conpty` 和 notices. 不执行安装脚本, 不改 PATH 或注册自启动. 每次子进程使用同一独占 `HC_SESSION` 和 scratch `XDG_STATE_HOME`/`XDG_CONFIG_HOME`, 清除可能继承的 `HC_SOCKET` 与 caller 终端身份, 不改系统或用户环境. 先验证该实例的 version, generation, coreId, PID, Windows Session 和必要方法, 再进入下一单元.
2. **启动一个新 OMP, 只投递一次.** 用绝对 argv 启动交互式 OMP, 对应 `run --bg --no-parent --no-notify --json`, 不使用模板或 quoted shell command, 不启用 `--keep` 或 `--rm`. 使用独占 cwd 和新的 `--session-dir`, 不传 `--resume` 或 `--continue`, 不复制旧会话. 可以使用已有 provider 配置, 但不能修改其设置或降低审批策略. 确认新终端归属, OMP foreground PID 和 idle 后, 用 `send --if idle --json` 提交一次上轮 JSONL 事件汇总 coding 题面, 换新目录和完成标记, 不复制旧实现或报告. 不加 `--wait`; 保存 `turn` 与启动检测回执后退出投递调用.
3. **新连接观察, 验收后精准收尾.** 新 rdog 调用先 get/read 同一终端, 至少观察一次投递连接退出后的 working 与相同 OMP PID. 后续用有界 `hc wait` 等状态变化, 再读取输出; timeout 只是 check-in, 不重发任务. 结合保存的 sendTurn, 本轮唯一完成标记和新产物判断归属. 用目标机 Node 独立执行真实测试及 CLI 正反例. 验收后只 kill/rm 登记的本次终端, 核对隔离实例内没有未知终端后, 才 stop 本次 core; 保留代码, 原始回执和报告. 对旧 Herdr/OMP 只比对执行前后的进程身份, 不发送输入或做迁移.

若程序未报告状态, started 为 false/null, 出现 blocked, PID 或 coreId 变化, 收到 lost/restarted, 或无法确认提交结果, 先保留证据并停止新增输入. 不能用 `--force`, 自动批准, 更换目标会话或重发 prompt 来让原型变绿. 若工作过快, 未捕获断开后的 working, 只能记录投递与结果读取通过, 不能勾选长任务持续运行证据.

### 验收与停止条件

- 隔离实例的身份, 终端地址, labels, OMP PID 和 cwd 前后一致; 没有依赖保留终端地址后启动的新 shell 伪装原 OMP 存活.
- 原投递 runner 与 rdog 进程确实退出, 之后的新连接仍观察到同一 OMP working. 不依靠 sleep 或人为慢循环制造长任务.
- 终态有本轮唯一标记与实际代码产物; 控制端独立运行相关测试和 CLI, 错误输入的退出码与零 stdout 等行为符合题面. 不复用上一轮的 27 项通过数作为新结果.
- 旧 Herdr/OMP 没有被本次操作替换或重启; 全部资源处置限定在登记的原型实例. 临时 runner 和新 core 的清理记录完整, 测试产物按约定保留.

全部满足即结束原型, 再提交是否产品化的判断. 不追加小时级 soak, core 重启, 操作系统重启, 旧 pane 导入或 ACP 接入作为本轮完成门槛.

### 主要风险与处置

| 风险 | 影响 | 原型中的约束与证据 |
|---|---|---|
| 隔离环境或所有权错误 | 误操作用户默认 core, 旧终端或旧会话 | 每次调用固定 session 与 scratch homes, 检查 coreId/labels/PID; 启动不继承 parent, 不覆盖已有目录, 清理前再次确认归属 |
| 启动与连接寿命被混淆 | 只证明 CLI 退出, 未证明 OMP 仍运行; keep 恢复掩盖进程丢失 | core/holder 持有程序, 不开 keep; 记录进程关系, 新连接采样同一 OMP, 明确阻止把超时与重启当作成功 |
| 重复投递与 turn 误关联 | 断线后重复执行任务, 把旧 idle 或其他轮次当成本次完成 | 输入前登记提交意图, 一次发送, 回执未知转为 unknown; 保存 sendTurn, 不依赖换 caller 后的裸 wait 自动关联; 独占终端和唯一标记共同取证 |
| 输出截断或错误被吞 | 丢失 core JSON 头部, 将 native 失败当成正常结果 | 保存原始 stdout/stderr/退出码, 返回有界摘要, UTF-8, 屏幕读取不替代落盘代码验收 |
| 部署与远程信任边界 | core API 等同该账户的 shell 权限; 临时 session 不是安全沙箱 | 不暴露 core 新网络端口, 只复用已授权 rdog 通道; 不假定 raw TCP 自动继承 Zenoh 认证, 不自动升级或放宽权限; 校验和不冒充独立签名 |

本机 Herdr core 调用链的局域网权限拒绝仍是独立限制; 本原型使用已验证可用的 Terminal 入口, 不能据此声称该权限问题已修复. Windows 新 core 与旧 Herdr 的无损迁移也不在本原型保证内. [core 安全与隔离说明](https://github.com/herdrdev/herdr-core/blob/v0.1.17/docs/security.md).

## 2026-11-03 隔离 Core 原型实测

### 目标, 范围与终态

用户批准执行上节的进程外 Core 原型. 目标是证明 rdog 投递连接退出后, 同一 Windows OMP 继续 coding, 新连接能查询状态和输出, 控制端可以独立验收. 不操作已有 Herdr/OMP 会话, 不把本轮扩大为 ACP 接入或 Rust 库嵌入.

本轮已完成投递, 断开后重连, 独立代码验收和归属限定的清理. throughput checkpoint 为单一控制者串行调用, 一次提交意图, 一个新 Core 终端和一个新 OMP 主会话, 每次原生调用保存完整回执; 不保持提交连接等待 coding 完成. 原型代码保留用于复核, 不作为生产功能发布.

### 部署与保护基线

- 目标为 `RAIS-RAZER`, 复用 `192.168.50.61:5555` 上已有 rdog. `Get-NetTCPConnection` 确认监听进程为 PID `63640`.
- 独占根目录为 `C:\Users\vdcoo\AppData\Local\Temp\rdog-core-9fe0203057`, session 为 `rdog-proto-9fe0203057`. 下载 [0.1.17 固定清单](https://releases.herdr.dev/core/0.1.17/manifest.json) 指向的 Windows 便携包, SHA-256 与清单一致. 保留 `conpty` 等包内文件, 未执行安装器, 未改变 PATH 或注册常驻服务.
- 子进程环境来自该目录的 `.envrc`, 固定 `HC_SESSION`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, 清除继承的 HC/旧 Herdr 调用者身份. 没有设置用户级环境变量. 隔离不是安全沙箱, 仍使用当前账户的正常权限.
- 新 coreId 为 `a0372a252606590f84e1e4b52b09d87a`, core PID `106788`, holder PID `107676`, 终端为 `t1`, OMP 主进程和前台 PID 均为 `110304`. 实际 Node 为 `v24.4.1`.
- 保护基线包含 rdog PID `63640` 和 `103452`, 旧 Herdr PID `109648`, 旧 OMP PID `109356`. 启动后与清理后均核对 PID, ExecutablePath, ParentProcessId 和 SessionId 一致. 未向旧 `rais-dev / omp4` 输入任何内容, 未重启旧应用.

### 真实执行与断开证据

实际链路为 rdog `@run` 启动短命 Node runner, runner 调用便携 `hc` CLI, CLI 通过本地 Core 控制面操作私有终端, Core 的 PTY 承载交互式 OMP. 没有启动 `omp acp` 或 OMP RPC 子进程.

OMP 通过 `run --bg --no-parent --no-notify`, 独占 cwd 和 `--session-dir` 启动. 未使用 keep/rm 自动恢复选项, 未恢复旧 journal, 未放宽审批设置. 身份检查使用 coreId, core PID, terminal address, label, 创建时间, argv, cwd 和 OMP PID, 不能用地址还在来冒充原进程存活.

任务为全新目录中的 Node ESM JSONL 事件汇总 CLI, 需求沿用上轮状态迁移, 语义去重, 错误行号和 stdout/stderr 契约. 没有复用上轮实现或通过 sleep 延长任务.

- `send.intent` 先以独占创建方式落盘, 随后只发送一次, 不带 `--wait`. 原生 `hc send` 回执为 `started=false`, `status=idle`, `turn=0`, 原生退出码为 0. runner 停在未确认启动的保护断言, 没有重发.
- 提交控制进程 PID 为 `108244`. 其原生发送回执的 Windows 时间为 `08:51:48.253Z`. 控制端确认该次 rdog 调用退出后, 才发起新查询.
- 新查询进程 PID `10488` 在 Windows `08:52:24.217Z` 观察到同一 `t1`, 同一 OMP PID `110304` 和前台 PID `110304`, 状态为 `working`, `turn=0`. 之后的新查询仍能读取进度及实际代码文件.
- 最终 Core 任务状态为 `done`, `turn=1`, 终端生命周期仍为 `live`, OMP PID 没有变化. 本次主 journal 的 assistant 最终消息 ID 为 `93d2164d`, `stopReason=stop`, 正文包含 `rdog-core-9fe0203057:DONE`.

时间戳只作为各主机的采样记录, 不用于跨主机耗时或性能比较. 断开先后以提交调用退出后才发出新查询的程序顺序为依据. 屏幕中出现 nonce 本身不代表完成, 因为输入回显也含该文本; 验收使用主 journal 的 assistant 最终回复, turn 变化和真实产物.

### 控制端独立验收

控制端没有让 OMP 自报结果作为验收. 新 rdog 调用直接运行独立 Node 检查器, 实际重跑 `journal.test.mjs`, 得到 24 tests, 24 pass, 0 fail, 0 skipped, 0 cancelled, 退出码 0.

检查器另建输入并启动真实 CLI, 通过 8 个场景. 正常交错任务和字段顺序不同的重复事件得到 6 个唯一事件; 空输入得到零事件. 非法 JSON, 额外字段, 冲突事件, 时间倒退和非法迁移均返回约定的退出码 2, error code, 物理行号及空 stdout. 缺失文件返回退出码 1, `IO_ERROR`, `line=null` 和空 stdout. 这些错误是预期负例, 不是验证失败.

验收前后六个实现/测试/文档/示例文件的 SHA-256 一致, 验收后再查同一 OMP 仍为已收敛状态且 turn 未变. 清理后读取文件摘要, 确认哈希仍一致, 修改时间不晚于验收报告.

Windows 可直接复核, 不需要重启 core:

```powershell
$root = 'C:\Users\vdcoo\AppData\Local\Temp\rdog-core-9fe0203057'
& 'C:\nvm4w\nodejs\node.exe' (Join-Path $root 'independent-checker.mjs') (Join-Path $root 'work')
```

原始报告为 `work\controller-acceptance-ULXNhT\result.json`, 同目录保留完整 TAP stdout/stderr 和独立输入. 此命令会另建一份验收目录, 不覆盖本轮报告.

### 原型错误分类与协议边界

- runner 曾直接比较 PowerShell 与 Node 主机名的大小写, 在任何 core 启动之前失败. 已在主机身份边界按不区分大小写比较, 随后完成真实启动与全部后续验证.
- 被证伪的共同前提是 "`hc --json` 的 CLI 结果等同 `hc schema` 描述的 RPC 对象". 实际 `run --bg` 返回启动摘要, `ls` 返回无 labels 的列表摘要. runner 已改为从 `get` 的完整记录登记和验证身份, 列表只用于检查是否出现未知地址. 未通过 optional 字段兜底放宽归属检查, 未重复创建 OMP.
- Core 任务状态包含 `done`, 不等于终端进程 `finished`. 原型中只接受 `idle` 的验收断言已修正, 实测在 `live/done` 下独立验收成功. 状态类型真相源为本轮保存的 `hc schema` 和 [Core API 源码](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/crates/herdr-core/src/api.rs), 不能照搬旧 Herdr 的归一化状态.
- `started=false` 在本轮没有表示未投递. 后续 `working` 与最终产物证明任务实际运行. 未确认回执只能触发只读核查, 不能据此重发或将初次保护断言算作任务失败. 本轮未定位该首次启动观察未命中的内部原因.
- 一次较长报告的 `@run` 回执头部被截断. 完整文件已落盘, 后续只读已有文件的有界摘要, 没有重写报告或重复清理. 本机控制通道仍出现已知的 Zenoh session-close 日志, 但原生命令退出码和结构化窗口动作均成功; 本轮没有扩大修复此日志.

这些证据支持进程外 Core 持有会话的可行性, 但不把 CLI 摘要当成稳定 RPC schema, 不把短时开始观察当成完整任务生命周期协议.

### 清理, 产物与限制

只清理私有 `t1` 和私有 core. 先核对完整身份与未知终端, 登记进程树, 再关闭, 删除本次终端记录并停止私有 core. 清理审计确认本次 core, holder, OpenConsole, OMP 主进程及其运行时子进程均无残留, 四项保护基线保持一致.

根目录保留 `runner.mjs`, `independent-checker.mjs`, `.envrc`, `config.json`, `prompt.txt`, `run.json`, `controller-summary.json`, `ops\` 原始 stdout/stderr/真实退出码, 本次 OMP journal 和 `work\` 代码. 本机控制端另保留 `local://core-proto-controller-evidence.json` 与 `local://core-proto-window-cleanup.json`. 已清理 28 组本次本机诊断文件; 窗口按精确随机标记定位并仅用 graceful close, 最终 fresh 查询为 complete 且本次前缀匹配数为 0, 没有调用 app 终止路径.

本机共享 core PID `58295`, coreId `bdd8dc6614b1ff608ddbcadb4c157c8e` 和当前 OMP 前台 PID `91577` 的只读核对保持一致. 其他终端数量不作为不变项, 不处置其他活动.

本轮没有修改 rdog Rust 生产代码, 没有完成 ACP 接入, core 重启恢复, 小时级 soak, 旧 pane 导入或网络故障注入. 本机原 Herdr 调用链的局域网权限问题仍未解决. 当前交付与必要验收已满足, 停止原型执行, 不据此自动进入产品化.

## 2026-11-03 活跃 OMP 接口与 rdog 原生宿主评估

### 本轮问题与证据等级

用户要求继续调查活跃 OMP 接口, 与 Core API 比较, 并评估不依赖 Herdr/core、让 rdog 承担后台 OMP 宿主的可行性和代码借鉴范围. 本轮交付是源码与实际 CLI 帮助支持的选型结论, 不是宿主实现. throughput checkpoint: n/a, read-only investigation.

实际运行本机 `omp --version`, `omp --help`, `omp collab --help`, `omp ps --help`, `omp session --help`, `omp acp --help`, 均正常退出. 本机版本为 18.16.0. 安装包包含源码与 SDK 导出; 另核对公开固定 `v18.15.0` 的 RPC client 和 Rust SDK, 对应此前 Windows 二进制的版本. 没有连接任何 Collab host/relay, 获取分享链接或凭据, 开启分享, 启动新的 OMP, 投递任务或改变现有会话.

以下以源码事实和设计判断为主. 上轮 Core 端到端结果仍有效, 但本轮没有实测 Windows 的 Collab、私有 RPC 或 ACP 宿主. 不把源码存在或 help 可用当成后台宿主验收通过.

### 活跃会话接口确实存在, 但不都适合任务托管

1. **Collab 能控制 host 的同一个活动 AgentSession.** Host 将 guest prompt 交给自身 `ctx.session.promptCustomMessage`, 显式 abort 才调用 `ctx.session.abort`. Guest 离开只清理 peer/UI 状态, 不取消 host 任务. Host TUI 退出仍会 dispose 其 session, 因而 Collab 不是独立的保活宿主. 已安装 `18.16.0 src/collab/host.ts:1004-1069,1099-1108`, `src/modes/interactive-mode.ts:8100-8135`, `src/session/agent-session.ts:6384-6415`; [公开 18.15.0 host](https://github.com/can1357/oh-my-pi/blob/v18.15.0/packages/coding-agent/src/collab/host.ts).
2. **Collab 是协作协议, 不是 ACP, 也不是完整 task API.** 默认 autoStart 为 off, 默认经 WebSocket relay 协作, relay 可配置. Control/view 权限不同, host 限定一个 writer. Guest prompt 是 fire-and-forget, 没有按单个 prompt ID 关联的 accepted/finished 回执. 重连重新取得会话 snapshot, 不等于带持久游标的任务事件查询. UI 提问/审批有部分转发, 不能当作任意 UI callback 都完整远程化. 已安装 `src/collab/{settings,protocol,guest,host}.ts`, `src/modes/controllers/extension-ui-controller.ts:640-797,969-1155`.
3. **本地 discovery IPC 只有 snapshot/link 操作.** 它不是隐藏的本地 prompt API. `omp collab list --json` 也不是严格无副作用的文件读取, 会扫描共享 registry、认证查询 host, 并可能清理 malformed/stale metadata. CLI 没有 workspace/profile 过滤项. 因本轮保持严格只读, 没有执行 list, 也没有读取真实 registry token. 已安装 `src/collab/registry.ts:254-333,583-650`, `src/cli/collab-cli.ts:46-103`.
4. **预先加载的 Extension 可以访问同一 TUI session 的回调.** 公共 API 有 sendUserMessage、abort 和 agent/message/tool 事件, 但没有因此自动获得一个跨进程 listener. 自建本机 ingress 扩展是可行的设计接缝, 需要代码在当前进程中已加载; 不能给任意运行中的 TUI 动态附着. SDK `createAgentSession` 创建新 session, 不是查找另一个进程的对象. 已安装 `src/extensibility/extensions/types.ts:315-350,1795-1870,2038-2064`, `src/modes/controllers/extension-ui-controller.ts:238-290,1469-1500`, `src/sdk.ts:1919-1955,5374-5410,5861-5882`.
5. **`omp ps` 和 session lease 不替代任务接口.** OMP 自带 broker 可监督新子进程, 包含 PTY/ConPTY 路径; `ps` CLI 主要提供 list/info/logs/stop/kill/restart, 没有向某个 AgentSession prompt 的操作. `session lease` 只探测存储会话写入归属, 不取得 lease 或 attach. 已安装 `src/launch/{services,broker}.ts`, `src/cli/ps-cli.ts:1-150`, `src/session/session-storage.ts:625-669`.

### 关键新增发现: 已有官方 Rust RPC SDK

OMP 提供原生 Rust `omp-rpc` SDK, 不止 TypeScript `RpcClient`. 固定 [18.15.0 SDK README](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.15.0/sdk/rust/omp-rpc/README.md) 与 [client.rs](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.15.0/sdk/rust/omp-rpc/src/client.rs) 显示:

- `Client::spawn` 用 stdin/stdout pipes 启动 `omp --mode rpc`; `Client::from_io` 可接入调用方已有的 reader/writer. SDK 持有 writer 线程和协议 reader, 提供事件 receiver. 这是独立后台 OMP 的客户端, 不是任意现存 TUI 的 attach API.
- `call(PromptCommand)` 对应 admission 回执. `prompt_and_wait` 在写入前登记 collector, 等待与同一 request ID 对应的 `prompt_result`, 不用其他轮次的 `agent_end` 猜测完成. 状态、历史、host tools/URI 和 extension UI response 有已有接缝.
- OMP 同版 TypeScript client 还有 `session_settled`, 表示不只是当前 agent 暂停, 而是没有仍可能唤醒它的后台工作; `getState`, `getEntries(since)`, `getMessages` 提供结构化观察. [18.15.0 RPC client](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.15.0/packages/coding-agent/src/modes/rpc/rpc-client.ts).
- SDK 的 close/drop 会关闭 OMP stdin 并收尾进程. Windows 分支只终止 leader, 不是整棵进程树的完整保证. 所以客户端对象必须归长期宿主所有, 不能归某次短连接所有. 已安装 RPC runner 在 stdin EOF 后清理 side channels、drain 并 dispose/exit, 也不能把 EOF 当 detach. `src/modes/rpc/rpc-mode.ts:2944-2965`; Rust `client.rs:744-786,986-1040,1105-1108,1163-1174`.

SDK [manifest](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.15.0/sdk/rust/omp-rpc/Cargo.toml) 为 MIT, `0.1.0`, `publish=false`, 采用 serde/serde_json 和 Unix libc, 不是一个已承诺稳定外部版本的 crates.io 依赖. 可以研究固定版本的源码依赖或许可合规的引入, 本轮没有添加依赖或生成 SDK 文件. 它的 protocol v2 是 OMP 私有 RPC 的分帧协商, 不是 ACP v2.

ACP 仍有标准化互操作价值, 但现有 `omp acp` 通过 stdio 管理自己的 sessions, 同样需要远端长期宿主持有连接. 对只接 OMP 的首个后台宿主, 原生 RPC 已有更直接的任务关联和 Rust 代码可复用; 若确定接入其他 ACP agents, 再比较统一 adapter 的价值. 不把全部主机控制 API 改写为 ACP.

### rdog 现有基础与剩余职责

当前 rdog 已有跨机控制、后台普通任务 registry、输出捕获和取消. 但不能直接用 `@spawn omp acp` 代替 AgentHost:

- `TaskEntry` 记录一个 shell 子进程, 没有可供调用方写入的 stdin peer. stdout/stderr 被捕获为输出尾部, 不是 ACP/RPC 双向会话. Registry 和输出在内存中, 没有完整事件游标或重启恢复. [当前数据结构](../src/task_control.rs#L73-L155), [spawn 路径](../src/task_control.rs#L175-L237).
- 普通 task 的终态按子进程退出决定; 长驻 OMP 可以执行多个 prompt, 某个 prompt 完成不能意味着 OMP 退出. 需要分别建模 Agent 会话、一次请求及宿主进程, 而不是向普通 shell task 增加几个混用状态. [waiter](../src/task_control.rs#L245-L307).
- 当前 PTY 后端被 `cfg(unix)` 门控, Windows open/attach 等入口未实现. 但这不是库不支持 Windows. 项目已有 `portable-pty 0.9.0`, 只是依赖也位于 Unix target 下; 本机该库 `src/win/conpty.rs` 已实现 ConPTY open/read/write/resize/spawn. 因而 Windows TUI 接入不必从零手写 ConPTY FFI. [依赖门控](../Cargo.toml#L77-L88), [现有 PTY imports](../src/pty_control.rs#L93-L120).
- `portable-pty` 的 Windows killer 使用 `TerminateProcess`, 不能当作整树清理保证. rdog 当前 shell task 取消使用 `taskkill /T`, 未用 Job Object 托管该任务树; Windows system crate 的 JobObjects feature 已声明. [现有进程组与取消实现](../src/control_actions.rs#L869-L945), [依赖](../Cargo.toml#L87-L88). 本机库源码 `portable-pty-0.9.0/src/win/mod.rs:41-79`.

设计判断: rdog 原生 AgentHost 对新建 headless OMP 是合理且范围可控的方向. 它不需要终端屏幕引擎, 但仍必须持有双向协议、持续消费事件、保存请求关联和必要历史、保留离线审批状态、区分 abort prompt 与 close session, 并精确清理自己拥有的进程树. 不能在同步 control 请求里一直 `prompt_and_wait`, 否则又占住同一控制连接.

### 比较结论与实现难度

下表难度为基于代码责任的设计判断, 不是工期或性能测量. 表中原生宿主路线的最新验收见后文无 Core 原型实测; ACP 与 Collab 的 Windows 端到端仍未验证.

| 路线 | 适合的使用结果 | 主要缺口或代价 | 当前取舍 | 最后复核 |
|---|---|---|---|---|
| rdog 对接 Core API | 原样交互式 OMP, 屏幕观察和人机接管 | 新后端身份接线; terminal turn 不是精确 task 完成 | 上轮已通过隔离 Windows 原型, 保留为 TUI 路线 | 2026-11-03 |
| rdog 原生宿主 + OMP 私有 RPC | 新建后台 coding 会话, 按请求查完成和结构化历史 | 中等, 复用 Rust SDK, 补宿主/事件持久化/审批/Windows 树清理 | 本文后续隔离原型已通过断开、重连与独立产物验收, 不依赖 Core | 2026-11-03 |
| rdog 长期宿主 + ACP | 标准化多 Agent session/prompt/tool/permission 交互 | 中等, 仍需宿主和断开语义, OMP 专属能力需逐项核对 | 不否定标准协议, 不作为去掉 Core 的强制前提 | 2026-11-03 |
| rdog 接 Collab | 控制已开启协作的同一个 TUI session | relay/授权/单 writer/请求关联, host 仍需其他保活机制 | 现成活跃接口, 不是后台宿主的完整替代 | 2026-11-03 |
| rdog 重做完整 Herdr/Core 终端职责 | 自有 Windows TUI 托管、屏幕、attach、多 viewer 和升级保活 | 明显更大; ConPTY 只是其中一环 | 不为后台 coding 目标重做整套终端引擎 | 2026-11-03 |

这细化了此前 Core 优先的判断, 不是推翻 Core 原型. 保留原样 TUI 时, Core 仍直接; 由 rdog 创建新的 headless OMP 时, 原生 RPC 更接近产品所需的数据和行为. 已有旧 Herdr/TUI 不能用启动另一个 RPC/ACP 进程、读取同一 journal 的方式无损接管.

### 借鉴 Herdr 哪些部分

值得借鉴的是宿主机制, 不是复制整个应用:

- **进程拥有者与观察者分离.** [holder.rs](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/crates/herdr-core/src/holder.rs) 持有程序和 PTY, 总是读输出, 把慢观察者与输入阻塞隔离. headless 宿主可借鉴这一所有权和背压原则, 不必同时引入屏幕解析器.
- **区分 handover、shutdown 与连接断开.** [holder_proto.rs](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/crates/herdr-core/src/holder_proto.rs) 明确区分进程退出原因. 若以后要求 rdog daemon 升级/重启仍保留同一 OMP, 必须有独立 AgentHost/holder 持有其管道, 不能只依赖 daemon 内存对象或 PID 重连.
- **Windows 的受控启动与精确树清理.** [windows_exec.rs](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/crates/herdr-core/src/platform/windows_exec.rs) 使用 suspended spawn、加入 Job 后 resume; [windows.rs](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/crates/herdr-core/src/platform/windows.rs) 还处理 detached 启动、句柄继承和 ConPTY. 可借鉴可分离的平台机制, 不能按程序名终止所有 OMP.

控制端断开继续运行与 daemon 重启继续运行是两档保证. 前者可由长期持有 SDK/stdio 的宿主实现; 后者需要独立进程和可重新连接的本地宿主协议, 不能从本轮静态调查直接宣称已具备.

旧 [Herdr 0.9.5 LICENSE](https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/LICENSE) 与 [Core 0.1.17 LICENSE](https://raw.githubusercontent.com/herdrdev/herdr-core/v0.1.17/LICENSE) 都是 Apache-2.0. 实际借用须保留许可、版权/NOTICE 和修改说明, 并核对被带入的第三方部分. 对 headless OMP 没必要一并引入 Ghostty、Zig、完整 viewer/终端渲染或重复的跨机传输.

### 建议的下一个判别原型

若后续批准施工, 首个新原型应去掉 Core, 在独占目录用 rdog 所拥有的 AgentHost 长期持有新建 `omp --mode rpc`, 复用或对照固定版本 Rust SDK. 验证单次请求 ID 从 admission 到 `prompt_result`, CLI 退出后同 PID 继续、重连能补查输出, 背景工作与 `session_settled` 的区别, 未确认发送不重发, 审批缺席不自动允许, 最终仅清理本次进程树. 保持现有 OMP/Herdr 不动.

这只是下一轮建议, 本轮没有实施. 若要求立即接管当前 TUI 或保证 daemon 升级不断任务, 需分别纳入活跃会话接缝或独立 holder 的验收, 不能用新建 headless 会话的成功替代. 当前调查结论与边界已齐, 不追加服务、分享开关或新实现.

## 2026-11-03 无 Core 的 OMP RPC 宿主原型实测

### 交付与范围

用户授权用最小原型验证 rdog 托管新的后台 OMP, 控制端断开后同一 OMP 继续 coding, 重连查询真实状态和输出, 并独立验收产物. 现有 Herdr/OMP 不接管、不输入、不重启.

本轮已完成该交付. rdog 的现有 `@spawn` 托管临时 Rust AgentHost, AgentHost 通过官方 SDK 独占新 OMP 的 stdio. 没有集成 Herdr/Core, 没有新增生产 `src/` 或 Cargo 依赖, 也没有实现 ACP. 临时文件触发接口不是已发布的 rdog Agent API.

### 最小结构与 SDK 边界

- 数据形状为独立的 `RunConfig`、`Submit`、`HostState`、`Connection` 与 `OwnedOmp`. 外部 request ID、SDK native request ID、OMP session ID、OMP PID 和 rdog host task ID 分开保存.
- Host 单写 `state.json` 与 `events.jsonl`, 一个 worker 持有 SDK 的 `prompt_and_wait` 调用. controller 发布一次 `submit.json` 后退出; Host 不重发不明结果. `send.intent` 在调用前独占写入. Host 仅在匹配本 run/PID 的 stop 信号后结束本次进程树.
- 施工时核实 [SDK `client.rs`](https://github.com/can1357/oh-my-pi/blob/v18.15.0/sdk/rust/omp-rpc/src/client.rs) 与 [wire.rs](https://github.com/can1357/oh-my-pi/blob/v18.15.0/sdk/rust/omp-rpc/src/wire.rs), 排除了 `encode_command + send` 注入原生 ID 的假设. 公开 `send` 只接受 host 回调响应. 实际使用官方 `prompt_and_wait`, 由返回的 `PromptTurn.result.id` 关联外部任务. 原生 ACK 在调用期间不公开, 不能伪造一个提前取得的 ACK.
- [SDK `PromptTurn`](https://github.com/can1357/oh-my-pi/blob/v18.15.0/sdk/rust/omp-rpc/src/turn.rs) 提供排除 thinking 的可见文本. 原生 `prompt_result`、新采样的 `get_state`、进程存活分别取证, 不用 `agent_end` 或旧的 settled 通知猜完成. Windows 使用 suspended spawn、专属 Job、assign 后 resume; 正常 stop 核对 Job 已空并回收主进程. 不按程序名终止进程.

SDK 固定为 v18.15.0, 源 commit `3fedf8d53267e32a6d7521ce725fdcc4a36fdc67`, 与 Windows OMP 18.15.0 对齐. 原 SDK 未修改, clone 保留仓库 LICENSE 与锁定依赖. 此处 native ID 只在相应 RPC 连接内有意义, 两轮均出现 `req_3`, 不能脱离 run/session 混为同一任务.

### 两轮实测与必要网络前置

| 隔离轮次 | 新进程网络环境 | rdog host task | OMP PID | 原生 coding 结果 | 最后复核 |
|---|---|---|---|---|---|
| `rdog-rpc-ab35cf441c` | 原样继承 daemon 环境 | `t-5bf8ccef` | `114044` | `req_3` 返回 `error`, 会话日志记录模型 fetch 的 `ECONNRESET`, 工作目录为空 | 2026-11-03 |
| `rdog-rpc-ea8015f353` | 仅向新进程传递用户现有系统代理 | `t-b77dd9c0` | `114192` | `req_3` 返回 `completed`, 真实 coding 与独立验收通过 | 2026-11-03 |

第一轮不是保活或 RPC 失联. 查询时同一 OMP 与 Host 仍存活, 相同 session 可读到失败结果. 该轮不能算 coding 验收通过, 已保留失败证据并精确收停, 没有重发原请求.

Windows 的用户系统代理已启用 `127.0.0.1:7897`, 但 daemon 环境及同形状启动器的 `HTTP_PROXY`、`HTTPS_PROXY` 均缺失. 对同一 `https://chatgpt.com/backend-api/codex/responses` 做无凭据连通性对照, 直连超时, 使用现有代理取得 HTTP 405. 405 仅证明该路径收到 HTTP 响应, 不代表模型鉴权成功.

第二轮未修改宿主二进制、模型、profile 或审批策略. 新建隔离根后, 只在该次启动子进程中设置 `HTTP_PROXY` 与 `HTTPS_PROXY`, 值取自上述现有系统设置. 同一二进制 SHA-256 为 `a82bfd1fb972e69696a8639f8da38ea0604d76bd82e18628e50778a61b6bdaef`. 之后真实模型调用成功完成 coding, 支持本机此次失败来自启动网络环境不完整的结论. 未修改用户或系统全局环境, 未建设自动代理发现机制.

### 成功轮的完整证据

1. `ready` 时 Host PID 为 `111188`, OMP PID 为 `114192`, session ID 为 `01a1a160-5fc1-7666-88fa-91a0f291fe20`. 独立 CIM 查询确认 OMP 父进程为该 Host, command line 属于本轮独占根, 不是旧会话.
2. 外部请求 `rdog-rpc-ea8015f353-coding-1` 发布后, 提交 controller 与远端发布程序均退出 0. 新连接先观察 `submitted`, 后观察同一 PID/session 处于 `working`, 且已生成 `journal.mjs`、`cli.mjs`、README. Host stdio 不归这些短连接所有.
3. 后续新连接取得 `prompt_result.id=req_3`, `status=completed`, `agentInvoked=true`; 重新采样的状态显示 `isSettled=true`, 无待处理异步工作. `final.txt` 来自 SDK 的可见 assistant 文本, 包含本轮唯一标识, 不是终端输入回显.
4. 控制端独立执行 `verifier.mjs <work>` 并保留完整 stdout/stderr. Windows Node v24.4.1 下, worker 的串行 Node suite 为 30 pass、0 fail、0 skipped; 独立追加的 8 个真实 CLI 场景全部通过, 覆盖交错任务/语义重复、JSON/事件错误、冲突、逆序时间、非法迁移、缺文件和空输入. 真实输出核对到 `rdog.coding-demo.v1`, 两个任务、6 个唯一事件. 验收前后源码摘要的 `jd` 对比无差异. `examples/valid.jsonl` 与 `examples/invalid.jsonl` 也经文件盘点确认存在.
5. coding 完成后, `@task-status:t-b77dd9c0` 仍为 `running`, 同一 OMP 仍存活, 证明任务完成不等于会话退出. 随后只对本轮发布 stop, Host 写 `stopped`, CIM 确认 Host/OMP 均消失, host task 退出 0. 第一轮也已按相同拥有权边界收停. 原有 PID `63640`、`103452`、`109648`、`109356` 的名称、父 PID、SessionId、可执行路径和创建时间前后 `jd` 对比无差异.

这轮独立验收由控制端实际运行 Node 和 CLI, 不是复述 OMP 自报的测试结果. 自报、SDK 终态、OS 进程身份、独立产物判据分别保留.

### 源码、命令与保留产物

- Mac 独占源码根为 `/var/folders/58/3f9_69ts3bx4slnb8tgl572m0000gn/T/rdog-rpc-ab35cf441c-gp5d457t`. `host/` 保存完整 Rust/Cargo/lock, `upstream-repo/` 保存固定 SDK 与 LICENSE, `inspect.mjs`、`accept.mjs`、`verifier.mjs` 保存可复跑判据. 对照轮目录中的 `launch.ps1` 保存只影响新进程的实际启动命令; 临时 `.envrc` 记录相关变量默认值, 不在仓库或用户配置目录生效.
- Windows 两轮根分别为 `C:\Users\vdcoo\AppData\Local\Temp\rdog-rpc-ab35cf441c` 与 `C:\Users\vdcoo\AppData\Local\Temp\rdog-rpc-ea8015f353`. 后者的 `work/` 保留交付, 独立验收报告位于 `work/controller-acceptance-5SOF6Q/`, `ops/` 保存 ready/working/completed/stop 身份快照、代理环境摘要、源码摘要与验收收据. `sessions/` 为本轮新建 OMP 自己的 journal, 不是旧会话存储.
- 本 OMP 会话 `01a19c3e-d786-7255-a34e-642718a6517b` 的 `local/` 保留 `rpc-host-controller-evidence.json`、`rpc-host-run2-{ready,working,completed,acceptance,stopped}.json`、`rpc-host-run2-final.txt`、`rpc-host-protected-{before,after}.json` 和 `rpc-host-terminal-cleanup.json`. `local://` 是该会话的产物命名空间, 不应在新会话中当作同名本地文件.
- 构建与静态检查实际执行 `cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path <scratch>/host/Cargo.toml`、`cargo xwin clippy --target x86_64-pc-windows-msvc --manifest-path <scratch>/host/Cargo.toml --no-deps -- -D warnings`. 宿主通过; 未改 SDK 在 Windows 留有 `KILL_SETTLE_TIMEOUT` unused 警告, 未压制或 fork 修补. 未运行生产 rustdog 全量测试, 因生产代码未改.

### 吞吐检查点补记

- 前置门禁是目标、原会话保护基线、版本与进程所有权; SDK 公共接口核对纠正了不能编译的 send 假设.
- 独立工作面只拆 Host main 与 Windows Job 模块, 在不同临时目录并行施工; 父 Agent 负责唯一集成.
- SDK stdio 和状态写入由宿主独占, 提交/观察/独立验收按真实依赖串行, 没有并行触碰现有会话.
- 最小安全拆分没有引入 Core、管道服务器或通用队列. 此处记录执行组织, 未开展吞吐或性能基准.

### 限制与收尾

- 只验证 controller 断开, 未验证 daemon 重启、Host 崩溃、OS 重启或小时级 soak. Host 崩溃时专属 Job 的 kill-on-close 是清理机制, 不是原 OMP 的恢复机制.
- 仅有一个任务的隔离原型, 未实现生产多会话/多请求 registry、分页事件查询、交互审批往返或 abort 与 close 的完整公开 API. 代码没有自动批准 UI 请求, 但本轮没有进行审批故障注入. 原生 ACK 的提前可观察性也未实现.
- 发起端仍沿用有权限的 Terminal 执行 rdog, 没有重启 Herdr, 没有修复旧 Herdr 调用链的 Local Network 权限缓存. SDK 为固定版本私有协议依赖, 不能据此宣称与未来 OMP 或其它 ACP Agent 兼容.
- 最终本机 `window-find` 返回 `complete`、本轮 marker 匹配 0. 所有 controller 退出文件已核对, 54 份自建诊断目录在保存收据后移除, 未终结 Terminal/Herdr 应用. 本机控制连接退出 0; 其关闭后出现既有 `Unable to publish transport event: session closed` 日志, 不影响已返回的窗口结果. Windows 两轮新建进程已收停, 代码和证据保留, 不把临时目录保留等同于生产持久化.

轻量知识判断保留一项已验证候选. 常驻 daemon 的环境不等于用户当前交互环境, 本机缺失代理会让后台 OMP 的模型请求失败. 证据为两轮相同二进制对照、无凭据网络探测、启动环境摘要和最终独立验收; 尚未推广为通用代理策略或正式 solutions 条目.

当前最小原型及必要验收已完成. 不继续追加生产接口、ACP、Core/TUI 接管或重启恢复施工.

## 2026-11-04 正式集成第一阶段 SDK 门禁

用户授权只执行已交付方案的第一阶段, 不影响现有 Herdr/OMP. 本轮没有修改生产 `src/`、`Cargo.toml`、用户配置或安装文件. 正式 CLI、Host IPC、持久化、daemon 重启恢复均未施工.

### 结论与验收对象

第一阶段门禁通过, 但不能直接把上游 `prompt_and_wait` 当作正式后台接口. 实验使用固定 SDK 的临时副本, 增加了关联请求 ID 返回和受限事件出口. 原始 SDK 仍保持不变, 补丁尚未引入生产依赖.

|门禁|实际结果|最后复核|
|---|---|---|
|同一 OMP 多轮|首轮完成真实 Node 代码, 第二轮不调用工具即回忆首轮验证词. 两轮原生结果均为 `completed`.|2026-11-04|
|等待期间查询、取消及继续|工具活动时 fresh `get_state` 返回 `isStreaming=true`、`isSettled=false`. `abort` 后收到 `aborted` 和 SDK host-tool 取消回调, 同一会话又完成下一轮.|2026-11-04|
|断开后的确认交互|提交 controller 已退出. 新连接仍看到同一待答确认和 `isSettled=false`. 显式拒绝后收到原生 `rejected` 通知及相同 native ID 的完成结果. 没有自动批准.|2026-11-04|
|不累计整轮事件|独立短时 OMP 直接发送 256 条编号通知, 实收严格按序的 0 到 255, 随后收到原生命令完成结果. SDK 事件队列配置为 32, Host 不调用 `prompt_and_wait` 或保存全轮 events Vec.|2026-11-04|
|原有会话与资源收尾|原有四个 PID 的名称、父 PID、SessionId、可执行路径和创建时间前后 `jd` 无差异. 两轮自有 Host/OMP 均已消失, 两个宿主 task 均 `completed` 且退出 0.|2026-11-04|

功能轮的 Host PID 为 `117652`, OMP PID 为 `113756`, 原生 session ID 为 `01a1a43e-6e6d-7198-a663-4fdd22e563fc`. ready 与确认待答期间的两份 CIM 身份经 `jd` 核对无差异. `turn-one`、`turn-two`、`cancel-turn`、`after-cancel-turn`、`confirm-turn` 分别对应原生请求 `req_4`、`req_6`、`req_7`、`req_11`、`req_12`. 取消只是结束活动请求, 没有结束该 OMP.

首轮产物为 `trim_lines.mjs`, 实现字符串换行规范化、行末空格和 tab 清理, 保留空行及行内空白, 非字符串抛 `TypeError`. 控制端读取实际源码后, 在独立 Node 进程执行了 13 项断言, 包括混合换行、空输入、空行、Unicode、NBSP、非法类型和幂等性. 验收前后源码 SHA-256 均为 `12184849dad40703189eea3aca503670ffc23121798b241c94d0e87b787d41dd`, 不是采用 OMP 的自报结果.

### SDK 与协议边界

- SDK 源固定为 OMP v18.15.0 的 commit `3fedf8d53267e32a6d7521ce725fdcc4a36fdc67`, MIT 许可原文已保留. 实际 Windows OMP 报 `omp/18.15.0`, 二进制 SHA-256 为 `978846a816cb77f44a55e32b16493375ca837fbf87e0aec35a9c1c934bff0cd0`. 版本字符串不单独证明该二进制对应的构建 commit.
- 临时 SDK 在 `ClientOptions` 增加非零 `event_capacity`, 并增加 `Client::call_correlated`. 该方法沿用 SDK 自己的 ID 分配、编码、request 和唯一 stdio 读写路径, 返回 ID 与真实 ACK. 没有在 rdog daemon 中散布私有 JSON-RPC.
- 事件出口不能直接换成阻塞的 `sync_channel.send`. 构造期间消费者尚未拿到 receiver, 队列满会阻断协商; 关闭也不能放任 sender 一直等待. 实验副本的 `Shared::emit_event` 在构造期容量耗尽时明确关闭并报错, 运行期实行可被关闭打断的背压. 控制调用位于工作线程, 主循环持续排空事件.
- ACK、确认请求与原生终态必须独立建模. 本轮确认请求的 ACK 已返回, `agentInvoked` 缺省, 但确认仍待答且 `isSettled=false`. 显式拒绝后, 原生 `PromptResult` 才返回 `completed`、`agentInvoked=false`. 先前观察器把 ACK 当成交互结束的判据已撤回, 没有通过修改 OMP 来迎合错误判据.
- 队列容量只限制事件元素数量. 本轮没有证明 SDK outgoing queue、单帧字节或 OMP 整体内存严格有界, 也没有测量 RSS 或性能收益. 确认夹具没有系统副作用, 只验证显式拒绝回调, 不能冒称真实敏感操作已获授权.

第一种压力夹具要求模型调用 HostTool 并发出进度, 实际只观察到外层 `eval` 的进度, 没有可核对的编号事件. 该尝试没有计入通过, 不能把 `send_update` 的调用次数当成 RPC 收到的事件数. 最终改用官方 extension API 的 `ctx.ui.notify` 直接发编号通知, 无模型调用和工具路由. 事件轮使用另一全新 OMP, 不重启功能轮来伪装同进程连续性.

事件轮的 Host PID 为 `110604`, OMP PID 为 `119172`, session ID 为 `01a1a456-3e86-7355-90dc-7a9d85349176`. 原生命令 `req_4` 返回 `completed`、`agentInvoked=false`. 两轮的 SDK 相同, 事件轮仅新增通知编号日志和通知夹具. 功能轮的实际源码及二进制另存, 不把两份候选混为同一构建.

### 来源、命令和保留证据

Mac scratch 为 `/var/folders/58/3f9_69ts3bx4slnb8tgl572m0000gn/T/rdog-sdk-gate-353bb575a6-l5cfr2bj`. `sdk/` 保存已测 SDK 副本, `UPSTREAM-LICENSE` 保存许可, `functional-source/` 和 `functional-host.exe` 保存功能轮原件, `gate-host/` 保存事件轮源码和 Cargo.lock. `fixture.mjs`、`observe.mjs`、`verify.mjs`、上传清单与收据保存可复跑的夹具和验收方法. 它们是临时验证代码, 不是可安装的正式功能. 重跑需使用新的独占目录, 不重启已留证的旧 root.

Windows 两个独占根为 `C:\Users\vdcoo\AppData\Local\Temp\rdog-sdk-gate-353bb575a6` 和同路径加 `-events`. 目录保留 worker 和夹具源码、结果、规范化事件、可见 assistant 输出及本轮自己的 session journal. 没有导出 thinking 或凭据. 功能轮宿主二进制 SHA-256 为 `bd2455f05895bba1c9e459ede7f9e912d7c68341d1269832daf793bdd013c190`, 事件轮为 `c101f96bb2130ca7b73591ca4b839a8878f5f6e356d5312735215819a819bb48`.

实际执行并观察了以下检查:

1. `cargo run --manifest-path <scratch>/sdk/Cargo.toml --example phase1_boundary_smoke`, 构造溢出明确失败与满事件出口关闭两项边界检查通过.
2. `cargo nextest run --manifest-path <scratch>/sdk/Cargo.toml --test-threads 1`, 60 项通过. 另两项上游真实服务 smoke 原本标为 ignored, 未执行, 没有算入通过.
3. `cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path <scratch>/gate-host/Cargo.toml` 和对应 `cargo xwin clippy --no-deps -- -D warnings`. 两份最终候选均完成构建及 root lint. 原有 SDK Windows `KILL_SETTLE_TIMEOUT` unused 警告保留; 新增 runner 的 enum 大变体及嵌套 if 已修复, 未用 suppression.
4. 控制端独立 Node 产物验收、原生多轮/取消/确认/事件实验, 以及 `jd protected-before.json protected-after.json`、`jd functional-identities-ready.json functional-identities-reconnected.json`. 两项 JSON 对账均无差异.

本会话 `01a19c3e-d786-7255-a34e-642718a6517b` 的 `local/` 保存 `rdog-phase1-controller-evidence.json`、`rdog-phase1-protocol-facts.json`、`rdog-phase1-artifact-acceptance.json`、`rdog-phase1-final-os.json` 及逐阶段快照. controller evidence 保存原始调用收据和全部单次 Terminal 的 stdout、stderr、退出码. `local://` 只指本会话产物命名空间.

### 运行前置、收敛与停止

本轮早期从当前调用链连接成功, 后来转为 `No route to host`. ICMP 仍通, 独立 Terminal 中相同 rdog `@ping` 成功, 因而没有把它误报为 Windows 关机或重启 daemon. 后续复用此前获准的独立 Terminal 文件入口. 桌面工具另报 `sqlite-locks` 模块缺失, 已向工具 QA 上报, 未修改全局 OMP 安装. 这轮没有修复当前调用链的网络权限.

新 Windows Host 只在自己的进程环境中复用当时已启用的系统代理, 没有改变全局代理或审批政策. 初始化异步启动只回 `running` 时曾过早读取到空事件; 后续先等真实 ready, 没有重复启动.

共享前置为 SDK 固定源与原进程基线. SDK 和 runner 在不重叠的临时目录并行施工, 主代理集成后集中构建, Windows 生命周期验收串行. 第一种事件夹具没有关闭门禁缺口, 因而撤换为更直接的原生通知, 没有扩建通用运行器或服务.

最终两轮自有进程已收停, 原有 PID `63640`、`103452`、`109648`、`109356` 的基线不变. 本机 `window-find` 返回 `complete`, 本轮 Terminal marker 匹配 0. 第一阶段停止条件已满足, 不自动进入第二阶段. 尚未验证 daemon/Host/OS 重启、生产鉴权、持久化恢复或长时 soak.

轻量知识判断保留一个候选. SDK 构造期间没有事件消费者, 因而事件背压与关闭必须一起设计. 证据在边界 smoke、固定源码和原生通知实验; Windows EOF 与满队列关闭的更多并发组合尚未覆盖, 本轮不把实验补丁发布成正式通用规则.
