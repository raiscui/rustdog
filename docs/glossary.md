# Glossary

Terms specific to the rdog `@computer-act` surface and its supporting
primitives. General rdog line-protocol terms (`@observe`, `observation_id`,
`@flow`, `AX ref`, etc.) are not redefined here — see
`.codex/skills/rdog-control/SKILL.md` and the existing specs.

## Computer-Use Domain

**Computer-Use Action (CUA)**:
A verb in the closed set a vision-language model was trained to emit at
inference time as a GUI driver (Mano-CUA's 16 verbs, Holo 3.1's click /
write / answer, etc.).
_Avoid_: GUI action, model output, primitive.

**`@computer-act`**:
The rdog meta-command that consumes one CUA and dispatches it to the
underlying rdog primitive(s), optionally bundling implicit observation
and verification. See ADR-0001 through ADR-0006.

**`rdog.computer-act.v1`**:
The current JSON schema for `@computer-act` request and response
envelopes. Future CUA models that need a different shape introduce a new
schema id (e.g. `rdog.computer-act.holo.v1`) without breaking v1 clients.

## Observation Lifecycle

**implicit_observe**:
The `@observe` call that `@computer-act` runs internally when the input
is a normalized coordinate (`start_box`) instead of an AX ref. The cost
appears in the response's `density.implicit_observe_ms` field.

**observation_used.freshness**:
Three-state tag in the response telling the client whether the daemon
actually used the supplied `observation_id`:
- `fresh` — ref was valid, no re-observe needed.
- `stale_re_observed` — ref was expired; daemon re-observed and the new
  id is in `observation_used.re_observe_id`. Not an error.
- `stale_fallback_to_coords` — ref was expired and the original input
  had no coords; daemon re-observed and re-targeted.

**observation TTL**:
Implicit observations are valid for 5 seconds (matching rdog's default
`@observe` cache window). After 5 s the obs id is treated as stale.

## Verify and Error

**verify policy**:
The `verify` field on the request, controlling how much evidence
`@computer-act` returns:
- `none` — dispatch only, no observation overhead.
- `best_effort` — AX-tree diff (no screenshot); cheap confirmation that
  something changed near the target.
- `always` — full observation including screenshot, AX tree, and window
  state.

**retry strategy**:
The `retry.strategy` value on the error response envelope:
- `never` — surface to user, do not auto-retry (e.g. permission denied).
- `re_observe_then_retry` — call `@observe` first, then re-issue.
- `change_locator` — loosen keyword, add window scope, or switch from
  coords to ref.
- `reconnect_then_retry` — re-establish the daemon session first.
- `manual_only` — the action cannot be safely retried; surface to user.

**`verify_failed`**:
An `error_code` returned when the dispatch succeeded but the
post-action verify showed no expected GUI change. Treated structurally
distinct from dispatch-layer errors so the agent loop can apply
`re_observe_then_retry` specifically.

## Observability

**density metrics**:
The set of fields shared between `@gui-probe` and `@computer-act`
responses: `backend_request_count`, `control_frame_count`,
`elapsed_ms_total`, `semantic_action_count`, `mouse_fallback_count`,
`stale_ref_recovery_count`, `verification_passed`, `false_success_count`,
`payload_bytes`, `trace_step_count`. `@computer-act` adds three
`@computer-act`-specific fields: `implicit_observe`,
`implicit_observe_ms`, `dispatch_ms`, `verify_ms`.

**`trace_summary`**:
Four-entry list on every `@computer-act` response:
`implicit_observe / ref_resolve / dispatch / verify`, each with
`elapsed_ms` and `status`. Full trace (sub-steps, AX dumps) is opt-in
via `trace: "savefile"` and returned through the existing `@savefile`
mechanism as `trace_savefile`.

## Cancel and Lifecycle

**`@cancel#seq`**:
The companion command that aborts an in-flight `@computer-act`
(releasing held mouse, interrupting `wait`, returning partial verify).
The cancelled request's response carries `error_code: "cancelled"` and is
delivered with the matching seq number. See ADR-0005.

**`@flow` `policy.allow_computer_act`**:
Per-flow opt-in flag (default false) that allows `@computer-act` to be
embedded as a `ControlLine` step inside a `@flow` body. Mirrors the
existing deny-by-default style of `@flow` policy flags.
