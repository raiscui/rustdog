## [2026-07-30 11:05:28] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 笔记: architecture deepening candidates

## 来源

### 领域与 ADR

- `CONTEXT.md` 定义了 Recorder、Recording Session、Recording Journal、Replay Script、Semantic Promotion、Participating Window 等 canonical terms。本轮没有重新定义这些词。
- ADR-0003 规定 `@computer-act` 保持 thin dispatcher,OS adaptation 不进入该 dispatcher。
- ADR-0004/0005/0006 固定 request/response、implicit_observe、TTL、cancel、ControlLine 和 trace/density 语义。
- `specs/control-frame-refactor-plan.md` 已记录 ControlExecutionOutcome、ControlFrame、ControlPeerSession 的部分落地状态。
- `specs/rdog-observation-scoped-refmap-plan.md` 已把 Observation、short ref、durable selector、semantic re-find 放在同一演进线。
- `specs/rdog-display-aware-control-chain-plan.md` 已记录 display scope、window identity、focus verification、scoped AX/visual 和 post-action evidence 的控制链。

### Candidate 1: Observation module

- AX `AxSnapshot::with_observation()` 自己分配 refs、构造 selector drafts 并调用 record path。
- Window `attach_window_observation()` 自己收集 candidates、refs、selectors 并调用 record path。
- Web `build_web_find_response_json_with_refresh()` 和 Web Act 对 snapshot 做 clone + with_observation。
- `control_observation.rs` 同时承载 in-memory TTL/ref store、durable initialization、selector get/resolve、candidate backend 和 response rendering。
- deletion test成立: 删除当前 Observation module 后,ref allocation、selector promotion、TTL、durable policy 会回到 AX/Window/Web/mouse 多处。
- strength: Strong。top recommendation。

### Candidate 2: Control execution module

- `control_core.rs` dispatch command and response rendering。
- `control_frames.rs` owns frame variants and wire parse/serialize。
- `control_session.rs` owns queue, ordering, request-id collection, lifecycle gate and frame log。
- `control_invocation.rs` has TCP/WebSocket/Zenoh send loops and artifact exchange collection。
- `control_transport.rs` owns transport implementations; `zenoh_control.rs` retains session/query compatibility paths。
- existing ControlExecutionOutcome and ControlPeerSession are real seams, but execution policy still crosses modules。
- strength: Strong。aligns with existing control-frame-refactor plan, not a new protocol proposal。

### Candidate 3: Target evidence module

- Display scope is centralized, but AX query resolves window identity, Web resolves target window id/ref, and mouse resolves observation ref to current rect independently。
- The same observation_id + ref_id + window_id + display guard semantics occur in AX/Web/window/mouse paths。
- deletion test成立: without current display scope seam, selector parsing and guard matching spread back to each action family; current seam is real, missing depth is shared evidence。
- strength: Worth exploring。must preserve temporary `@eN`, separate display identity, stale/ambiguous fail-closed behavior。

### Candidate 4: Platform adapter module

- `WindowBackend` and `AxBackend` exist, but each has one System adapter delegating to one macOS implementation。
- Tests mostly use resolver closures and synthetic values; full OS effect adapter seam is not yet real。
- deletion test does not currently concentrate much complexity because there is only one production adapter。
- strength: Speculative。do not add a seam until a second production adapter or full deterministic test adapter exists。

## Report

- Path: `/var/folders/58/3f9_69ts3bx4slnb8tgl572m0000gn/T/architecture-review-20260730-110448.html`
- HTML structure check: 4 candidate articles, 8 Mermaid blocks, 1 top recommendation.
- Mermaid bodies validated with `beautiful-mermaid-rs --ascii`。
- Report opened with macOS `open`。
- No production files modified。
