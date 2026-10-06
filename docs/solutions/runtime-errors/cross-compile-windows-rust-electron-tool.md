---
title: "Windows 交叉编译必须走 cargo xwin; 隐藏 child 活不过 ssh 会话"
date: 2026-10-06
last_updated: 2026-10-06
module: build/release
component: cross-compilation
problem_type: runtime_error
severity: high
status: active
tags:
  - windows
  - cross-compile
  - cargo-xwin
  - msvc
  - ring
  - scheduled-task
  - daemon
  - resident
verified_by:
  - "cargo xwin build --release --target x86_64-pc-windows-msvc 成功, 产物 target/x86_64-pc-windows-msvc/release/rdog.exe 43611136 bytes (2026-10-06 12:18, commit a7ecbec 验证段)"
  - "Windows 11 真机 vdcoo@rais-razer.local (4K@200%) 部署, SHA256 两端一致 (A2CDE103..., Size 43611136), 六项控制面验证全绿 (task_plan.md 2026-10-06 12:36 条目)"
  - "ssh 会话内 Start-Process 起的 daemon 两次 PID 秒死, 日志无崩溃记录"
  - "cargo-xwin 0.23.1 已装, x86_64-pc-windows-msvc 已在 rustup target list --installed 中 (2026-10-06 复核)"
  - "cargo xwin check --all-targets --target x86_64-pc-windows-msvc: a7ecbec 状态下 112 errors, 补 cfg 门后同轮降到 1 error (src/win_dpi.rs:70, windows-sys feature)"
root_cause: "cargo build --target 用宿主 cc 编译依赖的 C 代码 (ring), macOS 上没有 MSVC CRT/SDK 头; Windows 的创建标志 CREATE_NO_WINDOW|DETACHED_PROCESS 只解决控制台可见性, 不改变进程对 sshd 会话 job object 的归属"
resolution_type: "交叉编译统一改用 cargo xwin (自带 CRT/SDK 头 + clang-cl + lld-link); Windows 常驻改由 schtasks /SC ONLOGON 承载, 不靠交互式拉起"
related_skills:
  - rdog-control
---

# Windows 交叉编译必须走 cargo xwin; 隐藏 child 活不过 ssh 会话

## Problem

rdog 有一条 Windows 常驻面 (`rdog daemon` / `rdog hidden-daemon` +
`src/control_window/windows.rs` 的原生窗口 lane), 但 **"怎么在 macOS 上编出 Windows 版"
这条知识此前完全没有落盘**:

- `grep -rn "x86_64-pc-windows-msvc" --include=*.md` 在 `a7ecbec` 之前零命中
  (今天只出现在未提交的 `task_plan.md:67` 里)
- `grep -rn "xwin" --include=*.md` 零命中
- `.github/workflows/ci.yml:27` 的矩阵是 `os: [ubuntu-latest, macos-latest]`,
  **没有任何 Windows 覆盖**

后果很具体: `a7ecbec` 引入的 `src/control_window/windows.rs` 有 1308 行新代码,
**此前从未被编译过** —— 不是编译器报错, 是这条面根本没有被任何流水线或文档碰过。
一个只存在于少数人脑子里的配方, 空白本身就是缺陷。

本文记录两条同日踩实的运维坑: 交叉编译配方, 以及 Windows 上 daemon 常驻的载体选择。

## Symptoms

### 坑 1 — 用 `cargo build --target` 交叉编译, 在依赖的 C 阶段假失败

```bash
$ /Users/cuiluming/.cargo/bin/cargo build --release --target x86_64-pc-windows-msvc
...
fatal error: 'assert.h' file not found
```

> 上面这一行是本轮实测记录下来的**原始报错文本**。cargo 外层的
> `failed to run custom build command` 包裹信息在此不复述 —— 没有留档, 不臆造。

报错指向 `assert.h` —— 一个**在 macOS 上必然存在、在 MSVC 目标下必然不存在**的头文件。
这个矛盾本身就是判据: 报错的不是"缺依赖", 是"编译器用错了宿主"。

### 坑 2 — daemon 从 ssh 拉起, PID 秒死, 日志无任何崩溃记录

```powershell
PS> Start-Process -FilePath C:\Users\vdcoo\yino-test\rdog.exe `
      -ArgumentList "daemon","--config","rdog.toml" -WorkingDirectory C:\Users\vdcoo\yino-test
PS> Get-Process rdog
    Id  ProcessName
    <PID> rdog          # 起来了, PID 当场可见
# ssh 退出, 重新登录:
PS> Get-Process rdog
# (空)                  # 没了, 当天实测两次
```

`rdog-run.log` 里**没有崩溃记录、没有 panic、没有退出码** —— 因为它根本没机会跑
到会写日志的那一步。客户端侧的表现是 `@ping` 频繁 `Connection refused`,
**极易被误判成"程序 bug"**, 而真因是进程压根没活到能干活的时刻。

`a7ecbec` 之后 `src/main.rs:761-777` 的注释已经把这条钉在代码里了
("当天最后靠 schtasks /SC ONLOGON 才稳住"), 但当时只在代码注释里, 没有独立文档。

## What Didn't Work

- **`cargo build --release --target x86_64-pc-windows-msvc`**: 在 ring 的 C 阶段挂。
  换个 flag (`--no-default-features`、手动 `CC=`) 都是在宿主 cc 上打补丁,
  补不齐 MSVC CRT/SDK 头。
- **`Start-Process` + `-WindowStyle Hidden`**: 只藏控制台窗口, 不改进程归属。
  实测 PID 秒死。
- **`rdog hidden-daemon` / `rdog daemon` 在 Windows 上的自动隐藏化**
  (`src/main.rs:778` `spawn_windows_resident_daemon` → `src/hidden_mode.rs:102-107`
  的 `creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)`):
  这两个标志解决的是**控制台可见性**, 从交互式终端 (console / 资源管理器双击) 拉起时是对的,
  但**从 ssh 会话拉起时不够** —— 进程仍然挂在 sshd 给该会话建的 job object 里。
- **在 rdog 里加"脱离 job object"的代码路径** (Windows 上要用
  `CREATE_BREAKAWAY_FROM_JOB`, 且要求 job 本身允许 breakaway):
  等于把一个部署载体问题塞进产品代码。本次不需要 —— 计划任务这条载体已经足够,
  所以没做 (见 "为什么没有走 breakaway" 一节)。

## Verified Root Cause

### 坑 1 的根因 — 宿主 cc 找不到 MSVC 头

两层, 静态 + 动态各占一半:

**静态**: `ring v0.17.14` (`Cargo.lock` 内为传递依赖) 的 `build.rs` 会把 `.c/.S` 源文件
交给 `cc` crate 现场编译。`cc` crate 在 `x86_64-pc-windows-msvc` 下期望一个
MSVC 工具链 (或者像 `cargo-xwin` 那样注入 clang-cl + Windows SDK 头);
裸 `cargo build --target` 只设了 `TARGET`/`HOST`, **没有换 C 编译器、没有提供头文件根**,
于是 `cc` 落到宿主 (macOS) 的 clang 上, 按 macOS 的 include 路径去找
`assert.h` —— 而这是 MSVC CRT 头, macOS 上根本没有。

**动态**: `cargo xwin build --release --target x86_64-pc-windows-msvc` 跑通,
产物 `target/x86_64-pc-windows-msvc/release/rdog.exe` 43,611,136 bytes。
`cargo-xwin` 会自动拉取并缓存 Windows CRT/SDK 头, 把 `CC`/`AR` 指向 `clang-cl`,
把链接器指向 `lld-link`, 从而把上面那个"宿主 cc"换成"带 Windows SDK 的 clang-cl"。
**同一天同一份代码, 换掉 xwin 就红、去掉 xwin 就绿** —— 这条对照本身就是根因判据。

### 坑 2 的根因 — job object 归属, 与进程可见性是两件事

**现象 (已验证)**: ssh 会话内 `Start-Process` 起的 rdog, 两次实测都是
"起来 → 确认 PID 存在 → ssh 退出 → 进程消失", 日志零崩溃记录。

**机制解释 (未在目标机上做对照实验, 按 Windows 已知行为推断)**:
OpenSSH for Windows 的 sshd 会把每个会话的进程收进一个 job object,
会话结束时对该 job 施加 kill。`CREATE_NO_WINDOW` / `DETACHED_PROCESS` 是
*进程创建标志*, 只影响控制台分配, **不影响 job 归属**;
`DETACHED_PROCESS` 甚至不解除原有的 job 关联。
所以"隐藏 child 活不过 ssh 会话"不是 rdog 的 bug, 是**载体选错了**。

**为什么这条解释站得住 (而不只是听起来合理)**: 同一天换成
`schtasks /SC ONLOGON` 拉起后, 进程跨会话稳定存活、`5555` 端口保持 listen ——
载体一换, 现象消失, 与 rdog 自身代码路径无关。这构成一次可复现的对照。

**未验证的部分**: 没有在目标机上写一个"裸 Win32 程序 + `CREATE_BREAKAWAY_FROM_JOB`"
的对照实验, 因此"breakaway 也能解决"目前只是机制推断, 本文不把它列为解法。

## Solution

### 前置 (一次性)

```bash
# 1. 装 cargo-xwin (0.23.1 实测可用)
/Users/cuiluming/.cargo/bin/cargo install cargo-xwin

# 2. 装 Windows 目标标准库
/Users/cuiluming/.cargo/bin/rustup target add x86_64-pc-windows-msvc
```

> 本仓库的 cargo 不在 PATH, 一律用绝对路径 `/Users/cuiluming/.cargo/bin/cargo`。

### 坑 1 解法 — 交叉编译

```bash
# ✅ 唯一正确的 Windows 交叉编译姿势
/Users/cuiluming/.cargo/bin/cargo xwin build --release --target x86_64-pc-windows-msvc

# ❌ 假失败: 宿主 cc 找 MSVC 头, 挂在 ring 的 C 编译阶段
/Users/cuiluming/.cargo/bin/cargo build --release --target x86_64-pc-windows-msvc
```

产物: `target/x86_64-pc-windows-msvc/release/rdog.exe`

`cargo xwin` 不止 `build`, `check` / `clippy` / `test` 子命令同样带这套环境
(`cargo xwin --help`), 需要做 Windows 目标的静态检查时用 `cargo xwin check --all-targets`
而不是 `cargo check --target ...`。

### 坑 2 解法 — 用计划任务承载常驻

```powershell
schtasks /Create /TN RdogDaemon /TR "cmd /c cd /d C:\Users\vdcoo\yino-test && rdog.exe daemon --config rdog.toml >> C:\Users\vdcoo\yino-test\rdog-run.log 2>&1" /SC ONLOGON /RL LIMITED /F
```

要点:

1. **`cmd /c cd /d <dir> && ...`** —— `cd /d` 是为了跨盘符切目录;
   整条命令必须包在一层引号里 (`/TR "..."`), 否则 `&&` 会被 schtasks 拆坏。
2. **`>> ... 2>&1` 重定向到文件** —— 计划任务没有可交互的控制台,
   不重定向就等于把 stderr 扔进虚空, 出事无从查。
   `a7ecbec` 同时加了 `RDOG_LOG_FILE` 让 daemon 日志落文件, 是同一条诉求的代码侧出口。
3. **`/RL LIMITED`** —— 任务以账号自身的权限运行, 不提权。rdog 常驻面不需要提权;
   换成 `/RL HIGHEST` 需要创建者本人有管理员权限, 别默认它等价。
4. **`/SC ONLOGON`** —— 用户登录时触发, 不依赖任何交互式会话先于它存在。
5. 启动方式: 建完任务后 `schtasks /Run /TN RdogDaemon` 立即生效, 不用等下次登录。

**"起来了没"的判据** (别只看进程表, 也别只看日志):

```powershell
schtasks /Query /TN RdogDaemon /V /FO LIST   # Last Result 是 0x0
netstat -ano | findstr :5555                   # 0.0.0.0:5555 LISTENING
```

### 为什么没有走 breakaway

`CREATE_BREAKAWAY_FROM_JOB` 确实是对付 job object 的标准手段, 但它有两个前提:
目标 job 必须允许 breakaway (`JOB_OBJECT_LIMIT_BREAKAWAY_OK`), 而 sshd 建的那个 job
**不保证给这个权限**; 给了权限也要在 spawn 处传标志 + 失败时优雅降级。
为一个部署载体问题往产品二进制里加一条平台分支, 收益不抵复杂度。
计划任务是 Windows 官方推荐的常驻载体, 权限模型、日志、开机自启全都现成。
**若将来确实需要在 job 内 breakaway (例如从 CI runner 起常驻), 那是独立一票。**

## Why This Works

交叉编译的失败点不在 Rust 代码, 而在**工具链**: `cargo build --target` 只切 Rust 的
`TARGET`, 不切 C 工具链; `ring` 的 `build.rs` 把缺口暴露成"缺 `assert.h`"。
`cargo-xwin` 补的正是这一层 (Windows SDK 头 + clang-cl + lld-link), 所以同一个
`--target` 从红变绿。

常驻失败同理不在 rdog, 而在**进程归属**: 可见性由创建标志决定, 存活由 job 归属决定。
`schtasks` 建的进程属于任务计划程序自己的 job, 不随任何交互式会话消亡。
两条坑的共同形状是: **症状出现在产品层, 根因在产品层之外一格**。

## Verification

### 编出来的东西"能用"的判据: 拷到真机 + SHA256 两端一致

跨平台产物最容易被"存在即正确"骗过 —— 文件在, 但可能是上次构建的。
今天的判据链 (记录在 `task_plan.md` 2026-10-06 12:36 条目):

```bash
# 1. 记下本地产物指纹
ls -la target/x86_64-pc-windows-msvc/release/rdog.exe
shasum -a 256 target/x86_64-pc-windows-msvc/release/rdog.exe
#  → 43611136 bytes, SHA256 A2CDE103...

# 2. scp 上去后, 在目标机上算同一个指纹
ssh -i ~/.ssh/id_rsa vdcoo@rais-razer.local \
  "powershell -NoProfile -Command \"Get-FileHash -Algorithm SHA256 C:\Users\vdcoo\yino-test\rdog.exe\""
#  → 与本机一致

# 3. 确认目标机上没有更旧的 .rs / 二进制在跑
#    (本次: 0 个 .rs 更新于该二进制)
```

**SHA256 一致才说明"目标机上跑的就是我编的那个", 后续验证才有意义。**

### 本文档的事实核对 (2026-10-06 复核)

| 事实 | 核对方式 | 结果 |
| --- | --- | --- |
| `cargo-xwin` 已装 | `cargo xwin --version` | `cargo-xwin-xwin 0.23.1` |
| Windows 目标标准库已装 | `rustup target list --installed` | 含 `x86_64-pc-windows-msvc` |
| CI 无 Windows 覆盖 | 读 `.github/workflows/ci.yml:27` | `os: [ubuntu-latest, macos-latest]` |
| 隐藏 spawn 标志 | 读 `src/hidden_mode.rs:85-86,102-107` | `CREATE_NO_WINDOW \| DETACHED_PROCESS` |
| 自动隐藏化的边界与注释 | 读 `src/main.rs:761-800` | 仅 TCP transport, `RDOG_DAEMON_FOREGROUND` 逃生口 |
| 产物尺寸与时间 | `ls -la target/x86_64-pc-windows-msvc/release/rdog.exe` | 43611136 bytes, Oct 6 12:18 |

### 边界 — "能构建" 与 "目标下测试能跑" 是两件事

**`cargo xwin` 只解决编译, 不解决测试。** `cargo xwin` 提供了 Windows 工具链环境,
但仓库里的约 1050 个内嵌单测里有大量 Unix-only 模块, 它们在 Windows 目标下会编译失败。

**实测 (2026-10-06, `a7ecbec` 状态)**:

```bash
$ /Users/cuiluming/.cargo/bin/cargo xwin check --all-targets --target x86_64-pc-windows-msvc
error: could not compile `rustdog` (bin "rdog" test) due to 112 previous errors; 8 warnings emitted
```

**112 个存量编译错误**, 集中在 Unix-only 模块。按文件分布:

| 错误数 | 文件 | 说明 |
| --- | --- | --- |
| 62 | `src/zenoh_runtime/local_default/tests.rs` | unix 专属的 local-default 测试 |
| 47 | `src/zenoh_runtime/unixpipe/tests.rs` | unixpipe 测试引用 unix-only 函数 |
| 13 / 11 / 9 | `src/control_ax.rs` / `src/pty_control.rs` / `src/control_window.rs` | 平台分支在 Windows 下走空实现 |
| 2 / 2 / 3 | `src/zenoh_runtime/session.rs` / `src/control_actions/tests.rs` / `src/control_actions.rs` | 同上 |
| 各 1 | `unixpipe.rs` / `target_resolve.rs` / `win_dpi.rs` / `screenshot.rs` / `listener/mod.rs` / `control_overlay.rs` / `error_envelope.rs` / `control_cdp.rs` / `control_ax/types.rs` | 零散 |

**成因 (静态核实)**: `src/zenoh_runtime.rs:12` 的 `mod unixpipe;` **没有 `#[cfg(unix)]` 门** ——
第 7-8 行的 `#[cfg(unix)]` 门的是 `pub(crate) mod process_lease;`, 不是它。
于是 `unixpipe.rs` 连同它的 `tests.rs` 在 Windows 目标下照常参与编译,
而里面引用的 `unixpipe_socket_path` / `cleanup_stale_unixpipe_socket` /
`try_unixpipe_probe` / `prepare_unixpipe_listener` 全部是 unix-only 实现,
于是满屏 `E0425: cannot find function ... in this scope`。
**修法就是补 `#[cfg(unix)]` 门**, 不是改实现。

**这个数字是移动靶**: 写下本文当天, 另一组正在补这些门 ——
同一条命令在几分钟后的第二次运行只剩 **1 个错误**:

```
error[E0432]: unresolved import `windows_sys::Win32::UI::HiDpi::GetProcessDpiAwarenessContext`
  --> src/win_dpi.rs:70:9
```

残留那 1 个位于 `#[cfg(all(test, windows))]` 的**测试模块**里
(`src/win_dpi.rs:68-71`), 是 `windows-sys` crate 的 feature 没开全,
与 Unix-only 无关, 属于另一类问题。
**读本文时不要把 112 当常量, 以当轮实测为准。**

因此下面两条必须分开陈述, 不要混为一谈:

1. **"Windows 版能构建"** —— `cargo xwin build --release --target x86_64-pc-windows-msvc` 通过。
   这是 `a7ecbec` 达成的状态, 也是本文的适用范围。注意 `build` 只编 bin,
   `--all-targets` 额外编测试 —— 上面那 112 个错误**不影响这一条成立**。
2. **"Windows 目标下测试能跑"** —— 另一件事, 2026-10-06 当天**尚未达成**。
   在它达成之前, Windows 侧的回归保障只有两档: 宿主 (macOS/Linux) 全量单测 +
   Windows 真机人工/脚本验证。

CI 矩阵里没有 Windows 这一格, 意味着**未来新增的 Windows 代码仍无自动兜底**。
本文只解决"配方没落盘", 不解决"CI 没覆盖" —— 后者需要单独决定
(自托管 Windows runner 是常规解, 但会显著增加 CI 时长与维护面)。

## Prevention

- 任何 `cargo build --target <windows-target>` 出现在本仓库, 一律先问:
  **这是要跑 `cargo xwin` 的场景吗?** 本仓有 `ring` 这类带 C 代码的传递依赖,
  裸 `cargo build --target` 一定会在那里红。
- 新增传递依赖时留意 `Cargo.lock` 里是否引入带 `build.rs` + C 源的包 (`ring`、
  `zstd-sys`、`libsqlite3-sys` 等), 它们是这条坑的触发面。
- Windows 常驻形态部署时, **默认假设"交互式拉起是临时的"**。
  需要跨会话存活就用 `schtasks` /SC ONLOGON; 只想临时看一眼用
  `RDOG_DAEMON_FOREGROUND=1` (`src/main.rs:784-793`)。
- 计划任务部署后按 "Why This Works" 后半段的判据核对 (Last Result + 端口 listen),
  **不要只看进程表** —— 进程在 ≠ 能服务。
- 计划任务的 `/TR` 必须自带日志重定向。没有日志的常驻服务等于没有服务。

## Related

- `src/hidden_mode.rs:76-110` —— 隐藏 child spawn, `CREATE_NO_WINDOW | DETACHED_PROCESS`
- `src/main.rs:761-800` —— `rdog daemon` 在 Windows 的自动隐藏化与 `RDOG_DAEMON_FOREGROUND`
- `src/winshell.rs:9,64` —— shell lane 的 `CREATE_NO_WINDOW`
- `src/zenoh_runtime.rs:7-8,12` —— `process_lease` 已带 `#[cfg(unix)]` 门, 而 `mod unixpipe;` 没有;
  这一处是"测试目标边界"一节里 112 个错误的主因
- `src/control_window/windows.rs` —— 本次首次被编译的 1308 行 Windows 原生窗口面
- `.github/workflows/ci.yml:26-27` —— 当前测试矩阵 (无 Windows)
- [`../../best-practices/remote-control-blocked-command-pitfalls.md`](../best-practices/remote-control-blocked-command-pitfalls.md)
  —— 本次 Windows 真机 8 小时作业的问题清单; 本文的坑 2 是其中 P2 的运维侧续篇