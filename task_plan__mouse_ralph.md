# 任务计划: rdog mouse control Option A Ralph 实现

## [2026-05-14 11:08:32] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: Ralph 实施启动

### 目标
- 按 `.omx/plans/rdog-mouse-control-implementation-plan.md` 的 Option A 实现鼠标控制。
- 让 `@mouse-move`、`@mouse-button`、`@click`、`@drag`、`@wheel` 复用 screenshot manifest 的 `os-logical` 坐标语义。
- 成功响应必须是结构化 mouse value,不能只返回 `@response 0`。
- 完成后必须有 fresh verification evidence,并清理 Ralph active state。

### 阶段
- [x] 阶段0: 恢复 Ralph 状态和读取 approved plan
- [ ] 阶段1: 协议类型、parser 和 parser tests
- [ ] 阶段2: 鼠标执行层、backend facade、structured response
- [ ] 阶段3: control core / integration tests
- [ ] 阶段4: README、spec、global skill 同步
- [ ] 阶段5: fmt、focused tests、全量相关验证
- [ ] 阶段6: 本地 architect/deslop review 和 Ralph state complete

### 当前约束
- 默认 `task_plan.md` 已 992 行,本轮只写 `__mouse_ralph` 支线上下文。
- 不新增第二套坐标体系。
- `@mouse-button mode:"press"` 不自动 release。
- `@drag` press 后失败必须尝试 release 并报告 recovery 状态。
- 若 backend 无法证明多显示器/负坐标 absolute 支持,必须返回 Unsupported / code 78,不能 clamp。

### 状态
**目前在阶段1** - 准备阅读协议/执行层源码并实现 parser。

## [2026-05-14 11:37:17] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 用户要求 review 当前 diff 后做 local commit

### 当前目标
- 先对当前未提交 diff 做阻断性 review。
- 如无必须修复的问题,只提交本次代码/文档改动,不提交支线运行态上下文文件。
- 不执行 push。

### 阶段
- [ ] 阶段A: 读取 diff 和关键实现,找出阻断项
- [ ] 阶段B: 运行提交前验证
- [ ] 阶段C: local commit 并复核状态

### 状态
**目前在阶段A** - 正在检查 `control_mouse`、协议接线、结构化响应和测试覆盖。

## [2026-05-14 11:49:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: review 后补齐提交前修复

### 已完成
- 发现并修复 drag 插值中 `i32` 先相减可能溢出的风险。
- 补充极端坐标插值测试。
- 补齐 README / control-line protocol / code-agent usage 中的鼠标控制表格、坐标语义和权限说明。

### 状态
**目前在阶段B** - 运行 fmt、focused tests、bin tests、integration compile 和 diff check。

## [2026-05-14 11:51:05] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: review 和 local commit 完成

### 已完成
- [x] 阶段A: 读取 diff 和关键实现,找出阻断项
- [x] 阶段B: 运行提交前验证
- [x] 阶段C: local commit 并复核状态

### 证据
- commit: `f0b0dfc Enable mouse control over the existing desktop coordinate contract`
- `cargo fmt -- --check`: 通过
- focused tests: 8 passed
- `cargo test --package rustdog --bin rdog`: 153 passed
- `cargo test --tests --no-run`: 通过
- `git diff --check`: 通过

### 状态
**当前 review + local commit 任务完成** - 未 push,工作树只剩未跟踪支线记录文件。

## [2026-05-14 11:55:51] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: Stop hook 要求继续 Ralph 收口

### 当前目标
- 根据 Ralph hook 提示,继续完成 fresh verification evidence 和 Ralph state cleanup。
- 补齐原计划中尚未完成的全局 `rdog-control` skill 同步。
- 做本地 architect/deslop review,不扩大到新的业务实现。

### 阶段
- [ ] 阶段D: 同步全局 skill 和 references
- [ ] 阶段E: fresh verification + skill validation
- [ ] 阶段F: architect/deslop 审核
- [ ] 阶段G: Ralph state complete

### 状态
**目前在阶段D** - 准备修改 `/Users/cuiluming/.codex/skills/rdog-control`。

## [2026-05-14 12:01:41] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: fresh verification 与本地审核完成

### 已完成
- [x] 阶段D: 同步全局 skill 和 references
- [x] 阶段E: fresh verification + skill validation
- [x] 阶段F: architect/deslop 审核

### 证据
- skill validation: `Skill is valid!`
- `cargo fmt -- --check`: 通过
- `git diff --check`: 通过
- focused tests: 8 passed
- `cargo test --package rustdog --bin rdog`: 153 passed
- `cargo test --tests --no-run`: 通过

### 状态
**目前在阶段G** - 准备写 Ralph complete state 并复核。

## [2026-05-14 12:06:47] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: completion audit 已补齐

### 已完成
- [x] prompt-to-artifact checklist 已写入 `notes__mouse_ralph.md`
- [x] fresh verification evidence 已重新收集
- [x] 原始 Ralph plan / 用户 local commit 要求 / hook 要求均已映射到产物和证据

### 证据
- focused tests: 8 passed
- bin tests: 153 passed
- integration compile: 通过
- fmt check: 通过
- diff check: 通过
- skill validation: `Skill is valid!`

### 状态
**completion audit 通过** - 准备写回 Ralph state: `completion_audit_gate=passed`。

## [2026-05-14 12:12:40] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 继续排查 Stop hook completion audit gate

### 当前目标
- 用户要求 review diff 后做 local commit,本地 commit 已存在,但 Stop hook 仍标记 `missing_completion_audit`。
- 本轮先查 hook 真实判定逻辑,再按实际字段/产物形态补齐,避免盲写 state。
- 不修改产品代码,不 push。

### 阶段
- [ ] 阶段H: 只读检查 hook 代码和 Ralph state 判定条件
- [ ] 阶段I: 按 hook 期望补齐 audit state 或 artifact
- [ ] 阶段J: 复跑最小 fresh verification 并清理 Ralph state
- [ ] 阶段K: 最终复核 git 状态和提交信息

### 状态
**目前在阶段H** - 准备检查 oh-my-codex Stop hook 的 completion audit 条件。

## [2026-05-14 12:18:30] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 清理旧 ralplan runtime state

### 当前目标
- `ralph` completion audit 已按 hook 判定通过并显示 inactive complete。
- `omx state list-active` 仍显示旧 `ralplan` planning state,这是执行前遗留状态。
- 清理该 stale runtime state,避免最终 Stop hook 把已完成任务重新阻塞。

### 状态
**目前在阶段J** - 准备执行 `omx state clear` 清理旧 ralplan state,然后复核 active modes。

## [2026-05-14 12:19:12] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: Ralph 和 local commit 收口完成

### 已完成
- [x] 阶段H: 只读检查 hook 代码和 Ralph state 判定条件
- [x] 阶段I: 按 hook 期望补齐 audit state 或 artifact
- [x] 阶段J: 复跑最小 fresh verification 并清理 Ralph state
- [x] 阶段K: 最终复核 git 状态和提交信息

### 证据
- `evaluateRalphCompletionAuditEvidence`: `completion_audit_passed`
- `omx state read --input '{"mode":"ralph"}' --json`: `active:false`, `current_phase:"complete"`
- `omx state list-active --json`: `active_modes:[]`
- 最新 local commit: `f0b0dfc Enable mouse control over the existing desktop coordinate contract`

### 状态
**当前任务完成** - 已 local commit,未 push,剩余未跟踪项仅为支线记录文件。
