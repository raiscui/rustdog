#!/usr/bin/env bash

set -euo pipefail

# 这个脚本把 `mac.lab` 的本机 live smoke 固化成一个固定入口。
# 默认先探测是否已经有可用的 `mac.lab` daemon。
# 如果已有实例可用,脚本只复用,不会去停它。

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
target_name="${RDOG_TARGET_NAME:-mac.lab}"
cargo_bin="${RDOG_CARGO_BIN:-cargo}"
tmp_dir=""
daemon_log=""
started_daemon_pid=""
reused_existing_daemon="0"

log() {
    printf '[maclab-smoke] %s\n' "$*"
}

fail() {
    printf '[maclab-smoke] error: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    local exit_code=$?

    # 只清理本脚本自己拉起的 daemon。
    # 如果是复用用户已有实例,这里什么都不碰。
    if [[ -n "$started_daemon_pid" ]]; then
        if kill -0 "$started_daemon_pid" 2>/dev/null; then
            log "stopping temporary daemon pid=$started_daemon_pid"
            kill "$started_daemon_pid" 2>/dev/null || true
            wait "$started_daemon_pid" 2>/dev/null || true
        fi
    fi

    # 默认删掉临时目录。
    # 调试脚本行为时可以设置 `RDOG_KEEP_SMOKE_TMP=1` 保留日志。
    if [[ -n "$tmp_dir" && -d "$tmp_dir" && "${RDOG_KEEP_SMOKE_TMP:-0}" != "1" ]]; then
        rm -rf "$tmp_dir"
    fi

    exit "$exit_code"
}

trap cleanup EXIT INT TERM

build_binary() {
    if [[ "${RDOG_SKIP_BUILD:-0}" == "1" && -x "$binary" ]]; then
        log "reusing existing binary: $binary"
        return
    fi

    log "building target/debug/rdog"
    (
        cd "$repo_root"
        "$cargo_bin" build --quiet
    )
}

run_control_pipe() {
    local payload="$1"

    (
        cd "$repo_root"
        printf '%s\n' "$payload" | "$binary" control "$target_name"
    )
}

probe_existing_daemon() {
    local output

    if output="$(run_control_pipe '@ping' 2>&1)" && [[ "$output" == *'@response "pong"'* ]]; then
        reused_existing_daemon="1"
        log "reusing existing mac.lab daemon"
        return 0
    fi

    return 1
}

start_local_daemon() {
    local waited_loops=0

    tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rdog-maclab-smoke.XXXXXX")"
    daemon_log="$tmp_dir/maclab-daemon.log"

    log "starting temporary mac.lab daemon from $config"
    (
        cd "$repo_root"
        "$binary" daemon -c "$config" >"$daemon_log" 2>&1
    ) &
    started_daemon_pid="$!"

    # ready 日志是最稳定的启动完成信号。
    # 同时还要检查进程是否意外退出,避免傻等超时。
    while (( waited_loops < 100 )); do
        if ! kill -0 "$started_daemon_pid" 2>/dev/null; then
            if [[ -f "$daemon_log" ]]; then
                cat "$daemon_log" >&2
            fi
            fail "temporary daemon exited before ready"
        fi

        if [[ -f "$daemon_log" ]] && grep -Fq "zenoh router daemon ready:" "$daemon_log" && grep -Fq "service_name(daemon_name)=$target_name" "$daemon_log"; then
            log "temporary daemon is ready"
            log "daemon log: $daemon_log"
            return 0
        fi

        sleep 0.1
        waited_loops=$((waited_loops + 1))
    done

    if [[ -f "$daemon_log" ]]; then
        cat "$daemon_log" >&2
    fi
    fail "temporary daemon did not become ready within 10s"
}

run_ping_smoke() {
    local output

    output="$(run_control_pipe '@ping' 2>&1)"
    [[ "$output" == *'@response "pong"'* ]] || fail "expected @response \"pong\" from @ping, got: $output"
    log "pipe @ping smoke passed"
}

run_literal_shell_smoke() {
    local output

    output="$(run_control_pipe 'printf MACLAB_LITERAL_SHELL_OK' 2>&1)"
    [[ "$output" == *'@response "MACLAB_LITERAL_SHELL_OK"'* ]] || fail "literal shell smoke failed: $output"
    log "literal shell smoke passed"
}

run_pty_smoke() {
    local output

    output="$(
        cd "$repo_root"
        "$binary" control "$target_name" --pty -- /bin/sh -c 'if [ -t 0 ]; then printf MACLAB_PTY_OK; else printf MACLAB_NOT_TTY; fi' 2>&1
    )"
    [[ "$output" == *'MACLAB_PTY_OK'* ]] || fail "--pty smoke failed: $output"
    [[ "$output" != *'MACLAB_NOT_TTY'* ]] || fail "--pty returned non-tty marker: $output"
    log "pty smoke passed"
}

run_tty_display_smoke() {
    log "running tty display smoke"

    python3 - "$binary" "$repo_root" "$target_name" <<'PY'
import os
import pty
import re
import select
import subprocess
import sys
import time

binary = sys.argv[1]
repo_root = sys.argv[2]
target_name = sys.argv[3]

master_fd, slave_fd = pty.openpty()
proc = subprocess.Popen(
    [binary, "control", target_name],
    cwd=repo_root,
    stdin=slave_fd,
    stdout=slave_fd,
    stderr=slave_fd,
    close_fds=True,
)
os.close(slave_fd)

buffer = b""

def read_for(seconds: float) -> None:
    global buffer
    deadline = time.time() + seconds
    while time.time() < deadline:
        remaining = deadline - time.time()
        ready, _, _ = select.select([master_fd], [], [], max(0.0, remaining))
        if not ready:
            continue
        chunk = os.read(master_fd, 4096)
        if not chunk:
            return
        buffer += chunk

try:
    # 先吃掉启动信息和行编辑初始化输出。
    read_for(1.0)
    os.write(master_fd, b"@ping\n")
    read_for(1.2)
    os.write(master_fd, b"\x04")
    read_for(0.5)
finally:
    try:
        proc.wait(timeout=2.0)
    except subprocess.TimeoutExpired:
        proc.terminate()
        proc.wait(timeout=2.0)
    os.close(master_fd)

text = buffer.decode("utf-8", errors="replace")
text = re.sub(r"\x1b\[[0-9;?]*[ -/]*[@-~]", "", text)
text = text.replace("\r", "")

if "pong" not in text:
    raise SystemExit(f"TTY smoke did not show pong. output={text!r}")
if '@response "pong"' in text:
    raise SystemExit(f"TTY smoke leaked raw protocol line. output={text!r}")
PY

    log "tty display smoke passed"
}

main() {
    build_binary

    if ! probe_existing_daemon; then
        start_local_daemon
    fi

    run_ping_smoke
    run_literal_shell_smoke
    run_pty_smoke
    run_tty_display_smoke

    if [[ "$reused_existing_daemon" == "1" ]]; then
        log "all checks passed while reusing existing daemon"
    else
        log "all checks passed with temporary daemon"
    fi
}

main "$@"
