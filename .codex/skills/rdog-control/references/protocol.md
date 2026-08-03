# Line-Control Protocol Reference

## Input Classification

`rdog control` sends full text lines.

| Input | Meaning |
| --- | --- |
| `@@echo hi` | literal shell line `@echo hi` |
| `@ping` | explicit protocol request |
| `@bootstrap` | read-only liveness + capabilities + optional observe preflight |
| `@capabilities` | structured capability report request |
| `@cmd#42:"printf READY"` | explicit protocol request with request id |
| `printf PLAIN_OK` | bare one-shot shell line |

Explicit request ids apply only to `@...` requests.
Bare shell lines do not have request ids.

## Common Requests

```text
@ping
@ping#1
@bootstrap
@bootstrap#6:{mode:"gui",capability_policy:"fresh",observe:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}}
@capabilities
@capabilities#7
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
@observe#19:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@window-find#11:{app:"TextEdit",title_contains:"release-notes",limit:5,include_state:true,include_recipes:true}
@window-activate#12:{target:{ref:"@e1",observation_id:"obs-123"}}
@window-activate#13:{window_id:"pid:123/window:0"}
@window-resize#18:{target:{ref:"@e1",observation_id:"obs-123"},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}
@window-resize#19:{target:{window_id:"pid:123/window:0"},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}
@window-close#14:{target:{ref:"@e1",observation_id:"obs-123"}}
@window-close#15:{window_id:"pid:123/window:0"}
@window-close#16:{window_id:"pid:123/window:0",strategy:"terminate"}
@window-close#17:{window_id:"pid:123/window:0",strategy:"kill"}
@ax-find#20:{role:"AXButton",name_contains:"Cancel",limit:20}
@ax-get#21:{target:{ref:"@e2",observation_id:"obs-123"},depth:2,include_values:false}
@ax-tree#22:{mode:"interactive"}
@ax-press#23:{target:{ref:"@e2",observation_id:"obs-123"}}
@selector-get#30:{selector_id:"sel-v1-29b3963a312473d5",include_history:true}
@selector-resolve#31:{selector_id:"sel-v1-29b3963a312473d5",limit:10,dry_run:true,include_explanations:true}
@selector-refind#32:{selector_id:"sel-v1-29b3963a312473d5",policy:"safe",min_confidence:0.9,include_explanations:true}
@selector-refind#33:{selector_id:"sel-v1-29b3963a312473d5",source:{observation_id:"obs-old",ref:"@e8"}}
@mouse-move#10:{x:1200,y:540,coordinate_space:"os-logical"}
@mouse-move#11:{dx:10,dy:-5,coordinate_space:"relative"}
@mouse-button#12:{button:"left",mode:"press"}
@mouse-button#13:{button:"left",mode:"release"}
@click#14:{x:1200,y:540,button:"left",count:1}
@click#17:{target:{ref:"@e4",observation_id:"obs-123"},button:"left",count:1}
@click#18:{target:{selector_id:"sel-v1-29b3963a312473d5",auto_refind:false},button:"left"}
@click#19:{target:{selector_id:"sel-v1-29b3963a312473d5",auto_refind:true,policy:"safe",min_confidence:0.9},button:"left"}
@drag#15:{from:{x:900,y:420},to:{x:1200,y:540},button:"left"}
@drag#20:{from:{ref:"@e1",observation_id:"obs-123"},to:{x:1200,y:540},button:"left"}
@wheel#16:{x:1200,y:540,delta_y:-3}
@wheel#21:{target:{ref:"@e8",observation_id:"obs-123"},delta_y:-3}
@mouse-move#22:{target:{ref:"@e9",observation_id:"obs-123"}}
@web-find#40:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@web-find#400:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@web-find#402:{target:{window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@web-act#41:{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true}
@web-act#401:{target:{window_id:"pid:96405/window:3"},match:{text:"首页"},action:"press",verify:true}
@web-act#403:{target:{window_ref:"@e1",observation_id:"obs-123"},match:{text:"首页"},action:"press",verify:true}
@gui-bench#42:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}
@gui-bench#43:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}
@gui-bench#44:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"live",allow_side_effects:true}
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
@response {"id":7,"value":{"kind":"capabilities","schema":"rdog.capabilities.v1",...}}
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

`@capabilities` returns a structured JSON report with:

- `kind:"capabilities"`
- `schema:"rdog.capabilities.v1"`
- `status:"complete"` or `status:"degraded"`
- `platform`
- `capabilities.*.status`
- `capabilities.*.error_code`
- `capabilities.*.permissions`
- `capabilities.*.failure_hints`
- `gui_agent_recipe`

Treat `permission_denied` as an actionable answer, not as a generic failure.

`@bootstrap` returns a structured read-only preflight report:

- `kind:"bootstrap"`
- `schema:"rdog.bootstrap.v1"`
- `status:"complete"`, `status:"degraded"`, or `status:"blocked"`
- `liveness`
- `capability_policy`
- `capabilities`
- `observation`
- `lanes`
- `errors`
- `frames`
- optional `trace`

Use `@bootstrap#id:{mode:"gui",capability_policy:"fresh"}` as the first GUI read when the daemon supports it.
It does not click, type, scroll, focus, activate, or move the mouse.
`capability_policy:"cached"` is reserved for a future TTL cache and currently returns `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`.
All `@bootstrap` requests are Zenoh session-channel-only, including `mode:"basic"`.
For older daemons, fall back to `@ping`, `@capabilities`, and `@observe` in one control session.

`@gui-bench` returns a structured fixture-runner report:

- `kind:"gui-bench"`
- `schema:"rdog.gui-bench.v1"`
- `runner:"fixture"`
- `metrics`
- `thresholds`
- `checks`
- `threshold_failures`
- `steps_summary`
- `runs`
- `dense_target_passed`
- optional `artifact`

Phase 3B supports `suite:"computer-use-density"`, `case:"xhs-left-nav-home"`, and variants `baseline-low-level`, `dense-web-find`, `dense-web-act`, or `all`.
It is read-only and does not touch the live GUI.
For this baseline, `status:"complete"` with `dense_target_passed:false` is expected.
It means the runner completed and the old low-level flow failed the density target.
For `variant:"all"`, compare `runs[]`; top-level `metrics` is omitted because there is no single selected variant.
`write_artifact:true` writes the same report under `target/rdog-bench/`; the default is false.
Phase 3D adds live replay opt-in: `runner:"live"` must be paired with `allow_side_effects:true`.
Live replay rejects `variant:"all"` and only replays one selected dense variant.
For `dense-web-act`, `runs[].live_replay.performed` and `runs[].live_replay.verified` must be true before treating the replay as passing.
The default remains `runner:"fixture"`, which never touches the live GUI.

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

Freshness / stale guard can stop a composite screenshot before any `@savefile` frame.
When this happens, treat the response as a hard visual-evidence failure and keep the payload for diagnosis:

```text
@response {"id":7,"code":70,"kind":"screenshot-stale-frame","error_code":"SCREENSHOT_STALE_FRAME","guard_policy":"reject-consecutive-identical-composite-fingerprint","error":"...","display_count":2,"displays":[...]}
```

This means the daemon captured the same display layout and pixel fingerprint twice in a row.
Do not use older screenshot files as proof after this error.

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

## Observe

`@observe` is the recommended read-only GUI observation entry.
It packages visual, AX, window, refs, selectors, and recovery hints into one `rdog.observe.v1` bundle.
It does not execute any action.

Common forms:

```text
@observe#19
@observe#20:{mode:"window",target:{app:"System Settings"},limit:5}
@observe#21:{mode:"ax",target:{app:"System Settings"},ax_mode:"interactive",ax_required:false}
@observe#22:{mode:"visual",include_screenshot:true,include_manifest:true}
@observe#23:{mode:"hybrid",include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
@observe#24:{mode:"hybrid",scope:{display:{id:"d2"}},include_screenshot:true,include_ax:true,include_windows:true,ax_required:false,ax_mode:"interactive"}
```

Request fields:

- `mode`: `hybrid`, `visual`, `ax`, or `window`. Default is `hybrid`.
- `target`: optional `app`, `bundle_id`, `window_title`, or `window_title_contains`.
- `scope.display`: optional display selector object for multi-display filtering.
- `include_screenshot`: controls the visual screenshot section.
- `include_ax`: controls the accessibility section.
- `ax_required`: when true, AX permission/backend failure makes the request fail.
- `include_windows`: controls the window summary section.
- `include_manifest`: controls whether the visual manifest savefile is returned.
- `include_refs` and `include_selectors`: control summary sections.
- `limit`: caps returned window/ref samples.
- `ax_mode`, `ax_depth`, `ax_max_elements`, and `ax_include_values`: reuse the AX tree budget model.
  `skeleton` is accepted as an alias of the shallow `windows` preset.

Response rules:

- `visual` uses the same composite screenshot producer as `@screenshot`.
  Image and manifest data still arrive as `@savefile` frames before the final `@response`.
- `target` filters window and AX summaries.
  `scope.display` filters window and AX summaries through the shared display resolver.
- With `scope.display`, the visual lane selects the resolved display from the same capture and emits a `single-display` JPEG/manifest.
  The scoped manifest keeps the display's global `os_rect`, starts `image_rect` at `{x:0,y:0}`, and reports `scope_applied:true`.
- Without `scope.display`, visual screenshots remain the all-display `composite` bundle.
- `hybrid` does not create a merged observation namespace.
  The top-level `observation` points at one primary section, and `refs.sample[]` items carry both `section` and `observation_id`.
- `refs.sample[]` is intentionally compact.
  Each item has `section`, `observation_id`, `ref`, `kind`, and optional `name`.
- `selectors.count` reports stable selector records written by the daemon.
  The first response does not inline full selector bodies; use `@selector-get` for the durable record.
- Old observation commands remain valid.
  Use `@screenshot`, `@window-find`, `@ax-tree`, `@ax-find`, and `@ax-get` when you need a narrower lane.

## Display Scope

Display scope is the canonical multi-display filter and guard shape.
Use it when a host has more than one display, when the user names a monitor, or when a coordinate/window should pin the action to one display.

Request examples:

```text
@observe#30:{mode:"hybrid",scope:{display:{id:"d2"}}}
@window-find#31:{title_contains:"Chrome",scope:{display:{name_contains:"DELL"}}}
@ax-find#32:{window:{window_id:"pid:123/window:0"},role:"AXButton",name_contains:"Publish",scope:{display:{contains_point:{x:1800,y:500}}}}
@web-find#33:{text_contains:"Submit",scope:{display:{window_id:"pid:123/window:0"}}}
@web-find#34:{text_contains:"Submit",scope:{display:{window_ref:"@e4",observation_id:"obs-..."}}}
@click#35:{target:{ref:"@e12",observation_id:"obs-..."},guard:{display:{id:"d2"}}}
```

Supported selector fields:

- `id`: display id such as `d1` or `d2`.
- `name_contains`: case-insensitive display name fragment.
- `contains_point`: an `os-logical` point `{x,y}` that must fall inside a display rect.
- `window_id`: resolve to the display with the largest overlap against the window rect.
- `window_ref + observation_id`: resolve a window ref from a fresh observation, then use its window rect.

Rules:

- The request shape is always `scope:{display:{...}}` for reads and `guard:{display:{...}}` for mouse actions.
- Top-level `display_id:"d2"` is rejected as a request field.
- `scope:{display:{ref:"@d2"}}` is rejected; `@eN` refs are observation UI refs, not display refs.
- Responses may include `display_id`, `display_id_stability:"session"`, `stable_key`, and `primary` as resolved identity fields.
- A display selector that matches multiple displays must fail with an ambiguity error instead of falling back to primary.

## Window Control

Window control is the structured state-and-lifecycle layer for windows that may not be visible in the screenshot.

Common requests:

```text
@window-find#11:{app:"TextEdit",title_contains:"release-notes",limit:5,include_state:true,include_recipes:true}
@window-find#16:{title_contains:"Chrome",scope:{display:{id:"d2"}},limit:5}
@window-activate#12:{target:{window_id:"pid:123/window:0"},guard:{display:{id:"d2"}},verify:{focused:true,timeout_ms:2000,poll_interval_ms:50}}
@window-resize#17:{target:{window_id:"pid:123/window:0"},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}
@window-resize#18:{target:{ref:"@e1",observation_id:"obs-..."},size:{width:1200,height:800,unit:"os-logical",box:"outer"},origin:"keep",verify:true}
@window-close#13:{window_id:"pid:123/window:0"}
@window-close#14:{window_id:"pid:123/window:0",strategy:"terminate"}
@window-close#15:{window_id:"pid:123/window:0",strategy:"kill"}
```

`@window-find` returns:

- `kind:"window-find"`
- `schema:"rdog.window.v1"`
- `observation`
- `snapshot_id`
- `observed_at_unix_ms`
- `matches[].window_id`
- `matches[].ref`
- `matches[].state`
- `matches[].recipes`

Current `window_id` shape:

```text
pid:<pid>/window:<index>
```

This locator is short-lived. Prefer a fresh `@window-find` before side-effectful actions.
For follow-on actions, prefer `matches[].ref` together with `observation.observation_id` as `target:{ref:"@e1",observation_id:"obs-..."}`.
If the daemon returns `OBSERVATION_EXPIRED` or `STALE_REF`, re-run `@window-find`.

`matches[].state` contains:

- `occluded`
- `minimized`
- `app_hidden`
- `current_space`
- `fullscreen_space`
- `interactable`
- `confidence`

Phase 1 rules:

- `@window-resize` is the preferred high-density path when the next work needs a fixed window size; it may unhide, unminimize, activate, raise, or attempt a Space hop by default before resizing
- `@window-activate` is the backup explicit path for restoring/focusing a window without changing its size
- ordinary `@click` / `@key` do not auto-activate
- default `@window-close` is graceful
- `strategy:"terminate"` and `strategy:"kill"` are explicit escalation only
- ambiguous query targets return a structured code `64` error by default

Typical agent flow:

```text
@window-find -> @click / @key / @ax-press / @window-close
@window-find -> @window-resize -> @click / @key / @ax-press / @window-close
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
@ax-find#305:{role:"AXButton",name_contains:"Cancel",scope:{display:{id:"d2"}},limit:20}
@ax-find#306:{window:{window_id:"pid:123/window:0"},role:"AXButton",name_contains:"Cancel",limit:20}
@ax-find#307:{window:{ref:"@e1",observation_id:"obs-123"},role:"AXButton",name_contains:"Cancel",limit:20}
@ax-get#303:{target:{ref:"@e2",observation_id:"obs-123"},depth:2,include_values:false}
@ax-press#304:{target:{ref:"@e2",observation_id:"obs-123"}}
```

`ax_mode:"windows"` means `ax_depth:1,ax_max_elements:80,ax_include_values:false`.
`ax_mode:"skeleton"` is accepted as the same shallow preset for `@observe` and AX budget parsing.
`ax_mode:"interactive"` means `ax_depth:2,ax_max_elements:200,ax_include_values:false`.
`ax_mode:"full"` keeps the full-tree default and can be large: `ax_depth:4,ax_max_elements:1000,ax_include_values:true`.

Use `ax_mode` on `@screenshot`.
Use `mode` on AX-only commands such as `@ax-tree`, `@ax-find`, and `@ax-get`.
All AX rectangles use `coordinate_space:"os-logical"`, same as the screenshot manifest.
`@ax-find.window` is optional. When present, it accepts either `window_id` or `ref + observation_id` and captures only that AXWindow subtree before applying display scope and query filters.

For `@ax-focus` with `activate:true`, inspect the nested `activation` report. AX focus is not performed unless `activation.verify.status` is `passed`; activation failure returns `performed:false` and preserves the `WINDOW_*` error code.

AX permission failure uses the same first-class error style:

```text
@response {"id":301,"code":77,"error":"macOS Accessibility permission denied"}
```

Observation refs are short-lived and observation-scoped:

- `@observe`, `@screenshot include_ax`, `@ax-tree`, `@ax-find`, `@ax-get`, and `@window-find` return an `observation` header.
- Their window / element payloads may include `ref:"@eN"`.
- `observation.selector_count` is the number of durable selector records written for that observation.
- Use `target:{ref:"@eN",observation_id:"obs-..."}` for follow-on actions.
- Do not mix `ref` with semantic locators in the same target.
- On `OBSERVATION_EXPIRED` or `STALE_REF`, re-run the observation command instead of guessing.
- If the error payload includes `durable.selector_hint_available:true`, inspect `durable.selector_id` with `@selector-get`, then use `@selector-refind` for a recovery decision.
  The old short ref is still invalid; selector re-find can only return a new fresh ref.
- `@selector-resolve` stays the lower-level dry-run candidate probe. Use it when you want raw candidates, not a recovery decision.

Durable observation state is daemon-owned.
When enabled, the current store uses these files under the configured or platform default observation state dir:

- `meta.json`: durable store identity, privacy, and retention.
- `index.json`: replayable observation / selector index.
- `observations.jsonl`: observation metadata records.
- `selectors.jsonl`: stable selector records.
- `ref_cache.jsonl`: hint-only ref cache.

The ref cache is only a recovery hint.
It must not be treated as proof that an old `@eN` can still be executed.

Selector commands:

```text
@selector-get#401:{selector_id:"sel-v1-29b3963a312473d5"}
@selector-resolve#402:{selector_id:"sel-v1-29b3963a312473d5",limit:10,dry_run:true,include_explanations:true}
@selector-refind#403:{selector_id:"sel-v1-29b3963a312473d5",policy:"safe",min_confidence:0.9,include_explanations:true}
```

`@selector-resolve` is read-only in P2.
If `dry_run:false` is requested, the daemon returns `SELECTOR_ACTION_DEFERRED`.
`@selector-refind` is also read-only, but it returns a P3 recovery decision:

- `decision:"rebound"`: safe re-bind succeeded. The response must include `fresh_target` and `verify_hint`.
- `decision:"needs_disambiguation"`: candidates exist, but the agent must choose after more evidence.
- `decision:"not_found"`: no candidate matched current UI state.
- `decision:"blocked"`: permission, backend, capability, or selector schema blocks recovery. This is a normal selector response, not a side-effect action error.

`fresh_target` only means "use this new observation ref for the next explicit command".
It does not mean pressed, focused, value-set, or action-verified.
Run the returned `verify_hint` before any side-effect command.
If a caller skips verification, it must log audit evidence with selector id, fresh target, skip reason, actor or request id, and timestamp.

P3 scoring contract:

- `scoring_version:"rdog.selector.score.v1"`
- default `min_confidence:0.9`
- `high` means `score >= min_confidence`
- `medium` means `0.65 <= score < min_confidence`
- `low` means `score < 0.65`
- multiple high-confidence candidates still return `needs_disambiguation`
- `blocked` must not contain `fresh_target`

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
@click#17:{target:{ref:"@e4",observation_id:"obs-123"},button:"left",count:1}
@click#23:{target:{ref:"@e4",observation_id:"obs-123"},guard:{display:{id:"d2"}},button:"left",count:1}
@click#18:{target:{selector_id:"sel-v1-29b3963a312473d5",auto_refind:false},button:"left"}
@drag#15:{from:{x:900,y:420},to:{x:1200,y:540},guard:{display:{id:"d2"}},button:"left",duration_ms:450,steps:24}
@drag#20:{from:{ref:"@e1",observation_id:"obs-123"},to:{x:1200,y:540},button:"left"}
@wheel#16:{x:1200,y:540,delta_y:-3}
@wheel#21:{target:{ref:"@e8",observation_id:"obs-123"},delta_y:-3}
@mouse-move#22:{target:{ref:"@e9",observation_id:"obs-123"}}
```

Rules for agents:

- Prefer semantic GUI commands first. Mouse remains a fallback lane.
- Prefer `target:{ref,observation_id}` from the latest observation before manually deriving coordinates.
- Coordinate payloads remain valid, but successful responses mark them as `target_resolution.source:"coordinate_fallback"`.
- Selector mouse targets default to no action. `auto_refind:false` returns `performed:false`, `gate_decision:"handoff_required"`, and a recovery `@selector-refind` command.
- `auto_refind:true` is explicit opt-in. It can execute only when typed selector-refind returns `decision:"rebound"` and the fresh target verifies to a current rect. `blocked`, `not_found`, `needs_disambiguation`, low confidence, or missing rect must stay no-action.
- Read the screenshot manifest before sending absolute `@click`, `@drag`, or positioned `@wheel`.
- Reject points in display gaps or outside the manifest bounds before sending the command.
- On multi-display hosts, use `guard:{display:{...}}` on `@mouse-move`, `@click`, `@drag`, and `@wheel`.
  `@mouse-button` has no target point and does not accept display guard.
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

- Inside a `@pty` session, `@key` and `@script` are streamed as remote stdin text
  (the bytes go to the running program, not to the local OS).
- Outside a `@pty` session, `@key` is a **local** control action that supports key
  chords via `+` syntax, e.g. `@key:"Cmd+R"` triggers a Cmd+R keystroke on the
  local machine. See the "Local Key Chords" section in `SKILL.md` for the
  full modifier / main-key grammar and examples.
- `~.` is not intercepted
- `Ctrl-C` and `Ctrl-D` go to the remote PTY program

Use another control request or CLI lifecycle option for close/detach/attach.
