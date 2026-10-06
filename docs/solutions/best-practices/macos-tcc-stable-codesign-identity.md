---
title: macOS 本地开发二进制的 TCC 授权身份稳定方案
date: 2026-08-09
last_updated: 2026-09-02
module: build/release
component: codesign
problem_type: best_practice
severity: medium
status: active
tags:
  - macos
  - tcc
  - codesign
  - cargo-install
  - adhoc-signing
  - authorization
  - responsible-process
verified_by:
  - "本机 codesign 取证 + 完整实施 (2026-08-09, rustdog issue #40)"
  - "live OCR e2e 实证补充 (2026-09-02, rustdog #102/#103): 重编覆盖重签 + 归属链两个新维度"
related_skills:
  - self-learning.macos-codesign-stable-dr-check
---

# macOS 本地开发二进制的 TCC 授权身份稳定方案

## Context

macOS 上通过 `cargo install --path ./`(或任意"重编后替换二进制"的方式)发布的 CLI/daemon,
如果依赖 Accessibility、Screen Recording 等 TCC 服务,每次重新编译安装后都会被系统要求重新授权。
原因:macOS TCC 按代码签名身份 (designated requirement, 简称 DR) 记录授权;
默认 adhoc 签名把 DR 钉死为 cdhash (二进制内容哈希),重新编译内容必变,系统视为"新程序"。

适用版本:macOS (TCC 机制长期稳定),任意 adhoc 签名构建流程 (cargo install、直接 cp 二进制等)。

## Guidance

让 DR 跨构建字节级稳定,即可让 TCC 授权跨版本保留。两条可行路径:

1. **固定 identifier + 自定义 DR (推荐, 零证书)**: 安装后重签,
   DR 只钉 identifier,不再钉 cdhash。

   ```bash
   codesign -f -s - --identifier "rdog" \
     --requirements '=designated => identifier "rdog"' \
     ~/.cargo/bin/rdog
   ```

   `--requirements` 的内联文本必须以 `=` 前缀,否则被当作文件路径解析。

2. **自签名证书**: DR 锚定证书 (certificate leaf),证书不变则 DR 稳定。
   多一个 825 天过期维护点,本地开发场景通常没必要。

两种路径都保留"一次性成本":首次切换到新身份后,需要在系统设置重新授权一次
Accessibility + Screen Recording,之后重编/重装/升级不再需要。

校验必须做 fail-closed:提取 canonical DR 归一化后精确匹配 identifier,
不能依赖 codesign 的显示格式 (纯字母数字 identifier 会省略引号)。

### 补充 (2026-09-02, live OCR e2e 实证的两个新维度)

**1. 重签只对单个文件生效, 重编即回退**: 对 `target/release/<bin>` 手动重签后,
任何一次 `cargo build` (哪怕只改了测试代码触发的增量重编) 都会把二进制覆盖回
默认 adhoc 签名, TCC 身份随之丢失——症状是"明明重签过却又失败了"。重签必须是
"构建之后、运行之前"的最后一步; live e2e 前置检查应断言 DR (见下方可复跑命令),
不要信任上次的重签。根治做法是重签只发生在安装副本上 (install-signed 流程),
target/ 下的开发副本每次构建后重签。

**2. DR 只解决"跨构建保留", 不解决"从哪生" (responsible process 归属链)**:
同一 DR 身份的二进制, 从用户终端手动 spawn 与从 agent shell / cargo test
spawn, Screen Recording 的可用性不同——macOS 按 responsible process 归属
判定权限。归属链不对时症状不是显式拒绝, 而是静默失败 (截屏返回空/旧帧, 或
"没有可截图的显示器")。缓解: 由已授权宿主 (如 Terminal) 承载被测进程
(`open -a Terminal <.command>` 模式, rustdog control_ax_e2e /
control_ocr_e2e 的 `*_VIA_TERMINAL` 先例)。

## Evidence

- 本机取证 (2026-08-09):
  - 默认 adhoc 签名: `codesign -d --requirements - <bin>` 输出
    `# designated => cdhash H"<hash>"`,cdhash 每次重编必变。
  - 固定身份验证: 两个内容不同 (sha256 不同) 的二进制签同一 identifier 后,
    `Internal requirements count=1 size=52` 完全一致,DR diff 仅路径行。
- 完整实施: rustdog `scripts/install-signed.sh` (cargo install -> 重签 -> fail-closed 校验),
  真实安装后 `DR check: designated => identifier rdog` 通过;破坏签名副本被拒绝。
- 可复跑验证:

  ```bash
  # 签名后断言 DR (归一化提取, 不依赖显示格式)
  codesign -d --requirements - <bin> 2>&1 \
    | sed 's/"//g' \
    | grep -o 'designated => identifier [A-Za-z0-9._-]*' | head -1
  # 期望输出: designated => identifier rdog
  ```

## Why This Matters

不处理的话,每次重编都要人工重新授权,且 agent 驱动的开发流程中授权失效
会在无人值守时阻塞自动化;更糟的是,权限失效往往表现为"daemon 功能神秘不可用",
排查成本远高于一次签名固化。

## When to Apply

- macOS 本地 CLI/daemon 开发,依赖 TCC 服务 (Accessibility、Screen Recording、Input Monitoring 等)。
- 用 `cargo install --path`、直接复制二进制等方式发布。
- 需要让升级/重装不打断权限授权。

## When Not to Apply

- App Store / notarization / Developer ID 分发 (有各自完整签名链)。
- 企业 PPPC / MDM 管控环境 (走配置 profile,不应自签)。
- Linux / Windows (无 TCC 机制)。
- 生产签名证书 (应继续用正式 identity,不要用本方案的 adhoc 身份)。

## Examples

rustdog 的 `scripts/install-signed.sh`: install -> `codesign -f -s - --identifier "rdog"
--requirements '=designated => identifier "rdog"'` -> canonical DR 归一化断言,
失败即中止 (fail-closed)。完整流程见该脚本与 specs/rdog-stable-signing-identity.md。

## Related

- skill: `self-learning.macos-codesign-stable-dr-check` (可执行校验流程与三个坑)
- spec: `specs/rdog-stable-signing-identity.md` (rustdog issue #40)
- 背景: `specs/rdog-macos-operation-capture-research.md` (TCC/AX 权限生命周期)
