---
name: rdog-control
version: "2.21-darwin-dim3"  # 2026-07-28:加 Failure Handling 三段式表
description: "Use for rdog control on a local or named machine: health, shell, GUI, browser, window, AX, PTY, flow, verification, and safety."
---

# Rdog Control

`rdog control` sends ordered frames to a trusted `rdog daemon`. Every bash call
must begin with `rdog control`. Read the literal `@response ` from full stdout.

## Tight Loop

1. Read current state and resolve one exact target.
2. Choose one lane below.
3. Send the narrowest semantic action once.
4. Read fresh state in a separate call.
5. Report only the result proved by that fresh read.

Use the local fast path when agent and daemon share one machine:

```bash
rdog control @ping
rdog control @ping @capabilities#1 @observe#2
```

Put `TARGET` after `control` only for a named or remote daemon. Permission and
capability errors are final results.

## Native App Lane

Use this lane for visible app controls, values, and buttons. `app:APP` is valid
only when the app resolves to one interactable window. `pid:PID/window:INDEX`
from a fresh `@window-find` is also accepted.

```bash
rdog control @open-app:APP
rdog control @ax-find:app:APP,ROLE
rdog control @ax-press:app:APP,DESCRIPTION
rdog control @ax-press-sequence:app:APP,DESCRIPTION_1,DESCRIPTION_2
```

The 5-field `@ax-press:app:APP,DESCRIPTION,RESULT_ROLE,EXPECTED_VALUE,MAX_ATTEMPTS`
form is intentionally not in this primary list. Read the GUARDED PRESS section
below before considering it; most tasks do not need it.

Read current state before any press. After `@open-app`, the very first read
must be for the role that carries the app's current value or text (commonly
`AXStaticText` or `AXValue`), not the control role. The control role
(`AXButton`, `AXMenuItem`, …) lists the available actions; it does not
reveal what state those actions will modify. Pressing a control without
first reading the current state is guessing, and an over-defensive guess
(such as pressing a clear / reset button before input) is a protocol
violation, not a safety measure.

Use only descriptions returned by that fresh app-scoped AX read. A sequence
has exactly one comma field per observed AXButton description, in the
intended action order. Do not place a compound instruction or unobserved
label in one field.

GUARDED PRESS — opt-in only. The 5-field form
`@ax-press:app:APP,DESCRIPTION,RESULT_ROLE,EXPECTED_VALUE,MAX_ATTEMPTS`
makes the program repeat `DESCRIPTION` up to `MAX_ATTEMPTS` (1 to 3) times,
taking a fresh AX read of `RESULT_ROLE` after each press, and stopping as
soon as the read matches `EXPECTED_VALUE`. The program does not know the
app, the button meaning, or the desired baseline — those come from the model.

Use guarded press only when BOTH conditions hold:
  1. A prior fresh AX read of `RESULT_ROLE` already showed non-target
     content, AND
  2. You cannot safely count individual presses to reach the baseline
     (for example the stale length is unknown, or the input has nested
     structure).

If the fresh read already matches the intended baseline, issue input
directly without reset. If the stale content is a short known string or
number of characters, prefer counting N plain `@ax-press` calls; each
attempt then runs independently and the runner records them as separate
performed actions.

Do not preemptively reset before input. Adding a reset on a fresh app
that already shows the target baseline (or starts empty) is a protocol
violation, not a precaution. Reserve guarded press for genuinely stale
or unknown content.

After a guarded press, regardless of `verified`, take a separate fresh
`@ax-find` for the result role before issuing the next input step. The
fresh read is the only proof the reset actually took effect.

Use `@key` for an explicit shortcut request or when no semantic action can express
the operation. Use guarded screenshot coordinates only after semantic lookup fails.

## Browser Lane

Read `references/cookbook-web-content.md` before a browser content action. Use
global capture only to identify one focused browser, then targeted capture. After
navigation, use a fresh window or `browser:"active"`; never reuse `/window:N`
across turns.

Success requires fresh PID, display/window ownership, page or URL state, and the
intended content change. `performed:true` or a screenshot alone is incomplete.

## Other Lanes

| Need | First action |
| --- | --- |
| Health | `@ping` |
| Permissions | `@capabilities` or `@bootstrap` |
| Window ownership | `@window-find` |
| One shell command | `@cmd` or `@script` |
| Finite workflow | `@flow` |
| Stateful terminal | `rdog control TARGET --pty -- COMMAND` |

Read only the matching reference before using an Other Lane:

- `references/control-workflow.md`: targets, windows, PTY, hosts, and hardware.
- `references/protocol.md`: frame syntax, responses, errors, AX, mouse, and flow.
- `references/zenoh-hardware.md`: discovery, SDK, serial, and bridge hosts.

## Failure Handling

When a control returns `performed:false`, zero matches, or an unexpected
fresh read, walk the three-tier ladder below before retrying with a new
lane or locator. Each repair step must end with a fresh read that proves
the change actually took effect on the relevant subtree.

| Trigger (observable evidence) | First repair | Still failing |
| --- | --- | --- |
| `@window-find` matches 0 windows | Verify the app is open (`@open-app` first); check `app:APP` spelling — ASCII case-insensitive after v2.18, but Chinese names do not resolve | Take a `@screenshot` to confirm visual state; do not guess window state blind |
| `@ax-find` returns 0 elements for a role | Verify role spelling; re-query the parent role (e.g. `AXWindow`) to find the right subtree first | Drop to `@key` keyboard shortcut if the role has no children |
| `@ax-press` returns `performed:false` | Re-`@ax-find` for the role; the button may have been renamed, moved, or disabled | Switch to a sibling description or `@key` |
| `@ax-press-sequence` stops mid-way | The failed step's `target_id` is in the response; re-query just that role and continue from the failing step | Restart the sequence from a known fresh state (`@ax-find` first) |
| Fresh AX read returns a different value than expected | You may have read the wrong subtree; re-query with the correct value role (commonly `AXStaticText` or `AXValue`) | Use `@screenshot` for visual confirmation |
| Window ownership is ambiguous (≥2 matching interactive windows) | Disambiguate via fresh `@window-find` and pass `pid:PID/window:INDEX` instead of `app:APP` | Stop; ask the user which window is the target |
| `@capabilities` reports `permission_status: denied` or `unavailable` | The capability name is in the response; for Accessibility on macOS, system prompt requires System Settings → Privacy & Security → Accessibility | Do not retry without the missing permission; further calls will keep failing identically |
| `@ping` times out or returns non-`pong` | Check the daemon log/socket path; on macOS, restart with `rdog daemon --config rdog_macos.toml` | Do not keep issuing commands; the daemon may be wedged and a flood will only obscure the real error |
| `@screenshot` writes to a path the agent cannot read | The daemon writes to its own temp dir; do not guess the path; use the `@observe#N` ref the response returned | Switch to global AX read without window scope |
| `rdog control` rejected a frame with `code: 64` | Read the literal `error` field; it is the single source of truth for parser/locator failures | Do not retry the same payload — fix the indicated field first |

Never infer app state from screenshot output alone; screenshot is a
cross-check, not the primary source. The primary source is always a
fresh `@ax-find` (or `@window-find` for window state) read that returns
matching elements.

## Safety

Stop on ambiguous ownership, permission denial, or a failed action. Retry a step at
most three times with a different locator or lane. Ask before destructive, security,
permission, reboot, or production actions.
