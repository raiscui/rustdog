# CI 合并门: 必需检查表与「未运行 / 失败」口径

本文件是 `.github/workflows/ci.yml` 的配套契约: **门上到底等哪几条检查**, 以及检查红了
时怎么判是"没跑到"还是"跑失败"。

## 0. 为什么有这份文件

2026-11-26 实锤 (RAI-397): `main` 的 `test (windows-latest)` 连续红了一整天, 一次测试都
没跑到 —— 红的形状与"测试真的失败"完全一样, 只能人肉翻日志才看出是 `ort-sys` 的 build
script 在 `copy-dylibs` 里 panic。同一时期 `main` 上还有 `recording_e2e` 夹具和
`figment::Jail` 两处红, 三个红点混在一起, 合并门实际上退化成"人工归因"。

结论: **"没跑到" 与 "失败" 必须是两个可辨识的事实**, 而且这个事实要由 job 名和注解承载,
不能靠谁记得去读日志。

## 1. 必需检查表

`main` 的分支保护必需检查必须与下表逐项一致 (既不多等一条, 也不白跑一条):

| 检查名 | 含义 | 红了的含义 |
| --- | --- | --- |
| `build (ubuntu-latest)` | 编译 `rdog` 二进制 | 代码 / 工具链 / 原生依赖 / 外部依赖的问题 |
| `build (macos-latest)` | 同上 | 同上 |
| `build (windows-latest)` | 同上 (含 `ort-sys` 的 DLL 铺放断言) | 同上 |
| `test (ubuntu-latest)` | 跑 `cargo test --bin rdog` + `recording_e2e` | 测试失败 |
| `test (macos-latest)` | 同上 | 测试失败 |
| `test (windows-latest)` | 跑 `cargo test --bin rdog` (Windows 上无 e2e, 见 §4) | 测试失败 |
| `knowledge carriers` | 载体断言 + 记账报告 | 载体与 `src/` 分叉 |

**检查名的真相源是 `.github/workflows/ci.yml` 里的 job 名**, 不是本表 —— 而且**不要改用 reusable workflow (`uses:`)** : GitHub 会把被调用工作流里的检查重命名成 `<caller name> / <callee name>` (实测 `build (macos-latest) / build`), 必需表里这些名字就永远不报到了。三个 OS 的 build / test 步骤因此是复制的, 改一处要改三处。

**检查名的真相源是 `.github/workflows/ci.yml` 里的 job 名**, 不是本表。改 job 名必须同票
改本表 **和** 仓库设置里的必需检查列表 —— 后者只有仓库管理员能改, 改 workflow 的人拿不到
这个权限时, 在 PR 里写明"需要在 Settings → Branches 里换名字", 不要静默放行。

> 注意: 本仓库的 GitHub App 身份**读不到** branch protection (`403 Resource not accessible
> by integration`), 所以"设置侧是否已对齐"只能由人核对。这条是已知缺口, 不是本文件的省略。

## 2. 触发面

```yaml
on:
  push: {branches: [main]}
  pull_request: {branches: [main], types: [opened, synchronize, reopened, ready_for_review]}
  workflow_dispatch:
```

`pull_request.types` **必须显式列 `ready_for_review`**。默认类型不含它, 草稿期开立的 PR
标 ready 后一个 run 都不会起, 而门会把"该 head 0 个必需检查"读成"没有失败" (2026-11-26
在 `raiscui/pireel` 的真实事故形态)。

## 3. 红点怎么读

| 现象 | 结论 | 去哪看 |
| --- | --- | --- |
| `build (os)` 红, 同 OS 的 `test (os)` **skipped** | **测试未运行**。门应当拦住, 但不能当成"测试失败"记 | `build` job 里失败步骤的 `::error title=…` 注解 |
| `build (os)` 绿, `test (os)` 红 | 测试真的失败 | `test` job 的 `Unit tests` / 断言步骤日志 |
| `test (os)` 绿, 但日志里 `tests did not run` 是红的 | 断言步骤红 = 零测试被执行 | `assert-tests-ran.py` 输出 |
| `build (os)` 红的注解是 `build prep failed` | 准备阶段 (工具链 / 拉取 / 目录) 出了问题, 不是测试结论 | `cargo-prep` 步骤日志 |
| 该 head 一个 run 都没有 | `missing-checks`, 门结构性缺口 | `ci-check` skill 第 2/3 检查 |
| `knowledge carriers` 红 | 载体断言与 `src/` 真相源分叉 | 脚本输出 |

两类"未运行"的注解标题是本口径的接口:

- `onnxruntime dist unavailable` / `onnxruntime dist digest mismatch` — 外部依赖拿不到或被篡改
  (见 §4), 测试没跑;
- `build prep failed` — 准备阶段异常 (工具链 / registry 等), 同样没有测试证据;
- `tests did not run` — 测试框架退出码 0 但执行数为 0 (filter 打错, 或 `#[cfg(windows)]`
  之类平台门把整组测试编译掉了)。

**这些都不许改判成绿。** 需要的是"可辨识", 不是"放行"。

两条结构性保证 (2026-11-26 第一版实现被评审指出后补的):

- **每个 OS 的 `test` 只依赖自己 OS 的 `build`** (六个独立 job, 不是一对 matrix),
  所以 Windows 的外部依赖红了不会连带吞掉 ubuntu / macOS 的测试证据;
- **测试目标在 build 阶段用 `--no-run` 编译** —— 范围是 bin 的 `#[cfg(test)]` 模块与
  (非 Windows 的) `recording_e2e`。否则"测试代码编译不过"会落在 test 阶段, 把 cargo
  的编译失败与测试失败混成一个红点, 零执行断言还会被跳过。

### 3b. integration target 的覆盖范围 (RAI-411 之后)

`tests/` 下 19 个 target 现在由 `scripts/ci/run-integration-targets.py` 按平台执行, **每个
target 的 `passed` 必须 > 0**, 否则判红 —— 「退出码 0 但零执行」在 CI 上同样算失败。

按平台清单的真相源是 `scripts/ci/run-integration-targets.py` 的 `CLASSIFICATION`:
每个 target 标 `all` (三平台) / `unix` (ubuntu + macos) / `macos` / `windows` /
`compile-only:macos` / `exempt:<理由>`。**`tests/` 下新增文件而没登记 = 红** (脚本会枚举
`tests/*.rs` 双向对账), 所以「新增的 target 悄悄没人跑」这条不可达。
比整片豁免细一档的出口是 `TARGET_TEST_SKIPS` (**具名跳过**): target 照常执行并计入 ran 集合,
只按名字跳过列出的用例; 名字进 job summary, 且脚本用 `-- --list` 验证名字真实存在 —— 打错名字
会判红, 不会变成「summary 说跳过了、其实全跑」或者反过来。它的成立条件是**根因已定位的同一族
缺陷**, 不是「下一轮换个用例继续红」。

| 平台 | 执行的 target |
| --- | --- |
| ubuntu | `connect_modes`、`control_lanes`、`control_mode`、`control_pty`、`control_tty`、`shell_pty`、`daemon_inbound`、`listen_suspend`、`computer_use_density`、`control_websocket`、`zenoh_router_client`、`tls_link` |
| macos | ubuntu 那 12 个 target 全部, 且无具名跳过 |
| windows | `computer_use_density`、`tls_link` (另有 2 个 target 在 windows 显式豁免, 见下) |
| 全平台 (build 阶段) | `cargo test --no-run` 编译**全部** target, 含下面的 compile-only 面 |
| compile-only (macos) | `zenoh_unixpipe_fast_path` (issue #164)、`control_ax_e2e`、`control_display_aware_e2e`、`control_mouse_e2e`、`control_ocr_e2e`、`control_window_e2e` |

**各平台实际执行 target 数 (每次放量或豁免变更都更新这张表)**:

| 平台 | 实际执行的 integration target 数 | 具名跳过 / 豁免 / 只编译 |
| --- | --- | --- |
| ubuntu | **12** | 无具名跳过 (RAI-524 已撤, 见下); 20 处 `#[ignore]` 见 §6 |
| macos | **12** | 6 个 compile-only (`zenoh_unixpipe_fast_path` 见 issue #164; 5 个 GUI 面需真实桌面会话) |
| windows | **2** (`computer_use_density`、`tls_link`) | `control_websocket`、`zenoh_router_client_windows` (issue #162) |

**判读口径: 「三平台绿」在 windows 上只代表 2 个 integration target, 不能读成「windows 的 e2e 面
在跑」; 读绿点必须连着这三个数字一起读。** 数字与 `PLATFORM_EXEMPTIONS` 是同一次改动的东西:
加/撤一条豁免就要改这里, 否则下一份报告会拿旧的覆盖数去说新的绿。

**ubuntu 的 integration 步骤必须在虚拟 X display 下执行** (RAI-464): `@key` / `@type` 的真实注入
走 enigo, 而 linux 后端要先连上 X11 或 Wayland display; runner 两样都没有时 `Enigo::new` 直接返回
`no connection could be established: (no successful connection)` (code 70), 「注入成功」类断言在
无显示会话的机器上**确定性**失败。`ci.yml` 的 `test-ubuntu` 用 `xvfb-run -a` 包住这一步, 断言保持
原样、环境按测试的假设显式提供 (同 `docs/solutions/test-failures/tty-term-dumb-environment-deterministic-failure.md`
对 `TERM` 的口径: 固定假设的环境, 而不是改断言或延长时间)。反向判据: 去掉 `xvfb-run` 后
`control_lanes::daemon_control_lane_should_accept_key_object_payload_with_request_id` 会在
0.1s 内以同一句 code 70 失败 —— 这是环境前提, 不是时序。

`recording_e2e` 仍由它自己那一步执行 (unix)。

**跑批脚本的编码要在两个方向上都固定 utf-8。** windows runner 的控制台是 cp1252: 只修解码
(`subprocess.run(..., encoding="utf-8")`) 会让下一步在**输出**端撞上 `UnicodeEncodeError`
(summary 表格与失败详情里有中文), 归因看起来却是「target 红」。2026-11-26 为这件事连修两轮。
现在 `main()` 开头把 `stdout` / `stderr` 一起 `reconfigure(encoding="utf-8", errors="replace")`。

**墙钟上限分三层, 缺一层就漏一类对象** (RAI-510):

| 层 | 值 | 缺了会怎样 |
| --- | --- | --- |
| 单 target | `TARGET_TIMEOUT_SECONDS` = 1500s | 挂住的 target 撑到 job timeout 被 cancel, 日志只剩 `The job has been cancelled` |
| 整 job 预算 | `job_timeout(60) − 已用 − JOB_RESERVE_SECONDS(180s)`; 起点由 **job 的第一个 step** 写 `$RUNNER_TEMP/job-started-at` (必须排在 Checkout / cargo-prep 之前, 否则前缀不计入) | 两次 1500s 挂起就把 60 分钟的 job 撑爆, 「点不出是哪个」原样复现 |
| 编译期 (`--no-run`, compile-only) | 同上两个 | `--no-run` 也会挂 (链接/依赖解析), 没有上限同样无名 |

`timeout-minutes` 与 `RDOG_JOB_TIMEOUT_MINUTES` **共用一个 YAML anchor** (`&job-timeout-minutes`, 定义在 `ci.yml` 第一个 job 的 `timeout-minutes` 上; **不能放文件头的自定义顶层键 —— GitHub Actions 只接受已知顶层键, `x-` 开头的键会让整份 workflow 判非法、run 直接 failure 且没有 job**),
不许散落字面量 —— 抄三遍、改一处漏两处, 就是下一次「预算算出剩余 3299s 却仍被 cancel」。

**CI 上起点缺失/为空/不可解析 = 判红, 不退回固定预算。** 静默退化会让 job 前缀重新隐形
(冷缓存 `cargo-prep` 几百秒), 于是「预算充足」这个结论变成猜的。本地跑 (无 `GITHUB_ACTIONS`)
才允许退回本步固定预算。

预算耗尽**不是跳过而是失败**: 未执行的 target 会被点名并判红 (`job 预算耗尽: …` 未执行),
因为「没跑」不能报成「通过」。

**超时按进程组整组终止, 不是只 kill 直接子进程。** `cargo test` 会把测试二进制和 daemon 派生
出去; 只 kill 直接子进程时它们变成孤儿留在 runner 上, 污染同一个 job 里后续 target (实测: 一个
挂死的 target 让后面 target 的全局状态断言连带变红, 看起来像另一个用例的问题)。POSIX 用
`start_new_session` + `killpg`, windows 用 `taskkill /F /T /PID` (按 PID 带 `/T`, 绝不按映像名)。

**kill 之后要回读, 不是「代码里加了 kill」**:

- POSIX: 按进程序号扫 `/proc`, 把该 pgid 下仍存活 (排除僵尸 `Z`) 的进程数写进日志
  (`killpg <pgid>, 回读 0 存活` / `组内仍存活 N 个: …`); 
- windows: **没有回读**, 只能看 `taskkill` 的返回码, 非 0 时把 stderr 写进日志并判该 target 失败;
- **不能因直接子进程已退出就跳过整组回收**: 「中间进程退出、后代还活着」正是孤儿的定义
  (2026-11-26 评审用本函数实测: 跳过形状下存活后代 1, 只有无条件 `killpg` 才是 0)。pgid 在
  spawn 时就固定 (`start_new_session` 保证 pgid == pid), 不由 `getpgid` 现查。
- **两条已知边界 (写下来, 因为它们让「回读 0」不等于「没有孤儿」)**:
  1. **POSIX**: 后代若自己 `setsid` 逃出进程组, `killpg` 打不到它; 日志只声明「**组内**回读」
     (`组内回读 0 存活`), 不声明「没有孤儿」。判定孤儿要看进程启动时间/父链, 不是看这句。
  2. **windows**: leader 已经退出时, `taskkill /T /PID <dead pid>` 只能报 not found, 后代不被回收
     —— 这种形状下 F1 的语义是「**诚实判红、孤儿仍在**」, 不是「杀干净了」。
- **windows 上 daemon 会主动脱离进程树**: `rdog daemon` 默认把自己重开成 `hidden-daemon --child`
  (`CREATE_NO_WINDOW | DETACHED_PROCESS`), `taskkill /T` 也够不着。所以跑 integration target 时
  统一注入 `RDOG_DAEMON_FOREGROUND=1`, 把 daemon 留在 job 的进程树里。

附带产物: 失败详情里写明终止方式与回读结果, summary 同时给出每个 target 的 `elapsed`。

**ubuntu 侧 1 个 target 显式豁免** (`PLATFORM_EXEMPTIONS`): `zenoh_router_client`。它是**两族**
不稳定叠在一起, 2026-11-26 (RAI-464) 已按证据拆开:

| 家族 | 机制 | 现状 |
| --- | --- | --- |
| `@key` 注入类 (`control_lanes` 的对象载荷用例 + 本 target 的 key 事件用例) | linux 输入后端要先连 X11/Wayland display; runner 没有时 enigo 直接报 `no connection could be established: (no successful connection)` (code 70), 与 control client 到 daemon 的连接**无关** | **已修**: ubuntu 的 integration 步骤改在 `xvfb-run` 下执行 (`ci.yml`), `control_lanes` 那条已撤 `#[ignore]` 并在 ubuntu 执行 |
| TLS/mTLS 两例 | `zenoh-link-tls` 1.8.0 让读任务与写任务可同时进入同一个 rustls `Connection` (`UnsafeCell` + 两把不同的锁), 首帧写入被以 `handshake not complete` 拒绝并关链路 (daemon 侧), client 侧看到 `peer closed connection without sending TLS close_notify`; 同族还有 `tls_init` 并发写坏材料 (两个 `tls-init` 交错落盘 → TLS 握手必失败), 后者已修为「整目录 rename 原子落地」 | **已收回 (2026-11-27, RAI-524)**: 读写共用一把 `tls_mtx` 且不跨 await 持锁, `handshake not complete` 在 1600 次连接里 0 次; 2 条用例在 ubuntu 与 macos 连续三轮通过后撤掉具名跳过, 机制由 `tls_link` target 的三条回归测试长期看着 |

当时处置是 `TARGET_TEST_SKIPS` 的**具名跳过** (target 照常执行并计入 ran 集合, 只按名字放过那 2 条)。
**2026-11-27 (RAI-524) 已收回**: 机制定位为「读任务与写任务可同时进入同一个 rustls 连接」并修掉,
2 条用例在 ubuntu 与 macos 连续三轮通过, `TARGET_TEST_SKIPS` 现在为空表。机制本身改由 `tls_link`
target 的三条回归测试 (close 完整性 / 空闲 CPU / 并发写不交错) 长期盯着, 所以下一轮即使再红,
也是这三条行为断言先红, 而不是靠人记得去翻 `zenoh_router_client` 的日志。

**windows 侧 2 个 target 显式豁免** (`PLATFORM_EXEMPTIONS`): `control_websocket` (整 target 挂死,
1500s 上限才终止) 与 `zenoh_router_client_windows` (daemon 因遗留 `service_name` 守卫拒启动) ——
两者都没隔离 HOME, 在 runner 的真实 `C:\Users\runneradmin` 上与前序用例共用 `.rdog` 与
`zenoh-guards`。`control_websocket` 在 ubuntu / macos 照常执行; `zenoh_router_client_windows`
是 windows-only, 所以**此刻在 CI 里只编译不执行**。两者在 job summary 里单列
**exempt on this platform** 一行、**不计入 ran 集合**; 收回条件是 issue #162 的隔离修复通过
windows 连续 3 轮, 随后删掉这两条豁免。所以本 PR 之后 windows 的 integration 面只有
`computer_use_density` 一个 —— 这是这一轮的真实覆盖, 不写成"windows 也跑了 e2e"。

逐条 `#[ignore]` 只在**单个用例**确有平台不兼容时用, 且必须挂理由与到期日 (见 §6)。
`tests/` 下现有 20 处 `#[ignore]`: `control_ax_e2e` 6、`control_display_aware_e2e` 8、
`zenoh_router_client` 2、`control_lanes` / `control_mouse_e2e` / `control_window_e2e` /
`control_websocket` 各 1。其中 CI 在当前平台会执行的 target 里有 4 处 (`control_lanes` 的截图用例、
`zenoh_router_client` 的真实输入模拟与截图用例、`control_websocket` 的截图用例), 其余都在
execute 面之外 (5 个 macOS GUI 面是 compile-only)。
**`#[cfg_attr(target_os = ..., ignore = ...)]` 这种平台级逐条收窄已归零** —— `control_lanes` 的
key 注入用例曾用它在 ubuntu 跳过, 根因是 enigo 连不上 display, 已由 ubuntu job 的 `xvfb-run`
前提解决 (见上)。`zenoh_router_client` 的 key 事件用例在 main 上曾按 `#[ignore]` 跳过 (commit
`9dbf80f` 加、`f0ebc7e` 随 target 级豁免一起撤), 现在同样不带 `#[ignore]`; ubuntu 上它随
`zenoh_router_client` 一起执行 (该 target 现在只用 `TARGET_TEST_SKIPS` 跳过 2 条 TLS 用例)。

**compile-only 是显式的, 不是静默 skip —— 但它也不计入「ran」。** 这 6 个 target 里, 5 个
需要真实桌面会话 (已登录的 GUI 用户 + Accessibility 与 Screen Recording 授权, GitHub runner
没有这些条件), 1 个是 `zenoh_unixpipe_fast_path` (issue #164, runner 上连接类用例接力红)。
它们的处置是:

| 位置 | 处置 |
| --- | --- |
| build 阶段 | `cargo test --no-run` 编译 (编译回归照样能抓到) |
| test 阶段 | job summary 里单列一行 **`compiled, not executed`**, 与 `passed/failed/ignored` 的已执行行分开 |
| ran 集合 | **不计入** —— `assert-tests-ran.py` 的口径与 §3 的零执行断言都不把它们当「跑过」 |

真机 lane 的跑法与前置 (不写下来的话, 「compile-only」三个月后会变成「没人知道怎么跑」):

```bash
# 真机 (macOS) 仓库根, 且已登录 GUI 会话
# 前置: 系统设置 → 隐私与安全性 → 辅助功能 / 屏幕录制, 给终端 (或 rdog 二进制) 授权
cargo test --test zenoh_unixpipe_fast_path   # issue #164: runner 上连接类用例逐个接力红, 真机可跑
cargo test --test control_ax_e2e
cargo test --test control_display_aware_e2e
cargo test --test control_mouse_e2e
cargo test --test control_ocr_e2e
cargo test --test control_window_e2e
```

**一个只能编译、没有任何可执行路径的测试等价于注释。** 上面这 6 条命令就是它们的可执行路径;
改动其中任何一个 target 的名字或前置时, 同票更新本表与 `run-integration-targets.py` 的
`CLASSIFICATION` (标成 `compile-only:macos`)。

### 3c. 堆叠 PR = 未验证 (RAI-433 顺带补的合并门缺口)

**基座不是 `main` 的 PR, 它的检查不能当合并依据。** 两个独立原因:

1. 必需检查是按 `main` 配的, 一次 `pull_request` 运行即使全绿, 证明的是「相对那个基座」;
2. 基座分支一旦前进, 已通过的层不会自动重跑 —— 于是那个「绿」可能是几轮之前的事实。

处置 (硬判定, 写在这里而不是靠人记):

- 要合的那一层, **先把 base 改成 `main` 并重跑一轮**, 用那一轮的读数做合并门;
- 层与层之间的绿只用来判断「这一层自己有没有把东西改坏」, 不用于合并;
- rebase 到最新 main 之后同样要重跑 (旧 head 的绿不作数)。

2026-11-26 实测过一次代价: 分支 rebase 前 `mergeable=dirty` 时新 head 的 `pull_request` 运行
**一条都没有**, 看起来像「没有新提交」, 实际是 GitHub 生成不出 merge commit 就不触发。

## 4. `ort-sys` / onnxruntime 的获取契约

`oar-ocr` → `ort` → `ort-sys` 在 build 时按目标三元组取一份 pyke 预编译 onnxruntime。
这条链上有两个已知坑, 都由 CI 侧兜住:

1. **dist 选择与校验** — `scripts/ci/prefetch-ort-dist.py` 从 `Cargo.lock` 锁定的那个
   `ort-sys` crate 内嵌的 `build/download/dist.tsv` 读目标→URL→sha256 (**不手抄摘要**),
   校验后用 pyke 自己的缓存布局落到 `<cache_root>/dfbin/<target>/<sha256>/`, 于是
   `ort-sys` build script 命中缓存、完全跳过网络。选行规则与 `resolve_dist()` 一致:
   本仓库没有开启任何 ort backend feature, 取该目标第一行 (Windows `+directml`、
   macOS `+coreml`, 那是 pyke 的基线包)。
2. **`copy-dylibs` 要求 `<OUT_DIR 祖先>/examples` 存在** — `ort-sys` 的
   `build/dynamic_link.rs` 把 dist 里的 DLL 复制到 `<OUT_DIR 的第 3 个祖先>` 及其
   `examples/`、`deps/`, 而 `cargo build --bin rdog` 从不创建 `examples/`。Windows 的
   dist 带 `DirectML.dll`, 于是 build script panic (`Os { code: 3, kind: NotFound }`,
   `dynamic_link.rs:42`); Linux 的 dist 只有静态 `libonnxruntime.a`, 匹配不到 `.so`,
   所以这条路在 Linux 上是**潜伏**的。CI 由 `cargo-prep` 的
   `Create the dylib copy targets ort-sys expects` 步骤铺好两个层级的目录。

**改 `Cargo.lock` 里的 `ort-sys` 版本 = 复核这条链**: dist 表内容、`copy-dylibs` 的目录假设
都可能变。复核方式 (本地可跑):

```bash
python3 scripts/ci/prefetch-ort-dist.py            # 打印命中的 dist 与其 sha256
cargo clean -p ort-sys && cargo build --bin rdog    # 走一遍 build script
```

`ort-sys` 想绕过本机制时它只有一条路: 缓存目录不存在 → 自己下载, 所以预取失败必须**红**,
不能 `continue-on-error`。

## 5. 修改本工作流时的义务

- 改 job 名 → 同票改 §1 的表 + 提醒管理员改仓库设置的必需检查;
- 新增/退役一条检查 → 同上, 并跑一遍 `ci-check` skill 的五项;
- 任何 `paths` 过滤都别加在这份 workflow 上: 静态必需表 + 路径门控 = 门结构性不可达
  (`ci-check` skill 第 3 检查的反例)。

## 6. 已知红的登记与到期日

**已知红不是豁免, 是一个带到期日的债。** 登记格式固定为四列, 缺一列就当没登记:

| 现象 | 归因 | 负责人 | 到期日 |
| --- | --- | --- | --- |
| `zenoh_control::tests::diagnostic_ping_should_bypass_busy_query_worker_and_preserve_queue` 间歇红 (28 轮 2 次, panic 文案 `worker 未进入执行`, 疑 TMPDIR 进程级改写) | 单跑 5/5 绿, 只在并行/全量时出现 (`src/zenoh_control.rs` 的 panic 行) | RAI-405 承接线程 | 2026-11-30 |
| ~~ubuntu runner 上 `zenoh_router_client` 的跨进程连接 e2e 不稳定~~ (2026-11-27 收口: RAI-524 修掉 TLS 首帧竞态并撤具名跳过) | **两族, 2026-11-26 (RAI-464) 按证据拆开**: ① `@key`/`@type` 类 = enigo 连不上 display (code 70) → **已修** (ubuntu 步骤改 `xvfb-run`); ② TLS/mTLS 两例 = `zenoh-link-tls` 1.8.0 首帧写入早于 rustls 握手完成 (`TX task failed: handshake not complete` + client 侧 `peer closed ... without TLS close_notify`), 串行跑同样复现; ②的同族 `tls_init` 并发写坏材料已修为整目录原子落地。**不是**「client 连不上刚启动的 daemon」, 也不是 runner 特有; ① 已撤销 `control_lanes` 那条 `#[ignore]`, ② 在 ubuntu 上按 `TARGET_TEST_SKIPS` 具名跳过那 2 条 (target 其余用例照跑); 入口=具名跳过表 + issue #163 | issue #163 / RAI-464 | 2026-12-03 |
| windows runner 上 `control_websocket` 整 target 挂死、`zenoh_router_client_windows` daemon 因遗留 `service_name` 守卫拒启动 | 两者都不隔离 HOME, 与同 job 前序用例共用 `.rdog` / `zenoh-guards`; 入口=`PLATFORM_EXEMPTIONS` 两条理由 + `tests/` 无 `HOME` 注入 | issue #162 承接 | 2026-12-03 |
| macos runner 上 `zenoh_unixpipe_fast_path` 的连接类用例逐个接力红, 最后一个卡在「运行中的 daemon 拒绝新 client 的专用 FIFO」(`Received a close message (reason GENERIC) in response to an OpenSyn`) | 前两条是用例假设 `~/.rdog/run` 已存在 (已修); 第三条开发机绿、runner 红, 是 runner 与真机在这条链路上的环境差; 处置=分类降为 `compile-only:macos` 并给真机 lane 命令; 入口=CLASSIFICATION + issue #164 | issue #164 承接 | 2026-12-03 |

规则:

- 到期日必须是具体日期 (`YYYY-MM-DD`), 不写"下轮"、"等 RAI-4xx"这类相对说法;
- **到期未清 = 阻塞项**, 与"本改动坏了东西"同等处理 —— 不允许再用"这是已知红"给新 PR
  放行, 也不允许把到期日续期当成处置;
- 清掉后从表里删除并在对应票面留回执 (哪次运行、哪个 head 变绿);
- 登记项必须有归因入口 (哪条注解 / 哪个测试名), 否则无法验证它到底还在不在。

**边界: 覆盖缺口不进这张表。** 已知红是"某次运行红了、还没修"; 覆盖缺口是"某个东西
从来没跑过" —— 它不会因为某次运行变绿而消失, 也没有可引用的失败回执。前者按上表记账,
后者只在 §3 披露 + 由承接票 (当前是 RAI-411) 放量, 放量后的判据是"新增目标的实际执行
计数 > 0", 而不是"某次运行变绿"。

当前本仓库的三类红 (2026-11-26 RAI-397 起点) 的去向: 工具链漂移与 flake 已在
`718eb8c4` 清掉; 「未运行」这一类由本文件的 §4 机制消掉; 若将来再现, 按上表重开一行。
