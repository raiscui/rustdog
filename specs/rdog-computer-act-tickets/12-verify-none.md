# 12 — Verify tier 'none'

**What to build:** Implement the simplest verify tier: `verify:"none"` means no observation is run for verification, response carries no `verification` block. This is the default if `verify` field is omitted.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Request without `verify` field → defaults to `verify:"none"`.
- [ ] Response has no `verification` key.
- [ ] `density.implicit_observe` is false (no implicit_observe unless coords path needs it for ref resolution).

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
