# 任务计划: rustdog architecture deepening review

## [2026-07-30 10:59:06] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [计划]: 探索并生成 HTML report

## 目标

读取 `CONTEXT.md`、相关 ADR 和真实调用链,提出 3-4 个 deepening candidates。每个候选必须包含 module/interface/depth/shallow/seam/adapter/leverage/locality/test surface 证据,并生成系统临时目录中的自包含 HTML。当前阶段不修改生产代码,不设计具体 interface。

## 阶段

- [ ] 阶段 1: 领域词汇、ADR、技能模板和 working tree 基线。
- [ ] 阶段 2: 有机探索 control execution、observation、target resolution、platform adapters。
- [ ] 阶段 3: 形成候选、做 deletion test、生成并打开 HTML report。
- [ ] 阶段 4: report 交付并询问用户选择候选,等待后续 grilling。

## 约束

- `@computer-act` 保持 ADR 规定的 thin dispatcher,不建议把 OS adaptation 塞回 dispatcher。
- `@flow` 继续复用 ControlLine 语义,不提出重复 interface。
- CONTEXT 中的 Recorder、Recording Session、Recording Journal、Replay Script、Semantic Promotion、Participating Window 等词保持原义。
- 报告只落 `$TMPDIR/architecture-review-<timestamp>.html`,不落 repo。
- CodeGraph/Explore runtime 不可用时,明确走本地静态读取,不伪造 agent 结果。

## 当前状态

**阶段 2 进行中**: 读取真实调用链和测试 surface。

## [2026-07-30 11:05:28] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [阶段完成]: HTML report 已生成

- [x] 阶段 1: CONTEXT、ADR、spec、codebase-design vocabulary 和 working tree 边界已读取。
- [x] 阶段 2: control execution、Observation、target evidence、platform adapter 调用链已探索。
- [x] 阶段 3: 4 个候选已完成 deletion test、strength 分级和 before/after 图。
- [x] HTML 已写入 `/var/folders/58/3f9_69ts3bx4slnb8tgl572m0000gn/T/architecture-review-20260730-110448.html` 并打开。
- [ ] 阶段 4: 等待用户选择候选;选择后进入 `$grilling`。

**状态: waiting for candidate selection**。
