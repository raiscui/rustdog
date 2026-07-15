#!/usr/bin/env bash

# scripts/smoke_computer_act_min.sh
#
# Minimum vertical slice smoke (ticket 05 of the @computer-act
# implementation plan; see specs/rdog-computer-act-tickets/05-minimum-vertical-slice.md).
#
# 范围: @computer-act 只覆盖两个最简单动作 `wait` 和 `open_app`,
#       不含 implicit_observe / verify / error envelope (后续 ticket 11/12/15)。
#
# 覆盖:
# 1. wait(100ms)   → ok=true, action=wait, dispatched_to=@wait, duration_ms≈100
# 2. open_app("Calculator") → ok=true, action=open_app, dispatched_to=@open-app
#                            + Calculator 进程真的被启动 (pgrep 验证)
# 3. response shape: 顶层字段 (ok, action, dispatched_to, duration_ms) 都在
#
# 已知不在范围:
# - 6 个 placeholder 字段 (observation_id / verification / observation_used /
#   density / trace_summary / trace_savefile) — ticket 11/12/16/18 才填充
# - verify 三档 / error envelope E2 / implicit_observe 详见后续 ticket

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-min-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-min-smoke] error: %s\n' "$*" >&2; exit 1; }

cleanup() {
    local exit_code=$?
    if [[ "$reused_existing_daemon" != "1" && -n "$started_daemon_pid" ]]; then
        if kill -0 "$started_daemon_pid" 2>/dev/null; then
            log "stopping temporary daemon pid=$started_daemon_pid"
            kill "$started_daemon_pid" 2>/dev/null || true
            wait "$started_daemon_pid" 2>/dev/null || true
        fi
        if [[ "${RDOG_KEEP_TMP:-0}" != "1" && -n "$tmp_dir" && -d "$tmp_dir" ]]; then
            rm -rf "$tmp_dir"
        fi
    fi
    exit "$exit_code"
}
trap cleanup EXIT INT TERM

log "building target/debug/rdog"
( cd "$repo_root" && cargo build --bin rdog --quiet )

probe_feature_specific() {
    local probe_seq=99999
    local probe_out
    probe_out="$("$binary" control mac.lab "@computer-act#${probe_seq}:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" 2>&1)" || return 1
    if printf '%s' "$probe_out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true'; then
        reused_existing_daemon="1"
        log "reusing existing local-default daemon (feature probe OK)"
        return 0
    fi
    return 1
}

start_local_daemon() {
    tmp_dir="$(mktemp -d -t rdog-computer-act-min-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-min-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99998:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.25
    done
    [[ "$ready" == "1" ]] || fail "daemon never became ready (log: $daemon_log)"
}

if ! probe_feature_specific; then
    local_default_pid_file="${HOME}/.local/state/rustdog/local-default/lab.pid"
    if [[ -f "$local_default_pid_file" ]]; then
        stale_pid="$(cat "$local_default_pid_file" 2>/dev/null || true)"
        if [[ -n "$stale_pid" ]] && kill -0 "$stale_pid" 2>/dev/null; then
            log "killing stale daemon pid=$stale_pid (feature probe failed)"
            kill "$stale_pid" 2>/dev/null || true
            sleep 1
        fi
    fi
    start_local_daemon
fi

# preflight: kill leftover Calculator
killall Calculator 2>/dev/null || true
sleep 0.5

# --- Test 1: wait(100ms) ---
log "test 1: @computer-act#1:wait(100ms)"
START_MS=$(python3 -c "import time; print(int(time.time()*1000))")
out="$("$binary" control mac.lab '@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}' 2>&1)"
END_MS=$(python3 -c "import time; print(int(time.time()*1000))")
ELAPSED=$((END_MS - START_MS))
echo "  response: $out"
echo "  wall_clock_ms: $ELAPSED"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
echo "$out" | grep -qE '"action"[[:space:]]*:[[:space:]]*"wait"' || fail "test 1: action != wait (output: $out)"
echo "$out" | grep -qE '"dispatched_to"[[:space:]]*:[[:space:]]*"@wait"' || fail "test 1: dispatched_to != @wait (output: $out)"
echo "$out" | grep -qE '"duration_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: duration_ms missing (output: $out)"
[[ $ELAPSED -ge 100 && $ELAPSED -lt 500 ]] || fail "test 1: wall clock $ELAPSED ms not in [100, 500]"
log "test 1 OK (wall ${ELAPSED}ms)"

# --- Test 2: open_app Calculator ---
log "test 2: @computer-act#2:open_app(\"Calculator\")"
out="$("$binary" control mac.lab '@computer-act#2:{schema:"rdog.computer-act.v1",action:"open_app",args:{app_name:"Calculator"}}' 2>&1)"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 2: ok != true (output: $out)"
echo "$out" | grep -qE '"action"[[:space:]]*:[[:space:]]*"open_app"' || fail "test 2: action != open_app (output: $out)"
echo "$out" | grep -qE '"dispatched_to"[[:space:]]*:[[:space:]]*"@open-app"' || fail "test 2: dispatched_to != @open-app (output: $out)"
echo "$out" | grep -qE '"duration_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 2: duration_ms missing (output: $out)"

# Verify Calculator process actually started
sleep 1
if pgrep -f "Calculator.app/Contents/MacOS/Calculator" >/dev/null; then
    CALC_PID=$(pgrep -f "Calculator.app/Contents/MacOS/Calculator")
    log "test 2 OK (Calculator running, pid=$CALC_PID)"
else
    fail "test 2: Calculator process not found via pgrep"
fi

# cleanup
killall Calculator 2>/dev/null || true

log "all 2 computer-act minimum vertical slice tests passed"
