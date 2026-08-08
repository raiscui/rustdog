# @computer-act Outcome 三态 (postcondition)

## Use When

Reading `@computer-act` response envelope and deciding what to do with
the result. Read this when you see top-level `outcome` field,
`verification.status` field, or `error_code: "verify_failed"` from
older clients.

## Core Idea

`ok` and `outcome` answer different questions. They were once conflated
(Phase F-2 used `ok:false + error_code:verify_failed` to mean "GUI did
not change"), which made clients retry dispatch when they should have
changed selector instead.

`outcome` 三态 is borrowed from
[pi-computer-use ActOutcome](https://github.com/microsoft/pi-computer-use)
to keep cross-product integration trivial.

| Field | Question | Values |
| --- | --- | --- |
| `ok` (top level) | Did the dispatch succeed? | `true` / `false` |
| `outcome` (top level) | Did the postcondition hold? | `worked` / `didnt` / `unknown` |
| `verification.status` (nested) | Finer ax_diff signal | `verified` / `preexisting` / `failed` |

## Decision Table

| dispatch_ok | verify_requested | verify_ran | verify_passed | outcome |
| --- | --- | --- | --- | --- |
| `false` | - | - | - | `"unknown"` (placeholder; client reads `ok` first) |
| `true` | `false` | - | - | `"worked"` (no verify; default dispatch success) |
| `true` | `true` | `false` | - | `"unknown"` (verify timed out / canceled) |
| `true` | `true` | `true` | `true` | `"worked"` (AX changed) |
| `true` | `true` | `true` | `false` | `"didnt"` (verify ran, AX did not change) |

`outcome` is **always written**, even when `ok:false` it stays
`"unknown"` as a placeholder so the response shape is consistent.

## verification.status

Inside `verification`, `status` is a finer ax_diff signal derived in
`verification_status_for_diff`:

| status | ax_diff signature | meaning |
| --- | --- | --- |
| `"verified"` | `windows_modified + elements_modified > 0` | action took effect |
| `"preexisting"` | modified == 0 but added/removed > 0 | AX topology changed but fields did not (rare; treat as suspicious) |
| `"failed"` | all zero | AX did not move at all |

`status` is independent of `outcome`:

- `outcome:"didnt"` ⇔ `verification.status:"failed"` (always paired)
- `outcome:"unknown"` may pair with any status (including absent) because
  verify never produced a diff when it timed out / was canceled

## Client Handling

| outcome | What to do |
| --- | --- |
| `"worked"` | continue |
| `"didnt"` | change selector / change verify policy / look at screenshot. **Do not auto-retry** the same action — re-running on the same view produces the same `didnt` |
| `"unknown"` | re-observe yourself (`@observe`) or call `verify:"always"` to get a screenshot before retrying |

When `ok:false`, ignore `outcome` (it is `"unknown"` by contract) and
read the top-level `error_code` + `retry` block instead.

## Migration From Phase F-2

Phase F-2 (removed):

```json
{
  "ok": false,
  "error_code": "verify_failed",
  "retry": {"strategy": "manual_only", "hint": "..."},
  "evidence": {...}
}
```

Current (`feature/computer-act-outcome-3state`):

```json
{
  "ok": true,
  "outcome": "didnt",
  "verification": {
    "method": "ax_diff",
    "status": "failed",
    "ax_diff": {...}
  }
}
```

If client code still matches `error_code == "verify_failed"`, it must
change to:

- read `outcome` (top level) → `"didnt"` means verify failed postcondition
- read `verification.status` (nested) → `"failed"` is the same signal at
  finer granularity

## Don't

- Do not treat `ok:true + outcome:"didnt"` as success — dispatch succeeded
  but the action did not change the GUI.
- Do not auto-retry on `outcome:"didnt"` — same action + same view produces
  same `didnt`.
- Do not assume `outcome:"unknown"` is rare — it fires when verify hits
  the timeout, when caller passes `verify:"none"` but shape stays
  consistent, or when implicit_observe fails before verify runs.

## Reference

- spec: `specs/rdog-computer-act-spec.md` (Implementation Decisions → Outcome 三态)
- tickets: `specs/rdog-computer-act-tickets/13-verify-best-effort.md`,
  `14-verify-always.md`
- impl: `src/control_computer_act/outcome.rs`,
  `src/control_computer_act/verify.rs::verification_status_for_diff`
- commit: `7764c29 feat(control): outcome 三态替代 Phase F-2 verify_failed envelope`
