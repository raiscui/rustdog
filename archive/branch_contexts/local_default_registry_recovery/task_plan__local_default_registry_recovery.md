# 任务计划: local-default registry 恢复与一致性验证

## [2026-07-18 00:10:44] [Session ID: omx-1784304547353-h5409r] [计划]: 建立证据链

### 目标

让同一 namespace 下的 daemon PID guard、local-default registry、unixpipe FIFO 与实际进程恢复一致,并验证裸 `rdog control` 能稳定解析唯一目标.

### 阶段

- [x] 阶段1: 读取历史上下文、项目规则与既有 local-default 契约.
- [x] 阶段2: 采集 PID 文件、进程、registry、FIFO、配置和二进制来源的运行态证据.
- [x] 阶段3: 沿真实调用路径核对状态创建、校验与清理逻辑,提出主假设和最强备选解释.
- [x] 阶段4: 用最小可证伪实验区分环境残留与生命周期代码缺陷.
- [ ] 阶段5: 执行已验证的恢复或代码修复,补回归测试并完成编译/动态验证.
- [ ] 阶段6: 更新任务记录,回溯延期项与重大风险,交付结论和后续建议.

### 两个收口方向

1. 最佳方案: 如果动态证据确认生命周期存在破口,修正 guard/registry/FIFO 的原子性或退出清理不变量,增加故障场景回归测试.
2. 先恢复可用: 如果代码已经能识别陈旧状态,而本机只是旧版本或异常终止留下的孤立文件,只定点清理已证实陈旧的状态并重新启动,不扩大代码改动.

### 当前现象与假设

- 现象: daemon 启动路径先打印 unixpipe fast path,随后因 `lab.pid` 已存在而退出.
- 现象: control 无法读取可用 local-default registry,随后从 FIFO 扫描得到 `wechat.ax.test` 与 `wechat.wechatax` 两个候选.
- 主假设: `lab.pid` 是陈旧 guard,或指向的旧 daemon 已不再能提供有效 registry/FIFO.
- 最强备选解释: guard 对应的 daemon 仍存活,但当前 `rdog` 二进制版本、namespace、registry 路径或文件内容与旧进程不兼容.
- 推翻主假设的证据: PID 文件指向存活且身份匹配的 `rdog daemon`,同时其配置和 unixpipe uplink 可用.

### 关键验证问题

1. 失败那一轮是否实际进入了 local-default guard 校验与 control registry 校验路径?
2. daemon 和 control 是否使用同一 namespace、state 目录、临时目录及二进制版本?
3. PID 文件指向的进程是否存活且确实属于当前 daemon?
4. registry 是否缺失、格式无效、PID 失效,或仅被 control 判为不可用?
5. 修改或清理 guard 时,是否会破坏正在运行 daemon 的单实例不变量?

### 状态

**目前在阶段4**: 在既有 unixpipe e2e 公开边界添加同名双启动回归测试,先验证当前代码确实会破坏第一实例的 FIFO.

### 遇到错误

- 只读采样的 JavaScript 编排命令因嵌套 zsh 参数展开与引号冲突,在解析阶段报 `SyntaxError`,没有启动 shell 子命令,也没有改变运行状态. 后续拆成短命令分别采样.

## [2026-07-18 00:25:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 静态与现场证据收敛

### 已验证事实

- PID 32191 是 2026-07-17 23:13 启动的 `target/debug/rdog daemon -c rdog_macos.toml`,仍监听 UDP 7447.
- 显式 `rdog control mac.lab @ping` 返回 `@response "pong"`,证明旧 daemon 主进程和网络 control path 仍活着.
- `zenoh-guards/lab__mac.lab.pid` 仍指向 32191,但 `local-default/` 已空,canonical `rdog-lab-mac.lab.pipe_{uplink,downlink}` 不存在.
- daemon 原始 tmux 日志证明第一实例曾成功注册 local-default,随后进入 ready.
- `run_zenoh_router` 当前先调用 `cleanup_stale_unixpipe_socket`,再注册 local-default,最后才进入会获取 service-name guard 的 `run_router_daemon`.

### 主假设与备选解释

- 主假设: 同名第二实例在 guard 拒绝前先 unlink 第一实例的 canonical FIFO,导致 control 将原 registry 判为失效并清除,最终回退到多个无关 FIFO 候选.
- 备选解释: 另一个测试或进程独立删除了 FIFO,而第二次启动只看到了已经损坏的状态.
- 最小证伪实验: 隔离 namespace/XDG state 启动第一实例并确认 FIFO,再启动同名第二实例;若第二实例失败后 FIFO 仍存在且裸 control 仍返回 pong,主假设不成立.

### 测试边界

- 使用现有 `tests/zenoh_unixpipe_fast_path.rs` CLI e2e seam,不直接断言私有 helper 调用次数.
- 行为契约: 同名第二 daemon 必须失败,但不得改变第一 daemon 的 FIFO、registry 与空 target control 可用性.

## [2026-07-18 00:32:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: RED 动态复现确认根因

### 验证命令与关键输出

- scoped format: `rustfmt --edition 2021 --check tests/zenoh_unixpipe_fast_path.rs`,exit 0.
- exact test: `cargo nextest run --package rustdog --test zenoh_unixpipe_fast_path -E 'test(=duplicate_daemon_start_should_not_break_running_local_default_unixpipe)' --no-capture`,exit 100.
- 关键失败: `duplicate daemon removed the running daemon FIFO .../rdog-dupstart-d58063.dupstart.pipe_uplink`.
- 第二实例输出顺序: 先 `zenoh unixpipe fast path 启用`,再 `本机默认 daemon 已存在`.

### 已验证根因

- 失败轮真实执行了 FIFO cleanup 路径,且操作的是第一实例正在使用的同一 base path.
- cleanup 在任何 ownership guard 之前发生,因此第二实例虽然最终被拒绝,已经 unlink 第一实例的 canonical FIFO.
- 修改清理顺序不会移除 stale cleanup 不变量: 合法获得 service-name ownership 的新实例仍会在打开 Zenoh session 前清理陈旧 FIFO.

### 正式修复决策

- 将 unixpipe cleanup 和 local-default register 作为 router startup ownership 的一部分,移到 `run_router_daemon` 获取 service-name guard 之后.
- 不采用只把 local-default register 提前的局部修法,因为 `local_default=false` 的同名第二实例仍会破坏第一实例 FIFO.
- daemon 配置解析与 endpoint 组合仍由 `run_zenoh_router` 负责;向 runtime config 传递明确的 unixpipe startup 配置,保持状态真相源唯一.

### 验证障碍

- 全仓 `cargo fmt --all -- --check` 在本任务文件之外已有大量 rustfmt diff,exit 1. 本轮不格式化这些既有 control-act 改动,改用 touched-file scoped rustfmt.
- 精确测试编译同时显示 6 个既有 control-act warning. 它们与 unixpipe 代码无关,但最终交付会明确保留该基线问题,不把 warning 隐藏为成功.

## [2026-07-18 00:39:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 正式修复进入 GREEN

### 实施

- `src/daemon.rs` 不再在 ownership guard 前清理 FIFO,只负责解析 `ZenohUnixpipeStartupConfig`.
- `src/zenoh_control.rs::run_router_daemon` 先获取 service-name guard,随后初始化 observation、清理 stale FIFO、注册 local-default,最后打开 Zenoh session.
- `specs/zenoh-unixpipe-fast-path-plan.md` 已同步新启动顺序.

### GREEN 证据

- touched-file rustfmt check: exit 0.
- 同一个 exact nextest: `1 passed,9 skipped`,耗时 1.230s.
- 用例判据覆盖: 第二实例失败、第一实例 FIFO 保留、隔离空 target `@ping` 返回 pong.

### 当前状态

**目前在阶段5**: 扩大运行时/集成回归,审查现有 e2e cleanup 是否会污染真实 daemon,之后恢复本机 `mac.lab` live state.

## [2026-07-18 00:44:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 收紧 e2e 资源隔离

- 发现 `self_target_should_error_when_no_local_daemon_running` 会删除 `$TMPDIR` 下所有 `rdog-*.pipe_uplink`,可能破坏测试进程之外的真实 daemon.
- 该风险只有静态证据,不能表述为本次现场根因;但它阻止安全运行完整 unixpipe e2e.
- 处理: 改用每轮唯一 namespace、独立 `XDG_STATE_HOME`,只清理该 namespace 前缀和本轮 state home.
- 完成后再串行运行整个 `zenoh_unixpipe_fast_path` 测试文件.

## [2026-07-18 00:49:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 显式 socket_path 反例动态确认

- 隔离实验使用不同 daemon name、`local_default=false`、同一显式 `socket_path`.
- 第一实例 ready 后 uplink inode=`512007699`;第二实例也成功 ready,随后同一路径 inode=`512007707`.
- 结论: service-name guard 不能作为 FIFO cleanup 的完整 ownership 证明;不同 identity 可共享显式 base path.
- 修复升级: 在 cleanup 前增加基于 canonical base path 的 PID guard,guard 生命周期覆盖 router 主循环.
- TDD: 先增加 different-name/shared-explicit-path e2e 并跑出 RED,再实现 path guard.

## [2026-07-18 01:33:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: path ownership 与安全 e2e 收口

### RED / GREEN

- shared explicit path exact test RED:第二实例不同 daemon name 也成功 ready,uplink inode 从 `512009597` 变为 `512009604`.
- 增加 canonical base sidecar PID guard,并将 guard acquire + stale cleanup 封装为 `prepare_unixpipe_listener`.
- 两条 ownership e2e GREEN:同名 local-default 与不同名共享显式 path 均通过.

### 单一真相源

- `compose_listen_endpoints` 现在同时返回最终 endpoints 与 resolved unixpipe base.
- 显式 listen endpoint、`socket_path`、默认推导不再由 daemon 二次计算.
- 多个显式 unixpipe endpoint或显式 endpoint与`socket_path`不一致时 fail-fast.
- compose focused tests:5 passed.

### 测试安全

- 把 test helper 提取到 `tests/zenoh_unixpipe_fast_path/support.rs`,主文件降至 800 行以下.
- `TestDaemon` / `TestStateHome` RAII 负责子进程、输出线程、配置和私有 state cleanup.
- 所有 namespace-sensitive 用例改为动态 namespace + 私有 `XDG_STATE_HOME`.
- 完整 `zenoh_unixpipe_fast_path`:11 passed,真实 PID 32191 未变,用户 guard 目录/临时 TOML/测试进程均无新增残留.

### 本阶段遇到并已处理的错误

- shared-path 测试首版把 `Result<u64>` 与 `Option<u64>` 比较,编译 E0369;改为 `.ok().map(...)` 后进入预期 RED.
- support 模块首版使用 `mod support;`,Rust integration test 实际寻找 `tests/support.rs`;改为仓库既有模式的显式 `#[path = "zenoh_unixpipe_fast_path/support.rs"]`.
- control helper 重构时静态复查发现可能双 spawn,在任何测试前改回单 Command/单 spawn.
- 一次大块 apply_patch 因上下文不匹配未应用,随后拆成小补丁完成,没有留下部分修改.

### 当前状态

**阶段5继续**: 执行 runtime/router/check 回归,再安装新二进制并恢复真实 local-default live state.

## [2026-07-18 01:45:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: router-client 隔离路径约束

### 已观察现象

- 隔离运行 `zenoh_router_client` 时 4 个用例失败,关键错误均为 unixpipe base 路径达到 111-114 字节,超过 95 字节上限.
- 该轮使用的 sandbox 位于较长的仓库临时目录下,失败发生在 daemon 启动前的路径长度校验,不是 query/reply 行为断言失败.

### 结论与下一步

- 当前结论: 这轮失败验证了路径长度保护正常生效,不能作为本次 ownership 修复的回归证据.
- 保持 `TMPDIR` 与 `XDG_STATE_HOME` 隔离,改用短路径 `/tmp/rr.*` 重跑同一测试文件.
- 重跑通过后执行 touched-file rustfmt、all-targets check、独立 diff review,最后安装 CLI 并做真实 daemon 双启动 smoke.

### 当前状态

**阶段5继续**: 先用短 sandbox 复跑 `zenoh_router_client`,不取消测试环境隔离.

## [2026-07-18 01:50:00] [Session ID: omx-1784304547353-h5409r] [错误记录]: zsh 包装变量冲突

- 短路径重跑的测试本体报告 `26 passed, 2 skipped`,证明 95 字节问题已被短 sandbox 消除.
- 测试结束后的包装脚本使用变量名 `status`,zsh 报 `read-only variable: status`;这会让命令整体状态和清理动作不可信.
- 处理: 检查并仅删除本轮 `/tmp/rr.*` 残留,把变量改为 `exitCode`,重新执行同一测试并要求 shell 整体 exit 0.

### 当前状态

**阶段5继续**: router-client 测试本体已绿,等待包装脚本无错误复验后再推进编译 gate.

## [2026-07-18 02:06:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: router-client 复验通过

### 验证证据

- 使用短路径 `/tmp/rr.*`,并继续隔离 `TMPDIR` 与 `XDG_STATE_HOME`.
- `cargo nextest run --package rustdog --test zenoh_router_client -j 4` 整体 exit 0.
- 关键输出: `26 passed, 2 skipped`,耗时 8.863s.
- 包装脚本改用 `exitCode`,并通过 EXIT trap 清理 sandbox;没有再出现 zsh 变量错误.

### 待办事项

- [x] 短路径隔离重跑 `zenoh_router_client`.
- [ ] 对本轮涉及的 Rust 文件执行 scoped rustfmt.
- [ ] 执行 `cargo check --all-targets` 和必要 build.
- [ ] 复查完整 diff,处理本轮引入的问题.
- [ ] 安装当前工作树 CLI,重启真实 daemon 并执行重复启动 smoke.
- [ ] 完成支线上下文收口.

### 当前状态

**阶段5继续**: 进入格式化、编译与代码审查 gate.

## [2026-07-18 02:10:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: scoped 静态 gate 通过

### 验证证据

- 本轮 5 个 Rust 文件执行 `rustfmt --edition 2021 --check`,exit 0.
- `git diff --check`,exit 0.
- 集成测试主文件已降至 683 行,support helper 为 340 行.

### 结构风险

- `src/zenoh_control.rs` 当前 1075 行,`src/zenoh_runtime.rs` 当前 1848 行,超过项目建议的 1000 行上限.
- 这是当前模块职责持续增长的结构性问题,但不在本次 FIFO ownership 修复中混入大规模拆分;完成当前 live recovery 后记录到延期项.

### 待办事项

- [x] 对本轮涉及的 Rust 文件执行 scoped rustfmt.
- [ ] 执行 `cargo check --all-targets`,读取 warning/error 完整输出.
- [ ] 执行当前 CLI build,复查完整 diff.
- [ ] 安装并执行真实 daemon 重启/重复启动 smoke.

### 当前状态

**阶段5继续**: 运行 all-targets 编译 gate.

## [2026-07-18 02:13:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: all-targets check 通过并保留 warning 基线

### 验证证据

- `cargo check --all-targets --message-format short`,exit 0,耗时 14.93s.
- bin 构建报告 6 个 warning,test 构建报告 8 个 warning.
- warning 全部位于既有 `control_actions.rs` / `control_computer_act/*`,本轮修改的 daemon、Zenoh runtime、unixpipe e2e 和 support helper 没有 warning.

### warning 处理边界

- 这些 warning 仍是需要处理的代码质量债务,不能描述成 warning-free.
- 当前任务不修改上述 control-act 文件,避免把另一条工作线混入 local-default 生命周期修复.
- 完成当前修复后把 warning 清理和超长模块拆分一起登记到延期项.

### 待办事项

- [x] 执行 `cargo check --all-targets`,读取 warning/error 完整输出.
- [ ] 执行当前 CLI build.
- [ ] 检查本轮 diff、结构影响和协议文档一致性.
- [ ] 安装并执行真实 daemon 重启/重复启动 smoke.

### 当前状态

**阶段5继续**: 运行 build 并进入提交前代码审查.

## [2026-07-18 02:18:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: build 通过,补齐 review 测试缺口

### build 与结构审查

- `cargo build --package rustdog --bin rdog --message-format short`,exit 0,耗时 0.62s.
- build 仍报告同一组 6 个既有 control-act warning,本轮文件没有新增 warning.
- CodeGraph 与生产 diff 确认 ownership-sensitive 路径只有 router daemon 启动链;cleanup 已收敛到 `prepare_unixpipe_listener` 内部.
- 协议文档已同步 resolved base、两级 guard 与 destructive cleanup 顺序.

### review 发现的测试缺口

- `compose_listen_endpoints` 已实现多个显式 unixpipe endpoint 的 fail-fast,但缺少直接单测.
- path owner guard 已实现 stale PID 接管,现有 e2e 只覆盖活跃 owner 拒绝,缺少崩溃残留 guard + FIFO 的恢复测试.
- 处理: 增加两个 focused 单测,再重跑 runtime tests、完整 unixpipe e2e、router-client 和格式/编译 gate.

### 当前状态

**阶段5继续**: 补两个行为测试,不改变生产语义.

## [2026-07-18 02:24:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: review 补测通过

### 新增覆盖

- 多个显式 unixpipe endpoint 直接返回 `InvalidInput`,避免 listener 与 cleanup 路径分裂.
- stale owner sidecar(PID 0)可被接管,三份残留 unixpipe 文件被清理,新 guard drop 后 sidecar 被删除.

### 验证证据

- `rustfmt --edition 2021 src/zenoh_runtime.rs`,exit 0.
- 两个精确 nextest 用例: `2 passed, 605 skipped`,exit 0,耗时 0.311s.

### 当前状态

**阶段5继续**: 扩大到完整 runtime 单测和 unixpipe e2e.

## [2026-07-18 02:27:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: runtime 与 unixpipe e2e 全绿

### 验证证据

- `zenoh_runtime::tests`: `32 passed, 575 skipped`,exit 0,耗时 4.159s.
- 完整 `zenoh_unixpipe_fast_path`: `11 passed`,exit 0,耗时 6.506s.
- e2e 串行运行,所有 namespace-sensitive 用例继续使用动态 namespace 和私有 state home.

### 待办事项

- [x] 检查本轮 diff、结构影响和协议文档一致性.
- [ ] 复验用户 PID、tmux 会话、端口、guard/registry 与安装版来源.
- [ ] 安装当前工作树 CLI.
- [ ] 终止旧 daemon,用新 CLI 启动真实 daemon.
- [ ] 执行 ping -> 重复启动失败 -> ping 的 live smoke.
- [ ] 完成支线上下文与延期风险收口.

### 当前状态

**阶段5继续**: 开始 live recovery 前只读检查.

## [2026-07-18 02:32:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 真实环境重启前基线确认

### 运行态证据

- PID 32191 仍由 `rdog-daemon` tmux 会话持有,命令是当前仓库 `target/debug/rdog daemon -c rdog_macos.toml`.
- PID 32191 正在监听 UDP 7447,service-name guard `lab__mac.lab.pid` 内容为 32191.
- 显式 `rdog control mac.lab @ping` exit 0,返回 `@response "pong"`.
- 裸 `rdog control @ping` exit 1,仍报告 `wechat.ax.test` / `wechat.wechatax` 两个候选且没有 local-default registry.
- `local-default/` 为空;canonical base、uplink、downlink 和新的 owner sidecar 均不存在.

### 安装策略

- 先安装当前工作树 CLI,安装成功前不停止旧 daemon.
- 安装完成后再终止旧 tmux 启动终端,确认 PID 和 UDP 7447 都释放,避免留下孤立 GUI/daemon 进程.

### 当前状态

**阶段5继续**: 执行 `cargo install --path . --force --locked`.

## [2026-07-18 02:39:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 当前工作树 CLI 安装完成

### 安装证据

- `cargo install --path . --force --locked`,exit 0.
- release 构建耗时 2m08s,最终替换 `/Users/cuiluming/.cargo/bin/rdog`.
- 本项目仍报告同一组 6 个既有 control-act warning,本轮文件无新增 warning.
- Cargo 另报告 lockfile 中 `spin 0.9.8`、`spin 0.10.0`、`stabby 72.1.1` 已 yanked;安装未失败,后续登记依赖治理.

### 当前状态

**阶段5继续**: 终止旧 tmux daemon,确认 PID/端口释放后启动安装版.

## [2026-07-18 10:19:00] [Session ID: omx-1784304547353-h5409r] [错误记录]: 前序追加记录误用 UTC 时间

- 本机 `date` 输出为 `2026-07-18 10:18:51 CST`.
- 本 Session 从 02:06 到 02:39 的追加标题误沿用 UTC 口径,正文证据与执行顺序不受影响.
- 六文件是 append-only,不回改既有标题;从本条开始统一使用 Asia/Shanghai 本机时间.

### 当前状态

**阶段5继续**: 按计划停止旧 daemon 并检查资源释放.

## [2026-07-18 10:20:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 旧 daemon 已干净停止

### 验证证据

- `tmux kill-session -t rdog-daemon`,exit 0.
- 有界轮询确认 PID 32191 已退出.
- `lsof -nP -iUDP:7447` 无占用,UDP 7447 已释放.

### 当前状态

**阶段5继续**: 用安装版 CLI 重建 `rdog-daemon` 会话并等待 ready.

## [2026-07-18 10:22:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 新版真实 daemon ready

### 启动证据

- 安装版 `/Users/cuiluming/.cargo/bin/rdog` 已在新 `rdog-daemon` tmux 会话启动.
- 日志出现 `zenoh unixpipe fast path 启用`,base 为 canonical `rdog-lab-mac.lab.pipe`.
- 日志出现 `zenoh unixpipe local-default 已注册: namespace=lab, daemon_name=mac.lab`.
- 日志出现 router ready,最终 endpoints 包含 canonical unixpipe 和 `udp/0.0.0.0:7447`.

### 当前状态

**阶段5继续**: 执行裸 ping -> 重复启动失败 -> 状态不变 -> 裸 ping 的真实 smoke.

## [2026-07-18 10:24:00] [Session ID: omx-1784304547353-h5409r] [状态更新]: 第一次真实裸 ping 通过

### 验证证据

- `rdog control @ping`,exit 0,明确日志显示命中 canonical unixpipe fast path.
- 响应为 `@response "pong"`.
- 新 daemon PID/service guard/owner sidecar 均为 69053,UDP 7447 由该 PID 监听.
- canonical uplink inode 为 512111047;`local-default/lab.json` 与 `lab.pid` 已创建.

### 探针错误

- 状态探针尝试 `stat` canonical `_downlink`,得到 NotFound.当前 Zenoh 的 downlink 使用会话后缀或在连接结束后回收,不能把 canonical `_downlink` 作为常驻不变量.
- 后续重复启动前后只比较 canonical uplink inode、owner PID、service/local-default guard 和 daemon PID.

### 当前状态

**阶段5继续**: 读取 registry 内容和实际 FIFO 集合,然后执行预期失败的第二次 daemon 启动.

## [2026-07-18 10:21:44] [Session ID: omx-1784304547353-h5409r] [状态更新]: 真实重复启动 smoke 通过

### 时间校正

- 本条写入前的本机时间为 `2026-07-18 10:21:44 CST`.
- 前两条 10:22/10:24 标题使用了估算值,比实时时钟快数分钟;append 顺序与正文证据仍是实际执行顺序,不回改既有记录.

### 重复启动结果

- 第二次 `rdog daemon -c rdog_macos.toml` exit 1,在 service-name guard 处拒绝重复实例.
- 输出没有出现 unixpipe cleanup/local-default 注册日志,证明失败实例未进入 destructive 初始化.
- 重复启动前后 canonical uplink inode 都是 512111047.
- owner sidecar、service-name guard、local-default guard 与存活 daemon PID 均保持 69053.
- 重复启动后的裸 `rdog control @ping` 与显式 `rdog control mac.lab @ping` 都 exit 0,命中 unixpipe fast path并返回 `@response "pong"`.

### 当前状态

**阶段5验证完成**: 执行最终格式、diff、all-targets check和残留检查后进入阶段6文档收口.

## [2026-07-18 10:23:45] [Session ID: omx-1784304547353-h5409r] [完成]: 支线任务收口

### 阶段状态

- [x] 阶段1: 读取历史上下文、项目规则与既有 local-default 契约.
- [x] 阶段2: 采集 PID 文件、进程、registry、FIFO、配置和二进制来源的运行态证据.
- [x] 阶段3: 沿真实调用路径核对状态创建、校验与清理逻辑,提出主假设和最强备选解释.
- [x] 阶段4: 用最小可证伪实验区分环境残留与生命周期代码缺陷.
- [x] 阶段5: 完成代码修复、回归测试、编译、安装和真实重复启动 smoke.
- [x] 阶段6: 完成 notes、WORKLOG、ERRORFIX、LATER_PLANS、EPIPHANY回溯与记录.

### 最终验证矩阵

- focused review测试:2 passed.
- Zenoh runtime测试:32 passed.
- unixpipe e2e:11 passed.
- router-client:26 passed,2 skipped.
- scoped rustfmt、diff check、all-targets check、debug build、release install:全部exit 0.
- 真实 daemon:重复启动正确失败,前后裸 ping均命中 unixpipe并返回pong.
- 残留检查:只有预期tmux父进程与daemon,没有近期测试sandbox或临时TOML.

### 回溯结论

- `LATER_PLANS` 保留原子状态源、stale guard审计、超长模块、warning、yanked依赖和suffix FIFO治理.
- `EPIPHANY_LOG` 记录资源路径ownership必须先于destructive cleanup的通用规律.
- 没有已经落地或失效的支线延期项需要删除.

### 状态

**已完成**: 本机 `rdog control @ping` 已恢复,新版 daemon PID 69053在 `rdog-daemon` tmux会话持续运行.

## [2026-07-18 10:50:38] [Session ID: omx-1784304547353-h5409r] [继续]: scoped commit 收口

### 触发

- 用户要求"按建议继续".
- 上一轮明确建议的下一步是提交已经完成验证的 local-default ownership修复.

### 待办事项

- [ ] 核对当前分支、子模块和工作区边界.
- [ ] 仅暂存本任务的生产代码、测试、规格和上下文文件.
- [ ] 审查 staged diff、运行 staged边界与格式检查.
- [ ] 创建 scoped commit,不混入其他工作线.
- [ ] 检查提交结果、剩余工作区和运行中 daemon.

### 状态

**阶段7进行中**: 收集提交边界证据,尚未暂存任何文件.

## [2026-07-18 10:51:17] [Session ID: omx-1784304547353-h5409r] [状态更新]: 提交边界确认

- 当前分支: `main`.
- 仓库不存在 `.gitmodules`,`git submodule status` 无输出,没有子模块指针需要同步提交.
- 当前 modified/untracked文件均属于本任务的生产代码、测试、规格、主线摘要与后缀六文件.
- 暂存策略: 使用显式文件清单,禁止 `git add .`.

### 状态

**阶段7进行中**: 暂存明确清单并审查 cached diff.

## [2026-07-18 10:52:05] [Session ID: omx-1784304547353-h5409r] [状态更新]: staged边界通过审查

- 暂存15个文件,共1700行新增、474行删除.
- staged name-status与预期清单完全一致,没有子模块或无关路径.
- `git diff --cached --check`通过,工作区没有 unstaged改动.
- 提交前门槛:runtime测试、unixpipe e2e、scoped rustfmt与 live裸 ping必须再次通过.

### 状态

**阶段7进行中**: 执行最终提交前验证.

## [2026-07-18 10:52:51] [Session ID: omx-1784304547353-h5409r] [状态更新]: commit gate通过

- Zenoh runtime测试:32 passed,575 skipped.
- unixpipe e2e:11 passed.
- scoped rustfmt:exit 0.
- live `rdog control @ping`:exit 0,命中unixpipe并返回pong.

### 状态

**阶段7进行中**: 验证已完成,下一步创建 scoped commit.

## [2026-07-18 10:53:37] [Session ID: omx-1784304547353-h5409r] [完成]: scoped commit阶段收口

### 阶段状态

- [x] 核对当前分支、子模块和工作区边界.
- [x] 仅暂存本任务的生产代码、测试、规格和上下文文件.
- [x] 审查 staged diff、运行 staged边界与格式检查.
- [x] 创建 scoped commit,没有混入其他工作线.
- [x] 检查提交结果、剩余工作区和运行中 daemon.

### 提交结果

- commit message: `fix(zenoh): protect active unixpipe listener ownership`.
- commit包含15个本任务文件,提交后工作区干净.
- 提交后 `rdog control @ping` 仍命中unixpipe并返回pong.

### 状态

**阶段7已完成**: 将本条记录amend进同一commit后,转入独立的原子lease后续任务.
