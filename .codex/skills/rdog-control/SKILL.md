---
name: rdog-control
version: "1.0"
description: Use when an AI agent, coding assistant, automation tool, or operator needs to control a named LAN or reachable host, hardware bridge machine, lab device, or microcontroller through `rustdog`/`rdog`. Covers `rdog daemon`, `rdog control`, Zenoh target-name discovery, `--entry-point` fallback, the **local fast path** shortcuts `rdog control @<line>` and `rdog control self @<line>` (and `rdog control --namespace <ns> @<line>`) which scan `$TMPDIR/rdog-{ns}-*.pipe_uplink` to find the unique local daemon when no `TARGET` is given, line-control commands like `@ping`, `@bootstrap`, `@capabilities`, `@cmd`, `@key`, `@paste`, `@observe`, `@screenshot`, `@window-find`, `@window-activate`, `@window-close`, `@web-find`, `@web-act`, `@gui-bench`, `@ax-tree`, `@ax-find`, `@ax-get`, `@ax-press`, `@selector-get`, `@selector-resolve`, `@selector-refind`, `@mouse-move`, `@mouse-button`, `@click`, `@drag`, `@wheel`, `@savefile`, remote PTY flows such as `rdog control TARGET --pty -- COMMAND`, and the `rdog ax-diff` subcommand for structured AX snapshot diff. Works for Codex, Claude, GPT, openai-compatible clients, MCP agents, and human operators.
---

# Rdog Control

## Core Contract

Treat `rdog control` as a stdio-friendly remote control bridge, not as SSH.

The normal short-task path is:

1. a trusted target runs `rdog daemon`
2. an agent (Codex, Claude, GPT, openai-compatible client, MCP, or a human operator) runs `rdog control TARGET @ping @capabilities#1 ...`
3. each trailing `@<line>` is executed in order on one shared control connection
4. the agent parses `@response`, `@savefile`, or `@pty-*` frames

This skill is **agent-agnostic**. The same `rdog control TARGET '@observe#1:...'` one-shot invocation works the same whether the caller is a Codex native subagent, a Claude tool-call bridge, a local Python script driven by an openai-compatible LLM, or a human running it from zsh.
The old stdin / heredoc / pipeline session form is still supported for long generated batches or deliberate bare-shell compatibility, but it is no longer the primary example shape.

Use this skill when the user asks to control a named machine such as `mac.lab`, `win11.lab`, `linux-build.lab`, `mini-a.lab`, a hardware bridge host, or a microcontroller reachable through a bridge/Zenoh/serial setup.

## First Checks

- Prefer the installed `rdog` binary. Inside the rustdog repo, prefer `./target/debug/rdog` when it already exists.
- Verify live syntax with `rdog --help`, `rdog control --help`, and `rdog daemon --help` if command shape matters.
- **如果 daemon 跟 agent 在同一台机器上,优先用本机 fast path**:`rdog control @ping` 或 `rdog control self @ping`
  省略 `TARGET` 写法,客户端扫 `$TMPDIR/rdog-{ns}-*.pipe_uplink` 找唯一 daemon。
  Zenoh `transport_unixpipe` 让本机 round-trip 降到 ~20ms(对比跨 socket ~200ms+)。
  多个 daemon 同机时仍要显式 `rdog control <name> @ping`。详细见 "Local Shortcut Forms"。
- Start with `@ping` for a minimal non-GUI liveness check.
- For fresh GUI or platform-sensitive work, prefer one read-only `@bootstrap#id:{mode:"gui"}` request before acting. Treat `status:"permission_denied"` as a stop-and-explain lane unless the user explicitly asks to change permissions.
- If the target daemon does not support `@bootstrap`, fall back to one trailing one-shot invocation containing `@ping`, `@capabilities`, then `@observe` with screenshot / AX / windows.
  This keeps old daemons usable while making new daemons return one structured `rdog.bootstrap.v1` preflight.
- Use request ids for programmatic calls: `@cmd#1:"printf READY"`.
- Treat `@cmd`, `@script`, and bare shell lines as remote code execution. Use them only on trusted targets.
- Do not assume bare shell lines keep cwd, env, shell variables, or session state. Use PTY for stateful interaction.

## Decision Flow

0. **Are you on the same machine as the daemon you want to control?**
   Skip `TARGET` and use the **local fast path** shortcut:

   ```bash
   # 显式 `self` 关键字(本机 fast path,需要先确定本机只有 1 个 daemon)
   rdog control self @ping
   rdog control self @ping @capabilities#1 @observe#3
   rdog control self --namespace lab @ping        # 显式 namespace 时也走 `self` 路径

   # 空 target + --namespace(隐式本机 fast path,跟 `self` 等价)
   rdog control --namespace lab @ping
   rdog control --namespace lab @ping @capabilities#1

   # 完全省略 target 名(短到极致,等价于 `rdog control self @<line>`)
   rdog control @ping
   rdog control @ping @capabilities#1 @observe#3
   ```

   客户端会扫 `$TMPDIR/rdog-{ns}-*.pipe_uplink` 找唯一 daemon:
   - **找到 0 个** → 报 `未找到本地 daemon`,提示启动 daemon 或显式指定 target name
   - **找到 1 个** → 等价于 `rdog control <name> @<line>`,Zenoh 走 unixpipe 本机 link(2~5x 加速)
   - **找到 ≥2 个** → 报 `本机发现多个 unixpipe daemon: [a, b]`,要求显式指定 target name

   适用场景:AGI agent 跟 daemon 同机跑、agent 知道"我只需要连本机那一个"、批量 GUI 任务想省掉每个 command 都打 target name 的输入成本。
   不支持 PTY 长时间会话(one-shot 多 line 支持,单 session 串行发)。

   如果本机有多个 daemon(不同 namespace),`rdog control self @ping` 会因为找不到唯一匹配而报错——这时必须显式 `rdog control <specific-name> @ping`。
   `rdog_macos.toml` / `rdog_linux.toml` 默认已经开启 `[zenoh.unixpipe] enabled = true`,无需额外配置。

1. Need a quick host check:
   `rdog control TARGET @ping`
   For multiple lines, append more `@<line>`:
   `rdog control TARGET @ping @capabilities#1 @observe#3`
   (shared connection, ordered, fail-fast on first error)
2. Need to know whether GUI, screenshot, AX, mouse, PTY, savefile, or Zenoh session paths are usable:
   `rdog control TARGET @capabilities#1`
   Read `capabilities.*.status`, `error_code`, `permissions`, and `failure_hints`.
   `permission_denied` maps to code `77`; `unsupported` maps to code `78`.
   Do not guess macOS Accessibility, macOS Screen Recording, Windows UIPI, or Linux display backend state from the OS name alone.
3. Need a fast GUI bootstrap before choosing a lane:
   prefer the productized read-only bootstrap:
   ```bash
   rdog control TARGET '@bootstrap#1:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}'
   ```
   (use single quotes around the line to keep the shell from interpreting `{` / `:` / `"`)
   Parse `rdog.bootstrap.v1` lanes: `liveness`, `capabilities`, `observation`, `lanes`, `errors`, and optional `trace`.
   `capability_policy:"cached"` is reserved and currently returns `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`; use `fresh`.
   For older daemons, use one trailing one-shot invocation:
   `rdog control TARGET @ping#1 @capabilities#2 '@observe#3:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}'`.
   If daemon screenshot is permission-denied but AX is available, keep AX/window evidence and use another explicitly stated visual source if the local environment permits it.
4. Need GUI observation before choosing a lane:
   prefer `@observe#id:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}`.
   If `@observe` is unavailable, fall back to `@screenshot include_ax`, `@ax-tree`, `@window-find`, `@ax-find`, or `@ax-get`.
   `@observe` is read-only. It does not activate windows, press controls, type text, scroll, or move the mouse.
5. Need deterministic one-shot automation:
   `@cmd#id:"COMMAND"` or a bare shell line.
6. Need a window that might be hidden, minimized, occluded, or in another desktop state:
   start with `@window-find`, then explicitly `@window-activate`, then do input or AX actions.
   Default close should use `@window-close` without strategy.
   Only use `strategy:"terminate"` or `strategy:"kill"` when the user clearly wants escalation.
7. Need GUI or desktop side effects on a window that is already interactable:
   `@key`, `@paste`, semantic AX/window commands, then mouse fallback by ref or coordinate, then `@screenshot` for evidence.
   Bare `@paste` means focus-based system paste (`Cmd+V` on macOS, `Ctrl+V` on Windows/Linux).
   Use it only when the remote foreground focus is already correct.
   Bare `@screenshot` returns a virtual-desktop JPEG plus a manifest JSON.
   Prefer `@click:{target:{ref:"@e4",observation_id:"obs-..."}}`,
   `@drag:{from:{ref:"@e1",observation_id:"obs-..."},to:{x:900,y:520}}`,
   `@wheel:{target:{ref:"@e8",observation_id:"obs-..."},delta_y:-3}`,
   and `@mouse-move:{target:{ref:"@e9",observation_id:"obs-..."}}` before deriving raw coordinates.
   Read the manifest before deriving mouse coordinates.
   Absolute mouse commands use the same `coordinate_space:"os-logical"` contract.
   Coordinate payloads remain valid, but they are explicit `coordinate_fallback`.
   Selector mouse targets default to no action; `auto_refind:false` returns a recovery `@selector-refind` command, and `auto_refind:true` executes only after typed selector re-find rebounds and verifies a fresh rect.
   A safe no-op mouse smoke is `@mouse-move#id:{dx:0,dy:0,coordinate_space:"relative"}`.
   Raw `@mouse-button mode:"press"` does not auto-release; recover with the matching `mode:"release"`.
8. Need macOS UI structure or semantic button/menu activation:
   start with a token-friendly AX summary, not a full tree:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_mode:"interactive"}`.
   If you only need window inventory, use:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_mode:"windows"}`.
   Equivalent explicit low-token forms are:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_depth:2,ax_max_elements:200,ax_include_values:false}`
   and
   `@screenshot#id:{include_ax:true,ax_required:false,ax_depth:1,ax_max_elements:80,ax_include_values:false}`.
   For active browser page content, prefer the read-only helper first:
   `@web-find#id:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`.
   It searches inside the active `AXWebArea`, excludes browser chrome, and does not perform actions.
   If multiple browser windows make `target:{browser:"active"}` ambiguous, first use `@window-find` or `@observe` to get the intended `window_id`, then scope the same query:
   `@web-find#id:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`.
   If you have a fresh window observation ref instead, use:
   `@web-find#id:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}`.
   `window_ref` is short-lived and must come from the same daemon's current observation store. It is not a durable selector.
   Window-scoped `@web-find` is still read-only. It does not activate or focus the window.
   When the user explicitly wants a simple page-content press, use:
   `@web-act#id:{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true}`.
   It executes only a unique `AXPress` match, re-finds once on stale-like target errors, verifies with a fresh AXWebArea subtree or AX snapshot, and does not use mouse fallback.
   The same `target.window_id` shape works for `@web-act` when side effects are intended:
   `@web-act#id:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},action:"press",verify:true}`.
   `target.window_ref + observation_id` also works for `@web-act`, but only when the user intends side effects.
   For page-changing tasks where the user cares about visible content, verify with a fresh screenshot or screenshot diff before calling the task successful.
   For repeated page-content clicks after `@web-find` has returned a stable page-owned AX id, direct `@ax-action` on that id is the fastest semantic path; if it returns stale/not found, re-run `@web-find` and refresh the cached id.
   For feed-changing pages, treat `performed:true` as action evidence only; require before/after visual evidence such as a cropped screenshot diff.
   To inspect the current computer-use density baseline without touching the live GUI, use:
   `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}`.
   It runs the built-in fixture runner and returns `rdog.gui-bench.v1` metrics; `dense_target_passed:false` is expected for the low-level baseline.
   Use `variant:"all"` to compare `baseline-low-level`, `dense-web-find`, and `dense-web-act`.
   Add `write_artifact:true` only when you explicitly want a JSON file under `target/rdog-bench/`.
   Use live replay only when real GUI side effects are intended:
   `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"live",allow_side_effects:true}`.
   `runner:"live"` rejects `variant:"all"` and records `runs[].live_replay`; for `dense-web-act`, require both `performed:true` and `verified:true` before calling it passed.
   Use `@ax-find#id:{role:"AXButton",name_contains:"Cancel",limit:20}` to get a compact match list,
   use `@ax-get#id:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}` to drill into one element,
   use `@ax-tree#id:{mode:"interactive"}` to read AX structure without a screenshot,
   use `@ax-action#id:{target:{id:"pid:123/window:0/path:3.2"},action:"AXPress"}` for explicit semantic action,
   keep `@ax-press#id:{target:{id:"pid:123/window:0/path:3.2"}}` as compatibility AXPress shorthand,
   use `@ax-set-value#id:{target:{ref:"@e8",observation_id:"obs-..."},value:"hello",mode:"replace"}` for settable text fields,
   use `@ax-focus#id:{target:{ref:"@e8",observation_id:"obs-..."},activate:true}` or `@ax-focus#id:{window_id:"pid:123/window:0",activate:true}` when a hidden/minimized/occluded window must first become interactable,
   use `@ax-scroll#id:{target:{ref:"@e9",observation_id:"obs-..."},direction:"down",pages:2}` for non-mouse scrolling anchored by an AX locator,
   use `@key#id:{key:"Return",delivery:"pid-targeted",pid:556}` or `@key#id:{key:"Cmd+W",delivery:"window-targeted",window_id:"pid:556/window:0"}` for hotkeys, function keys, navigation keys, or app feature triggers,
   and use `@type-text#id:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"ax-value"}` / `mode:"targeted-keyboard"` / `mode:"clipboard",allow_clipboard:true` when you want plain text entry without moving the real mouse.
   Prefer ids or observation refs from the latest manifest/tree. Semantic locators are allowed but must not be ambiguous.
   Preferred non-mouse order is:
   `@ax-find/@ax-get -> @ax-action or @ax-set-value/@type-text -> mouse only as explicit fallback`.
   Short refs like `@e8` live inside one observation only. If the daemon says `OBSERVATION_EXPIRED` or `STALE_REF`, prefer the durable selector workflow when the error payload provides one; otherwise re-run `@ax-find`, `@ax-tree`, `@window-find`, or `@screenshot include_ax` before trying again.
9. Need a real terminal, TUI, shell state, `Ctrl-C`, or `Ctrl-D`:
   `rdog control TARGET --pty -- COMMAND`.
10. Need to control hardware or a microcontroller:
   control the bridge host with `rdog control`, then run the bridge's serial, flashing, SDK, or device CLI from that host. Do not assume rdog can magically execute code inside MCU firmware unless that firmware exposes a compatible control path.
11. Need direct app integration instead of spawning `rdog control`:
   read `references/zenoh-hardware.md` and use the session-channel model.

Compatibility note:
stdin, heredoc, and pipeline input still work, for example when a script generates a large batch or when you intentionally need raw bare shell lines.
Treat pipeline input as a compatibility path only; for smoke tests and short batches, prefer `rdog control TARGET @ping` or `rdog control TARGET @a @b @c`.

## Local Key Chords

Outside a `@pty` session, `@key` is a **local** control action that delivers
keystrokes to the focused app via enigo. It supports chord syntax: separate
the modifier key(s) and the main key with `+`.

```bash
# Single key
rdog control '@key:"F11"'
rdog control '@key:"Return"'

# Two-key chord (most common in browser / editor shortcuts)
rdog control '@key:"Cmd+R"'            # refresh the active browser page
rdog control '@key:"Cmd+Shift+R"'      # hard refresh (bypass cache)
rdog control '@key:"Cmd+L"'            # focus URL bar
rdog control '@key:"Cmd+T"'            # new tab
rdog control '@key:"Cmd+W"'            # close tab
rdog control '@key:"Cmd+,"'            # open Settings
rdog control '@key:"Alt+F4"'           # close window (Windows)
rdog control '@key:"Ctrl+Shift+P"'     # VSCode command palette

# Full object form (hold_ms, mode, delivery are still available)
rdog control '@key#7:{key:"Cmd+R",hold_ms:80,mode:"press_release"}'
```

**Modifier aliases** (all case-insensitive):

| Form | Aliases |
|---|---|
| Cmd | `cmd` / `command` / `meta` / `super` |
| Alt | `alt` / `option` |
| Ctrl | `ctrl` / `control` |
| Shift | `shift` |
| Side-specific (macOS) | `left-cmd` / `right-cmd` / `left-alt` / `right-alt` |

**Main key options:** named keys (`F1`–`F12`, `Return`, `Tab`, `Space`,
`Esc`, `Backspace`, `Delete`, `Home` / `End` / `PageUp` / `PageDown`, arrow
keys) or any single Unicode character via `Key::Unicode`.

**Inside a `@pty` session**, `@key` is **remote stdin text** instead — the
bytes go to the running program, not the local OS. See
`references/protocol.md` for the full PTY streaming rules.

Verify every chord with a fresh `@observe` / `@screenshot`. Do not assume
the GUI re-rendered just because `@key` returned `@response 0` — SPA
refreshes can keep the same AX tree hash while still changing the feed.

## GUI Agent Recipe

Use this fixed workflow for GUI tasks:

1. Bootstrap: send `@bootstrap#id:{mode:"gui",capability_policy:"fresh"}` when starting a fresh GUI task.
2. Fallback bootstrap: on older daemons, send `@ping`, `@capabilities`, and `@observe` together with one trailing one-shot invocation.
3. `@capabilities`: check screenshot, accessibility, window_control, keyboard_input, mouse_input, and type_text.
4. Observe: prefer `@observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}` for extra observation. Existing low-level observation commands remain valid.
5. Locate: for active browser page content use `@web-find`; if the task is an explicit simple press, use `@web-act` instead. Otherwise use `@window-find`, `@ax-find`, then `@ax-get` for one target.
6. Activate/focus: use `@window-activate` or `@ax-focus activate:true` only when the state says the window is not interactable.
7. Semantic action: prefer `@ax-action`, `@ax-set-value`, `@type-text`, `@ax-scroll`, or targeted `@key`.
8. Verify: use a fresh screenshot, AX tree/get, window state, or command output. Do not treat a permission-denied screenshot as visual proof.
8. Fallback recipe: only then use mouse by observation ref, selector-gated recovery, or coordinates from the latest manifest. If fallback is not allowed or capability status is `permission_denied`, return a limited result instead of improvising.

Observation rule:
`@observe`, `@screenshot include_ax`, `@ax-tree`, `@ax-find`, `@ax-get`, and `@window-find` return an `observation` header plus short refs such as `@e1`.
Use follow-on targets as `target:{ref:"@e1",observation_id:"obs-..."}`.
Do not store those refs across daemon restarts, and do not mix `ref` with semantic locators.
`observation.selector_count` reports durable selector records written by the daemon.
If an expired/stale error contains `durable.selector_hint_available:true`, treat `durable.selector_id` as a stable selector, not as a revived short ref.
Inspect it with `@selector-get:{selector_id:"sel-v1-..."}`, then use `@selector-refind:{selector_id:"sel-v1-...",policy:"safe",include_explanations:true}` for recovery decisions.
Use `@selector-resolve:{selector_id:"sel-v1-...",dry_run:true}` only as the lower-level candidate probe.
Only act on the fresh `observation_id` / `ref` returned by `decision:"rebound"` after running the returned `verify_hint`, or by a new observation.
Never treat the old `@eN` as revived.
For mouse fallback, `target:{selector_id:"...",auto_refind:false}` is a no-action handoff.
Use `auto_refind:true` only when an explicit workflow accepts selector-gated mouse fallback.
`blocked`, `not_found`, `needs_disambiguation`, low confidence, and missing rect are all no-action results.

Minimal live evidence chain for observation-ref mouse fallback:

```text
@capabilities#1
@observe#2:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@mouse-move#3:{target:{ref:"@eN",observation_id:"obs-..."}}
@observe#4:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
```

Evidence must show:

- `@capabilities` has usable `screenshot`, `accessibility`, `window_control`, and `mouse_input` statuses, or a first-class permission/unsupported result.
- `@observe` returns `kind:"observe"`, `schema:"rdog.observe.v1"`, an `observation_id`, and a `refs.sample[]` item with `section`, `observation_id`, and `ref`.
- The mouse response reports `target_resolution.source:"observation_ref"` for ref fallback, or `target_resolution.source:"coordinate_fallback"` for raw coordinates.
- The verify step is a fresh observation, screenshot, AX query, window state, or command output. Do not count the mouse response alone as GUI verification.

Selector command roles:

- `@selector-get`: inspect the stable selector and history.
- `@selector-resolve`: dry-run candidate probe; it is read-only and may return ambiguity as an error.
- `@selector-refind`: semantic recovery decision with `scoring_version:"rdog.selector.score.v1"`, confidence, reason codes, `decision`, and recovery recipe.

`@selector-refind` decisions:

- `rebound`: one high-confidence candidate was safely selected. It must include `fresh_target` and `verify_hint`.
- `needs_disambiguation`: candidates exist, but confidence or multiplicity is not safe enough. Do not auto-pick from this list.
- `not_found`: no current candidate. Re-observe or follow `recovery_recipe`.
- `blocked`: permission, backend, capability, or schema blocks recovery. This is a normal response, not an action error, and it must not contain `fresh_target`.

Never treat `fresh_target` as action success. It only means the stable selector has been rebound to a new observation ref.
The required order is:
`@selector-get -> @selector-refind -> verify_hint -> explicit @ax-action/@ax-set-value/@window-activate/...`.
If you skip `verify_hint`, record audit evidence with selector id, fresh target, skip reason, actor or request id, and timestamp.

For GUI launch or deep-link flows, keep launch and observation in separate `rdog control` sessions.
Examples include `open x-apple.systempreferences:...`, launching System Settings, or opening a file/app before inspecting its UI.
Treat `open` as a fire-and-return action, not as proof that the target window and AX tree are settled.
After the launch returns, start a fresh `rdog control TARGET` session and then run `@window-find`, `@ax-*`, or `@screenshot`.
If a session reports `Zenoh session bridge subscriber ... closed before receiving result` immediately after a launch, retry once in a new session before classifying it as permission denied or unsupported.

## Scenario Cookbooks

- Read `references/cookbook-web-content.md` when the user wants to inspect, search, or click controls inside the active browser page, not browser chrome such as tabs, address bar, toolbar buttons, extensions, or bookmarks.
- Future scenario cookbooks can follow the same pattern for apps such as WeChat or Finder, but do not create empty cookbook files before a scenario has verified experience to record.

## Local Fast Path Troubleshooting

`rdog control @<line>` / `rdog control self @<line>` 走本机 fast path 时,常见错误和排查:

- **`未找到本地 daemon`**:
  - 0 个 daemon 在跑。`RUST_LOG=info rdog daemon -c ./rdog_macos.toml` 启一个。
  - daemon 启了但没创建 FIFO。检查 daemon 启动日志有没有 `info: zenoh unixpipe fast path 启用: base=...`。
    没有说明 `[zenoh.unixpipe] enabled = false` 被关了,或者平台是 Windows(强制 disabled)。
  - `$TMPDIR` 不一致。Zenoh unixpipe 用 `$TMPDIR` 写 FIFO,daemon 和 client 必须在同一用户(同一 `$TMPDIR`)。
- **`本机发现多个 unixpipe daemon: [a, b]`**:
  - 多个 daemon 在跑(同 namespace)。用 `ls $TMPDIR/rdog-*.pipe_uplink` 看候选。
  - 显式 `rdog control <name> @<line>` 选一个,或者用 `rdog control --namespace <ns> @<line>` 缩小扫描。
- **`缺少 control 目标`**(old 错误,2026-06-21 之前版本会出):
  - 升级到包含 `d3fdc9b` 之后的 rdog,前置拦截已移除。
- **本机 fast path 后 round-trip 仍 ~200ms**:
  - 检查 client log `info: unixpipe endpoint detected, taking fast path` 是否出现。
    没出现说明 fallback 到了 UDP scout,大概率 FIFO 不存在或同名多个。
  - macOS 上 `sun_path` 限制 104 字节,`{base}_downlink` 9 字节后缀,base 必须 ≤ 95 字节;daemon_name + namespace 过长会被 daemon 启动 fail-fast。
- **PTY 不支持**:
  - `rdog control self --pty -- bash` 会报 "`rdog control self` 不支持 PTY 操作"。
    PTY 需要长 session 复用,跟"短任务一次性执行"语义不符。改用 `rdog control <name> --pty -- bash` 显式 target。

## Reference Loading

- Read `references/control-workflow.md` for exact command forms, target selection, safety, and common host/hardware workflows.
- Read `references/protocol.md` when parsing or generating line-control frames, request ids, error codes, `@savefile`, or PTY lifecycle frames.
- Read `references/zenoh-hardware.md` when working with Zenoh target discovery, serial endpoints, SDK clients, hardware bridge hosts, or microcontroller workflows.

## Safety Boundaries

- Do not use `rdog` against unknown or untrusted targets.
- Ask before destructive or irreversible hardware actions such as flashing firmware, erasing storage, unlocking security state, rebooting production devices, or changing remote OS permissions.
- Permission errors are first-class results. On macOS, `@key` / `@paste` / mouse commands and AX commands need Accessibility permission. `@screenshot` needs Screen Recording permission for the actual `rdog` process. `@screenshot include_ax` can need both.
- `@observe` is read-only. Its visual section still needs Screen Recording permission, and its AX/window sections still depend on Accessibility or the platform backend. It cannot bypass permissions and it never performs an action for you.
- Window control on macOS also needs Accessibility permission for the actual daemon host process, because `@window-find`, `@window-activate`, and graceful `@window-close` read or operate AX window state.
- `include_ax:true,ax_required:false` degrades only AX metadata on Accessibility denial. It does not bypass Screen Recording permission.
- Avoid full AX trees unless necessary. `ax_mode:"full"` or `ax_depth:4,ax_max_elements:1000,ax_include_values:true` can create very large manifests and waste agent context. Prefer `ax_mode:"windows"`, `ax_mode:"interactive"`, `@ax-find`, and `@ax-get`.
- `@key` is meaningful, but treat it as a hotkey/function-key/navigation tool first.
  Do not rely on `@key:"1"` / `@key:"a"` as stable plain text input, because IME state, focus, and app-specific handlers can change the result.
- Bare `@paste` is also a hotkey path, not deterministic text entry.
  It requires the remote focus to be correct and should report `used_hotkey:true` plus `requires_focus:true`.
  Treat `@paste:"text"` as legacy text injection compatibility only; prefer `@ax-set-value` or `@type-text mode:"ax-value"` for normal text entry.
- `@type-text mode:"clipboard"` is opt-in only. It may temporarily overwrite the remote system clipboard, so prefer AXValue or targeted-keyboard first.
  Current macOS behavior restores the previous clipboard only when the clipboard still contains rdog's temporary text.
  Check `clipboard_restore_policy`, `clipboard_restored`, and `clipboard_restore_skipped_reason` in the response before claiming clipboard safety.
- `@type-text mode:"targeted-keyboard"` is still a text-input path.
  It is more targeted than global typing, but it can still be affected by IME state and focus.
- `@ax-scroll` is a semantic AX path on macOS. It should report `delivered_via:"ax-scrollbar-value"` when it scrolls by setting an AXScrollBar value, not pretend it used a global wheel event.
- `@key delivery:"window-targeted"` and `@ax-focus activate:true` may change which remote window becomes active inside the target app. Use them deliberately.
- Treat a macOS desktop-only screenshot as a permission/backend failure, not as reliable visual evidence.
- On Windows, input simulation can be blocked when the target window has higher integrity than the daemon.
