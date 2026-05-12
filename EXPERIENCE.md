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
