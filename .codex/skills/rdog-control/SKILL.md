---
name: rdog-control
version: "2.27-key-contract"
description: "Use for rdog control on a local or named machine: health, shell, GUI, browser, window, AX, PTY, flow, verification, and safety."
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

向文本框 / 文本区 / 地址栏输入文字时,`@type-text mode:"auto"` 会先尝试 AXValue
直写。只有 AXValue 不可写或不支持时,才回退到 targeted keyboard;clipboard 仍须
`allow_clipboard:true`。response 的 `delivered_via` 是本次实际投递路径。

```bash
rdog control '@ax-find:{window:{window_id:"pid:123/window:0"},role:"AXTextArea",limit:5}'
rdog control '@type-text:{target:{id:"pid:123/window:0/path:0.0"},text:"hello rdog 42",mode:"auto"}'
rdog control '@ax-find:{window:{window_id:"pid:123/window:0"},role:"AXTextArea",include_values:true,limit:5}'
```

三步流程:先 `@ax-find` 拿到文本控件的 AX id → `@type-text mode:"auto"` 写入 →
再独立 `@ax-find` 读回 value 证明输入生效。需要严格禁止键盘 / 剪贴板回退时,
显式使用 `mode:"ax-value"`;该模式保持 fail-closed。

`auto` 仅会因 `InvalidInput` / `Unsupported` 继续下一层。权限和传输错误不会回退。
`@paste` 走剪贴板,`@key` 只发按键,都不能替代可写控件的 AXValue 直写。详细语法见
`references/protocol.md` 的 Text Input 小节。

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

## Browser Lane

Read `references/cookbook-web-content.md` before a browser content action. Use
global capture only to identify one focused browser, then targeted capture. After
navigation, use a fresh window or `browser:"active"`; never reuse `/window:N`
across turns.

Success requires fresh PID, display/window ownership, page or URL state, and the
intended content change. `performed:true` or a screenshot alone is incomplete.

## Other Lanes

| Need | First action |
| --- | --- |
| Health | `@ping` |
| Permissions | `@capabilities` or `@bootstrap` |
| Window ownership | `@window-find` |
| One shell command | `@cmd` or `@script` |
| Finite workflow | `@flow` |
| Stateful terminal | `rdog control TARGET --pty -- COMMAND` |

Read only the matching reference before using an Other Lane:

- `references/control-workflow.md`: targets, windows, PTY, hosts, and hardware.
- `references/protocol.md`: frame syntax, responses, errors, AX, mouse, and flow.
- `references/zenoh-hardware.md`: discovery, SDK, serial, and bridge hosts.

## Failure Handling
🔴 **READ FIRST for `@computer-act` responses.** Before matching on `ok` or `error_code`,
read [references/computer-act-outcome.md](references/computer-act-outcome.md).
The shape changed in `feature/computer-act-outcome-3state`: `outcome` and
`verification.status` carry postcondition signal that used to live in
`error_code:"verify_failed"`. Phase F-2's `ok:false + error_code:verify_failed`
path is removed.


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
