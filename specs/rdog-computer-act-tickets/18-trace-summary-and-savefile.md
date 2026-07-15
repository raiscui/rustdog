# 18 — trace_summary inline + trace_savefile opt-in

**What to build:** Implement the 4-entry `trace_summary` (implicit_observe / ref_resolve / dispatch / verify) on every response. Add `trace:"savefile"` request field that triggers full trace dump via the existing `@savefile` mechanism; response carries `trace_savefile` path.

**Blocked by:** 17 (density metrics must exist so trace_summary can reference stage costs)

**Status:** ready-for-agent

- [ ] Every successful response carries `trace_summary:[{step, elapsed_ms, status}, ...]` with exactly 4 entries (verify step is recorded even when verify=none, with status="skipped").
- [ ] Request with `trace:"savefile"` triggers full trace dump; response has `trace_savefile:"<path>"`; without the field, `trace_savefile` is absent.
- [ ] Full trace includes implicit_observe sub-steps (screenshot_capture, ax_tree_scan, ref_resolution) and dispatch sub-steps.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
