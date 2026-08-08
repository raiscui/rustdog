# 13 — Verify tier 'best_effort' (AX diff)

**What to build:** Implement the middle verify tier: `verify:"best_effort"` runs an AX-tree diff between pre-action and post-action states and returns the changed / added / removed refs. No screenshot. Lightweight.

**Blocked by:** 11 (implicit_observe plumbing must exist), 12

**Status:** ready-for-agent

- [ ] Request with `verify:"best_effort"` triggers an internal post-action AX-tree scan, diffs against the prior AX tree, returns `verification.method:"ax_diff"`, `verification.ax_diff:{added, removed, changed}`.
- [ ] No screenshot is captured; `observation_used.freshness` reflects only the pre-action implicit_observe.
- [ ] `density.verify_ms` reports the AX-diff cost separately from `density.dispatch_ms`.
- [ ] Response top-level `outcome: "worked" | "didnt" | "unknown"` (feature/computer-act-outcome-3state, follows pi-computer-use `ActOutcome`). `outcome: "didnt"` 表示 verify 跑了但 AX 完全没变 (`verification.status: "failed"`); `outcome: "worked"` 表示 modified > 0; `outcome: "unknown"` 表示 verify 没跑 (timeout / cancel / policy 错误). `outcome` 字段总是写入.
- [ ] `verification.status: "verified" | "preexisting" | "failed"` 由 ax_diff 决策: modified > 0 → verified; modified == 0 但 added/removed > 0 → preexisting; 全 0 → failed.
- [ ] Test: synthetic AX-tree diff fixture; verify the diff shape and the omitted screenshot field.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
