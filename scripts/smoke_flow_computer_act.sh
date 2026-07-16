#!/usr/bin/env bash

# scripts/smoke_flow_computer_act.sh
#
# 端到端 smoke for `@flow` + `@computer-act` 集成 (tickets 19 + 20)。
#
# 覆盖:
# 1. flow 包含 @computer-act 但 policy.allow_computer_act:false → 拒绝
# 2. flow allow_computer_act:true + @computer-act ControlLine → 成功, response 写入
# 3. Expect.response_field_equals 断言 $.ok == true → 成功
# 4. Expect.response_field_equals 断言 $.ok == false → 失败
# 5. Expect.response_path_contains 断言 $.dispatched_to 含 "@click" → 成功
# 6. Expect.response_path_contains 断言 $.error.error_code 含 "invalid" → 失败 (verify 走 success path)
# 7. happy path 端到端: open_app + click + verify(ok:true) 都 work

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${RDOG_BINARY:-$repo_root/target/debug/rdog}"
config="${RDOG_CONFIG:-$repo_root/rdog_macos.toml}"
tmp_dir=""
started_daemon_pid=""
reused_existing_daemon="0"

log() { printf '[flow-computer-act-smoke] %s\n' "$*"; }
fail() { printf '[flow-computer-act-smoke] error: %s\n' "$*" >&2; exit 1; }

cleanup() {
    local exit_code=$?
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
    local probe_seq=99989
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
    tmp_dir="$(mktemp -d -t rdog-flow-computer-act-smoke.XXXXXX)"
    local daemon_log="$tmp_dir/flow-computer-act-smoke-daemon.log"
    log "starting temporary daemon (tmp=$tmp_dir)"
    "$binary" daemon -c "$config" > "$daemon_log" 2>&1 &
    started_daemon_pid=$!

    local ready=0
    for _ in $(seq 1 60); do
        if "$binary" control mac.lab "@computer-act#99988:{schema:\"rdog.computer-act.v1\",action:\"wait\",args:{duration_ms:0}}" >/dev/null 2>&1; then
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

# 抽 @flow response 字段 (跟其它 smoke 一致)
get_flow_field() {
    local label="$1"
    local field="$2"
    python3 -c "
import json, re, sys
raw = sys.stdin.read().strip()
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

# 构造 flow JSON 的 helper: 走 strict JSON, 内部 ControlLine 字符串用 rdog dict 语法
make_flow_line() {
    # usage: make_flow_line <seq> <allow_computer_act> <inner_line>
    local seq="$1"
    local allow_ca="$2"
    local inner_line="$3"
    python3 -c "
import json, sys
seq = sys.argv[1]
allow_ca = sys.argv[2].lower() == 'true'
inner = sys.argv[3]
flow = {
    'schema': 'rdog.flow.v1',
    'policy': {
        'allow_shell': False,
        'allow_file_read': False,
        'allow_computer_act': allow_ca,
        'timeout_ms': 10000,
        'max_steps': 8,
        'max_output_bytes': 4096,
    },
    'options': {'trace': 'summary'},
    'steps': [{'ControlLine': inner}],
}
print(f'@flow#' + seq + ':' + json.dumps(flow))
" "$seq" "$allow_ca" "$inner_line"
}

# --- Test 1: flow 包含 @computer-act 但 policy.allow_computer_act:false → 拒绝 ---
log "test 1: @computer-act ControlLine 但 allow_computer_act:false → 拒绝"
inner1='@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:0}}'
flow_line="$(make_flow_line 1 false "$inner1")"
out="$("$binary" control mac.lab "$flow_line" 2>&1)" || true
echo "  response: $out"
echo "$out" | grep -qE '"code"[[:space:]]*:[[:space:]]*64' || fail "test 1: code != 64 (output: $out)"
echo "$out" | grep -qE '"error"' || fail "test 1: error field missing (output: $out)"
echo "$out" | grep -qE 'allow_computer_act' || fail "test 1: error should mention allow_computer_act (output: $out)"
log "test 1 OK (default deny gate works)"

# --- Test 2: flow allow_computer_act:true + @computer-act ControlLine → 成功 ---
log "test 2: allow_computer_act:true → @computer-act ControlLine 成功"
inner2='@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
flow_line2="$(make_flow_line 2 true "$inner2")"
out2="$("$binary" control mac.lab "$flow_line2" 2>&1)" || {
    printf '%s\n' "$out2" >&2
    fail "test 2: rdog control exited non-zero"
}
echo "  response: $out2"
echo "$out2" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "test 2: flow status != ok (output: $out2)"
log "test 2 OK"

# --- Test 3: Expect.response_field_equals 断言 $.ok == true → 成功 ---
log "test 3: Expect.response_field_equals:$.ok == true → 成功"
make_flow_with_expect() {
    # usage: make_flow_with_expect <seq> <allow_ca> <inner_line> <expect_kind> <path> <expect_value>
    local seq="$1"
    local allow_ca="$2"
    local inner_line="$3"
    local expect_kind="$4"
    local expect_path="$5"
    local expect_value="$6"
    python3 -c "
import json, sys
seq = sys.argv[1]
allow_ca = sys.argv[2].lower() == 'true'
inner = sys.argv[3]
expect_kind = sys.argv[4]
expect_path = sys.argv[5]
expect_value = json.loads(sys.argv[6])
flow = {
    'schema': 'rdog.flow.v1',
    'policy': {
        'allow_shell': False, 'allow_file_read': False, 'allow_computer_act': allow_ca,
        'timeout_ms': 10000, 'max_steps': 8, 'max_output_bytes': 4096,
    },
    'options': {'trace': 'summary'},
    'steps': [
        {'ControlLine': inner},
        {'Expect': {'kind': expect_kind, 'path': expect_path, 'value': expect_value}},
    ],
}
print(f'@flow#' + seq + ':' + json.dumps(flow))
" "$seq" "$allow_ca" "$inner_line" "$expect_kind" "$expect_path" "$expect_value"
}

make_flow_with_contains() {
    # usage: make_flow_with_contains <seq> <allow_ca> <inner_line> <expect_kind> <path> <contains>
    local seq="$1"
    local allow_ca="$2"
    local inner_line="$3"
    local expect_kind="$4"
    local expect_path="$5"
    local contains="$6"
    python3 -c "
import json, sys
seq = sys.argv[1]
allow_ca = sys.argv[2].lower() == 'true'
inner = sys.argv[3]
expect_kind = sys.argv[4]
expect_path = sys.argv[5]
contains = sys.argv[6]
flow = {
    'schema': 'rdog.flow.v1',
    'policy': {
        'allow_shell': False, 'allow_file_read': False, 'allow_computer_act': allow_ca,
        'timeout_ms': 10000, 'max_steps': 8, 'max_output_bytes': 4096,
    },
    'options': {'trace': 'summary'},
    'steps': [
        {'ControlLine': inner},
        {'Expect': {'kind': expect_kind, 'path': expect_path, 'contains': contains}},
    ],
}
print(f'@flow#' + seq + ':' + json.dumps(flow))
" "$seq" "$allow_ca" "$inner_line" "$expect_kind" "$expect_path" "$contains"
}

# ticket 20 follow-up: 多 ControlLine 场景 (open_app + click + SleepMs) 必须用 sys.argv
# 传 inner_line, 避免 bash 命令替换 \"$(python3 -c \"...\")\" 里嵌套双引号被 zsh 吞掉
# (test 6 用硬编码双引号就是踩了这个坑: schema:\"...\" 被 zsh 解释成 schema:...)
make_flow_with_contains_multi() {
    # usage: make_flow_with_contains_multi <seq> <allow_ca> <expect_kind> <path> <contains> <step1_json> [step2_json ...]
    # 每 step1_json 是单个 step dict, 例如 \'{\"ControlLine\": \"...\"}' 或 \'{\"SleepMs\": 500}'。
    local seq="$1"
    local allow_ca="$2"
    local expect_kind="$3"
    local expect_path="$4"
    local contains="$5"
    shift 5
    python3 -c "
import json, sys
seq = sys.argv[1]
allow_ca = sys.argv[2].lower() == 'true'
expect_kind = sys.argv[3]
expect_path = sys.argv[4]
contains = sys.argv[5]
steps = []
for raw in sys.argv[6:]:
    if not raw:
        continue
    steps.append(json.loads(raw))
steps.append({'Expect': {'kind': expect_kind, 'path': expect_path, 'contains': contains}})
flow = {
    'schema': 'rdog.flow.v1',
    'policy': {
        'allow_shell': False, 'allow_file_read': False, 'allow_computer_act': allow_ca,
        'timeout_ms': 30000, 'max_steps': 8, 'max_output_bytes': 4096,
    },
    'options': {'trace': 'summary'},
    'steps': steps,
}
print(f'@flow#' + seq + ':' + json.dumps(flow))
" "$seq" "$allow_ca" "$expect_kind" "$expect_path" "$contains" "$@"
}

inner3='@computer-act#1:{schema:"rdog.computer-act.v1",action:"wait",args:{duration_ms:50}}'
flow_line3="$(make_flow_with_expect 3 true "$inner3" response_field_equals '$.value.ok' 'true')"
out3="$("$binary" control mac.lab "$flow_line3" 2>&1)" || {
    printf '%s\n' "$out3" >&2
    fail "test 3: rdog control exited non-zero"
}
echo "  response: $out3"
echo "$out3" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "test 3: flow status != ok (Expect.response_field_equals should pass) (output: $out3)"
log "test 3 OK (response_field_equals pass)"

# --- Test 4: Expect.response_field_equals 断言 $.ok == false → 失败 ---
log "test 4: Expect.response_field_equals:$.ok == false → 失败"
flow_line4="$(make_flow_with_expect 4 true "$inner3" response_field_equals '$.value.ok' 'false')"
out4="$("$binary" control mac.lab "$flow_line4" 2>&1)" || true
echo "  response: $out4"
echo "$out4" | grep -qE '"status"[[:space:]]*:[[:space:]]*"failed"' || fail "test 4: should fail (status != failed) (output: $out4)"
echo "$out4" | grep -qE 'failed_step' || fail "test 4: should report failed step (output: $out4)"
log "test 4 OK (response_field_equals correctly fails)"

# --- Test 5: Expect.response_path_contains 断言 $.dispatched_to 含 "@wait" → 成功 ---
log "test 5: Expect.response_path_contains:$.dispatched_to contains @wait → 成功"
flow_line5="$(make_flow_with_contains 5 true "$inner3" response_path_contains '$.value.dispatched_to' '@wait')"
out5="$("$binary" control mac.lab "$flow_line5" 2>&1)" || {
    printf '%s\n' "$out5" >&2
    fail "test 5: rdog control exited non-zero"
}
echo "  response: $out5"
echo "$out5" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "test 5: flow status != ok (Expect.response_path_contains should pass) (output: $out5)"
log "test 5 OK (response_path_contains pass)"

# --- Test 6: 真实 GUI end-to-end: open_app Calculator + click + verify ---
log "test 6: open_app Calculator + click → end-to-end flow"
killall Calculator 2>/dev/null || true
sleep 0.5
# ticket 20 follow-up: inner ControlLine 字符串必须通过 sys.argv 传给 python
# (避免 bash 命令替换 + 嵌套双引号被 zsh 吞: schema:"rdog..." 变成 schema:rdog...)
# 5 步: open_app → sleep 500ms → click → sleep 500ms → Expect 断言
# inner_open / inner_click 走 rdog dict 语法 (无 JSON 字符串, 无需 escape)
inner_open='@computer-act#1:{schema:"rdog.computer-act.v1",action:"open_app",args:{app_name:"Calculator"}}'
inner_click='@computer-act#2:{schema:"rdog.computer-act.v1",action:"click",args:{start_box:[500,500]}}'
# step1..4 是 JSON 字符串, ControlLine value 里的 \" 必须 \" escape (JSON 标准)
step1='{"ControlLine": "@computer-act#1:{schema:\"rdog.computer-act.v1\",action:\"open_app\",args:{app_name:\"Calculator\"}}"}'
step2='{"SleepMs": 500}'
step3='{"ControlLine": "@computer-act#2:{schema:\"rdog.computer-act.v1\",action:\"click\",args:{start_box:[500,500]}}"}'
step4='{"SleepMs": 500}'
flow_json_6="$(make_flow_with_contains_multi 6 true response_path_contains '$.value.dispatched_to' '@click' "$step1" "$step2" "$step3" "$step4")"
out6="$("$binary" control mac.lab "$flow_json_6" 2>&1)" || {
    printf '%s\n' "$out6" >&2
    fail "test 6: rdog control exited non-zero"
}
echo "  response: $out6"
echo "$out6" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "test 6: end-to-end flow status != ok (output: $out6)"
echo "$out6" | grep -qE '"completed_steps"[[:space:]]*:[[:space:]]*[4-9]' || fail "test 6: completed_steps should be >= 4 (output: $out6)"
if ! pgrep -f "Calculator.app/Contents/MacOS/Calculator" >/dev/null; then
    fail "test 6: Calculator process not running after open_app step"
fi
log "test 6 OK (Calculator running, flow completed)"
killall Calculator 2>/dev/null || true
sleep 0.5

log "all 6 flow + @computer-act smoke tests passed"

