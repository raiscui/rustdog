## [2026-07-29 11:30:00] [Session ID: omx-1784512435044-92wxat] 任务名称: Simplify Wayfinder overdesign (ticket #13)

### 任务内容

扫描已 close Wayfinder resolution spec 的过度设计抽象,出简化方案,每项标 ceiling + upgrade trigger。

### 完成过程

- gh issue list 确认 next ticket #13。
- 创建 ticket #13 + 支线 task_plan__wayfinder_overdesign_simplification.md。
- 扫描 4 个 closed spec: #4 (290行) / #9 (575行) / #8 (prototype) / #7 (439行)。
- grep 计数 reason code (16 in #4, 7 in #9) + gate (43 in #4) + fixture (20 in #7) + sections (17 in #7) 等。
- 写 spec `specs/rdog-wayfinder-overdesign-simplification.md` (197 行),包含 4 大简化章节,每个 ceiling + upgrade trigger 表。
- AGENTS.md 追加新规格索引。
- commit 9573d82 (auto-optimize 分支,不 push)。
- 关闭 ticket #13 + 更新 Wayfinder map #2。

### 简化摘要

- #4: 8 gates → 5 gates (permission+application 合并, window+geometry 合并); 11 reject codes → 6 (window_* + geometry_precondition_failed → window_unresolved with structured_field)。
- #9: 删 warnings 字段; redaction_summary 4 → 1 (segment_count only); 11 reject codes → 4 (其余复用 #4 namespace)。
- #8: 移除 stub / emit-time / full 三个实现标签(11 pass 不变,只去标签)。
- #7: 3 soak → 1 (release tag only); 3-section markdown report → 1 JSON block; 9 硬条件 → 6 (性能预算不是 SLA,合并为单一 gate)。

### 总结感悟

- 已 close 的 spec 不能直接修改,需要新 ticket 记录简化方案。Wayfinder map 的 closed ticket 是 immutable history,简化通过新 ticket 引导后续实施。
- ponytail 风格的 ceiling + upgrade trigger 表让"删除"决策可审计,避免 YAGNI 简化的代价隐形。
- spec 体量从 575+439+290+ prototype 缩减目标(如果实施后)合计可减 30-50%,主要来源是 redundant reason codes、YAGNI warning 字段、多余 report 渲染。
- 简化不影响 fail closed 安全语义。所有改动都是"删抽象、不改行为"。
