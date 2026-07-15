# 01 — @wait primitive

**What to build:** Add a new rdog line-protocol command `@wait` that sleeps the dispatcher's worker thread for a duration and returns an OK response with the actual elapsed time. This is a foundation primitive — `@computer-act`'s `wait` action will route here in ticket 05.

**Blocked by:** None — can start immediately

**Status:** ready-for-agent

- [ ] Line-protocol parser accepts `@wait#N:{duration_ms:N}` and rejects negative / non-numeric values with a parse error.
- [ ] Runtime sleeps for the requested duration (within 50 ms tolerance on macOS) and returns `{ok:true, dispatched_to:"@wait", duration_ms:<actual>}`.
- [ ] Unit test in `control_protocol/tests/` covers: valid request, missing duration, negative duration, malformed JSON.
- [ ] Smoke script `scripts/smoke_wait.sh` invokes real daemon and prints OK response within tolerance.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
