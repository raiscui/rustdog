## [2026-05-19 23:50:49] [Session ID: 019e38be-b9d9-76f0-aabc-fad94a2bcf12] 后续计划: P1 之后的 selector/re-find 演进

### 背景
- 本轮 P1 只细化 durable observation state 和 selector draft / selector envelope。
- P1 明确不做自动 semantic re-find、`@observe` bundle 和 mouse ref 化。

### P1 后续延期项
- [ ] P1b: JSONL compact、fsync 强化、以及 SQLite 迁移评估。
- [ ] P2: permanent selector 稳定 schema,允许跨 observation 作为显式 target 输入。
- [ ] P3: semantic re-find、candidate set、confidence ranking 和不确定性解释。
- [ ] P4: 新增 `@observe` bundle,统一 screenshot / AX / window observation。
- [ ] P5: mouse command ref 化,但仍保持语义控制优先。

### 触发条件
- P1 durable observation state 实现并验证完成。
- `selector_count` 已经从固定 `0` 变成真实 selector draft 数量。
- daemon 重启后旧 ref 不复活,但 error hint 能指向 durable selector 线索。
