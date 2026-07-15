# ADR-0002: `@computer-act` surface shape and scope

`@computer-act` is the only new entry point the agent loop needs to learn. The
13 Mano-CUA daemon-side actions (`open_app`, `open_url`, `click`, `doubleclick`,
`triple_click`, `right_single`, `hover`, `type`, `hotkey`, `hotkey_click`,
`scroll`, `drag`, `wait`) all flow through it. The 3 control-flow signals
(`finish()`, `stop(reason='')`, `call_user()`) never reach rdog — the client
parses them out of the model's XML output and exits its own loop before sending
`@computer-act`.

Wire format is **client-side**: the rdog protocol stays plain JSON
(`{"schema":"rdog.computer-act.v1","action":"...","args":{...}}`). VLM-specific
parsing rules (Mano-CUA XML, Holo 3.1 Qwen3-Coder, EvoCUA free JSON) live in the
client / orchestrator, not in the daemon. This avoids dragging model edge cases
into the rdog protocol and keeps `rdog` as a "trusted control bridge to a
daemon" rather than a model adapter.

## Status

Accepted.

## Considered Options

- **A.** Add 16 native primitives, one per Mano-CUA verb. Rejected: duplicates
  `@click` / `@key` / `@mouse-move`; contradicts the "do not replace existing
  commands" rule in `rdog-computer-use-density-plan.md`.
- **B.** Adapter layer entirely client-side, zero rdog changes. Rejected: 1
  Mano-CUA action becomes 3-5 rdog round-trips, kills the density goal.
- **C.** Single `@computer-act` accepting rdog-owned JSON. ✅
- **α vs β wire format**: client parses to rdog JSON (α) ✅ vs rdog consumes
  model XML directly (β). β rejected — risks pulling upstream parser bugs
  (cf. mlx-lm `qwen3_coder._function_regex` `$`-anchor trap) into rdog.
- **P1 / P2 / P3 scope**: cover all 16 (P1) / only 13 daemon-side (P2) ✅ /
  split into `@computer-act` + `@computer-signal` (P3). P2 chosen because
  `finish / stop / call_user` are agent-loop signals, not daemon actions.

## Consequences

- One new command name to teach clients and benchmarks.
- `finish / stop / call_user` never appear in `@computer-act` requests or
  responses; the client's dispatch table handles them before any daemon call.
- Schema version is `rdog.computer-act.v1`. Future CUA models that need a
  different shape (e.g. Holo 3.1 three-tool subset) can introduce
  `rdog.computer-act.holo.v1` without breaking v1.
