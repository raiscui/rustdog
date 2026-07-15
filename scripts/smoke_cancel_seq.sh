#!/usr/bin/env bash

# scripts/smoke_cancel_seq.sh
#
# End-to-end smoke for the @cancel#seq primitive (ticket 03 of the @computer-act
# implementation plan; see specs/rdog-computer-act-tickets/03-cancel-seq-command.md).
#
# 已知限制 (smoke 范围之外): rdog 当前 dispatcher 是同步单线程处理请求,
# `@cancel#seq` 与 in-flight `@wait` 不能在 wall-clock 维度上真正 race。
# "cancel during sleep" 行为由 `cancellation::sleep_cancellable` 单测覆盖
# (`src/cancellation.rs` 内部 #[cfg(test)]),证明机制正确;smoke 只验证
# wire 协议层 (cancel 命令语法、unknown_target_seq、parse error)。
#
# 覆盖:
# 1. cancel 一个已完成的 seq → unknown_target_seq (cancel 命令本身 OK)
# 2. cancel 一个从未存在的 seq → unknown_target_seq
# 3. cancel 错误格式 (missing target_seq) → parse error
# 4. cancel 错误格式 (non-object payload) → parse error

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[cancel-seq-smoke] %s\n' "$*"; }
fail() { printf '[cancel-seq-smoke] error: %s\n' "$*" >&2; exit 1; }

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
    probe_out="$("$binary" control mac.lab "@wait#${probe_seq}:{duration_ms:0}" 2>&1)" || return 1
    if printf '%s' "$probe_out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true'; then
        reused_existing_daemon="1"
        log "reusing existing local-default daemon (feature probe OK)"
        return 0
    fi
    return 1
}

start_local_daemon() {
    tmp_dir="$(mktemp -d -t rdog-cancel-seq-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/cancel-seq-smoke-daemon.log"
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

# 触发一次 @wait 让 daemon 完成一个 seq (用于后续 cancel "已完成")
log "preflight: complete a @wait to populate seq=200"
"$binary" control mac.lab "@wait#200:{duration_ms:50}" >/dev/null

# --- Test 1: cancel 已完成的 seq ---
log "test 1: cancel seq=200 (already-completed): expect unknown_target_seq"
out="$("$binary" control mac.lab "@cancel#seq#201:{target_seq:200}" 2>&1)"
echo "  response: $out"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"unknown_target_seq"' || fail "test 1: error_code != unknown_target_seq (output: $out)"
echo "$out" | grep -q '"registry_state"[[:space:]]*:[[:space:]]*"empty_or_completed"' || fail "test 1: registry_state != empty_or_completed (output: $out)"
log "test 1 OK"

# --- Test 2: cancel 从未存在的 seq ---
log "test 2: cancel seq=99999 (never existed): expect unknown_target_seq"
out="$("$binary" control mac.lab "@cancel#seq#202:{target_seq:99999}" 2>&1)"
echo "  response: $out"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"unknown_target_seq"' || fail "test 2: error_code != unknown_target_seq (output: $out)"
log "test 2 OK"

# --- Test 3: parse error: missing target_seq ---
log "test 3: @cancel#seq#203:{} (missing target_seq)"
set +e
out="$("$binary" control mac.lab "@cancel#seq#203:{}" 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'target_seq|invalid|error' || fail "test 3: missing error indicator (output: $out)"
log "test 3 OK"

# --- Test 4: parse error: non-object payload ---
log "test 4: @cancel#seq#204:\"1\" (non-object payload)"
set +e
out="$("$binary" control mac.lab '@cancel#seq#204:"1"' 2>&1)"
set -e
echo "  response: $out"
echo "$out" | grep -qiE 'object|invalid|error' || fail "test 4: missing error indicator (output: $out)"
log "test 4 OK"

# --- Test 5: self-cancel (cancel 命令自己的 seq) ---
log "test 5: @cancel#seq#205:{target_seq:205} (self-target)"
out="$("$binary" control mac.lab "@cancel#seq#205:{target_seq:205}" 2>&1)"
echo "  response: $out"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"unknown_target_seq"' || fail "test 5: error_code != unknown_target_seq (output: $out)"
log "test 5 OK"

log "all 5 cancel-seq smoke tests passed"
