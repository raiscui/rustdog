---
title: "GNU coreutils kill 的负 pid 参数歧义: 外部 kill 命令发进程组信号不可靠"
date: 2026-08-28
last_updated: 2026-08-28
module: control-flow
component: control-flow-process
problem_type: logic_error
severity: medium
status: active
tags:
  - linux
  - process-group
  - signal
  - coreutils
  - kill
  - pipe-eof
  - ci
verified_by:
  - "docker ubuntu:24.04 strace 实证: /usr/bin/kill -TERM -<pgid> 实际执行 kill(-2, SIGTERM) = -1 ESRCH (2026-08-28)"
  - "docker python 复刻 process.rs 全逻辑: TOTAL duration=2.008s, stdout_join_wait=1.937s (孤儿持管道)"
  - "ubuntu CI run 33145050743 (e1f61dc): unit tests 首次全绿, shell_lane_should_mark_timeout 通过"
---

# GNU coreutils kill 的负 pid 参数歧义: 外部 kill 命令发进程组信号不可靠

## Context

`src/control_flow/process.rs` 的 `terminate_process_tree` 原实现用外部命令
`Command::new("kill").args(["-TERM", "-{child_id}"])` 向 shell 子进程的进程组
发终止信号。macOS (BSD kill) 上一直正常; 2026-08-28 ubuntu CI 的 wayland-sys
存量红被修通后, `@flow` shell lane 的超时测试首次在 linux 上真实执行并暴露
`duration_ms: 2001` (50ms 超时的命令拖满 2 秒)。

## Symptoms

- 仅 linux 挂, macOS 过: `shell_lane_should_mark_timeout_and_continue_to_expect`
  断言 `duration_ms < 1000` 失败, 实际 2001ms, `timed_out: true`, `exit_code: None`
- 慢机器 (CI runner) 上更容易触发; 开发机上难以复现

## Root Cause

两层叠加:

1. **GNU coreutils 参数解析歧义 (决定性)**: strace 显示
   `/usr/bin/kill -TERM -2555` 实际执行 `kill(-2, SIGTERM) = -1 ESRCH` —
   信号被发往进程组 2 而非目标进程组 2555, 静默失败。BSD kill (macOS)
   对同参数正确解析为负 pid, 所以仅 linux 暴露。
2. **孤儿管道写端放大**: `child.kill()` (SIGKILL 单进程) 杀死 shell 后,
   子进程 (如 `sleep 2`) 因组信号未达成为孤儿, 继续持有 stdout/stderr 管道
   写端; `join_stream_reader` 阻塞在 `read()` 等 EOF, 直到孤儿子孙自然退出,
   duration 被拉满。

## Fix

`terminate_process_tree` (unix) 改用进程内 syscall 直发:

```rust
let process_group_id = -(child_id as libc::pid_t);
unsafe { libc::kill(process_group_id, signum); }
let _ = child.kill();  // 兜底直杀组长
```

`Cargo.toml` 的 `[target.'cfg(unix)'.dependencies]` 加 `libc = "0.2"`
(依赖树已有, 零编译成本)。消灭外部命令的三重脆弱点: 参数解析歧义
(GNU/BSD 差异) + PATH 依赖 + spawn 开销。

## Verification (可复跑)

```bash
# 1. 参数歧义实证 (docker)
docker run --rm ubuntu:24.04 bash -c '
  setsid sh -c "sleep 2" & SH=$!; sleep 0.1
  strace -e trace=kill /usr/bin/kill -TERM -$SH 2>&1 | grep kill'
# 期望输出: kill(-2, SIGTERM) = -1 ESRCH  (目标错, 组从未收到信号)

# 2. 修复后 CI: ubuntu unit tests 中 shell_lane_should_mark_timeout 通过
```

## When to re-read

- 任何需要向进程组/会话发信号的代码 (PTY 清理, 后台任务取消, shell lane 超时)
- 发现"超时标记正确但总时长超标"类症状 (孤儿持管道 EOF 依赖)
- 考虑用外部命令 (kill/ps/pgrep) 做进程管理时 — 优先 libc syscall
