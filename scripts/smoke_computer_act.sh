#!/usr/bin/env bash

# scripts/smoke_computer_act.sh
#
# End-to-end smoke for the @computer-act meta-command (ticket 04 of the
# @computer-act implementation plan).
#
# 覆盖:
# 1. happy path: @computer-act@wait 路由到 @wait, 实际跑 sleep
# 2. happy path: @computer-act@open_app 路由到 @open-app
# 3. unknown action: @computer-act@bogus → error_code:"unknown_action"
# 4. parse error: 缺 schema → 拒绝
# 5. parse error: schema 不对 → 拒绝
# 6. response envelope 包含 6 个 placeholder 字段 (ticket 11/12/16/18 填充)

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-smoke] error: %s\n' "$*" >&2; exit 1; }

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
    tmp_dir="$(mktemp -d -t rdog-computer-act-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-smoke-daemon.log"
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

run_computer_act() {
    local label="$1"
    local line="$2"
    local out
    out="$("$binary" control mac.lab "$line" 2>&1)" || {
        printf '%s\n' "$out" >&2
        fail "$label: rdog control exited non-zero"
    }
    printf '%s' "$out"
}

# --- Test 1: happy path - wait ---
log "test 1: @computer-act#1:wait(100ms)"
START_MS=$(python3 -c "import time; print(int(time.time()*1000))")
out="$(run_computer_act t1 '@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}')"
END_MS=$(python3 -c "import time; print(int(time.time()*1000))")
ELAPSED=$((END_MS - START_MS))
echo "  response: $out"
echo "  wall_clock_ms: $ELAPSED"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
echo "$out" | grep -q '"dispatched_to"[[:space:]]*:[[:space:]]*"@wait"' || fail "test 1: dispatched_to != @wait (output: $out)"
echo "$out" | grep -qE '"action"[[:space:]]*:[[:space:]]*"wait"' || fail "test 1: action != wait (output: $out)"
# response envelope 校验 (ticket 11/12/13/14/17/18 后的契约):
# - observation_id / observation_used: ticket 11 填充
# - verification: verify=none 时 omit (ticket 12)
# - density: ADR-0006 全字段 (ticket 17): dispatch_ms / implicit_observe_ms /
#   elapsed_ms_total / verification_passed / trace_step_count 等
# - trace_summary: 4 entry 数组 (ticket 18): implicit_observe / ref_resolve /
#   dispatch / verify
# - trace_savefile: 默认 omit, 仅 request.trace=="savefile" 时存在
echo "$out" | grep -q '"density"[[:space:]]*:[[:space:]]*{' || fail "test 1: density not object (output: $out)"
echo "$out" | grep -qE '"dispatch_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.dispatch_ms missing (output: $out)"
echo "$out" | grep -qE '"implicit_observe_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.implicit_observe_ms missing (output: $out)"
echo "$out" | grep -qE '"elapsed_ms_total"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.elapsed_ms_total missing (output: $out)"
echo "$out" | grep -qE '"verification_passed"[[:space:]]*:[[:space:]]*(true|false)' || fail "test 1: density.verification_passed missing (output: $out)"
# verify=none 时 omit verification key (ticket 12 acceptance)
if echo "$out" | grep -q '"verification"'; then
    fail "test 1: verification key should be omitted when verify=none (output: $out)"
fi
# trace_summary: 4 entry 数组 (ticket 18)
echo "$out" | grep -qE '"trace_summary"[[:space:]]*:[[:space:]]*\[' || fail "test 1: trace_summary not array (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"implicit_observe"' || fail "test 1: trace_summary missing implicit_observe (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"ref_resolve"' || fail "test 1: trace_summary missing ref_resolve (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"dispatch"' || fail "test 1: trace_summary missing dispatch (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"verify"' || fail "test 1: trace_summary missing verify (output: $out)"
# trace_savefile: 默认 omit (opt-in)
if echo "$out" | grep -q '"trace_savefile"'; then
    fail "test 1: trace_savefile should be omitted without trace:savefile (output: $out)"
fi
[[ $ELAPSED -ge 100 && $ELAPSED -lt 500 ]] || fail "test 1: wall clock $ELAPSED ms not in [100, 500]"
log "test 1 OK"

# --- Test 2: happy path - open_app ---
log "test 2: @computer-act#2:open_app Calculator"
# preflight: kill leftover Calculator
killall Calculator 2>/dev/null || true
sleep 0.5
out="$(run_computer_act t2 '@computer-act#2:{schema:"rdog.computer-act.v1",action:"open_app",args:{app_name:"Calculator"}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 2: ok != true (output: $out)"
echo "$out" | grep -q '"dispatched_to"[[:space:]]*:[[:space:]]*"@open-app"' || fail "test 2: dispatched_to != @open-app (output: $out)"
sleep 1
if pgrep -f "Calculator.app/Contents/MacOS/Calculator" >/dev/null; then
    log "test 2 OK (Calculator running, pid=$(pgrep -f Calculator.app/Contents/MacOS/Calculator))"
else
    fail "test 2: Calculator process not found"
fi
killall Calculator 2>/dev/null || true
sleep 0.5

# --- Test 3: unknown action ---
log "test 3: @computer-act#3:action=telekinesis (unknown)"
out="$(run_computer_act t3 '@computer-act#3:{schema:"rdog.computer-act.v1",action:"telekinesis",args:{}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*false' || fail "test 3: ok != false (output: $out)"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"unknown_action"' || fail "test 3: error_code != unknown_action (output: $out)"
log "test 3 OK"

# --- Test 4: parse error - missing schema ---
log "test 4: @computer-act#4 missing schema"
set +e
out="$(run_computer_act t4 '@computer-act#4:{action:"wait",args:{duration_ms:100}}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'schema|invalid|error' || fail "test 4: missing error indicator (output: $out)"
log "test 4 OK"

# --- Test 5: parse error - wrong schema ---
log "test 5: @computer-act#5 wrong schema (v2)"
set +e
out="$(run_computer_act t5 '@computer-act#5:{schema:"rdog.computer-act.v2",action:"wait",args:{duration_ms:100}}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'schema|v2|invalid' || fail "test 5: missing schema mismatch indicator (output: $out)"
log "test 5 OK"

log "all 5 computer-act smoke tests passed"
