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

| 平台 | 执行的 target |
| --- | --- |
| ubuntu | `connect_modes`、`control_lanes`、`control_mode`、`control_pty`、`control_tty`、`shell_pty`、`daemon_inbound`、`listen_suspend`、`zenoh_router_client`、`computer_use_density`、`control_websocket` |
| macos | 同上 + `zenoh_unixpipe_fast_path` |
| windows | `computer_use_density` (另有 2 个 target 在 windows 显式豁免, 见下) |
| 全平台 (build 阶段) | `cargo test --no-run` 编译**全部** target, 含下面的 compile-only 面 |
| compile-only (macos) | `control_ax_e2e`、`control_display_aware_e2e`、`control_mouse_e2e`、`control_ocr_e2e`、`control_window_e2e` |

`recording_e2e` 仍由它自己那一步执行 (unix)。

**单个 target 有 1500s 墙钟上限** (`TARGET_TIMEOUT_SECONDS`), 超时判该 target 红并把它
已有的输出留进失败详情, summary 里同时给出每个 target 的 `elapsed`。原因是反过来的:
挂住的 target 会一路撑到 GitHub 的 job timeout 被 cancel, 日志只剩一句
`The job has been cancelled` —— 既指不出是哪个 target, 也分不清「慢」与「死等」。

**windows 侧 2 个 target 显式豁免** (`PLATFORM_EXEMPTIONS`): `control_websocket` (整 target 挂死,
1500s 上限才终止) 与 `zenoh_router_client_windows` (daemon 因遗留 `service_name` 守卫拒启动) ——
两者都没隔离 HOME, 在 runner 的真实 `C:\Users\runneradmin` 上与前序用例共用 `.rdog` 与
`zenoh-guards`。它们照常在没有平台豁免的那一侧执行 (前者 ubuntu/macos), job summary 里单列
**exempt on this platform** 一行、**不计入 ran 集合**; 收回条件是 issue #162 的隔离修复通过
windows 连续 3 轮, 随后删掉这两条豁免。所以本 PR 之后 windows 的 integration 面只有
`computer_use_density` 一个 —— 这是这一轮的真实覆盖, 不写成"windows 也跑了 e2e"。

**ubuntu 侧 3 条已知红** (`control_lanes` 1 条 + `zenoh_router_client` 2 条) 用
`#[cfg_attr(target_os = "linux", ignore = …)]` 显式收窄: 它们仍然在 macos / windows
上执行, 只在 ubuntu runner 上跳过, 登记与到期日见 §6 (`RAI-464`)。收窄的是**平台**, 不是
断言 —— 同一 target 的其余用例照常跑, `passed` 计数仍然要求 > 0。

**compile-only 是显式的, 不是静默 skip —— 但它也不计入「ran」。** 这 5 个 target 需要
真实桌面会话: 已登录的 GUI 用户 + Accessibility 与 Screen Recording 授权, GitHub runner
没有这些条件。它们的处置是:

| 位置 | 处置 |
| --- | --- |
| build 阶段 | `cargo test --no-run` 编译 (编译回归照样能抓到) |
| test 阶段 | job summary 里单列一行 **`compiled, not executed`**, 与 `passed/failed/ignored` 的已执行行分开 |
| ran 集合 | **不计入** —— `assert-tests-ran.py` 的口径与 §3 的零执行断言都不把它们当「跑过」 |

真机 lane 的跑法与前置 (不写下来的话, 「compile-only」三个月后会变成「没人知道怎么跑」):

```bash
# 真机 (macOS) 仓库根, 且已登录 GUI 会话
# 前置: 系统设置 → 隐私与安全性 → 辅助功能 / 屏幕录制, 给终端 (或 rdog 二进制) 授权
cargo test --test control_ax_e2e
cargo test --test control_display_aware_e2e
cargo test --test control_mouse_e2e
cargo test --test control_ocr_e2e
cargo test --test control_window_e2e
```

**一个只能编译、没有任何可执行路径的测试等价于注释。** 上面这 5 条命令就是它们的可执行路径;
改动其中任何一个 target 的名字或前置时, 同票更新本表与 `run-integration-targets.py` 的
`COMPILE_ONLY`。

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
| ubuntu runner 上 3 条 control/TLS e2e 连不上 daemon: `control_lanes::daemon_control_lane_should_accept_key_object_payload_with_request_id`、`zenoh_router_client::control_should_publish_key_event_after_successful_key_request` (两者 code 70 `no successful connection`)、`zenoh_router_client::tls_enabled_daemon_should_serve_ca_client_and_reject_wrong_ca` (换正确 CA 后仍连不上, 空输出) | 本地单跑与 `taskset -c 0,1` 压到 2 核均绿, 只在 ubuntu GHA runner 红; 处置=按平台收窄成 `#[cfg_attr(target_os = "linux", ignore = …)]`, macos/windows 照常执行; 入口=三处 `#[cfg_attr]` 注记 | RAI-464 承接线程 | 2026-12-03 |
| windows runner 上 `control_websocket` 整 target 挂死、`zenoh_router_client_windows` daemon 因遗留 `service_name` 守卫拒启动 | 两者都不隔离 HOME, 与同 job 前序用例共用 `.rdog` / `zenoh-guards`; 入口=`PLATFORM_EXEMPTIONS` 两条理由 + `tests/` 无 `HOME` 注入 | issue #162 承接 | 2026-12-03 |

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
