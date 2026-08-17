# rdog macOS ops 交互效率

## 状态

已定稿。实现前不再需要补充产品决策。

## 目标

降低每个 macOS ops case 中 agent 发起的 `rdog control` 请求数和决策轮次。

正确性不能让步。每个成功 case 仍要有真实 rdog 动作和动作后的新鲜 AX、window 或 URL 证据。suite 总耗时只记录，不作为通过门槛。

这项工作优化共享 rdog 控制能力和 canonical skill 的通用控制难度。它不为任何 app、case 或用户任务写固定操作序列。

## 触发条件

只在以下内容变化后运行:

- 共享的 rdog control parser、协议或通用高密度 primitive。
- canonical `rdog-control` skill 的通用控制策略。
- macOS ops case 的定义。

无关改动不运行，也不按日程运行。

## 计量口径

- 一个 agent 决策是 Pi 产生的一个 bash tool call。所有这类调用都计入 `agentDecisionCount`。
- 一个 `rdog control` 请求是该 bash 调用中经 shell 解析后唯一可识别的 `rdog control` invocation。它计入 `requestCount`;一个调用可以含有多个 frame，仍只算一次请求。
- 不含 `rdog control` 的 bash 调用记录为 `supporting_shell`,计入 agent 决策但不计入 control 请求。这样 `sleep ...; rdog control ...` 等通用 shell 组合不会被错误排除。
- 同一 case 的所有 attempt 都计入成本，包括失败 attempt 和随后成功的 retry。
- runner 的 setup、before/after capture、cleanup 也可能调用 rdog，但它们不是 agent 请求，只保留为证据，不计入主指标。
- 同一 bash 含多个 control invocation,或 shell 无法可靠解析时是不可计量样本。认证必须失败，不能从 app 名或 case 文本猜测请求数。

## Interaction Ledger

单一事实来源是机器可读的 `rdog.macos-ops.interaction-ledger.v1`。它由每个 attempt 的 `pi-events.jsonl`、`pi-summary.json` 和 `run-result.json` 生成。Markdown 只生成摘要，不参与统计。

ledger 的 run metadata 必须包含:

- rustdog commit。
- canonical skill SHA-256。
- runner、配置和 case 文件的 SHA-256。
- provider、model、运行时间和输出目录。

每个 agent 决策必须记录:

- model、case id、attempt、调用序号和原始 bash command。
- `kind`: `rdog_control` 或 `supporting_shell`。
- `rdog_control` 的 `classification`: `query`、`action`、`post_action_evidence`、`recovery` 或 `unknown`;supporting shell 的分类固定为 `supporting_shell`。
- 对应的 tool result、rdog response、错误状态和原始 artifact 路径。

分类只能使用通用协议 verb、错误响应和相邻请求顺序:

- `query`: 只读协议请求，且不是动作后的验证或错误恢复。
- `action`: 含有通用状态改变 verb 的请求。
- `post_action_evidence`: 已有动作后、用于取得新鲜可验证状态的只读请求。
- `recovery`: 紧接同一 attempt 的 rdog 或 tool 错误后发起的请求。
- `unknown`: 无法从上述通用信息可靠归类的请求。

`unknown` 不等于冗余。分类不得读取 app 名、case id、expected result 或任务文本。

## Baseline 和比较

已认证 baseline ledger 不可修改。候选运行必须固定 provider、model、runner 配置和未变化的 case。rustdog commit 与 skill SHA-256 因候选改动而变化时，必须显式列入 comparison manifest。

新增或语义变化的 case 先建立独立 baseline。它们不参与旧矩阵的优化收益计算。未变化的 case 继续逐项与旧 baseline 比较。

baseline 存在 `../pi-rdog-calculator-eval/results/macos-ops-interaction/<baseline-id>/`。目录包含 ledger、comparison manifest、摘要和每个 case 的原始 JSONL/source artifact 副本，并随评测仓库提交。`/tmp` 只用于一次运行，不能作为长期审计来源。

一个候选通过认证，必须同时满足:

- 全部 5 个当前远程模型 × 8 个当前 case 都成功。LFM2.5-2.6B-OptiQ-4bit 已退出当前评测范围,历史结果不参与此门槛。
- 每个成功都具备真实 rdog 调用和新鲜结果证据。
- 未变化 case 的请求数不高于其 baseline。
- 矩阵总请求数严格下降。
- 个别 case 请求数增加时，必须明确证明新增项是不可替代的通用验证证据。
- recoverable protocol error 单独报告，不用最终成功掩盖。

## 候选门槛

只有同时满足以下两点的模式，才能进入补丁 brief:

1. 静态证据指向共享 rdog 或 canonical skill 层。
2. 同一通用意图或失败模式出现在至少两个独立的 `(model, case)` 样本中。

单次样本留在 ledger 中观察，不生成补丁候选。

## 工作流

1. 检查触发的改动是否属于允许范围，并读取最近已认证 baseline manifest。
2. 固定本次 comparison manifest。除候选改动和新增/变化 case 外，输入必须与 baseline 一致。
3. 开发阶段可运行定向样本诊断。它只用于定位，不可用于认证。
4. 从真实 attempt artifacts 生成 ledger，并在遇到不可计量样本时立即失败。
5. 用 ledger 查找跨样本的高交互模式。再从 rdog 或 skill 的共享调用链取得静态代码位置。
6. 达到候选门槛后，生成一份 decision brief。它包含共享摩擦点、代码位置、触发样本数、按角色分组的请求差额、协议错误差额、影响面、ledger 链接，以及批准、拒绝、暂缓三个决定。
7. 在获得批准前，不修改 rdog 协议行为或 canonical skill 控制策略。
8. 批准后，优先复用或改进共享 parser、协议或通用 primitive。仅当协议已能准确表达意图时，才缩短 skill 的通用决策路径。runner 只测量和门禁，不补偿协议缺口。
9. 为改动补最小回归测试。先跑定向测试，再运行全部 5 × 8 live matrix，并重新生成 ledger 与比较摘要。
10. 通过认证后，将 candidate ledger 和 comparison manifest 提升为新的不可变 baseline。失败则保留 artifacts，baseline 不变。

## 实现边界

当前评测工程位于 `../pi-rdog-calculator-eval`。优先复用现有路径:

- `runner/run_macos_ops_eval.py`: attempt 生命周期、artifact 落盘与 suite result。
- `vendor/pi_events.py`: Pi JSONL 的纯解析与聚合。
- `runner/test_run_macos_ops_eval.py`: runner 的回归测试入口。
- `runner/eval-macos-ops.sh`: 5 个当前远程模型的完整 live matrix 入口 (deepseek / minimax / qwen37 / qwen36 / m27hs)。`lfm25` profile 仅保留为历史配置,不得作为当前评测入口。
- `.codex/skills/rdog-control/SKILL.md`: 唯一 canonical skill。

没有证据证明现有 protocol 或 primitive 缺口前，不新增 command、app recipe、case 特例或独立补偿层。

## 完成定义

实现完成时应有:

- 可重复生成的 ledger 与 comparison manifest。
- 通过 fail-closed 计量和分类测试的 runner。
- 一份经批准、带静态和动态证据的共享层改动，或一份证明当前没有合格候选的 brief。
- 一次完整 5 × 8 live matrix 的认证结果及其新鲜证据。
- 若候选通过，新的 immutable baseline；若未通过，完整失败 artifacts 和未变的旧 baseline。
