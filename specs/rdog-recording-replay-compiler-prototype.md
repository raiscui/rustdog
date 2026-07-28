# rdog Recording Replay Compiler Prototype (ticket #8)

## Status

This is the prototype resolution asset for Wayfinder ticket
[原型验证 Recording Journal 到 rdog.flow.v1 的确定性编译](https://github.com/raiscui/rustdog/issues/8)
and its parent map
[录制操作并生成可回放的 rdog control 脚本](https://github.com/raiscui/rustdog/issues/2).

It demonstrates a minimal viable compiler that turns a `rdog.recording.v1`
Recording Journal into a `rdog.flow.v1` Replay Script. The prototype
proves determinism: identical journal bytes + fixed compiler version
produce byte-equal canonical flow.json.

## Scope

In scope:

- A standalone Rust binary `replay-compiler` with two CLI commands:
  `compile` and `check-determinism`.
- A 10-pass optimization pipeline covering all 10 ticket-listed items,
  each backed by at least one fixture case.
- A fixture `tests/fixtures/replay-compiler/journal_optimizations.jsonl`
  that triggers every pass except `shortcut_hotkey` (no multi-key chord
  in fixture; covered by stub pass definition).
- Integration tests in `tests/recording_replay_compiler.rs` covering
  determinism, pass coverage, provenance consistency, semantic
  promotion suppression, and redacted parameter emission.

Out of scope (deferred to production compiler):

- Multi-key chord detection (Cmd+Key, Ctrl+Key, Alt+Key).
- Scroll wheel delta aggregation.
- Window geometry propagation across multiple participating windows.
- Coordinate-only fallback when semantic re-find fails.
- Recording Bundle provenance check (covered separately in
  `specs/rdog-replay-preflight-guard-verification.md`).
- Daemon-side flow runtime integration (covered in
  `specs/rdog-flow-control-plan.md`).

## Layout

```
src/bin/replay_compiler.rs                                # standalone binary
tests/recording_replay_compiler.rs                        # integration tests
tests/fixtures/replay-compiler/
  journal_optimizations.jsonl                              # 28-event fixture
  flow_optimizations.json                                  # generated golden output
```

## Determinism contract

Same input journal bytes + same compiler version produces byte-equal
canonical flow.json output. The compiler enforces this by:

- `serde_json` compact mode (no indent).
- Sorted object keys at every level via `BTreeMap` canonicalization.
- Paired `(event, original_index)` propagation through every pass so
  that the `journal_index_range` in `source_provenance` carries the
  exact source journal indices even after passes drop events.
- No `HashMap` iteration; no `SystemTime`; no `Instant::now()`.

Verification:

```bash
cargo run --bin replay_compiler -- check-determinism \
    --input tests/fixtures/replay-compiler/journal_optimizations.jsonl
# expected: "determinism ok: <N> bytes"
```

## Optimization pipeline

The compiler runs the following passes in order. Each pass takes paired
`(event, original_index)` and returns paired events in the same order,
possibly dropping entries.

| # | Pass | Impl | `changes_semantics` |
| --- | --- | --- | --- |
| 1 | `debounce` | full | no |
| 2 | `mouse_move_coalesce` | full | no |
| 3 | `scroll_coalesce` | stub | no |
| 4 | `text_merge` | emit-time | no |
| 5 | `shortcut_hotkey` | stub | no |
| 6 | `sleep_mark` | emit-time | no |
| 7 | `semantic_promotion` | full | yes |
| 8 | `coordinate_fallback` | emit-time | yes |
| 9 | `window_precondition` | emit-time | yes |
| 10 | `redacted_parameter` | emit-time | yes |
| 11 | `source_provenance` | always | no |

`changes_semantics:true` means the pass can drop or alter the action
that the runtime would execute. The compiler itself decides whether to
emit a coordinate vs. semantic action and whether to redact a value.
For preflight, see
`specs/rdog-replay-preflight-guard-verification.md`.

`text_merge`, `sleep_mark`, `coordinate_fallback`, `window_precondition`,
`redacted_parameter`, and `source_provenance` are implemented at emit
time inside `emit_steps` rather than as separate passes. This keeps the
prototype under 700 lines while still demonstrating each optimization's
effect on the output.

## Pass semantics

### 1. `debounce`

Collapses identical consecutive key events within a 50 ms window. The
prototype does not detect larger-character compositions (e.g. `Shift+a`
vs `A`); that is deferred to production.

### 2. `mouse_move_coalesce`

Keeps only the last consecutive `MouseMove` event until the next
non-`MouseMove` event. This does not affect clicks or drags because
those events break the run.

### 3. `scroll_coalesce` (stub)

Sum consecutive `Scroll` deltas into a single event. Deferred.

### 4. `text_merge`

Merge consecutive printable, non-redacted keys into a single `TypeText`
step. The merge runs at emit time and walks forward while
`is_printable_key` is true.

### 5. `shortcut_hotkey` (stub)

Detect multi-key chords. Deferred.

### 6. `sleep_mark`

Emit a `Sleep` step for each `Mark { redaction_active: true }`. Other
marks are dropped from output.

### 7. `semantic_promotion`

Drop a `Click` event if it occurs within 100 ms after an `AxPress`.
This is a conservative implementation: real semantic promotion uses
`specs/rdog-recording-semantic-promotion-policy.md` rules with locator
match and ownership checks.

### 8. `coordinate_fallback`

Emit a coordinate `Click` step when no `AxPress` is available. Real
fallback uses `specs/rdog-recording-semantic-promotion-policy.md`
9-gate check.

### 9. `window_precondition`

Emit `WindowResize` from `WindowGeometry` events. Production compiler
must chain multiple geometry events into a precondition sequence per
`specs/rdog-recording-window-geometry-policy.md`.

### 10. `redacted_parameter`

For `AxValue { redacted: true }`, attach `parameter:"typed_text"` and
keep the value as a placeholder. Real redaction uses canonical
descriptors from `specs/rdog-recording-redaction-parameter-model.md`.

### 11. `source_provenance`

Every emitted step carries a `SourceProvenance { journal_index_range,
pass }` entry. The `journal_index_range` always refers to the source
journal, not the post-pass event indices.

## Fixture

`tests/fixtures/replay-compiler/journal_optimizations.jsonl` covers
these cases in 28 events:

- Window geometry precondition (`WindowGeometry`).
- Semantic promotion suppression (click after `AxPress#username`).
- Text merge of 5 printable keys into one `TypeText "abcde"`.
- Redacted key burst (`p`, `a`, `s` with `redacted:true`).
- Single `s` key text merge.
- Mouse move coalesce (3 moves -> 1).
- Coordinate fallback click.
- Two scrolls (no coalesce, stub).
- `sleep_mark` from redaction boundary.
- `ax_value` with redacted parameter (`hunter2` placeholder).
- Final `AxPress#Submit`.
- Debounce collapse of `x`, `x`, `y` into `TypeText "xy"`.

## Test coverage

`tests/recording_replay_compiler.rs` runs six tests:

- `fixture_journal_parses`: smoke compile.
- `determinism_byte_equal`: two compilations produce byte-equal bytes.
- `all_optimization_passes_represented`: every required pass name
  appears in some step's `provenance.pass`.
- `provenance_journal_index_ranges_are_consistent`: every range lies
  within the source journal bounds and is non-empty.
- `semantic_promotion_suppresses_coordinate_click`: only one `Click`
  step survives (the one not preceded by `AxPress`).
- `redacted_ax_value_carries_parameter_descriptor`: the password
  `AxValue` carries `parameter:"typed_text"`.

Run:

```bash
cargo test --test recording_replay_compiler
```

## Limitations

- No multi-key chord (`KeyChord` step variant exists but never emitted).
- No scroll aggregation (`Scroll` step variant preserved as-is).
- Window geometry emits only one `WindowResize` per snapshot; real
  compiler must chain multiple snapshots into a sequence.
- No recording bundle provenance check; production compiler must
  validate `manifest.compiler.version` (ticket `#4`).
- `JournalEvent` is a minimal subset of `rdog.recording.v1`; the
  prototype does not parse all journal event kinds.

## Open work for production

- Implement `scroll_coalesce` aggregation.
- Implement `shortcut_hotkey` chord detection.
- Replace stub `WindowGeometry` emission with full
  `WindowPrecondition` sequencing.
- Add integration with `rdog.flow.v1` runtime (`@flow` execution).
- Wire `manifest.compiler.version` validation into preflight.
