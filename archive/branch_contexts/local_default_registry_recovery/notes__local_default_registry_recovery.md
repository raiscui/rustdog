# local-default registry 恢复与一致性验证笔记

## [2026-07-18 00:25:00] [Session ID: omx-1784304547353-h5409r] 笔记: 现场状态与启动顺序

### 现场动态证据

- 当前 shell 的安装版是 `/Users/cuiluming/.cargo/bin/rdog`,版本 `rustdog 3.0.0`,mtime 为 2026-07-17 23:17.
- 既有 daemon 是 PID 32191,命令为仓库 `target/debug/rdog daemon -c rdog_macos.toml`,启动于 2026-07-17 23:13.
- tmux `rdog-daemon` 日志依次包含 unixpipe fast path 启用、local-default 注册成功、router ready.
- `lab__mac.lab.pid` 内容仍为 32191,UDP 7447 仍由该进程持有;显式 `rdog control mac.lab @ping` 返回 pong.
- 采样时 `~/.local/state/rustdog/local-default/` 为空,canonical `rdog-lab-mac.lab.pipe_uplink` 和 `_downlink` 不存在.
- `$TMPDIR` 只剩多个历史 suffix FIFO 和两个可被 fallback 识别的 WeChat FIFO,与用户看到的候选一致.

### 静态调用顺序

- `src/daemon.rs::run_zenoh_router` 先调用 `cleanup_stale_unixpipe_socket(base_path)`.
- 同一函数随后调用 `register_local_default_daemon(...)`.
- service-name guard 直到 `src/zenoh_control.rs::run_router_daemon` 内才获取.
- `cleanup_stale_unixpipe_socket` 对 base、uplink、downlink 只要不是目录就直接 `remove_file`,没有先验证活跃 owner.
- `find_local_daemon_name` 会把 PID 存活但 uplink 缺失且超过启动宽限的 registry 当作 stale,并同时删除 JSON 与 namespace PID guard.

### 当前结论等级

- 已验证结论: 现场存在"daemon 进程与 service-name guard 活着,但 unixpipe FIFO 和 local-default registry 消失"的 split-brain 状态.
- 候选根因: 同名第二次启动在 ownership guard 前清理 FIFO. 静态路径与用户日志顺序支持它,仍需隔离双启动 e2e 动态复现后才能定为根因.
- 最强备选: 其他测试或进程删除了 FIFO. 双启动实验若无法复现,应转查 FIFO 删除者和 local-default JSON/guard 分裂窗口.

## [2026-07-18 00:32:00] [Session ID: omx-1784304547353-h5409r] 笔记: 最小实验推翻备选解释

- 隔离 namespace `dupstart`、独立 `XDG_STATE_HOME`、同名两个 daemon 的 e2e 稳定复现.
- 第一实例 ready 且 canonical uplink 已创建后才启动第二实例,排除了"FIFO 启动时尚未出现".
- 第二实例日志先打印 unixpipe fast path 启用,再因活跃 local-default guard 退出.
- 测试随后观察到第一实例 canonical uplink 已被删除. 故"其他进程独立删除 FIFO"不是该实验的必要解释.
- 结论: 根因是 `run_zenoh_router` 在 service-name/local-default ownership guard 前执行 destructive stale cleanup.

### 审计补充

- 现有测试覆盖 dead PID、missing uplink、启动宽限和有效 registry 优先,但没覆盖重复 daemon 对活跃 FIFO 的非破坏性.
- PID guard 与 JSON registry 是双文件状态,仍有 JSON 缺失/损坏但 live PID 存在、PID 复用、写入中断等未覆盖一致性风险. 这些不是本次动态复现所需条件,不混入当前修复.

## [2026-07-18 00:39:00] [Session ID: omx-1784304547353-h5409r] 笔记: 修复后的 ownership 顺序

- `ZenohUnixpipeStartupConfig` 只携带 `base_path` 与 `local_default`,不在 daemon 配置层执行文件副作用.
- service-name guard 成为 router 共享状态初始化的第一道门禁.
- 同名第二实例在 guard 处退出,不会到达 FIFO cleanup;该保护同样覆盖 `local_default=false`.
- 合法重启在旧 PID 已死亡时仍能接管 service-name guard,随后执行原有 stale cleanup,不破坏崩溃恢复不变量.
- local-default guard 在 `run_router_daemon` 栈帧中持有到 router 主循环结束,生命周期没有缩短.

## [2026-07-18 00:44:00] [Session ID: omx-1784304547353-h5409r] 笔记: e2e cleanup 风险

- `self_target_should_error_when_no_local_daemon_running` 原实现为制造"没有 daemon"环境,遍历真实 `$TMPDIR` 并删除所有 `rdog-*.pipe_uplink`.
- 这会跨 namespace 删除测试未创建的资源,与 e2e 隔离原则冲突;若真实 daemon 正在运行,其进程仍活着但 fast path 会被测试破坏.
- 当前没有动态证据证明这条测试参与用户本次命令序列,因此只记录为独立的已确认测试缺陷.
- 正确边界是唯一 namespace + 独立 state home,不需要也不允许清空全局候选.

## [2026-07-18 00:49:00] [Session ID: omx-1784304547353-h5409r] 笔记: base path ownership 才是 cleanup 真相源

- service-name guard 的 key 是 `(namespace, daemon_name)`,只能保护默认推导路径下的同名重复启动.
- `unixpipe.socket_path` 可让不同 daemon identity 指向同一个 base path;两者能分别拿到 service-name guard.
- 隔离动态实验确认第二实例会 unlink/recreate 第一实例 FIFO,inode 从 `512007699` 变为 `512007707`.
- 因此 destructive cleanup 的单一真相源必须是 canonical base path ownership,不能由 daemon identity 间接推断.
- 建议 sidecar PID guard 与 base path 一一映射,先 acquire,再 cleanup;崩溃后 dead PID 允许接管并保留 stale cleanup 能力.

## [2026-07-18 01:33:00] [Session ID: omx-1784304547353-h5409r] 笔记: 最终启动不变量

1. endpoint composition 产出最终 listener 列表和唯一 resolved base.
2. service-name guard 证明当前进程拥有 daemon identity.
3. base-path sidecar guard 证明当前进程有权操作该 FIFO path.
4. 两把 guard 均成功后才允许 stale cleanup.
5. local-default guard/record 在 cleanup 后注册,并与 router 主循环同生命周期.
6. Zenoh session 关闭后,local-default guard和path guard再按逆序释放.

### 测试 fixture 结论

- 旧 helper 的默认 HOME state + `Child::kill()` 会留下大量 stale service-name guard和临时 TOML.
- 新 RAII fixture 把 state 隔离到临时目录,Drop 时 kill/wait、join collector、删除 config/state.
- FIFO cleanup 不能归每个 daemon fixture所有,因为两个 fixture可能共享 base;场景测试在两个 daemon 都 Drop 后再做一次 base cleanup.
- full e2e 后对真实 guard目录、临时 config、测试进程做了反查,均无本轮残留.

## [2026-07-18 10:22:47] [Session ID: omx-1784304547353-h5409r] 笔记: 最终 review 与真实环境恢复

### review 结论

- 生产 diff 将 endpoint 列表与 resolved base 收敛为 `ComposedListenEndpoints`,daemon 不再二次推导 cleanup 路径.
- `prepare_unixpipe_listener` 把 path guard 与 stale cleanup 固定成不可颠倒的顺序.
- CodeGraph 与实际 diff 确认 router 启动链持有 service-name、path、local-default 三类 guard直到主循环退出.
- 新增直接测试覆盖多个显式 endpoint拒绝,以及 stale path owner + 残留文件接管.
- 没有发现本轮引入的 warning;现存 warning 均位于 control-act 工作线.

### 最终测试证据

- focused 新测试:2 passed.
- `zenoh_runtime::tests`:32 passed.
- `zenoh_unixpipe_fast_path`:11 passed.
- 短路径隔离 `zenoh_router_client`:26 passed,2 skipped.
- scoped rustfmt、`git diff --check`、`cargo build --bin rdog`、`cargo check --all-targets` 均 exit 0.

### 真实环境证据

- 修复前:PID 32191 仍能响应显式 ping,但 local-default 目录和 canonical FIFO 均缺失;裸 ping因两个 WeChat FIFO 候选失败.
- 安装当前工作树后,旧 tmux daemon 已停止,PID 32191 与 UDP 7447 均释放.
- 新安装版 daemon PID 69053 ready,registry、service guard、path owner和 canonical uplink均指向同一实例.
- 第一次裸 ping命中 unixpipe并返回 pong.
- 第二次同配置 daemon启动在 service-name guard 处 exit 1,没有进入 unixpipe cleanup.
- 重复启动后 uplink inode仍为512111047,三个 guard PID仍为69053,裸/显式 ping继续返回 pong.

### 仍需后续处理

- local-default JSON + namespace PID guard是双文件状态,仍有写入中断、PID复用和损坏恢复的一致性风险.
- 用户级 zenoh-guards目录存在大量历史 stale PID文件,需要单独设计只读审计与安全清理工具,不能在本轮直接批量删除.
- `src/zenoh_control.rs` 与 `src/zenoh_runtime.rs` 已超过1000行;模块拆分应单独规划.
- lockfile含3个已yanked依赖,control-act有6/8个编译 warning,均应另开治理任务.
