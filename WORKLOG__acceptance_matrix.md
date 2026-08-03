## [2026-07-29 10:50:00] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义首版验收矩阵、fixtures 与性能预算 (ticket #7)

### 任务内容

- 落盘 Wayfinder ticket `#7` 的 resolution asset。
- 整合 ticket `#7` question 列出的 14 项验收边界为 7 大类测试维度。
- 引用已有 closed ticket (#3 / #4 / #5 / #6 / #8 / #9 / #11) 作为权威证据,不重写语义规则。
- 提交到合适分支,不污染用户 dirty worktree。

### 完成过程

- 7 项 HITL 决策:
  1. 7 大类测试维度(protocol_parser / compiler_golden / security_redaction / gui_determinism / multi_display_and_remote / lifecycle_and_crash_recovery / e2e_record_compile_replay)
  2. fixtures 目录沿用现有结构,新增 `tests/fixtures/acceptance/<class>/<test>`,文件平铺
  3. 性能预算具体数值(录制 15% avg / 40% P95 CPU、1 GiB RAM、10 MB/min、500/2000 事件;compile 1h ≤10s;replay ≥0.5x;daemon ≤3s)
  4. crash recovery 3 SIGKILL 场景 + fixture Mark 触发(测试-only)
  5. E2E 5 分钟真实 record + 5 次随机操作 + byte-equal AX tree snapshot
  6. soak 2 小时连续(静态 1h + 高频 Key 30min + 中频 mouse 30min)
  7. acceptance 9 条硬条件 + report 格式(Summary + Detail + Attestation)
- 写规格 `specs/rdog-acceptance-matrix.md`(439 行),涵盖 scope / 7 大类 / fixtures 布局 / 性能预算 / crash 场景 / E2E / soak / 硬条件 / report / CI 集成 / cross references。
- AGENTS.md 追加新规格长期文件索引。
- git commit cff76c5 在 auto-optimize 分支(沿用 ticket #4 模式,不 push 不污染 main)。
- 关闭 ticket #7 + 更新 Wayfinder map #2。

### 总结感悟

- Wayfinder map 的所有 9 个子 ticket (#3-#11, #12) 现在全部关闭。这是从 2026-07-20 开始录制回放 Wayfinder 工作的完整闭环。
- ticket `#7` 作为最后关闭的规格 ticket,引用了所有前面 ticket 的 evidence,不重写规则,只固化验收 policy。这种"refer-not-rewrite"风格让 acceptance matrix 与已有规格保持 single source of truth。
- 7 大类比 14 项更易实施:14 项里 journal crash recovery / 长时间录制 / 事件速率 都属于 lifecycle_and_crash_recovery 大类;secure input redaction / AX / Web / no-AX / 窗口 move/resize / preflight fail-closed 都属于 gui_determinism / security_redaction / multi_display 大类。归类是 acceptance matrix 的核心价值。
- 性能预算写明"不是 SLA",避免 acceptance 变成 blocking gate。这与 AGENTS.md 的 "不要过度设计" 一致 — 验收门槛应当可衡量但不可僵化。
- 报告不进 Git commit、保留 ≥10 份的设计,确保 audit trail 独立于代码版本,适合发布审计场景。
