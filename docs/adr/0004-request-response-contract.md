# ADR-0004: `@computer-act` request, response, verify, error contract

The request envelope is **flat JSON** with a top-level `schema` field, an
`action` discriminator, and an `args` bag whose keys depend on the action.
Field names follow Mano-CUA's Python-style syntax where natural
(`start_box`, `end_box`, `app_name`, `content`, `direction`, `amount`) and
rdog conventions where they predate Mano-CUA
(`duration_ms`, `ref`, `observation_id`, `coordinate_space`).

The response always carries `ok`, `dispatched_to`, `duration_ms`, plus a
`verification` block whose presence and depth are driven by the request's
`verify` field:

| `verify` | Response additions |
|---|---|
| `none` | (no verification, no observation overhead) |
| `best_effort` | `verification.method = "ax_diff"`, lightweight AX-tree diff, optional `observation_id` |
| `always` | full observation (`screenshot_id`, `ax_tree_id`, windows) + AX diff + window state |

Errors use an `E2` envelope: `error_code` + `error_message` + `retry`
sub-object with `strategy` and `hint`, plus an `evidence` block. `strategy`
is one of `never` / `re_observe_then_retry` / `change_locator` /
`reconnect_then_retry` / `manual_only` and aligns with the existing rules in
the `rdog-control` skill (OBSERVATION_EXPIRED → re-observe, permission
denied → never, match_count:0 → change_locator). `verify_failed` is its
own `error_code` because it is structurally distinct from dispatch-layer
errors (action succeeded, GUI did not change).

## Status

Accepted.

## Considered Options

- **T1 / T2 / T3 JSON shape**: mirror Mano-CUA / mirror rdog existing /
  mixed (T3) ✅. T3 lets `start_box → x/y` and `content → text` translation
  live in the dispatcher where it belongs; client stays model-agnostic.
- **V1 / V2 / V3 verify**: never / always / per-field tier (V3) ✅. V3 lets
  `type("hello")` use `best_effort` and `open_app` use `always`, instead of
  paying full screenshot+AX cost on every call.
- **E1 / E2 / E3 errors**: flat binary retry / code + retry hint + evidence
  (E2) ✅ / typed-union Rust-style (E3). E2 matches rdog's existing
  "change something on each retry" rule; E3 breaks flat-JSON convention.

## Consequences

- Schema `rdog.computer-act.v1` is the single source of truth for the wire
  shape; trace + density metrics live at the same envelope depth as success
  fields (no nested `meta` block).
- The client is expected to handle `verify_failed` distinctly from
  `permission_denied` and `target_not_found` — each carries its own retry
  strategy, so the agent loop's retry handler can dispatch by
  `retry.strategy` directly.
- The error code list is open: new codes (e.g. `audio_unavailable`) can be
  added later without breaking v1 clients as long as they keep the
  `error_code / error_message / retry / evidence` envelope.
