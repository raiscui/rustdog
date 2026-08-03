# 任务计划: 定义首版验收矩阵、fixtures 与性能预算

## 目标

固化 macOS 首版实施验收的动态证据门槛:测试矩阵、fixtures 集、性能预算和真实 record→compile→replay E2E,使实施团队不需要再决定 "该跑哪些测试才算合格"。

## 阶段

- [x] 阶段 1: 任务计划与设置
- [x] 阶段 2: 静态证据收集
- [ ] 阶段 3: HITL 决策(一次只问一个问题)
- [ ] 阶段 4: 写规格、commit、push、关闭 ticket、更新 map

## 关键问题

1. 验收矩阵覆盖哪些测试维度?
2. fixtures 集如何组织(per-test 还是 shared)?
3. 性能预算具体数值?
4. crash recovery 如何验证?
5. E2E record→compile→replay 如何跑?
6. 长录制测试多长、什么场景?

## 做出的决定

(待 HITL 决策)

## 遇到错误

- 无

## 状态

**当前在阶段 2.5**:收集已有测试边界与 prototype #8 fixtures,准备第一项 HITL 决策。

## [2026-07-29 10:10:00] [Session ID: omx-1784512435044-92wxat] [调查更新]: 已有测试边界已核对

- tests/ 已有 17 个 integration test (control_lanes / control_mode / control_pty / control_window_e2e / zenoh_unixpipe_fast_path 等)。
- specs/rdog-flow-control-plan.md 已固化 Focused tests 列表(`cargo test control_flow::tests` 等)。
- ticket #8 prototype 已建立 `tests/fixtures/replay-compiler/journal_optimizations.jsonl` + `flow_optimizations.json` golden fixture。
- ticket #4 / #8 / #9 / #3 / #5 / #6 / #11 都已固化测试覆盖目标,只需在 acceptance matrix 里引用。
- ticket #7 不需要新加 fixture 覆盖已 closed ticket 已有的场景,只补未覆盖的 gap(crash recovery / soak / 性能 / E2E)。

## [2026-07-29 10:14:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: 7 大类测试维度

- [x] Human确认protocol_parser / compiler_golden / security_redaction / gui_determinism / multi_display_and_remote / lifecycle_and_crash_recovery / e2e_record_compile_replay。
- [x] 每个大类对应一个spec section,引用已有closed ticket作为权威证据。
- [x] E2E默认macOS,Linux/Windows可选,不阻塞首版acceptance。
- [x] 沿用ticket #4 / #9 reason codes,不新增failure mode分类。
- [ ] 下一决策:确认fixtures集组织方式。

## [2026-07-29 10:18:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: fixtures 目录组织

- [x] Human确认沿用tests/fixtures/replay-compiler/作为compiler_golden类fixture。
- [x] Human确认新建tests/fixtures/acceptance/作为其他6大类fixture。
- [x] Human确认fixture文件平铺,不嵌套子目录。
- [x] protocol_parser类不需要新fixture,复用现有src/control_protocol/tests/。
- [x] e2e fixture由录制脚本生成并固化。
- [ ] 下一决策:确认性能预算具体数值。

## [2026-07-29 10:22:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: 性能预算数值

- [x] Human确认录制CPU 15%/40% P95、内存1GiB、磁盘10MB/min、事件500/2000 sustained/burst。
- [x] Human确认编译1小时Journal ≤10秒、Bundle commit ≤5秒。
- [x] Human确认replay实时倍率 ≥0.5x、CPU 25%、内存500MiB。
- [x] Human确认daemon冷启动 ≤3秒。
- [x] Human确认远程delivery 100MB ≤RTT×1.5+bandwidth。
- [x] 数值是macOS首版目标,不是SLA。
- [x] 预算修订需新ticket。
- [ ] 下一决策:确认crash recovery测试覆盖矩阵。

## [2026-07-29 10:26:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: crash recovery 测试覆盖

- [x] Human确认3个crash场景:recording / finalizing / completed。
- [x] Human确认每个场景用SIGKILL真实触发,不mock daemon内部状态。
- [x] Human确认crash时间点通过fixture Mark触发(测试-only)。
- [x] Human确认crash recovery用 `--crash-recovery` flag,默认true。
- [x] 测试断言只读lifecycle metadata + journal + bundle sha256。
- [x] 不引入新crash detection API或reason code。
- [ ] 下一决策:确认E2E record→compile→replay验收条件。

## [2026-07-29 10:30:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: E2E record→compile→replay 验收条件

- [x] Human确认5分钟真实GUI操作。
- [x] Human确认replay在fresh daemon session。
- [x] Human确认byte-equal AX tree snapshot比较。
- [x] Human确认5次E2E随机操作(click/type/scroll/window)。
- [x] Human确认CI macOS required check。
- [x] Human确认bundle integrity校验(per-file + whole-archive)。
- [x] 不引入新E2E framework。
- [ ] 下一决策:确认长录制soak测试场景。

## [2026-07-29 10:34:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: 长录制soak测试场景

- [x] Human确认3场景合计≤2小时:静态1h + 高频Key 30min + 中频mouse+scroll 30min。
- [x] Human确认每个场景独立录制独立Bundle。
- [x] Human确认soak不阻塞CI PR check,但阻塞release tag。
- [x] Human确认soak fixture在tests/fixtures/acceptance/lifecycle_and_crash_recovery/soak/生成,不预生成。
- [x] 不引入新内存profiling hook,沿用top/ps/fs_usage。
- [ ] 下一决策:确认acceptance pass/fail硬条件与report格式。

## [2026-07-29 10:38:00] [Session ID: omx-1784512435044-92wxat] [决策确认]: acceptance pass/fail 硬条件 + report 格式

- [x] Human确认9条硬条件全部满足才算PASS。
- [x] Human确认Report格式:Summary + Detail + Attestation。
- [x] Human确认Report路径tests/reports/acceptance_<date>.md,旧报告保留≥10份。
- [x] Human确认Report不进Git commit,只作CI artifact。
- [x] Human确认不引入新acceptance framework。
- [x] ticket #7所有HITL决策已具备写规格条件,进入写规格阶段。

## [2026-07-29 10:50:00] [Session ID: omx-1784512435044-92wxat] [任务完成]: ticket #7 resolution delivered

- [x] 写完整规格 `specs/rdog-acceptance-matrix.md` (439行)。
- [x] AGENTS.md 追加长期文件索引。
- [x] git commit cff76c5 在 auto-optimize/20260728-2316-rdog-control 分支,scope 限定 specs + AGENTS.md。
- [x] 不 push(由用户决定何时 merge 到 main)。
- [x] gh issue close 7 with full resolution comment。
- [x] Wayfinder map ticket #2 body 追加 ticket #7 entry,放在第一个位置(最新优先)。
- [x] 范围内dirty worktree未污染(用户的 modified 文件不动)。
- [x] Wayfinder map 所有 9 个子ticket全部CLOSED,#2 map 自身仍 OPEN。
