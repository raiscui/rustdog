# Spec: `@computer-act` for computer-use VLMs

> Companion to ADRs `0001`–`0006`. This document is the **what**; the ADRs
> are the **why**. Read this for build scope; read the ADRs for design
> rationale.

## Problem Statement

Computer-use vision-language models (Mano-CUA, Holo 3.1, EvoCUA, GTA1) emit
closed-set action verbs at inference time — strings like
`<action>click(start_box='<|box_start|>(499,565)<|box_end|>')</action>` —
and expect a deterministic, low-latency executor on the other side. Today
there is no rdog primitive purpose-built for this: an external agent must
hand-compose `@observe` + `@ax-find` + `@click` + `@observe` for every
single action, which inflates round-trips, leaks AX-ref lifetime management
to the caller, and prevents the agent from expressing "do this action with
verify" as a single declarative request.

## Solution

Add a single rdog line-control primitive, `@computer-act`, that accepts a
structured JSON envelope describing one action, dispatches it to existing
rdog primitives (`@click` / `@key` / `@mouse-move` / `@type-text` / `@wheel`
/ `@drag` / `@ax-action`) — plus two new primitives (`@open-app` and
`@wait`) — and returns a unified response carrying dispatch result,
optional verify evidence, optional observation reference, density metrics,
and an error envelope with retry strategy when something goes wrong. The
client parses VLM-specific XML / JSON into the rdog-native envelope; rdog
itself never sees model syntax.

## User Stories

1. As a Mano-CUA agent loop, I want to send `@computer-act#N:{action:"click",args:{start_box:[x,y]}}` and get back whether the click landed, so that I can drive a 16-action closed set without learning thirteen different rdog commands.
2. As a Mano-CUA agent loop, I want `verify:"best_effort"` to give me an AX-tree diff without paying the screenshot cost, so that high-frequency typing and scrolling stay fast.
3. As a Mano-CUA agent loop, I want `verify:"always"` to give me a fresh screenshot plus AX tree plus window state, so that I can hand it back to the model as the next-turn observation.
4. As a Mano-CUA agent loop, I want to pass a `target.ref + observation_id` instead of a coordinate on the second turn, so that I can avoid re-observing every time.
5. As a Mano-CUA agent loop, I want the response to tag whether the daemon actually used my supplied observation, so that I know when my ref is going stale before the model does.
6. As a Mano-CUA agent loop, I want `wait(N)` to sleep for N seconds and `wait(30s)` not to be killed by an over-eager timeout, so that pacing and debounce work as the model intends.
7. As a Mano-CUA agent loop, I want to cancel an in-flight `@computer-act` mid-wait without killing the daemon, so that I can recover from a wrong decision without restarting.
8. As a Mano-CUA agent loop, I want a structured error envelope telling me which retry strategy to apply, so that I don't have to invent retry logic per error type.
9. As a `@flow` author, I want to embed `@computer-act` as a `ControlLine` step gated by `policy.allow_computer_act`, so that I can build composite multi-action flows in one round-trip.
10. As a benchmark author, I want `density` metrics on every response, so that I can compare `@computer-act` against the manual `@observe + @click + @observe` baseline.
11. As a debugger, I want a 4-step `trace_summary` on every response, so that I can see at a glance whether implicit_observe, ref_resolve, dispatch, or verify is the bottleneck.
12. As a debugger, I want an opt-in full trace via `trace:"savefile"`, so that I can deep-dive without bloating the default response payload.
13. As a daemon operator, I want `@computer-act` to fall back to existing per-command permission checks rather than declaring a new capability, so that `@capabilities` output stays meaningful.
14. As a future Holo 3.1 adapter, I want the schema to be versioned as `rdog.computer-act.v1`, so that Holo's three-tool subset can ship as a sibling schema without breaking v1 clients.
15. As a future cross-platform user, I want `@open-app` on macOS to use `open -a`, with Windows / Linux paths queued in LATER_PLANS.

## Implementation Decisions

(All decisions are recorded in detail in ADRs `0001`–`0006`. Summary below.)

**Surface** (ADR-0002): one new command `@computer-act`. Three other
control-flow signals (`finish`, `stop`, `call_user`) stay in the client
loop and never reach rdog. Wire format is rdog-owned JSON; the client
parses VLM XML.

**Target** (ADR-0003): `@computer-act` accepts either normalized
coordinates (`start_box`, `[0, 1000]`) or rdog-native AX refs
(`target.ref` + `observation_id`). Coordinates trigger an implicit
`@observe` to resolve a ref before dispatch.

**Gap audit** (ADR-0003): two new primitives added — `@open-app` and
`@wait`. Two actions fold into existing primitives — `open_url` becomes
`@cmd "open <url>"`, `hotkey_click` becomes `@key down` + `@click` +
`@key up`.

**Schema** (ADR-0004): flat JSON envelope at `rdog.computer-act.v1`.
Top-level fields are `schema`, `action`, `args`, `verify`,
`observation_id`, `timeout_ms`, `trace`. Field names follow Mano-CUA
where natural (`start_box`, `app_name`, `content`, `direction`,
`amount`) and rdog conventions where pre-existing (`duration_ms`,
`ref`, `observation_id`, `coordinate_space`).

**Verify** (ADR-0004): three tiers — `none` (no observation overhead),
`best_effort` (AX-tree diff only), `always` (full observation +
screenshot + AX tree + window state). Tier is selected per request.

**Errors** (ADR-0004): `E2` envelope — `error_code` + `error_message`
+ `retry.strategy` + `retry.hint` + `evidence`. Strategies are
`never`, `re_observe_then_retry`, `change_locator`,
`reconnect_then_retry`, `manual_only`. `verify_failed` is its own
`error_code` to distinguish silent-failure from dispatch failure.

**Lifecycle** (ADR-0005): implicit observations are exposed to the
client (returned as `observation_id`), reusable across turns within a
5-second TTL, with `observation_used.freshness` reporting `fresh`,
`stale_re_observed`, or `stale_fallback_to_coords`. Timeouts are
per-action defaults (with `wait` derived from `duration_ms`); clients
may override via `timeout_ms`. Cancellation is first-class this round
via `@cancel#seq:{target_seq:N}`.

**Integration** (ADR-0006): `@computer-act` is **not** declared in
`@capabilities`. It **is** allowed inside `@flow` as a `ControlLine`
step, gated by `policy.allow_computer_act: true` (default false). Flow
gains two new `Expect.kind` values for structured response assertions.

**Observability** (ADR-0006): every response carries a `density` block
(shared field names with `@gui-probe`) and a 4-entry `trace_summary`.
Full trace is opt-in via `trace: "savefile"`, returned through the
existing `@savefile` mechanism as `trace_savefile`.

## Testing Decisions

**Good tests test external behavior, not implementation.** For
`@computer-act`, "external" means: an end-to-end script sends a line,
the daemon returns a structured response, and the script asserts on the
fields visible in the OpenAI-style / line-protocol contract.

**Test layers** (consistent with existing rdog test seams):

1. **Unit tests** in `src/control_protocol/tests/` for parser cases
   (each `action`, each `verify` tier, each `error_code`, malformed
   requests). Pattern: same style as `tests/web_gui.rs`.

2. **Integration tests** for the dispatcher — feeding parsed requests
   into a fake `ControlActionExecutor` and asserting the right
   underlying primitive is called with the right arguments. Pattern:
   same as `src/control_actions.rs` test module.

3. **End-to-end smoke tests** — start a real `rdog daemon`, send
   `@computer-act` requests via the `rdog control` CLI, assert
   behavior. One smoke per action group (`wait`, `open_app`, click
   family, `type`, `drag`, etc.). Pattern: same as
   `specs/rdog-ax-screenshot-manifest-control-plan.md` smoke scripts.

4. **Density benchmark** (LP3) — a separate benchmark script that
   exercises 5–10 typical Mano-CUA tasks and reports the
   `density` fields. This is the proof that ADR-0001's
   high-density promise holds in practice.

**Prior art**: every existing control command in
`src/control_actions.rs` has the same four-layer pattern (parser
unit, executor integration, smoke, optional benchmark). `@computer-act`
follows it without exception.

**Permission failures**: tests must not silently pass when Accessibility
or Screen Recording is missing. A test that requires such permission
should fail loudly with a message that names the missing capability —
same convention as `looks_like_macos_accessibility_permission_denied`.

## Out of Scope

Recorded in `LATER_PLANS.md` (LP1–LP6) for this rdog repository:

- **LP1** — Cross-platform support for `@open-app` and the underlying
  input primitives on Windows and Linux.
- **LP2** — Adapters for Holo 3.1, EvoCUA, and GTA1.
- **LP3** — Density benchmark suite (proof of ADR-0001 promise).
- **LP4** — `rdog.computer-act.v2` evolution (multimodal / audio).
- **LP5** — Rate-limit / quota on `@computer-act`.
- **LP6** — Cross-referencing `docs/glossary.md` with existing rdog
  terms.

Explicitly **not** in this spec:

- Server-side parsing of VLM-native XML (Mano-CUA `<action>...</action>`,
  Holo 3.1 `<tool_call>...</tool_call>`). The client owns that.
- Re-implementation of any existing rdog primitive. `@computer-act`
  routes to existing commands; it does not duplicate them.
- A new `error_code` taxonomy beyond what's in ADR-0004's `E2`
  envelope. New codes can be added later without breaking v1.
- Changes to `@capabilities` schema (K1: not declared).
- General-purpose event-streaming or async push notifications
  from daemon to client. Out of scope for this round.

## Further Notes

- ADRs are the design rationale. The spec is the build scope.
  If they ever disagree, the ADR is wrong (or the spec is) — fix one.
- Tickets in `specs/rdog-computer-act-tickets/` are the unit of work.
  Each ticket declares its blocking edges and fits a fresh `/implement`
  context window.
- The benchmark ticket is intentionally the last one — until
  `@computer-act` is functionally complete, there is nothing to
  benchmark.
