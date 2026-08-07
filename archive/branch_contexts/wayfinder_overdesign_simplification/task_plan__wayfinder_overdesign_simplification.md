# 任务计划: Simplify Wayfinder overdesign (ticket #13)

## 目标

固化 7 个 closed Wayfinder sub-ticket 规格的过度设计抽象合并/删除/推迟决策,每个简化标注 ceiling 与升级触发条件。

## 阶段

- [x] 阶段 1: 任务计划与设置,创建 ticket #13
- [x] 阶段 2: 静态证据扫描(spec 各章节计数 + overdesign 候选)
- [ ] 阶段 3: 写最小 spec
- [ ] 阶段 4: commit + push + close ticket + update map

## 关键问题

1. #4 8 gates 哪些可以合并?
2. #9 11 reject codes 哪些可以归到 4 个?
3. #9 warnings / redaction_summary 哪些字段 YAGNI?
4. #7 soak 场景 / acceptance report 哪些简化?
5. #8 pass 实现细节合并标签?

## 做出的决定

- 沿用 ponytail ladder,每个改动标 ceiling + 升级触发条件
- 不改原 spec,在新 spec 里描述删减/合并,作为新 ticket 的 resolution asset

## 遇到错误

- 无

## 状态

**当前在阶段 3**: 写 spec。

## [2026-07-29 11:30:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: ticket #13 simplification delivered

- [x] 写正式规格 `specs/rdog-wayfinder-overdesign-simplification.md` (197行)。
- [x] AGENTS.md 追加长期文件索引。
- [x] git commit 9573d82 (auto-optimize 分支, scope 限定 specs + AGENTS.md)。
- [x] gh issue close 13 with full resolution comment。
- [x] Wayfinder map ticket #2 body 追加 ticket #13 entry,放在第一个位置。
- [x] 范围内dirty worktree未污染。
