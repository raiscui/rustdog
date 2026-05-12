## [2026-03-21 14:43:41] [Session ID: codex-20260321-143912] 主题: 初始化后值得优先推进的后续事项

### 背景
- 本轮只完成了仓库初始化和基线建立,没有改动业务逻辑。
- 初始化过程中已经暴露出几个适合后续推进,但不应在无明确需求时直接动手的方向。

### 后续事项
- 为 `listen` 和 `connect` 的参数解析补充更完整的单元测试,不只覆盖 Unix 非法端口这一条路径。
- 评估 `src/unixshell.rs` 中 `unsafe` fd 接管逻辑的可维护性和资源释放语义,必要时做一次专门的健壮性审查。
- 补一份面向开发者的架构说明或 README 开发章节,降低下次接手时的重新理解成本。

## [2026-04-02 01:00:17] [Session ID: codex-20260402-001922] 主题: command-script-intercept 二期增强候选

### 背景
- 首版已经交付:
  - `@key`
  - `@paste`
  - `@script`
  - `listen` / `connect` / `daemon` 的显式 control-capable lanes
- 当前阻断级测试已经够用,但还有几条很值得后续补强的路线。

### 后续事项
- 细化 `@key` / `@paste` 的 unsupported-backend 失败契约测试,锁住 exit code 和 stderr 文案。
- 评估是否要把 `@paste` 从“直接 keystroke 文本注入”升级成更贴近真实 paste 的平台实现。
- 评估是否要给 README / `cmd.md` 增加轻量 help / 文档 smoke test,防止后续文档口径漂移。
- 评估是否需要新增独立的 `attach` / `client` 模式,让用户可以只用 `rcat` 自己去接入 `daemon inbound` 暴露出来的 bind shell,而不是依赖 `nc` 这类普通 TCP 客户端。

## [2026-04-02 02:57:57] [Session ID: omx-1775069552136-j9r53i] 主题: 为 `@key` 增加可观测 GUI 焦点夹具

### 背景
- 当前已经拿到两类证据:
  - 解析层 / 路由层单测通过
  - live `enigo` backend 在本机返回 `@exit 0`,且日志确认具备输入模拟权限
- 但 `@key` 和 `@paste` 的天然问题是: 它们不像 `@script` 那样自带 stdout,因此“最终 GUI 效果”还缺自动化观测夹具。

### 后续事项
- 评估是否要补一个专门的 macOS GUI smoke harness:
  - 打开可控文本输入目标
  - 触发 `@key` / `@paste`
  - 自动采集目标内容或窗口状态变化
- 如果要做,优先把它设计成可选的本机 smoke test,不要强行塞进默认 CI。

## [2026-04-02 08:27:31] [Session ID: omx-1775069552136-j9r53i] 主题: control lane 应把 `@key` 执行期错误回传给客户端

### 背景
- 当前 `@key:"right-option"` 这类“不支持的键名”会在服务端 `execute_key()` 阶段失败。
- 现象是:
  - daemon 日志里有真实错误
  - `rcat control` 本地只看到 `control connection closed`
  - 退出码仍然是 `0`

### 后续事项
- 评估是否要把 built-in control action 的执行期错误也包成协议响应返回,例如沿用 simple-command 风格返回 stderr + `@exit 64`。
- 这样用户就不用去翻 daemon 日志才能知道 `@key` 为什么失败。

## [2026-04-02 08:38:04] [Session ID: omx-1775069552136-j9r53i] 主题: 已完成清理 - control action 错误回传

### 背景
- 之前登记过“control lane 应把 `@key` 执行期错误回传给客户端”。

### 当前状态
- 本轮已实现:
  - built-in control action 执行期错误会回传 stderr 和 `@exit <code>`
  - `hyper` 这类非法键名现在能被客户端直接看到
- 该项可视为已完成,后续无需再把它当成未落地事项。

## [2026-04-06 11:20:00] [Session ID: omx-1775467041829-gy5cxx] 主题: Zenoh router/client + serial 迁移后的补强项

### 背景
- 当前主路径已经从 `peer/peer` 迁成:
  - `daemon = router`
  - `control = client`
  - 默认 autodiscovery, `--entry-point` 仅作 fallback
- 本地验证已经覆盖:
  - `cargo build`
  - `cargo test -- --test-threads=1`
  - Unix 下 router/client 集成测试
- 但仍有两类证据缺口没有在本轮完全补齐。

### 后续事项
- 在真实 ESP32 / 串口环境上跑一次 hardware smoke,确认:
  - serial endpoint 真能接入
  - daemon router 日志能观测到节点加入
  - control 侧仍能在同一网络里完成 `@ping` / `@cmd#id`
- 在 Windows 现场真正执行新的 `tests/zenoh_router_client_windows.rs` 对应 smoke,不要只停留在编译通过。
- 评估是否要为 autodiscovery / fallback 行为补更细的可观测日志,明确区分:
  - 当前是 autodiscovery 命中
  - 还是 `--entry-point` fallback 命中
## [2026-04-13 19:41:16] [Session ID: codex-20260413-zenoh-timeout] 主题: 为 Zenoh autodiscovery 拆出独立 timeout 配置

### 背景
- 当前为修复 Windows 多 locator autodiscovery 超时,在无 `--entry-point` 时新增了 manual scout + locator 排序。
- 当前实现先复用了 `request_timeout_ms` 作为 discovery 窗口,这样能在不扩 CLI/config 的前提下尽快落地修复。

### 后续事项
- 评估是否为 control / daemon 补一个独立的 `zenoh.discovery_timeout_ms`。
- 如果后续出现“请求超时想调长,但 discovery 不想一起变长”的现场,就把这项从备忘转成正式实现。

## [2026-05-01 14:24:00] [Session ID: 019de364-f2af-7432-ad6a-40552af185c8] 主题: 若后续实现远程截图,先补结果载荷设计而不是先开第二控制面

### 背景
- 本轮已判断“远程请求本地 daemon 截图”更适合走现有显式 control 协议,例如 `@screenshot`。
- 但当前 `control_core` 成功响应模型仍偏向 `0` / `stdout` / `stderr`,对截图这种二进制结果还缺一个正式 value contract。

### 后续事项
- 若进入实现阶段,优先先定 `@screenshot` 的成功响应形态:
  - 小图是否直接 base64 内嵌
  - 大图是否返回 metadata + 独立 result keyexpr / 临时文件引用
- 若确实需要旁观订阅或大结果分发,再新增 screenshot result pub/sub keyexpr,但不要把 request 入口改成订阅频道主入口。
- 如果 macOS 屏幕录制权限成为实测阻塞点,再评估是否需要单独 helper 进程; 在那之前不要先拆新 bin。

## [2026-05-05 17:59:16] [Session ID: codex-20260505-pty-design] 主题: 后续实现 `@pty` / `rcat control --pty` 远程交互会话

### 背景
- 用户需要 `codex` 这类要求真实 TTY 的程序能通过 `rcat control` 远程执行。
- 当前裸 shell 行是一次性 `Command::output()` 模型,不提供 PTY,所以裸 `codex` 会报 `stdin is not a terminal`。

### 后续事项
- 设计协议层 `@pty` 作为单一真相源,再提供 CLI 入口 `rcat control TARGET --pty -- COMMAND ...`。
- `@pty` session 期间,普通键盘输入默认全部进入 PTY stdin,不能再按 line-control 解析 `@key` / `@script`。
- 不保留 in-band escape 入口:
  - `~.` / `~?` / `~~` 这类字符序列也可能被 `codex`、shell、vim、REPL 等远端 TUI 使用
  - 第一版 PTY 输入流必须尽量字节透明,不要在用户输入中偷藏 control 命令
- 明确 `Ctrl-C`、`Ctrl-D` 等按键默认发给远端 PTY 程序,不直接退出 `rcat control`。
- 强制关闭 / detach 应走 out-of-band 控制面,例如另开一个 control 请求 `@pty-close#id` 或 `rcat control TARGET --pty-close SESSION_ID`,不要复用 PTY 输入流内的 escape。
- 非交互大模型任务仍优先建议走 `codex exec ...`,只有需要 TUI 时才进入 `@pty`。

## [2026-05-05 18:58:00] [Session ID: codex-20260505-pty-implementation] 主题: 已完成清理 - `@pty` / `rcat control --pty`

### 背景

- 之前登记过“后续实现 `@pty` / `rcat control --pty` 远程交互会话”。

### 当前状态

- 本轮已实现:
  - `@pty` / `@pty-close`
  - TCP / WebSocket / Zenoh session channel PTY
  - PTY 输入透明
  - out-of-band close
  - focused tests 和文档同步
- 该项可视为已完成,后续无需再把它当成未落地事项。

## [2026-05-11 22:08:00] [Session ID: omx-1778469026342-c6n34v] 主题: rdog 发布远端和外部包名迁移

### 背景

- 本轮已把项目源码、Cargo metadata、README 和安装脚本口径改为 `rustdog` / `rdog`。
- 已验证 `aur.archlinux.org/rustdog.git` 存在,并把 `.gitmodules` 切到新地址。
- 但 `git ls-remote git@github.com:raiscui/rustdog.git HEAD` 当前返回 `Repository not found`。

### 后续事项

- 创建或迁移 GitHub 仓库 `raiscui/rustdog`。
- 迁移 GitHub release、wiki、badge、installer 依赖的 release asset 发布面。
- 如果仍需要保留旧 `raiscui/rustcat` 或 `robiot/rustcat` 的来源说明,应在 README 中明确写成 upstream/legacy,不要混入新主路径。

## [2026-05-12 11:54:07] [Session ID: codex-app-2026-05-12-rustdog-repush] 主题: rdog GitHub 远端创建项已落地,但 release 发布面仍未迁移

### 背景

- 旧记录里有一项“创建或迁移 GitHub 仓库 `raiscui/rustdog`”。
- 本轮已经按 fresh init 状态重新创建 public 仓库并推送 `main`。

### 当前状态

- 已完成:
  - 创建 `https://github.com/raiscui/rustdog`
  - 设置 `origin = git@github.com:raiscui/rustdog.git`
  - 推送 `main`
  - 验证远端 HEAD 与本地 HEAD 一致
- 未在本轮处理:
  - GitHub release 迁移
  - wiki 迁移
  - badge / installer / 外部 release asset 依赖面迁移

### 后续事项

- 如果要正式对外发布,下一轮应检查 README badge、安装文档、release workflow 和旧仓库跳转说明。
- 不要把本轮 fresh init push 误记成完整 release 迁移。
