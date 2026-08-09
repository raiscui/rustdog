## [2026-07-30 01:28:06] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 任务名称: Rustdog 算法复杂度只读审计

### 任务内容

- 对当前 dirty working tree 做 Rust 生产路径复杂度和性能热点审计。
- 覆盖 macOS window/AX、Web、observation/durable selector、flow、protocol/frame、screenshot 和 Zenoh 路径。
- 只修改支线上下文日志和主计划索引,没有修改生产代码。

### 完成过程

- 运行 complexity-optimizer scanner并证伪两个非 Rust 命中。
- 逐段读取真实调用路径,区分算法阶数、外部进程常数、输入上限和冷路径。
- 对 macOS N+1 `osascript` 做 3 轮源码等价 probe,并验证 batched payload 的 bundle-id 等价性。
- 只统计 durable state 元数据、文件大小和记录数,确认 50 MiB cap 未执行。
- 建立 4 项排序结论,并把未实施工作写入支线 LATER plans。

### 验证

- `cargo nextest` durable tests: 4 passed。
- `cargo nextest` Web tests: 19 passed。
- `cargo nextest` window tests: 31 passed。
- `cargo check --package rustdog --bin rdog --quiet`: 0 errors,6 warnings。warning 来自既有 dirty production files,本次未改。
- 支线上下文 `git diff --check` 通过。

### 总结感悟

- 当前最便宜且收益最确定的优化是把 bundle id 放进现有 JXA 批量结果,不是新增缓存或 native abstraction。
- durable 路径的第一优先级不是微调 HashMap,而是让已经配置的 byte cap 真正生效。否则所有局部优化都会被无界历史增长吞掉。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 任务名称: 按复杂度审计建议实施优化

### 任务内容

- 合并 macOS app metadata JXA 查询。
- 为 durable observation 实施 byte cap、compaction、O(R+S) ref 映射和批量 JSONL 写入。
- 消除 Web traversal 的逐节点 ancestor Vec 复制。

### 完成过程

- 修改 `src/control_window/macos.rs`,批量返回 bundle id并新增 parser-level 降级测试。
- 修改 `src/control_observation/durable.rs`,新增 `src/control_observation/durable/tests.rs`,把生产文件控制在 929 行。
- 在当前 dirty `src/control_web.rs` 上只替换 traversal 参数和递归状态,未回退用户其他 Web 改动。
- scoped review 发现并修复 compaction orphan cost 的 O(K²) 扫描,以及 duplicate key 被 HashSet 折叠的问题。

### 验证

- 649 bin tests passed,1 skipped。
- 18 个额外 integration tests passed,1 skipped。
- macOS JXA 3 轮中位 882.5 ms,相对旧基线中位 1661.4 ms 降低约 46.9%。
- all-target check 0 errors;scoped rustfmt/diff checks通过。

### 总结感悟

- 最有效的性能修复仍是删除外部进程 N+1,不需要缓存。
- byte cap 不能只裁 index。日志、index 和 corrupt replay 必须一起设计,否则“上限”只是 metadata。
