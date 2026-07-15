# 项目经验沉淀

## [2026-04-02 12:16:33] line-control 协议的长期经验

- 单次请求结果不要再用 `@exit` 表达。
  - 对长期控制通道来说,`@exit` 很容易被误解成“客户端应退出”。
  - 当前正确口径是每条请求只回一条 `@response ...`。

- 显式协议请求和裸 shell 行必须分层。
  - 显式协议请求适合做可关联、可扩展、可自动化的控制协议。
  - 裸 shell 行适合保留终端式顺序流体验。
  - 这两条路径可以共存,但不能混成一锅。

- 如果 shell 请求也需要 request id,优先走 `@cmd#id:"..."`。
  - 不要强行把裸 shell 行也塞进 request-id 协议。
  - `@cmd#id` 是显式协议入口,裸 shell 行继续保留原心智,这是当前最稳的边界。

- 带 request id 的响应应统一包成对象。
  - 成功: `@response {"id":...,"value":...}`
  - 错误: `@response {"id":...,"code":...,"error":"..."}`
  - 这样客户端最容易做稳定关联和解析。

## [2026-04-05 10:28:31] 跨平台终端输入缓冲修复的经验

- 只要问题和“当前终端 / 控制台的输入回灌”有关,不要把 Unix 修复当成跨平台已完成。
  - Unix 可以用 `tcflush(TCIFLUSH)`。
  - Windows 需要单独补控制台输入缓冲清理,例如 `FlushConsoleInputBuffer`。

- “工作日志里写过已修复”不能代替平台定向测试。
  - 这类问题必须至少补一条对应平台的单测,锁住:
    - 什么时候应该清理
    - 哪些非控制台 / 非 TTY 场景必须跳过

- 如果用户现场出现类似 `1@key:"3"` 这种“上一次按键字符混进下一次协议输入”的形态,优先怀疑输入回灌污染,而不是先怀疑 parser 自己把协议拆坏了。

## [2026-04-05 10:54:18] Zenoh peer/peer 跨主机接入的经验

- 不要把 peer/peer LAN profile 误当成“daemon 固定 TCP server 直连”。
  - 默认路径依赖 LAN discovery / scouting。
  - 同机能通,不代表跨主机 discovery 一定成立。

- 当 zero-config discovery 在某个平台或某个局域网里不稳定时,优先补 deterministic fallback:
  - daemon 侧固定 `listen_endpoints`
  - control 侧显式 `--entry-point`

- 这类 fallback 最好做成配置能力,不要只靠口头建议。
  - 否则现场一旦从 macOS 换到 Windows,或者从有 multicast 的 LAN 换到受限网络,体验会断崖式变化。

## [2026-04-05 11:04:21] Windows 输入注入失败的经验

- 当 `@key` / `@paste` 在 Windows 上报 `blocked by UIPI`、`access is denied` 或 `os error 5` 时,优先把它视为权限边界。
  - 不要继续从 transport / discovery / parser 方向乱查。

- 这类错误应归类成 `PermissionDenied`,不要混进泛化执行失败。
  - 否则用户会误以为是 daemon 坏了,而不是目标窗口权限更高。

- 产品层最少要给出可执行的恢复路径:
  - 让 daemon 与目标窗口处于相同或更高完整性级别
  - 避免让用户在“Zenoh 已通,输入仍不生效”的黑盒里反复试错

## [2026-04-06 11:20:00] Zenoh router/client 迁移时不要把 entrypoint 错当成唯一主路径

- 即使拓扑已经从 `daemon=peer / control=peer` 切成 `daemon=router / control=client`,也不要自动推导出“control 必须显式 `--entry-point` 才能连”。
  - 如果 Zenoh 的 multicast scouting / autodiscovery 仍然可用,最稳的产品口径通常是:
    - 默认 autodiscovery
    - `--entry-point` 只做 fallback

- 对用户来说,真正重要的是“还保不保持原来那种只写 `--target-name` 就能连上的体验”,而不是内部 role 名字换成了什么。
  - 如果实现和文档过早把 fallback 上升成必经主路径,会把原本可保留的 UX 白白收窄。

- 这类迁移的正确验证口径应至少同时覆盖两条链路:
  - 不带 `--entry-point` 的 autodiscovery 成功
  - 带 `--entry-point` 的 fallback 成功

## [2026-04-06 11:20:00] Zenoh duplicate-name 不能只靠网络 liveliness,同机还要本地 guard

- 如果 duplicate daemon/service name 保护只依赖 Zenoh liveliness 预检,同机并发启动时会有竞争窗口:
  - 第二个实例可能在第一个实例 declare liveliness 之前就通过检查

- 对这类“逻辑身份必须唯一”的系统,最稳的口径是双层约束:
  - 本地 PID/lock guard 先挡住同机竞争窗口
  - 网络 liveliness 再挡住已入网后的重复实例

- 本地 guard 的实现细节最好具备:
  - `create_new(true)` 原子创建
  - 文件中记录 PID
  - stale PID 检测与自动清理
  - 进程退出时自动删除 guard
## [2026-04-13 19:40:36] Windows Zenoh autodiscovery 不能盲信 Hello 原始 locator 顺序

- 现象层:
  - Windows daemon 监听 `tcp/0.0.0.0:<port>` + `serial/...` 时,Zenoh Hello 可能带出多张网卡的 locator。
  - 如果前面排着多个 `169.254.*` link-local / tentative 地址,`rcat control --transport zenoh` 即使“看见了 router”,也可能在 `zenoh::open()` 阶段先被这些慢连接拖到超时。

- 本质层:
  - 问题不在 `resolve_target()` 或 query/reply。
  - 真正失败点是 autodiscovery autoconnect 对 locator 顺序过于被动,把“发现到了”误当成“按原序逐个连就一定能及时连上”。

- 已验证口径:
  - 显式 `--entry-point tcp/<preferred-ip>:<port>` 正常时,优先怀疑 locator 选择而不是 daemon/queryable 坏掉。
  - 应用层更稳的做法是:
    - 先 scout
    - 过滤 serial locator
    - 优先 loopback / 非 link-local IP
    - 再把排序后的 endpoints 显式交给 `zenoh::open()`

- 工程结论:
  - 仅仅延长 timeout 不是最好修复。
  - 对多网卡 Windows 现场,manual scout + locator 排序比盲信 Hello 原始顺序更稳。

## [2026-05-07 01:08:33] Zenoh PTY 生命周期与 session bridge 经验

- Zenoh FIFO `recv_timeout()` 的 `Ok(None)` 是 timeout,不是 subscriber closed。
  - active PTY / session bridge 轮询里遇到 `Ok(None)` 应继续轮询。
  - 只有在明确没有 active PTY 的 bridge idle 状态下,timeout 才能作为 session 回收信号。

- PTY 完成条件必须由 terminal lifecycle frame 决定。
  - 成功或自然退出: `@pty-exit`
  - 强制关闭、owner 断开或策略回收: `@pty-closed`
  - transport close / subscriber close 只能说明链路状态,不能说明远端 PTY 进程已经怎样结束。

- `@pty-detached` / `@pty-attached` 不是完成信号。
  - 它们只表示控制端所有权变化。
  - detach / attach 的正确单一真相源是后台 session runtime + 当前 attached sink,不是最初打开 PTY 的 transport。

- attach / detach 不能只切输出路由。
  - 如果 `@pty-output` 切到新 attached client,但 terminal frame 仍发给旧 channel,新 client 会看到输出却永远等不到 `@pty-exit` / `@pty-closed`。
  - 因此输出 frame 和 terminal frame 必须一起跟随当前 attached control session。

- PTY frame schema 改动后,live smoke 不能只看 `@ping` 是否通。
  - `@ping` 只能证明基础控制面可达。
  - 如果 `--pty` 报 `@pty-exit 缺少必填字段 reason`,优先检查是否复用了旧 daemon。

## [2026-05-07 01:08:33] Codex/OMX 提交与上下文归档经验

- 通过 Codex/OMX 执行 `git commit` 时,commit message 需要同时满足两层要求:
  - Lore Commit Protocol trailers
  - 固定 trailer: `Co-authored-by: OmX <omx@oh-my-codex.dev>`

- 带反引号、换行和 trailer 的 commit message 不要用容易被 shell 污染的写法。
  - 优先用多个 `-m` 段落。
  - 或使用经过本地 hook 允许的 inline message 形式。

- 长任务归档文件也必须经过 `git diff --check`。
  - 历史 Markdown 里的尾随空格、CRLF 行尾、rebase conflict marker 都会挡住提交。
  - 修这类问题时应只清理格式,保留 append-only 日志两边正文。

- `.omx/*` 默认是运行态噪音。
  - 提交前必须检查 staged 范围里没有 `.omx`。
  - `git status --short` 里只剩 `.omx` 修改时,可以视作本轮代码/docs 已提交干净。

## [2026-05-11 23:40:00] rdog 更名与 legacy 兼容经验

- 产品级更名不能只做全局文本替换。
  - 新默认入口、配置文件、环境变量、下载目录、日志文件和 Zenoh keyexpr root 应统一切到新名字。
  - 旧入口如果已经被用户部署过,应作为 legacy fallback 保留一段兼容窗口。
  - 残留旧名扫描时,要区分三类命中: 兼容层、历史记录、真正漏改。

- 对 `rdog` 这类远程控制工具,兼容层本身也是协议契约。
  - `rcat.toml`、`rcat_*.toml`、`RCAT_`、`rcat/...`、`__rcat_session_*` 这类旧名字如果仍被旧客户端或旧 daemon 使用,不能为了“扫描全绿”直接删除。
  - 更稳的口径是新名字优先,旧名字只作为 upgrade fallback,并在 README/cmd/spec 中明确写出来。

## [2026-05-11 23:40:00] CLI 二进制更名会改变系统权限主体

- macOS Accessibility、输入监控、屏幕录制这类系统权限通常跟实际 app / binary 身份相关。
  - 从 `target/debug/rcat` 改成 `target/debug/rdog` 后,即使旧二进制有权限,新二进制也可能没有。
  - `@key` / `@paste` / `@screenshot` 相关测试和 smoke 不能把“旧名字能成功”当成“新名字也必然成功”。

- 重命名后的测试契约要能识别权限边界。
  - 如果返回 `PermissionDenied` 或项目约定的 code 77,应验证错误说明和恢复路径。
  - 只有真实输入成功时,才继续等待 keyinput event 或 GUI 副作用。

## [2026-05-12 17:35:27] rdog-control skill 与硬件/单片机表述边界

- 给 code agent 做 `rdog control` 使用 skill 时,不要只提供命令速查。
  - 需要同时写清楚 target-name 寻址、line-control 请求、`@response` / `@savefile` / `@pty-*` 解析、权限错误和安全边界。

- `rdog control mac.lab` 这类短命令是当前主路径。
  - `--entry-point` 是 autodiscovery 不可用时的 fallback,不要把 fallback 写成唯一入口。

- 面向硬件和单片机场景时,默认表述应是:
  - Codex 通过 `rdog control` 操作 bridge host。
  - bridge host 再通过串口、JTAG、SDK、烧录器或厂商工具控制设备。
  - 除非固件或设备侧 app 明确实现了兼容控制协议,否则不要暗示 `rdog` 能直接在 MCU 内执行 shell。

- 这条经验已经沉淀成全局 skill:
  - `/Users/cuiluming/.codex/skills/rdog-control/SKILL.md`

## [2026-05-25 09:10:00] rdog-control GUI live smoke: Chrome 网页 AX 不足时使用截图 manifest 坐标 fallback

- `rdog control mac.lab` 做 GUI 任务时,证据链应先固定为:
  - `@ping#1`
  - `@capabilities#2`
  - `@observe#3:{mode:"hybrid",include_screenshot:false,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}`
  - 必要时再 `@screenshot#id` 读取 JPEG + manifest。

- 如果 Chrome / 网页内容只暴露外层 AXWindow / AXGroup,没有网页内按钮 ref,不要假装可以 AXPress。
  - 正确 fallback 是读取 screenshot manifest 的 `virtual_bounds` 和 `image_to_os`。
  - 先裁剪/查看目标区域,确认 UI 目标。
  - 再用 `@click:{x,y,coordinate_space:"os-logical"}` 明确记录 `target_resolution.source:"coordinate_fallback"`。

- 本次 live 验证的坐标换算案例:
  - screenshot image size: `3390x1080`
  - manifest `virtual_bounds={x:0,y:-124,width:3390,height:1080}`
  - 左侧“小红书/首页”按钮中心约 `image=(78,343)`
  - OS logical 坐标为 `(78,219)`
  - `@click#6` 返回 `status:"ok"`, `released:true`, `source:"coordinate_fallback"`。

- 操作注意:
  - request id 必须是无符号整数,例如 `@ping#1`,不能用 `@ping#ping`。
  - 如果 `rdog control mac.lab` 报 autodiscovery 找不到 router,先确认 `rdog daemon` 是否运行; 临时启动的 daemon 完成任务后要停止。

## [2026-06-20 09:30:00] log 输出路径是隐性契约,改动必须先查 e2e

- `rdog` CLI 的 `log::info!` 输出是 e2e 测试的隐性"启动 sentinel"。
  - 大量 Zenoh e2e 用 `wait_until_output_contains(buffer, "zenoh router daemon ready", ...)` 等日志字符串做 daemon 启动就绪判断。
  - 类似地,`Connection Received` (listener) / `PTY ready` / `remote PTY closed` 等都是 e2e 关注的 log marker。

- 任何"输出路径"相关的改动(比如 `init_logger` 走 stderr 而不是 stdout)必须:
  - 先 grep 仓库里"谁 pipe 这个 stream 等这个 marker"
  - 一次性把所有依赖改掉,或者用合流 buffer 兼容(后者更省事)

- 教训:2026-06-19 把 `init_logger` 从 stdout 切到 stderr 时,看似一行修复(`stdout()` → `stderr()`),实际连带改 4 个 e2e(control_lanes / control_pty / shell_pty / zenoh_router_client)。
  - zenoh_router_client 还有 24+ 个测试只 pipe stdout,改用 `sh -c "exec rdog ... 2>&1"` sh wrapper 临时兼容。
  - 后续把这个 sh wrapper 退役,改用 `start_zenoh_daemon_with_combined_output` 在 helper 内部合流 stdout+stderr,这才把"输出路径"耦合彻底解开。

- 给后续改 daemon/control 启动行为的经验:
  - 改 log level / log target / log format 之前先 `rg "log::info|log::error|log::warn"` 看 e2e 依赖
  - 改 daemon 启动的 "ready" 日志之前先看 e2e 怎么等 daemon ready
  - 如果新设计要换 log 路径,要么同步改 e2e,要么用合流 buffer helper(`start_zenoh_daemon_with_combined_output` 是这次产出的现成模板)

- `start_zenoh_daemon_with_combined_output` 这个 helper 的价值:
  - 调用方只看到一个合流的 `Arc<Mutex<String>>`
  - 后续 init_logger 路径再变,改 helper 内部就行
  - 24+ 个测试都不用动

## [2026-06-20 18:45:00] scoped skill metadata commit 与旧支线归档经验

- mixed worktree 里同一个文件已经有非本轮改动时,不要为了提交一行 metadata 直接 `git add <file>`。
  - 本轮 `.codex/skills/rdog-control/SKILL.md` 工作区已有 agent-agnostic / one-shot 文档改动。
  - 用户只要求补 `version: "1.0"`,因此正确做法是从 HEAD 内容生成临时文件,只插入版本字段,用 `git hash-object` + `git update-index --cacheinfo` 写入 index,再提交。
  - 这样 commit `9d74d7e` 只包含 1 行新增,不会把其它未审阅 diff 混进去。

- `rdog-control` 这类 agent-facing skill 应显式维护 frontmatter 版本号。
  - 版本字段让 Codex / Claude / GPT / openai-compatible / MCP / human operator 能判断 skill 文档兼容边界。
  - 如果只是补 metadata,提交信息应聚焦到 skill metadata,不要顺手夹带 cookbook / protocol / source code 改动。

- 根目录旧支线六文件会显著污染每次上下文检索。
  - 本轮 `$continuous-learning` 将 23 个旧支线组、90 个文件归档到 `archive/branch_contexts/<suffix>/`。
  - 归档前必须先按后缀分组摘要,再生成 manifest,不要把 `__suffix` 文件平铺搬走。
  - 后续追溯旧支线时先看 `archive/manifests/ARCHIVE_MANIFEST__2026-06-20_branch_context_cleanup.md`。

- docs/specs 同步检查不能把不存在目录当作成功搜索。
  - 本轮首次 `rg docs specs plans roadmap milestones ...` 因缺少 `docs/` / `plans/` / `roadmap/` / `milestones/` 返回 exit code 2。
  - 修正做法是只搜索实际存在的 `README.md`、`AGENTS.md`、`EXPERIENCE.md` 和 `specs/`。
  - 搜索暴露 `README.md` 与 `specs/code-agent-rdog-control-usage.md` 对 one-shot N=1 / N>1 管线仍有旧描述,已同步为统一 `send_control_lines_*` 口径。

## [2026-06-20 23:58:00] [Session ID: omx-1781788115552-szl2hn] 主题: Zenoh `transport_unixpipe` 同机 fast path 实施经验

### 同机 fast path 优先用 zenoh 自带的 `transport_unixpipe` feature,不要新增独立 UDS 控制面

- **场景**: `rdog control <target>` 同机高频 round-trip 慢,想要 2~5x 提速。
- **错误方向**: 自己写一个 Unix domain socket 控制面,绕过 Zenoh query/reply 协议栈。理由是"理论 10~50x 更快"。
- **正确方向**: 启用 Zenoh 1.8.0 自带的 `transport_unixpipe` Cargo feature,把 link 层从 UDP 换成 named pipe (FIFO),保留 Zenoh 全部 query/reply/liveliness 协议。
- **结论**: zenoh 自带 transport 已经是经过实战验证的能力,工作量 1/10,2~5x 提速对高频 agent 调用是质变。只有在 unixpipe 仍不满足时,再启动方向 B(直接 UDS 控制面)。
- **为什么这一点关键**: 选错方向会被 Zenoh 协议层、维护成本、协议兼容性一起咬,自定义 UDS 控制面长期会成为维护负担。

### Zenoh 1.8.0 `transport_unixpipe` 实际是 named pipe (FIFO),不是 Unix domain socket

- **场景**: 实施 unixpipe fast path 时,默认按 "Unix domain socket" 的语义去推导路径长度上限和清理逻辑。
- **踩坑**: 实际 Zenoh 1.8.0 实现的 `transport_unixpipe` 是 named pipe (FIFO),`mkfifo` 创建,Zenoh 内部从 base 路径派生 `<base>_uplink` / `<base>_downlink` 两条 FIFO,每个 client connection 还会派生带 suffix 的 dedicated FIFO。
- **结论**: 路径上限必须按 macOS `sun_path` 104 字节 - `_downlink` 9 字节后缀 = 95 字节计算,不是 100 字节。stale cleanup 也得 unlink 这三条 FIFO。
- **为什么这一点关键**: 如果按 100 字节上限实施,某些长 namespace + name 组合会在 macOS 上 bind 报 ENAMETOOLONG,而 Linux 上不一定复现,导致 bug 难复现。

### Zenoh unixpipe client 探测用 `Path::exists` 不用 open FIFO

- **场景**: plan 写"用 200ms connect 短超时探测 FIFO 是否可连接"。
- **踩坑**: 实施时发现 Zenoh 1.8.0 的 `transport_unixpipe` listener 用单 reader 复用 FIFO 作为 request channel。
  主动 open 写端再立即关闭会让 daemon 端 `Invitation::receive` 看到 EOF,导致后续 client 无法再 connect。
- **结论**: client 探测只用 `std::path::Path::exists` 检查 `<base>_uplink` 文件是否存在。0 副作用,1us 内返回。真正的连接性由 `zenoh::open` 内部处理,失败会拿到明确错误。
- **为什么这一点关键**: 一旦 open 探测破坏了 daemon 的 request channel,daemon 端要重启才能恢复,产品上不可接受。

## [2026-06-21 15:30:00] [Session ID: omx-1781788115552-szl2hn] 主题: rdog control self / 空 target 本机 fast path 实施经验

### Zenoh 1.8.0 unixpipe 实际只创建 `<base>_uplink` 和 `<base>_downlink` 文件

- **场景**: 实现 `find_local_daemon_name()` 扫描 $TMPDIR 找本机 daemon 时,按 `*.pipe` 后缀找文件名,发现死活找不到 daemon。
- **踩坑**: Zenoh 1.8.0 `transport_unixpipe` listener 实际只创建 `<base>_uplink` 和 `<base>_downlink` 两个 FIFO 文件。
  `<base>` 本身(=`rdog-{ns}-{name}.pipe`)不一定存在(只是路径标识符)。
- **结论**: 扫描 unixpipe daemon 必须按 `*.pipe_uplink` 后缀,不是 `*.pipe`。
- **为什么这一点关键**: 文档没写明这个细节,只看 Zenoh 1.8.0 source 才看出来。如果按 `*.pipe` 扫,永远找不到任何 daemon。

### `rdog control self` 实现时必须强制 PTY 互斥但允许 one-shot

- **场景**: `rdog control self @<line>` 既要走 fast path,又要支持 `@ping` 之类的 one-shot。
- **踩坑**: 一开始我把 PTY 和 one-shot 都禁了,导致 `rdog control self @ping` 也报错"不支持 one-shot"。
- **结论**: PTY 必须禁(PTY 涉及长生命周期 session 复用,本机 fast path 短任务不适用);
  one-shot 必须允(一个 `rdog control self @ping @capabilities#1` 就能一次发 2 条命令)。
- **实现**: PTY 互斥检查在 ZenohLocal dispatch 入口,one-shot 复用 `send_control_lines_zenoh` 走单 session 串行。
- **为什么这一点关键**: 这是用户最常用的交互模式,禁了就没法用 self 走 batch 任务。

### 跨测试 namespace 隔离:e2e test 必须用独立 namespace,不能用共享 `lab`

- **场景**: `tests/zenoh_unixpipe_fast_path.rs` 加了 `self_target_*` 系列 e2e,要求 namespace 范围内只有 1 个 daemon。
- **踩坑**: 已有测试用 `lab` namespace,新测试也用 `lab`,并发跑时会跨测试污染,`find_local_daemon_name` 报"多候选"错。
- **结论**: e2e test 必须用独立 namespace(如 `selfexp`/`selfinf`/`selfmulti`),保证并发安全;或用 `--test-threads=1` 串行。
- **为什么这一点关键**: unixpipe 路径是文件,跨测试隔离比 Zenoh scout 这种内存协议更难;不留心容易出 nondeterministic 失败。

## [2026-06-29 14:18:00] rdog-control skill 文案瘦身与 token 纪律

- `rdog-control` 这种 agent-facing skill 不应把所有协议细节都塞进主文件。
  - 主文件负责高频执行路径、硬边界、lane 选择和验证规则。
  - 完整协议、低频示例和历史背景放到 `references/` 与 `specs/`。
  - 这样能减少 prompt token,也能降低 agent 被长篇背景带偏的概率。

- 文案瘦身不能牺牲语义边界。
  - 本轮必须保留 agent-agnostic 表述,不能退回 Codex-only。
  - 必须保留 `@flow`、`@window-resize`、display scope、AX diff、PTY、permission 和 destructive-action safety。
  - `@flow` 要写清 daemon-local、`policy.allow_shell:true`、inner response 被消费、outer final summary、v1 拒绝嵌套 `@flow` / `@pty` / `ControlLine:"@cmd..."`。

- 避免 prompt stuffing。
  - 之前已经验证过,单纯往 `SKILL.md` 或 profile prompt 增加更多示例,不一定改善模型行为。
  - 更稳的做法是保持 skill 短而可执行,并用真实命令输出、fixture、AX JSON diff 或测试结果验证行为。

- 这类改写的最小验证矩阵:
  - 对比行数 / 词数,确认 token 目标真的改善。
  - 检查 Markdown fence 成对。
  - `git diff --check`。
  - grep 关键协议词和反例词,确认没有删掉不可丢失的边界。

## [2026-07-14 23:57:10] [Session ID: omx-1784030029111-h2qsls] 坐标AX hit-test必须独立证明真实窗口归属

### 触发场景

- macOS应用的标准`AXChildren`树缺少内容节点,实现尝试通过`AXUIElementCopyElementAtPosition`或类似坐标hit-test发现补充AX root。
- snapshot给结果写入了目标PID、window index或backend ID,后续`find`、`get`和stale拒绝也都能通过。
- 这条内部自洽的控制链仍不能证明命中的树真正属于用户指定窗口。重叠窗口和z-order可能让hit-test返回浏览器或其它应用的foreign tree。

### 已验证的失败模式

- 一次WeChat兼容性实验把`发现`、`直播`、`发布`以及完整小红书WebArea包装成WeChat backend ID。
- backend ID可重放只证明同一候选树可再次定位,不证明候选树的owner正确。
- 当前复核中,两个WeChat窗口各9次application-scoped hit-test均返回`kAXErrorNotImplemented(-25208)`。system-wide hit落到前景VS Code,扫描24个WeChat相关PID也没有找到`文件传输助手`。
- 因此先前"WeChat AX兼容已动态GREEN"的结论被撤回。具体是哪一个boundary条件曾放行foreign tree仍未复现,不能继续写成已确认根因。

### owner证据门禁

坐标发现路径至少要保存并交叉验证以下证据,不能只保留重写后的target metadata:

1. hit元素的实际PID和角色。
2. parent链到达的精确application/window identity,以及途中遇到的foreign boundary。
3. 同一点的CGWindow owner与前后z-order。
4. hit元素、目标窗口和display之间的几何关系。
5. AX树业务语义是否与截图中可见内容一致。

任何一项出现foreign归属或证据缺失时,都应fail closed。不要把"find成功 + get成功 + stale拒绝"当作owner验证的替代品。

### WeChat当前边界

- 对`com.tencent.xinWeChat`内容定位暂时不使用AX。运行时真相源在`.codex/skills/rdog-control/SKILL.md`的`WeChat Temporary No-AX Policy`。
- 当前允许路径是window metadata、fresh screenshot、视觉坐标、`guard.display`、动作前window rect复核和动作后新截图。
- 这是应用级暂定安全策略,不是"WeChat永久不支持macOS Accessibility"的结论。
- 重新启用前必须通过受控重叠窗口owner回归、真实`文件传输助手`命中、focused/occluded多状态重复验证和全链路fail-closed。

### 可复用结论

- 请求目标和实际hit owner是两份不同状态,必须分别采集证据后再建立关联。
- GUI自动化中,稳定操作错误目标比明确返回"不可定位"更危险。
- 对坐标发现机制的测试必须包含foreign重叠窗口反例,不能只用单窗口正向fixture。
