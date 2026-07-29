# rdog Acceptance Matrix

> Status: skeleton. `rdog-recording-auto-stop` section is the only E2E
> smoke documented so far. Other categories will be filled by ticket #7
> (acceptance matrix) and follow-ups.

## Recording auto-stop (issue #23)

End-to-end smoke for the `RecordingHandler` auto-stop timer. This is
the manual smoke the issue #23 acceptance checklist requires; it is
not covered by CI.

### Prerequisites

- macOS daemon with `rdog daemon` running (or `pnpm tauri dev`).
- `rdog record start --duration` accepts a humantime value (e.g.
  `3s`, `1m`, `500ms`); on macOS the underlying TCC permissions must
  already be granted (Accessibility / Screen Recording / Input
  Monitoring) — the smoke does not exercise the permission prompt.

### Smoke

```bash
# 1. Start a 3-second auto-stop recording against the local daemon.
rdog record start --duration 3s self

# 2. Wait > 3 seconds without owning any other request.
sleep 4

# 3. Probe the current state. The response should report
#    last_session.phase == "completed" and stop_trigger == "auto_duration".
rdog record status self

# 4. Verify the bundle file is on disk under the daemon's
#    `bundle_dir` (default: `~/.cache/rdog/recording/bundle/`).
ls -lh "$(rdog config --get recording.bundle_dir)/rec-*.rdogrec.tar"
```

### Expected results

- `rdog record status self` returns `status: "idle"` with
  `last_session.phase == "completed"` and
  `last_session.stop_trigger == "auto_duration"`.
- The bundle `.rdogrec.tar` file exists and is non-empty.
- The journal file `rec-*.journal.jsonl` is no longer in the `journal`
  directory under `latest` (cleaned up by the finalize flow).

### Negative paths

- `rdog record start --duration 50ms self` → 4121
  `DURATION_TOO_SMALL`.
- `rdog record start --duration 100m self` → 4120
  `DURATION_TOO_LARGE`.
- `rdog record start --duration 0s self` → accepted, no timer
  spawned (manual stop only).
