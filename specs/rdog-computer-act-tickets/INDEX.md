# Tickets Index: rdog `@computer-act`

22 tickets implementing `@computer-act` and supporting primitives. Read
`specs/rdog-computer-act-spec.md` first; this file is the build order.

## Build Order (Dependency Graph)

Tickets are numbered in dependency order. Work the **frontier** — any
ticket whose blockers are all done.

### Phase A — Foundation primitives (parallel, no blockers)
- `01-wait-primitive.md` — `@wait` primitive
- `02-open-app-primitive.md` — `@open-app` primitive (macOS only)
- `03-cancel-seq-command.md` — `@cancel#seq` parser + cancel runtime

### Phase B — Skeleton + minimum slice (sequential)
- `04-computer-act-parser-dispatcher-skeleton.md` — depends on 01, 02
- `05-minimum-vertical-slice.md` — depends on 04

### Phase C — Action coverage (parallel after 05)
- `06-click-family.md` — depends on 04
- `07-hover-and-type.md` — depends on 04
- `08-hotkey-and-hotkey-click.md` — depends on 04
- `09-scroll-and-drag.md` — depends on 04
- `10-open-url-folded.md` — depends on 04

### Phase D — Implicit observe
- `11-implicit-observe-and-freshness.md` — depends on 05

### Phase E — Verify tiers (sequential)
- `12-verify-none.md` — depends on 05
- `13-verify-best-effort.md` — depends on 11, 12
- `14-verify-always.md` — depends on 13

### Phase F — Errors + timeout (parallel after 05)
- `15-error-envelope-e2.md` — depends on 05
- `16-timeout-per-action-table.md` — depends on 05

### Phase G — Observability (sequential)
- `17-density-metrics.md` — depends on 05
- `18-trace-summary-and-savefile.md` — depends on 17

### Phase H — `@flow` integration (sequential)
- `19-flow-policy-allow-computer-act.md` — depends on 05
- `20-flow-expect-kind-extensions.md` — depends on 19

### Phase I — End-to-end + benchmark (sequential)
- `21-all-actions-e2e-smoke.md` — depends on 06 through 18
- `22-density-benchmark.md` — depends on 21

## Frontier

After completing Phase A (no blockers), the **frontier** is ticket 04.
After completing 04, the frontier is 05 (single) and 06-10 (parallel
group). After completing 05, the frontier expands to 06-10, 11, 12, 15,
16, 17, 19.

## Critical Path

`01` → `04` → `05` → `11` → `13` → `14` → `18` → `21` → `22`

This is the longest dependency chain; it gates the final benchmark.
Tickets off the critical path (06-10, 15-16, 19-20, 17) can be picked
up by parallel sessions.

## Open Frontier After Each Milestone

| After completing | Open frontier |
|---|---|
| 01, 02, 03 (parallel) | 04 |
| 04 | 05 |
| 05 | 06, 07, 08, 09, 10, 11, 12, 15, 16, 17, 19 |
| 11 | 13 |
| 12, 13 | 14 |
| 14 | (no new tickets depend on 14; safe to defer) |
| 17 | 18 |
| 19 | 20 |
| 06-18 (all) | 21 |
| 21 | 22 |
