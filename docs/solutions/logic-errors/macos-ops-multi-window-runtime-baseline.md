---
title: "多窗口 GUI 评测必须使用运行时窗口基线"
date: 2026-08-12
last_updated: 2026-08-12
module: eval
component: macos-ops-runner
problem_type: logic_error
severity: high
status: active
tags:
  - macos-ops
  - multi-window
  - runtime-baseline
  - textedit
  - verification
verified_by:
  - "2026-08-12 MiniMax M3: TextEdit 2 -> 3, one Cmd+N, all required checks passed"
  - "2026-08-12 Qwen 3.7: TextEdit 2 -> 3, one Cmd+N, all required checks passed"
root_cause: "setup 预建窗口且 prompt 写死绝对数量,而 verifier 使用相对增量;TextEdit 重启后还可能恢复旧窗口"
resolution_type: "以运行时 N 为唯一基线,让 prompt、setup 和 verifier 共同要求 N 到 N+1"
related_solutions:
  - ../best-practices/eval-carrier-drift-vs-model-regression.md
---

# 多窗口 GUI 评测必须使用运行时窗口基线

## Problem

`textedit-multi-window` 的旧 setup 会在模型运行前发送一次 `Cmd+N`,
但 prompt 要求窗口数从 1 增加到 2。旧 verifier 又只要求 after 大于 before。
TextEdit 的窗口恢复会让实际 before 变成 2、3 或更多,因此同一 case 同时存在三份不同状态定义。

## Symptoms

- MiniMax M3 和 Qwen 3.7 的旧 artifact 都在多窗口 case 失败,但均有 provider route、真实 rdog 调用和 fresh window evidence。
- 旧 attempt 的 before 窗口数为 2 到 4,不符合 prompt 声称的 1。
- Qwen 3.6 与 M2.7 在同一宽松 verifier 下可以通过,不能据此证明绝对窗口数契约成立。

## What Didn't Work

- 只改 prompt 为“从 2 增加到 3”: TextEdit 可能恢复更多窗口,新的绝对数字仍会再次失效。
- 只保留 `after > before`: 一次动作误建多个窗口也会被判为成功。
- 只依赖 `killall TextEdit`: 进程结束不等于下一次启动不恢复未命名窗口。

## Verified Root Cause

静态证据: runner 的旧 TextEdit setup 在捕获 before 前执行 `Cmd+N`;prompt 写死 1 到 2;verifier 只检查相对大于。

动态证据: 历史 M3/Qwen 3.7 artifact 的 before 分别记录到 2 到 4 个 TextEdit 窗口。修订后两份独立真实运行均从 2 增加到 3,模型只执行一次 `Cmd+N`,并通过 fresh window 验证。

## Solution

1. setup 只打开目标 app 并保存实际窗口数 N,不预建被测动作。
2. prompt 明确要求先读取 N,只执行一次新增动作,再读取 N+1。
3. verifier 必须要求 `after == before + 1`。
4. 新 setup 必须加入 finally cleanup 映射,并有 setup、精确增量和 cleanup 回归测试。

## Why This Works

运行时 N 是 setup、模型和 verifier 唯一共享的状态真相源。它允许 macOS 恢复旧窗口,但不允许评测把恢复状态误写成模型已经完成的新增动作。

## Verification

- `python3 -m unittest -v test_macos_ops_interaction test_run_macos_ops_eval test_upstream_pi_contract test_pi_events`
  - 52 tests passed。
- `ruff check run_macos_ops_eval.py test_run_macos_ops_eval.py test_upstream_pi_contract.py test_macos_ops_interaction.py test_pi_events.py`
  - no issues。
- `/tmp/pi-rdog-macos-ops-minimax-multiwindow-fixed-20260812-210439`
  - MiniMax M3: before 2, after 3,无 tool/rdog error。
- `/tmp/pi-rdog-macos-ops-qwen37-multiwindow-fixed-20260812-210555`
  - Qwen 3.7: before 2, after 3;case 通过,但有一次可恢复的短格式 rdog error。

## Prevention

- 新增任何多窗口 case 前,让 setup、prompt 和 verifier 都引用同一运行时基线。
- 评测失败时,先查看 before/after state artifact,再把结果归因到模型能力。
- finally cleanup 新增 setup 时,必须由定向测试覆盖映射。

## When to Apply

- macOS app 在退出后可能恢复窗口、标签页或文稿的评测。
- 验收目标是创建、关闭或改变一个可计数 GUI 资源的任务。

## When Not to Apply

- case 要求固定的绝对初始状态,且 setup 可以证明并强制该状态时。
- 单窗口只读检查,没有由模型产生的资源数量变化时。

## Related

- [评测载体差异会被误判成模型退步](../best-practices/eval-carrier-drift-vs-model-regression.md)
- `workflows/macos-ops-interaction-efficiency.md`
