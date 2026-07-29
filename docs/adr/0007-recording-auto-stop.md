# ADR 0007: Recording auto-stop timer + lifecycle integration

> Status: accepted. Issue #23.

## Context

`rdog record start --duration <X>` is the operator-facing way to
record a fixed-length session. Without a daemon-side auto-stop, an
owner who forgets to issue `@record-stop` leaves a recording session
open indefinitely. The recorder would keep capturing events into the
journal, the bundle would never be committed, and `last_session` would
never be populated.

The existing manual `@record-stop` already commits the bundle and
populates `last_session`. We need an auto-stop that runs the same
path automatically.

## Decision 1 — Use a dedicated worker thread, not a daemon tick

A 100 ms polling worker thread is acceptable because:

- It is local to `RecordingHandler` and does not require a global
  daemon task scheduler.
- The polling thread is bounded by `duration_ms`; it exits within
  ≤ 100 ms after `cancel` / `stop` / `Drop`.
- Adding a daemon tick would couple recording to the main loop; we
  defer that.

## Decision 2 — Three-state atomic flag, not a channel

`Arc<AtomicU8>` with `PENDING` / `CANCELLED` / `FIRED` is sufficient
because:

- The flag is the only cross-thread signal.
- `Ordering::Acquire` on the load is enough since the worker thread
  does not call back into the handler.
- A channel would require the worker to send a message and the
  handler to receive it, which adds a lock and a queue.

## Decision 3 (3a) — Auto-stop runs inline in the next handler call

The worker thread does NOT call into the recording handler. It only
writes the flag. The auto-stop itself runs inline at the start of
the next `RecordingHandler::handle` call.

This avoids a classic lock deadlock: if the worker tried to lock the
handler while the owner was holding it inside `stop` / `cancel`, the
`stop` / `cancel` would block on `join` waiting for the worker to
finish, while the worker would block on `lock` waiting for the owner
to release.

The next handler call is always observed within the daemon's request
loop, so the auto-stop fires within one tick of the deadline.

## Decision 4 — `0` duration means "no auto-stop"

`--duration 0s` is treated the same as omitting `--duration`. No
timer is spawned. This avoids a corner case where the worker would
fire immediately and the test for `duration_too_small_rejected`
would also need to reject `0`.

## Decision 5 — `last_session_override` on the handler

`RecordingHandler` keeps its own `last_session_override:
Option<TerminalSummary>` field. The `status` method prefers it over
`LifecycleManager::last_session`. This lets the handler attach a
`StopTrigger` to the summary without modifying the manager's API
(issue #23 acceptance).

## Decision 6 — Validation range `[100, 3_600_000]` ms

Reject durations below 100 ms (`4121 DURATION_TOO_SMALL`) and above
60 minutes (`4120 DURATION_TOO_LARGE`). Rationale:

- 100 ms is the tick granularity; shorter durations would race the
  worker poll.
- 60 minutes is the upper bound for "auto-stop without a budget".
  Longer recordings should be split into segments.

## Decision 7 (3a) — 100 ms tick poll, not blocking sleep

The worker computes `deadline = now + duration_ms` and sleeps at most
100 ms per iteration. This means `cancel` / `stop` is observed within
≤ 100 ms regardless of `duration_ms`.

## Decision 8 — Auto-stop respects handler-side lock

The auto-stop path (`auto_stop_internal`) takes the same lock as
`stop` (because it runs inside `handle`). This guarantees:

- `complete_current` is called exactly once.
- The bundle is committed atomically.
- `last_session_override` is updated before the next request returns.

## Consequences

- New dependency: `std::thread::JoinHandle`. Already in `std`.
- Existing 690 tests remain green; +7 new tests added.
- E2E smoke documented in `specs/rdog-acceptance-matrix.md`.
- `@record-stop` adds an optional `trigger` field; old clients ignore
  it.
