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

## Code Agent Workflow

Start with a non-destructive smoke:

```bash
rdog control mac.lab <<'RDOG'
@ping
@cmd#1:"printf READY"
printf PLAIN_OK
@mouse-move#2:{dx:0,dy:0,coordinate_space:"relative"}
RDOG
```

Expected raw programmatic output contains:

```text
@response "pong"
@response {"id":1,"value":"READY"}
@response "PLAIN_OK"
@response {"id":2,"value":{"kind":"mouse","action":"move",...}}
```

Use stable daemon names:

| Target | Typical role | Good first actions |
| --- | --- | --- |
| `mac.lab` | macOS GUI host | `@ping`, `@key`, `@paste`, `@screenshot`, `@click`, `@drag`, `@wheel`, `--pty` |
| `win11.lab` | Windows GUI host | `@ping`, `@key`, `@paste`, `@screenshot`, `@click`, `@drag`, `@wheel` |
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
@key#1:{key:"F11",hold_ms:200,mode:"press_release"}
@screenshot#2
RDOG
```

Use screenshot coordinates for mouse actions:

```bash
rdog control mac.lab <<'RDOG'
@screenshot#10
@mouse-move#11:{dx:0,dy:0,coordinate_space:"relative"}
@click#12:{x:1200,y:540,button:"left",count:1}
@drag#13:{from:{x:900,y:420},to:{x:1200,y:540},button:"left"}
@wheel#14:{x:1200,y:540,delta_y:-3}
RDOG
```

Before sending `@click`, `@drag`, or positioned `@wheel`, parse the manifest from `@screenshot#10`.
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
@screenshot#201:{include_ax:true,ax_required:false,ax_mode:"windows"}
@screenshot#202:{include_ax:true,ax_required:false,ax_mode:"interactive"}
@ax-find#203:{role:"AXButton",name_contains:"Cancel",limit:20}
@ax-get#204:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}
@ax-press#205:{target:{id:"pid:123/window:0/path:3"}}
RDOG
```

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
