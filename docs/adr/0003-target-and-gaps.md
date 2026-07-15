# ADR-0003: Target representation and primitive gap audit

`@computer-act` accepts both Mano-CUA-style normalized coordinates
(`{"start_box":[499,565]}`, `[0, 1000]` space) **and** rdog-native AX refs
(`{"target":{"ref":"@e1","observation_id":"obs-789"}}`). When a coordinate is
given, the daemon runs an implicit `@observe` to resolve the pixel to an
element ref before dispatching; when a ref is given, the daemon validates the
ref against the observation and may re-observe if expired.

Of the 13 Mano-CUA actions, 9 already have direct rdog equivalents. The 4
gaps are closed as follows:

| Gap | Closing strategy | Reasoning |
|---|---|---|
| `open_app` | **Add `@open-app` primitive** | Real OS abstraction; cross-platform; useful beyond Mano-CUA. |
| `open_url` | **Fold**: dispatch to `@cmd "open <url>"` | Equivalent to existing shell idiom; no new primitive. |
| `hotkey_click` | **Fold**: dispatch to `@key down` + `@click` + `@key up` | Composite, not atomic; trace stays explicit. |
| `wait` | **Add `@wait` primitive** | Generic sleep; useful in `@flow` and other contexts. |

`@computer-act` itself stays a thin dispatcher in all four cases — no OS
adaptation logic is hidden inside it.

## Status

Accepted.

## Considered Options

- **S1 / S2 / S3 target**: coords-only / refs-only / hybrid (S3) ✅.
  S1 maximizes density but adds implicit observe to every call (slow for
  typing/scrolling). S2 matches rdog style but kills density. S3 lets the
  agent pick per turn.
- **G1 / G2 / G3 gap handling**: four new primitives / fold everything /
  classify by nature (G3) ✅. G1 bloats the command space; G2 forces
  cross-platform logic into the dispatcher (violates "rdog = control bridge"
  abstraction).

## Consequences

- Two new commands added to rdog: `@open-app` and `@wait`. Both follow the
  existing line-control frame conventions.
- `open_url` and `hotkey_click` show up in `@computer-act` trace as
  multi-step entries (e.g. `dispatch → @cmd` or
  `dispatch → @key + @click + @key`), giving full transparency without new
  primitives.
- The implicit_observe step (when start_box is used) appears in the response's
  `density.implicit_observe_ms` field — see ADR-0006.
