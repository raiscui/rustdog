# 归档说明: 2026-08-08 macOS ops interaction ledger 前的旧支线整理

## 背景

- 当前默认六文件仍在记录 macOS ops current-binary 三轮 ledger 分析,属于活跃主线,不归档。
- 根目录其余带 `__suffix` 的上下文文件最后记录集中在 2026-07-18 至 2026-07-31,本轮没有发现活跃任务证据。
- 按 `continuous-learning` 规则,旧支线按主题整组移动到 `archive/branch_contexts/<topic>/`,保留原文件名和正文。

## 判定

- 本批归档 24 个主题,共 81 个文件。
- 归档动作只移动上下文文件,没有修改 parser、协议、primitive、runner、case 或测试。
- 后续追溯旧支线时,先阅读本 manifest,再进入对应主题目录。

## 归档映射

### `acceptance_matrix`

- `WORKLOG__acceptance_matrix.md` -> `archive/branch_contexts/acceptance_matrix/WORKLOG__acceptance_matrix.md`
- `task_plan__acceptance_matrix.md` -> `archive/branch_contexts/acceptance_matrix/task_plan__acceptance_matrix.md`

### `architecture_review`

- `WORKLOG__architecture_review.md` -> `archive/branch_contexts/architecture_review/WORKLOG__architecture_review.md`
- `notes__architecture_review.md` -> `archive/branch_contexts/architecture_review/notes__architecture_review.md`
- `task_plan__architecture_review.md` -> `archive/branch_contexts/architecture_review/task_plan__architecture_review.md`

### `canonical_skill_8b`

- `notes__canonical_skill_8b.md` -> `archive/branch_contexts/canonical_skill_8b/notes__canonical_skill_8b.md`
- `task_plan__canonical_skill_8b.md` -> `archive/branch_contexts/canonical_skill_8b/task_plan__canonical_skill_8b.md`

### `complexity_audit`

- `EPIPHANY_LOG__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/EPIPHANY_LOG__complexity_audit.md`
- `ERRORFIX__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/ERRORFIX__complexity_audit.md`
- `LATER_PLANS__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/LATER_PLANS__complexity_audit.md`
- `WORKLOG__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/WORKLOG__complexity_audit.md`
- `notes__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/notes__complexity_audit.md`
- `task_plan__complexity_audit.md` -> `archive/branch_contexts/complexity_audit/task_plan__complexity_audit.md`

### `control_ax_deepen`

- `WORKLOG__control_ax_deepen.md` -> `archive/branch_contexts/control_ax_deepen/WORKLOG__control_ax_deepen.md`
- `task_plan__control_ax_deepen.md` -> `archive/branch_contexts/control_ax_deepen/task_plan__control_ax_deepen.md`

### `darwin_calculator`

- `ERRORFIX__darwin_calculator.md` -> `archive/branch_contexts/darwin_calculator/ERRORFIX__darwin_calculator.md`
- `WORKLOG__darwin_calculator.md` -> `archive/branch_contexts/darwin_calculator/WORKLOG__darwin_calculator.md`
- `notes__darwin_calculator.md` -> `archive/branch_contexts/darwin_calculator/notes__darwin_calculator.md`
- `task_plan__darwin_calculator.md` -> `archive/branch_contexts/darwin_calculator/task_plan__darwin_calculator.md`

### `local_default_atomic_lease`

- `EPIPHANY_LOG__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/EPIPHANY_LOG__local_default_atomic_lease.md`
- `ERRORFIX__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/ERRORFIX__local_default_atomic_lease.md`
- `LATER_PLANS__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/LATER_PLANS__local_default_atomic_lease.md`
- `WORKLOG__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/WORKLOG__local_default_atomic_lease.md`
- `notes__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/notes__local_default_atomic_lease.md`
- `task_plan__local_default_atomic_lease.md` -> `archive/branch_contexts/local_default_atomic_lease/task_plan__local_default_atomic_lease.md`

### `local_default_legacy_retirement`

- `EPIPHANY_LOG__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/EPIPHANY_LOG__local_default_legacy_retirement.md`
- `ERRORFIX__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/ERRORFIX__local_default_legacy_retirement.md`
- `LATER_PLANS__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/LATER_PLANS__local_default_legacy_retirement.md`
- `WORKLOG__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/WORKLOG__local_default_legacy_retirement.md`
- `notes__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/notes__local_default_legacy_retirement.md`
- `task_plan__local_default_legacy_retirement.md` -> `archive/branch_contexts/local_default_legacy_retirement/task_plan__local_default_legacy_retirement.md`

### `local_default_registry_recovery`

- `EPIPHANY_LOG__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/EPIPHANY_LOG__local_default_registry_recovery.md`
- `ERRORFIX__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/ERRORFIX__local_default_registry_recovery.md`
- `LATER_PLANS__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/LATER_PLANS__local_default_registry_recovery.md`
- `WORKLOG__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/WORKLOG__local_default_registry_recovery.md`
- `notes__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/notes__local_default_registry_recovery.md`
- `task_plan__local_default_registry_recovery.md` -> `archive/branch_contexts/local_default_registry_recovery/task_plan__local_default_registry_recovery.md`

### `postcondition_other_verbs`

- `WORKLOG__postcondition_other_verbs.md` -> `archive/branch_contexts/postcondition_other_verbs/WORKLOG__postcondition_other_verbs.md`

### `postcondition_split`

- `WORKLOG__postcondition_split.md` -> `archive/branch_contexts/postcondition_split/WORKLOG__postcondition_split.md`
- `task_plan__postcondition_split.md` -> `archive/branch_contexts/postcondition_split/task_plan__postcondition_split.md`

### `recording_bundle_schema`

- `WORKLOG__recording_bundle_schema.md` -> `archive/branch_contexts/recording_bundle_schema/WORKLOG__recording_bundle_schema.md`
- `notes__recording_bundle_schema.md` -> `archive/branch_contexts/recording_bundle_schema/notes__recording_bundle_schema.md`
- `task_plan__recording_bundle_schema.md` -> `archive/branch_contexts/recording_bundle_schema/task_plan__recording_bundle_schema.md`

### `recording_journal`

- `ERRORFIX__recording_journal.md` -> `archive/branch_contexts/recording_journal/ERRORFIX__recording_journal.md`
- `WORKLOG__recording_journal.md` -> `archive/branch_contexts/recording_journal/WORKLOG__recording_journal.md`
- `notes__recording_journal.md` -> `archive/branch_contexts/recording_journal/notes__recording_journal.md`
- `task_plan__recording_journal.md` -> `archive/branch_contexts/recording_journal/task_plan__recording_journal.md`

### `recording_lifecycle`

- `ERRORFIX__recording_lifecycle.md` -> `archive/branch_contexts/recording_lifecycle/ERRORFIX__recording_lifecycle.md`
- `WORKLOG__recording_lifecycle.md` -> `archive/branch_contexts/recording_lifecycle/WORKLOG__recording_lifecycle.md`
- `notes__recording_lifecycle.md` -> `archive/branch_contexts/recording_lifecycle/notes__recording_lifecycle.md`
- `task_plan__recording_lifecycle.md` -> `archive/branch_contexts/recording_lifecycle/task_plan__recording_lifecycle.md`

### `recording_redaction`

- `ERRORFIX__recording_redaction.md` -> `archive/branch_contexts/recording_redaction/ERRORFIX__recording_redaction.md`
- `WORKLOG__recording_redaction.md` -> `archive/branch_contexts/recording_redaction/WORKLOG__recording_redaction.md`
- `notes__recording_redaction.md` -> `archive/branch_contexts/recording_redaction/notes__recording_redaction.md`
- `task_plan__recording_redaction.md` -> `archive/branch_contexts/recording_redaction/task_plan__recording_redaction.md`

### `recording_replay_compiler`

- `WORKLOG__recording_replay_compiler.md` -> `archive/branch_contexts/recording_replay_compiler/WORKLOG__recording_replay_compiler.md`
- `task_plan__recording_replay_compiler.md` -> `archive/branch_contexts/recording_replay_compiler/task_plan__recording_replay_compiler.md`

### `recording_window_geometry`

- `WORKLOG__recording_window_geometry.md` -> `archive/branch_contexts/recording_window_geometry/WORKLOG__recording_window_geometry.md`
- `notes__recording_window_geometry.md` -> `archive/branch_contexts/recording_window_geometry/notes__recording_window_geometry.md`
- `task_plan__recording_window_geometry.md` -> `archive/branch_contexts/recording_window_geometry/task_plan__recording_window_geometry.md`

### `replay_preflight_guard_verification`

- `WORKLOG__replay_preflight_guard_verification.md` -> `archive/branch_contexts/replay_preflight_guard_verification/WORKLOG__replay_preflight_guard_verification.md`
- `task_plan__replay_preflight_guard_verification.md` -> `archive/branch_contexts/replay_preflight_guard_verification/task_plan__replay_preflight_guard_verification.md`

### `semantic_promotion_prototype`

- `EPIPHANY_LOG__semantic_promotion_prototype.md` -> `archive/branch_contexts/semantic_promotion_prototype/EPIPHANY_LOG__semantic_promotion_prototype.md`
- `ERRORFIX__semantic_promotion_prototype.md` -> `archive/branch_contexts/semantic_promotion_prototype/ERRORFIX__semantic_promotion_prototype.md`
- `WORKLOG__semantic_promotion_prototype.md` -> `archive/branch_contexts/semantic_promotion_prototype/WORKLOG__semantic_promotion_prototype.md`
- `notes__semantic_promotion_prototype.md` -> `archive/branch_contexts/semantic_promotion_prototype/notes__semantic_promotion_prototype.md`
- `task_plan__semantic_promotion_prototype.md` -> `archive/branch_contexts/semantic_promotion_prototype/task_plan__semantic_promotion_prototype.md`

### `target_locator_seam`

- `WORKLOG__target_locator_seam.md` -> `archive/branch_contexts/target_locator_seam/WORKLOG__target_locator_seam.md`
- `task_plan__target_locator_seam.md` -> `archive/branch_contexts/target_locator_seam/task_plan__target_locator_seam.md`

### `verb_dispatcher`

- `WORKLOG__verb_dispatcher.md` -> `archive/branch_contexts/verb_dispatcher/WORKLOG__verb_dispatcher.md`
- `task_plan__verb_dispatcher.md` -> `archive/branch_contexts/verb_dispatcher/task_plan__verb_dispatcher.md`

### `wayfinder_overdesign_simplification`

- `WORKLOG__wayfinder_overdesign_simplification.md` -> `archive/branch_contexts/wayfinder_overdesign_simplification/WORKLOG__wayfinder_overdesign_simplification.md`
- `task_plan__wayfinder_overdesign_simplification.md` -> `archive/branch_contexts/wayfinder_overdesign_simplification/task_plan__wayfinder_overdesign_simplification.md`

### `xhs_web_area`

- `ERRORFIX__xhs_web_area.md` -> `archive/branch_contexts/xhs_web_area/ERRORFIX__xhs_web_area.md`
- `WORKLOG__xhs_web_area.md` -> `archive/branch_contexts/xhs_web_area/WORKLOG__xhs_web_area.md`
- `notes__xhs_web_area.md` -> `archive/branch_contexts/xhs_web_area/notes__xhs_web_area.md`
- `task_plan__xhs_web_area.md` -> `archive/branch_contexts/xhs_web_area/task_plan__xhs_web_area.md`

### `zenoh_runtime_split`

- `ERRORFIX__zenoh_runtime_split.md` -> `archive/branch_contexts/zenoh_runtime_split/ERRORFIX__zenoh_runtime_split.md`
- `WORKLOG__zenoh_runtime_split.md` -> `archive/branch_contexts/zenoh_runtime_split/WORKLOG__zenoh_runtime_split.md`
- `notes__zenoh_runtime_split.md` -> `archive/branch_contexts/zenoh_runtime_split/notes__zenoh_runtime_split.md`
- `task_plan__zenoh_runtime_split.md` -> `archive/branch_contexts/zenoh_runtime_split/task_plan__zenoh_runtime_split.md`

## 摘要

- `local_default_*`: local-default owner、legacy retirement、registry recovery 与原子 lease 经验。
- `recording_*` / `semantic_promotion_prototype` / `replay_preflight_guard_verification`: recorder、replay compiler、redaction、geometry 与 preflight 规格上下文。
- `postcondition_*` / `verb_dispatcher` / `control_ax_deepen` / `target_locator_seam`: control action、postcondition 和 AX dispatcher 演进记录。
- `complexity_audit` / `architecture_review` / `wayfinder_overdesign_simplification`: 架构审计、简化与交付复盘。
- `darwin_calculator` / `xhs_web_area` / `canonical_skill_8b`: macOS GUI 与模型 skill 验证记录。
- `zenoh_runtime_split`: Zenoh runtime 拆分和测试隔离记录。

## 后续

- 新任务继续以根目录默认六文件为入口。
- 需要追溯旧支线时,先阅读本 manifest,再按主题进入 `archive/branch_contexts/<topic>/`。
