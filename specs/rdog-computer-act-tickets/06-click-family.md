# 06 — Click family: click / doubleclick / triple_click / right_single

**What to build:** Add the four click variants to the dispatcher. All four route to `@click` with appropriate `count` and `button` parameters.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `@computer-act#N:{action:"click",args:{start_box:[x,y]}}` routes to `@click {x,y, button:"left", count:1, hold_ms:80}`.
- [ ] `doubleclick` → `@click {count:2, interval_ms:120}`.
- [ ] `triple_click` → `@click {count:3, interval_ms:120}`.
- [ ] `right_single` → `@click {button:"right", count:1}`.
- [ ] Tests: one per variant, asserting the correct `@click` parameters are emitted.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
