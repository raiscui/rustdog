---
name: rdog-control
version: "1.4"  # 2026-06-26: @window-resize 作为固定窗口尺寸的高密度动作,默认恢复/激活目标窗口; @window-activate 弱化为备用恢复能力
description: Use when an AI agent, automation tool, or operator needs to control a local or named machine through rustdog/rdog. Covers the local fast path `rdog control @ping`, named-target one-shot commands, GUI bootstrap/observe, shell/PTY control, AX/web helpers, hardware bridges, and when to load deeper references.
---

# Rdog Control

## Core Contract

`rdog control` is a stdio-friendly remote control bridge, not SSH.
Use it to send ordered line-control frames to a trusted `rdog daemon`, then parse frames such as `@response`, `@savefile`, `@pty-*`, or structured JSON payloads.

This skill is agent-agnostic. It applies to Codex, Claude, GPT, openai-compatible clients, MCP agents, scripts, and human operators.

## Fast Path First

When the agent and daemon are on the same machine, prefer the local fast path:

```bash
rdog control @ping
rdog control @ping @capabilities#1
rdog control @ping @capabilities#1 @observe#3
```

Use `self` or `--namespace` only when that is clearer:

```bash
rdog control self @ping
rdog control self --namespace lab @ping
rdog control --namespace lab @ping
```

Use a named target only when the user names one, multiple daemons exist, or the target is remote:

```bash
rdog control TARGET @ping
rdog control TARGET @ping @capabilities#1 @observe#3
```

Local fast path behavior:

- local-default registry present and valid -> use that daemon, even if extra FIFO candidates exist.
- no valid registry and 0 local daemon FIFOs -> report that no local daemon was found.
- no valid registry and 1 local daemon FIFO -> use it through the Zenoh unixpipe fast path.
- no valid registry and 2+ local daemon FIFOs -> ask for or use the explicit daemon name / namespace, or start the intended daemon with `[zenoh.unixpipe] local_default = true`.

If command shape matters, verify live syntax with:

```bash
rdog --help
rdog control --help
rdog daemon --help
```

## Agent Tool-Use Rules

When the user asks you to execute rdog, use the available shell / bash tool.
Do not invent stdout. Do not claim success until the command output contains the expected frame.

Minimal liveness check:

```bash
rdog control @ping
```

Expected success frame:

```text
@response "pong"
```

For one-shot batches, append more `@<line>` frames to one command so they share one ordered connection:

```bash
rdog control @ping @capabilities#1
rdog control TARGET @ping @capabilities#1 @observe#3
```

If the command output contains ANSI-colored `info:` lines, keep them as diagnostic context, but parse the final `@response` / structured frame for the answer.
Avoid piping rdog output through `jq`, `grep`, `sed`, `awk`, `head`, or `tail` unless the user explicitly asks, because ANSI escapes can break downstream parsing.

## Decision Flow

Use the smallest safe lane:

1. **Same-machine daemon** -> start with `rdog control @ping`.
2. **Named or remote daemon** -> start with `rdog control TARGET @ping`.
3. **Need capability state** -> add `@capabilities#1` and read status / permissions / error codes.
4. **Fresh GUI task** -> prefer read-only bootstrap:

   ```bash
   rdog control TARGET '@bootstrap#1:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}'
   ```

   If the daemon is old, fall back to `@ping`, `@capabilities`, and `@observe` in one one-shot command.
5. **Shell command** -> use `@cmd#id:"COMMAND"` for deterministic one-shot work.
6. **Stateful terminal / TUI / Ctrl-C / Ctrl-D** -> use `rdog control TARGET --pty -- COMMAND`.
7. **Browser page content** -> read `references/cookbook-web-content.md`, prefer `@web-find` for read-only search and `@web-act` only when side effects are intended.
8. **AX / window / mouse GUI work** -> first observe, then use fresh refs / selectors. Prefer semantic AX/window actions before mouse coordinates. When a workflow needs a fixed window size, prefer `@window-resize`; it recovers/activates the target window by default.
9. **Hardware or MCU work** -> control the trusted bridge host first, then run serial/flashing/device tools from that host. Do not assume rdog can execute inside firmware unless the firmware exposes a compatible control path.

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

## GUI Safety Mini-Recipe

For GUI tasks, keep this order:

1. `@bootstrap` or `@capabilities` + `@observe`.
2. Confirm permissions and capabilities before acting.
3. Locate with `@window-find`, `@web-find`, `@ax-find`, or `@ax-get`.
4. Use semantic actions (`@web-act`, `@ax-action`, `@ax-set-value`, `@paste`, `@ax-scroll`, targeted `@key`, `@window-resize`) before mouse fallback.
5. Verify with a fresh observation, screenshot, AX query, window state, command output, or `rdog ax-diff` when comparing AX snapshots.

Observation refs such as `@e1` are short-lived.
Only use them with the matching `observation_id`, and re-observe after daemon restarts, stale refs, or `OBSERVATION_EXPIRED`.

For dual-display or multi-display hosts, choose the display before acting.
Use `scope:{display:{...}}` on `@bootstrap` nested observe, `@observe`, `@window-find`, `@ax-find`, and `@web-find`.
Use `guard:{display:{...}}` on mouse fallback commands that have a target point.

```bash
rdog control TARGET '@bootstrap#1:{mode:"gui",observe:{mode:"hybrid",scope:{display:{id:"d2"}},include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}'
rdog control TARGET '@observe#2:{mode:"hybrid",scope:{display:{name_contains:"DELL"}},include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}'
rdog control TARGET '@web-find#3:{text_contains:"Submit",scope:{display:{window_id:"pid:123/window:0"}}}'
rdog control TARGET '@click#4:{target:{ref:"@e12",observation_id:"obs-..."},guard:{display:{id:"d2"}}}'
```

Supported display selectors are `id`, `name_contains`, `contains_point`, `window_id`, and `window_ref + observation_id`.
Do not generate top-level `display_id:"d2"` as a request field.
Do not generate `scope:{display:{ref:"@d2"}}`; `@eN` refs are UI-element refs, not display refs.

For window sizing flows, use `@window-resize` as the high-density action:

```bash
rdog control TARGET '@window-resize#5:{target:{ref:"@e1",observation_id:"obs-..."},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}'
```

`@window-resize` restores/activates the target window by default, then resizes and verifies it in one action report.
Do not add an `activate:true` field.
Use `@window-activate` only when the task is to restore/focus a window without changing its size, or as a manual recovery fallback after a resize report says recovery was limited.

## Reference Loading

Load deeper references only when the task needs them:

- `references/control-workflow.md` -> exact target forms, local shortcut forms, PTY, common host/hardware workflows.
- `references/protocol.md` -> line-control syntax, request ids, response shapes, `@savefile`, screenshots, observe, AX, mouse, PTY frames.
- `references/cookbook-web-content.md` -> active browser page search/action workflows and AX JSON diff verification.
- `references/zenoh-hardware.md` -> Zenoh discovery, SDK clients, serial endpoints, hardware bridge hosts, microcontroller workflows.
- Project spec `specs/rdog-display-scope-control-plan.md` -> canonical multi-display `scope.display` / `guard.display` resolver and negative forms.



## Unattended Autonomous Mode (无人值守模式)

**This skill is often executed in unattended mode by automated agents.** When you are the agent executing this skill, the user is NOT waiting at a terminal to answer questions. You must complete the entire task end-to-end on your own.

Mandatory rules in unattended mode:

1. **Never ask the user to pick among options.** No "do you want A, B, or C?" prompts. No "shall I retry?" questions. No "the current page does not match — what should I do?" pauses. The user has already left.
2. **You decide.** When a sub-step is ambiguous, pick the most reasonable option, document the choice in your final report, and keep moving.
3. **Validation is mandatory, not optional.** Every GUI action (click, type, focus, scroll) and every shell/PTY/observe step must be followed by a verification step that proves the action took effect. If verification fails, retry (see Retry Loop Pattern).
4. **Chain the entire task in one turn.** A multi-step task (bootstrap → ax-find → ax-set-value → ax-action → observe) is a single user turn. Do not stop after one step and "wait for the next user message" — there is no next user message.
5. **Final report, not a question.** When the chain finishes (success or final-failure), emit a concise final report listing: which steps ran, what was verified, what failed, and what was retried. Do not ask "what next?" at the end.

## Post-Action Validation (操作后必验证)

**Never trust that an action succeeded just because the daemon returned 200.** Every action needs an explicit verification step.

| Action | Required verification | Acceptable evidence |
|---|---|---|
| `@cmd` shell run | inspect stdout / stderr / exit code | non-zero exit, expected line in stdout |
| `@observe` AX/screenshot | confirm `observation_id` is fresh (TTL 5 min) and `ax lane: complete` | bootstrap JSON's `lanes.accessibility.complete: true`, `element_count > 0` |
| `@ax-find` element search | confirm `match_count >= 1` and returned refs are non-empty | response `matches: [...]` length matches `returned_count` |
| `@ax-set-value` text input | re-`@ax-get` the same field, confirm the value matches what you wrote | re-fetched `value` field equals intended string |
| `@ax-action` click | re-`@observe`, confirm UI state changed (new element, new ref, new content) | new observation_id shows the post-action UI |
| `@window-find` | confirm `match_count >= 1` and the returned window is the intended one (by title / role) | response `matches[].title` matches expected |
| `@savefile` screenshot | check the returned `path` exists on disk and is a non-empty image file | `ls -la <path>` shows > 0 bytes |
| `@pty` session | read the next frame, confirm it matches the command's expected output | `@pty-*` response has the expected text |

If verification fails, do NOT report success. Enter the Retry Loop.

## Retry Loop Pattern (失败重试范式)

**Every multi-step rdog chain must wrap each step in this if-then three-line retry table.** This is a SkillLens failure-mechanism encoding — without it, the agent either gives up too early or re-tries the same broken command forever.

For each step, define:

| Trigger (one specific failure signal) | First fix (one concrete change) | If still failing (after 1 retry) |
|---|---|---|
| `match_count: 0` on ax-find | drop `name_contains`, use `role + label` instead, or `window-find` first to scope | re-bootstrap with `capability_policy:"fresh"` for new obs_id, then retry |
| `OBSERVATION_EXPIRED` | re-`@observe` immediately to get a new obs_id, do not reuse the old ref | switch target from `@eN` to `@ax-find` with a different attribute |
| `code 77` Screen Recording | verify daemon process is in System Settings → Privacy & Security → Screen Recording | restart daemon via tmux (`tmux kill-session rdog-daemon; tmux new-session -d -s rdog-daemon ...`) |
| `code 70` target_required | add explicit `target_window: "@eN"` to bootstrap | use `@window-find` first to obtain a target, then re-bootstrap |
| `match_count >= 1` but the matched ref is wrong element | re-`@ax-get` the ref, confirm `role` and `label` match intent; if not, narrow the search | switch to coordinate fallback (rare, last resort) |
| `@ax-set-value` succeeded but the field reverts | the field is a custom widget, not a stock text field; try `@ax-action` with `AXPress` first to focus, then re-`@ax-set-value` | drop to `@paste` (clipboard mode) |
| `@cmd` exit 0 but stdout empty | command produced no output, not an error; re-`@cmd` with `--verbose` if available, else accept and continue | report as observation-only result |
| `capability.status: permission_denied` | explain which TCC permission and where to grant it; do NOT auto-grant | abort the chain with a clear explanation |

**Hard cap**: 3 retries per step. After 3 retries, exit the chain with a concise failure report. Do not loop indefinitely. Do not ask the user.

**Do not retry the same broken command.** Each retry must change at least one of: target window, observation_id, keyword, attribute, or fallback strategy. A retry that re-runs the same command with the same args is not a retry — it is a waste of tool calls.

## Do NOT (反例黑名单)

These are common failure modes observed in real runs. Do not do any of these.

1. **Do NOT skip verification.** A response that says "succeeded" is not the same as a verified post-action state. Every action needs its verification step from the table above.
2. **Do NOT ask the user mid-chain.** No "shall I retry?", "which option (a) (b) (c)?", or "the page does not match — what now?" prompts during the chain. The user is not there. Decide and continue.
3. **Do NOT chain multi-step without `@observe` between sub-steps.** Skipping observe to "save time" is a false economy; ref-expiry will hit you later and the retry will cost more than the saved observe.
4. **Do NOT reuse a ref across daemon restarts.** A ref from `obs-A` is invalid after the daemon restarts. Re-observe.
5. **Do NOT pipe rdog output through `jq` / `grep` / `head` / `tail` / `awk` / `sed` unless the user explicitly asks.** ANSI escapes in the daemon output will break these tools. Parse the `@response` line directly.
6. **Do NOT retry the same command 3 times with the same args.** Change something on each retry (keyword, target, attribute, fallback strategy) or stop.
7. **Do NOT treat a screenshot-only result as visual evidence when the underlying capability is `permission_denied`.** A black screen with `code 77` is not "the desktop is black" — it is "you do not have Screen Recording permission". Report the real cause.
8. **Do NOT ignore `OBSERVATION_EXPIRED`.** Treat it as a hard failure of the entire chain. Re-observe and re-anchor before continuing.
9. **Do NOT fire-and-forget across multi-step chains.** A 5-step chain without verification is 5 opportunities for silent failure. Verify each step.
10. **Do NOT rely on `minimax-M3` high-reasoning to "figure it out".** When the API endpoint is slow or chain-thinks for minutes, the chain does not progress on its own. Either simplify the chain to 1-2 steps per turn, or switch to single-step prompts. The skill must be executable, not just theoretically correct.

## Safety Boundaries

- Use rdog only against trusted targets.
- Ask before destructive or irreversible actions: flashing firmware, erasing storage, rebooting production devices, changing OS permissions, unlocking security state.
- Permission errors are first-class results. Explain them; do not bypass them.
- On macOS, keyboard, mouse, AX, window control, and screenshots require the actual daemon/process to hold the relevant Accessibility or Screen Recording permissions.
- `@observe` is read-only. It cannot activate windows, type, click, scroll, move the mouse, or bypass permissions.
- Avoid full AX trees unless necessary. Prefer compact windows/interactive modes, `@ax-find`, and `@ax-get`.
- Treat screenshot-only desktop images caused by permission/backend failure as insufficient visual evidence.
- Treat `@cmd`, `@script`, bare shell lines, PTY sessions, and hardware bridge commands as remote code execution.
