---
name: rdog-control
description: Use when Codex needs to operate rustdog/rdog for remote control of LAN or reachable hosts, hardware bridge machines, lab devices, or microcontrollers. Covers `rdog daemon`, `rdog control`, Zenoh target-name discovery, `--entry-point` fallback, line-control commands like `@ping`, `@bootstrap`, `@capabilities`, `@cmd`, `@key`, `@paste`, `@observe`, `@screenshot`, `@window-find`, `@window-activate`, `@window-close`, `@ax-tree`, `@ax-find`, `@ax-get`, `@ax-press`, `@selector-get`, `@selector-resolve`, `@selector-refind`, `@mouse-move`, `@mouse-button`, `@click`, `@drag`, `@wheel`, `@savefile`, and remote PTY flows such as `rdog control TARGET --pty -- COMMAND`.
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
- Start with `@ping` for a minimal non-GUI liveness check.
- For fresh GUI or platform-sensitive work, prefer one read-only `@bootstrap#id:{mode:"gui"}` request before acting. Treat `status:"permission_denied"` as a stop-and-explain lane unless the user explicitly asks to change permissions.
- If the target daemon does not support `@bootstrap`, fall back to one `rdog control` session containing `@ping`, `@capabilities`, then `@observe` with screenshot / AX / windows.
  This keeps old daemons usable while making new daemons return one structured `rdog.bootstrap.v1` preflight.
- Use request ids for programmatic calls: `@cmd#1:"printf READY"`.
- Treat `@cmd`, `@script`, and bare shell lines as remote code execution. Use them only on trusted targets.
- Do not assume bare shell lines keep cwd, env, shell variables, or session state. Use PTY for stateful interaction.

## Decision Flow

1. Need a quick host check:
   `printf '@ping\n' | rdog control TARGET`
2. Need to know whether GUI, screenshot, AX, mouse, PTY, savefile, or Zenoh session paths are usable:
   prefer a single GUI bootstrap on new daemons:
   ```bash
   printf '@bootstrap#1:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}\n' | rdog control TARGET
   ```
   Parse `rdog.bootstrap.v1` lanes: `liveness`, `capabilities`, `observation`, `lanes`, `errors`, and optional `trace`.
   `capability_policy:"cached"` is reserved and currently returns `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`; use `fresh`.
   For older daemons, send `@ping#1`, `@capabilities#2`, and `@observe#3:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}` in one session.
   For a minimal non-GUI report, `printf '@capabilities#1\n' | rdog control TARGET` remains valid.
   Read `capabilities.*.status`, `error_code`, `permissions`, and `failure_hints`.
   `permission_denied` maps to code `77`; `unsupported` maps to code `78`.
   Do not guess macOS Accessibility, macOS Screen Recording, Windows UIPI, or Linux display backend state from the OS name alone.
3. Need extra GUI observation after bootstrap:
   prefer `@observe#id:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}`.
   If `@observe` is unavailable, fall back to `@screenshot include_ax`, `@ax-tree`, `@window-find`, `@ax-find`, or `@ax-get`.
   `@observe` is read-only. It does not activate windows, press controls, type text, scroll, or move the mouse.
4. Need deterministic one-shot automation:
   `@cmd#id:"COMMAND"` or a bare shell line.
5. Need a window that might be hidden, minimized, occluded, or in another desktop state:
   start with `@window-find`, then explicitly `@window-activate`, then do input or AX actions.
   Default close should use `@window-close` without strategy.
   Only use `strategy:"terminate"` or `strategy:"kill"` when the user clearly wants escalation.
6. Need GUI or desktop side effects on a window that is already interactable:
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
7. Need macOS UI structure or semantic button/menu activation:
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
   use `@ax-set-value#id:{target:{ref:"@e8",observation_id:"obs-..."},value:"hello",mode:"replace"}` for settable text fields,
   use `@ax-focus#id:{target:{ref:"@e8",observation_id:"obs-..."},activate:true}` or `@ax-focus#id:{window_id:"pid:123/window:0",activate:true}` when a hidden/minimized/occluded window must first become interactable,
   use `@ax-scroll#id:{target:{ref:"@e9",observation_id:"obs-..."},direction:"down",pages:2}` for non-mouse scrolling anchored by an AX locator,
   use `@key#id:{key:"Return",delivery:"pid-targeted",pid:556}` or `@key#id:{key:"Cmd+W",delivery:"window-targeted",window_id:"pid:556/window:0"}` for hotkeys, function keys, navigation keys, or app feature triggers,
   and use `@type-text#id:{target:{id:"pid:123/window:0/path:8.2"},text:"hello",mode:"ax-value"}` / `mode:"targeted-keyboard"` / `mode:"clipboard",allow_clipboard:true` when you want plain text entry without moving the real mouse.
   Prefer ids or observation refs from the latest manifest/tree. Semantic locators are allowed but must not be ambiguous.
   Preferred non-mouse order is:
   `@ax-find/@ax-get -> @ax-action or @ax-set-value/@type-text -> mouse only as explicit fallback`.
   Short refs like `@e8` live inside one observation only. If the daemon says `OBSERVATION_EXPIRED` or `STALE_REF`, prefer the durable selector workflow when the error payload provides one; otherwise re-run `@ax-find`, `@ax-tree`, `@window-find`, or `@screenshot include_ax` before trying again.
8. Need a real terminal, TUI, shell state, `Ctrl-C`, or `Ctrl-D`:
   `rdog control TARGET --pty -- COMMAND`.
9. Need to control hardware or a microcontroller:
   control the bridge host with `rdog control`, then run the bridge's serial, flashing, SDK, or device CLI from that host. Do not assume rdog can magically execute code inside MCU firmware unless that firmware exposes a compatible control path.
10. Need direct app integration instead of spawning `rdog control`:
   read `references/zenoh-hardware.md` and use the session-channel model.

## GUI Agent Recipe

Use this fixed workflow for GUI tasks:

1. Bootstrap: send `@bootstrap#id:{mode:"gui",capability_policy:"fresh"}` when starting a fresh GUI task.
2. Fallback bootstrap: on older daemons, send `@ping`, `@capabilities`, and `@observe` together in one read-only control session.
3. `@capabilities`: check screenshot, accessibility, window_control, keyboard_input, mouse_input, and type_text.
4. Observe: prefer `@observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}` for extra observation. Existing low-level observation commands remain valid.
5. Locate: use `@window-find`, `@ax-find`, then `@ax-get` for one target.
6. Activate/focus: use `@window-activate` or `@ax-focus activate:true` only when the state says the window is not interactable.
7. Semantic action: prefer `@ax-action`, `@ax-set-value`, `@type-text`, `@ax-scroll`, or targeted `@key`.
8. Verify: use a fresh screenshot, AX tree/get, window state, or command output. Do not treat a permission-denied screenshot as visual proof.
9. Fallback recipe: only then use mouse by observation ref, selector-gated recovery, or coordinates from the latest manifest. If fallback is not allowed or capability status is `permission_denied`, return a limited result instead of improvising.

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
