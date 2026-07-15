# 10 — open_url folded to @cmd

**What to build:** Add `open_url` to the dispatcher. As decided in ADR-0003, this folds to `@cmd "open <url>"` on macOS rather than getting its own primitive. On non-macOS platforms returns `platform_unsupported`.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] `@computer-act#N:{action:"open_url",args:{url:"https://example.com"}}` → `@cmd "open https://example.com"` on macOS.
- [ ] On Linux / Windows, returns `platform_unsupported` (LP1 will fill in).
- [ ] `trace_summary` shows `dispatch → @cmd`.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
