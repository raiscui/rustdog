# 22 — Density benchmark suite (LP3)

**What to build:** A standalone benchmark script that exercises 5-10 typical Mano-CUA tasks through `@computer-act` and through the manual `@observe + @click + @observe` baseline, reporting the `density` fields per ADR-0006. This is the proof that ADR-0001's high-density promise holds in practice.

**Blocked by:** 21 (e2e smoke must pass)

**Status:** ready-for-agent

- [ ] Benchmark script `scripts/bench_computer_act_density.py` (or `.sh`) covers 5-10 tasks: form submit, login flow, browser search, file open + save, multi-step dialog, scroll-and-click, etc.
- [ ] Each task runs once with `@computer-act` and once with the manual baseline; both report elapsed_ms_total, backend_request_count, semantic_action_count.
- [ ] Benchmark output is a JSON report at `docs/benchmarks/rdog-computer-act-density-<date>.md` (Markdown with embedded JSON).
- [ ] The benchmark's median `@computer-act` round-trip count is provably lower than the manual baseline for at least 80% of tasks.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
