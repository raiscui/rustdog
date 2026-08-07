## [2026-07-30 01:52:00] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 错误修复: cargo fmt 扩大格式化范围

### 现象

- 运行 `cargo fmt -- src/control_window/macos.rs` 后,3 个原本 clean 且不在本 lane 所有权内的 Rust 文件出现格式 diff。

### 原因

- `cargo fmt` 仍按 Cargo target/module graph 处理文件,`-- <path>` 不能作为可靠的单文件所有权边界。

### 修复

- 对照命令前的 `git status` 确认新增文件集合。
- 逐文件审阅 diff,确认仅为 rustfmt 变化后,只恢复 `src/control_observation/observe/request.rs`、`src/control_observation/observe_tests.rs`、`src/control_protocol/tests.rs`。
- 后续改用 `rustfmt --edition 2021 <exact files>`。

### 验证

- 恢复后 3 个文件不再出现在 `git status --short`。
- 目标文件 `src/control_window/macos.rs` 的功能 diff 保留。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 错误修复: 验证并发与 compaction key 计数

### 问题 1: 并行 nextest 删除运行中的测试二进制

- 现象:全 bin run 报 `double-spawn ... No such file or directory`,422/649 已启动测试中 421 passed、1 failed。
- 原因:另一个 nextest 同时重建 target测试 binary,导致运行中的 nextest找不到原 binary。
- 修复:所有 Cargo 验证改为串行。最终全 bin run 649 passed,1 skipped。

### 问题 2: selector multiset 一次替换遗漏

- 现象:新增 duplicate compaction test 首次编译报 `cannot find value selector_keys`。
- 原因:从 HashSet 迁移为 key count HashMap 时,一个 rustfmt 后的 closure 没有被首轮文本替换命中。
- 修复:定点改为 `take_selector_key(&mut selector_counts, ...)`,并让 load/retain/size 都按重复 key计数。
- 验证:durable 8 tests通过,duplicate compaction 保持 2 条 selector记录。
