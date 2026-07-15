# 02 — @open-app primitive

**What to build:** Add a new rdog line-protocol command `@open-app` that opens a macOS application by name via `@cmd "open -a <app_name>"`. On non-macOS platforms returns `error_code: "platform_unsupported"` with evidence pointing at the OS. This is a foundation primitive for `@computer-act`'s `open_app` action.

**Blocked by:** None — can start immediately (cross-platform error path included)

**Status:** ready-for-agent

- [ ] Line-protocol parser accepts `@open-app#N:{app_name:"...", wait_ms:1500}` with default `wait_ms` of 1500 if absent.
- [ ] On macOS, dispatch to `@cmd "open -a <app_name>"` and return `{ok:true, dispatched_to:"@open-app", app_name:...}` on success.
- [ ] On Linux / Windows, return `{ok:false, error_code:"platform_unsupported", error_message:"@open-app is macOS-only in this round; see LATER_PLANS LP1"}`.
- [ ] Permission denied (e.g. Accessibility for shell) returns `error_code:"permission_denied"`.
- [ ] Smoke: on macOS, `@open-app Calculator` returns OK and a `ps -ef | grep Calculator` shows the process running.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
