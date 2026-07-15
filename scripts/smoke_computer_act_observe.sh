#!/usr/bin/env bash

# scripts/smoke_computer_act_observe.sh
#
# End-to-end smoke for `@computer-act` implicit_observe plumbing (ticket 11).
#
# 覆盖 (per specs/rdog-computer-act-tickets/11-implicit-observe-and-freshness.md):
# 1. start_box 路径 → response 携带 observation_id + observation_used.freshness="fresh"
# 2. target.ref + observation_id 在 TTL 内 (5s) → freshness="fresh", 复用同一个 id
# 3. target.ref + observation_id 已过期 (>5s) → freshness="stale_re_observed",
#    新 observation_id,previous_observation_id 等于原 id
# 4. non-mouse 动作 (wait) → 不写 observation_id / observation_used 到 response
#
# 设计:
# - 走 unixpipe + mac.lab (跟其它 smoke 一致)
# - feature-specific liveness probe (`@computer-act:wait:0`) 检测本地 daemon 是否可用
# - 测试期间持有同一个 daemon 进程,避免跨请求 cache 被新 daemon 清空

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[computer-act-observe-smoke] %s\n' "$*"; }
fail() { printf '[computer-act-observe-smoke] error: %s\n' "$*" >&2; exit 1; }

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
    local probe_seq=99997
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
    tmp_dir="$(mktemp -d -t rdog-computer-act-observe-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/computer-act-observe-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99996:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
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

# 抽 json 字段的小工具 (避免 jq 依赖; 用 python3 解析)。
# stdin 期望是 `@response {"id":...,"value":...}` 形式,我们只关心 `value` 部分。
get_field() {
    local label="$1"
    local field="$2"
    python3 -c "
import json, re, sys
raw = sys.stdin.read().strip()
# 剥掉 @response 前缀和任何 [INFO]/[WARN] 等日志行,只保留最后一行 JSON
lines = [ln for ln in raw.splitlines() if ln.startswith('@response')]
if not lines:
    sys.exit(f'no @response line found in: {raw!r}')
m = re.match(r'@response\s+(.*)', lines[-1])
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

# --- Test 1: start_box 路径 → fresh observation_id ---
log "test 1: start_box → observation_used.freshness=fresh"
out="$(run_computer_act t1 '@computer-act#11:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:0,start_box:[500,500]}}')"
echo "  response: $out"
echo "$out" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 1: ok != true (output: $out)"
echo "$out" | grep -qE '"freshness"[[:space:]]*:[[:space:]]*"fresh"' || fail "test 1: freshness != fresh (output: $out)"
echo "$out" | grep -qE '"observation_id"[[:space:]]*:[[:space:]]*"computer-act-obs-' || fail "test 1: observation_id missing (output: $out)"
echo "$out" | grep -qE '"ref_id"[[:space:]]*:[[:space:]]*"@e[0-9]+"' || fail "test 1: ref_id missing/invalid (output: $out)"
log "test 1 OK"

# --- Test 2: target.ref + observation_id 在 TTL 内 → fresh + 复用 ---
log "test 2: target.ref+obs_id within TTL → fresh + 复用"
# 用 test 1 拿到的 observation_id, 立即 (TTL 5s 内) 复用
first_obs_id="$(printf '%s' "$out" | get_field t1 observation_id)"
first_ref_id="$(printf '%s' "$out" | get_field t1 observation_used.ref_id)"
log "  first obs_id=$first_obs_id ref_id=$first_ref_id"
out2="$(run_computer_act t2 "@computer-act#12:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0,target:{ref:\"$first_ref_id\",observation_id:\"$first_obs_id\"}}}")"
echo "  response: $out2"
echo "$out2" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 2: ok != true (output: $out2)"
echo "$out2" | grep -qE '"freshness"[[:space:]]*:[[:space:]]*"fresh"' || fail "test 2: freshness != fresh (output: $out2)"
second_obs_id="$(printf '%s' "$out2" | get_field t2 observation_id)"
[[ "$first_obs_id" == "$second_obs_id" ]] || fail "test 2: obs_id should be reused within TTL: $first_obs_id vs $second_obs_id"
log "test 2 OK (obs_id reused)"

# --- Test 3: 等待 6 秒, 让 observation_id 过期 → stale_re_observed ---
log "test 3: target.ref+obs_id expired → stale_re_observed (等待 6 秒)"
sleep 6
out3="$(run_computer_act t3 "@computer-act#13:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0,target:{ref:\"$first_ref_id\",observation_id:\"$first_obs_id\"}}}")"
echo "  response: $out3"
echo "$out3" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 3: ok != true (output: $out3)"
echo "$out3" | grep -qE '"freshness"[[:space:]]*:[[:space:]]*"stale_re_observed"' || fail "test 3: freshness != stale_re_observed (output: $out3)"
prev_obs_id="$(printf '%s' "$out3" | get_field t3 observation_used.previous_observation_id)"
new_obs_id="$(printf '%s' "$out3" | get_field t3 observation_id)"
[[ "$prev_obs_id" == "$first_obs_id" ]] || fail "test 3: previous_observation_id should match input ($first_obs_id vs $prev_obs_id)"
[[ "$new_obs_id" != "$first_obs_id" ]] || fail "test 3: new observation_id should differ (both $new_obs_id)"
log "test 3 OK (stale → re-observe)"

# --- Test 4: non-mouse 动作 → 不写 observation_id 到 response ---
log "test 4: wait 单独 duration_ms → 不带 observation_id"
out4="$(run_computer_act t4 '@computer-act#14:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}')"
echo "  response: $out4"
echo "$out4" | grep -q '"ok"[[:space:]]*:[[:space:]]*true' || fail "test 4: ok != true (output: $out4)"
# observation_id 字段应保持 null 占位 (ticket 11 non-mouse fallback 路径)
echo "$out4" | grep -q '"observation_id"[[:space:]]*:[[:space:]]*null' || fail "test 4: observation_id should be null for non-mouse (output: $out4)"
echo "$out4" | grep -q '"observation_used"[[:space:]]*:[[:space:]]*null' || fail "test 4: observation_used should be null for non-mouse (output: $out4)"
log "test 4 OK"

log "all 4 computer-act implicit_observe smoke tests passed"
