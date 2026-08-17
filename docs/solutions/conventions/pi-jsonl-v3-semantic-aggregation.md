---
title: "Pi JSONL v3 语义聚合合同"
date: 2026-08-13
last_updated: 2026-08-13
module: eval
component: pi-events-parser
problem_type: convention
severity: medium
status: active
tags:
  - pi-jsonl
  - jsonl-v3
  - event-aggregation
  - provider-route
  - multi-turn
verified_by:
  - "pi-rdog-calculator-eval/vendor/pi_events.py"
  - "runner/test_pi_events.py: 4 targeted regression tests"
  - "2026-08-12 legacy Rust Pi and upstream Pi artifact replay"
---

# Pi JSONL v3 语义聚合合同

## Context

旧 Rust Pi 和 upstream Pi v0.84.1 都使用 provider 生成、工具执行、tool result 回填、再次生成的 agent loop,但 JSONL envelope 的 route 和 turn 字段位置不同。评测器必须聚合事件语义,不能把某个版本的字段名当成跨版本协议。

## Guidance

- 只从完成的 `message_end` 读取 assistant content 和 tool calls,不要从 streaming delta 重复计数。
- 旧事件若在 `session` 中提供 `provider` 和 `modelId`,要求 session route 与预期完全匹配,并要求每个完成 assistant message 的 route 也匹配。
- upstream v3 session 可能只有 `id` 和 `cwd`;这时从每个完成 assistant `message_end` 的 `provider` 和 `model` 聚合 route,并要求所有完成 assistant message 都精确匹配预期 provider/model。
- `turn_end` 的数量表示有序 agent 回合。旧格式要求 `turnIndex` 全部存在且连续;v3 可全部缺失,但至少要有两个有序 `turn_end` 才能证明多轮。字段部分存在、部分缺失或索引不连续时 fail closed。
- 分开记录 `turn_end` 数量、assistant message 数量、bash/tool call 数量和 provider request 语义。`maxToolIterations` 只是上限,不能作为实际对话轮数。
- route 和多轮门槛通过后,仍需结合真实 tool result、rdog response 和 fresh AX/window/URL 证据完成 case 验收。

## Evidence

- `/Users/cuiluming/local_doc/l_dev/my/rust/pi-rdog-calculator-eval/vendor/pi_events.py::summarize_events` 实现了完成消息筛选、旧 session route、v3 assistant route、连续或缺失 `turnIndex` 的分支。
- `/Users/cuiluming/local_doc/l_dev/my/rust/pi-rdog-calculator-eval/runner/test_pi_events.py` 有 4 项定向回归测试,覆盖 legacy 连续索引、v3 无 session route、v3 无索引多轮和混合索引拒绝。
- 真实 upstream artifact `/tmp/pi-rdog-macos-ops-deepseek-20260812-005051/textedit-type-text--canonical-profile/attempt-1/pi-events.jsonl` 含完成 assistant route 和多条无 `turnIndex` 的 `turn_end`;修复后 route/multi-turn 判定为 true。
- 同一 TextEdit 多窗口意图的历史样本中,Rust Pi 为 7 个 `turn_end`、6 次工具执行;upstream 修订契约样本为 4 个 `turn_end`、3 次工具执行。两次 setup/prompt 不同,只能证明轨迹不同,不能证明 upstream runtime 固定少 3 回合。
- `python3 -m unittest runner/test_pi_events.py` 输出 4 passed;artifact 回放同时断言旧索引为 `0..6`、v3 索引全部缺失。

## Why This Matters

如果只读取旧 session 字段,upstream 已经完成的请求会被误判为 route failure;如果把缺失 `turnIndex` 当成单轮,真实多步 GUI 动作会被重复执行或被错误拒绝。统一语义聚合可以在不放宽真实动作和 fresh evidence 门槛的前提下兼容两种 envelope。

## When to Apply

- 评测 upstream Pi v3 JSONL 或同时支持 Rust Pi 与 upstream Pi 的 runner。
- 需要统计 agent 回合、工具执行、provider route 或判断 `multiTurnVerified` 时。

## When Not to Apply

- 解析完全不同的事件协议时,不能套用 `message_end`、`turn_end` 或 route 字段假设,应先建立该协议的完成事件合同。
- 仅做原始日志展示而不做 route、回合或工具语义判断时,不需要引入这套聚合门槛。

## Examples

```text
v3 route = every completed assistant.message_end(provider, model) matches expected
v3 turns = ordered turn_end count >= 2 and every turnIndex is absent
legacy turns = turnIndex values are exactly 0, 1, ..., N-1
```

最终报告应同时给出实际回合数和工具执行数,并注明 setup/prompt/case 变化;不能用一个固定的 `maxToolIterations` 数字代替 artifact 计数。

## Related

- [upstream Pi macOS ops CLI 合同](../tooling-decisions/upstream-pi-macos-ops-cli-contract.md)
- [多窗口 GUI 评测必须使用运行时窗口基线](../logic-errors/macos-ops-multi-window-runtime-baseline.md)
- `workflows/macos-ops-interaction-efficiency.md`
