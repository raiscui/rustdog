# Zenoh, Hardware, And Microcontroller Reference

## Current Zenoh Model

The canonical rdog Zenoh profile is:

- daemon = router
- control = client
- control tries autodiscovery first
- `--entry-point` is fallback when discovery is unavailable
- `daemon_name` is the stable human-facing target name
- `service_name = daemon_name` in the current static model

Example:

```bash
rdog daemon --transport zenoh --name mini-a.lab --namespace lab
rdog control mini-a.lab
rdog control mini-a.lab --entry-point tcp/192.168.1.20:17447
```

## Keyexprs For Direct SDK Clients

If a code agent uses `rdog control` as a subprocess, it does not need these keyexprs.

If it writes a direct Zenoh SDK integration, current keyexprs are:

```text
rdog/<namespace>/daemon/<service_name>/member/<member_id>/alive
rdog/<namespace>/daemon/<service_name>/member/<member_id>/control
rdog/<namespace>/daemon/<service_name>/member/<member_id>/keyinput
rdog/<namespace>/session/<session_id>/to-daemon
rdog/<namespace>/session/<session_id>/to-control
```

In static mode:

```text
service_name = daemon_name
member_id = daemon_name
```

Session bootstrap still uses the control queryable:

```text
__rdog_session_open__:<session_id>
__rdog_session_close__:<session_id>
```

After bootstrap:

- publish control requests to `session/<id>/to-daemon`
- subscribe to results from `session/<id>/to-control`
- expect `@response`, `@savefile`, and `@pty-*` frames

## Retry Strategy

Use the same posture as `rdog control`:

1. resolve target at session start
2. use the current target while requests succeed
3. on timeout, re-resolve target
4. rebuild the session bridge
5. retry once
6. if retry fails, report the failure

Do not permanently cache a control key across daemon restarts.

## Serial Endpoint And Hardware Bridge

A daemon router can expose both a client-reachable TCP endpoint and a serial endpoint.

```toml
[zenoh]
enabled = true
mode = "router"
namespace = "lab"
daemon_name = "mini-a.lab"
listen_endpoints = [
  "tcp/0.0.0.0:17447",
  "serial//dev/ttyACM1#baudrate=112500",
]
```

Important boundary:

- TCP or another client-reachable endpoint lets `rdog control` join the router.
- Serial lets a device-side Zenoh participant join when firmware/tooling supports it.
- Rdog does not automatically create a shell inside a microcontroller.

## Practical Hardware Patterns

Use `rdog control` to drive the machine that has physical access:

```bash
rdog control mini-a.lab \
  @ping \
  '@cmd#1:"ls /dev/tty* | head"' \
  '@cmd#2:"python3 tools/read_sensor.py --port /dev/ttyACM1"'
```

Use PTY when the vendor tool is interactive:

```bash
rdog control mini-a.lab --pty -- /bin/bash
```

Use screenshots/key/paste for GUI-only hardware tools:

```bash
rdog control win11.lab \
  '@observe#0:{mode:"hybrid",include_screenshot:true,include_ax:false,include_windows:true}' \
  '@key#1:{key:"F5",hold_ms:200,mode:"press_release"}' \
  @screenshot#2 \
  '@mouse-move#3:{dx:0,dy:0,coordinate_space:"relative"}'
```

For GUI-only hardware tools, start with `@observe` when available.
Use lower-level `@screenshot` when you specifically need a fresh coordinate manifest.
For GUI-only tools that require mouse input, first capture a screenshot and parse the manifest.
Then send `@click`, `@drag`, or `@wheel` with `coordinate_space:"os-logical"` coordinates from that manifest.
Do not invent a second coordinate system for the hardware tool window.

Before firmware flashing, chip erase, reset loops, relay toggles, or destructive device commands, ask unless the user has already explicitly requested that exact action.

## Do Not Assume

- payloads are JSON objects; protocol frames are UTF-8 text lines
- queryable always means single query to single final reply
- bare shell lines keep cwd or shell state
- `@paste`, `@key`, or mouse commands bypass OS permissions
- duplicated `daemon_name` instances are supported in one namespace
- NAT traversal is automatic
