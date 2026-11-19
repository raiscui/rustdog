# Replay binding preparation

This procedure prepares metadata locally. It does not establish authorization, freshness, desktop exclusivity or application success. It does not authorize any live action. The current helper is limited to POSIX clients and pins the reviewed checker and a specific frozen macOS client by SHA-256. A different client build is rejected, even if it is otherwise compatible. Do not update a deployed skill or replace a binary merely to use this procedure.

## Prepare names

Create an independent metadata-only template from the current authorized task inventory, not by copying names from the replay plan. Each entry names one text or pointer parameter, its logical target and a reference into a separate value source or observation. It contains no values, PID, HWND, coordinates or verifier. This illustrative template is not a current target binding or proof of independent authorship:

```json
{"schema":"rdog.binding-template.v1","entries":[{"parameter":"move","kind":"pointer","target":"destination","reference":"move_observation"},{"parameter":"text","kind":"text","target":"destination","reference":"text_value"}]}
```

Run these commands from the repository root, replacing metadata paths with actual local paths. `names.json` must not already exist.

```text
python3 .codex/skills/rdog-control/scripts/prepare-replay.py names --template TEMPLATE.json --output names.json
python3 .codex/skills/rdog-control/scripts/prepare-replay.py validate --template TEMPLATE.json --names names.json --plan PLAN.json --rdog PINNED_MAC_CLIENT JOURNAL_A JOURNAL_B
```

The validation command runs the pinned checker and its local inspect. Success means only that the plan and parameter names passed that checker. It does not prove that the template came from an independent source. Preserve the source inventory, author, creation method, template hash, generated names hash and any independent review separately. An earlier timestamp and matching hashes prove ordering and identity, not independent authorship.

## Execute only after separate authorization
The caller must obtain present, explicit authorization for the actual target and operation, an eligible exclusive desktop and fresh target evidence. The `--operation-approval` argument is a nonsecret opaque accountability reference. It is not a signature, a grant, or proof that those conditions hold. A file field such as `authorized: true` cannot supply that proof. The observation must be separately prepared from current nonsecret identity, point and verification evidence. The helper does not observe the desktop or repair focus.

The saved nonsecret observation has schema `rdog.binding-observation.v1`. Its `targets` map uses logical target keys and contains `pid`, `root_hwnd` and `text_target`; the latter is a `native_control` with `hwnd`, a `focused_control` with `control_type`, or `null` for a pointer-only target. Its `pointers` map uses pointer reference keys and contains the logical `target`, `points` and `verify` in the existing runner shapes. Target and pointer keys must exactly match their template references with no extras. The helper's `parse_observation` and `validate_verify` define the closed fields and bounds. A saved observation has no text values, but the file does not certify freshness.

The parameter FD contains a JSON map from text reference keys to strings, not parameter names to complete bindings. Its keys must exactly match the template's text references; a pointer-only template still supplies an empty object.

The caller supplies a protected anonymous pipe FD containing only the independent parameter values object. An explicitly approved external provider must own the writing end, and the helper must be the exclusive reader. The provider must not use a TTY, environment variable, argv, persistent file or named FIFO for values. Do not construct a complete bound runtime in tool-call code, including Python or JSON literals, even with public markers. Do not place it in a shell command, file, log or other artifact. Do not use shell tracing or `tee`. No provider is supplied here. If an approved provider or fresh evidence is unavailable, stop.

Only when those separate conditions hold, an authorized caller can invoke the existing runner through the helper with inherited FD `FD` and real metadata paths. This syntax does not itself supply the FD, validate approval or authorize the operation.

```text
python3 .codex/skills/rdog-control/scripts/prepare-replay.py execute --template TEMPLATE.json --names names.json --plan PLAN.json --rdog PINNED_MAC_CLIENT --observation OBSERVATION.json --parameter-fd FD --operation-approval OPAQUE_RUN_REFERENCE --target-name TARGET --entry-point TLS_ENTRY_POINT JOURNAL_A JOURNAL_B
```

The helper discards both runner output streams. `runner_exit_zero` is only a process exit result, not evidence that the application reached the intended state. Obtain independent task-specific evidence separately. A timeout or nonzero exit after runner start is `outcome_unknown_do_not_retry`. Stop and inspect separately; do not automatically replay, re-record or refocus. A `not_dispatched` result refers to replay child dispatch, not a proof that the environment remained unchanged. Input snapshots and runner checks do not eliminate all check-to-use races. The helper rejects final-component symlinks and nonregular metadata inputs, but these checks do not make an unresponsive filesystem or operating system memory private.
