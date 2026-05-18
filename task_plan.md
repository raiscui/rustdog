# 任务计划: 默认主线上下文续档后入口

## [2026-05-14 15:20:00] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [续档]: 默认 task_plan 超过 1000 行

### 续档原因
- 默认 `task_plan.md` 在 AX plan 索引写入后达到 1003 行,超过仓库六文件 1000 行上限.
- 旧文件已复制到 `archive/default_history/2026-05-14_ax_plan_context_rollover/task_plan_2026-05-14_before_ax_plan_rollover.md`.

### 当前活跃支线
- `__ax_plan`: 正在生成 `@screenshot include_ax` 与 `@ax-*` 控制能力计划.
- `__mouse_e2e`: 仍有未提交的真实 GUI E2E 修改现场,本轮 AX plan 不继续处理该支线.

### 当前状态
**默认主线已续档** - 后续默认任务从本文件继续记录;AX plan 状态继续写入 `task_plan__ax_plan.md`.

## [2026-05-17 00:21:14] [Session ID: codex-20260517-non-mouse-control-research] [索引]: 启用非鼠标控制调研支线

### 启用原因
- 用户要求调研 `https://github.com/iFurySt/open-codex-computer-use`。
- 目标是寻找“完整能力的非鼠标类控制”,避免 live 鼠标测试干扰人类当前操作。

### 支线文件
- `task_plan__non_mouse_control_research.md`: 调研计划和状态。
- `notes__non_mouse_control_research.md`: 仓库与方案调研笔记。
- `WORKLOG__non_mouse_control_research.md`: 本轮调研交付记录。

### 当前状态
**非鼠标控制调研支线已启用** - 本轮不运行任何真实鼠标点击、拖拽或滚轮测试。

## [2026-05-17 10:27:25] [Session ID: 019e1b72-d659-7a60-91b4-66cea3fc6ce0] [索引]: 启用非鼠标语义控制实现支线

### 启用原因
- 用户执行 `$ralph .omx/plans/rdog-non-mouse-semantic-control-improvement-plan.md`。
- 目标是把调研方案推进到代码实现,优先完成非鼠标语义控制协议,避免干扰用户正在操作的电脑。

### 支线文件
- `task_plan__non_mouse_semantic_control.md`: Ralph 执行计划和状态。
- `notes__non_mouse_semantic_control.md`: 实现调研和关键决策。
- `WORKLOG__non_mouse_semantic_control.md`: 本轮交付记录。

### 当前状态
**非鼠标语义控制实现支线已启用** - 本轮禁止运行 live 鼠标移动、点击、拖拽或滚轮测试。
## [2026-05-18 10:36:34] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: 迁移 rdog-control skill 到仓库内

### 行动目的
- 把全局 `/Users/cuiluming/.codex/skills/rdog-control` 复制进 rustdog 仓库,作为项目长期资产维护。

### 为什么现在做
- 仓库已经把 `rdog control` 作为核心入口,skill 应该跟着仓库一起版本化,而不是只留在用户级目录。
- 这样后续改协议、改 README、改 skill references 时,可以在同一个仓库里对齐。

### 将要做什么
- 先确认仓库里 skill 的落点和索引方式。
- 再把 `rdog-control` skill 复制到项目目录中,清理本机编辑器噪音。
- 最后更新项目索引,让它成为长期维护入口。

### 当前阶段
**目前在阶段1** - 先确认项目内 skill 目录和长期索引约定。

## [2026-05-18 10:46:10] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [状态更新]: 项目内 skill 复制和索引已完成

### 已完成
- [x] 确认全局 skill 来源: `/Users/cuiluming/.codex/skills/rdog-control`
- [x] 复制到项目内: `.codex/skills/rdog-control`
- [x] 排除本机编辑器噪音: 未复制 `.vscode`
- [x] 更新 `AGENTS.md`,将维护入口改为项目内相对路径
- [x] 移除旧的用户级绝对路径索引,避免双入口漂移

### 当前验证
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 已通过
- `git diff --check`: 已通过
- `diff -ru --exclude='.vscode' /Users/cuiluming/.codex/skills/rdog-control .codex/skills/rdog-control`: 无差异

### 当前状态
**目前在阶段4** - 收尾记录 WORKLOG,并最终复查工作区 diff。

## [2026-05-18 10:50:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: rdog-control skill 已迁入项目目录

### 已完成结果
- `rdog-control` skill 已复制到 `.codex/skills/rdog-control`
- `AGENTS.md` 已改为索引项目内相对路径
- 旧的用户级绝对路径索引已清除
- `notes.md`、`WORKLOG.md` 已补齐本轮记录

### 最终验证
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过
- `diff -ru --exclude='.vscode' /Users/cuiluming/.codex/skills/rdog-control .codex/skills/rdog-control`: 无差异
- `git diff --check`: 通过

### 当前状态
**本轮任务已完成** - skill 已成为仓库长期维护入口。

## [2026-05-18 10:55:40] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [计划]: 将全局 rdog-control 改成项目内连接目录

### 行动目的
- 让 `/Users/cuiluming/.codex/skills/rdog-control` 指向项目内 `.codex/skills/rdog-control`。
- 后续只维护项目内 skill,全局入口通过连接目录同步使用同一份内容。

### 已确认现状
- 全局路径目前是普通目录,不是连接目录。
- 项目内 `.codex/skills/rdog-control` 已存在。
- 两边实质内容在排除 `.vscode` 后无差异。

### 将要做什么
- 先备份/移走当前全局普通目录。
- 创建 `/Users/cuiluming/.codex/skills/rdog-control -> /Users/cuiluming/local_doc/l_dev/my/rust/rustdog/.codex/skills/rdog-control` 的符号链接。
- 通过 `ls -ld`、`readlink` 和 `quick_validate.py` 验证连接目录可用。

### 当前状态
**目前在阶段2** - 准备替换全局目录为连接目录。

## [2026-05-18 10:57:20] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] [完成]: 全局 rdog-control 已改为连接目录

### 已完成结果
- `/Users/cuiluming/.codex/skills/rdog-control` 已替换为符号链接。
- 链接目标指向项目内 `.codex/skills/rdog-control`。
- 全局路径和项目路径现在共享同一份 skill 内容。

### 验证结果
- `ls -ld /Users/cuiluming/.codex/skills/rdog-control`: 显示为 `lrwxr-xr-x`。
- `readlink /Users/cuiluming/.codex/skills/rdog-control`: 指向仓库内 skill 目录。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py /Users/cuiluming/.codex/skills/rdog-control`: 通过。
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/rdog-control`: 通过。
- `git diff --check`: 通过。

### 备份位置
- 旧的全局普通目录已移到 `/tmp/rdog-control-global-backup-20260518-104751`。

### 当前状态
**本轮任务已完成** - 全局入口已切成项目内连接目录。
