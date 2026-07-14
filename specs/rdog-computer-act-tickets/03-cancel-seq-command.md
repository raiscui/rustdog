# 03 — @cancel#seq parser and runtime cancel logic

**What to build:** Add a new rdog line-protocol command `@cancel#seq:{target_seq:N}` that signals an in-flight line-protocol request to abort. The cancelled request receives a final response tagged `error_code:"cancelled"`. Mechanism: per-request cancellation token (atomic bool) checked at every await point in the dispatcher.

**Blocked by:** None — can start immediately

**Status:** ready-for-agent

- [ ] Line-protocol parser accepts `@cancel#seq#M:{target_seq:N}` and rejects when target_seq is unknown / already completed.
- [ ] Runtime: any in-flight dispatcher step (mouse held, sleep thread, @observe wait) checks the cancellation token at least every 50 ms.
- [ ] Cancelled request's response carries `error_code:"cancelled"`, `error_message:"cancelled by @cancel#seq#M"`, `evidence:{cancelled_at_step:"..."}`.
- [ ] Mouse / keyboard state is restored cleanly — no stuck mouse-down after cancel.
- [ ] Unit tests: cancel during `@wait`, cancel during `@key` with held modifier, cancel on already-completed request.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
