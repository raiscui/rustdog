# 19 — @flow policy.allow_computer_act opt-in + ControlLine parsing

**What to build:** Update `@flow` v1 schema to allow `@computer-act` as a `ControlLine` step. Add `policy.allow_computer_act:bool` (default false). Update `flow.process` to recognize and route `@computer-act` control lines.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Flow schema validator accepts `policy.allow_computer_act:true` and rejects `@computer-act` control lines when the flag is false.
- [ ] When `@computer-act` runs as a flow step, its full response is captured in flow state and surfaced as the step result.
- [ ] Existing flow tests continue to pass.
- [ ] New flow smoke: `policy.allow_computer_act:true` + ControlLine + Expect.ok → flow succeeds.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
