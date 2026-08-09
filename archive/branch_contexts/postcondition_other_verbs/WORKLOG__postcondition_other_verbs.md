## [2026-07-31 02:00:00] [Session ID: current] 任务名称: postcondition 接入其他 verb (LATER_PLANS item 4) — YAGNI 撤销

### 决策

grilling #1 选 **C**:不接入 postcondition 到 @ax-set-value / @type-text。

### 理由 (grilling 推荐)

- 当前唯一使用方是 @ax-press
- postcondition 的语义对 @ax-set-value (verify text changed) 与 @ax-press
  (verify state changed) 不完全一致(角色 + expected value 在不同 verb 下
  表达不同含义)
- 在真正有第二个 caller 提出"我也要 verify"之前,抽出 / 扩展是
  speculative 复杂度
- Ponytail "YAGNI" 适用 —— 没有真实需求,就不预先抽象

### 关闭

LATER_PLANS item 4 (postcondition 接入其他 verb) 关闭,
不写入待办,等真正需求出现时再开新任务。

### 4 个 LATER_PLANS item 总结

| # | item | 状态 | commit |
|---|---|---|---|
| 1 | postcondition 抽取 | ✅ 完成 | d3ea584 |
| 2 | verb dispatcher helper 化 | ✅ 完成 | e5d117a |
| 3 | target locator seam | ✅ 完成 | 9973fae |
| 4 | postcondition 接入其他 verb | ❌ YAGNI 撤销 | — |
