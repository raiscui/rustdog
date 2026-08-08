#!/usr/bin/env bash
# scripts/runner/eval-macos-ops.sh
#
# 5 model x 8 case macOS ops live matrix entry.
#
# 默认 dry 模式: 不调 Pi, 只 emit manifest + 验证骨架, 0 风险.
# live 模式: 真正起 daemon + 调 Pi + 拿 40 个 run.
#
# 用法:
#   runner/eval-macos-ops.sh dry                    # dry-run 整个 matrix
#   runner/eval-macos-ops.sh dry deepseek           # dry-run 单 model
#   runner/eval-macos-ops.sh live deepseek          # live 单 model (需要 API key)
#   runner/eval-macos-ops.sh live all               # live 整个 5x8 (40 run)
#
# 加新 model/case 必须确认完整目标列表 (不假装跑过子集).
# live mode 输出到 --output 目录 (默认 tmp), 包含 manifest.json + suite-result.json.

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
script="${1:-dry}"
target="${2:-all}"

mode="dry"
case "$script" in
  dry)  mode="dry" ;;
  live) mode="live" ;;
  -h|--help)
    head -20 "$0"
    exit 0
    ;;
  *)
    echo "usage: $0 {dry|live} [model-id|all]" >&2
    exit 1
    ;;
esac

# Prepend target/debug to PATH so any Pi-spawned `rdog` resolves to current binary.
# archive 5x8 lesson: candidate ledger 高于 baseline 是因为 Pi bash tool 没调用 current binary.
export PATH="$repo_root/target/debug:$PATH"

cd "$repo_root"
python3 -m runner.lib.runner \
  --config runner/config.json \
  --mode "$mode" \
  --output "${3:-}" \
  ${target:+--models "$target"}
