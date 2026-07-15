# ADR-0005: implicit_observe, timeout, and cancel semantics

`@computer-act` runs an implicit `@observe` when the input is a coordinate
rather than a ref. The resulting `observation_id` is returned to the client
in the response (when `verify` is not `none`) and the client may pass it back
as `target.observation_id` in the next `@computer-act` call. **TTL is 5
seconds** (matching rdog's default observation cache), and a stale obs id is
**not** an error — the daemon automatically re-observes, returns a new id,
and tags the response with `observation_used.freshness = "stale_re_observed"`.

Timeouts are **per-action defaults**, client-overridable:

| Action | Default timeout | Why |
|---|---|---|
| `wait` | `duration_ms * 1.5 + 1000` | Derived; never self-kills. |
| `open_app` / `open_url` | 10000 ms | Cold-start launch services. |
| `type` | 5000 ms | Per-character bursts. |
| `hotkey_click` | 3000 ms | Composite key + click. |
| `drag` | 5000 ms | Multi-step mouse motion. |
| `click` family, `scroll`, `hover` | 1500-3000 ms | Single mouse events. |

Clients may override with `timeout_ms` in the request. On timeout the
response carries `error_code: "timeout"` with `evidence.last_step` showing
which internal stage the daemon was in when the deadline hit.

Cancellation is **first-class in this round**: a separate
`@cancel#seq:{target_seq:N}` command wakes any in-flight `@computer-act`
(releasing held mouse, interrupting `wait`, returning partial verify), and
the cancelled request's response carries `error_code: "cancelled"` with the
matching seq number. `@cancel` is paired with `@computer-act` by sequence
id, exactly like PTY frames already do.

## Status

Accepted.

## Considered Options

- **L1 / L2 / L3 implicit_observe**: no id exposed / id without reuse /
  id with TTL + reuse (L3) ✅. L1 silently degrades S3 hybrid target to
  S1 coords-only; L2 half-baked (can't reuse).
- **T1 / T2 / T3 timeout**: protocol-none / per-call override / per-class
  defaults with override (T3) ✅. T3 keeps "wait forever" decisions away
  from the model while still respecting rare `wait(120s)` cases.
- **N1 / N2 / N3 cancel**: timeout-only / separate `@cancel#seq` (N2) ✅ /
  via existing `@flow`. N2 keeps `cancelled` distinct from `timeout` in
  the error_code taxonomy.

## Consequences

- 5-second TTL is enforced by the daemon, not negotiated with the client;
  clients that need longer windows must call `@observe` explicitly and pass
  the resulting ref.
- The action-class timeout table is maintained server-side; adding a new
  `@computer-act` action requires adding a default timeout row.
- `@cancel#seq` reuses the existing line-protocol parser and adds no new
  vocabulary beyond the command name and `target_seq` field.
