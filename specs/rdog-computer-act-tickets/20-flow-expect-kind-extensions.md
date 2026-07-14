# 20 — @flow Expect.kind extensions for structured response fields

**What to build:** Add two new `Expect.kind` members: `response_field_equals(path, value)` and `response_path_contains(path, substring)`. These let flow steps assert on structured response fields (e.g. `verification.passed`, `observation_id`, `error.error_code`) instead of stdout substring match.

**Blocked by:** 19

**Status:** ready-for-agent

- [ ] Flow schema accepts the two new `Expect.kind` values.
- [ ] `response_field_equals` accepts JSON-pointer-like paths (e.g. `$.verification.passed`).
- [ ] `response_path_contains` does substring match on the path's stringified value.
- [ ] Smoke: flow with `@computer-act` ControlLine + `Expect.response_field_equals:$.ok:true` exits 0; same flow with `$.ok:false` exits non-zero.

**References:** ADR-0001 (meta) / ADR-0002 (surface & scope) / ADR-0003 (target & gaps) / ADR-0004 (contract) / ADR-0005 (lifecycle) / ADR-0006 (integration & observability). Read the ADR sections that match this ticket's scope before implementing.

**Spec:** `specs/rdog-computer-act-spec.md` (read alongside this ticket).
