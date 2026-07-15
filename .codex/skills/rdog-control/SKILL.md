---
name: rdog-control
version: "1.6"  # 2026-07-14: WeChat内容定位暂时禁用AX,改走window+screenshot+guarded coordinates
description: "Use when controlling a local or named machine through rdog control: target selection, line-control frames, GUI/web/window actions, shell/PTY/@flow, validation, and safety."
---

# Rdog Control

## Contract

`rdog control` is a stdio-friendly control bridge to a trusted `rdog daemon`.
It is not SSH.
Send ordered line-control frames, then read frames such as `@response`, `@savefile`, `@pty-*`, or structured JSON.

This skill is agent-agnostic.
It applies to Codex, Claude, GPT, openai-compatible clients, MCP agents, scripts, and human operators.

Treat `@cmd`, `@script`, bare shell lines, `@flow` shell steps, PTY, and hardware bridge commands as remote code execution on the daemon host.

## Start Here

Prefer the local fast path when the agent and daemon are on the same machine:

```bash
rdog control @ping
rdog control @ping @capabilities#1
rdog control @ping @capabilities#1 @observe#3
```

Use `self` or namespace forms only when they make the target clearer:

```bash
rdog control self @ping
rdog control self --namespace lab @ping
rdog control --namespace lab @ping
```

Use a named target when the user names one, multiple local daemons exist, or the daemon is remote:

```bash
rdog control TARGET @ping
rdog control TARGET @ping @capabilities#1 @observe#3
```

Expected liveness frame:

```text
@response "pong"
```

Local fast path selection:

- valid local-default registry -> use that daemon, even if extra FIFO candidates exist.
- no valid registry and 0 FIFO candidates -> report no local daemon.
- no valid registry and 1 FIFO candidate -> use the Zenoh unixpipe fast path.
- no valid registry and 2+ FIFO candidates -> use an explicit daemon name / namespace.

If command shape matters, verify live syntax:

```bash
rdog --help
rdog control --help
rdog daemon --help
```

Do not invent stdout.
Do not claim success until the real command output contains the expected frame.
Avoid piping rdog output through `jq`, `grep`, `sed`, `awk`, `head`, or `tail` unless the user explicitly asks; ANSI diagnostics can break downstream parsing.

## Choose The Lane

Use the smallest lane that can prove the result:

1. Liveness or health -> `@ping`.
2. Capability or permission state -> `@capabilities#1`.
3. Fresh GUI context -> `@bootstrap` or `@observe`.
4. One daemon-side shell command -> `@cmd#id:"COMMAND"` or `@script`.
5. Finite daemon-side workflow -> `@flow`.
6. Stateful terminal, TUI, Ctrl-C, Ctrl-D -> `rdog control TARGET --pty -- COMMAND`.
7. Browser page content -> `@web-find` first, `@web-act` only when side effects are intended.
8. GUI controls -> prefer semantic AX/window/web actions before mouse coordinates, except for the temporary WeChat policy below.
9. Hardware or MCU work -> control the trusted bridge host, then run serial/flashing/device tools there.

Read-only GUI bootstrap example:

```bash
rdog control TARGET '@bootstrap#1:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}'
```

## WeChat Temporary No-AX Policy

When the target app is WeChat (`com.tencent.xinWeChat`) or the user names "WeChat" / "微信", do not use AX to locate or interact with content controls.
This is a temporary fail-closed policy based on the 2026-07-14 ownership probe.

- Do not call `@ax-find`, `@ax-get`, `@ax-action`, `@ax-set-value`, `@ax-scroll`, or `rdog ax-diff` for WeChat content.
- Do not reuse AX-derived refs or labels such as `发现`, `直播`, or `发布`; a previous fallback attached a Xiaohongshu/Chrome AX tree to WeChat target metadata.
- The restriction applies to WeChat content targeting. `@window-find`, `@window-activate`, screenshot capture, guarded mouse actions, `@paste`, and targeted `@key` remain allowed.
- Use `@window-find` to resolve the current WeChat window, then capture a fresh visual observation with `include_screenshot:true`, `include_windows:true`, and `include_ax:false`.
- Locate visible controls from screenshot pixels plus the resolved window/display geometry. Use `guard.display`, and confirm the point is still inside the fresh window rect before coordinate actions.
- If the window is hidden, occluded, or stale, activate it and capture a new screenshot before deriving coordinates. Do not use AX hit-testing to bypass occlusion.
- Verify every action with a fresh screenshot and window state. If visual ownership or coordinates are ambiguous, stop without clicking or typing.

Do not re-enable WeChat AX targeting until a controlled overlap/z-order regression proves root ownership and a live query reliably finds `文件传输助手` without accepting foreign browser content.

## CLI-Side UI Script

Use `rdog ui-script run [TARGET] script.json` or `rdog control --ui-script script.json [TARGET]` for controller-side JSON UI automation.
Both entries share the same runner, target resolver, trace writer, artifact handling, and `Expect` evaluator.
`--compat iced-emg` and daemon-side GUI-only `@ui-flow` are not implemented.

## Daemon-Side Flow

Use `@flow` when several steps should run on the daemon with one ordered request.
It can mix shell, control lines, expects, artifacts, and trace.

```bash
rdog control TARGET '@flow#9:{"schema":"rdog.flow.v1","policy":{"allow_shell":true},"steps":[{"Cmd":{"run":"echo flow-ok","capture":"cmd1"}},{"Expect":{"kind":"cmd_exit_code","capture":"cmd1","code":0}},{"ControlLine":"@ping"},{"Expect":{"kind":"response_contains","contains":"pong"}},{"Exit":null}]}'
```

Rules:

- `Cmd` and `Script` require `policy.allow_shell:true`.
- `SaveArtifact` reads daemon-local files and requires `policy.allow_file_read:true`.
- `cwd`, `env`, command execution, file reads, and artifact paths are daemon-local.
- Controller-local files must be inlined or uploaded before the daemon can use them.
- `ControlLine` reuses the existing control parser/core.
- inner `@response` frames are consumed into flow state; the outer final response is one flow summary.
- `SaveArtifact`, inner `@savefile`, and trace savefiles return before the final `@response`.
- v1 rejects nested `@flow`, `@pty`, and `ControlLine:"@cmd..."` / `ControlLine:"@script..."`.
- `@ui-flow`, if added later, is a GUI-only profile or alias. It is not the full script runtime.

## GUI Targeting

The WeChat no-AX policy overrides this generic procedure for WeChat content.

For GUI work:

1. Get capability state and an observation.
2. Locate with `@window-find`, `@web-find`, `@ax-find`, or `@ax-get`.
3. Act with semantic commands first: `@web-act`, `@ax-action`, `@ax-set-value`, `@paste`, `@ax-scroll`, targeted `@key`, `@window-resize`.
4. Verify with a fresh observation, AX query, screenshot, window state, command output, or `rdog ax-diff`.

Observation refs such as `@e1` are short-lived.
Use them only with the matching `observation_id`.
After daemon restart, stale refs, or `OBSERVATION_EXPIRED`, observe again and re-anchor.

For multi-display hosts, scope before acting:

```bash
rdog control TARGET '@observe#2:{mode:"hybrid",scope:{display:{name_contains:"DELL"}},include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}'
rdog control TARGET '@click#4:{target:{ref:"@e12",observation_id:"obs-..."},guard:{display:{id:"d2"}}}'
```

Supported display selectors: `id`, `name_contains`, `contains_point`, `window_id`, `window_ref + observation_id`.
Request shape is `scope:{display:{...}}` for observe/find and `guard:{display:{...}}` for mouse fallback.
Do not generate top-level `display_id:"d2"`.
Do not generate `scope:{display:{ref:"@d2"}}`; `@eN` refs are UI-element refs, not display refs.

For fixed window size, use `@window-resize`:

```bash
rdog control TARGET '@window-resize#5:{target:{ref:"@e1",observation_id:"obs-..."},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}'
```

`@window-resize` restores/activates the target window by default, resizes, then verifies.
Do not add `activate:true`.
Use `@window-activate` only to restore/focus without changing size, or as recovery after resize reports limited recovery.

## Keyboard And PTY

Outside a PTY session, `@key` sends local OS keystrokes to the focused app:

```bash
rdog control '@key:"Return"'
rdog control '@key:"Cmd+R"'
rdog control '@key#7:{key:"Cmd+Shift+R",hold_ms:80,mode:"press_release"}'
```

Common modifiers: `Cmd`, `Alt`, `Ctrl`, `Shift`.
Common main keys: `Return`, `Tab`, `Space`, `Esc`, arrows, `F1` to `F12`, or one Unicode character.

Inside `rdog control TARGET --pty -- COMMAND`, `@key` means remote stdin text for the running program, not local OS input.
See `references/protocol.md` for full PTY frame rules.

## Validate And Retry

Every side-effecting action needs a proof step.
A success response is not proof by itself.

| Action | Proof |
|---|---|
| `@cmd` / `@script` / `@flow` | inspect exit code, stdout/stderr, final summary, or expected `@savefile` |
| `@observe` / `@bootstrap` | fresh `observation_id`, expected lanes and element/window counts |
| `@ax-find` / `@web-find` / `@window-find` | `match_count >= 1`, returned refs/titles match intent |
| `@ax-set-value` / `@paste` | re-read value or observe visible state |
| `@ax-action` / `@web-act` / mouse action | fresh observation or AX diff shows the intended change |
| `@window-resize` | response reports verified size or explains bounded failure |
| `@savefile` | returned path exists and is non-empty when saved locally |
| PTY | next PTY frame contains expected output/state |

Retry rules:

- Change something on each retry: observation id, target scope, keyword, attribute, or fallback lane.
- Do not repeat the same broken command three times.
- Hard cap: 3 retries per step.
- `OBSERVATION_EXPIRED` -> observe again immediately.
- `match_count:0` -> loosen or change locator, or scope by window first.
- permission denied -> report the missing permission and stop; do not bypass it.

## Safety Boundaries

- Use rdog only against trusted targets.
- Ask before destructive work: flashing firmware, erasing storage, rebooting production devices, changing OS permissions, unlocking security state.
- Permission errors are first-class results. Explain them.
- On macOS, keyboard, mouse, AX, window control, and screenshots require the daemon process to hold the relevant Accessibility or Screen Recording permissions.
- `@observe` is read-only. It cannot activate windows, type, click, scroll, move the mouse, or bypass permissions.
- Avoid full AX trees unless necessary. Prefer compact observe modes, `@ax-find`, and `@ax-get`.
- Treat screenshot-only desktop images caused by permission/backend failure as insufficient visual evidence.

## References

Load deeper references only when needed:

- `references/control-workflow.md` -> target forms, local shortcuts, PTY, common host/hardware workflows.
- `references/protocol.md` -> line-control syntax, request ids, response shapes, `@savefile`, screenshots, observe, AX, mouse, PTY frames.
- `references/cookbook-web-content.md` -> active browser page search/action workflows and AX JSON diff verification.
- `references/zenoh-hardware.md` -> Zenoh discovery, SDK clients, serial endpoints, hardware bridge hosts, microcontroller workflows.
- `specs/rdog-flow-control-plan.md` -> daemon-side `@flow` schema, runtime, frame, artifact, and safety invariants.
- `specs/rdog-display-scope-control-plan.md` -> canonical `scope.display` / `guard.display` resolver and negative forms.
