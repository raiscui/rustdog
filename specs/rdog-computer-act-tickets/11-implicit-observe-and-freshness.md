# 11 — implicit_observe plumbing + freshness + TTL

**What to build:** Implement `implicit_observe`: when the request has `start_box` and no `target.ref`, run an internal `@observe` to resolve the coordinate to a ref before dispatch. Track the resulting `observation_id` and return it. Implement `observation_used.freshness` three-state (`fresh` / `stale_re_observed` / `stale_fallback_to_coords`). Enforce 5-second TTL on observation_id.

**Blocked by:** 05 (skeleton must exist)

**Status:** ready-for-agent

- [ ] Request with `start_box` and no `target.ref` triggers implicit observe; response carries `observation_id` and `observation_used.freshness`.
- [ ] Request with `target.ref + observation_id` valid → `freshness:"fresh"`, no re-observe.
- [ ] Request with `target.ref` but expired `observation_id` → daemon re-observes internally, returns new id in `observation_used.re_observe_id`, tags `freshness:"stale_re_observed"`.
- [ ] Request with expired ref and no coords → `stale_fallback_to_coords` is unreachable (no fallback to coords); instead, daemon re-observes and the response is `stale_re_observed` with a clear `evidence`.
- [ ] TTL enforced: after 5 seconds, observation is marked stale.
- [ ] Tests: explicit clock manipulation to verify TTL boundary; explicit fresh-ref path; explicit stale-ref path.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
