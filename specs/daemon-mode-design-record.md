# Daemon 模式设计决策记录

> 状态：**历史设计记录**（2026-03-21，openspec 归档迁入 2026-10-06）
> 这份文档记录 daemon 模式的设计动机与取舍。**现行行为规格**见
> [Daemon 模式运行时规格](daemon-mode-runtime-spec.md)。

---

## Why

当前 `rustcat` 的运行模型还是一次性 CLI。`connect` 连不上就结束,`listen` 接完一轮也会退出。这和“启动后主动连出去、连不上就持续重试、同时打开本地端口接收入站连接、进程长期存活不自行退出”的目标是两套语义。

这次变更要先把运行方式拉直。我们需要一个配置驱动的 daemon 入口,把端口、重试、生命周期这些信息从零散命令参数收回到一份可管理的 TOML 配置里,再用明确的监督规则保证进程常驻。

## What Changes

- 新增一个配置驱动的 daemon 运行入口,用于启动长期运行的 `rustcat` 服务实例。
- 使用 `figment` 支持 TOML 配置文件加载,同时保留默认值和环境变量覆盖能力。
- 新增启动即主动连接上游地址的 outbound connector,连接失败或后续断开后按配置间隔持续重试。
- 新增启动即绑定本地端口的 inbound listener,可持续接收入站连接,单次会话失败不应拉垮整个进程。
- 新增 daemon 生命周期约束: 运行中遇到可恢复网络错误时记录日志并继续存活,而不是直接退出。
- 保留现有 `listen` 和 `connect` 子命令,继续服务一次性、人工操作型场景,本次不把它们改成 daemon 语义。

## Capabilities

### New Capabilities

- `runtime-config`: 定义 daemon 模式的配置模型,支持通过 Figment 从默认值、TOML 文件和环境变量提取强类型配置。
- `daemon-supervision`: 定义长期运行、不因单次网络错误或会话结束而退出的生命周期与恢复规则。
- `outbound-retry-connector`: 定义启动后主动连出、失败后按固定间隔重试、恢复后继续服务的出站连接行为。
- `inbound-service-listener`: 定义启动即绑定本地端口、持续接受连接、隔离单次会话失败影响的入站监听行为。

### Modified Capabilities

- 无。当前仓库还没有既有 OpenSpec capability 需要修改。

## Impact

- 受影响代码:
  - `src/main.rs`
  - `src/input.rs`
  - `src/listener/mod.rs`
  - `src/unixshell.rs`
  - `src/winshell.rs`
  - 预计新增配置加载和 daemon runtime 相关模块
- 受影响依赖:
  - 新增 `figment`
  - 预计新增 `serde` 及 TOML 配置提取所需支持
- 受影响行为:
  - 新增 `daemon` 类启动路径
  - 现有一次性 `listen` / `connect` 行为保持兼容
- 受影响验证:
  - 需要补充配置解析、重试调度、监听常驻和异常恢复相关测试

---

## 设计

## Context

当前 `rustcat` 的入口只有 `listen` 和 `connect` 两条一次性命令路径。它们都把“当前终端正在参与会话”当成默认前提,而且底层实现里存在多个直接 `exit()` 的路径。这个结构很适合临时交互,但不适合 daemon。

这次变更同时带来三类新约束。第一,运行参数不能再只靠命令行零散传入,需要一份稳定的配置模型。第二,出站连接和入站监听要能并行存活,单边失败不能拖垮另一边。第三,守护进程必须把“网络暂时失败”看作常态,而不是异常退出条件。

当前代码体量不大,而且整体还是同步 IO 模型。这给了我们一个很实际的机会: 不用把仓库整套改成 async runtime,也能先把 daemon 模型搭起来。

## Goals / Non-Goals

**Goals:**

- 增加一个独立于现有 `listen` / `connect` 的 `daemon` 启动路径。
- 用 `figment` 构建“默认值 + TOML 文件 + 环境变量覆盖”的配置加载链。
- 让出站连接器和入站监听器都能在 daemon 模式下长期运行。
- 把当前库内直接 `exit()` 的控制流收回到顶层,让 supervisor 决定何时重试、何时继续存活。
- 保持现有一次性 CLI 行为兼容,不把老命令硬改成守护语义。

**Non-Goals:**

- 本次不把整个项目迁移到 Tokio 或其他 async runtime。
- 本次不移除现有 `listen`、`connect`、交互模式和本地交互模式。
- 本次不做系统级 service 安装能力,例如 launchd/systemd unit 生成。
- 本次不追求多监听器、多出站目标、指数退避、热重载配置这类扩展能力。
- 本次不把 daemon 模式做成多并发入站会话复用器,先保持每个监听器串行处理一条会话。

## Decisions

### 决策1: 新增 `daemon` 子命令,而不是改写现有 `listen` / `connect`

选择:

- 新增独立的 `daemon` 子命令,例如 `rcat daemon --config rcat.toml`。

原因:

- 现有 `listen` / `connect` 明显偏人工操作和一次性会话。直接把它们改成常驻,会让原有用法变味。
- daemon 需要配置文件、监督线程、重试策略,这些概念和临时命令的心智模型不一样。
- 保留旧命令可以把兼容性风险压到最低。

备选方案:

- 在 `listen` / `connect` 上继续叠 `--config`、`--retry`、`--daemon` 参数。

不选原因:

- 这样会把当前已经不算复杂的 CLI 迅速堆成条件分支集合。
- daemon 和交互模式混在一起后,很多“不支持 TTY 的场景”会变成运行时惊喜。

### 决策2: 用强类型配置结构承载 daemon 行为,加载顺序固定为 defaults -> TOML -> env

选择:

- 新增 `DaemonConfig` 及其子结构,由 `Figment::from(Serialized::defaults(...))`
  开始,再 `merge(Toml::file(...))`,最后 `merge(Env::prefixed(\"RCAT_\").split(\"__\"))`。
- 如果显式传入 `--config` 且文件不可读,启动直接失败。
- 如果未显式传入配置文件且默认路径不存在,允许仅靠默认值和环境变量启动。

建议的配置骨架:

```toml
[daemon]
retry_seconds = 5

[outbound]
enabled = true
host = "127.0.0.1"
port = 4444
shell = "/bin/bash"

[inbound]
enabled = true
host = "0.0.0.0"
port = 5555
shell = "/bin/bash"
```

原因:

- 用户关心的是端口、重试间隔和是否长期运行。这些都天然属于配置文件,不适合再散落进多个 CLI 参数。
- Figment 的 layered merge 很贴合这类“默认值 + 文件 + 环境覆盖”的服务程序。
- 强类型配置可以在启动前做一次完整校验,把不可恢复的问题挡在 supervisor 之外。

备选方案:

- 手写 TOML 解析或继续只用 `clap` 参数。

不选原因:

- 手写解析会平白增加维护面。
- 只用参数无法自然表达双端点并存和守护生命周期策略。

### 决策3: 保持同步线程模型,新增 supervisor 层统一托管重试和常驻行为

选择:

- 新增 `daemon` 运行模块。
- 顶层根据配置为 `outbound` 和 `inbound` 各启动一个 worker 线程。
- 每个 worker 自己维护一个无限循环:
  - 成功建立会话后执行本次会话
  - 会话结束或连接失败后记录日志
  - 睡眠 `retry_seconds`
  - 继续下一轮
- 主线程只负责等待 worker,自身不主动退出。

原因:

- 当前项目已经是同步阻塞 IO。先用线程把 daemon 做稳,比一边改 async 一边改语义更可靠。
- 用户要求的是“持续运行,任何情况都处理好,不自行退出”,线程监督足够满足这个目标。
- 小仓库先选更短的实现路径,能少引入一层大框架就少引。

备选方案:

- 直接迁移到 Tokio,把连接、监听、重试全部改成 async task。

不选原因:

- 这会把本次变更从“新增 daemon 能力”升级成“重写 IO 模型”,风险不成比例。
- 现有 `unixshell` / `winshell` 也没有现成的 async 结构可直接复用。

### 决策4: 库级代码不再直接 `exit()`,统一返回结果交给 supervisor 或 `main`

选择:

- 把当前 `listener` 中线程内 `exit()`、`readline` 中 `exit()` 之类的路径,重构为返回错误或会话结束状态。
- `main()` 仍然可以为一次性 CLI 命令决定退出码。
- daemon worker 只记录错误并进入下一轮,不会因为单条路径直接结束整个进程。

原因:

- daemon 最怕“底层某个线程自作主张把整个进程杀掉”。这件事必须收口。
- 只要 `exit()` 还藏在库层,守护语义就不可信。

备选方案:

- 保留现状,仅在 daemon 模式外围包一层 `catch_unwind` 或子进程重启。

不选原因:

- 这只能掩盖问题,不能真正定义好控制流边界。
- 线程内 `exit()` 是进程级副作用,不是 supervisor 能优雅恢复的那种错误。

### 决策5: daemon 入站监听不复用现有交互监听,而是采用 unattended session handler

选择:

- 现有 `listen` 依赖本地 `stdin/stdout` 和可选的 `rustyline` 交互,不适合无人值守。
- daemon 的 `inbound` worker 改为在收到连接后启动配置好的本地 shell 或命令,并把子进程 stdio 绑定到 socket。
- 每次只处理一个入站会话。会话结束后返回 accept 循环,继续等待下一条连接。

原因:

- daemon 模式没有稳定的操作员终端可读写。继续依赖 `stdin/stdout` 会让行为变得不可预测。
- 绑定到本地 shell 的模式和当前 `connect` 的能力最接近,技术上也最容易复用现有平台差异代码。
- 先做串行单会话,能把边界讲清楚。后续如果真要并发,再单独开 change。

备选方案:

- 让 daemon `inbound` 继续复用当前 `listen_tcp_normal()`。

不选原因:

- 那条路径默认把本地终端当作会话的一部分,和 daemon 的基本假设冲突。

## Risks / Trade-offs

- [库层 `exit()` 路径较多,重构时容易漏] → 先用搜索和编译器报错把所有直接退出点摸全,再统一收口到顶层。
- [daemon 入站改成 unattended shell 后,语义与现有 `listen` 不同] → 在文档和 spec 里明确区分“人工监听”与“守护监听”,避免误会。
- [固定重试间隔在持续失败时会刷日志] → 首版只保留固定间隔,同时要求日志带上 worker 名称和错误摘要,后续如有需要再加节流或指数退避。
- [线程模型扩展性一般] → 本次只做单入站监听、单出站连接,线程方案足够。等确实出现并发诉求再评估 async。
- [bind shell 能力本身带来更高安全风险] → 变更内默认保持 endpoint 为显式启用,并在文档中强调这是一条高风险能力。

## Migration Plan

1. 添加 `figment`、`serde` 及配置提取所需依赖。
2. 新增 daemon 配置结构和加载入口,定义默认配置与 `--config` 解析规则。
3. 重构现有连接 / 会话代码,去掉库层 `exit()` 副作用,统一改为结果返回。
4. 实现 `outbound` worker 和 `inbound` worker,接入 supervisor 循环与固定间隔重试。
5. 为新配置和新命令补测试,覆盖配置优先级、连接失败重试、监听常驻和会话结束后继续服务。
6. 更新 README 或后续开发文档,补充 daemon 配置示例和行为说明。

回滚策略:

- 这是纯新增能力,没有数据迁移。
- 如需回滚,移除 `daemon` 子命令入口和相关新模块即可,原有 `listen` / `connect` 不受影响。

## Open Questions

- 当前没有阻塞实现的开放问题。
- 如果后续实现阶段发现 Windows 入站 shell 的交互细节和 Unix 差异太大,允许以同一配置接口保留平台内部分支实现,但不改变外部 spec。

---

## 任务清单

## 1. 配置与 CLI 入口

- [x] 1.1 为 daemon 模式引入 `figment`、`serde` 及所需配置提取依赖
- [x] 1.2 在 `src/input.rs` 中新增 `daemon` 子命令和 `--config` 参数入口
- [x] 1.3 定义 `DaemonConfig`、`OutboundConfig`、`InboundConfig` 与默认值结构
- [x] 1.4 实现 daemon 配置加载链,覆盖 defaults -> TOML -> `RCAT_` 环境变量 -> 启动前校验

## 2. 库层控制流收口

- [x] 2.1 盘点并移除 `listener`、`unixshell`、`winshell` 等模块中的库层 `exit()` 路径,统一改为返回结果或会话结束状态
- [x] 2.2 重构现有会话处理函数,提取可被一次性模式和 daemon 模式复用的 socket / child process 适配层
- [x] 2.3 保持现有 `listen` / `connect` 子命令兼容,把退出码决策收敛到 `main()` 顶层

## 3. Daemon 运行时

- [x] 3.1 新增 daemon supervisor 模块,按配置启动并等待 inbound / outbound worker
- [x] 3.2 实现 outbound worker 的“启动即连接、失败重试、会话结束后重连”循环
- [x] 3.3 实现 inbound worker 的“启动绑定端口、持续 accept、绑定失败重试”循环
- [x] 3.4 为 daemon 入站连接实现 unattended session handler,在单次会话结束后回到监听循环
- [x] 3.5 为 daemon worker 增加结构化日志,明确区分启动、连接失败、监听失败、会话结束和重试等待

## 4. 验证与文档

- [x] 4.1 补充配置加载测试,覆盖默认值、TOML 覆盖、环境变量覆盖和非法配置失败
- [x] 4.2 补充 daemon 行为测试,覆盖出站连接失败重试、入站绑定失败重试和单次会话失败不退出
- [x] 4.3 回归验证现有一次性 `listen` / `connect` 行为未被破坏
- [x] 4.4 更新 README 或开发文档,补充 daemon 配置示例、端点说明和风险提示
