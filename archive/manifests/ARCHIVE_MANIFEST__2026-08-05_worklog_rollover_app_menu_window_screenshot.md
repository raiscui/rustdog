# WORKLOG 续档归档说明

## [2026-08-05 23:38:00] [Session ID: omx-1785926019233-oohizd] 归档范围

- 触发原因: 默认 `WORKLOG.md` 在本轮交付记录后达到 1001 行,超过 1000 行阈值。
- 归档文件: `WORKLOG.md` -> `archive/default_history/WORKLOG_2026-08-05_233200_app_menu_window_screenshot.md`。
- 新入口: 根目录重新创建精简 `WORKLOG.md`,保留归档路径和本轮学习结论。

## 持续学习摘要

- 默认六文件已回读: task plan 记录本轮三项 GUI control 能力和验证;notes 保存后端 timeout 与 live smoke 事实;LATER 已移除完成候选;ERRORFIX 没有本任务适用的错误修复;EPIPHANY 没有新增重大风险。
- 根目录没有同名的默认历史版本。本次盘点到的带后缀上下文均为既有独立支线,本轮未使用且未移动。
- 长期经验已追加到 `EXPERIENCE.md`: app-menu 必须用唯一 PID 缩小 capture 范围,不能事后过滤;native screenshot backend 必须 bounded 且限制 in-flight worker。
- `AGENTS.md` 已更新 `EXPERIENCE.md` 的用途和阅读时机索引。
- 三项协议和执行边界已经同步在 `.codex/skills/rdog-control/SKILL.md`、`specs/control-line-protocol.md`、`specs/rdog-ax-screenshot-manifest-control-plan.md`、`specs/rdog-non-mouse-semantic-control-plan.md` 与 `specs/zenoh-screenshot-control-plan.md`,不再新增重复文档或 skill。

## 归档验证

- 旧 Worklog 在归档前的行数: 1001。
- 新 Worklog 保持当前入口;其他默认六文件未归档。
- 后续应在涉及 app-menu、screenshot backend 或 GUI 控制协议前阅读 `EXPERIENCE.md` 和相应规格。
