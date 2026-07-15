#!/usr/bin/env bash

# scripts/smoke_computer_act_verify.sh
#
# End-to-end smoke for `@computer-act` verify tier (ticket 12 + ticket 13).
#
# 覆盖 (per specs/rdog-computer-act-tickets/12-verify-none.md + 13-verify-best-effort.md):
# 1. 默认无 verify 字段 → response 不带 `verification` key (ticket 12)
# 2. verify:"none" → 同上 (显式 verify=none)
# 3. verify:"best_effort" → response 带 `verification.method:"ax_diff"` +
#    `verification.ax_diff.{windows_added,windows_removed,windows_modified,elements_added,elements_removed,elements_modified,changed}`
#    + `density.verify_ms` 字段
# 4. verify:"always" → ticket 14 占位,本轮等同 none (不带 verification 字段)
# 5. verify:"bogus" → error_code:"invalid_verify"

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-verify-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-verify-smoke] error: %s\n' "$*" >&2; exit 1; }

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
    local probe_seq=99995
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
    tmp_dir="$(mktemp -d -t rdog-computer-act-verify-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-verify-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99994:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
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

# 抽 json 字段 (跟 observe smoke 一致)
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

# --- Test 1: 默认无 verify 字段 → 不带 verification (ticket 12) ---
log "test 1: 默认 (无 verify 字段) → 不带 verification"
out="$(run_computer_act t1 '@computer-act#21:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
# ticket 12 acceptance: verify=none 时 omit 整个 verification 字段
if echo "$out" | grep -q '"verification"'; then
    fail "test 1: verification key should be omitted when verify defaults to none (output: $out)"
fi
# density 字段应该存在 (ticket 12/13 新加)
echo "$out" | grep -q '"density"' || fail "test 1: density field missing (output: $out)"
log "test 1 OK"

# --- Test 2: verify:"none" 显式 → 不带 verification ---
log "test 2: verify=\"none\" → 不带 verification"
out="$(run_computer_act t2 '@computer-act#22:{schema:"rdog.computer-act.v1",action:"wait",verify:"none",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 2: ok != true (output: $out)"
if echo "$out" | grep -q '"verification"'; then
    fail "test 2: verification key should be omitted when verify=none (output: $out)"
fi
log "test 2 OK"

# --- Test 3: verify:"best_effort" → AX diff + density.verify_ms ---
log "test 3: verify=\"best_effort\" → AX diff"
out="$(run_computer_act t3 '@computer-act#23:{schema:"rdog.computer-act.v1",action:"wait",verify:"best_effort",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 3: ok != true (output: $out)"
# ticket 13 acceptance: verification.method=ax_diff + verification.ax_diff.{...}
echo "$out" | grep -qE '"method"[[:space:]]*:[[:space:]]*"ax_diff"' || fail "test 3: verification.method != ax_diff (output: $out)"
echo "$out" | grep -qE '"ax_diff"' || fail "test 3: verification.ax_diff missing (output: $out)"
echo "$out" | grep -qE '"windows_added"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 3: ax_diff.windows_added missing (output: $out)"
echo "$out" | grep -qE '"elements_added"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 3: ax_diff.elements_added missing (output: $out)"
echo "$out" | grep -qE '"changed"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 3: ax_diff.changed missing (output: $out)"
# density.verify_ms 字段
echo "$out" | grep -qE '"verify_ms"[[:space:]]*:[[:space:]]*[0-9]+' || fail "test 3: density.verify_ms missing (output: $out)"
verify_ms="$(printf '%s' "$out" | get_field t3 density.verify_ms)"
log "  verify_ms=$verify_ms"
log "test 3 OK"

# --- Test 4: verify:"always" → ticket 14 占位,本轮等同 none (不带 verification) ---
log "test 4: verify=\"always\" → ticket 14 占位"
out="$(run_computer_act t4 '@computer-act#24:{schema:"rdog.computer-act.v1",action:"wait",verify:"always",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 4: ok != true (output: $out)"
# ticket 14 未实现,本轮 verify=always 等同 none (omit verification)
if echo "$out" | grep -q '"verification"'; then
    fail "test 4: verification key should be omitted when verify=always (ticket 14 not implemented yet, output: $out)"
fi
log "test 4 OK"

# --- Test 5: verify:"bogus" → error_code:"invalid_verify" ---
log "test 5: verify=\"bogus\" → invalid_verify"
out="$(run_computer_act t5 '@computer-act#25:{schema:"rdog.computer-act.v1",action:"wait",verify:"bogus",args:{duration_ms:0}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*false' || fail "test 5: ok != false (output: $out)"
echo "$out" | grep -qE '"error_code"[[:space:]]*:[[:space:]]*"invalid_verify"' || fail "test 5: error_code != invalid_verify (output: $out)"
echo "$out" | grep -q '"bogus"' || fail "test 5: error_message should mention 'bogus' (output: $out)"
log "test 5 OK"

log "all 5 computer-act verify smoke tests passed"
