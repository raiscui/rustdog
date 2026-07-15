# ADR-0001: Add `@computer-act` meta-command for computer-use VLMs

Computer-use vision-language models (Mano-CUA 16 actions, Holo 3.1 click/write/answer, EvoCUA,
GTA1) emit action verbs at inference time. Today the only path for an external agent to drive
them through rdog is to manually compose N low-level line-control frames per turn, which
inflates round-trips, blows past the "1-2 backend requests per GUI task" goal in
`specs/rdog-computer-use-density-plan.md`, and leaves the agent loop to manage AX ref
freshness and screenshot reuse itself.

We add a single meta-command `@computer-act` that accepts a structured action JSON, dispatches
to existing `@click` / `@key` / `@mouse-move` / `@type-text` / `@ax-action` etc., bundles
implicit observation and verify when requested, and reports a unified response that the
agent loop can hand straight back to the model as "post-action state".

The 5 thematic ADRs in this folder describe the design decisions in detail:

- **ADR-0002** Surface & scope: single command, client-side XML parse, 13-action scope.
- **ADR-0003** Target & gap audit: hybrid coordinate/ref input, gap handling.
- **ADR-0004** Request / response contract: JSON shape, verify tiers, error envelope.
- **ADR-0005** Lifecycle: implicit_observe reuse, per-action timeouts, cancel.
- **ADR-0006** Integration & observability: `@flow` embedding, trace & density metrics.

## Status

Accepted (2026-07-14, after a 15-question grill session).

## Considered Options

- A. Add 16 native primitives (`@mano-click`, `@mano-open-app`, ...): one per Mano-CUA verb.
- B. Adapter layer in the agent / Pi client: parse XML and translate to existing commands.
- **C. Single meta-command `@computer-act` dispatching to existing primitives.** ✅

## Consequences

- rdog gains 1 new command (`@computer-act`) and 2 new primitive commands (`@open-app`,
  `@wait`). Existing commands are reused unchanged.
- `model_profiles.json` and `rdog-computer-use-density-plan.md` §3 metrics gain a
  `density.implicit_observe` family of fields populated for every `@computer-act` call.
- A follow-up cancel primitive (`@cancel#seq`) is added in the same round to keep the
  `wait` action first-class.
- Cross-platform support (Windows UIPI, Linux xdg-open) and additional CUA models
  (Holo 3.1, EvoCUA, GTA1) are deferred to LATER_PLANS — see
  `docs/glossary.md` and `LATER_PLANS.md` (this repository) for the entry points.
