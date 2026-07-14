# 17 — Density metrics on every response

**What to build:** Populate the `density` block on every successful `@computer-act` response. Field names are shared with `@gui-probe` per ADR-0006. `@computer-act`-specific fields are `implicit_observe`, `implicit_observe_ms`, `dispatch_ms`, `verify_ms`.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Response carries `density:{backend_request_count:1, control_frame_count:1, elapsed_ms_total, semantic_action_count:1, mouse_fallback_count, stale_ref_recovery_count, verification_passed, false_success_count, payload_bytes, trace_step_count, implicit_observe, implicit_observe_ms, dispatch_ms, verify_ms}`.
- [ ] `verification_passed` is true iff `verify` was not `none` and the AX diff was non-empty.
- [ ] Field names match ADR-0006 §'Consequences'.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
