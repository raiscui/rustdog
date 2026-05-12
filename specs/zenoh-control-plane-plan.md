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
