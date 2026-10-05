---
title: 远程被控端实战反馈 — 阻塞死锁、坐标空间与 Windows 功能缺口
date: 2026-10-06
last_updated: 2026-10-06
module: control plane
component: cmd/spawn/screenshot/window-lane
problem_type: feedback
severity: high
status: proposed
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

## P0 — 阻塞死锁：同步 shell lane 无超时，一次卡死拖垮整个控制面

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

今天反复手搓同一个组合：**启动进程 → 等它退出 → 读退出码**。至少写了 5 次。

建议提供 `@run`（同步 + 强制超时 + 返回退出码）或 `@run-and-wait`
（后台 + 等待 + 超时后转后台 id），把"跑个程序看结果"变成一个动作。

---

## 优先级建议

| 级别 | 项 | 理由 |
| --- | --- | --- |
| **P0** | `execute_script` 超时 + 进程树清理 | 一次卡死拖垮整个控制面，且症状极具误导性 |
| **P0** | 控制面"排队中"状态可见 | 让 agent/人能区分"慢"与"死" |
| **P1** | 截图↔click 坐标换算前置声明 | 今天的实际时间黑洞 |
| **P1** | `@spawn` 短等待语义 | 消灭无反馈轮询 |
| **P2** | Windows window-find/activate/close | 高频场景，当前全靠退化 |
| **P3** | daemon 诊断 + Windows 输出编码 | 可观测性 |

---

## 一句话总结

r dog 的**动作面**（点击/截图/按键）在远程场景下够用；缺的是**时间面与身份面**：
"这个动作要跑多久、跑完了没有"和"这个窗口是谁、怎么把它叫到前面来"。
今天 8 小时的损耗，几乎全部落在这两个面上。
