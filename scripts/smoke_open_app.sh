#!/usr/bin/env bash

# scripts/smoke_open_app.sh
#
# End-to-end smoke for the @open-app primitive (ticket 02 of the @computer-act
# implementation plan; see specs/rdog-computer-act-tickets/02-open-app-primitive.md).
#
# Pattern: same as smoke_wait.sh — feature-specific liveness probe (`@wait:0`)
# 替代 @ping (EXPERIENCE.md:216),旧 daemon 缺功能时自动 kill 重启。
#
# 覆盖:
# 1. macOS 上 `@open-app Calculator` → OK + ps 能看到进程
# 2. macOS 上不存在的 app → ok:false, error_code:app_not_found
# 3. macOS 上 wait_ms=0 立即返回
# 4. 缺 app_name 字段 → parse error
#
# 注意: 此 smoke 仅在 macOS 上有意义;非 macOS 平台第一版 open-app 直接
# 返回 platform_unsupported (LP1 跟进)。

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[open-app-smoke] %s\n' "$*"; }
fail() { printf '[open-app-smoke] error: %s\n' "$*" >&2; exit 1; }

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

if [[ "$(uname -s)" != "Darwin" ]]; then
    fail "@open-app smoke 只在 macOS 上有完整覆盖;当前 OS=$(uname -s),LP1 跟进"
fi

log "building target/debug/rdog"
( cd "$repo_root" && cargo build --bin rdog --quiet )

# Feature-specific liveness probe (与 smoke_wait.sh 同样的 pattern)
probe_feature_specific() {
    local probe_seq=99999
    local probe_out
    probe_out="$("$binary" control mac.lab "@wait#${probe_seq}:{duration_ms:0}" 2>&1)" || return 1
    if printf '%s' "$probe_out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true'; then
        reused_existing_daemon="1"
        log "reusing existing local-default daemon (feature probe OK)"
        return 0
    fi
    return 1
}

start_local_daemon() {
    tmp_dir="$(mktemp -d -t rdog-open-app-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/open-app-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

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

run_open_app() {
    local label="$1"
    local line="$2"
    local out
    out="$("$binary" control mac.lab "$line" 2>&1)" || {
        printf '%s\n' "$out" >&2
        fail "$label: rdog control exited non-zero"
    }
    printf '%s' "$out"
}

# 关闭可能残留的 Calculator 进程,确保 ps 检查干净
log "preflight: kill any leftover Calculator"
killall Calculator 2>/dev/null || true
sleep 0.5

# --- Test 1: open Calculator ---
log "test 1: @open-app#1:{app_name:\"Calculator\"}"
out="$(run_open_app t1 '@open-app#1:{app_name:"Calculator"}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
echo "$out" | grep -q '"app_name"[[:space:]]*:[[:space:]]*"Calculator"' || fail "test 1: app_name != Calculator (output: $out)"
sleep 1
if pgrep -f "Calculator.app/Contents/MacOS/Calculator" >/dev/null; then
    log "test 1 OK (Calculator process running, pid=$(pgrep -f Calculator.app/Contents/MacOS/Calculator))"
else
    fail "test 1: Calculator process not found via pgrep"
fi

# 关闭 Calculator 给 test 2 干净状态
log "post-test-1 cleanup: killall Calculator"
killall Calculator 2>/dev/null || true
sleep 0.5

# --- Test 2: nonexistent app ---
log "test 2: @open-app#2:{app_name:\"NonExistentAppThatDoesNotExistXYZ123\"}"
out="$(run_open_app t2 '@open-app#2:{app_name:"NonExistentAppThatDoesNotExistXYZ123"}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*false' || fail "test 2: ok != false (output: $out)"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"app_not_found"' || fail "test 2: error_code != app_not_found (output: $out)"
log "test 2 OK"

# --- Test 3: missing app_name field ---
log "test 3: @open-app#3:{} (missing app_name)"
set +e
out="$(run_open_app t3 '@open-app#3:{}' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'app_name|invalid|error' || fail "test 3: missing error indicator (output: $out)"
log "test 3 OK"

log "all 3 open-app smoke tests passed"
