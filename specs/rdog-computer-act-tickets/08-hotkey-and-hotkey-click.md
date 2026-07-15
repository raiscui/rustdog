# 08 — Hotkey + hotkey_click composite

**What to build:** Add `hotkey` and `hotkey_click` to the dispatcher. `hotkey` routes directly to `@key`. `hotkey_click` is a composite: press modifier, click target, release modifier — implemented as three sequential dispatcher calls.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `hotkey(key:"Cmd+C")` → `@key "Cmd+C"`.
- [ ] `hotkey_click(start_box:[x,y], key:"shift")` → `@key down` + `@click {x,y}` + `@key up`.
- [ ] `trace_summary` shows three dispatch entries (one per sub-step) for `hotkey_click`, single entry for `hotkey`.
- [ ] If the click step errors after the modifier is pressed, the modifier is released before returning the error.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
