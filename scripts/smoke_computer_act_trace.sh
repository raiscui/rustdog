#!/usr/bin/env bash

# scripts/smoke_computer_act_trace.sh
#
# End-to-end smoke for `@computer-act` trace observability (ticket 17 + 18).
#
# 覆盖:
# 1. 默认无 trace 字段 → trace_summary 4 entry array, trace_savefile omit
# 2. trace="savefile" → trace_savefile 字段存在且指向 rdog_downloads/trace-*.json
# 3. trace_savefile 文件落地, 内容含 implicit_observe / dispatch / verify 三段
# 4. density 字段含 ADR-0006 全字段集 (elapsed_ms_total / verification_passed /
#    trace_step_count / dispatch_ms / implicit_observe_ms 等)

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-trace-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-trace-smoke] error: %s\n' "$*" >&2; exit 1; }

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
    local probe_seq=99993
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
    tmp_dir="$(mktemp -d -t rdog-computer-act-trace-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-trace-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99992:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
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

# 抽 json 字段 (跟其它 smoke 一致)
get_field() {
    local label="$1"
    local field="$2"
    python3 -c "
import json, re, sys
raw = sys.stdin.read().strip()
lines = [ln for ln in raw.splitlines() if ln.startswith('@response')]
if not lines:
    sys.exit(f'no @response line found in: {raw!r}')
m = re.match(r'@response\\s+(.*)', lines[-1])
if not m:
    sys.exit(f'@response prefix parse failed: {lines[-1]!r}')
envelope = json.loads(m.group(1))
val = envelope.get('value', envelope)
for part in '$field'.split('.'):
    if isinstance(val, dict):
        val = val.get(part)
    else:
        val = None
        break
if val is None:
    sys.exit(0)
if isinstance(val, bool):
    print('true' if val else 'false')
elif isinstance(val, (dict, list)):
    print(json.dumps(val, separators=(',', ':')))
else:
    print(val)
"
}

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

# --- Test 1: 默认无 trace 字段 → trace_summary 4 entry, trace_savefile omit ---
log "test 1: 默认无 trace → trace_summary 4 entry, trace_savefile omit"
out="$(run_computer_act t1 '@computer-act#31:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
echo "$out" | grep -qE '"trace_summary"[[:space:]]*:[[:space:]]*\[' || fail "test 1: trace_summary not array (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"implicit_observe"' || fail "test 1: trace_summary missing implicit_observe (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"ref_resolve"' || fail "test 1: trace_summary missing ref_resolve (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"dispatch"' || fail "test 1: trace_summary missing dispatch (output: $out)"
echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"verify"' || fail "test 1: trace_summary missing verify (output: $out)"
# density 字段 ADR-0006 全字段集
echo "$out" | grep -qE '"elapsed_ms_total"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.elapsed_ms_total missing (output: $out)"
echo "$out" | grep -qE '"verification_passed"[[:space:]]*:[[:space:]]*(true|false)' || fail "test 1: density.verification_passed missing (output: $out)"
echo "$out" | grep -qE '"trace_step_count"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.trace_step_count missing (output: $out)"
echo "$out" | grep -qE '"dispatch_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.dispatch_ms missing (output: $out)"
echo "$out" | grep -qE '"implicit_observe_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 1: density.implicit_observe_ms missing (output: $out)"
# verify=none → trace_summary[3].verify 是 skipped
verify_status="$(printf '%s' "$out" | python3 -c "
import json, re, sys
raw = sys.stdin.read().strip()
lines = [ln for ln in raw.splitlines() if ln.startswith('@response')]
m = re.match(r'@response\\s+(.*)', lines[-1])
env = json.loads(m.group(1))
ts = env['value']['trace_summary']
verify_step = [s for s in ts if s['step'] == 'verify'][0]
print(verify_step['status'])
")"
[[ "$verify_status" == "skipped" ]] || fail "test 1: verify step should be skipped (got: $verify_status)"
# trace_savefile omit
if echo "$out" | grep -q '"trace_savefile"'; then
    fail "test 1: trace_savefile should be omitted without trace:savefile (output: $out)"
fi
log "test 1 OK"

# --- Test 2: trace="savefile" → trace_savefile 字段存在 + 文件落地 ---
log "test 2: trace=\"savefile\" → trace_savefile 文件落地"
out2="$(run_computer_act t2 '@computer-act#32:{schema:"rdog.computer-act.v1",action:"wait",trace:"savefile",args:{duration_ms:0}}')"
echo "  response: $out2"
echo "$out2" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 2: ok != true (output: $out2)"
echo "$out2" | grep -qE '"trace_savefile"[[:space:]]*:[[:space:]]*"[^"]+"' || fail "test 2: trace_savefile not a path string (output: $out2)"
trace_path="$(printf '%s' "$out2" | get_field t2 trace_savefile)"
log "  trace_savefile=$trace_path"
[[ -n "$trace_path" ]] || fail "test 2: trace_savefile path is empty"
[[ -f "$trace_path" ]] || fail "test 2: trace_savefile file not found at $trace_path"
# 文件内容含三段
grep -q '"implicit_observe"' "$trace_path" || fail "test 2: trace file missing implicit_observe (path: $trace_path)"
grep -q '"dispatch"' "$trace_path" || fail "test 2: trace file missing dispatch (path: $trace_path)"
grep -q '"verification_passed"' "$trace_path" || fail "test 2: trace file missing verification_passed (path: $trace_path)"
log "test 2 OK (trace file written to $trace_path)"

# --- Test 3: trace_summary verify 步骤在 best_effort 时是 ok (不是 skipped) ---
log "test 3: trace=\"savefile\" + verify=\"best_effort\" → verify step 是 ok"
out3="$(run_computer_act t3 '@computer-act#33:{schema:"rdog.computer-act.v1",action:"wait",verify:"best_effort",trace:"savefile",args:{duration_ms:0}}')"
echo "  response: $out3"
echo "$out3" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 3: ok != true (output: $out3)"
verify_status="$(printf '%s' "$out3" | python3 -c "
import json, re, sys
raw = sys.stdin.read().strip()
lines = [ln for ln in raw.splitlines() if ln.startswith('@response')]
m = re.match(r'@response\\s+(.*)', lines[-1])
env = json.loads(m.group(1))
ts = env['value']['trace_summary']
verify_step = [s for s in ts if s['step'] == 'verify'][0]
print(verify_step['status'])
")"
[[ "$verify_status" == "ok" ]] || fail "test 3: verify step should be ok when verify=best_effort (got: $verify_status)"
log "test 3 OK"

log "all 3 computer-act trace smoke tests passed"
