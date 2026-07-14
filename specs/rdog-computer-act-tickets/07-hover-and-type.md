# 07 — Hover + type

**What to build:** Add `hover` and `type` to the dispatcher. `hover` routes to `@mouse-move`. `type` routes to `@type-text` with mode `"ax-value"` (semantic preferred), falling back to `@paste` only when target ref is missing.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `hover` → `@mouse-move {x,y}`.
- [ ] `type(content:"hello")` → `@type-text {text:"hello", mode:"ax-value"}` when a ref is supplied.
- [ ] `type(content:"hello")` without ref → `@paste "hello"` and report `dispatched_to:"@type-text -> @paste fallback"` in trace_summary.
- [ ] Tests: hover (no GUI change), type with ref (AX value set), type without ref (paste buffer used).

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
