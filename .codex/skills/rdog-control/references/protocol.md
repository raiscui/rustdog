# Line-Control Protocol Reference

## Input Classification

`rdog control` sends full text lines.

| Input | Meaning |
| --- | --- |
| `@@echo hi` | literal shell line `@echo hi` |
| `@ping` | explicit protocol request |
| `@cmd#42:"printf READY"` | explicit protocol request with request id |
| `printf PLAIN_OK` | bare one-shot shell line |

Explicit request ids apply only to `@...` requests.
Bare shell lines do not have request ids.

## Common Requests

```text
@ping
@ping#1
@cmd:"printf READY"
@cmd#42:"printf READY"
@script:"git status --short"
@paste:"hello"
@key:"F11"
@key#7:{key:"right-control",hold_ms:200,mode:"press_release"}
@screenshot
@screenshot#7
@screenshot#7:{target:"display",display:"all",layout:"composite",coordinate_space:"os-logical",format:"jpeg",quality:75}
@screenshot#8:{target:"display",display:"primary",layout:"single",format:"jpeg",quality:75}
@screenshot#9:{include_ax:true,ax_required:false,ax_mode:"windows"}
@screenshot#10:{include_ax:true,ax_required:false,ax_mode:"interactive"}
@window-find#11:{app:"TextEdit",title_contains:"release-notes",limit:5,include_state:true,include_recipes:true}
@window-activate#12:{window_id:"pid:123/window:0"}
@window-close#13:{window_id:"pid:123/window:0"}
@window-close#14:{window_id:"pid:123/window:0",strategy:"terminate"}
@window-close#15:{window_id:"pid:123/window:0",strategy:"kill"}
@ax-find#20:{role:"AXButton",name_contains:"Cancel",limit:20}
@ax-get#21:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}
@ax-tree#22:{mode:"interactive"}
@ax-press#23:{target:{id:"pid:123/window:0/path:3"}}
@mouse-move#10:{x:1200,y:540,coordinate_space:"os-logical"}
@mouse-move#11:{dx:10,dy:-5,coordinate_space:"relative"}
@mouse-button#12:{button:"left",mode:"press"}
@mouse-button#13:{button:"left",mode:"release"}
@click#14:{x:1200,y:540,button:"left",count:1}
@drag#15:{from:{x:900,y:420},to:{x:1200,y:540},button:"left"}
@wheel#16:{x:1200,y:540,delta_y:-3}
@pty:"codex"
@pty:{cmd:"codex",args:["resume","019e..."],cols:120,rows:40}
@pty-close:{session_id:"..."}
@pty-detach:{session_id:"..."}
@pty-attach:{session_id:"..."}
```

For agents, prefer object payloads when fields matter.
Human shorthand is fine for temporary terminal use.

## Response Shapes

Most requests end with one `@response ...`.
This is a request result, not a signal that `rdog control` should exit.

```text
@response 0
@response "READY"
@response {"id":42,"value":"READY"}
@response {"exit_code":1,"stdout":"","stderr":"..."}
@response {"code":64,"error":"unsupported key"}
@response {"id":99,"code":64,"error":"unsupported key"}
```

Common error codes:

| Code | Meaning |
| --- | --- |
| `64` | request is invalid |
| `77` | permission denied |
| `78` | platform/backend unsupported |
| `70` | other server-side execution failure |

When stdin or stdout is not a real TTY, `rdog control` preserves raw protocol lines.
This is the preferred mode for code agents.

## Savefile Results

File-like results can return one or more `@savefile` frames before a final result.

```text
@savefile {"id":7,"filename":"screenshot-123-virtual-desktop.jpg","mime":"image/jpeg","encoding":"base64","data":"..."}
@savefile {"id":7,"filename":"screenshot-123-manifest.json","mime":"application/json","encoding":"base64","data":"..."}
@response {"id":7,"value":{"kind":"screenshot-bundle","image":"screenshot-123-virtual-desktop.jpg","manifest":"screenshot-123-manifest.json","layout":"composite","coordinate_space":"os-logical","display_count":2}}
```

The receiver should decode and save `data`, not dump base64 into the user-visible answer.
The CLI stores downloaded files under `./rdog_downloads/`.

## Screenshot

Default screenshot request:

```text
@screenshot#7
```

Default values:

- `target = "display"`
- `display = "all"`
- `layout = "composite"`
- `coordinate_space = "os-logical"`
- `format = "jpeg"`
- `quality = 75`

The default screenshot captures all active displays as one virtual-desktop JPEG.
It also returns a manifest JSON file.
The manifest is the coordinate source of truth for later mouse work:

```text
os_x = image_x + virtual_bounds.x
os_y = image_y + virtual_bounds.y
image_x = os_x - virtual_bounds.x
image_y = os_y - virtual_bounds.y
```

Use the explicit compatibility path when only the primary display is needed:

```text
@screenshot#8:{target:"display",display:"primary",layout:"single",format:"jpeg",quality:75}
```

Permission failure should be treated as a real result:

```text
@response {"id":7,"code":77,"error":"screen capture permission denied"}
```

## Window Control

Window control is the structured state-and-lifecycle layer for windows that may not be visible in the screenshot.

Common requests:

```text
@window-find#11:{app:"TextEdit",title_contains:"release-notes",limit:5,include_state:true,include_recipes:true}
@window-activate#12:{window_id:"pid:123/window:0"}
@window-close#13:{window_id:"pid:123/window:0"}
@window-close#14:{window_id:"pid:123/window:0",strategy:"terminate"}
@window-close#15:{window_id:"pid:123/window:0",strategy:"kill"}
```

`@window-find` returns:

- `kind:"window-find"`
- `schema:"rdog.window.v1"`
- `snapshot_id`
- `observed_at_unix_ms`
- `matches[].window_id`
- `matches[].state`
- `matches[].recipes`

Current `window_id` shape:

```text
pid:<pid>/window:<index>
```

This locator is short-lived. Prefer a fresh `@window-find` before side-effectful actions.

`matches[].state` contains:

- `occluded`
- `minimized`
- `app_hidden`
- `current_space`
- `fullscreen_space`
- `interactable`
- `confidence`

Phase 1 rules:

- `@window-activate` is the explicit path that may unhide, unminimize, activate, raise, or attempt a Space hop
- ordinary `@click` / `@key` do not auto-activate
- default `@window-close` is graceful
- `strategy:"terminate"` and `strategy:"kill"` are explicit escalation only
- ambiguous query targets return a structured code `64` error by default

Typical agent flow:

```text
@window-find -> @window-activate -> @click / @key / @ax-press / @window-close
```

## AX Metadata And Semantic UI Control

For macOS UI structure, start with the smallest request that can answer the question.
Do not ask for the full AX tree unless you really need it.

Window inventory:

```text
@screenshot#201:{include_ax:true,ax_required:false,ax_mode:"windows"}
@screenshot#201:{include_ax:true,ax_required:false,ax_depth:1,ax_max_elements:80,ax_include_values:false}
```

Interactive controls:

```text
@screenshot#202:{include_ax:true,ax_required:false,ax_mode:"interactive"}
@screenshot#202:{include_ax:true,ax_required:false,ax_depth:2,ax_max_elements:200,ax_include_values:false}
```

Find first, then drill into one target:

```text
@ax-find#301:{role:"AXButton",name_contains:"Cancel",limit:20}
@ax-find#302:{action:"AXPress",mode:"interactive",limit:30}
@ax-get#303:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}
@ax-press#304:{target:{id:"pid:123/window:0/path:3"}}
```

`ax_mode:"windows"` means `ax_depth:1,ax_max_elements:80,ax_include_values:false`.
`ax_mode:"interactive"` means `ax_depth:2,ax_max_elements:200,ax_include_values:false`.
`ax_mode:"full"` keeps the full-tree default and can be large: `ax_depth:4,ax_max_elements:1000,ax_include_values:true`.

Use `ax_mode` on `@screenshot`.
Use `mode` on AX-only commands such as `@ax-tree`, `@ax-find`, and `@ax-get`.
All AX rectangles use `coordinate_space:"os-logical"`, same as the screenshot manifest.

AX permission failure uses the same first-class error style:

```text
@response {"id":301,"code":77,"error":"macOS Accessibility permission denied"}
```

## Mouse Control

Mouse commands reuse the screenshot manifest coordinate contract.
For absolute desktop actions, use `coordinate_space:"os-logical"` and convert from the composite screenshot manifest:

```text
os_x = image_x + virtual_bounds.x
os_y = image_y + virtual_bounds.y
```

Common requests:

```text
@mouse-move#10:{x:1200,y:540,coordinate_space:"os-logical"}
@mouse-move#11:{dx:0,dy:0,coordinate_space:"relative"}
@mouse-button#12:{button:"left",mode:"press"}
@mouse-button#13:{button:"left",mode:"release"}
@click#14:{x:1200,y:540,button:"left",count:1,hold_ms:80}
@drag#15:{from:{x:900,y:420},to:{x:1200,y:540},button:"left",duration_ms:450,steps:24}
@wheel#16:{x:1200,y:540,delta_y:-3}
```

Rules for agents:

- Read the screenshot manifest before sending absolute `@click`, `@drag`, or positioned `@wheel`.
- Reject points in display gaps or outside the manifest bounds before sending the command.
- Use `@mouse-move#id:{dx:0,dy:0,coordinate_space:"relative"}` as the safest real control-path smoke.
- `@mouse-button mode:"press"` intentionally leaves the button down.
  Always send the matching `mode:"release"` during recovery.

Successful mouse actions return structured metadata:

```text
@response {"id":10,"value":{"kind":"mouse","action":"move","backend":"enigo","status":"ok","coordinate_space":"os-logical","x":1200,"y":540}}
```

## PTY Frames

PTY output is not carried by `@response`.
It uses lifecycle and byte-stream frames:

```text
@pty-ready {"session_id":"...","cols":120,"rows":40}
@pty-output {"session_id":"...","encoding":"base64","data":"..."}
@pty-exit {"session_id":"...","exit_code":0,"reason":"process_exit"}
@pty-closed {"session_id":"...","reason":"force_close"}
@pty-detached {"session_id":"...","reason":"owner_detach"}
@pty-attached {"session_id":"...","control_session_id":"..."}
```

Only `@pty-exit` and `@pty-closed` are terminal completion frames.
`@pty-detached` and `@pty-attached` are ownership changes, not process completion.

During PTY streaming, input bytes are transparent:

- `@key` is remote stdin text, not a local control action
- `@script` is remote stdin text
- `~.` is not intercepted
- `Ctrl-C` and `Ctrl-D` go to the remote PTY program

Use another control request or CLI lifecycle option for close/detach/attach.
