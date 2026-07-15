# 05 — Minimum vertical slice: @computer-act for wait + open_app end-to-end

**What to build:** Wire `@computer-act` end-to-end for `wait` and `open_app` only — the two simplest actions. Includes the dispatcher table, the request parsing, the response shape, and a CLI smoke script that demonstrates the slice working through real daemon. No implicit_observe, no verify tier yet, no error envelope.

**Blocked by:** 04 (@computer-act skeleton must exist)

**Status:** ready-for-agent

- [ ] `@computer-act#1:{action:"wait",args:{duration_ms:100}}` returns `{ok:true, action:"wait", dispatched_to:"@wait", duration_ms:~100}`.
- [ ] `@computer-act#2:{action:"open_app",args:{app_name:"Calculator"}}` returns `{ok:true, action:"open_app", dispatched_to:"@open-app", ...}` on macOS.
- [ ] Smoke script `scripts/smoke_computer_act_min.sh` runs both back-to-back and exits 0.
- [ ] Minimum response shape matches ADR-0004 §'Considered Options T3' top-level fields (without verify / observation_id / density / trace yet).

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
