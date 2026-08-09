#!/usr/bin/env bash

set -euo pipefail

# 这个脚本把 rdog 发布流程固化: cargo install -> 固定签名身份重签 -> DR 校验。
# 解决 macOS TCC 授权随 cdhash 每次重编失效的问题, 方案见
# specs/rdog-stable-signing-identity.md (issue #40)。
#
# 注意: 首次切换到固定身份后, 需要去系统设置重新授权一次
# Accessibility + Screen Recording; 之后重编/重装/升级不再需要授权。

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_bin="${RDOG_CARGO_BIN:-cargo}"
identifier="${RDOG_IDENTIFIER:-rdog}"
signature_dr="designated => identifier \"$identifier\""
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
installed_bin="${RDOG_INSTALLED_BIN:-$cargo_home/bin/rdog}"

log() { printf '[install-signed] %s\n' "$*"; }
fail() { printf '[install-signed] error: %s\n' "$*" >&2; exit 1; }

# 1. 发布安装 (force 覆盖旧版本)
log "cargo install --path ."
"$cargo_bin" install --path "$repo_root" --force

# 2. 固定签名身份重签: adhoc + 固定 identifier + 自定义 DR (不钉 cdhash)。
#    重编后 cdhash 会变, 但 DR 字节级稳定, TCC 授权按 DR 匹配得以保留。
[ -f "$installed_bin" ] || fail "未找到已安装的二进制: $installed_bin"
log "codesign: identifier=$identifier"
codesign -f -s - --identifier "$identifier" --requirements "=$signature_dr" "$installed_bin"

# 3. fail-closed 校验: 提取 canonical DR (去 # 前缀与引号) 必须精确匹配,
#    否则中止, 避免装出"未授权身份"的二进制。
#    注: codesign 显示格式随 identifier 内容变化 (纯字母数字省略引号),
#    因此先归一化再比较, 不依赖显示细节。
actual="$(codesign -d --requirements - "$installed_bin" 2>&1 | sed 's/"//g' | grep -o 'designated => identifier [A-Za-z0-9._-]*' | head -1)"
expected="designated => identifier $identifier"
log "DR check: $actual"
[ "$actual" = "$expected" ] || fail "签名身份校验失败: 期望 '$expected', 实际 '$actual'"

log "完成: $installed_bin (签名身份稳定, 后续重装无需重新授权 macOS 权限)"
