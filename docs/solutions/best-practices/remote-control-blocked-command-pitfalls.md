---
title: 远程被控端实战反馈 — 阻塞死锁、坐标空间与 Windows 功能缺口
date: 2026-10-06
last_updated: 2026-10-06
module: control plane
component: cmd/spawn/screenshot/window-lane
problem_type: feedback
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
resolution_type: "P0-1 / P1-1 / P1-3 / P2 已由 commit a7ecbec 落地并真机复验; P0-2 / P1-2 / P3-1 部分落地 (只覆盖 bind_state/uptime 与 shell 回显, 未覆盖队列深度与 worker 忙闲); P3-2 / P4 完全未做"
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
(4K@200%) 上复验。**未落地的条目全部保留在下方原章节里作为待办**，
下表是唯一的进度真相源 —— 读某一节的问题描述时，回来看这张表看它现在什么状态。

| 条目 | 状态 | 依据 (源码 / 验证) |
| --- | --- | --- |
| **P0-1** `@cmd` 超时 + 杀进程树 | **已落地** | `src/control_actions.rs:672-695` (`RDOG_SHELL_LANE_TIMEOUT_MS`, 默认 120s)、`:737-807` 超时/取消分支、Windows 走 `taskkill /F /T`、unix 走进程组。真机复验: 15s 超时后 `@ping` 仍活 |
| **P0-1** 协议层显式区分"排队中"与"已死锁" | **未落地** | 全仓无 `busy` 语义、无控制面 worker 队列深度字段。超时只是不再无限期挂死，客户端仍分不出"慢"与"死"。`src/control_session.rs:73` 的 `queued_frames` 是**出站帧队列**，与此无关 |
| **P1-1** 截图↔click 坐标换算前置声明 | **已落地** | `src/screenshot.rs:437` manifest 补 `click_coordinate_space`，`src/control_mouse/target.rs:417` 补 `image_to_click_scale`；`transforms` 换算公式改用 `os_logical_bounds`。真机复验: 4K 图上 (1210,2112) → os-logical (605,1056) 点中开始菜单 |
| **P1-1** `@click` 支持物理像素入口 | **已落地** | `src/control_mouse/screenshot_frame.rs` (418 行): 截图坐标 → os-logical 的进程内缓存 (TTL 10min / 容量 32)，`@click` 接受 `{screenshot, x, y}` 引用式端点 (`screenshot_frame.rs:127-128` 为换算式) |
| **P1-1** 文档/skill 里的换算口诀 | **未核实** | 不在 `a7ecbec` 的 24 个改动文件内。`~/.zcode/skills/rdog-control` 不在本仓，需另行确认 |
| **P1-2** `@spawn` 短等待语义 (`wait_ms`) | **未落地** | 全仓无 `wait_ms` 字段。**建议 1 原文的方案未被采纳也未被否决**，需重新拍板 |
| **P1-3** 响应回显实际 shell | **部分落地 (范围有意收窄)** | 失败 / 超时 / 取消三类响应带 `shell` (`control_actions.rs:935/951/972`)，`@ping` 也带 (`control_actions.rs:993`)；**成功响应刻意不带** —— `control_actions.rs:782-786` 写明理由: 成功形状是既有契约，大量消费方按字符串读。原文"响应首行回显"是按全部响应提的，落地方案比建议窄 |
| **P1-3** 裸路径缺引号的解释器提示 | **部分落地** | `src/control_protocol/parsers.rs:766` 对"含空格的裸路径"给 PowerShell `& "path" args` 提示。但提示文案里**没有点明当前解释器是 cmd 还是 PowerShell** —— 原文建议 2 的后半句未做 |
| **P1-3** 自动给裸路径加引号 | **明确未做 (有意否决)** | `parsers.rs:887` 注释: 单 token 裸路径在 PowerShell / cmd 下本来就能跑，拦下就是假阳性。commit 记录为"只提示不代改写" |
| **附带坑** 引号 payload 被判多余内容 | **已落地** | `parsers.rs` 增加闭合引号后的容错尾巴，并放宽引号内 shell 文本的转义 (修 `C:\Program Files` 被当转义序列) |
| **附带坑** 多行 PowerShell 走 SSH 被引号吃掉 | **未落地 (agent 侧自律, 非产品问题)** | 仍是"复杂 PowerShell 一律 scp 传 .ps1 再执行" |
| **P2** Windows window-find / activate / close | **已落地** | `src/control_window/windows.rs` (1308 行，本次首次被编译)；`src/control_window.rs:735/836/842` 三个 `execute_default_*` 入口。真机复验: `@window-find` 列出 320 个窗口 |
| **P2** Windows window-resize | **有意推迟** | `src/control_window.rs:266-269` 注明: 窗口身份链 (EnumWindows) 已就位，但 resize 要确定外框与 per-monitor DPI 的尺寸语义，尚未验证。刻意不做成"平台整体 unsupported"，以免调用方分不出"能列窗口"与"能改尺寸" |
| **P3-1** daemon 诊断 | **部分落地** | `@ping` 结构化 + `daemon::bind_state()` / `daemon::uptime()` 公开 (`control_actions.rs:987-996`)。**worker 忙闲与队列深度未做**，也**没有 reset session 入口** |
| **P3-2** Windows 中文输出乱码 | **未落地** | 子进程输出仍走 `String::from_utf8_lossy` (`task_control.rs:96`、`control_actions.rs:611/927/948`)。无 `[Console]::OutputEncoding` 前缀、无 `-EncodedCommand` |
| **P4** `@run` / `@run-and-wait` 组合原语 | **未落地** | 全仓无该 verb |
| **运维侧新增** Windows 交叉编译配方 | **已补文档** | 见 `docs/solutions/runtime-errors/cross-compile-windows-rust-electron-tool.md`。此前 CI 无 Windows 覆盖、文档零记录 |

**真机复验口径** (`task_plan.md` 2026-10-06 12:36 条目): 六项全部通过，
无失败项; 远端跑的是当日 12:18 交叉编译的产物，SHA256 两端一致
(`A2CDE103...`, Size 43611136)。

**残留风险一句话**: 超时/坐标/窗口三条"卡住用户"的主线已通，
剩下的是**可观测性欠账** —— P0-2 与 P3-1 都停在"能看出 daemon 活着"，
没到"能看出它忙"；P3-2 的乱码会让下一轮远程排障继续靠猜。

---

## P0 — 阻塞死锁：同步 shell lane 无超时，一次卡死拖垮整个控制面

> **落地状态 (2026-10-06)**: 超时 + 杀进程树 **已落地**; "排队中"状态可见 **未落地**。
> 详见上方「落地情况」小节。

### 现象

执行 `@cmd:Start-Process ... -Wait` 之后，**后续所有 control 命令都没有执行**。
用户侧观察是"命令好像没生效，只有我打断它之后才会继续"。

### 根因（源码实证）

`src/control_actions.rs:614` 的 `execute_script`:

```rust
pub(crate) fn execute_script(shell: &str, script_text: &str) -> io::Result<ActionExecutionResult> {
    let output = build_shell_command(shell, script_text).output()?;   // ← 裸同步等待, 无超时
    Ok(from_process_output(output))
}
```

`Command::output()` 无限期等待子进程退出。`-Wait` 类命令不返回 → 控制面 worker
被占死 → 后续请求排队 → 对客户端表现为"命令没执行"。

这个**症状与病因严重错配**：不是解析失败、不是权限问题、不是网络断连，而是排队。
客户端没有任何信号能区分这三种，agent（和人）都会往错误方向归因。

值得注意的是，`@spawn`（`task_control.rs::spawn_task`）是异步的，**没有这个问题** ——
差异没有在协议层暴露出来，agent 只能靠踩坑发现。

### 建议

1. **`execute_script` 加超时**（必做）。超时后杀掉**进程树**（不是只杀 shell ——
   `Start-Process` 起的子进程会残留，正是今天排查 Windows 安装器时踩到的形态）。
2. **协议层显式区分"排队中"与"已死锁"**：控制面 worker 忙时，客户端应收到
   明确的 `busy` 语义（可附队列深度/已等待时长），而不是静默等待。
3. **`@cmd` 响应里回显实际使用的 shell**（见 P1-3），让 agent 知道自己在哪个
   解释器里。

### 反模式（agent 侧自律）

在 rdog 修好之前，**远程 shell lane 内禁止任何阻塞等待**。要等长任务用
`@spawn` + `@task-status` 轮询；必须同步等时显式加超时：

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

> **落地状态 (2026-10-06)**: P3-1 **部分落地** (bind_state / uptime 有, worker 忙闲与
> 队列深度无); P3-2 **未落地**。详见上方「落地情况」小节。

### P3-1 daemon 卡死时客户端无诊断手段

今天遇到过 daemon 被占死后"看起来像不响应"。客户端没有健康检查、没有
worker 状态查询、没有"reset session"入口。

**建议**：`@ping` 响应里带 worker 忙闲状态与当前排队深度；长时间无响应时
`@ping` 应明确区分"网络不通"与"worker 被占"。

### P3-2 Windows 中文输出乱码

`@task-output` 回传 PowerShell 输出时，GBK/UTF-8 混杂（截图里能看到
"�޷�..."）。agent 只能靠猜测。

**建议**：daemon 侧统一按 UTF-8 读子进程输出（PowerShell 加
`[Console]::OutputEncoding = [Text.Encoding]::UTF8` 前缀，或用 `-EncodedCommand`
传 UTF-16LE 脚本）。

---

## P4 — 缺失的组合原语

> **落地状态 (2026-10-06)**: **未落地**, 全仓无该 verb。详见上方「落地情况」小节。

今天反复手搓同一个组合：**启动进程 → 等它退出 → 读退出码**。至少写了 5 次。

建议提供 `@run`（同步 + 强制超时 + 返回退出码）或 `@run-and-wait`
（后台 + 等待 + 超时后转后台 id），把"跑个程序看结果"变成一个动作。

---

## 优先级建议

状态列的逐条依据见上方「落地情况」小节。

| 级别 | 项 | 理由 | 状态 (2026-10-06) |
| --- | --- | --- | --- |
| **P0** | `execute_script` 超时 + 进程树清理 | 一次卡死拖垮整个控制面，且症状极具误导性 | 已落地 (a7ecbec) |
| **P0** | 控制面"排队中"状态可见 | 让 agent/人能区分"慢"与"死" | 未落地 |
| **P1** | 截图↔click 坐标换算前置声明 | 今天的实际时间黑洞 | 已落地 (a7ecbec) |
| **P1** | `@spawn` 短等待语义 | 消灭无反馈轮询 | 未落地 |
| **P2** | Windows window-find/activate/close | 高频场景，当前全靠退化 | 已落地 (a7ecbec); resize 有意推迟 |
| **P3** | daemon 诊断 + Windows 输出编码 | 可观测性 | 部分落地 (bind_state/uptime 有; 队列深度与乱码无) |

---

## 一句话总结

r dog 的**动作面**（点击/截图/按键）在远程场景下够用；缺的是**时间面与身份面**：
"这个动作要跑多久、跑完了没有"和"这个窗口是谁、怎么把它叫到前面来"。
今天 8 小时的损耗，几乎全部落在这两个面上。
