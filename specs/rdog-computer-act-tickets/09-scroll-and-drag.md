# 09 — Scroll + drag

**What to build:** Add `scroll` and `drag` to the dispatcher. `scroll` routes to `@wheel` (with start_box as the target). `drag` routes to `@drag {from, to}` — `start_box` and `end_box` translate to `from` and `to`.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `scroll(start_box:[x,y], direction:"down", amount:3)` → `@wheel {x,y, delta_y:-3}` (positive amount = down).
- [ ] `drag(start_box:[100,200], end_box:[400,500])` → `@drag {from:{x:100,y:200}, to:{x:400,y:500}, duration_ms:450, steps:24}`.
- [ ] Negative `amount` rejected at parse time with `error_code:"invalid_args"`.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
