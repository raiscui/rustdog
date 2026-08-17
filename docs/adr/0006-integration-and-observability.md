# ADR-0006: `@capabilities`, `@flow`, and trace/density observability

`@computer-act` is **not** declared in `@capabilities`. The output of
`@capabilities` continues to describe only OS-level permissions
(screenshot / accessibility / window_control). The 13 actions' per-action
availability is dynamic (depends on window state, foreground app, OS
permissions at dispatch time) and is not enumerable at daemon start; clients
probe `@computer-act` support with one `@computer-act#1:{}` call and
treat `unknown_command` as a clean fallback signal. This matches the
existing pattern for `@gui-probe`, `@flow`, `@ax-action`, and
`@web-act` — none of which appear in `@capabilities` either.

Inside `@flow`, `@computer-act` is **allowed** as a `ControlLine` step,
gated by an explicit `policy.allow_computer_act: true` opt-in (default
false, mirroring `@flow`'s existing deny-by-default style). `verify`,
`error`, `observation_id`, and `density` semantics are preserved unchanged
when embedded. To assert structured response fields inside flow steps
(`verification.passed`, `ok`, `observation_id`), the `Expect.kind` enum
gains two new members: `response_field_equals(path, value)` and
`response_path_contains(path, substring)`.

The current flow runtime also has a deliberately narrow `GuiTransaction` step:
it chains a first real ref/epoch mutation to later `$successor` mutations and
stops at the first checked failure. This is not the rejected general-purpose
`ComputerAct` step type. A flow can additionally set
`policy.execution.strict_background:true`; the shared flow gate rejects
foreground activation and raw physical input before dispatch while preserving
semantic AX paths.

Every successful `@computer-act` response carries a `density` block
(shared with `@gui-probe`) and a `trace_summary` (4 entries:
`implicit_observe / ref_resolve / dispatch / verify`, each with
`elapsed_ms` and `status`). Full trace — including sub-steps and AX
dumps — is **opt-in** via `trace: "savefile"` in the request, returning a
`trace_savefile` path in the response. This matches the existing
`@savefile` mechanism and keeps the default response under ~3 KB.

## Status

Accepted.

## Considered Options

- **K1 / K2 / K3 capabilities**: not declared (K1) ✅ / boolean / structured
  subset. K1 follows the existing rdog convention; K3's "actions list"
  would be a misleading static claim.
- **F1 / F2 / F3 `@flow` integration**: allow as ControlLine (F1) ✅ /
  forbid / add a general-purpose `ComputerAct` step type. The later
  `GuiTransaction` addition is narrower than F3: it only owns successor
  chaining and first-error stopping, while each action still executes through
  the existing line-control parser.
- **U1 / U2 / U3 trace**: savefile-only / summary inline + opt-in savefile
  (U2) ✅ / full inline. U2 keeps the response payload small while
  exposing the four steps the agent loop cares about.

## Consequences

- `@capabilities` schema is unchanged; `K1` means rdog version discovery
  remains the path for "is `@computer-act` even compiled in?".
- `@flow` schema gains `policy.allow_computer_act` (default false), optional
  `policy.execution.strict_background`, checked `GuiTransaction`, and two
  structured-field `Expect.kind` members. Existing callers remain interactive
  unless they explicitly select the new policy or step.
- `density` field names are now shared between `@gui-probe` (read-only
  high-density) and `@computer-act` (side-effecting high-density); a
  future density benchmark suite can consume both without per-command
  translation.
