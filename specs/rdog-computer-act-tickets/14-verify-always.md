# 14 — Verify tier 'always' (full observation)

**What to build:** Implement the strongest verify tier: `verify:"always"` runs a full observation (screenshot, AX tree, window state) after the action. Returns the full `observation` block plus `verification.ax_diff`.

**Blocked by:** 13

**Status:** ready-for-agent

- [ ] Request with `verify:"always"` triggers post-action `@observe` with screenshot.
- [ ] Response carries `verification.method:"full"`, `verification.observation:{screenshot_id, ax_tree_id, windows:[]}` and `verification.ax_diff`.
- [ ] If the screenshot is too large (>2 MB), server reports `verification.observation.screenshot_truncated:true` rather than dropping it.
- [ ] Response top-level `outcome: "worked" | "didnt" | "unknown"` (feature/computer-act-outcome-3state). 三态由 verify=always 决策: `didnt` 表示 full observe 没观察到变化 (`verification.ax_diff` 全 0 + `verification.status: "failed"`); `worked` 表示观察到了 modified > 0; `unknown` 表示 observe timeout / cancel. `verification.status` 跟 ticket 13 共用同一套 ax_diff 决策.
- [ ] Test: synthetic fixture that asserts all three observation sub-fields are present.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
