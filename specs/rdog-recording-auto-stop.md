# rdog recording auto-stop via `--duration`

> Status: **Draft spec** (under `/to-spec`)
> ADR: [`docs/adr/0007-recording-auto-stop.md`](../docs/adr/0007-recording-auto-stop.md)

## Problem Statement

`rdog record start` 当前只支持手动 `@record-stop` 触发 Bundle 提交 + 单帧 base64 `@savefile` 投递。人类录制场景经常有"录 30 秒后自动结束"的需求 (录一段交互 demo、自动化脚本运行窗口等)。

但**任何现有 stop 触发路径都不能满足"到时间自动结束"**:
- 手动 stop 要求用户主动发命令,在录制期间人工保持在线 (容易忘、按 Ctrl-C、SSH 掉线)
- 客户端 sleep + 自动 stop 会在 client 退出时让 daemon 端的 Session 永远停在 `Recording` 状态,Bundle 永不提交,占 single-active slot

用户需要一个**最小信任的"设置一次, 准时自动收尾"** 录制入口,失败语义对齐 ticket #19 的 atomic Bundle + completed retry。

## Solution

`rdog record start` 加一个 `--duration <humantime>` 可选参数。`@record-start` 把 `duration_ms` 字段传到 daemon 端 `RecordingHandler::start`,daemon 在 `LifecycleManager` 的 session 旁边起一个独立线程 timer,到时间后**复用现有手动 stop 路径** (`begin_finalize` + Bundle commit + 单帧 base64 `@savefile` delivery),然后静默退出。

- CLI 端参数:humantime 三件套 (`30s` / `5m` / `1h`),支持小数,内部统一到毫秒
- Timer 在 daemon 端,client 可立刻 disconnect,daemon 自己跑完
- Bundle 始终落盘到 `rdog_downloads/<recording_id>.rdogrec.tar`, 跟 ticket #19 完全一致
- 响应只在 owner 当前连接上发出; owner disconnect 时走 completed retry
- Owner 提前 cancel 或手动 stop 都让 timer 立刻放弃, 不会重复触发 stop

**User-facing 调用形态**:

```text
rdog record start --duration 30s mac.lab
# 30s 后 daemon 自动 stop, Bundle 落盘,
# owner 在 30s 内 disconnect 也没事, 重连后 rdog record stop <recording_id> 读回 Bundle
```

## User Stories

1. As a CLI user recording a short interaction demo, I want to set a fixed duration when starting the recording, so that I don't have to remember to stop it manually.
2. As a CLI user who backgrounded the recording (`nohup rdog record start ... &`), I want the daemon to auto-stop the session when the duration elapses, so that I can safely exit the shell without leaving the daemon stuck in `Recording` phase.
3. As a CLI user whose SSH connection drops mid-recording, I want the daemon to keep running until the duration elapses and then auto-commit the Bundle, so that I can recover the recording on reconnect.
4. As a CLI user who realizes mid-recording that I want to stop early, I want my manual `@record-stop` to immediately cancel the pending timer, so that I don't get a surprise auto-stop after I've already stopped.
5. As a CLI user who realizes mid-recording that I want to abandon the recording, I want my manual `@record-cancel` to immediately cancel the pending timer, so that no Bundle is committed and the Session returns to idle.
6. As a CLI user trying `--duration 24h` by accident, I want the CLI to refuse the request before talking to the daemon, so that I get a clear error rather than a session I can't stop.
7. As a CLI user trying `--duration 50ms` (effectively zero), I want the CLI to refuse the request with `DURATION_TOO_SMALL`, so that I don't accidentally trigger a self-cancel race.
8. As a CLI user trying `--duration 0s`, I want the request accepted and immediately triggering the stop path, so that I can use this as a programmatic "start + auto-stop" idiom in shell scripts.
9. As a daemon operator, I want a pending duration timer to never leak past the Session's lifetime, so that a crashed session doesn't leave a daemon thread running forever.
10. As a daemon operator, I want a pending duration timer to never double-trigger stop (race with manual stop), so that Bundle commit path is never re-entered.
11. As a CLI user who re-runs `rdog record start --duration 30s` while a previous auto-stopped session's Bundle is still on disk, I want the new session to use a fresh `recording_id`, so that the previous Bundle isn't overwritten or aliased.
12. As a CLI user inspecting a recording after auto-stop, I want `@record-status` to clearly indicate the previous session ended via `auto_duration` trigger (not `manual` stop), so that I can audit whether my recording actually used the duration I set.
13. As a CLI user who set a duration, I want the auto-stop Bundle's owner-only delivery to fall back to completed retry (per ticket #19), so that I can still fetch the Bundle on a fresh connection after the original owner disconnected.
14. As a CLI user specifying `--duration 1.5m`, I want the parser to accept fractional minutes and convert to 90s, so that I get human-friendly duration values in the CLI surface.

## Implementation Decisions

### 1. Duration syntax (CLI + protocol)

- CLI flag: `--duration <humantime>` on `rdog record start` only.
- humantime grammar (custom ~30-line parser, **no new dep**):
  - `<integer>[.<fraction>]? <unit>` where unit is one of `s` / `m` / `h`.
  - `1s` = 1000 ms, `1m` = 60_000 ms, `1h` = 3_600_000 ms.
  - `1.5m` = 90_000 ms (fraction only allowed before `m` or `h`; before `s` is also allowed).
  - Whitespace optional between number and unit.
- Line-control `@record-start` payload extension: add `duration_ms: Option<u64>` JSON field. No breaking change to existing parsers (extra field is ignored by older daemon, missing field is `None`).
- Recording canonical protocol field name: `duration_ms` (matches the existing `started_at_unix_ms` style).

### 2. Timer owner = daemon

- `@record-start` carries `duration_ms` (or `None` for indefinite).
- `RecordingHandler::start` on `Some(duration_ms)` spawns a dedicated `std::thread` (matches existing sync LifecycleManager model, no tokio migration in this ticket).
- The timer thread:
  1. Polls the cancelled flag every 100 ms (cheap busy-wait is fine; do not use `std::thread::sleep` for the full duration because we need the cancel flag to be checked promptly on owner action).
  2. When elapsed time ≥ `duration_ms`, **atomically** set "timer fired" and invoke the existing manual stop path (`lifecycle.begin_finalize` + Bundle commit + delivery).
  3. The "timer fired" / "cancel triggered" mutual exclusion is a single `Arc<AtomicU8>` state machine: `0 = pending`, `1 = cancelled`, `2 = fired`.

### 3. Auto-stop Bundle delivery

- Always commit Bundle to `<bundle_dir>/<recording_id>.rdogrec.tar` per ticket #19 (no change).
- `frame_for_owner` is called on the **owner** connection at the moment of auto-stop. If owner has disconnected by then, delivery returns `NotOwner`; the handler logs a warning and the Bundle remains in the completed cache for the completed-retry path.
- Final `@response` to the current owner connection (if still connected) reports:
  ```json
  {
    "kind": "record-stop",
    "trigger": "auto_duration",
    "recording_id": "...",
    "bundle_filename": "...",
    "bundle_size_bytes": 12345,
    "bundle_sha256": "...",
    "delivery_status": "delivered" | "owner_disconnected"
  }
  ```
- The `trigger` field is **new** in the response envelope; existing manual stop responses report `"trigger": "manual"`. Parser remains backwards compatible because older clients ignore the new field.

### 4. Owner disconnect fallback = completed retry

- Reuse ticket #19's `completed` HashMap in `RecordingHandler`. Auto-stopped sessions are added the same way as manual-stopped sessions.
- Owner reconnects and runs `rdog record stop <recording_id>` → handler recognises the session is already `Completed`, returns the cached Bundle via `frame_for_owner` (read-only, no re-compile, per ticket #19 completed retry contract).
- No new "push notification" on reconnect. Owner must explicitly call `@record-status` or `@record-stop` to learn about the auto-stopped Bundle.

### 5. Cancel / stop preemption

- `RecordingHandler` holds `Option<AutoStopTimer>` inside itself, where `AutoStopTimer` contains:
  - `JoinHandle`
  - `cancelled: Arc<AtomicU8>` (the same state machine as in the timer thread)
  - `owner: ConnectionId` (for the auto-stop's delivery attempt)
  - `recording_id: String`
- `cancel_current()` and `complete_current()` (the "manual stop" path) **both** set the flag to `cancelled` (1) and `join()` the thread before returning.
- The timer thread checks the flag after each 100 ms tick; if it sees `1`, it returns immediately without invoking the stop path.
- If the timer thread sees `2` (fired) when waking up (race: flag set after the stop was already invoked), the manual path's `join()` waits for the thread to finish Bundle commit; the manual caller then returns `RECORD_ALREADY_COMPLETED` to the client.
- If a second `RecordingHandler::start` is attempted while the timer thread is still alive, return `RECORDING_ALREADY_ACTIVE` (single-active slot, unchanged).

### 6. Limits and error codes

- **Hard upper bound**: `3_600_000 ms` (1 hour). Exceeding returns `DURATION_TOO_LARGE` (error code 4120), 跟 ticket #19 `DELIVERY_RATE_LIMITED` (4200) 编号风格一致.
- **Lower bound**: `100 ms`. Below returns `DURATION_TOO_SMALL` (error code 4121).
- `0` ms is **accepted** (immediate stop, useful for shell scripting idioms).
- Negative, missing unit, unknown unit, integer overflow: CLI layer panic via clap value parser, never reaches daemon.
- Bundle size limit (256 MiB from ticket #19) and 1-hour duration limit are independent: a 1-hour recording may still hit the size limit and fail-closed (per ticket #19). No interaction between the two limits.

### 7. Lifecycle and `lifecycle.last_session` metadata

- `TerminalSummary` gets one new optional field: `stop_trigger: Option<StopTrigger>` where `StopTrigger` is `Manual | AutoDuration | OwnerDisconnected | AutoFailed`.
- `@record-status` includes the same field in `last_session` so users can audit "did the recording actually run for the duration I set?".
- The field defaults to `None` for sessions terminated before this feature ships; populated for any new auto-stop or auto-cancel.

### 8. Thread lifecycle / leak prevention

- `RecordingHandler::Drop` (or an explicit `shutdown()` call from the daemon) sets the cancelled flag, joins all live timer threads, and drops the `LifecycleManager` last. This guarantees no timer thread holds a dangling reference to a dropped `LifecycleManager`.
- If the daemon process is killed (SIGKILL), the timer thread dies with it; the orphan Session is handled by the existing `CrashRecovery` from ticket #18 (no change).

## Testing Decisions

### External behavior only

- Tests assert observable I/O contracts: CLI error codes, line-control response JSON, file system artifacts (Bundle on disk), thread state (no leak via JoinHandle).
- Tests must **not** assert internal state of `LifecycleManager` (e.g. exact internal flags, monotonic counters not surfaced via `@record-status`).

### Test seams

- **Primary seam: `RecordingHandler`** (unit tests in `control_recording/control_handler_tests.rs`).
  - Add cases: `auto_stop_fires_after_duration`, `auto_stop_cancelled_by_manual_stop`, `auto_stop_cancelled_by_cancel`, `auto_stop_continues_when_owner_disconnects`, `duration_too_large_rejected`, `duration_too_small_rejected`.
- **Secondary seam: `RecordSubcommand::Start` parser** (unit tests in `control_recording/cli_tests.rs`).
  - Add cases: `--duration 30s`, `--duration 1.5m`, `--duration 2h`, invalid format panic.
- **Tertiary seam: end-to-end smoke** (manual, not CI). Documented in `specs/rdog-acceptance-matrix.md` (the existing acceptance matrix spec).
  - Sequence: `rdog record start --duration 3s self` → wait 4s → `rdog record status self` → verify `last_session.stop_trigger == "auto_duration"` and `bundle_*` fields populated.

### Prior art

- `control_recording/control_handler_tests.rs::start_then_status_then_stop_emits_savefile_and_response` is the closest existing test. The auto-stop tests follow the same structure: build a temp dir, instantiate `RecordingHandler`, call `handle()` in sequence, assert on the `ControlExecutionOutcome` shape.
- `control_recording/cli_tests.rs::start_default_profile_emits_semantic` is the prior art for CLI parser tests. Add a new test that asserts `RecordCommand::Start { profile, duration_ms }` is correctly populated from a parsed CLI invocation.

## Out of Scope

- **Pre-existing Session state migration**: sessions started before this feature shipped (i.e. with no `duration_ms` field in the journal) are not retroactively given a `stop_trigger`. Their `TerminalSummary.stop_trigger` stays `None`.
- **Per-stop-hook redaction pause**: the `redaction_active` flag on `@record-mark` is orthogonal to the auto-stop path. No interaction.
- **Real-time duration progress events**: a `@record-status` style "remaining: 15s" polling endpoint is not in scope. Status reflects phase + counters only.
- **Multi-stage auto-stop (e.g. record for 5m, then pause for 1m, then continue for 5m)**: only one duration per session. Repeated manual stop + start cycles are required for multi-stage.
- **Automatic restart on session end**: not in scope. Use the rdog UI script runner or a shell loop.
- **Pausing a duration timer**: the timer is monotonic from start. To "pause" a recording, cancel and re-start (with the same `recording_id` policy is **not** in scope either; new start always gets a new id).
- **Timer accuracy guarantees**: we aim for ±200 ms accuracy (100 ms tick + wake-up variance). Tighter timing is not a contractual guarantee; if it ever becomes important, migrate to tokio with `tokio::time::Instant`, but that's a separate ticket.

## Further Notes

- ADR `0007-recording-auto-stop.md` is the source of truth for decisions; this spec is the contract for implementation. If they ever diverge, the ADR wins (spec can be re-derived from ADR + 实施 code).
- The `trigger` field in `@record-stop` response is the only breaking-ish change: existing clients that strictly validate the response shape (e.g. assert `kind` is the only key) will still work because we add a new key, not change existing keys. Document this in the line-control protocol spec when this lands.
- This feature is intentionally a **single-actor** enhancement (one duration, one timer, one session). Future enhancements (e.g. multiple durations, dynamic extension) should be modeled as separate tickets and should not retroactively complicate this contract.
