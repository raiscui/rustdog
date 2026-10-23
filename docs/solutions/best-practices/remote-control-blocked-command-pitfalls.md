---
title: 远程被控端实战反馈 — 阻塞死锁、坐标空间与 Windows 功能缺口
date: 2026-10-06
last_updated: 2026-10-23
module: control plane
component: cmd/spawn/screenshot/window-lane
problem_type: best_practice
severity: high
status: active
tags:
  - control-plane
  - blocking
  - timeout
  - coordinate-space
  - windows
  - remote-ops
  - agent-ux
verified_by:
  - "Windows 真机远程控制实战 2026-10-05 (vdcoo@rais-razer.local, Win11 x64): 约 200 次 control 调用, 8 小时连续作业"
  - "2026-10-06 commit a7ecbec 逐项落地 P0-1 / P1-1 / P1-3 / P2, 见下方「落地情况」小节"
  - "2026-10-06 Windows 11 真机 (4K@200%) 复验六项全绿: 启动横幅 / @ping 结构化 / @cmd 超时后控制面存活 / @window-find 列出 320 窗口 / 截图坐标换算命中开始按钮 (点击后 37.22% 像素变化, 关闭后 0.00%) / @cmd 引号容错"
  - "2026-10-22 复核: @run 已存在; UTF-8 初始化当时仅覆盖 execute_script_with_timeout (2026-10-23 调用链校正); 坐标换算权威定义见 src/control_mouse/screenshot_frame.rs"
  - "2026-10-11 跨机 Electron 调试增量 (带空格路径拉起 / 主进程取证 / 双通道定位), 已并入「2026-10-11 增量」小节"
resolution_type: "P0-1 / P1-1 / P1-3 / P2 已由 commit a7ecbec 落地并真机复验; P4 已由 commit f7b94bd 落地; P0-2 / P3-1 / P3-2 与 Windows task cancel/hidden-shell control matrix 已通过 Native 与 Windows 11 指定回归; P1-2 仍未落地; P2 resize 仍显式不支持"
related_solutions:
  - docs/solutions/best-practices/electron-main-process-forensics.md
related_skills:
  - rdog-control
---

# 远程被控端实战反馈 — 阻塞死锁、坐标空间与 Windows 功能缺口

## 背景

2026-10-05 用 `rdog control` 远程操作一台 Windows 11 真机 (i7-9750H / 4K 200% 缩放 /
简体中文系统)，连续作业约 8 小时、约 200 次 control 调用，目标是验证一条 Electron
应用的自动更新链。

这是 rdog 第一次被当作**长时远程运维通道**使用（此前用例以单机 GUI 自动化为主），
因此暴露的问题集中在"长时间、跨会话、无本地反馈"这一类场景 —— 恰恰是单机自动化
测试覆盖不到的部分。

本文只记录**实测暴露的问题与改进建议**，不含未验证的推测。每条都标注了
现象、根因证据和建议动作。

---

## 落地情况 (2026-10-06 · commit a7ecbec)

本文提出的改进项已由 `a7ecbec` 逐条施工，并在 Windows 11 真机
(4K@200%) 上复验; 后续 `536afd0` / `f7b94bd` 分别补充同步脚本编码与 P4.
**未落地的条目全部保留在下方原章节里作为待办**，
下表是唯一的进度真相源 —— 读某一节的问题描述时，回来看这张表看它现在什么状态。
(每行复核日期见最后一列; P3-2 于 2026-10-23 校正覆盖范围.)

| 条目 | 状态 | 依据 (源码 / 验证) | 最后复核 |
| --- | --- | --- | --- |
| **P0-1** `@cmd` 超时 + 杀进程树 | **已落地** | `src/control_actions.rs::{shell_lane_timeout,execute_script_with_timeout,reap_after_terminate,terminate_process_tree}` 是超时与终止路径的真相源; Windows 检查 `taskkill /F /T /PID`, Unix 向隔离进程组发信号. 历史真机复验: 15s 超时后 `@ping` 仍活 | 2026-10-23 |
| **P0-2** `@ping` 入站忙闲与排队诊断 | **Native/Windows 指定回归通过** | Native 首轮 39/40 暴露 receiver close 后 queued 未清; `ChannelReceiver` 修复后最新相关 nextest 40/40 (7.739s), 原断言保留. Windows 11 真实 SDK busy 时 legacy Ping、observer session-open/bridgePing/close 与 queue close 清理通过. `control` 范围、same-session FIFO 及网络/PTY边界见 `specs/control-line-protocol.md`; 不代表全局队列或请求终态 | 2026-10-23 |
| **P1-1** 截图↔click 坐标换算前置声明 | **已落地** | `src/screenshot.rs:437` manifest 补 `click_coordinate_space`，`src/control_mouse/target.rs:417` 补 `image_to_click_scale`；`transforms` 换算公式改用 `os_logical_bounds`。真机复验: 4K 图上 (1210,2112) → os-logical (605,1056) 点中开始菜单 | 2026-10-22 |
| **P1-1** `@click` 支持物理像素入口 | **已落地** | `src/control_mouse/screenshot_frame.rs` (418 行): 截图坐标 → os-logical 的进程内缓存 (TTL 10min / 容量 32)，`@click` 接受 `{screenshot, x, y}` 引用式端点 (`screenshot_frame.rs:127-128` 为换算式) | 2026-10-22 |
| **P1-1** 文档/skill 里的换算口诀 | **已核实, 且发现写错 (2026-10-22 已修, 5 处)** | 同一份单段式公式 `os_x = image_x + virtual_bounds.x` 出现在 **5 处载体**: skill `.codex/skills/rdog-control/references/protocol.md` (2 处)、`README.md`、`specs/code-agent-rdog-control-usage.md`、`specs/rdog-multi-display-screenshot-coordinate-plan.md`、`specs/rdog-mouse-control-coordinate-plan.md` —— **在 HiDPI / 窗口裁剪图下不成立**: Windows 4K@200% 时 `virtual_bounds` 是物理像素 (与 os-logical 差 2 倍), 裁剪图还少一项 `image_rect` 平移。权威公式在 `src/control_mouse/screenshot_frame.rs:17`, 5 处已全部改为"以 manifest 的 `transforms` 字段为准 + 权威公式" | 2026-10-22 |
| **P1-2** `@spawn` 短等待语义 (`wait_ms`) | **未落地** | 全仓无 `@spawn` 的 `wait_ms` 字段 (`wait_ms` 只出现在 `open_app`)。**建议 1 原文的方案未被采纳也未被否决**, 需重新拍板。`@run` 的 `wait` / `timeout_ms` 是另一条原语, 不覆盖本条 | 2026-10-22 |
| **P1-3** 响应回显实际 shell | **部分落地 (范围有意收窄)** | 失败 / 超时 / 取消三类响应带 `shell` (`control_actions.rs:935/951/972`)，`@ping` 也带 (`control_actions.rs:993`)；**成功响应刻意不带** —— `control_actions.rs:782-786` 写明理由: 成功形状是既有契约，大量消费方按字符串读。原文"响应首行回显"是按全部响应提的，落地方案比建议窄 | 2026-10-22 |
| **P1-3** 裸路径缺引号的解释器提示 | **部分落地** | `src/control_protocol/parsers.rs:766` 对"含空格的裸路径"给 PowerShell `& "path" args` 提示。但提示文案里**没有点明当前解释器是 cmd 还是 PowerShell** —— 原文建议 2 的后半句未做 | 2026-10-22 |
| **P1-3** 自动给裸路径加引号 | **明确未做 (有意否决)** | `parsers.rs:887` 注释: 单 token 裸路径在 PowerShell / cmd 下本来就能跑，拦下就是假阳性。commit 记录为"只提示不代改写" | 2026-10-22 |
| **附带坑** 引号 payload 被判多余内容 | **已落地** | `parsers.rs` 增加闭合引号后的容错尾巴，并放宽引号内 shell 文本的转义 (修 `C:\Program Files` 被当转义序列) | 2026-10-22 |
| **附带坑** 多行 PowerShell 走 SSH 被引号吃掉 | **未落地 (agent 侧自律, 非产品问题)** | 仍是"复杂 PowerShell 一律 scp 传 .ps1 再执行" | 2026-10-22 |
| **P2** Windows window-find / activate / close | **已落地** | `src/control_window/windows.rs` (1308 行，本次首次被编译)；`src/control_window.rs:735/836/842` 三个 `execute_default_*` 入口。真机复验: `@window-find` 列出 320 个窗口 | 2026-10-22 |
| **P2** Windows window-resize | **有意推迟 (2026-10-22 复核仍如此)** | `src/control_window/windows.rs:88` 仍显式返回 `windows_resize_not_implemented`; `src/control_window.rs:266-269` 注明: 窗口身份链 (EnumWindows) 已就位，但 resize 要确定外框与 per-monitor DPI 的尺寸语义，尚未验证。刻意不做成"平台整体 unsupported"，以免调用方分不出"能列窗口"与"能改尺寸" | 2026-10-22 |
| **P3-1** daemon 诊断 | **Native/Windows 指定回归通过** | `@ping` 保留 `daemon::bind_state()` / `daemon::uptime()` 并附带 P0-2 scoped ingress snapshot; Windows busy-query 用例验证独立 legacy Ping 与 observer bridge 可用. 没有 reset session API; 字段边界见 `specs/control-line-protocol.md` | 2026-10-23 |
| **P3-2** Windows 中文输出乱码 | **Windows 11 对照通过** | `src/control_actions.rs::build_shell_command` 统一 PowerShell/cmd 初始化, 覆盖同步、后台 task、flow、裸 shell; Windows 用例验证 progress 被抑制, 中文 UTF-8 与真实 `Write-Error` 保留, stderr 分类正确. `@run` 直接执行, 不改写 args. 测试使用 `--no-default-features`, 不覆盖 OCR | 2026-10-23 |
| **附带坑** `@task-cancel` 后台任务进程树收割 | **Native/Windows 隔离回归通过** | `src/task_control.rs::cancel_task` 在 registry 锁外终止并收割; Unix `@spawn` 用 `src/control_actions.rs::isolate_process_group`, Windows 检查 `taskkill /F /T /PID`. Windows 11 用例确认目标 listener 退出、无关 listener 仍活、取消失败可重试且不持全局 registry 锁. 已脱离父 shell 的子孙仍不保证覆盖. | 2026-10-23 |
| **附带坑** Windows 非交互 shell 闪现终端 | **Windows 11 rdog 路径通过** | `src/control_actions.rs::build_shell_command` 设置 `CREATE_NO_WINDOW`; 新 test-exe 的 builder/task/flow 三路实测 `GetConsoleWindow=0`. `@run` window 选项和 PTY 不受影响, 命令显式启动的 GUI app 仍可见. 该结果不覆盖 OCR | 2026-10-23 |
| **P4** `@run` / `@run-and-wait` 组合原语 | **已落地 (2026-10-08, commit f7b94bd)** | `src/control_protocol.rs:620` 路由 `"run"`; `src/control_actions.rs:2293-2332`: `{path, args, timeout_ms, window, wait}` 结构化逐参 spawn 裸 exe (**不经 shell, 无引号层**), 同步等退出+退出码, 超时杀树, `wait=false` 启动即回 pid, `window=visible` 供 GUI 安装器; 回执 stdout/stderr 只带尾部 2KB | 2026-10-22 |
| **运维侧新增** Windows 交叉编译配方 | **已补文档** | 见 `docs/solutions/runtime-errors/cross-compile-windows-rust-electron-tool.md`。此前 CI 无 Windows 覆盖、文档零记录 | 2026-10-22 |

**真机复验口径** (`task_plan.md` 2026-10-06 12:36 条目): 六项全部通过，
无失败项; 远端跑的是当日 12:18 交叉编译的产物，SHA256 两端一致
(`A2CDE103...`, Size 43611136)。

**残留风险一句话**: Native 40/40 与 Windows 11 指定 control-plane 矩阵通过; Windows 构建
排除 OCR, feature-off 额外用例通过但不验证 OCR 识别. 快照不观测网络/内核缓冲、PTY 原始
stdin 数据帧或所有任务终态; detached 子孙也不保证能被取消收割. Windows debug-map 后处理
失败、33 条既有告警和 `ENGINE_READY` 独立等待问题仍未解决;共享 daemon 保护字段前后对账及
隔离测试清理已通过,详细边界见 `specs/yino-rdog-usage-improvement-plan.md`.

---

## 2026-10-11 增量 — 同一场跨机实战 (Electron 应用调试) 的新沉淀

2026-10-11 继续用 rdog 远程调一台 Windows 机上的 Electron 应用（Yino 一侧的
TB13 跨机测试）。本轮补了三条上一节没覆盖的经验，其中一条修正了本文此前的
结论口径。

### A. 带空格路径远程拉起：首选 `@run`，其次无空格链接

**现象**: 在被控机上拉起一个装在带空格路径下的可执行文件（如
`C:\Program Files\App\app.exe`）失败 —— shell 把路径按空格断成多个 token。

**机制未定位（证据缺口）**: 现场记录的现象是"`Start-Process '带 空格'` 的单引号
到 daemon 侧消失"，但 **rdog 的 Rust 源码里找不到剥离单引号的代码路径**：
协议层只认双引号（`src/control_protocol/parsers.rs` 的 `parse_quoted_payload_with`
只处理 `"`），`@cmd` 的 shell 文本走 `QuotedPayloadPolicy::ShellText` 原样透传，
one-shot 行直接来自 argv（`src/main.rs:317` 的 `extract_one_shot_lines`），
全仓 `trim_matches` 只有三处且都作用于双引号。
所以引号消失的层级**尚未定位**（候选：调用方 shell → client argv → 控制线 → shell），
本文不把它写成 rdog 的产品缺陷。

**稳妥解法（不依赖定位）**:

1. **首选 `@run` 结构化 args**: `Command::new(path).args(args)` 直接 spawn 裸 exe,
   **整条链上没有 shell, 也就没有引号层**（`src/control_actions.rs:2308-2312`）。
   带空格路径天然成立。
2. **必须在 shell 里跑时, 建无空格链接**: `mklink C:\link.exe "C:\real path.exe"`
   —— 用 ssh 跑 (引号自己全控), rdog 只跑无空格路径。
   注意目标卷可能禁用 8.3 短名（`dir /x` 无短名列），通配符也不是解法。
3. 不要指望在 rdog 控制行里用单引号护住带空格路径 —— 该层行为未定位。

### B. 主进程取证：渲染层 CDP 看不见主进程

`--inspect=9230` 开主进程 inspector；取 target 时**不要按 `type` 过滤**
（node target 的 type 不是 `page`，过滤会取到空集合）；`ipcMain` 的
`on()` 与 `handle()` 存在两个不同字段，查挂载清单要两处都看。
完整方法学见 `docs/solutions/best-practices/electron-main-process-forensics.md`。

### C. 双通道一次分清"发送链断"与"handler 错"

renderer CDP 发 + main inspector 手动调消费 handler 读态：inspector 侧行为正确
则失效点在发送链，反之在 handler 逻辑。单侧取证永远在对猜
（现场：busy 疑云三轮黑盒猜测未果，双通道一击定谳）。方法学同上篇。

### D. 附带修掉的一个文档缺陷（2026-10-22）

本表 P1-1 行原记"文档/skill 里的换算口诀 —— 未核实"。本次核实发现 skill
`.codex/skills/rdog-control/references/protocol.md` 给的公式**在 HiDPI / 窗口裁剪图下
是错的**（见上表该行），已按 `src/control_mouse/screenshot_frame.rs:17` 的权威公式
修正。这类"公式类断言"与代码真相源之间没有校验，是本轮发现的载体漂移类型。

---

## P0 — 同步 shell lane 长时间占用控制面

> **状态复核 (2026-10-23)**: 超时 + 杀进程树已落地; `@ping` busy/ingress 计数通过 Native
> 40/40 与 Windows 11 指定回归. 这些计数只描述 Zenoh ingress 范围,不等于全局队列或死锁判定.
> 详见上方「落地情况」小节。

### 现象

执行 `@cmd:Start-Process ... -Wait` 之后，**后续所有 control 命令都没有执行**。
用户侧观察是"命令好像没生效，只有我打断它之后才会继续"。

### 历史根因（2026-10-06 修复前）

当时的 `execute_script` 用 `Command::output()` 无期限等待子进程退出. 以下为修复前的
代码形态,不是当前源码:

```rust
pub(crate) fn execute_script(shell: &str, script_text: &str) -> io::Result<ActionExecutionResult> {
    let output = build_shell_command(shell, script_text).output()?;   // ← 裸同步等待, 无超时
    Ok(from_process_output(output))
}
```

`-Wait` 类命令不返回时,控制面 worker 会被占用,同一执行 lane 的后续请求排队,
客户端表现为"命令没执行". 当前同步 shell lane 已有超时和进程树终止;仍可能占用
lane 至超时,应使用 `@spawn` 运行长任务.

当时这个**症状与病因严重错配**: 不是解析失败或权限错误,而是同一 lane 排队.
当前 `@ping` 提供 scoped ingress busy/queue 快照,但不判定死锁,也不覆盖网络缓冲;
客户端超时仍不能证明请求未执行.

`@spawn` (`src/task_control.rs::spawn_task`) 立即返回 task id,任务执行与控制 lane 分开.

### 当前处置

1. 长任务走 `@spawn` 并用 `@task-status` / `@task-output` 查询.
2. 同步 shell lane 的超时与进程树清理见上方 P0-1 行;排查排队时用独立 control
   调用发送 `@ping`,其计数范围见 P0-2 行和 `specs/control-line-protocol.md`.
3. `@cmd` shell 回显范围见 P1-3 行.

### 反模式（agent 侧自律）

为避免长任务占用同步控制 lane,使用 `@spawn` + `@task-status` 轮询;
必须同步等待时,将 `@cmd` 的超时纳入预期,并用独立 control 调用观察 `@ping`:

```powershell
# ✅ 正确
$p = Start-Process -FilePath X; "started"      # 立即返回
# ❌ 错误
$p = Start-Process -FilePath X -Wait            # 卡死控制面
```

### 附带坑：多行/复杂 PowerShell 走 SSH 会被引号吃掉

**现象**：`ssh host "powershell -NoProfile -Command \"...多行...\""` 发过去
**静默无输出**，目标侧文件一个没删 —— 看起来执行了，实际命令根本没送达。

**根因**：SSH 的多层引号 + PowerShell 的转义规则叠加，多行脚本在传输中被
截断或解析失败，且不报错。

**正解**：复杂 PowerShell 一律 **scp 传 .ps1 再执行**（当天实测可靠）：

```bash
# ❌ 静默失败
ssh host "powershell -Command \"多行...; 多行...\""

# ✅ 可靠
scp script.ps1 host:C:/Users/x/script.ps1
ssh host "powershell -ExecutionPolicy Bypass -File C:\Users\x\script.ps1"
```

这条在 @cmd / @spawn 的远程清理与批量脚本场景会反复遇到。

---

## P1 — 契约缺口：坐标空间与解释器语义没有前置声明

> **落地状态 (2026-10-06)**: P1-1 **已落地**; P1-2 **未落地**; P1-3 **部分落地**
> (范围有意收窄)。详见上方「落地情况」小节。

### P1-1 截图坐标 ≠ click 坐标（高危，agent 连续踩坑）

**现象**：4K 屏 + 200% 缩放，`@screenshot` 返回 3840×2160 的 jpg，
`@click` 接受 1920×1080 的 `os-logical` 坐标。agent 按截图上的像素位置直接点，
**连续三次点空**，浪费约 20 分钟。

**已有信息不足以自证**：`@screenshot` 的 manifest 里有
`image_coordinate_space: "virtual-logical-pixels"`，`@click` 的响应里有
`target_resolution.coordinate_space: "os-logical"`，但**没有任何一处告诉使用者
"你看到的像素坐标需要换算成 click 坐标"**，也没有给出 scale factor。

**建议**：

1. `@screenshot` 响应内直接给出 `click_coordinate_space` 与
   `image_to_click_scale`（本次应为 0.5），省掉人工推算。
2. `@click` 显式支持物理像素入口（如 `space:"image"` + 引用 screenshot_id），
   让"看着图点"这条路成立。
3. 文档/skill 里给出**换算口诀**：HiDPI + 缩放时 `click = image_pixel × os_logical / image_size`。

### P1-2 `@spawn` 缺"等一会儿并汇报"的一等语义

**现象**：想确认"安装器跑完没有"，只能自己写 `for` 循环轮询。第一版写成阻塞版
（见 P0），第二版写成无输出长循环 —— 用户再次反馈"不知道你在等什么"。

**建议**：给 `@spawn` 加可选的等待参数，例如 `@spawn:{"cmd":"...","wait_ms":5000}`
—— 5 秒内退出就带退出码回来，没退出就转成 background task id 并明确告知
"已转后台，请用 @task-status"。这一条能把绝大多数"短任务确认"从轮询里解放出来。

### P1-3 `@cmd` 用的哪个 shell，没有在响应里说

**现象**：Windows daemon 配的是 `powershell.exe`，但 `@cmd` 解析失败时的报错
（`控制指令 payload 后存在多余内容`）**不提示当前解释器**，导致我在
PowerShell 语法和 cmd 语法之间反复试。

另外实测两个具体解析坑：

- payload 以引号开头会被判为"多余内容"（`@cmd:"X=" + ...` → code 64）
- 含空格的未加引号路径会被 PowerShell 拆成两个 token
  （`@spawn:C:\Users\x\Uninstall Yino.exe` → `CommandNotFoundException`），
  必须写成 `& "path" args` 或 JSON 对象形式

**建议**：

1. `@cmd` 响应首行回显 `shell=<实际路径>`。
2. code 64 的错误信息补上"当前解释器是 X，PowerShell 下含空格路径请用 `& \"path\"`"。
3. 考虑对明显是 Windows 路径的裸 token 自动加引号（保守：只在高置信场景）。

---

## P2 — Windows 功能缺口：window lane 整体未实现

> **落地状态 (2026-10-06)**: **find / activate / close 已落地** (本节描述的是 2026-10-05
> 的状态，已过时); **resize 有意推迟**。详见上方「落地情况」小节。

**现象**：`@window-find` 在 Windows 上直接返回
`{"platform":"unsupported","status":"unsupported"}`，`@window-activate` /
`@window-close` / `@window-resize` 同理。

**后果**：今天至少有 4 个场景被迫退化到手搓 `@cmd`：

| 想做的事 | 退化方案 |
| --- | --- |
| 找到 Yino 窗口 | `@cmd:Get-Process \| Where-Object ...` |
| 把被终端盖住的安装器窗口拉到前台 | `@cmd:(New-Object -ComObject WScript.Shell).AppActivate("Yino 安装")` |
| 确认窗口是否还活着 | `@cmd:` 查进程表 |
| 看当前焦点窗口 | 截图人工判断 |

用户在过程中直接指出："他们只是跑到后面去了" —— 窗口被遮挡是远程 GUI 操作的
高频场景，而激活它是纯机械动作，本该是一等能力。

**建议**：Windows 侧用 `EnumWindows` + `GetWindowText` + `SetForegroundWindow`
实现 window-find/activate/close 三个 verb（都不需要 UI Automation，成本低）。
AX tree 在 Windows 上成本高，可以继续标 unsupported —— 但窗口**身份管理**和
**可访问性树**是两件事，不该一起缺。

---

## P3 — 可观测性缺口

> **状态复核 (2026-10-23)**: P3-1 的 scoped ingress 状态已通过 Native/Windows 指定回归;
> P3-2 的 Windows PowerShell 对照通过,但 `--no-default-features` 不覆盖 OCR. 详见上表。

### P3-1 daemon 卡死时客户端无诊断手段

历史现象是 daemon 被长任务占用后"看起来像不响应"。当时客户端没有 worker 状态查询。
当前实现通过独立 `@ping` 返回 busy 与 ingress 队列计数,仍不提供 reset session;
客户端超时也不能据此判断请求未执行或 daemon 已死锁.

`@ping` 的字段语义和局限以 `specs/control-line-protocol.md` 为准.

### P3-2 Windows 中文输出乱码

`@task-output` 回传 PowerShell 输出时，GBK/UTF-8 混杂（截图里能看到
"�޷�..."）。agent 只能靠猜测。

> **2026-10-23 校正与验证**: 既有 UTF-8 初始化只由同步 `execute_script_with_timeout`
> 调用, 不能据此断言后台 task 的编码已修复. 本轮移到
> `src/control_actions.rs::build_shell_command`, 让现有 shell 调用方共享初始化,
> 并在 PowerShell 源头关闭 progress. `@run` 不改写 args, 调用者负责所选程序
> 的编码. Windows 11 对照确认 progress 不输出、中文为 UTF-8、真实 error 保留且
> stderr 分类正确. 测试使用 `--no-default-features`, 不是 OCR 验收; 构建与共享 daemon
> 限制见 `specs/yino-rdog-usage-improvement-plan.md`.

---

## P4 — 缺失的组合原语

> **落地状态 (2026-10-08)**: **已落地**, 走 `@run`（commit `f7b94bd`）。
> `{path, args, timeout_ms, window, wait}` 结构化逐参 spawn 裸 exe, 不经 shell;
> 同步等退出 + 退出码, 超时杀树, `wait=false` 启动即回 pid,
> `window=visible` 供 GUI 安装器; stdout/stderr 回执只带尾部 2KB。
> 详见上方「落地情况」小节。

今天反复手搓同一个组合：**启动进程 → 等它退出 → 读退出码**。至少写了 5 次。

建议提供 `@run`（同步 + 强制超时 + 返回退出码）或 `@run-and-wait`
（后台 + 等待 + 超时后转后台 id），把"跑个程序看结果"变成一个动作。

---

## 优先级建议

状态列的逐条依据与**最后复核日期**见上方「落地情况」表 (本表不重复日期 ——
重复的日期本身也会过期, 单一真相源才活得住)。

| 级别 | 项 | 理由 | 状态 |
| --- | --- | --- | --- |
| **P0** | `execute_script` 超时 + 进程树清理 | 一次卡死拖垮整个控制面，且症状极具误导性 | 已落地 (a7ecbec) |
| **P0** | 控制面"排队中"状态可见 | 让 agent/人能区分"慢"与"死" | **仍未落地** |
| **P1** | 截图↔click 坐标换算前置声明 | 今天的实际时间黑洞 | 已落地 (a7ecbec); skill 口径 2026-10-22 修正 |
| **P1** | `@spawn` 短等待语义 | 消灭无反馈轮询 | 仍未落地 |
| **P2** | Windows window-find/activate/close | 高频场景，当前全靠退化 | 已落地 (a7ecbec); resize 有意推迟 |
| **P3** | daemon 诊断 + Windows 输出编码 | 可观测性 | 诊断部分落地 (bind_state/uptime 有; 队列深度无); **输出编码已落地 (536afd0)** |
| **P4** | `@run` 组合原语 | 消灭手搓"启动→等退出→读码" | **已落地 (f7b94bd)** |

---

## 一句话总结

r dog 的**动作面**（点击/截图/按键）在远程场景下够用；缺的是**时间面与身份面**：
"这个动作要跑多久、跑完了没有"和"这个窗口是谁、怎么把它叫到前面来"。
今天 8 小时的损耗，几乎全部落在这两个面上。
