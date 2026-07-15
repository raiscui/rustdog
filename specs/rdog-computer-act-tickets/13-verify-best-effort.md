# 13 — Verify tier 'best_effort' (AX diff)

**What to build:** Implement the middle verify tier: `verify:"best_effort"` runs an AX-tree diff between pre-action and post-action states and returns the changed / added / removed refs. No screenshot. Lightweight.

**Blocked by:** 11 (implicit_observe plumbing must exist), 12

**Status:** ready-for-agent

- [ ] Request with `verify:"best_effort"` triggers an internal post-action AX-tree scan, diffs against the prior AX tree, returns `verification.method:"ax_diff"`, `verification.ax_diff:{added, removed, changed}`.
- [ ] No screenshot is captured; `observation_used.freshness` reflects only the pre-action implicit_observe.
- [ ] `density.verify_ms` reports the AX-diff cost separately from `density.dispatch_ms`.
- [ ] Test: synthetic AX-tree diff fixture; verify the diff shape and the omitted screenshot field.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
