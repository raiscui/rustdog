# 15 — Error envelope E2 + retry strategies

**What to build:** Implement the `E2` error envelope (`error_code` + `error_message` + `retry.strategy` + `retry.hint` + `evidence`) and the strategy enumeration. Wire error_code taxonomy for at minimum: `permission_denied`, `observation_expired`, `target_not_found`, `verify_failed`, `invalid_args`, `platform_unsupported`, `unknown_action`, `infrastructure`, `cancelled`, `timeout`.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Error response shape matches ADR-0004 §'Considered Options E2'.
- [ ] Each error_code carries the documented `retry.strategy`.
- [ ] `verify_failed` populates `evidence.verification.ax_diff` showing the missing change.
- [ ] `permission_denied` populates `evidence.missing_capability` (one of `accessibility`, `screen_recording`, `window_server`).
- [ ] Tests: one per error_code, each asserting the strategy and key evidence field.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
