# Rdog Control Workflow Reference

## Target Forms

Use the shortest target form that matches the deployment.

```bash
# Zenoh target-name shorthand. A single non-port argument is inferred as target name.
rdog control mac.lab

# Zenoh target-name with deterministic router fallback.
rdog control mac.lab --entry-point tcp/192.168.1.20:17447

# Explicit Zenoh form.
rdog control --transport zenoh --target-name mac.lab

# TCP control endpoint.
rdog control 127.0.0.1 5555
rdog control --transport tcp 127.0.0.1 5555

# WebSocket control endpoint.
rdog control --url ws://127.0.0.1:5555/control
```

Current daemon entry points:

```bash
rdog daemon --transport zenoh --name mac.lab --namespace lab
rdog daemon --config ./rdog_macos.toml
rdog daemon --config ./rdog_linux.toml
rdog daemon --config ./rdog_win.toml
```

The common Zenoh config shape is:

```toml
[zenoh]
enabled = true
mode = "router"
namespace = "lab"
daemon_name = "mac.lab"
listen_endpoints = [
  "tcp/0.0.0.0:17447",
]
request_timeout_ms = 3000
startup_guard_window_ms = 1000
```

Durable observation state is daemon-owned.
It stores observation metadata, stable selector records, and hint-only ref cache, but it does not make short `@eN` refs valid after daemon restart:

```toml
[observation]
durable_enabled = true
retention_observations = 256
retention_bytes = 52428800
persist_values = false
persist_screenshots = false
write_ref_cache = true
```

When a stale/expired ref error returns `durable.selector_id`, use the selector workflow before acting:

```text
@selector-get#201:{selector_id:"sel-v1-..."}
@selector-refind#202:{selector_id:"sel-v1-...",policy:"safe",include_explanations:true}
@ax-get#203:{target:{ref:"@e-new",observation_id:"obs-new"},depth:1,include_values:false}
```

`@selector-refind` returns a recovery decision, not an action result.
When it returns `decision:"rebound"`, follow its `verify_hint` and only then send the explicit side-effect command.
When it returns `decision:"needs_disambiguation"`, `decision:"not_found"`, or `decision:"blocked"`, do not auto-pick a candidate and do not fall back to mouse coordinates unless the user or workflow explicitly allows it.
`@selector-resolve` remains available as a lower-level dry-run candidate probe.
Neither command revives the old `@eN`.

## Code Agent Workflow

Start with a non-destructive smoke:

```bash
rdog control mac.lab <<'RDOG'
@ping
@capabilities#100
@cmd#1:"printf READY"
printf PLAIN_OK
@mouse-move#2:{dx:0,dy:0,coordinate_space:"relative"}
RDOG
```

Expected raw programmatic output contains:

```text
@response "pong"
@response {"id":100,"value":{"kind":"capabilities","schema":"rdog.capabilities.v1",...}}
@response {"id":1,"value":"READY"}
@response "PLAIN_OK"
@response {"id":2,"value":{"kind":"mouse","action":"move",...}}
```

Use stable daemon names:

| Target | Typical role | Good first actions |
| --- | --- | --- |
| `mac.lab` | macOS GUI host | `@ping`, `@observe`, `@key`, `@paste`, `@screenshot`, `@click`, `@drag`, `@wheel`, `--pty` |
| `win11.lab` | Windows GUI host | `@ping`, `@observe`, `@key`, `@paste`, `@screenshot`, `@click`, `@drag`, `@wheel` |
| `linux-build.lab` | build/test host | `@cmd#id`, `@script`, `--pty -- /bin/bash` |
| `mini-a.lab` | hardware bridge / experiment node | `@ping`, one-shot shell, device CLI, SDK control |

## Common Tasks

Run a deterministic command:

```bash
printf '@cmd#7:"pwd"\n' | rdog control linux-build.lab
```

Run a sequence:

```bash
rdog control linux-build.lab <<'RDOG'
@ping
@cmd#1:"git status --short"
@cmd#2:"cargo check --quiet"
RDOG
```

Operate a GUI and capture evidence:

```bash
rdog control mac.lab <<'RDOG'
@capabilities#200
@observe#201:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@key#202:{key:"F11",hold_ms:200,mode:"press_release"}
@screenshot#203
RDOG
```

For GUI agent work, use the fixed recipe:

```text
@capabilities -> @observe -> locate -> activate/focus -> semantic action -> verify -> fallback
```

Prefer `@observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}` for the observation step.
If a target does not support it, use the lower-level lanes: `@screenshot include_ax`, `@ax-tree`, `@window-find`, `@ax-find`, or `@ax-get`.
Those older commands are still stable and are not deprecated.

Read these capability entries before choosing the act path:

- `screenshot`: Screen Recording on macOS, display backend on Linux/Windows.
- `accessibility`: AX tree and semantic AX actions.
- `window_control`: hidden/minimized/occluded window recovery.
- `keyboard_input` and `mouse_input`: macOS Accessibility, Windows UIPI, Linux display backend policy.
- `type_text`: AXValue / targeted keyboard / clipboard text delivery.
- `pty`, `savefile_receiver`, and `zenoh_session_channel`: long-running terminal and multi-frame result support.

If an entry is `permission_denied`, stop that lane and explain the missing permission.
If an entry is `unsupported`, choose another lane instead of retrying the same command.

For app launch and deep-link GUI flows, split launch from observation.
Run launch commands such as `open x-apple.systempreferences:...` in one `rdog control` session, then open a fresh session for `@window-find`, `@ax-*`, or `@screenshot`.
This matters because `open` returns after asking LaunchServices to activate an app; it does not guarantee that the window, page, and AX tree are stable.
If the bridge closes with `Zenoh session bridge subscriber ... closed before receiving result` right after a launch, retry once in a new session before treating the lane as unsupported or permission denied.

Prefer observation refs for mouse fallback, then use screenshot coordinates only when needed:

```bash
rdog control mac.lab <<'RDOG'
@observe#10:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@mouse-move#11:{dx:0,dy:0,coordinate_space:"relative"}
@click#12:{target:{ref:"@e4",observation_id:"obs-..."},button:"left",count:1}
@drag#13:{from:{ref:"@e1",observation_id:"obs-..."},to:{x:1200,y:540},button:"left"}
@wheel#14:{target:{ref:"@e8",observation_id:"obs-..."},delta_y:-3}
RDOG
```

For a real GUI smoke, keep the evidence chain explicit:

```text
@observe -> choose refs.sample[] target -> @mouse-move/@click with target.ref -> fresh @observe or @screenshot verify
```

The mouse response should include `target_resolution.source:"observation_ref"`.
If the response says `coordinate_fallback`, then the test covered raw-coordinate fallback, not observation-ref fallback.
If a selector is used, `auto_refind:false` must stop with no action, and `auto_refind:true` must show `gate_decision:"verified_rebound"` before any mouse action is accepted.

Before sending coordinate `@click`, `@drag`, or positioned `@wheel`, parse the manifest from `@screenshot` / `@observe`.
Mouse is a fallback lane, not the default GUI path.
Use it when semantic/ref/selector lanes are unavailable, the target is canvas/free-space/drag-heavy, or the user explicitly asks for real pointer control.
Selector mouse targets are gated:
`auto_refind:false` returns no-action handoff and a recovery `@selector-refind` command.
`auto_refind:true` may execute only when typed refind returns `rebound` and the fresh ref verifies to a current rect.
For the default composite screenshot, convert `image_x/image_y` to OS coordinates by adding `virtual_bounds.x/y`.
Do not click into display gaps.
For raw button flows, `@mouse-button mode:"press"` does not auto-release; send the matching `mode:"release"` if the flow is interrupted.

Discover and recover a non-visible window before clicking:

```bash
rdog control mac.lab <<'RDOG'
@window-find#20:{app:"TextEdit",title_contains:"release-notes",limit:5,include_state:true,include_recipes:true}
@window-activate#21:{window_id:"pid:123/window:0"}
@click#22:{x:1200,y:540,button:"left",count:1}
RDOG
```

Close gently first, escalate only when the user clearly intends it:

```bash
rdog control mac.lab <<'RDOG'
@window-close#30:{window_id:"pid:123/window:0"}
@window-close#31:{window_id:"pid:123/window:0",strategy:"terminate"}
@window-close#32:{window_id:"pid:123/window:0",strategy:"kill"}
RDOG
```

`@window-find` is the right first step when:

- the screenshot does not show the target window
- the app may be hidden or minimized
- the window may be occluded by another app
- the agent needs an honest `limited` result for cross-Space or fullscreen situations

Do not treat ordinary `@click` or `@key` as an implicit window restore path in Phase 1.

Read macOS UI structure without blowing up the agent context:

```bash
rdog control mac.lab <<'RDOG'
@observe#200:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@screenshot#201:{include_ax:true,ax_required:false,ax_mode:"windows"}
@screenshot#202:{include_ax:true,ax_required:false,ax_mode:"interactive"}
@ax-find#203:{role:"AXButton",name_contains:"Cancel",limit:20}
@ax-get#204:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}
@ax-press#205:{target:{id:"pid:123/window:0/path:3"}}
RDOG
```

`@observe` is the recommended first read.
The explicit screenshot and AX commands are narrower lanes for follow-up or compatibility.
Use `ax_mode:"windows"` when you only need window titles and shallow structure.
Use `ax_mode:"interactive"` when you need common buttons, menu items, and text controls.
Use explicit `ax_depth:1,ax_max_elements:80,ax_include_values:false` or `ax_depth:2,ax_max_elements:200,ax_include_values:false` when the agent needs predictable token budgets.

Open a real terminal:

```bash
rdog control linux-build.lab --pty -- /bin/bash
rdog control mac.lab --pty -- codex
rdog control mac.lab --pty -- vim README.md
```

Manage PTY lifecycle:

```bash
rdog control mac.lab --pty-detach <SESSION_ID>
rdog control mac.lab --pty-attach <SESSION_ID>
rdog control mac.lab --pty-close <SESSION_ID>
```

## Hardware And MCU Pattern

Rdog usually controls hardware indirectly through a bridge machine.

Typical chain:

```text
Codex -> rdog control mini-a.lab -> shell/PTY on bridge host -> serial/JTAG/SDK/vendor CLI -> device
```

Examples:

```bash
rdog control mini-a.lab <<'RDOG'
@ping
@cmd#1:"ls /dev/tty* | head"
@cmd#2:"python3 tools/read_sensor.py --port /dev/cu.usbserial-0001"
RDOG
```

For flashing or destructive hardware actions, ask first unless the user explicitly requested that action.

## Known Non-Goals

- `rdog control` is not built-in public Internet NAT traversal.
- Zenoh `--entry-point` is a fallback, not the only normal path.
- Bare shell lines are one-shot; they do not keep shell state.
- Traditional interactive shell over Zenoh requires PTY. Use `--pty`.
- `@key`, `@paste`, mouse commands, and `@screenshot` do not bypass OS permissions.
