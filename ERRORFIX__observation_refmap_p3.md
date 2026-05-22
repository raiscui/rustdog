## [2026-05-20 23:18:46] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 错误修复: P3 selector-refind 编译与 fresh target 边界

### 问题

- 初版 `candidate_kind(&candidate) != expected_candidate_kind(selector)` 把 `Option<&str>` 和 `&str` 直接比较,导致 `control_observation::refind::tests` 编译失败。
- Deslop 扫描发现 `candidate.get("observation").is_none()` 只检查字段是否存在。如果候选里有 `"observation":null`,会绕过 hard gate,可能生成空的 `verify_hint`。

### 原因

- 编译错误是类型层面的接线失误。
- fresh target 判断把 JSON 字段存在性误当成语义有效性,没有验证 `observation_id` 和 `ref` 是否同时存在。

### 修复

- 将 kind 比较改为 `candidate_kind(&candidate) != Some(expected_candidate_kind(selector))`。
- 新增 `candidate_has_fresh_observation()`,要求 candidate 的 `observation.observation_id` 与 `observation.ref` 都存在。
- 新增 `refind_candidate_without_fresh_target_should_not_rebound` 测试,锁住 `observation:null` 时只能 `needs_disambiguation`,不能返回 `fresh_target`。

### 验证

- `cargo test --package rustdog --bin rdog control_observation::refind::tests`: 6 passed。
- Post-deslop 全部 P3 验证矩阵通过,详见 `WORKLOG__observation_refmap_p3.md`。
