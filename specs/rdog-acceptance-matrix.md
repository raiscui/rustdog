# rdog 首版验收矩阵、fixtures 与性能预算

## Status

本规格是 Wayfinder ticket [定义首版验收矩阵、fixtures 与性能预算](https://github.com/raiscui/rustdog/issues/7) 的 resolution asset。

它固化 macOS 首版实施验收的动态证据门槛:测试矩阵、fixtures 集、性能预算和真实 record→compile→replay E2E,使实施团队不需要再决定"该跑哪些测试才算合格"。

本规格是验收 policy,不是实施 ticket。

## Scope

本规格只定义:

- 7 大类验收测试维度
- fixtures 目录组织
- 性能预算数值
- crash recovery 3 场景
- E2E record→compile→replay 验收条件
- 长录制 soak 测试场景
- acceptance pass/fail 硬条件
- acceptance report 格式

以下内容由其他规格负责:

- Recording Bundle 物理形态: `specs/rdog-recording-bundle-schema.md`
- Replay preflight / guard / verification: `specs/rdog-replay-preflight-guard-verification.md`
- Recording Journal 模型: `specs/rdog-recording-journal-model.md`
- Recording Session lifecycle: `specs/rdog-recording-session-lifecycle.md`
- Semantic promotion 与 coordinate fallback: `specs/rdog-recording-semantic-promotion-policy.md`
- Window geometry precondition: `specs/rdog-recording-window-geometry-policy.md`
- Replay compiler prototype: `specs/rdog-recording-replay-compiler-prototype.md`
- Replay Script `rdog.flow.v1`: `specs/rdog-flow-control-plan.md`
- line-control 协议: `specs/control-line-protocol.md`

## Terms

- **acceptance**: 一组测试 + fixtures + 性能预算 + E2E 共同构成的"首版可交付"判定门槛。
- **fixture**: 测试用的固定输入或期望输出文件,固化在 `tests/fixtures/`。
- **golden**: 与 fixture 配对的期望输出,通常用于 byte-equal 比较。
- **soak**: 长时间连续运行,验证内存稳定性和无 panic / 无 crash。
- **E2E**: 真实 GUI 操作录制 + 真实 replay + byte-equal 校验。
- **budget**: 性能 / 资源的目标数值,达不到不阻塞 acceptance 但需记录差异。
- **report**: acceptance 跑完后 CI 生成的 markdown 文件,只作 audit trail。

## Invariants

1. acceptance matrix 引用已有 closed 规格,不重写语义规则。
2. 任何新增测试维度必须新建 ticket,不直接改本规格。
3. 性能预算是 macOS 首版目标,不是 SLA。达不到不阻塞 acceptance。
4. E2E 默认在 CI macOS runner 跑,Linux/Windows runner 标记为 skip。
5. acceptance report 不进入 Git commit,只作 CI artifact。
6. crash recovery 测试用 `SIGKILL` 真实触发,不 mock daemon 内部状态。
7. soak 测试不阻塞 CI PR check,只阻塞 release tag。
8. fixture 文件名 `<test_case>.jsonl` / `.json` / `.tar`,不嵌套子目录。
9. 测试断言只读 daemon lifecycle metadata / journal / bundle,不 mock 内部状态机。
10. 所有 reason code 命中 ticket `#4` / `#9` 命名空间,无未登记 code。

## Test classes

7 大类测试维度覆盖 ticket `#7` question 列出的 14 项验收边界。

### 1. protocol_parser

覆盖 line-control parser unit test + integration test,包括 `@flow` / `@savefile` / `@record-*` / 协议 frame 边界。

引用:

- `src/control_protocol/tests/`
- `tests/control_lanes.rs`
- `tests/control_mode.rs`
- `tests/control_pty.rs`
- `tests/connect_modes.rs`

不新增 fixture,直接复用现有 parser fixtures。

### 2. compiler_golden

复用 ticket `#8` prototype 的 6 个 integration test + golden fixture。

引用:

- `src/bin/replay_compiler.rs`
- `tests/recording_replay_compiler.rs`
- `tests/fixtures/replay-compiler/journal_optimizations.jsonl`
- `tests/fixtures/replay-compiler/flow_optimizations.json`

涵盖 11 项 compiler pass 的 deterministic 输出,包括 debounce / mouse_move_coalesce / scroll_coalesce / text_merge / shortcut_hotkey / sleep_mark / semantic_promotion / coordinate_fallback / window_precondition / redacted_parameter / source_provenance。

### 3. security_redaction

覆盖 ticket `#3` redaction 模型。Sensitive / unknown 输入全程不进入 journal、artifact、Bundle。`AxValue` 走 `typed_text` parameter。

Fixtures:

- `tests/fixtures/acceptance/security_redaction/journal_with_sensitive.jsonl`
- `tests/fixtures/acceptance/security_redaction/journal_with_unknown.jsonl`

Tests:

- sensitive / unknown 输入不进 artifact。
- `AxValue { redacted: true }` 携带 `parameter:"typed_text"`。
- redacted event 的 value 字段仍存在但仅作 placeholder,不进入 secret 流。

### 4. gui_determinism

覆盖 ticket `#4` preflight policy + ticket `#11` window geometry。`AX/Web/no-AX` / 窗口 move/resize / `@window-activate` focus verify / `@window-resize` rect verify / mouse guard display。

Fixtures:

- `tests/fixtures/acceptance/gui_determinism/window_geometry_two_displays.jsonl`
- `tests/fixtures/acceptance/gui_determinism/mouse_action_with_display_guard.json`

Tests:

- multi-display `display:"all"` screenshot manifest 校验。
- `@window-activate verify.focused` 三态报告(passed / timeout / unreadable)。
- `@window-resize` rect 验证 `ok_with_delta` 容忍度。
- mouse coordinate `os-logical` 与 display guard 强制。
- semantic promotion fail closed 在 stale / ambiguous locator 时。

### 5. multi_display_and_remote

覆盖 ticket `#9` Bundle 远程交付协议 + 多显示器 display scope。

Fixtures:

- `tests/fixtures/acceptance/multi_display_and_remote/bundle_remote_delivery.tar`
- `tests/fixtures/acceptance/multi_display_and_remote/bundle_corrupted_checksum.tar`

Tests:

- `@savefile` 单帧 base64 接收 + byte-equal。
- sha256 校验失败 → `delivery_failed:checksum_mismatch`。
- size 超 256 MiB → `delivery_failed:bundle_too_large`。
- owner-only delivery,其它 connection 收不到 Bundle 字节。
- rate limit 5 `@record-stop` / 秒 / connection。

### 6. lifecycle_and_crash_recovery

覆盖 ticket `#5` lifecycle + crash recovery。

Fixtures:

- `tests/fixtures/acceptance/lifecycle_and_crash_recovery/journal_orphan_crash.jsonl`
- `tests/fixtures/acceptance/lifecycle_and_crash_recovery/journal_mid_finalize.jsonl`
- `tests/fixtures/acceptance/lifecycle_and_crash_recovery/session_after_completed.jsonl`
- `tests/fixtures/acceptance/lifecycle_and_crash_recovery/soak/`(运行时生成)

Tests:

- 场景 1: `SIGKILL` daemon during `recording` → journal orphan 不恢复不导出。
- 场景 2: `SIGKILL` daemon during `finalizing` → staging 删除,Session `failed`。
- 场景 3: `SIGKILL` daemon after `completed` → completed retry 重放相同 `bundle_sha256`。

### 7. e2e_record_compile_replay

真实 record 5 分钟真实 GUI 操作 → compile → replay。

Fixtures:

- `tests/fixtures/acceptance/e2e/record_5min_session_manifest.json`(运行时生成)
- `tests/fixtures/acceptance/e2e/record_5min_session_bundle.tar`(运行时生成)

Tests:

- 真实 record 5 分钟真实 GUI 操作。
- stop 原子提交,验证 ticket `#9` 完整性(per-file + whole-archive SHA-256)。
- replay 在 fresh daemon session 跑。
- replay 完成后 AX tree snapshot 与 record 期最后一个 snapshot byte-equal。

## Fixture layout

```
tests/fixtures/
  replay-compiler/                # 已有 (ticket #8)
    journal_optimizations.jsonl
    flow_optimizations.json
  acceptance/
    security_redaction/
      journal_with_sensitive.jsonl
      journal_with_unknown.jsonl
    gui_determinism/
      window_geometry_two_displays.jsonl
      mouse_action_with_display_guard.json
    multi_display_and_remote/
      bundle_remote_delivery.tar
      bundle_corrupted_checksum.tar
    lifecycle_and_crash_recovery/
      journal_orphan_crash.jsonl
      journal_mid_finalize.jsonl
      session_after_completed.jsonl
      soak/                       # runtime generated
    e2e/
      record_5min_session_manifest.json   # runtime generated
      record_5min_session_bundle.tar      # runtime generated
```

规则:

- fixture 文件平铺,不嵌套子目录。
- fixture 在 `cargo test` 时是只读。
- E2E / soak 录制脚本写入新 fixture 时需明确路径。
- fixture 文件只包含预期数据,不包含 raw error 或敏感信息。

## Performance budget

macOS 首版性能目标(不是 SLA,达不到不阻塞 acceptance):

录制阶段(active recording):

| 指标 | 目标 |
| --- | --- |
| 平均 CPU | ≤15% 单核 |
| P95 CPU | ≤40% 单核 |
| 内存 | ≤1 GiB |
| 磁盘写入 | ≤10 MB/min(含 screenshot) |
| 事件采样率 sustained | 500 events/s |
| 事件采样率 burst | 2000 events/s,持续 ≤5 秒 |

编译阶段:

| 指标 | 目标 |
| --- | --- |
| 1 小时 Journal 编译时间 | ≤10 秒 |
| Bundle commit 时间 | ≤5 秒 |

回放阶段:

| 指标 | 目标 |
| --- | --- |
| 实时倍率 | ≥0.5x |
| 平均 CPU | ≤25% 单核 |
| 内存 | ≤500 MiB |

daemon 启动:

| 指标 | 目标 |
| --- | --- |
| 冷启动 | ≤3 秒 |

远程 delivery:

| 指标 | 目标 |
| --- | --- |
| 100 MB Bundle 完成传输 | ≤network_RTT × 1.5 + bandwidth-bound |

预算修订需新 ticket,不允许直接改本规格。

## Crash recovery scenarios

每个场景对应一个 integration test + 一个 fixture,使用 `SIGKILL`(不可捕获)触发,graceful shutdown 不算 crash。

### 场景 1: Recorder crash during `recording`

期望:

- Journal 保留为 orphan。
- Session 进入 `failed`。
- 不导出 Bundle。

Fixture: `tests/fixtures/acceptance/lifecycle_and_crash_recovery/journal_orphan_crash.jsonl`。

Test: `SIGKILL` daemon mid-record, restart, verify Session state + journal file。

### 场景 2: Recorder crash during `finalizing`

期望:

- staging 删除。
- Session `failed`。
- 不暴露 partial Bundle。

Fixture: `tests/fixtures/acceptance/lifecycle_and_crash_recovery/journal_mid_finalize.jsonl`。

Test: `SIGKILL` daemon during finalize, restart, verify staging path absent。

### 场景 3: Recorder crash after `completed`

期望:

- lifecycle metadata 保留。
- 允许 completed retry 重放相同归档。

Fixture: `tests/fixtures/acceptance/lifecycle_and_crash_recovery/session_after_completed.jsonl`。

Test: `SIGKILL` daemon after completed, restart, verify `@record-stop` retry returns same `bundle_sha256`。

crash 时间点通过 fixture `Mark { crash_after_ms: N }` 触发(测试-only 标记,不影响 production journal schema)。

crash recovery 用 `--crash-recovery` flag,默认 true。

## E2E acceptance

测试时长: 5 分钟真实 GUI 操作(每条测试用例)。

录制脚本: 真实 `rdog daemon` + `rdog record`(无需 mock)。

Bundle commit: 真实原子提交,验证 ticket `#9` 完整性。

Replay: 真实 `rdog replay` 在 fresh daemon session 中执行。

验证方式: replay 完成后的最终 AX tree snapshot 与 record 期最后一个 AX snapshot byte-equal。

Replay outcome: `replay_outcome == "completed"`,无 `performed:true,verified:false`。

Bundle integrity: `bundle_sha256` 与 lifecycle metadata 一致。

CI 集成:

- E2E 默认在 CI macOS runner 跑,Linux/Windows runner 标记为 skip with reason。
- E2E 不阻塞 release tag,但阻塞 merge to main(CI required check)。
- 每次 release 跑 5 次 E2E 随机操作(click / type / scroll / window move / resize)。
- E2E 失败时附 bundle path + journal.jsonl + flow.json,便于回归。

通过条件:

- 5 次 E2E 全部 `replay_outcome == "completed"`。
- 5 个 `bundle_sha256` 全部唯一(无重复归档)。
- 5 次 replay 后的 AX tree snapshot 与 record 期最后一个 snapshot byte-equal。
- Bundle integrity 校验通过(per-file + whole-archive)。
- 录制期间 CPU / 内存 / 磁盘在 ticket `#7` 预算内。

E2E 默认 5 分钟,允许 `--e2e-duration` flag 调整(测试 only,不影响 production)。

## Soak scenarios

3 个场景合计 ≤2 小时 wall-clock,连续运行,不允许分天执行。

### 场景 A: 静态观察(1 小时)

打开 Chrome 窗口,无操作。

- 平均 CPU ≤5%,内存 ≤500 MiB,journal 增长 ≤100 KB。
- 验证: 无事件丢失(gap 检测),capture backend 健康。

### 场景 B: 高频 Key(30 分钟)

模拟 200 字/分钟连续打字。

- 平均 CPU ≤20%,内存 ≤800 MiB,journal 增长 ≤3 MB。
- 验证: text_merge 触发,debounce 触发,sensitive 字段被 redact。

### 场景 C: 中频 mouse + scroll(30 分钟)

模拟 scroll 1 Hz + click 0.5 Hz,共 90 次 scroll + 45 次 click。

- 平均 CPU ≤25%,内存 ≤1 GiB,journal 增长 ≤5 MB。
- 验证: mouse_move_coalesce 触发,scroll_coalesce stub 不崩溃,semantic promotion 正确。

每个场景独立录制独立 Bundle,事后分析各自 CPU / 内存曲线。验证通过条件:无 panic / 无 daemon crash / 无 gap(required lane failure)。

soak fixture 在 `tests/fixtures/acceptance/lifecycle_and_crash_recovery/soak/` 生成,不预生成。

soak 不引入新的内存 profiling hook,沿用 `top` / `ps` / `fs_usage` 采样。

soak 不验证性能预算数值,只验证"无回归"。

## Acceptance hard conditions

acceptance 通过要求所有 9 条硬条件全部满足:

- [ ] 7 大类测试全部跑通,无 fail / 无 ignore。
- [ ] crash recovery 3 场景全部通过。
- [ ] E2E 5 次随机操作全部 `replay_outcome == "completed"`。
- [ ] soak 3 场景无 panic / 无 daemon crash / 无 gap。
- [ ] 编译 / Bundle commit 时间在 ticket `#7` 预算内。
- [ ] 录制 CPU / 内存 / 磁盘在 ticket `#7` 预算内。
- [ ] daemon 冷启动在 ticket `#7` 预算内。
- [ ] Bundle integrity(per-file + whole-archive SHA-256)校验通过。
- [ ] 所有 reason code 命中 ticket `#4` / `#9` 命名空间,无未登记 code。

## Acceptance report

CI 自动生成 markdown 报告,路径 `tests/reports/acceptance_<date>.md`,旧报告保留 ≥10 份。报告**不**进入 Git commit,只作 CI artifact。

```markdown
# rdog Acceptance Report — <date>

## Summary
- Result: PASS / FAIL
- Ticket: #7
- 7 大类测试结果(per-class pass/fail count)
- crash recovery 3 场景结果
- E2E 5 次结果(bundle_sha256 + replay_outcome)
- soak 3 场景结果(P95 CPU / max RSS / disk growth)

## Detail
- 性能预算实测值 vs ticket #7 目标
- 失败用例 bundle path + journal.jsonl + flow.json
- 未命中 reason code 列表(应为空)
- 与上一 release tag 的回归差异

## Attestation
- macOS 版本 / 硬件
- daemon commit + rdog-control skill commit
- 跑测试的 runner 信息
```

规则:

- FAIL 时附首次失败时间戳 + 重试 3 次失败率。
- Report 是 audit trail,不替代 ticket 决策。
- 不引入新的 acceptance framework,沿用 cargo test + bash + markdown。

## CI integration

CI macOS runner required checks:

- `cargo test --test recording_replay_compiler`(compiler golden)。
- `cargo test --test acceptance_*` (新增 acceptance tests)。
- `bash tests/scripts/run_e2e.sh --duration 5m --runs 5`(E2E)。

CI skip with reason:

- Linux runner: skip E2E 与 soak,保留 unit / integration。
- Windows runner: 同 Linux。

CI 不阻塞:

- soak 测试,只在 release tag 时跑。

## Cross references

- `specs/rdog-recording-bundle-schema.md`:Bundle 物理形态与完整性。
- `specs/rdog-recording-bundle-schema.md#remote-delivery`:远程交付。
- `specs/rdog-replay-preflight-guard-verification.md`:preflight / guard / verification。
- `specs/rdog-recording-semantic-promotion-policy.md`:semantic promotion 规则。
- `specs/rdog-recording-window-geometry-policy.md`:Window Geometry Precondition。
- `specs/rdog-recording-session-lifecycle.md`:lifecycle 状态机。
- `specs/rdog-recording-journal-model.md`:Journal 模型。
- `specs/rdog-recording-redaction-parameter-model.md`:redaction 模型。
- `specs/rdog-recording-replay-compiler-prototype.md`:compiler prototype。
- `specs/control-line-protocol.md`:line-control 协议。
- `specs/rdog-flow-control-plan.md`:`@flow` runtime。

## Open questions

无。本规格已包含 ticket `#7` question 列出的所有边界。
