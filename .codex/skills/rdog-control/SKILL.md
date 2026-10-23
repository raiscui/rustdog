---
name: rdog-control
version: "2.30-task-agent"
description: "Use for rdog control on a local or named machine: health, shell, GUI, browser, window, AX, PTY, flow, background tasks, companion agents, verification, and safety."
---

# Rdog Control

`rdog control` sends ordered frames to a trusted `rdog daemon`. Every bash call
must begin with `rdog control`. Read the literal `@response ` from full stdout.

## Tight Loop

1. Read current state and resolve one exact target.
2. Choose one lane below.
3. Send the narrowest semantic action once.
4. Read fresh state in a separate call.
5. Report only the result proved by that fresh read.

Use the local fast path when agent and daemon share one machine:

```bash
rdog control @ping
rdog control @ping @capabilities#1 @observe#2
```

Put `TARGET` after `control` only for a named or remote daemon. Permission and
capability errors are final results.

## Native App Lane

Use this lane for visible app controls, values, and buttons. `app:APP` is valid
only when the app resolves to one interactable window. `pid:PID/window:INDEX`
from a fresh `@window-find` is also accepted.

```bash
rdog control @open-app:APP
rdog control @ax-find:app:APP,ROLE
rdog control @ax-press:app:APP,DESCRIPTION
rdog control @ax-press-sequence:app:APP,DESCRIPTION_1,DESCRIPTION_2
```

### App Menu (应用菜单)

App 菜单栏不在窗口 AX tree 下面。菜单只用 `root:"app-menu"` 和 `app`;该 app
必须解析为唯一 PID,但不要求存在唯一或可交互的窗口。不能附带 window / process
选择器。先读菜单项,再将返回的 `pid:PID/menu-bar/path:...` 交给既有 AX action。

```bash
rdog control '@ax-find:{root:"app-menu",app:"Finder",role:"AXMenuItem",limit:20}'
rdog control '@ax-action:{target:{id:"pid:123/menu-bar/path:0.2"},action:"AXPress"}'
```

菜单 target 是短期定位器。每次动作前要 fresh `@ax-find`;不要把它伪装成
`pid:PID/window:N` 或跨应用复用。

### Text Input (文字输入)

面向 agent 的目标文字 mutation 统一使用带 observation-local ref 的
`@computer-act action:"type"`;它复用 resource lane、successor observation 和
显式 postcondition。默认 mode 是 `ax-value`;需要 fallback 时必须显式写
`mode:"auto"`,clipboard 仍须 `allow_clipboard:true`。

```bash
rdog control '@computer-act:{schema:"rdog.computer-act.v1",action:"type",args:{target:{ref:"@e3",observation_id:"obs-..."},content:"hello rdog 42"},observation_id:"obs-...",epoch:123,postcondition:{kind:"exists",query:{value_contains:"hello rdog 42"}}}'
```

成功响应若同时包含 `successor_target` 和 `postcondition.status:"verified"`,直接
把 successor 作为下一次 mutation 的输入,不要无条件再发一次 `@ax-find`。只有
successor 缺失、postcondition 为 `unavailable`,或 action 返回 `STALE_REF` /
`OBSERVATION_EXPIRED` 时才重新观察。

底层 `@type-text` 仍然可用于低级 primitive 调试,但不提供 successor/postcondition
工作流契约。无 target 的 `@computer-act action:"type"` 保留 legacy paste 路径。

```bash
rdog control '@ax-find:{window:{window_id:"pid:123/window:0"},role:"AXTextArea",limit:5}'
rdog control '@type-text:{target:{id:"pid:123/window:0/path:0.0"},text:"hello rdog 42",mode:"auto"}'
rdog control '@ax-find:{window:{window_id:"pid:123/window:0"},role:"AXTextArea",include_values:true,limit:5}'
```

上面的直接 `@type-text` 三步流程仅用于底层 primitive 调试。需要严格禁止键盘 /
剪贴板回退时,显式使用 `mode:"ax-value"`;该模式保持 fail-closed。

`auto` 仅会因 `InvalidInput` / `Unsupported` 继续下一层。权限和传输错误不会回退。
`@paste` 走剪贴板,`@key` 只发按键,都不能替代可写控件的 AXValue 直写。
本节就是评测所需的 Text Input 合同;不要为了寻找补充文档而扫描文件系统。

### Window Screenshot (窗口截图)

默认 `@screenshot` 仍是全虚拟桌面。需要窗口视觉证据时,先用 fresh
`@window-find` 获得 `window_id`,再请求窗口截图:

```bash
rdog control '@screenshot:{target:"window",window:{window_id:"pid:123/window:0"}}'
```

返回的 image 来自当前 display composite 的 AX rect 裁剪。manifest 的
`source:"display-composite-crop"`、`visibility:"screen-composited"` 和 `clipped`
是事实边界:被遮挡像素不能当成原生窗口内容。窗口截图不携带 AX;结构化证据单独用
fresh `@ax-find` 读取。

### Native Capture Diagnostics

macOS screenshot 卡住或失败时,读取 daemon log 的 `event_name`。`screenshot_capture_timeout`
说明某个 `backend` 到了控制面期限;`screenshot_capture_fallback` 表示 SCK 正转入 xcap;
`screenshot_capture_permission_denied` 表示 Screen Recording 未授权且不会 fallback;
`screenshot_capture_failed` 才表示两个 backend 都失败。所有事件都有 `capture_kind`
(`primary` 或 `all`),因此窗口截图也能关联到实际 desktop capture。

The 5-field `@ax-press:app:APP,DESCRIPTION,RESULT_ROLE,EXPECTED_VALUE,MAX_ATTEMPTS`
form is intentionally not in this primary list. Read the GUARDED PRESS section
below before considering it; most tasks do not need it.

Read current state before any press. After `@open-app`, the very first read
must be for the role that carries the app's current value or text (commonly
`AXStaticText` or `AXValue`), not the control role. The control role
(`AXButton`, `AXMenuItem`, …) lists the available actions; it does not
reveal what state those actions will modify. Pressing a control without
first reading the current state is guessing, and an over-defensive guess
(such as pressing a clear / reset button before input) is a protocol
violation, not a safety measure.

Use only descriptions returned by that fresh app-scoped AX read. A sequence
has exactly one comma field per observed AXButton description, in the
intended action order. Do not place a compound instruction or unobserved
label in one field.

GUARDED PRESS — opt-in only. The 5-field form
`@ax-press:app:APP,DESCRIPTION,RESULT_ROLE,EXPECTED_VALUE,MAX_ATTEMPTS`
makes the program repeat `DESCRIPTION` up to `MAX_ATTEMPTS` (1 to 3) times,
taking a fresh AX read of `RESULT_ROLE` after each press, and stopping as
soon as the read matches `EXPECTED_VALUE`. The program does not know the
app, the button meaning, or the desired baseline — those come from the model.

Use guarded press only when BOTH conditions hold:
  1. A prior fresh AX read of `RESULT_ROLE` already showed non-target
     content, AND
  2. You cannot safely count individual presses to reach the baseline
     (for example the stale length is unknown, or the input has nested
     structure).

If the fresh read already matches the intended baseline, issue input
directly without reset. If the stale content is a short known string or
number of characters, prefer counting N plain `@ax-press` calls; each
attempt then runs independently and the runner records them as separate
performed actions.

Do not preemptively reset before input. Adding a reset on a fresh app
that already shows the target baseline (or starts empty) is a protocol
violation, not a precaution. Reserve guarded press for genuinely stale
or unknown content.

After a guarded press, regardless of `verified`, take a separate fresh
`@ax-find` for the result role before issuing the next input step. The
fresh read is the only proof the reset actually took effect.

Clearing stale content is a step, never the end of a task: after the fresh
read proves the clear took effect, continue with the remaining input steps
and finish with the final confirm action. Do not stop after a reset, and do
not use shortcut keys (for example Esc) to clear — use the app's semantic
clear button and keep going until the full sequence is done.

Use `@key` for an explicit shortcut request or when no semantic action can express
the operation. `@key` covers explicit hotkeys (Cmd+T new tab) and global keys with
no AX semantics; clearing stale content, resetting state, and confirming buttons
are `@ax-press` responsibilities — do not use Esc / Delete / Cmd+A keyboard
shortcuts instead of pressing the actual button. Use guarded screenshot
coordinates only after semantic lookup fails.

### Local @key Actions

Outside a `@pty` session, `@key` targets the local macOS input system. Use the
compact form for a single key or chord, for example `@key:Cmd+R`. Use the object
form when timing, delivery, or an explicit target is needed:

```bash
rdog control '@key:{key:"R",modifiers:["Cmd"],mode:"press_release",hold_ms:200}'
rdog control '@key:{key:"R",delivery:"pid-targeted",pid:123}'
rdog control '@key:{key:"R",delivery:"window-targeted",window_id:"pid:123/window:0"}'
```

The object form accepts only `key` (required), `hold_ms`, `mode`, `modifiers`,
`delivery`, `pid`, and `window_id`. `delivery` defaults to `global`; targeted
delivery must name exactly one fresh `pid` or `window_id` and must not use an
app selector. `target`, `keys`, and `shortcut` are not `@key` fields. Do not
invent aliases for them or infer a target from the application name.

`@key` is a key action, not text input or an AX action alias. Use `@type-text`
for text and `@ax-press` for AXPress. Use `@ax-action` when the requested AX
action is something else, such as `AXConfirm` or `AXShowDefaultUI`.

## WeChat Temporary No-AX Policy

When the target app is WeChat (`com.tencent.xinWeChat`) or the user names "WeChat" / "微信", do not use AX to locate or interact with content controls.
This is a temporary fail-closed policy based on the 2026-07-14 ownership probe.

- Do not call `@ax-find`, `@ax-get`, `@ax-action`, `@ax-set-value`, `@ax-scroll`, or `rdog ax-diff` for WeChat content.
- Do not reuse AX-derived refs or labels such as `发现`, `直播`, or `发布`; a previous fallback attached a Xiaohongshu/Chrome AX tree to WeChat target metadata.
- The restriction applies to WeChat content targeting. `@window-find`, `@window-activate`, screenshot capture, guarded mouse actions, `@paste`, and targeted `@key` remain allowed.
- Use `@window-find` to resolve the current WeChat window, then capture a fresh visual observation with `include_screenshot:true`, `include_windows:true`, and `include_ax:false`.
- Locate visible controls from screenshot pixels plus the resolved window/display geometry. Use `guard.display`, and confirm the point is still inside the fresh window rect before coordinate actions.
- The OCR content layer is shipped: prefer `include_ocr:true` on that fresh screenshot request and locate content by OCR line boxes (`rdog.ocr.v1`, os-logical bbox) instead of pixel guessing; OCR boxes are coordinate hints, never AX refs. Full recipe: `references/cookbook-wechat-ocr.md` (spec: `specs/rdog-ocr-content-layer-plan.md`).
- If the window is hidden, occluded, or stale, activate it and capture a new screenshot before deriving coordinates. Do not use AX hit-testing to bypass occlusion.
- Verify every action with a fresh screenshot and window state. If visual ownership or coordinates are ambiguous, stop without clicking or typing.

Do not re-enable WeChat AX targeting until a controlled overlap/z-order regression proves root ownership and a live query reliably finds `文件传输助手` without accepting foreign browser content.

(Restored 2026-08-22 after the 2026-07-28 compaction removed it; durable
rationale: `docs/solutions/best-practices/gui-target-owner-evidence-gate.md`.
This is a safety boundary — do not remove it in token-optimization passes.)

## Browser Lane

浏览器内容动作直接遵守本 skill 的 AX/window/URL 证据合同。评测 cwd 可能不包含
仓库参考文件;此时不要读取相对路径,也不要执行 `find /` 或其他无界文件搜索。
Use global capture only to identify one focused browser, then targeted capture. After
navigation, use a fresh window or `browser:"active"`; never reuse `/window:N`
across turns.

Success requires fresh PID, display/window ownership, page or URL state, and the
intended content change. `performed:true` or a screenshot alone is incomplete.

## CDP Lane (Electron/Chromium 深度驱动, Windows 首选)

**定位**: 与 AX lane / Browser lane 互补不是替代。当 ①目标平台的 AX lane 整体
unsupported (Windows 实测如此) ②截图+坐标点击被 HiDPI 换算、前台窗口遮挡反复
搅局 ③需要读 JS 状态 (localStorage / IndexedDB / window 变量 / React 挂载态) 时,
CDP 可提供 DOM/JS 证据, 避免坐标定位; 原生窗口与落盘结果仍需独立核验.
**rdog 内置 CDP lane 由 daemon 连接目标机 loopback**, 也可通过下述脚本形态执行.
组合拳分工 = rdog 把 app 带参拉起到正确交互会话 (`@run` 或 `@cmd` Start-Process +
`--remote-debugging-port=9222`, CDP 只绑被控机 loopback) + 截屏做视觉取证,
CDP 驱动业务断言与输入。两条实现形态:

### 形态 A — daemon 内置 CDP lane (rdog 3.0+ 原生, 首选)

daemon 内置 CDP client (tungstenite, 连被控机 loopback 的 debug 端口), 控制端
直接发 verb, **不需要 ssh/隧道**:

```bash
# 被控端: rdog.toml 显式开启 (安全模型: CDP 等同任意代码执行, 默认关)
#   [cdp]
#   enabled = true
# 控制端:
rdog control <target> 5555 '@cdp-targets:9222'      # 列 targets (页面/worker)
rdog control <target> 5555 '@cdp-eval:9222,{"expression":"document.title"}'
rdog control <target> 5555 '@cdp:9222,{"method":"Runtime.evaluate","params":{...}}'
# @capabilities 会报告 "cdp":{"backend":"chrome-devtools-protocol"} 开关状态
```

⚠️ **未签名进程出站可能被安全软件按进程拦** (实测: 同机 loopback,
powershell/裸 TcpClient 通, 未签名 rdog-cdp.exe 连 9222 → 10060 静默丢 SYN,
提权/portproxy 都绕不过 — WFP 驱动层按进程签名)。识别特征 = 10060 出现在
connect 且白名单进程同目标 200。治本 = 给 exe 签名或加入安全软件信任区;
无法治本时用形态 B。

### 形态 B — CDP-over-@cmd (兜底, 免拦截免 ssh)

daemon 的 `@cmd` shell (powershell = Microsoft 签名, 白名单内) 跑 CDP 客户端
脚本: 被控机放一个 `cdp-eval.ps1` (ClientWebSocket 连 debug 端口,
Runtime.evaluate 回值; 支持 `-ExprFile` 传复杂表达式免引号转义), 控制端:

```bash
rdog control <target> 5555 '@cmd:powershell -NoProfile -ExecutionPolicy Bypass -File C:\path\cdp-eval.ps1 -Port 9222 -ExprFile C:\path\expr.txt'
```

复杂表达式一律走 `-ExprFile` (表达式写本地文件 scp 过去) — 单行 `-Expr`
的内层引号在 rdog 控制链路多层转义下必炸。

### @run — 裸 exe 同步执行 + 退出码

不经 shell: path/args 结构化逐参传递, 免引号转义; 收割/超时/杀树与 @cmd 同款。

```bash
# 同步等退出, 回 {exit_code,...}; 超时 (默认 120s) 杀树回 Timeout envelope
rdog control <target> 5555 '@run:{"path":"C:\\Windows\\System32\\cmd.exe","args":["/c","exit 7"]}'
# 启动不等待 (fire-and-forget), 立即回 {"pid":N}
rdog control <target> 5555 '@run:{"path":"...","args":[...],"wait":false}'
# GUI 安装器要看窗: "window":"visible" (默认 hidden = CREATE_NO_WINDOW)
```

- 中文输出的 CLI 先在 args 里自带 `chcp 65001&&` (@run 直接 spawn 裸 exe, 不做
  @cmd 那样的编码注入 — 不改写用户参数语义)。
- stdout/stderr 回执只带**尾部 2KB** (安装器输出 MB 级, 全量回传撑爆控制帧)。
- **被控机路径含空格时首选这个原语**: `{path, args}` 逐参传递, 整条链不经 shell,
  没有引号层 (`@cmd` 那种裸 shell 行的引号在远端未必保得住)。
- `wait:false` 只证明已取得 PID, 不证明 GUI ready 或能脱离父进程长期存活.
  `@cmd`/客户端超时也不证明没有启动. 重发前核对 exe 路径、profile、PID、
  Windows Session、窗口和端口; 已有目标实例时继续观察, 避免重复启动.

### Windows 作业前后核查

1. 先读目标 `@ping`/`@capabilities`, 以远端实际回执判断能力, 不以本地 client
   或文档版本推定 daemon 支持 `@run`. `code:64` 时先看 error, 区分未知命令与
   payload 错误. 旧 daemon 的能力缺口不授权自动更新或重启共享实例.
2. GUI 作业先确认 daemon 与目标进程属于同一交互 Session, 再 fresh
   `@window-find` 定位. SSH Session0 的窗口句柄为 0 不证明应用已退出.
   命名发现失败不等于主机失联; 只有已核实的 TCP/SSH 入口才可作为替代.
3. shell 初始化真相源为 `src/control_actions.rs::build_shell_command`.
   PowerShell 默认关闭 progress, Windows shell 复用 UTF-8 初始化;
   旧 daemon 或 `@run` 显式启动 PowerShell 时, 调用脚本自行设置
   `$ProgressPreference='SilentlyContinue'`. 不整体丢弃 stderr/CLIXML.
   `shell_stderr` 表示退出码为 0 但仍有 stderr, `shell_exit_nonzero` 才表示
   非零退出; 两者都保留诊断, 不能直接代替 fresh 业务状态核验.
4. 截图必须匹配 fresh PID/window、display 与实际项目/profile. 全桌面图可能
   拍到别的工作区或遮挡窗口; 不把图片存在当成目标页面验收.
5. Windows 的非交互 shell 经 `build_shell_command` 启动时使用
   `CREATE_NO_WINDOW`, 包括 agent shell; taskkill helper 也隐藏终端.
   `@run` 保留自己的 window 可见性选项, PTY 保留交互终端语义.
   这只隐藏执行 helper, 不隐藏命令显式启动的 GUI app.

### 通用坑 (两形态都适用)

- **app 重启后 target 重取** (GET /json), 旧 WS 报 "other side closed"。
- React 白屏定位: evaluate
  `document.getElementById('root').childElementCount` (=0 即崩) +
  console 收集 (Runtime.enable + Log.enable + Page.reload, 收
  exceptionThrown / consoleAPICalled / Log.entryAdded)。
- **rdog 3.0 的 `daemon` 子命令强制转隐藏常驻** (spawn detached child 后立即
  返回): schtasks 会话里 detached child 可能静默死 (child 校验
  `[hidden].log_file` 与 parent 推导不一致即 exit)。**配方 = 前台启动 bat**:
  `set RDOG_DAEMON_FOREGROUND=1` + `rdog daemon --config ...` 写成 bat,
  schtasks /IT 跑 bat (bat 常驻 = daemon 常驻, 落用户交互会话 Session 1)。
- Electron 主进程与渲染层是两个取证面 — 渲染层 CDP 拿不到主进程状态; 需要主
  进程信息时让 renderer 挂全局 getter (`window.__xxxGetter`), main 进程经
  `event.sender.executeJavaScript` 回取。
- 视觉证据 (布局/像素/渲染结果) 仍用 rdog `@screenshot`; 文件对话框/OS 权限
  弹窗等不可达 DOM 的系统级操作用 rdog 点击/按键。

### Windows 实战三招 (2026-10-11 TB13 跨机测试沉淀)

- **带空格路径拉起: 首选 `@run`, 别在控制行里靠引号**: 现场见过 `Start-Process
  '带 空格'` 的单引号到 daemon 侧消失、路径被空格断词; 但 **rdog 源码里找不到
  剥单引号的代码路径** (协议层只认双引号, one-shot 行直接来自 argv), 消失层级
  尚未定位, 所以别依赖引号能活到远端。`@run` 的 `{path, args}` 结构化逐参 spawn
  裸 exe, 整条链**没有 shell 也就没有引号层**; 必须走 shell 时用 ssh 建无空格
  链接 `mklink C:\link.exe "C:\real path.exe"` (ssh 引号自己全控), rdog 只跑无空格
  路径。注意目标卷可能禁用 8.3 短名 (`dir /x` 无短名列), 通配符也不是解法。
- **main 进程取证 = --inspect**: renderer CDP 看不到主进程。拉起加 `--inspect=9230`,
  ps1 客户端要**不过滤 type 的变体** (node target 的 type 不是 `page`, `Where type -eq
  'page'` 会取空)。字段事实: `ipcMain._events` 只存 on() 注册, `handle()` 存
  `_invokeHandlers` (Map) — 查挂载清单两处都要看; handler 可直接手动调用
  (`_events[ch]({sender:null}, payload)` 或 `_invokeHandlers.get(ch)({sender:null},...)`)
  一步验证「handler→状态→消费面」全链, 免黑盒对猜。
- **发送链 vs 行为错, 双通道一次分清**: 疑似「renderer 发了 main 没收到」时, renderer
  CDP (9222) 发 + main inspector (9230) 手动调消费 handler 读态 — inspector 侧行为正确
  则失效点在发送链, 反之在 handler 逻辑。单侧取证永远在对猜 (实证: busy 疑云三轮黑盒
  猜测, 双通道一击定谳)。
- 方法学与适用边界: `docs/solutions/best-practices/electron-main-process-forensics.md`
  (同场实战另两条沉淀: `docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md`)

## Background Tasks (Phase 3)

`@spawn` runs a shell command in the background and answers immediately -
the same session keeps serving other requests while it runs:

```bash
rdog control @spawn#11:cargo build --release   # -> {"task":"t-a1b2c3d4","state":"running"}
rdog control @task-status:t-a1b2c3d4           # state + exit_code when terminal
rdog control @task-output:t-a1b2c3d4           # tail (default 80 lines)
rdog control @task-cancel:t-a1b2c3d4           # sync kill; idempotent on terminal
```

Terminal frames (`@task-completed` / `@task-failed`) are pushed on the session
channel; `canceled` reuses `@task-failed` with `"canceled":true`. `@spawn`
tasks emit no progress frames - poll `@task-output` for details.
`@task-cancel` terminates and reaps the task process tree: Unix tasks use an
isolated process group; Windows uses checked `taskkill /F /T /PID`. A kill or
wait error remains retryable and cannot be reported as `canceled`; if the root
process already exited, use its actual exit status. A child detached before
cancellation is outside the guaranteed tree, so verify task-owned PIDs and
ports before retrying or declaring cleanup complete. Do not kill by process
name when another task or app may own a matching process.

Daemon restart: old task ids honestly report `task not found`.

Companion agents (hosted processes consuming task messages) live on their own
lane - see `references/agent-messaging.md` for the full loop.

## Other Lanes

| Need | First action |
| --- | --- |
| Health | `@ping` |
| Permissions | `@capabilities` or `@bootstrap` |
| Window ownership | `@window-find` |
| One shell command | `@cmd` or `@script` |
| Finite workflow | `@flow` |
| Stateful terminal | `rdog control TARGET --pty -- COMMAND` |
| Long command (non-blocking) | `@spawn` then `@task-status` / `@task-output` / `@task-cancel` |
| 跑裸 exe/安装器要退出码 (免 shell 引号) | `@run:{...}` (结构化 path+args, 见 @run 一节) |
| Electron/Chromium DOM 级驱动 (AX 不可用/需读 JS 状态) | CDP Lane (见上; rdog 拉起 + ssh 隧道 + 裸 CDP) |
| Delegate task to companion agent | `@agent-register` / `@agent-inbox` / `@agent-ack` |
| Agent capability discovery | `@agent-card:NAME` |

When a control call appears stuck, issue `@ping` through a separate control
invocation or connection. Read its `control` snapshot as scoped Zenoh ingress
evidence: same-session requests remain FIFO, and a client timeout does not prove
the request did not execute. Check current task/process state before retrying;
do not reset or restart a shared daemon to clear the snapshot.

本节和上面的具体 lane 合同已经覆盖常规评测路径。若当前 cwd 没有额外参考文件,
直接依据本 skill 和 daemon 的结构化响应继续;禁止执行 `find /`、`locate` 或其他
无界搜索来寻找文档。只有用户明确提供了可读的绝对路径时才读取额外资料。

## Failure Handling
🔴 **For `@computer-act` responses, read `ok` first.** When `ok:false`, use
`error_code` and `retry`; `outcome:"unknown"` is only a shape placeholder.
When `ok:true`, `outcome:"worked"` permits continuation, `"didnt"` requires a
new selector or verification policy without repeating the same action, and
`"unknown"` requires fresh observation before retry. `verification.status` is
the AX-diff signal: `verified` or `failed`. `postcondition.status` is separate
and may be `verified`, `failed`, or `unavailable`. Do not search the
filesystem for a separate outcome document; this section is the execution
contract for agent decisions.

Ref-based `@computer-act` uses this compact form:
`@computer-act:{schema:"rdog.computer-act.v1",action:"click",args:{target:{ref:"@e3",observation_id:"obs-..."}},observation_id:"obs-...",epoch:123}`.
After success, use the complete `successor_target` object for the next mutation.
Refs are observation-local: never combine an old `ref` with a new
`successor_observation`. If `successor_target` is absent, run a fresh scoped
`@ax-find` before the next mutation.

Map `successor_target.ref` and `.observation_id` into `args.target`; also copy
that `.observation_id` and `.epoch` to the next request's top level. `epoch` is
not an AX target field.


🔴 **REPAIR — when a control fails.** When a control returns `performed:false`, zero matches, or an unexpected
fresh read, walk the three-tier ladder below before retrying with a new
lane or locator. Each repair step must end with a fresh read that proves
the change actually took effect on the relevant subtree.

| Trigger (observable evidence) | First repair | Still failing |
| --- | --- | --- |
| `@window-find` matches 0 windows | Verify the app is open (`@open-app` first); check `app:APP` spelling — ASCII case-insensitive after v2.18, but Chinese names do not resolve | Take a `@screenshot` to confirm visual state; do not guess window state blind |
| `@ax-find` returns 0 elements for a role | Verify role spelling; re-query the parent role (e.g. `AXWindow`) to find the right subtree first | Drop to `@key` keyboard shortcut if the role has no children |
| `@ax-press` returns `performed:false` | Re-`@ax-find` for the role; the button may have been renamed, moved, or disabled | Switch to a sibling description or `@key` |
| `@ax-press-sequence` stops mid-way | The failed step's `target_id` is in the response; re-query just that role and continue from the failing step | Restart the sequence from a known fresh state (`@ax-find` first) |
| Fresh AX read returns a different value than expected | You may have read the wrong subtree; re-query with the correct value role (commonly `AXStaticText` or `AXValue`) | Use `@screenshot` for visual confirmation |
| Window ownership is ambiguous (≥2 matching interactive windows) | Disambiguate via fresh `@window-find` and pass `pid:PID/window:INDEX` instead of `app:APP` | Stop; ask the user which window is the target |
| `@capabilities` reports `permission_status: denied` or `unavailable` | The capability name is in the response; for Accessibility on macOS, system prompt requires System Settings → Privacy & Security → Accessibility | Do not retry without the missing permission; further calls will keep failing identically |
| `@ping` times out or returns non-`pong` | Check the daemon log/socket path; on macOS, restart with `rdog daemon --config rdog_macos.toml` | Do not keep issuing commands; the daemon may be wedged and a flood will only obscure the real error |
| `@screenshot` writes to a path the agent cannot read | The daemon writes to its own temp dir; do not guess the path; use the `@observe#N` ref the response returned | Switch to global AX read without window scope |
| `rdog control` rejected a frame with `code: 64` | Read the literal `error` field; it is the single source of truth for parser/locator failures | Do not retry the same payload — fix the indicated field first |

Never infer app state from screenshot output alone; screenshot is a
cross-check, not the primary source. The primary source is always a
fresh `@ax-find` (or `@window-find` for window state) read that returns
matching elements.

## Anti-Patterns (Don't)

Each row is a behavior we have observed in real rdog-control sessions.
None of these are valid tactics.

| Don't do | Why |
| --- | --- |
| Use `@screenshot` as the primary source of state | Screenshots are a cross-check; the canonical source is a fresh `@ax-find` or `@window-find` that returns matches. A screenshot cannot tell you which role's value is which. |
| Retry the same `@ax-press` more than three times with the same description | If the description still does not resolve, the locator is wrong. Re-query the role or change lane; do not throw the same payload at the daemon. |
| Pre-emptively press a clear / reset button on a fresh app | A reset before reading state is a protocol violation, not a safety measure. The fresh read tells you whether the app already starts at the target baseline. |
| Use `@cmd` to compute math, parse JSON, or evaluate expressions | Shell math has nothing to do with the GUI task. `@cmd` is for legitimate system commands; bypassing semantic actions defeats all rdog-control evidence. |
| 🔴 Use Esc / Delete / Cmd+A keyboard shortcuts to clear or reset app state | Clearing stale content, dismissing dialogs, or selecting all are visible control operations. They must be done with `@ax-press` on the actual button (e.g. a clear button). Keyboard shortcuts are not a substitute and will not be counted as performed button actions. |
| Pass `app:计算器`, `app:計算機`, or any non-ASCII app name | `@window-find` only resolves ASCII app names. Non-ASCII names return 0 matches and will never succeed. |
| Mix `app:APP` with `pid:PID/window:INDEX`, `process:`, or `window_title:` in the same target | The target validator rejects the request. Pick exactly one ownership channel. |
| Reuse `@eN` ref across daemon restarts or turn boundaries | Observation refs are short-lived. A ref that resolved in the previous turn may resolve to a different element today. Re-query via `@ax-find`. |
| Skip the fresh read between consecutive `@ax-press` calls | Every press must prove `performed:true` AND that the press changed the visible state for the value role. Sequential presses without reads cannot tell which press had which effect. |
| Treat `performed:true` as proof of success | `performed:true` only proves the press was dispatched. The result depends on a fresh `@ax-find` of the value role. |
| Assume a fixed number of reset presses when stale length is unknown | Use guarded press (`@ax-press:app:APP,BUTTON,RESULT_ROLE,EXPECTED_VALUE,MAX_ATTEMPTS`) so the program verifies each press. Counting blindly produces either residual content or wasted actions. |
| Pipe `@response` output to `jq` / `grep` / shell tools | Pipe bypasses the protocol; the daemon records `usedPipeInRdogCommand=true` and the audit field flags the run. Read `@response` literally from full stdout. |
| 🔴 Press `Cmd+Q` / `Cmd+W` / `Esc` / other destructive shortcuts without explicit user confirmation | These are terminal actions. The Safety section requires user confirmation before destructive operations. |

If you find yourself reaching for any of the above, the correct fix is
almost always: re-`@ax-find` for the right role, read the result, then
choose the smallest semantic action.

## When to Defer to the User

Stop the agent loop and surface to the calling user when any row below holds.

| Marker | Condition | Action |
| --- | --- | --- |
| 🔴 DEFER | Task involves `Cmd+Q`, `Cmd+W`, reboot, or any irreversible UI action | The user must explicitly confirm |
| 🔴 DEFER | Task needs permissions `@capabilities` reports denied | User grants permissions out-of-band (macOS: System Settings → Privacy & Security → Accessibility / Input Monitoring) |
| 🔴 DEFER | Window ownership ambiguous AND task is destructive (close / delete / submit) | Read-only tasks may proceed; write tasks must defer |
| 🛑 STOP | Same `@ax-press` description retried ≥ 3 times without `performed:true` changing | Surface the last failed description and error code |
| 🛑 STOP | Two consecutive fresh AX reads return different element roles or paths (not just different values) | The app's AX tree has shifted; surface the inconsistency |

When none of the above holds, continue the agent loop autonomously.
## Safety

🛑 **STOP — destructive operations require user confirmation.** Stop on ambiguous ownership, permission denial, or a failed action. Retry a step at
most three times with a different locator or lane. Ask before destructive, security,
permission, reboot, or production actions.

WeChat content targeting always follows the WeChat Temporary No-AX Policy above:
ambiguous visual ownership means stop, not guess.
