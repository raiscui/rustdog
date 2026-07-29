# rdog Wayfinder resolution spec overdesign simplification

## Status

本规格是 Wayfinder ticket [Simplify Wayfinder resolution spec overdesign](https://github.com/raiscui/rustdog/issues/13) 的 resolution asset。

它固化已 close 的 4 个 Wayfinder resolution spec 累积过度设计抽象的合并/删除/推迟决策,作为后续实施/重写/精简的单一权威依据。

本规格只描述删减/合并方案,不重写 closed spec;每个改动标注 ceiling 与升级触发条件。

## Scope

简化目标 spec:

- `specs/rdog-replay-preflight-guard-verification.md` (ticket #4)
- `specs/rdog-recording-bundle-schema.md` (ticket #9)
- `specs/rdog-recording-replay-compiler-prototype.md` (ticket #8)
- `specs/rdog-acceptance-matrix.md` (ticket #7)

不简化(已 close 且已 saturate):

- `specs/rdog-recording-session-lifecycle.md` (ticket #5)
- `specs/rdog-recording-journal-model.md` (ticket #10)
- `specs/rdog-recording-redaction-parameter-model.md` (ticket #3)
- `specs/rdog-recording-semantic-promotion-policy.md` (ticket #6)
- `specs/rdog-recording-window-geometry-policy.md` (ticket #11)
- `specs/rdog-macos-operation-capture-research.md` (ticket #12)

## Terms

- **ceiling**: 简化引入的已知性能/语义上限,超过时升级。
- **upgrade trigger**: 触发再次扩展的具体业务信号。
- **fail closed**: 简化后所有未知/边界场景回到最严格语义边界。
- **overdesign YAGNI**: 为"将来可能"预先设计的抽象。

## Invariants

1. 每个简化必须显式标注 ceiling 与 upgrade trigger,否则不通过。
2. 简化必须保留 close 原 spec 的 fail closed 语义,不能弱化安全门禁。
3. reason code 命名空间仍按 ticket `#4` / `#9` 风格(snake_case + structured_field),不允许第二套。
4. closed spec 一旦简化,通过本规格"建议删减"清单;实际重写在后续实施 ticket,本 ticket 不强制改动。
5. 简化不引入新依赖,不引入新 crate,不引入新 binary。

## Simplification: ticket #4 preflight gates

### 现状

8 gates + 11 reject reason codes + 3 action classes + 5 rollback triggers。

### 简化方案

**8 gates → 5 gates** 合并:

- `permission gate` + `application gate` → `permission gate`(Permission gate 同时验证 TCC 与 app reachability)。
- `participating-window gate` + `geometry precondition gate` → `window gate`(Window gate 同时验证唯一命中 + geometry restore)。
- `parameter gate`、`display topology gate`、`selector freshness gate`、`coordinate guard gate` 不变。

**11 reject codes → 6 reject codes** 合并:

- `permission_denied` 同时覆盖 permission 与 application 失败。
- `window_unresolved` 覆盖 `window_ambiguous` + `window_missing` + `geometry_precondition_failed`(具体类型用 structured_field)。
- 其余 8 codes 不变。

**action class 不变**:`state-mutating` / `state-read` / `input-primitives` 仍按 close spec。

**rollback trigger 不变**:5 类 trigger 仍按 close spec。

### ceiling

| 简化 | ceiling | upgrade trigger |
| --- | --- | --- |
| permission 合并 app | 多 app 链式 launch (Shell `open -a`) 需要独立 app reachability gate | ticket 提及时 |
| window 合并 geometry | 三方 drag 时 participating window 与 geometry 分别精确定位 | ticket 提及时 |

### skipped

- 不改 best-effort profile (close spec 已 minimal)
- 不改 Bundle provenance gate 顺序 (前置契约不简化)

## Simplification: ticket #9 Bundle manifest

### 现状

manifest 4 required fields (`recording_id` / `started_at_unix_ms` / `producer` / `compiler` / `files` / `redaction_summary` / `warnings`) + 11 reject reason codes + warnings 抽象 + redaction_summary 4 字段。

### 简化方案

**删除 `warnings` 字段**:多数场景没有 warning,失败直接 fail closed。`#9` 中规定的 2 个 warning code (`optional_evidence_failed` / `guarded_coordinate_fallback_used`) 改由 daemon 日志承载,不进 manifest。Reader 遇到 unknown additive `warnings` 字段时仍按 close spec 接受(向后兼容)。

**简化 `redaction_summary`** 4 字段 → 1 字段:

- 保留 `segment_count`(必需,Journal 必须能重算)。
- 删除 `required_parameter_count` / `suppressed_evidence_count` / `runtime_clipboard_exposure`(从 Journal 与 flow.json 派生,manifest 不冗余)。
- 简化后 redaction_summary 与 `manifest.files[*].sha256` 一起作为 commit 前 validator 必过项。

**11 reject codes → 4 reject codes** 归并:

- `delivery_failed:checksum_mismatch`(原 `#9` 已定义)
- `delivery_failed:bundle_too_large`(原 `#9` 已定义)
- `delivery_failed:cancelled`(用户主动中断)
- `delivery_failed:rate_limited`(owner-only 重试越界)
- 其余 7 个 reason codes(permission_denied、window_unresolved、parameter_unbound、application_unreachable、display_topology_invalid、selector_stale、coordinate_guard_missing/invalid)沿用 close spec `#4` reject codes,**不再重复定义**。Reader 看到 `#9` reject code 时同时承认 `#4` 命名空间。

### ceiling

| 简化 | ceiling | upgrade trigger |
| --- | --- | --- |
| 删除 warnings | 没有"非致命退化"通道 | 出现 partial delivery 重试(目前不需要) |
| redaction_summary 4 → 1 | 性能预算告警需要 manifest 字段 | 引入 SLA 预算告警 ticket 时 |
| 11 codes → 4 | reason code 不可读区分 | reader 投诉难定位时 |

### skipped

- 不改 manifest identity (`recording_id` + `started_at_unix_ms` 双字段最小)
- 不改 per-file SHA-256 + whole-archive SHA-256 两层完整性
- 不改 USTAR determinism 与 canonical JSON bytes
- 不改 evidence allowlist (3 个 role)
- 不改 size limit (256 MiB / 384 MiB)
- 不改 remote delivery single-frame `@savefile` 协议

## Simplification: ticket #8 compiler prototype

### 现状

11 pass:debounce / mouse_move_coalesce / scroll_coalesce / text_merge / shortcut_hotkey / sleep_mark / semantic_promotion / coordinate_fallback / window_precondition / redacted_parameter / source_provenance。区分 full / stub / emit-time 三类实现。

### 简化方案

**移除 "stub" / "emit-time" / "full" 标签**:实现细节不进 spec。spec 只声明每个 pass 的语义(是否改变 replay 行为)与输入输出契约,不声明实现状态。

**11 pass → 不变**:每个 pass 都对应 ticket question 中的一项。合并会损失语义边界。

### ceiling

| 简化 | ceiling | upgrade trigger |
| --- | --- | --- |
| 移除实现标签 | reviewer 看不到 prototype 进度 | spec 加 status table 时 |

### skipped

- 不改 determinism contract (BTreeMap key sort + serde_json compact)
- 不改 paired `(event, original_index)` 传播机制
- 不改 fixture `tests/fixtures/replay-compiler/` 内容
- 不改 integration test 6 个用例

## Simplification: ticket #7 acceptance matrix

### 现状

7 大类测试 + 9 硬条件 + 3 soak 场景 + acceptance report 3 sections (Summary / Detail / Attestation)。

### 简化方案

**3 soak 场景 → 1 soak 场景**:

- 保留 soak A (1h 静态观察) 作为 release tag 必跑 soak。
- 删除 soak B (高频 Key) 与 soak C (中频 mouse+scroll),原因:fixture 已经有 unit test 覆盖 (`#8` prototype 6 个 test);soak 与 unit test 重叠是 overdesign。
- 简化后 soak 失败不阻塞 release tag,但 report 标"skipped, see unit test"。

**3-section report → 1 JSON block**:

- acceptance report 简化为 CI artifact `tests/reports/acceptance_<date>.json`,字段:pass/fail per class、bundle_sha256s、replay_outcomes、soak status、unsupported reason codes。
- markdown 渲染在 PR review 时由 reviewer 按需触发,默认不进 commit。
- 旧 markdown 报告保留 ≥10 份的规则不变,只是默认产生 JSON。

**9 硬条件 → 6 硬条件** 合并:

- "编译 / Bundle commit 时间在 ticket #7 预算内" + "录制 CPU / 内存 / 磁盘在 ticket #7 预算内" + "daemon 冷启动在 ticket #7 预算内" → 合并为 "性能预算执行不被阻塞"(原 close spec 已声明预算不是 SLA,不阻塞 acceptance)。
- "Bundle integrity 校验通过" 不变。
- 其余 5 条不变。

### ceiling

| 简化 | ceiling | upgrade trigger |
| --- | --- | --- |
| 3 soak → 1 soak | 高频事件回归检测 | 出现 production 性能回归 ticket 时 |
| markdown → JSON | reviewer 失去 markdown 渲染 | 团队偏好 markdown 报告时 |
| 9 → 6 硬条件 | 性能预算偏离没独立 red flag | 引入 SLA 后 |

### skipped

- 不改 7 大类测试归类
- 不改 E2E 5 分钟 + 5 次随机操作 + byte-equal AX tree snapshot
- 不改 crash recovery 3 SIGKILL 场景
- 不改 performance budget 数值 (录 15% / 40% P95 CPU / 1 GiB RAM 等)

## Cross references

- `specs/rdog-replay-preflight-guard-verification.md`:5 gates + 6 codes 简化目标
- `specs/rdog-recording-bundle-schema.md`:删除 warnings + 简化 redaction_summary + 归并 reject codes
- `specs/rdog-recording-replay-compiler-prototype.md`:移除实现标签
- `specs/rdog-acceptance-matrix.md`:1 soak + JSON report + 6 硬条件
- ticket `#13` parent: Wayfinder map `#2`

## Open questions

无。每个简化都有 ceiling + upgrade trigger。
