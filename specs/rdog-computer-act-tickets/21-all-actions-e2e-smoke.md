# 21 — All 13 actions end-to-end smoke

**What to build:** One CLI smoke script per action (or one combined) that exercises `@computer-act` for every action end-to-end against a real daemon, asserting behavior on a real or synthetic input. This is the integration-level proof that the protocol works for the full Mano-CUA action set.

**Blocked by:** 06 through 18 (all dispatcher + verify + error + observability tickets complete)

**Status:** ready-for-agent

- [ ] Smoke script `scripts/smoke_computer_act_all.sh` covers all 13 actions in sequence, each with a deterministic prompt / screenshot / input fixture.
- [ ] All smokes exit 0.
- [ ] Smoke is runnable locally on macOS without external network.
- [ ] Smoke script is wired into CI / smoke collection.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
