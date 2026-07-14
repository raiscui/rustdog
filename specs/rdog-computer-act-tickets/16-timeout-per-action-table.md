# 16 — Timeout per-action class table

**What to build:** Implement the action-class timeout table from ADR-0005. Default timeouts: wait = `duration_ms * 1.5 + 1000`, open_app = 10000, open_url = 10000, type = 5000, hotkey_click = 3000, drag = 5000, click family = 1500-3000, hover = 1500, scroll = 2000. Client override via `timeout_ms` field.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Each action looks up its default at parse time; client `timeout_ms` overrides.
- [ ] `wait` timeout never self-kills: `duration_ms * 1.5 + 1000` is always strictly greater than `duration_ms`.
- [ ] Timeout returns `error_code:"timeout"` with `evidence.last_step` showing which stage hit the deadline.
- [ ] Test: explicit short timeout that fires mid-action; explicit long timeout that doesn't fire on a normal action.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
