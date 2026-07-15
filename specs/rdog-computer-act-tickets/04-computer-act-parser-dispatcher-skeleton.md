# 04 — @computer-act line-protocol parser + dispatcher skeleton

**What to build:** Add the `@computer-act` line-protocol command and a minimal dispatcher that routes parsed actions to underlying primitives. This ticket delivers the parser, the action→primitive routing table, the request envelope validation, and the response envelope skeleton — but **no** implicit_observe, **no** verify tier, **no** error envelope (those come in tickets 11–15). Skeleton returns `{ok:true, dispatched_to:..., duration_ms:...}` for any valid request whose action is wired up.

**Blocked by:** 01 (wait primitive must exist for default-timeout table), 02 (open-app primitive must exist for action dispatch)

**Status:** ready-for-agent

- [ ] Line-protocol parser accepts `@computer-act#N:{...}` with top-level `schema:"rdog.computer-act.v1"`, `action`, `args`.
- [ ] Action routing table covers all 13 daemon-side actions; unknown actions return `error_code:"unknown_action"`.
- [ ] Default timeout table is wired up (ticket 16 fills in the values; this ticket just calls the lookup function).
- [ ] Response envelope has all fields a later ticket will populate, marked `null` or absent: `observation_id`, `verification`, `observation_used`, `density`, `trace_summary`, `trace_savefile`.
- [ ] Tests: 13 routing smoke tests (one per action), each asserting the underlying primitive is called with the correct arguments.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
