## [2026-05-20 20:00:59] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: P3 semantic re-find brownfield 事实

## 来源

### 来源1: Roadmap

- 文件: `specs/rdog-observation-scoped-refmap-plan.md`
- 要点:
  - Observation 的来源包括 `@screenshot include_ax:true`、`@ax-tree`、`@window-find` 和未来 `@observe`。
  - P3 semantic re-find 的规则是: 先查当前 observation 的 ref,不存在则看 selector; 多候选返回候选集; 只有单候选且置信度足够高才允许自动 re-find; 低置信度必须交给 agent。
  - 错误契约禁止用普通 invalid args 掩盖 stale,也禁止 silent fallback 偷换候选。

### 来源2: P2 落地记录

- 文件: `task_plan__observation_refmap_p2.md`, `WORKLOG__observation_refmap_p2.md`, `ERRORFIX__observation_refmap_p2.md`
- 要点:
  - P2 已落地 `rdog.selector.v1` stable schema、`sel-v1-*` stable selector id、真实 durable history。
  - P2 已接入 `@selector-get` 和 `@selector-resolve` dry-run。
  - P2 reviewer 曾要求补齐 0 候选 / 多候选 / 权限与 unsupported 的结构化错误,这已经修复并通过复审。
  - P2 明确把 confidence ranking、automatic semantic re-find、action by selector 延期到 P3/P2b。

### 来源3: 当前代码触点

- 文件: `src/control_observation.rs`
- 要点:
  - `SelectorResolveRequest` 当前只有 `selector_id`, `limit`, `dry_run`, `include_explanations`。
  - `build_selector_resolve_response_json()` 明确拒绝 `dry_run:false`,错误码是 `SELECTOR_ACTION_DEFERRED`。
  - `finalize_selector_resolve_response_json()` 当前对 0 候选返回 `SELECTOR_NOT_FOUND`,对多候选返回 `AMBIGUOUS_SELECTOR`,单候选才返回 `selector-resolve` 成功。
  - window 和 AX backend 已能返回 candidate,并带 `matched_fields`, `missing_fields`, fresh `observation_id + ref`。
  - `selector_error()` 的 suggestion 仍然是通用提示,未区分 P3 的 auto-refind / candidate decision / verify recipe。

- 文件: `src/control_observation/selector.rs`
- 要点:
  - `PermanentSelector` 已包含 `constraints`, `hints`, `source`, `redaction`。
  - 当前 schema 有 anchors 字段,但 P2 resolver 主要用 app/window/element constraints。

- 文件: `src/control_observation/durable.rs`
- 要点:
  - durable index 已保存 `selector_id`, `fingerprint`, `last_seen_unix_ms`, `permanent_selector`。
  - 已有 `selector_by_id`, `selector_last_seen`, `selector_history`。

## 综合发现

### P3 应该解决的问题

- 把 P2 dry-run candidate 变成可解释的 re-find decision。
- 给 candidate 增加 scoring / confidence / reason,并定义阈值和人工决策边界。
- 给 stale / expired ref 增加明确 recovery route,让 agent 能从旧 ref 错误进入 `selector-get -> selector-resolve/refind -> fresh ref -> verify`。
- 允许 gated auto re-find,但不能直接执行 side-effect action。

### P3 不应该做的事情

- 不新增完整 `@observe` 总入口。那是 P4。
- 不把 mouse command 全部改成 ref/selector target。那是 P5。
- 不让旧 `@eN` 复活。P3 只能返回新 observation 中的新 ref。
- 不把 selector 多候选自动选成一个。

### 推荐 P3 主方案

- 继续改良 P2 surface,新增 `@selector-refind` 或扩展 `@selector-resolve` 的 `mode:"refind"`。
- 更稳的路线是新增显式 `@selector-refind`,避免把 P2 dry-run 的只读契约变得含糊。
- `@selector-refind` 返回 `rdog.selector.refind.v1`,包含 `decision`, `confidence`, `threshold`, `candidates`, `selected_candidate`, `fresh_target`, `verify_hint`, `reason_codes`。
- action 命令暂时仍然用 fresh ref 执行,不在 P3 默认支持 `target:{selector_id}` 直接 action。

## [2026-05-20 20:05:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Architect 审查采纳项

## 来源

### 来源1: Architect reviewer

- 结论: APPROVE,带非阻塞补强建议。
- 要点:
  - 新增 `@selector-refind` 的主要风险是命令面变复杂,但它换来更干净的语义层。
  - deterministic scoring 不能停留在权重建议,必须被 fixtures、reason codes 和 golden 约束住。
  - `fresh_target` 的消费规则必须写死: 它只是新 ref,不代表动作已成功。
  - P3 wire surface 推荐把权限不足、backend unsupported、schema unsupported 收束成 `decision:"blocked"`,只有 parse / invalid payload 走协议错误。

## 综合发现

- 最终计划需要把 conformance surface 写得更硬,否则 P3 可能变成调参数工程。
- `verify_hint` 应成为默认 agent workflow 的一部分,不是可有可无的建议。

## [2026-05-20 20:09:31] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Critic ITERATE 必须项

## 来源

### 来源1: Critic reviewer

- Verdict: ITERATE。
- 必须修改:
  - `decision:"blocked"` 规则要强制化。parse / invalid payload 走 protocol error; permission denied / backend unsupported / selector schema unsupported / capability blocked 统一返回正常 `selector-refind` response,`decision:"blocked"`。
  - `blocked` 必须没有 `fresh_target`,并返回 blockers、recovery_recipe、permission / capability / backend / schema 说明。
  - `decision:"rebound"` 必须有 required `verify_hint`,且 `@selector-refind` 不得返回 action success / action verified 字段。
  - scoring table 要版本化为 conformance surface,例如 `scoring_version:"rdog.selector.score.v1"`。
  - 每个 score source 必须有固定 reason code,每个 hard gate 必须有 fixture/golden,权重或 hard gate 改动必须更新 golden。
  - 需要稳定 tie-break 规则,避免同分候选排序不稳定。

## 综合发现

- Critic 不是反对 `@selector-refind`,而是要求把恢复决策做成可锁定的协议表面。
- 最终计划必须让执行者很难“顺手”把 blocked 走成 error 或把 fresh_target 当动作成功。

## [2026-05-20 20:13:47] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 笔记: Architect 二审通过与补强

## 来源

### 来源1: Architect reviewer 第二轮

- Verdict: APPROVE。
- 认可项:
  - `decision:"blocked"` 已强制化。
  - `fresh_target` 已明确不是 action result。
  - `scoring_version:"rdog.selector.score.v1"` 已成为 conformance surface。
- 非阻塞建议:
  - 如果调用方跳过 `verify_hint`,必须产出可审计 evidence/log 字段。

## 综合发现

- 最终计划可以保持 `@selector-refind` 主路径。
- 跳过 verify 不能只靠文档提醒,至少要在执行计划和验收中要求 evidence/log。
