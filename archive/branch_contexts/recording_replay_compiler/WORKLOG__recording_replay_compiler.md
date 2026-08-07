## [2026-07-29 09:30:00] [Session ID: omx-1784512435044-92wxat] 任务名称: 原型验证 Recording Journal → rdog.flow.v1 编译器 (ticket #8)

### 任务内容

- 落盘 Wayfinder ticket `#8` 的 prototype resolution asset。
- 写一个 standalone Rust binary,演示 Recording Journal → Replay Script 的 10 项 compiler 优化,验证 determinism。
- 把 prototype 推到独立分支 `prototype/recording-replay-compiler`,不污染 main 与 auto-optimize dirty worktree。

### 完成过程

- 在 `/tmp/rdog-replay-compiler-prototype` 创建独立 git worktree,基于 main b6bc5f8。
- 写 `src/bin/replay_compiler.rs` (719 行):
  - `JournalEvent` enum: Key / Click / MouseMove / Scroll / AxPress / AxValue / WindowGeometry / Mark / SessionTerminal。
  - `FlowStep` enum: Key / Click / MouseMove / Scroll / AxPress / AxValue / WindowResize / TypeText / KeyChord / Sleep。
  - `Pass` trait 用 paired `(event, original_index)` 传播,index_map 始终跟随 events,避免 provenance 错位。
  - 11 项 pass:debounce / mouse_move_coalesce (full),scroll_coalesce / text_merge / shortcut_hotkey / sleep_mark / semantic_promotion (full) / coordinate_fallback / window_precondition / redacted_parameter / source_provenance (emit-time 或 stub)。
  - 编译器 pipeline 顺序固定,emit_steps 维护 journal_index_range。
  - `serialize_canonical` 用 `BTreeMap` 排序 object key + `serde_json` compact,保证 determinism。
- 写 fixture `tests/fixtures/replay-compiler/journal_optimizations.jsonl` (28 events):WindowGeometry / AxPress / Click / 5 keys → text_merge / redacted keys / single key / 3 mouse_moves / 2 scrolls / Mark redaction / AxValue redacted / AxPress Submit / debounce x,x,y。
- 写 integration tests `tests/recording_replay_compiler.rs` (119 行,6 个 test):fixture parses / determinism byte-equal / pass coverage / provenance consistency / semantic promotion suppression / redacted parameter emission。
- 写 spec `specs/rdog-recording-replay-compiler-prototype.md` (224 行):scope / layout / determinism contract / pass 表格 / 各 pass 语义 / fixture 覆盖 / test coverage / limitations / 留给 production 的延后工作。
- commit 2b59688 on prototype/recording-replay-compiler,推送 origin。
- 关闭 ticket #8 + 更新 Wayfinder map #2。
- 主 repo AGENTS.md 末尾追加 prototype 索引条目(保持 dirty,不 commit,等用户 merge)。

### 总结感悟

- prototype ticket 与 spec ticket 工作流不同:prototype 需要实际 code + test + fixture,而不是纯 markdown 规格。
- 用 git worktree 在独立分支写 prototype 是最不影响用户 working tree 的方式。
- Pass trait 用 paired `(event, original_index)` 是关键设计 — 否则 pass 删除 events 后,provenance 的 journal_index_range 会错位。第一版用 index_map 中间层是错误设计,改成 paired 后立刻干净。
- emit-time 优化(text_merge / sleep_mark 等)与 pass-time 优化语义相同,只是实现位置不同。spec 显式区分两者,让 reviewer 知道哪些 pass 是 stub / emit-time。
- determinism 验证需要 BTreeMap key sort + serde_json compact,不能用普通 HashMap iteration。
- 单测用 `include!` 把 binary source embed 进 test module,避免把 binary 拆成 lib + bin (会扩大改动)。
- 7 个 spec ticket / prototype ticket 都在用户 dirty worktree 的外部独立 commit,不污染用户的 rdog-control 优化工作流。
