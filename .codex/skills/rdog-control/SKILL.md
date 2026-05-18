---
name: rdog-control
description: Use when Codex needs to operate rustdog/rdog for remote control of LAN or reachable hosts, hardware bridge machines, lab devices, or microcontrollers. Covers `rdog daemon`, `rdog control`, Zenoh target-name discovery, `--entry-point` fallback, line-control commands like `@ping`, `@cmd`, `@key`, `@paste`, `@screenshot`, `@window-find`, `@window-activate`, `@window-close`, `@ax-tree`, `@ax-find`, `@ax-get`, `@ax-press`, `@mouse-move`, `@mouse-button`, `@click`, `@drag`, `@wheel`, `@savefile`, and remote PTY flows such as `rdog control TARGET --pty -- COMMAND`.
---

# Rdog Control

## Core Contract

Treat `rdog control` as a stdio-friendly remote control bridge, not as SSH.

The normal path is:

1. a trusted target runs `rdog daemon`
2. Codex runs `rdog control TARGET`
3. Codex writes one line-control command per line
4. Codex parses `@response`, `@savefile`, or `@pty-*` frames

Use this skill when the user asks to control a named machine such as `mac.lab`, `win11.lab`, `linux-build.lab`, `mini-a.lab`, a hardware bridge host, or a microcontroller reachable through a bridge/Zenoh/serial setup.

## First Checks

- Prefer the installed `rdog` binary. Inside the rustdog repo, prefer `./target/debug/rdog` when it already exists.
- Verify live syntax with `rdog --help`, `rdog control --help`, and `rdog daemon --help` if command shape matters.
- Start with `@ping` before side-effectful work.
- Use request ids for programmatic calls: `@cmd#1:"printf READY"`.
- Treat `@cmd`, `@script`, and bare shell lines as remote code execution. Use them only on trusted targets.
- Do not assume bare shell lines keep cwd, env, shell variables, or session state. Use PTY for stateful interaction.

## Decision Flow

1. Need a quick host check:
   `printf '@ping\n' | rdog control TARGET`
2. Need deterministic one-shot automation:
   `@cmd#id:"COMMAND"` or a bare shell line.
3. Need a window that might be hidden, minimized, occluded, or in another desktop state:
   start with `@window-find`, then explicitly `@window-activate`, then do input or AX actions.
   Default close should use `@window-close` without strategy.
   Only use `strategy:"terminate"` or `strategy:"kill"` when the user clearly wants escalation.
4. Need GUI or desktop side effects on a window that is already interactable:
   `@key`, `@paste`, `@click`, `@drag`, `@wheel`, then `@screenshot` for evidence.
   Bare `@paste` means focus-based system paste (`Cmd+V` on macOS, `Ctrl+V` on Windows/Linux).
   Use it only when the remote foreground focus is already correct.
   Bare `@screenshot` returns a virtual-desktop JPEG plus a manifest JSON.
   Read the manifest before deriving mouse coordinates.
   Absolute mouse commands use the same `coordinate_space:"os-logical"` contract.
   A safe no-op mouse smoke is `@mouse-move#id:{dx:0,dy:0,coordinate_space:"relative"}`.
   Raw `@mouse-button mode:"press"` does not auto-release; recover with the matching `mode:"release"`.
5. Need macOS UI structure or semantic button/menu activation:
   start with a token-friendly AX summary, not a full tree:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_mode:"interactive"}`.
   If you only need window inventory, use:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_mode:"windows"}`.
   Equivalent explicit low-token forms are:
   `@screenshot#id:{include_ax:true,ax_required:false,ax_depth:2,ax_max_elements:200,ax_include_values:false}`
   and
   `@screenshot#id:{include_ax:true,ax_required:false,ax_depth:1,ax_max_elements:80,ax_include_values:false}`.
   Use `@ax-find#id:{role:"AXButton",name_contains:"Cancel",limit:20}` to get a compact match list,
   use `@ax-get#id:{target:{id:"pid:123/window:0/path:3"},depth:2,include_values:false}` to drill into one element,
   use `@ax-tree#id:{mode:"interactive"}` to read AX structure without a screenshot,
   use `@ax-action#id:{target:{id:"pid:123/window:0/path:3.2"},action:"AXPress"}` for explicit semantic action,
   keep `@ax-press#id:{target:{id:"pid:123/window:0/path:3.2"}}` as compatibility AXPress shorthand,
   use `@ax-set-value#id:{target:{id:"pid:123/window:0/path:8.2"},value:"hello",mode:"replace"}` for settable text fields,
   use `@ax-focus#id:{window_id:"pid:123/window:0",activate:true}` when a hidden/minimized/occluded window must first become interactable,
   use `@ax-scroll#id:{target:{id:"pid:123/window:0/path:10.1"},direction:"down",pages:2}` for non-mouse scrolling anchored by an AX locator,
   use `@key#id:{key:"Return",delivery:"pid-targeted",pid:556}` or `@key#id:{key:"Cmd+W",delivery:"window-targeted",window_id:"pid:556/window:0"}` for hotkeys, function keys, navigation keys, or app feature triggers,
   and use `@type-text#id:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"ax-value"}` / `mode:"targeted-keyboard"` / `mode:"clipboard",allow_clipboard:true` when you want plain text entry without moving the real mouse.
   Prefer ids from the latest manifest/tree. Semantic locators are allowed but must not be ambiguous.
   Preferred non-mouse order is:
   `@ax-find/@ax-get -> @ax-action or @ax-set-value/@type-text -> mouse only as explicit fallback`.
6. Need a real terminal, TUI, shell state, `Ctrl-C`, or `Ctrl-D`:
   `rdog control TARGET --pty -- COMMAND`.
7. Need to control hardware or a microcontroller:
   control the bridge host with `rdog control`, then run the bridge's serial, flashing, SDK, or device CLI from that host. Do not assume rdog can magically execute code inside MCU firmware unless that firmware exposes a compatible control path.
8. Need direct app integration instead of spawning `rdog control`:
   read `references/zenoh-hardware.md` and use the session-channel model.

## Reference Loading

- Read `references/control-workflow.md` for exact command forms, target selection, safety, and common host/hardware workflows.
- Read `references/protocol.md` when parsing or generating line-control frames, request ids, error codes, `@savefile`, or PTY lifecycle frames.
- Read `references/zenoh-hardware.md` when working with Zenoh target discovery, serial endpoints, SDK clients, hardware bridge hosts, or microcontroller workflows.

## Safety Boundaries

- Do not use `rdog` against unknown or untrusted targets.
- Ask before destructive or irreversible hardware actions such as flashing firmware, erasing storage, unlocking security state, rebooting production devices, or changing remote OS permissions.
- Permission errors are first-class results. On macOS, `@key` / `@paste` / mouse commands and AX commands need Accessibility permission. `@screenshot` needs Screen Recording permission for the actual `rdog` process. `@screenshot include_ax` can need both.
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
