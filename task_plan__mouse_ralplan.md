# 任务计划: mouse control Option A ralplan 共识审查

## [2026-05-14 10:34:15] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 支线计划启动

### 目标
- 对 `.omx/plans/rdog-mouse-control-implementation-plan.md` 按 Option A 做 `$ralplan` 共识审查。
- 只修订计划和记录,不实现代码。
- 保持 screenshot manifest `os-logical` 为鼠标控制坐标单一真相源。

### 阶段
- [x] 阶段1: 建立支线 ralplan 上下文
- [ ] 阶段2: Planner 复核并修订 Option A 计划
- [ ] 阶段3: Architect 顺序审查
- [ ] 阶段4: Critic 顺序验收
- [ ] 阶段5: 验证和记录最终计划

### 关键约束
- 不进入实现。
- Critic 之前必须等待 Architect 完成。
- 如果计划需要修订,只改 `.omx/plans/rdog-mouse-control-implementation-plan.md` 和支线记录。
- 主线默认六文件只保留索引,避免超过 1000 行。

### 状态
**目前在阶段2** - 准备读取计划、源规格和代码证据后进行 Planner 复核。

## [2026-05-14 10:38:06] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 状态更新: 子智能体失败后改为本地顺序共识

### 现象
- Planner 子智能体启动后返回 `insufficient_user_quota`,没有产出可用审查结论。

### 处理
- 不再继续启动子智能体消耗额度。
- 在主会话中按 `$ralplan` 的顺序完成 Planner -> Architect -> Critic 复核。
- 仍保持“不实现代码,只修订计划”的边界。

### 状态
**目前在阶段2** - 本地执行 Planner 复核。

## [2026-05-14 10:45:36] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] 完成记录: Option A 共识审查通过

### 阶段
- [x] 阶段1: 建立支线 ralplan 上下文
- [x] 阶段2: Planner 复核并修订 Option A 计划
- [x] 阶段3: Architect 顺序审查
- [x] 阶段4: Critic 顺序验收
- [x] 阶段5: 验证和记录最终计划

### 已完成
- 已生成 `.omx/context/rdog-mouse-control-option-a-20260514T023415Z.md` 上下文快照。
- 已按 Planner 复核结果修订计划: structured response、backend capability guard、drag/wheel 验收标准。
- 已按 Architect 审查结果修订计划: 增加内部 `MouseBackend` facade 和 fake backend failure tests。
- 已按 Critic 结论修订计划: 固定 `ActionExecutionResult.response_value_json` 路径,移除 structured response 的可选口径。

### 验证
- `git diff --check`: 通过。
- 搜索旧冲突口径: 未发现 `可选设计`、`如果 structured success metadata 成为硬需求`、`第一版可用 @response 0` 等旧方案残留。
- 剩余 `@response 0` 只出现在“不能只返回 `@response 0`”的共识说明中。

### 状态
**本轮 `$ralplan` 已完成** - Critic verdict 为 APPROVE,没有进入代码实现。
