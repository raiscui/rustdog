# rdog-recording-auto-stop

> Issue #23 — Recording: daemon-side auto-stop timer + lifecycle integration.

## Goal

When an owner issues `rdog record start --duration <X>`, the daemon
must end the recording automatically after `X` elapses, producing the
same bundle + response as a manual `@record-stop`. The owner may still
issue `@record-stop` or `@record-cancel` early; the auto-stop must
back off in that case.

## User stories

1. As an owner, I can run `rdog record start --duration 30s self` and
   walk away; 30 s later the bundle is committed and the next
   `rdog record status self` reports `phase: completed` with
   `stop_trigger: auto_duration`.
2. As an owner, when I cancel a recording before the deadline, the
   auto-stop does not fire — the journal lands in `cancelled` state
   with `stop_trigger: manual`.
3. As an owner, when I manually stop before the deadline, the bundle
   is committed with `stop_trigger: manual`; the auto-stop timer is
   released and exits within one tick (≤ 100 ms).
4. As an owner, I can call `start` without `--duration`; the recording
   stays open until I manually stop.
5. As an owner, I get a clear error for `--duration 50ms` (`4121
   DURATION_TOO_SMALL`) and for `--duration 100m` (`4120
   DURATION_TOO_LARGE`).
6. As an owner, when the recording session is auto-stopped, the
   `@record-stop` response shape is unchanged; a new `trigger` field
   indicates which mechanism ended the session.
7. As an owner, when the daemon is shut down while a recording is
   active, the auto-stop is dispatched (or the normal shutdown path
   runs); the daemon does not leave dangling threads.
8. As an integrator, the auto-stop is observable via the
   `rdog.recording.v1` `last_session.stop_trigger` field in
   `@record-status`.
9. As an operator, the `RecordingHandler` lifecycle respects the
   handler-level lock so the auto-stop never races with manual
   stop/cancel.
10. As an owner, `0` is treated as "no duration" — the timer is not
    spawned.
11. As an owner, the bundle path is reported in the same way as a
    manual `@record-stop`; downstream tooling does not need to
    distinguish between manual and auto-stop.
12. As an owner, the auto-stop path remains fail-closed: if the
    bundle commit fails, the session transitions to `Failed` rather
    than silently succeeding.
13. As an owner, the joinable thread does not block the handler — the
    next non-`start`/`stop`/`cancel` request observes the fired flag
    inline and runs the auto-stop path.
14. As an owner, the existing test suite (697 tests) stays green.

## Implementation decisions

1. **Storage**: `RecordingHandler` gains an `auto_stop_timer:
   Option<AutoStopTimer>` field. The struct holds `flag: Arc<AtomicU8>`,
   `join: Option<JoinHandle<()>>`, `owner: ConnectionId`,
   `recording_id: String`.

2. **State machine**: a single `Arc<AtomicU8>` flag with three states
   — `0 = PENDING`, `1 = CANCELLED`, `2 = FIRED`. `stop` / `cancel` /
   `Drop` write `CANCELLED`; the worker thread performs a CAS from
   `PENDING` to `FIRED` at the deadline.

3. **Tick poll**: the worker thread polls the flag every 100 ms,
   sleeping at most the remaining time. This lets `cancel` / `stop`
   interrupts be observed within at most one tick (≤ 100 ms) instead
   of waiting for the full duration.

4. **Auto-stop dispatch**: the worker thread does NOT call back into
   the recording handler. It only writes the flag. The auto-stop
   itself runs inline at the start of the next `RecordingHandler::handle`
   call, after acquiring the existing lock. This avoids the lock
   deadlock that would otherwise occur if the worker tried to lock the
   handler while the owner was holding it inside `stop` / `cancel`.

5. **Bundle commit**: the auto-stop path is the same as `stop`'s body
   minus the response framing — `begin_finalize` → `write_bundle` →
   `complete_current` → `StopTrigger::AutoDuration` is applied to the
   resulting `TerminalSummary`.

6. **Cancellation behavior**: `cancel_auto_stop_timer` sets the flag
   to `CANCELLED` and joins the worker thread. The worker thread
   exits within at most one tick (≤ 100 ms) after the flag flip
   because the sleep is bounded by 100 ms.

7. **Drop ordering**: `Drop for RecordingHandler` calls
   `cancel_auto_stop_timer` so the worker thread is joined before the
   `lifecycle` field is dropped. The worker holds only `Arc<AtomicU8>`
   so the join can always succeed.

8. **Duration validation**: `[100, 3_600_000]` ms inclusive. `0` is
   treated as "no duration" (no timer spawned). Negative numbers
   are rejected by the humantime parser (issue #22).

9. **Atomic ordering**: `Ordering::Acquire` on the load, `AcqRel` on
   the CAS. This is sufficient because the flag is the only cross-thread
   signal — the bundle itself is committed by the lock-holding thread.

10. **Status visibility**: `last_session_override` is the handler-side
    truth; `status` prefers it over `LifecycleManager::last_session`.
    Without this, the mutating `last_session` from
    `LifecycleManager::complete_current` would be unmodifiable without
    changing the manager's API.

## Wire changes

- `@record-start` response: add `duration_ms: Option<u64>` field
  (already added in issue #22).
- `@record-stop` response: add `trigger: Option<&str>` field —
  `"manual"` or `"auto_duration"`. Old clients ignore unknown fields.
- `@record-status` response: when `last_session` is present, include
  `stop_trigger: Option<&str>`.
- `@record-status` response (active session): include `duration_ms:
  Option<u64>` and `remaining_ms: Option<u64>` when an auto-stop timer
  is active. `remaining_ms` is computed from
  `Instant::now() - started_at` against `duration_ms` and is clamped
  to 0 once the deadline has passed. When no timer is configured
  (`--duration` omitted), both fields are null.

## Out of scope

- Real-time remaining-seconds polling (deferred to a follow-up ticket).
- Multi-stage duration (e.g., 10s then 5m).
- Per-stop-hook redaction pause.
- Owner-disconnect detection (the existing `try_adopt` covers
  reconnection; a separate offline disconnect detector is a follow-up).
