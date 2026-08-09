---
title: 评测载体差异会被误判成模型退步
date: 2026-08-09
last_updated: 2026-08-09
module: eval
component: runner
problem_type: best_practice
severity: high
status: active
tags:
  - eval
  - regression
  - baseline
  - carrier
  - bisect
verified_by:
  - "2026-08-09 bisect: 老 8 case + archive 40/40 同款载体 + 当前 binary/skill -> deepseek 8/8, minimax-cn 8/8"
---

# 评测载体差异会被误判成模型退步

## Context

对 LLM/macOS 自动化 agent 做能力评测时,"模型退步"的结论经常来自评测载体变化,
而不是模型本身。载体包括: runner 实现、case 集、prompt/skill 版本、被测二进制
(daemon/control)、校验逻辑、环境状态。任一项变化都会改变分数,与模型能力无关。

典型场景: 某模型此前 7/8、8/8,现在分数下降,第一反应是"模型退步"。

## Guidance

收到"模型退步"报告时,先按以下顺序取证,再下结论:

1. **固定载体**: 用与历史结果完全相同的载体重跑 (同一 runner、同一 case 集、
   同一 skill/prompt 版本、同一被测二进制版本)。
2. **做 bisect**: 一次只换一个变量 (case 集、runner、binary、skill),定位分数差异来源。
3. **分开分类失败**: parser/API 缺口、模型遗漏动作、基础设施波动要分开,
   不要直接把未分类失败归为模型能力问题。
4. **载体变更后必须重新 baseline**: 任何载体升级 (runner、case、prompt、binary)
   后,历史分数不再可比,要重新建立 immutableBaseline。

结论句式: "模型没退步,是评测载体变了" 必须附载体对照证据
(同一 case 下新旧载体的分数、差异变量列表)。

## Evidence

- 2026-08-09 rustdog 评测 (issue 链): deepseek/minimax-cn 报告"退步"后,
  用外部 runner (archive 40/40 同款) + 老 8 case + 当前 binary + 当前 skill 重跑,
  得到 deepseek 8/8、minimax-cn 8/8;随后仓库 runner 对齐 archive 载体
  (per-model maxToolIterations、case 集、prompt、严格验证),live 全矩阵 40/40。
  → 模型没有退步, 是评测载体与历史记录不一致。
- immutableBaseline 更新为 40/40 (RPC 口径) 后才具备可比性。

## Why This Matters

误判模型退步会引发错误的方向: 换模型、调 prompt、改产品行为,
浪费大量时间;而真正的问题 (载体漂移) 无人处理,后续评测持续失真。

## When to Apply

- 任何"某模型分数下降"的报告。
- case 集、runner、prompt、binary 任一变更后的对比。
- 需要给用户/团队证明"是模型问题还是载体问题"时。

## When Not to Apply

- 同载体下多次复跑确实稳定下降,且排除基础设施波动 (此时才谈模型侧)。
- 纯推理任务无工具链/环境依赖的评测 (载体影响小)。

## Examples

rustdog 评测链: 仓库 runner (evaluation/ 或 scripts/) 与 archive 的 40/40 基准
共用同一载体契约 (case 集 + strict verification + per-model maxToolIterations)。
比对历史分数前先确认载体 hash 或配置一致。

## Related

- `workflows/macos-ops-interaction-efficiency.md` (交互效率评测口径)
- rustdog 评测 runner 与 immutableBaseline 配置
