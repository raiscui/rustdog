## [2026-07-22 12:16:03] [Session ID: omx-1784512435044-92wxat] 任务名称: 定义 rdog.recording.v1 Recording Journal 模型

### 任务内容

- 解决 Wayfinder ticket `定义 rdog.recording.v1 Recording Journal 模型`,不实现 Recorder 生产代码。
- 创建正式 Journal 规格、长期知识索引,关闭 ticket 并更新 Wayfinder map。

### 完成过程

- 固定 UTF-8 JSON Lines encoding、公共 envelope、`journal_seq` canonical order 与 `capture_seq` provenance。
- 固定 9 个顶层 event families、recording-scoped window/display identity 和完整 context snapshot。
- 固定 raw physical 不可变、semantic candidate 后续追加与事实型 provenance。
- 固定 lane/redaction/gap/mark/terminal transition、start/mark/terminal durability barrier 和 crash orphan fail-closed。
- 固定 v1 additive evolution、mixed-major rejection 和 Replay compiler unknown-event fail-closed。
- 创建 `specs/rdog-recording-journal-model.md`,并在 `AGENTS.md` 增加索引。
- 通过 commit `b677264d75cd6588def6bcf23bbde22c2a1651c0` 推送到 `origin/main`。
- 发布 resolution、关闭 ticket,并在 map 的 Decisions so far 增加 Journal pointer。

### 验证

- 两个 Mermaid 图均通过 `beautiful-mermaid-rs --ascii`,输出分别为 `13704` 和 `11926` bytes。
- 6 个 JSON code block 全部通过 `jq` parse,18 个 Markdown fence 成对。
- 8 个引用规格存在,`git diff --cached --check` 无输出。
- GitHub commit API、ticket state、map pointer/fog 和 native dependency graph 均已动态复核。

### 总结感悟

- `journal_seq` 与 `capture_seq` 分工可以同时保住全局追加顺序和物理丢失证据,但不能让 timestamp 成为第三排序源。
- append-only 的正确边界是 physical 不变、semantic candidate 后补,不是等待富化后改写 raw event。
- crash recovery 在当前 lifecycle 中只负责验证和 privacy-first cleanup,不应该借 Journal 格式重新引入 active Session resume。
