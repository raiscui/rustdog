# Zenoh Router/Serial Control-Plane Canonical Spec

## Status

Canonical. This file describes the approved v1 direction for Zenoh in `rustdog`.

Historical note:
- `specs/zenoh-peer-peer-lan-profile.md` is legacy / historical reference only.
- New implementation work should follow this spec, the approved PRD, and the matching test spec.

## Goal

`rustdog` should run a **daemon-embedded Zenoh router** that can:

1. accept a local/host-reachable control client,
2. accept an ESP32 via native `transport_serial`, and
3. continue serving the existing line-control plane (`@ping`, `@cmd#id`, bare shell lines).
4. open explicit PTY sessions over the same session-channel control plane when requested by `@pty`, `@pty-detach`, or `@pty-attach`.

This is still a **control-plane first** migration.
Bare shell lines are accepted as one-shot line-control commands.
They are not an interactive shell data plane.
Interactive terminal programs must use explicit `@pty` / `rdog control --pty -- ...`.

## Non-goals

- no legacy interactive shell over Zenoh without `@pty`
- no cwd state kept by bare shell lines
- no custom serial bridge protocol
- no external `zenohd` supervision process
- no peer-style discovery as the v1 join contract
- no streaming shell/data-plane migration outside the explicit PTY session channel

## Approved topology

- `daemon = router`
- `control = client`
- `ESP32 = serial-attached Zenoh node`
- `control` joins the network through **Zenoh scouting / autodiscovery by default**
- `daemon` exposes at least:
  - one **client-reachable non-serial listen endpoint** (expected TCP)
  - one **serial listen endpoint** for MCU/ESP32 access

## Key architecture rules

### 1. Router/session ownership

The daemon owns the router role. The plan must prove one of these shapes:

- preferred: a single router session can host the control queryable/liveliness contract, or
- fallback: two in-process sessions (router session + app client session)

A fallback to external `zenohd` is not allowed.

### 2. Control join contract

`rdog control` should join the router **without requiring an explicit entrypoint in the common case**.

- v1 should allow `rdog control <target-name>` to join via router scouting / autodiscovery
- the explicit long form `rdog control --transport zenoh --target-name ...` remains valid for scripts and diagnostics
- CLI `--entry-point` remains as fallback when autodiscovery is unavailable
- no peer/peer transport role is required for that UX; daemon stays router and control stays client

### 3. Serial transport contract

`transport_serial` is a first-class Zenoh transport.

- serial is configured as a normal listen endpoint
- serial endpoint syntax must be validated explicitly
- serial endpoint failures must be observable and testable
- serial transport is for ESP32/MCU access, not for custom bridging

### 4. Legacy migration contract

The old peer/peer surface is not the primary path anymore.

- `zenoh-peer` CLI surface should be rejected or migrated explicitly
- `mode = "peer"` is historical, not the v1 daemon contract
- old peer/peer docs/specs are historical only

## Canonical control-plane behavior

The control plane remains the existing line-control request/reply protocol:

- `@ping`
- `@cmd#id`
- bare shell lines
- `@key`
- `@paste`
- `@savefile`
- `@screenshot`
- `@pty` / `@pty-close` / `@pty-detach` / `@pty-attach` over session channels
- explicit error responses

The daemon still must:

- enforce `daemon_name` uniqueness in a namespace
- publish liveliness
- expose a queryable control endpoint
- reply with the same explicit control semantics as today
- map PTY frames through `session/<id>/to-daemon` and `session/<id>/to-control`, including `@pty-ready`, `@pty-output`, `@pty-exit`, `@pty-closed`, `@pty-detached`, and `@pty-attached`

## Local fast path: unixpipe

macOS / Linux 上同机 `rdog daemon` + `rdog control <target>` 必须自动走 `transport_unixpipe`,
避免 UDP loopback 上的 Zenoh link 协议栈开销。`rdog control` 客户端的 fast path 行为契约:

1. **路径推导**:base 路径 = `$TMPDIR/rdog-{namespace}-{daemon_name}.pipe`,
   `$TMPDIR` 不存在时回退 `/tmp`。macOS 上 `$TMPDIR` 是 per-user(例如 `/var/folders/.../T/`),
   自然提供权限隔离,免 chmod。Linux 上 `$TMPDIR` 不一定存在,直接 `/tmp` 兜底。
2. **路径长度上限**:base 必须 ≤ 95 字节(`sun_path` 104 字节 - `_downlink` 9 字节后缀)。
   超过时 daemon 启动 fail-fast。
3. **client 端 fast path 判定**:`Path::exists` 检查 `<base>_uplink` 是否存在。
   - **不**主动 open FIFO 探活(那会让 Zenoh request channel 单 reader 复用机制看到 EOF 并破坏 daemon 状态)。
   - 存在 → 把 `unixpipe/{base}` 作为唯一 connect endpoint 传给 `zenoh::open`。
   - 不存在 → 走原来的 `autodiscover_router_endpoints` 路径。
4. **daemon 端**:启用 `unixpipe.enabled = true` 时自动把 `unixpipe/{base}` 注入 `listen_endpoints` 列表最前。
   启动时调用 `cleanup_stale_unixpipe_socket` unlink `<base>` / `<base>_uplink` / `<base>_downlink`
   三个残留文件。
5. **空 target / self target**:`rdog control @<line>` 和 `rdog control self @<line>` 先读 local-default registry。
   - daemon 只有在 `[zenoh.unixpipe] local_default = true` 时才声明自己是本 namespace 的本机默认 daemon。
   - registry 记录 `namespace`、`daemon_name`、`pid` 和 FIFO base path。
   - client 每次读取时检查 PID 存活和 `<base>_uplink` 是否存在;stale registry 会被清理。
   - 没有有效 registry 时,才 fallback 到旧的唯一 FIFO 扫描。
   - 这条规则避免 `$TMPDIR` 里存在多个测试 FIFO 时,空 target 直接失败。
6. **远端 fallback**:Unix 平台用户显式 `unixpipe.enabled = false` 可以关掉;跨主机场景 daemon
   端 unixpipe 不在另一台机器的 `$TMPDIR` 里,client 端 `Path::exists` 自然返回 false,透明走 UDP scout。
7. **CLI 不动**:`rdog control <target>` 仍然是无 flag 命令,fast path 对用户透明。
8. **日志约定**(实施时的实际字符串):
   - daemon 启动: `info: zenoh unixpipe fast path 启用: base=<path>`
   - daemon 声明默认: `info: zenoh unixpipe local-default 已注册: namespace=<ns>, daemon_name=<name>`
   - client fast path 触发: `info: unixpipe endpoint detected, taking fast path (path: <path>)`
   - client fallback: 走原 scout 日志(无 unixpipe 日志)。

详细规格和测试见 `specs/zenoh-unixpipe-fast-path-plan.md`。

## Configuration contract

### Daemon router config

The daemon router profile should contain:

- `mode = "router"`
- `namespace`
- `daemon_name`
- `listen_endpoints` with at least:
  - one host-reachable client endpoint (TCP expected)
  - one serial endpoint for ESP32 access
- request / startup timeout knobs

### Control join contract

The control client must be able to join using:

- autodiscovery as the default path
- a CLI entrypoint (`--entry-point`) only as fallback
- no separate control config surface in v1

### `connect_endpoints`

For v1, daemon-side `connect_endpoints` are not part of the main contract.
If future upstream connectivity is needed, define it separately rather than reusing peer-era semantics.

## Implementation boundary

- Keep Zenoh async inside a runtime/adapter boundary.
- Keep current app-side core code free of Zenoh-specific transport logic.
- Keep the explicit control core reusable across transports.

## Verification matrix

### Unit

- router mode parses correctly
- serial endpoint validation fails explicitly on bad syntax
- missing client-reachable endpoint fails explicitly
- control without entrypoint still works when router autodiscovery is available
- legacy peer transport/config is rejected with a clear migration message

### Integration

- daemon starts as router and exposes queryable + liveliness
- control joins via autodiscovery and succeeds with `@ping`
- control joins via autodiscovery and succeeds with `@cmd#id`
- control joins via autodiscovery and succeeds with a bare shell line
- control joins via explicit `--entry-point` fallback and succeeds with `@ping`
- control joins via explicit `--entry-point` fallback and succeeds with `@cmd#id`
- control joins via explicit `--entry-point` fallback and succeeds with a bare shell line
- duplicate daemon names still fail fast
- serial endpoint is bound and visible in startup logs

### Hardware/manual smoke

- ESP32 joins via serial
- daemon logs show the serial-attached node in the same network
- control still reaches the target daemon over the router network

## Relationship to historical docs

- `specs/zenoh-peer-peer-lan-profile.md` is historical context only.
- The README should point here, not to the peer/peer spec, for new work.

## Follow-ups

- keep the peer/peer spec only as archived reference
- maintain a separate runbook for serial endpoint smoke once implementation lands
- if upstream connectivity is needed later, draft a separate router-upstream extension spec
