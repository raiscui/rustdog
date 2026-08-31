# Agent Messaging & Background Tasks Reference

## @spawn / task primitives

```text
@spawn:CARGO_BUILD_COMMAND          # raw shell text (same as @cmd)
@spawn {command:"...",cwd:"..."}    # object form (cwd only here)
@task-status:TASK_ID                # {"task","seq","command","state","exit_code"?}
@task-output:TASK_ID                # tail 80 lines; {task:"...",lines:N} custom
@task-cancel:TASK_ID                # sync kill; idempotent on terminal state
```

- task id: `t-` prefix + 8 hex (registry key, survives across requests);
  request id still goes on `#N`
- state machine: running -> completed (exit 0) / failed (non-zero) / canceled
- output: stdout+stderr merged ring buffer, 1MB cap keep-tail,
  `truncated:true` marks overflow
- daemon restart: old task ids report `task not found` (registry is
  not persisted - honest boundary)
- process-launch failure (missing shell etc.) returns an error response and
  never enters the registry; a missing command = non-zero exit = `failed`
- running cap 64; terminal entries keep last 64 by finish time

## Progress frames (session channel)

`@task-started` / `@task-completed` / `@task-failed` are pushed to the
originating session; `canceled` reuses `@task-failed` with `"canceled":true`.
`seq` is globally monotonic (loss detection). `@spawn` tasks emit no progress
frames (no semantic events) - pull `@task-output` instead.

## Companion agents

daemon-side mailbox semantics: cache on arrival (cache even unregistered
agents - loss prevention over dedup), 256 msgs/agent cap drop-oldest,
message-id dedup window, ack clears, not persisted.

```text
@agent-register:helper-a.lab        # register (mailbox starts caching it)
@agent-inbox:helper-a.lab           # pull pending (+ dropped/duplicate counts)
@agent-ack:helper-a.lab:MSG_ID      # ack consumption (idempotent)
@agent-card:helper-a.lab            # capability card (null = never published)
```

- message envelope `rdog.agentmsg.v1`:
  `{v,id,from,to,kind,payload,sent_at}`; kind: task / reply / ack / control
- keyexpr: `rdog/<ns>/agent/<name>/{inbox,card,alive}`
- `rdog agent --name helper-a.lab` starts a companion agent
  (EchoDecision for smoke; real LLM decision callbacks implement the
  `AgentDecision` trait)
- CRITICAL timing contract (zenoh declaration propagation is async):
  declare the reply subscriber FIRST, leave a propagation window
  (200ms-1s), then deliver the task - a pub with no matched subscriber
  is dropped silently
- mailbox is at-least-once: delivering before the agent starts is safe
  (recovered via mailbox pull); same-id redelivery is deduplicated

## TLS (transport encryption)

`[tls] enabled = true` in daemon config switches tcp listen endpoints to
`tls/`; clients just use `tls/host:port` entry points (materials auto-loaded
from `~/.rdog/tls/`). Generate once with `rdog auth tls-init` (rcgen self-signed
CA + daemon cert + mTLS client suite). Auth (usrpwd) is separate and
default-on: same-user local flows need zero config.
