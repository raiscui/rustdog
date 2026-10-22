# ARCHIVE MANIFEST — 默认主线历史续档归档 (2026-10-22)

## 归档范围

| 原路径 | 新路径 | 行数 |
| --- | --- | --- |
| `task_plan_2026-09-02.md` | `archive/default_history/task_plan_2026-09-02_ocr_content_layer_rollover.md` | 1696 |
| `WORKLOG_2026-09-02.md` | `archive/default_history/WORKLOG_2026-09-02_ocr_content_layer_rollover.md` | 1125 |

## 为什么归档

这两份是**默认主线**在 2026-09-02 续档时留下的历史版本，不是任何支线的上下文。
续档后的当前文件 (`task_plan.md` / `WORKLOG.md`) 已在 2026-09-02 18:05 之后连续
追加了 6 轮状态更新 (含 PR #104 提交与合并、#102 根因修复、#103 关票、cookbook
交付、Windows 真机验证)，当前文件才是活跃账本。

按 `AGENTS.md` 索引与 `docs/solutions/` 的家风：**根目录只保留默认六文件入口**，
历史版本进 `archive/default_history/`，支线上下文进 `archive/branch_contexts/`。

## 归档前是否做过阅读与总结

是。本轮 continuous-learning 已回读两份文件的全部章节标题与首尾内容，并核对了
其未勾选项的归属。

## 未勾选项的转出核对

| 未勾选项族 | 归属 | 是否需补落盘 |
| --- | --- | --- |
| `#54` / `#55` / `#51` AX successor / changes 接入 | 远端 issue #51 / #55 仍 OPEN；本地知识由 `docs/solutions/logic-errors/gui-resource-epoch-read-write-race.md` 与 `docs/solutions/architecture-patterns/ax-observation-cached-progressive-queries.md` 承接 | 否 (已有 issue URL + solution) |
| `ax_action` 模块拆分 2.1–2.8 阶段清单 | 已于 2026-08 完成，落地记录在续档后的 `notes.md` 与 `AGENTS.md` 的 ax-split 索引条目 | 否 |
| `#102` / `#103` OCR overlay 与陈旧帧 | 已关票 (2026-09-02)，结论在 `notes.md` 同日条目 + `docs/solutions/best-practices/macos-live-gui-testing-environment-gates.md` | 否 |
| OCR 引擎横评方法论 | 明确留作 inbox，见 `EXPERIENCE.md` 2026-09-02 条目 (附升级条件) | 否 (有意留 inbox) |
| upstream Pi 迁移 canary blocker | 已在 `EXPERIENCE.md` 2026-08-22 / 08-28 积压核验中处置 (转为索引 + 明确外部 blocker) | 否 |

**转出缺口: 无。** 未发现"计划里声明要落 `LATER_PLANS.md` 或 `docs/solutions/`、
但实际并未落盘"的条目。

## 提取出的候选最终落到哪个载体

- 本次归档文件本身**不产生新的成熟经验** (OCR 段的结论已在 2026-09-02 那一轮
  continuous-learning 里 Capture 成 `macos-live-gui-testing-environment-gates.md`
  与 `appkit-helper-subprocess-isolation.md`)。
- 本轮真正的新沉淀来自 **2026-10-07 ~ 10-11** 的新增工作 (此前不在六文件里),
  落点见 `WORKLOG.md` 2026-10-22 条目与
  `docs/solutions/best-practices/electron-main-process-forensics.md` (新建)、
  `docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md`
  (Scoped Refresh)。

## 旧路径引用同步

- `task_plan.md` 头部 "前档" 指向已改为新归档路径。
- `WORKLOG.md` 头部 "前档" 指向已改为新归档路径。
- 全仓其余位置 (含 `archive/` 内部、`docs/`、`specs/`、`README.md`) 无其它引用。

## 未迁移候选及其证据缺口

- OCR 引擎横评方法论 (Apple Vision / Paddle / OCRS / OAR 的行级 vs 文本层召回口径):
  仍是 inbox, 证据缺口 = 评测脚本未入库、单日单机验证。升级条件写在
  `EXPERIENCE.md` 同名条目。
