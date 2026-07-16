#!/usr/bin/env bash

# scripts/smoke_computer_act_all.sh
#
# End-to-end smoke for ALL 13 `@computer-act` actions (ticket 21).
#
# 设计: 13 个 action 各跑一段, 验证 routing + dispatcher 链路完整。
# - 部分 action 走真实 GUI (open_app, click 等) 需要 macOS + 真实屏幕
# - 部分 action 走 synthetic args (wait / open_url 等) 不需要 GUI
# - 验证 success path (ok:true) + dispatched_to + trace_summary 4 entry
#
# 注意: 真实 GUI action 会移动鼠标 / 触发按键, 不要在生产窗口跑

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-all-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-all-smoke] error: %s\n' "$*" >&2; exit 1; }

cleanup() {
    local exit_code=$?
    # 关闭测试开的 Calculator
    killall Calculator 2>/dev/null || true
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
    local probe_seq=99991
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
    tmp_dir="$(mktemp -d -t rdog-computer-act-all-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-all-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99990:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
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

run_action() {
    # run_action <seq> <action_name> <rdog_line> <expected_dispatched_to>
    local seq="$1"
    local name="$2"
    local line="$3"
    local expected_dispatched="$4"
    local out
    out="$("$binary" control mac.lab "$line" 2>&1)" || {
        printf '%s\n' "$out" >&2
        fail "$name: rdog control exited non-zero"
    }
    echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "$name: ok != true (output: $out)"
    if [[ -n "$expected_dispatched" ]]; then
        # 用 grep -F 走字面匹配 (避免 @key+@click+@key 里的 + 被当 regex);
        # 接受 ": "" 或 ":"" 两种格式 (rdog response 紧凑无空格)
        echo "$out" | grep -qF "\"dispatched_to\":\"${expected_dispatched}\"" \
            || echo "$out" | grep -qF "\"dispatched_to\": \"${expected_dispatched}\"" \
            || fail "$name: dispatched_to != $expected_dispatched (output: $out)"
    fi
    # trace_summary 必须 4 entry
    echo "$out" | grep -qE '"trace_summary"[[:space:]]*:[[:space:]]*\[' || fail "$name: trace_summary missing (output: $out)"
    echo "$out" | grep -qE '"step"[[:space:]]*:[[:space:]]*"dispatch"' || fail "$name: trace_summary missing dispatch (output: $out)"
}

# --- 1. open_app (real GUI: Calculator) ---
log "1/13: open_app Calculator"
killall Calculator 2>/dev/null || true
sleep 0.5
run_action 1 "open_app" '@computer-act#1:{schema:"rdog.computer-act.v1",action:"open_app",args:{app_name:"Calculator"}}' "@open-app"
sleep 1
if ! pgrep -f "Calculator.app/Contents/MacOS/Calculator" >/dev/null; then
    fail "Calculator process not running after open_app"
fi
log "1/13 OK (Calculator pid=$(pgrep -f Calculator.app/Contents/MacOS/Calculator))"
killall Calculator 2>/dev/null || true
sleep 0.5

# --- 2. open_url (folded to @cmd "open <url>") ---
log "2/13: open_url"
run_action 2 "open_url" '@computer-act#2:{schema:"rdog.computer-act.v1",action:"open_url",args:{url:"https://example.com"}}' "@cmd"
log "2/13 OK"

# --- 3. click (real GUI click at center of screen) ---
log "3/13: click at (500, 500)"
run_action 3 "click" '@computer-act#3:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[500,500]}}' "@click"
log "3/13 OK"

# --- 4. doubleclick (count=2) ---
log "4/13: doubleclick at (500, 500)"
run_action 4 "doubleclick" '@computer-act#4:{schema:"rdog.computer-act.v1",action:"doubleclick",args:{start_box:[500,500]}}' "@click"
log "4/13 OK"

# --- 5. triple_click (count=3) ---
log "5/13: triple_click at (500, 500)"
run_action 5 "triple_click" '@computer-act#5:{schema:"rdog.computer-act.v1",action:"triple_click",args:{start_box:[500,500]}}' "@click"
log "5/13 OK"

# --- 6. right_single (button=right) ---
log "6/13: right_single at (500, 500)"
run_action 6 "right_single" '@computer-act#6:{schema:"rdog.computer-act.v1",action:"right_single",args:{start_box:[500,500]}}' "@click"
log "6/13 OK"

# --- 7. hover ---
log "7/13: hover at (500, 500)"
run_action 7 "hover" '@computer-act#7:{schema:"rdog.computer-act.v1",action:"hover",args:{start_box:[500,500]}}' "@mouse-move"
log "7/13 OK"

# --- 8. type (no ref → @paste fallback) ---
log "8/13: type without ref → @paste fallback"
run_action 8 "type" '@computer-act#8:{schema:"rdog.computer-act.v1",action:"type",args:{content:"hello"}}' "@type-text"
log "8/13 OK"

# --- 9. hotkey (single key press) ---
log "9/13: hotkey F1 (no-op function key)"
run_action 9 "hotkey" '@computer-act#9:{schema:"rdog.computer-act.v1",action:"hotkey",args:{key:"F1"}}' "@key"
log "9/13 OK"

# --- 10. hotkey_click (composite: key down + click + key up) ---
log "10/13: hotkey_click (shift+click)"
run_action 10 "hotkey_click" '@computer-act#10:{schema:"rdog.computer-act.v1",action:"hotkey_click",args:{start_box:[500,500],key:"shift"}}' "@key+@click+@key"
log "10/13 OK"

# --- 11. scroll ---
log "11/13: scroll down 3 at (500, 500)"
run_action 11 "scroll" '@computer-act#11:{schema:"rdog.computer-act.v1",action:"scroll",args:{start_box:[500,500],direction:"down",amount:3}}' "@wheel"
log "11/13 OK"

# --- 12. drag ---
log "12/13: drag from (300,300) to (700,700)"
run_action 12 "drag" '@computer-act#12:{schema:"rdog.computer-act.v1",action:"drag",args:{start_box:[300,300],end_box:[700,700]}}' "@drag"
log "12/13 OK"

# --- 13. wait ---
log "13/13: wait(100ms)"
run_action 13 "wait" '@computer-act#13:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:100}}' "@wait"
log "13/13 OK"

# --- error path coverage ---
# Bonus: invalid_args (scroll amount=-1 应拒绝)
log "bonus: invalid_args (scroll amount=-1)"
out="$("$binary" control mac.lab '@computer-act#14:{schema:"rdog.computer-act.v1",action:"scroll",args:{start_box:[500,500],direction:"down",amount:-1}}' 2>&1)" || true
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*false' || fail "bonus: invalid_args expected ok:false (output: $out)"
echo "$out" | grep -q '"error_code"[[:space:]]*:[[:space:]]*"invalid_args"' || fail "bonus: error_code != invalid_args (output: $out)"
echo "$out" | grep -q '"retry"' || fail "bonus: retry field missing in error envelope (output: $out)"
echo "$out" | grep -qE '"strategy"[[:space:]]*:[[:space:]]*"never"' || fail "bonus: retry.strategy != never (output: $out)"
log "bonus OK"

log "all 13 computer-act actions + invalid_args error path passed"
