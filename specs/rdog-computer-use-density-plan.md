# `rdog` computer-use density plan

## 1. Goal

`rdog` is primarily used by code agents.
For computer-use tasks, the main bottleneck is no longer whether the daemon has enough low-level commands.
The bottleneck is that agents must manually compose too many low-level requests:

```text
@ping -> @capabilities -> @window-find -> @ax-get -> @ax-get -> @ax-action -> verify
```

This plan defines high-density primitives and a benchmark suite so common GUI / web tasks can finish in 1-2 backend requests without hiding unsafe guesses.

The first target is browser page content, because the Xiaohongshu left-nav task proved the target can be found under Chrome `AXWebArea`, but the manual low-level flow took too many requests and was vulnerable to stale AX refs and stale screenshots.

## 2. Non-goals

- Do not replace existing `@observe`, `@ax-*`, `@window-*`, `@screenshot`, or mouse commands.
- Do not implement side-effectful `@web-act` before a baseline bench exists.
- Do not make mouse fallback implicit.
- Do not treat stale screenshot or stale AX refs as recoverable success.
- Do not depend on public websites for deterministic CI.

## 3. Density Metrics

Every dense primitive and every bench case should report task-density metrics:

```json
{
  "backend_request_count": 1,
  "control_frame_count": 1,
  "elapsed_ms_total": 620,
  "agent_decision_points": 0,
  "semantic_action_count": 1,
  "mouse_fallback_count": 0,
  "stale_ref_recovery_count": 1,
  "stale_visual_block_count": 0,
  "verification_passed": true,
  "false_success_count": 0,
  "payload_bytes": 4096,
  "trace_step_count": 7
}
```

Definitions:

- `backend_request_count`: number of line-control requests the agent sends.
- `agent_decision_points`: number of intermediate responses that require the agent to choose the next low-level command.
- `semantic_action_count`: count of AX/window/value/key semantic actions.
- `mouse_fallback_count`: count of physical mouse fallback actions.
- `false_success_count`: command returned success while verification did not prove task completion.
- `trace_step_count`: number of daemon-internal trace steps in a dense primitive.

## 4. Proposed Commands

### 4.1 `@gui-probe`

Read-only high-density observation.
It fuses liveness, capabilities, window state, optional visual freshness, and optional AX matching.

Example:

```text
@gui-probe#1:{mode:"web",target:{browser:"active"},scope:"active_web_area",match:{text:"首页"},include_screenshot:true}
@gui-probe#2:{mode:"web",target:{window_id:"pid:8231/window:0"},scope:"target_window_web_area",match:{text:"首页"},include_screenshot:true}
@gui-probe#3:{mode:"web",target:{window_ref:"@e1",observation_id:"obs-..."},scope:"target_window_web_area",match:{text:"首页"},include_screenshot:true}
```

Window-scoped target note:

- `target:{browser:"active"}` means the probe must resolve a unique active/focused browser window.
- `target:{window_id:"pid:.../window:..."}` should bypass active-window ambiguity and probe that browser window directly.
- `target:{window_ref:"@e1",observation_id:"obs-..."}` should resolve a short-lived window ref from `@observe` / `@window-find` to a backend `window_id`, then use the same window-scoped path.
- This command remains a proposed read-only composite. The currently implemented productized pieces are `@web-find.target.window_id` and `@web-find.target.window_ref`.

Response sketch:

```json
{
  "kind": "gui-probe",
  "schema": "rdog.gui-probe.v1",
  "status": "ready",
  "capabilities": {
    "screenshot": "available",
    "accessibility": "available",
    "window_control": "available"
  },
  "window": {
    "window_id": "pid:8231/window:0",
    "process_name": "Google Chrome",
    "title": "小红书 - 你的生活兴趣社区 - Google Chrome",
    "frontmost": true,
    "occluded": false,
    "interactable": true
  },
  "visual": {
    "status": "complete",
    "freshness": "fresh",
    "image": "screenshot-...jpg",
    "manifest": "screenshot-...json"
  },
  "web_area": {
    "status": "found",
    "target_id": "pid:8231/window:0/path:..."
  },
  "matches": [
    {
      "ref": "@e12",
      "id": "pid:8231/window:0/path:...",
      "role": "AXLink",
      "matched_field": "description",
      "matched_text": "首页",
      "actions": ["AXPress"],
      "rect": {"x":16,"y":274,"width":116,"height":48},
      "confidence": 0.98
    }
  ],
  "trace": [
    {"step":"capture-ax-snapshot","status":"complete"},
    {"step":"active-browser-window","status":"ok"},
    {"step":"find-ax-web-area","status":"ok"},
    {"step":"match-page-content","status":"ok","detail":"match_count=1"}
  ]
}
```

### 4.2 `@web-find`

Read-only page-content locator for a browser `AXWebArea`.

Example:

```text
@web-find#2:{target:{browser:"active"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@web-find#22:{target:{window_id:"pid:8231/window:0"},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
@web-find#23:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"首页"},roles:["AXLink","AXButton"],limit:10}
```

Default rules:

- Scope is `active_web_area` for `target:{browser:"active"}`.
- Scope is `target_window_web_area` for `target.window_id` and `target.window_ref`.
- Browser chrome is excluded.
- Match fields are `description`, then `name`, then `value`.
- Actionable ancestors are preferred when visible text is exposed on a child.
- Broad `@ax-find` false negatives are bypassed by drilling into the current `AXWebArea`.
- The command performs no action.
- `target.window_id` is read-only. It does not activate, focus, raise, close, click, or type into the window.
- `target.window_ref` must be paired with `observation_id`; it accepts only observation refs whose backend kind is `window`.
- `target.window_ref` is short-lived. If it is expired, stale, or points to an element ref, the command returns `status:"blocked"` with `error_code:"WINDOW_REF_INVALID"`.
- When no `window_id` is provided, multiple matching browser windows with no unique focused window still return `BROWSER_WINDOW_AMBIGUOUS`.

Phase 1 implementation status:

- Parser: `src/control_protocol.rs` accepts `@web-find`.
- Executor: `src/control_actions.rs` routes to the read-only AX snapshot path.
- Finder: `src/control_web.rs` selects a browser window, finds its `AXWebArea`, matches page-owned candidates, and returns structured blockers for no-window / no-web-area / no-match cases.
- Window scope: `target.window_id` can select a known browser AXWindow directly, avoiding `BROWSER_WINDOW_AMBIGUOUS` in multi-window Chrome/Safari/Edge/etc. setups.
- Window ref scope: `target.window_ref + observation_id` resolves a short-lived observation ref to the backend window id and uses the same window-scoped selector.
- Response schema: `rdog.web-find.v1`, `kind:"web-find"`, `scope:"active_web_area"` or `scope:"target_window_web_area"`, `target.window_id` / `target.window_ref` / `target.observation_id`, `matches[]`, `trace[]`, `match_count`, `returned_count`, and `truncated`.

### 4.3 `@web-act`

Side-effectful page-content action.
This command is implemented only after Phase 0 bench baseline and Phase 1 `@web-find`.

Example:

```text
@web-act#3:{target:{browser:"active"},match:{text:"首页"},action:"press",verify:true}
@web-act#33:{target:{window_id:"pid:8231/window:0"},match:{text:"首页"},action:"press",verify:true}
@web-act#34:{target:{window_ref:"@e1",observation_id:"obs-..."},match:{text:"首页"},action:"press",verify:true}
```

Internal algorithm:

1. Capture the current AX snapshot.
2. Resolve the active browser window, the explicit `target.window_id`, or `target.window_ref + observation_id` when provided.
3. Find the current `AXWebArea`.
4. Match page-owned candidates by `description`, `name`, and `value`.
5. Require exactly one actionable match.
6. Execute `AXPress` for `action:"press"`.
7. If the first action attempt hits a stale-like target error, re-find once inside the same daemon request and retry `AXPress` on the fresh match.
8. Verify by first refreshing the same `AXWebArea` subtree and re-running the same page-content match; fall back to a fresh AX snapshot only when subtree refresh is unavailable.
9. Return action and verification trace.

Safety defaults:

- No coordinate or mouse fallback in this phase.
- `action:"press"` is the only supported action and maps to `AXPress`.
- `verify:true` is the default.
- Verification is AX-based; for visual feed-change tasks, an external screenshot / URL verifier is still required before claiming task success.
- Ambiguous matches return `needs_disambiguation`.
- Missing `AXPress` returns `blocked` and does not act.

Phase 2 implementation status:

- Parser: `src/control_protocol.rs` accepts `@web-act`.
- Executor: `src/control_actions.rs` routes to the web action helper.
- Action: `src/control_web/act.rs` performs unique-match `AXPress` only.
- Retry: stale-like action errors trigger one internal re-find retry.
- Verification: `@web-act.verify` first refreshes the selected `AXWebArea` subtree and re-runs the same page-content match; if that refresh is unavailable, it falls back to a fresh AX snapshot.
- Response schema: `rdog.web-act.v1`, `kind:"web-act"`, `selected_match`, `action_result`, `verification`, `trace`, `performed`, and `verified`.

### 4.4 `@gui-act`

Generic GUI action for non-browser apps.
It should reuse the same task-compiler engine after `@web-act` proves the model.

Example:

```text
@gui-act#4:{target:{app:"TextEdit",title_contains:"release"},match:{role:"AXButton",name:"Save"},action:"press",activate:true,verify:true}
```

### 4.5 `@gui-bench`

Daemon-facing fixture bench runner for computer-use density cases.
Phase 3A/3B are implemented as a read-only fixture runner.
It does not open browsers, execute GUI actions, move the mouse, or depend on public websites.

Example:

```text
@gui-bench#10:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}
@gui-bench#11:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all"}
@gui-bench#12:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}
@gui-bench#13:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-find",runner:"live",allow_side_effects:true}
@gui-bench#14:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"dense-web-act",runner:"live",allow_side_effects:true}
```

Phase 3B implementation status:

- Parser: `src/control_protocol.rs` accepts `@gui-bench`.
- Executor: `src/control_actions.rs` routes to the fixture runner.
- Runner: `src/control_gui_bench.rs` reads the built-in `tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json` fixture.
- Supported variants: `baseline-low-level`, `dense-web-find`, `dense-web-act`, and `all`.
- Response schema: `rdog.gui-bench.v1`, `kind:"gui-bench"`, `runner:"fixture"`, `runs[]`, `metrics`, `thresholds`, `checks`, `threshold_failures`, `steps_summary`, optional `artifact`, and `trace`.
- Single-variant responses keep top-level `metrics` / `thresholds` / `steps_summary` for compatibility. `variant:"all"` uses `runs[]` as the comparison source.
- `write_artifact:true` writes pretty JSON under `target/rdog-bench/`. The default is false.
- `status:"complete"` means the runner completed. `dense_target_passed:false` means at least one selected variant did not meet density thresholds; this is expected when `baseline-low-level` is included.

Phase 3C CI artifact collection status:

- `src/shell/tests.rs` covers the real line-control receiver path for `variant:"all",write_artifact:true`.
- The verified CI collection artifact is `target/rdog-bench/computer-use-density__xhs-left-nav-home__all.json`.
- The artifact JSON must contain `schema:"rdog.gui-bench.v1"`, `runner:"fixture"`, `variant:"all"`, `variant_count:3`, `runs[]`, `threshold_failures[]`, and `artifact.path`.
- Tests that create this artifact must remove it before finishing, so normal test runs do not leave `target/rdog-bench/` files behind.

Phase 3D live replay opt-in status:

- Default `@gui-bench` remains `runner:"fixture"` and is read-only.
- Live replay requires both `runner:"live"` and `allow_side_effects:true`.
- Live replay rejects `variant:"all"` so a single request cannot execute multiple real GUI steps.
- Supported live replay variants are `dense-web-find` and `dense-web-act`.
- `dense-web-find` replays the fixture command through the existing `@web-find` response builder.
- `dense-web-act` replays the fixture command through the existing `@web-act` response builder, so the real side-effect contract remains unique match -> `AXPress` -> stale retry -> fresh AXWebArea/AX verification.
- Live responses keep the normal bench fields and add `runs[].live_replay` with `command`, `response_kind`, `response_schema`, `response_status`, `performed`, `verified`, `match_count`, `returned_count`, `error_code`, `message`, and `passed`.
- `runner:"live"` with `write_artifact:true` writes a separate `__live.json` artifact, avoiding collision with fixture artifacts.

## 5. Capability Policy

Agents should not pay separate `@ping` and `@capabilities` round trips for every dense task.

The first productized preflight is `@bootstrap`:

- `@bootstrap#id:{mode:"basic",capability_policy:"fresh"}` returns liveness + capabilities.
- `@bootstrap#id:{mode:"gui",capability_policy:"fresh"}` also embeds an observe bundle.
- `capability_policy:"cached"` is reserved for a later TTL cache and currently returns `BOOTSTRAP_CAPABILITY_CACHE_UNIMPLEMENTED`.
- All `@bootstrap` requests are read-only and Zenoh session-channel-only.
- `@gui-probe` remains a later read-only task probe, not part of the current bootstrap delivery.

Dense commands should:

- verify liveness internally,
- cache capability status per daemon process for a short TTL,
- expose `capability_policy:"cached" | "fresh"` in request/response,
- return structured blockers when permissions or platform support are missing.

This changes the contract from "agent must remember the preflight chain" to "dense command returns a first-class blocker".

## 6. Bench Suite

### 6.1 Fixture Schema

Fixtures use this shape:

```json
{
  "schema": "rdog.computer-use-density.bench.v1",
  "suite": "computer-use-density",
  "case": "xhs-left-nav-home",
  "dense_target": {
    "max_backend_request_count": 2,
    "max_agent_decision_points": 1
  },
  "variants": [
    {
      "variant": "baseline-low-level",
      "metrics": {
        "backend_request_count": 8,
        "agent_decision_points": 7,
        "semantic_action_count": 1,
        "mouse_fallback_count": 0,
        "false_success_count": 0
      },
      "steps": [
        {"name":"capabilities","command":"@capabilities#1","agent_decision_after":true}
      ]
    },
    {
      "variant": "dense-web-act",
      "metrics": {
        "backend_request_count": 1,
        "agent_decision_points": 0,
        "semantic_action_count": 1,
        "mouse_fallback_count": 0,
        "stale_ref_recovery_count": 1,
        "false_success_count": 0
      },
      "steps": [
        {"name":"press-page-owned-home-link","command":"@web-act#1:{...}"}
      ]
    }
  ]
}
```

### 6.2 Required Cases

Initial cases:

- `xhs-left-nav-home-baseline`: current low-level command sequence for a page-owned Chrome navigation link.
- `active-web-area-find-link`: local fixture page, read-only find.
- `web-action-stale-ref-retry`: fake backend or local fixture that re-renders between find and action.
- `web-ambiguous-link`: returns `needs_disambiguation`.
- `screenshot-stale-hard-stop`: no `@savefile` when composite screenshot freshness fails.

### 6.3 Acceptance

For the original task class:

```text
Given active Chrome on a web page and target text "首页",
rdog should locate the page-owned AX target and either perform AXPress or return a structured blocker
in no more than 2 backend requests,
without agent-authored AX path drilling,
without silent coordinate fallback,
and with verification evidence in the response.
```

## 7. Implementation Phases

### Phase 0: Baseline bench and docs

- Fix cookbook protocol shape from `target:{window_id:"..."}` to `target:{id:"..."}`.
- Add `tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json`.
- Add `tests/computer_use_density.rs` to validate the fixture and lock density metrics.
- Do not add new line-control commands yet.

### Phase 1: Read-only `@web-find`

Implemented touch points:

- `src/control_protocol.rs`
- `src/control_actions.rs`
- `src/control_web.rs`
- `src/shell/tests.rs`
- `src/zenoh_control.rs`
- `src/control_protocol/tests.rs`

Validation focus:

- `@web-find` remains read-only.
- The default target is `target:{browser:"active"}`.
- The default scope excludes browser chrome.
- `match_count` reports total matched candidates and `returned_count` reports the limited response size.
- `truncated:true` means either the AX snapshot was truncated or the result set exceeded `limit`.

### Phase 2: Side-effectful `@web-act`

Implemented after `@web-find`:

- `@web-act` reuses the `@web-find` match shape.
- It executes only a unique `AXPress` match.
- It re-finds once when the first action target is stale-like.
- It verifies through a fresh `AXWebArea` subtree first, with full AX snapshot fallback when subtree refresh is unavailable.
- It does not implement mouse fallback.

### Phase 3: Bench runner

Phase 3A/3B implemented `@gui-bench` as a daemon-facing fixture runner:

- `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"baseline-low-level"}`
- `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all"}`
- `@gui-bench#id:{suite:"computer-use-density",case:"xhs-left-nav-home",variant:"all",write_artifact:true}`
- single source fixture: `tests/fixtures/computer_use_density/xhs_left_nav_home_baseline.json`
- response schema: `rdog.gui-bench.v1`
- variants: `baseline-low-level`, `dense-web-find`, `dense-web-act`
- baseline returns `status:"complete"` and `dense_target_passed:false`
- dense variants return `dense_target_passed:true`
- `write_artifact:true` emits `target/rdog-bench/<suite>__<case>__<variant>.json`
- line-control coverage uses `SystemControlActionExecutor` through `src/shell/tests.rs`
- Phase 3C additionally verifies the CI artifact collection path for `variant:"all",write_artifact:true`
- Phase 3D adds explicit live replay with `runner:"live",allow_side_effects:true`; `variant:"all"` is rejected in live mode

Later Phase 3 work can extend live replay to additional cases, but every real GUI side effect must stay explicit opt-in and must not make `@gui-bench` touch the live GUI by default.

### Phase 4: Generalize to `@gui-act`

Move the task compiler beyond web content once the web path is proven.

## 8. Risks

- Dense primitives can become opaque. Mitigation: every response includes `trace[]`.
- Dense primitives can hide unsafe action. Mitigation: no silent fallback or disambiguation.
- Public sites are unstable. Mitigation: deterministic local fixtures for CI; public websites only as manual smoke.
- Capability caches can hide permission changes. Mitigation: short TTL and `capability_policy:"fresh"`.
- Chrome AX refs can churn. Mitigation: find and act inside one daemon request, with one bounded stale re-find retry.

## 9. Current Status

This spec is the formal handoff from the autoresearch artifact.
Phase 0, Phase 1, Phase 2, and Phase 3A are implemented by the code and tests named in section 7.
