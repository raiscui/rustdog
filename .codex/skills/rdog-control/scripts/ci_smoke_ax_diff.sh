#!/usr/bin/env bash
# =====================================================================
# rdog ax-diff CI smoke 脚本
#
# 目的: 给 rdog ax-diff 子命令一个最小回归网, 在 CI / pre-commit /
# rdog-control skill 升级时验证三种输出格式、退出码、--top-changes
# 截断提示都符合 expected summary 契约。
#
# 用法:
#   ./scripts/ci_smoke_ax_diff.sh
#   RDOG_BIN=./target/release/rdog ./scripts/ci_smoke_ax_diff.sh
#
# 环境变量:
#   RDOG_BIN     rdog binary 路径 (默认 ./target/debug/rdog)
#   SMOKE_DIR    临时目录 (默认 $TMPDIR/rdog_ax_diff_smoke)
#
# 退出码:
#   0 = 全部 smoke 通过
#   1 = 有 smoke 失败
#   2 = 环境错误 (找不到 rdog binary / fixture 等)
# =====================================================================

set -u

# 关键: 不开 -e, 因为我们想跑完全部 smoke 后再决定退出码。
# 也不开 pipefail, 避免一个测试 grep miss 整条 pipeline 失败。

RDOG_BIN="${RDOG_BIN:-./target/debug/rdog}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
EXAMPLES_DIR="$SKILL_DIR/examples"
BEFORE="$EXAMPLES_DIR/xhs_before.json"
AFTER="$EXAMPLES_DIR/xhs_after.json"
SMOKE_DIR="${SMOKE_DIR:-${TMPDIR:-/tmp}/rdog_ax_diff_smoke}"

# 浅绿/浅红/复位 ANSI, 没有颜色时退化为空。
if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
    GREEN="$(tput setaf 2 2>/dev/null || printf '')"
    RED="$(tput setaf 1 2>/dev/null || printf '')"
    RESET="$(tput sgr0 2>/dev/null || printf '')"
else
    GREEN=""; RED=""; RESET=""
fi

# ---------------------------------------------------------------------
# 准备: 验证环境和 fixture
# ---------------------------------------------------------------------

if [ ! -x "$RDOG_BIN" ]; then
    printf '%sERROR%s: rdog binary 不存在或不可执行: %s\n' "$RED" "$RESET" "$RDOG_BIN" >&2
    printf '提示: 先跑 `cargo build`, 或设置 RDOG_BIN=./target/release/rdog\n' >&2
    exit 2
fi

if [ ! -f "$BEFORE" ] || [ ! -f "$AFTER" ]; then
    printf '%sERROR%s: fixture 缺失: %s 或 %s\n' "$RED" "$RESET" "$BEFORE" "$AFTER" >&2
    exit 2
fi

mkdir -p "$SMOKE_DIR"

# 准备一份"自身"的副本, 用来测 "无差异" 退出码
SAME="$SMOKE_DIR/same.json"
cp "$BEFORE" "$SAME"

PASS=0
FAIL=0
FAILED_TESTS=()

# ---------------------------------------------------------------------
# smoke_test <name> <expected_exit_code> <expected_substring> -- <command...>
# ---------------------------------------------------------------------

smoke_test() {
    local name="$1"
    local expected_exit="$2"
    local expected_substring="$3"
    shift 3
    # 吃掉 "--" 分隔符
    if [ "${1:-}" = "--" ]; then
        shift
    fi
    local actual_exit=0
    local actual_stdout=""
    local actual_stderr=""
    local tmp_out="$SMOKE_DIR/${name}.out"
    local tmp_err="$SMOKE_DIR/${name}.err"
    if "$@" >"$tmp_out" 2>"$tmp_err"; then
        actual_exit=0
    else
        actual_exit=$?
    fi
    actual_stdout="$(cat "$tmp_out")"
    actual_stderr="$(cat "$tmp_err")"
    if [ "$actual_exit" = "$expected_exit" ] && { [ -z "$expected_substring" ] || printf '%s' "$actual_stdout$actual_stderr" | grep -qF -- "$expected_substring"; }; then
        printf '%sPASS%s  %s\n' "$GREEN" "$RESET" "$name"
        PASS=$((PASS + 1))
    else
        printf '%sFAIL%s  %s (期望 exit=%s, 实际=%s; 期望包含="%s")\n' \
            "$RED" "$RESET" "$name" "$expected_exit" "$actual_exit" "$expected_substring"
        printf '  --- stdout ---\n%s\n' "$actual_stdout" | sed 's/^/  /'
        if [ -n "$actual_stderr" ]; then
            printf '  --- stderr ---\n%s\n' "$actual_stderr" | sed 's/^/  /'
        fi
        FAIL=$((FAIL + 1))
        FAILED_TESTS+=("$name")
    fi
}

printf 'rdog ax-diff CI smoke\n'
printf '  binary:  %s\n' "$RDOG_BIN"
printf '  before:  %s\n' "$BEFORE"
printf '  after:   %s\n' "$AFTER"
printf '  tmpdir:  %s\n\n' "$SMOKE_DIR"

# ---------------------------------------------------------------------
# 核心 smoke: 三种 format + 退出码 + 截断
# ---------------------------------------------------------------------

# 1) summary 格式: 数字契约。xhs_before -> xhs_after 有 2 个 element 改动。
smoke_test "summary_diff_has_two_modified_elements" \
    "1" \
    "elements: +0 -0 ~2" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format summary

# 2) text 格式: 必须包含 "AXShowMenu" (home link 改后新增的 action)
smoke_test "text_format_mentions_new_action" \
    "1" \
    "AXShowMenu" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format text

smoke_test "text_format_mentions_dot_ai_description_change" \
    "1" \
    "点点 ai" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format text

# 3) json 格式: 必须包含 actions 数组里 "AXPress" (说明是 AX element)
smoke_test "json_format_emits_valid_structured_diff" \
    "1" \
    "AXPress" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format json

# 4) --top-changes=1 截断: 末尾必须出现 "被截断" 提示
smoke_test "top_changes_one_truncates_with_hint" \
    "1" \
    "被截断" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format text \
    --top-changes 1

# 5) --top-changes 远大于实际数: 两个 element 改动都出现, 不出现截断提示。
# 这里用 smoke_test 期望 "AXShowMenu" (一个改动元素的内容) 必须出现,
# 间接验证 top_changes=999 不截断 (截断后就看不到了)。
smoke_test "top_changes_larger_than_total_does_not_truncate" \
    "1" \
    "AXShowMenu" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format text \
    --top-changes 999

# 6) 同文件: 无差异, 退出码 0
smoke_test "identical_snapshots_exit_zero" \
    "0" \
    "" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$SAME" \
    --format summary

# 7) --format summary 同文件也退出 0, summary 字符串稳定
smoke_test "identical_snapshots_summary_is_zero_change" \
    "0" \
    "elements: +0 -0 ~0" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$SAME" \
    --format summary

# 8) 缺 --after: 退出码 2 (用法错误)
smoke_test "missing_after_argument_exits_two" \
    "2" \
    "" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE"

# 9) 缺 --before: 退出码 2
smoke_test "missing_before_argument_exits_two" \
    "2" \
    "" \
    -- \
    "$RDOG_BIN" ax-diff \
    --after "$AFTER"

# 10) --format 非法值: 退出码 2
smoke_test "invalid_format_exits_two" \
    "2" \
    "" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$AFTER" \
    --format xml

# 11) JSON 解析失败: 退出码 3
BAD_JSON="$SMOKE_DIR/bad.json"
printf '{not valid json' >"$BAD_JSON"
smoke_test "bad_json_exits_three" \
    "3" \
    "" \
    -- \
    "$RDOG_BIN" ax-diff \
    --before "$BEFORE" \
    --after "$BAD_JSON"

# 12) 帮助文本: 退出码 0
smoke_test "help_text_prints" \
    "0" \
    "--before" \
    -- \
    "$RDOG_BIN" ax-diff --help

# 13) 观测稳定: 同一次跑 summary 数字与 text 截断前的元素数一致。
# 抽取出 text 输出, 统计 [element ...] 块数。
TEXT_OUT="$SMOKE_DIR/text_full.out"
"$RDOG_BIN" ax-diff --before "$BEFORE" --after "$AFTER" --format text --top-changes 999 >"$TEXT_OUT" 2>/dev/null
TEXT_ELEM_COUNT=$(grep -cE '^\[element ' "$TEXT_OUT" || true)
if [ "$TEXT_ELEM_COUNT" = "2" ]; then
    printf '%sPASS%s  text_and_summary_element_counts_agree (text=%s, summary=2)\n' "$GREEN" "$RESET" "$TEXT_ELEM_COUNT"
    PASS=$((PASS + 1))
else
    printf '%sFAIL%s  text_and_summary_element_counts_agree (text=%s, summary=2)\n' "$RED" "$RESET" "$TEXT_ELEM_COUNT"
    FAIL=$((FAIL + 1))
    FAILED_TESTS+=("text_and_summary_element_counts_agree")
fi

# ---------------------------------------------------------------------
# 收尾
# ---------------------------------------------------------------------

printf '\n总计: %d 通过, %d 失败\n' "$PASS" "$FAIL"
if [ "$FAIL" -gt 0 ]; then
    printf '%s失败的测试:%s\n' "$RED" "$RESET"
    for t in "${FAILED_TESTS[@]}"; do
        printf '  - %s\n' "$t"
    done
    exit 1
fi
printf '%s所有 smoke 通过%s\n' "$GREEN" "$RESET"
exit 0
