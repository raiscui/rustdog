---
title: 交互终端绿 + CI 红 = 环境决定性失败, 不是 flake (TERM=dumb 假 flake)
date: 2026-08-28
last_updated: 2026-08-28
module: control-client
component: control_tty tests / rustyline
problem_type: test_failure
severity: medium
status: active
tags:
  - tty
  - term
  - flake
  - test-isolation
  - rustyline
  - environment
verified_by:
  - "受控实验 (2026-08-28): TERM=xterm-256color 单测 PASS / TERM=dumb 同单测 FAIL, 双向切换可复现"
  - "全量 cargo nextest 959 passed, 21 skipped (2026-08-28, 修复后分支首次完整绿灯)"
  - "逐字节比对: 失败输出与非 TTY 读取路径 for_each_buffered_line 的输出形状吻合"
related_solutions:
  - docs/solutions/best-practices/parallel-test-global-state-single-lock.md
root_cause: "rustyline 14.0.0 对 TERM=dumb 正确降级为无 raw mode 的整行读取; agent shell / CI 等非交互 harness 的 TERM 恰为 dumb, 而用户交互终端是 xterm-256color, 同一测试在两种环境确定性相反"
resolution_type: "测试显式固定其假设的环境: Command 增加 .env(\"TERM\", \"xterm-256color\") 与调用环境解耦, 生产代码零改动"
---

# 交互终端绿 + CI 红 = 环境决定性失败, 不是 flake (TERM=dumb 假 flake)

## Problem

`tests/control_tty.rs::control_cli_should_treat_arrow_keys_as_local_cursor_motion_in_tty`
自 2026-08-19 起在每轮全量门禁中失败, 被标记为 "疑似 TTY 时序竞态的 flake"。
实际行为是确定性的:

- 在用户交互终端 (TERM=xterm-256color 等) 里稳定通过;
- 在 agent shell / CI 等非交互 harness (TERM=dumb) 里稳定失败。

同一测试在两种环境里结果确定性相反, 却被当作随机 flake 处理了 9 天,
污染了多轮全量验证的判定 ("955/956, 唯一失败为既有 flake" 类结论)。

## Symptoms

- 断言失败输出: 远端收到 `@png\u{1b}[D\u{1b}[Di\u{1b}[C` (期望 `@ping`),
  方向键 ESC 序列全部原样透传到控制行, 没有被本地行编辑消费;
- 测试经 `script -q /dev/null` 提供 PTY, rdog CLI 的 stdin 在 PTY 内确为终端
  (实验: pipe 进 script, 内部 `test -t 0` 仍为 true), 排除 "根本没拿到 TTY" 方向。

## What Didn't Work

- **H1 raw-mode 启用竞态**: 被实验推翻。script 内 stdin 确为终端;
  且若只是竞态, 不会表现为整行 ESC 序列全部透传 (而是偶发部分失效)。
- **H3 代码回归**: 被 git log 排除。`control_client_input.rs` 与 rustyline 14.0.0
  自 8 月初零变动, 失败却从 8-19 才开始进入观察视野。
- 误判本身的教训: "疑似时序竞态 flake" 的标签一旦贴上, 后续每轮全量
  都会自动豁免这个失败, 无人再质疑标签本身。

## Verified Root Cause

静态证据: rustyline 对 `TERM=dumb` 降级为无 raw mode 的整行读取,
方向键序列不做本地解释 -- 这是正确的生产行为 (dumb 终端确实不支持行编辑)。
非交互 harness (agent shell / CI) 的 TERM 恰好是 dumb。

动态证据 (受控实验, 2026-08-28):

- `TERM=xterm-256color` 运行该单测 -> PASS;
- `TERM=dumb` 运行同一单测 -> FAIL;
- 失败输出逐字节吻合非 TTY 读取路径 (`for_each_buffered_line`) 的产物形状,
  直接锁定 "rustyline 行编辑路径未启用" 分支。

## Solution

测试显式固定自己假设的环境: `tests/control_tty.rs` 的 Command 增加
`.env("TERM", "xterm-256color")`, 与调用环境解耦。

测试的意图是模拟 "支持方向键的交互终端"; 继承 harness 的 dumb TERM
等于自我破坏模拟前提。生产代码零改动 (rustyline 的 dumb 降级是正确行为, 不修)。

## Why This Works

测试不再依赖调用方环境里 TERM 的取值。无论 harness 是交互终端
(xterm 系) 还是 agent/CI (dumb), 测试内部环境恒定为其断言所假设的形态,
断言对象从 "rustyline 在当前 TERM 下的行为" 收窄回测试真正想验证的
"交互终端里方向键是本地光标运动"。

## Verification

- `TERM=dumb` 下该单测 PASS (修复后);
- 默认环境下该单测 PASS;
- 全量 `cargo nextest run`: 959 passed, 21 skipped -- 分支工作以来首次完整绿灯;
- 受控实验本身可复现根因: 对未修复版本分别设 TERM 两个取值, 结果确定性相反。

## Prevention

- **指纹识别**: "在交互终端里是绿的" + "在 agent/CI 里是红的" =
  环境决定性失败, 不是 flake。第一反应应是排查环境差异 (TERM/LANG/TTY/HOME),
  而不是调时序或重跑抽签。
- **测试显式固定假设环境**: 任何依赖终端能力的测试都应显式设置
  TERM (以及同类 LANG/TTY 假设), 不继承 harness 环境。
- **输出形状直指代码分支**: 失败输出与哪条代码路径的产物逐字节吻合,
  是快速锁定分支的证据; 本次 ESC 整行透传 == 整行读取路径的形状,
  直接排除了 raw-mode 竞态假设。
- 贴上 flake 标签前先问: 它在两个已知不同的环境里是否各自稳定?

## Related

- `tests/control_tty.rs:84` (`.env("TERM", "xterm-256color")` 修复点, 带注释)
- ERRORFIX.md `[2026-08-28 15:10:00]` 完整假设-证伪链记录
- 同属测试隔离主题但机制不同: [parallel-test-global-state-single-lock](../best-practices/parallel-test-global-state-single-lock.md)
  (那条是测试之间共享进程全局状态, 本条是测试继承调用方环境)
