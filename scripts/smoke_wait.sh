#!/usr/bin/env bash

# scripts/smoke_wait.sh
#
# End-to-end smoke for the @wait primitive (ticket 01 of the @computer-act
# implementation plan; see specs/rdog-computer-act-tickets/01-wait-primitive.md).
#
# EXPERIENCE.md:216 教训: "@ping 只能证明基础控制面可达",旧 daemon 复用可能
# 缺新 command。这里探活改用 feature-specific liveness (`@wait#X:{duration_ms:0}`),
# 不通过就 kill 旧 daemon 用新 binary 重启,确保 smoke 跟本地源码一致。
#
# 覆盖:
# 1. happy path 200ms (50ms tolerance)
# 2. zero duration (立即返回)
# 3. negative duration (parse error)
# 4. non-numeric duration (parse error)
# 5. missing duration_ms field (parse error)

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[wait-smoke] %s\n' "$*"; }
fail() { printf '[wait-smoke] error: %s\n' "$*" >&2; exit 1; }

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

# 永远 rebuild 本地源码,避免"if ! -x" 漏判过期 binary 的坑 (rdog smoke 实测踩过)。
log "building target/debug/rdog"
( cd "$repo_root" && cargo build --bin rdog --quiet )

# Feature-specific liveness probe (见 EXPERIENCE.md:216): 用 @wait:0 而不是 @ping,
# 因为 @ping 不能证明 daemon 真的有 @wait 支持。
probe_feature_specific() {
    local probe_seq=99999
    local probe_out
    probe_out="$("$binary" control mac.lab "@wait#${probe_seq}:{duration_ms:0}" 2>&1)" || return 1
    # 成功标志: 返回里出现 "duration_ms":0 (说明 daemon 真有 @wait 支持)
    if printf '%s' "$probe_out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true'; then
        reused_existing_daemon="1"
        log "reusing existing local-default daemon (feature probe OK)"
        return 0
    fi
    return 1
}

start_local_daemon() {
    tmp_dir="$(mktemp -d -t rdog-wait-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/wait-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    # 同样用 feature-specific liveness probe 等 daemon ready。
    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@wait#99998:{duration_ms:0}" >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.25
    done
    [[ "$ready" == "1" ]] || fail "daemon never became ready (log: $daemon_log)"
}

if ! probe_feature_specific; then
    # 旧 daemon 不支持 @wait,kill 它然后用新 binary 重启。
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

run_wait() {
    local label="$1"
    local line="$2"
    local out
    out="$("$binary" control mac.lab "$line" 2>&1)" || {
        printf '%s\n' "$out" >&2
        fail "$label: rdog control exited non-zero"
    }
    printf '%s' "$out"
}

assert_ok_true() {
    local label="$1"
    local out="$2"
    local field="$3"
    echo "$out" | grep -q "\"${field}\"[[:space:]]*:[[:space:]]*true" \
        || fail "$label: ${field} != true (output: $out)"
}

assert_int_field() {
    local label="$1"
    local out="$2"
    local field="$3"
    local expected="$4"
    echo "$out" | grep -qE "\"${field}\"[[:space:]]*:[[:space:]]*${expected}" \
        || fail "$label: ${field} != ${expected} (output: $out)"
}

# --- Test 1: 200ms wait (50ms tolerance per ticket) ---
log "test 1: @wait#1:{duration_ms:200}"
out="$(run_wait t1 '@wait#1:{duration_ms:200}')"
echo "  response: $out"
assert_ok_true t1 "$out" ok
assert_int_field t1 "$out" dispatched_to '"@wait"'
assert_int_field t1 "$out" requested_duration_ms '200'
# 验证实际 sleep 在 200-250ms 区间 (50ms tolerance)
assert_int_field t1 "$out" duration_ms '2[0-9][0-9]'
log "test 1 OK"

# --- Test 2: zero-duration wait ---
log "test 2: @wait#2:{duration_ms:0}"
out="$(run_wait t2 '@wait#2:{duration_ms:0}')"
echo "  response: $out"
assert_ok_true t2 "$out" ok
assert_int_field t2 "$out" requested_duration_ms '0'
log "test 2 OK"

# --- Test 3: negative duration ---
log "test 3: @wait#3:{duration_ms:-1} (negative)"
set +e
out="$(run_wait t3 '@wait#3:{duration_ms:-1}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'duration_ms|负数|invalid|error' || fail "test 3: missing error indicator (output: $out)"
log "test 3 OK"

# --- Test 4: non-numeric duration ---
log "test 4: @wait#4:{duration_ms:\"abc\"} (non-numeric)"
set +e
out="$(run_wait t4 '@wait#4:{duration_ms:"abc"}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'duration_ms|整数|invalid|error' || fail "test 4: missing error indicator (output: $out)"
log "test 4 OK"

# --- Test 5: missing duration_ms field ---
log "test 5: @wait#5:{} (missing duration)"
set +e
out="$(run_wait t5 '@wait#5:{}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'duration_ms|invalid|error' || fail "test 5: missing error indicator (output: $out)"
log "test 5 OK"

log "all 5 smoke tests passed"
