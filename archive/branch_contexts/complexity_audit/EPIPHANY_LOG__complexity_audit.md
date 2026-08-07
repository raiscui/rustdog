## [2026-07-30 01:28:06] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 主题: durable observation 的 50 MiB 配置目前不是硬上限

### 发现来源

- 复杂度审计读取 `JsonlDurableObservationStore` 的 prune/write/replay 路径,并只统计本机 `mac.lab` 文件大小和记录数。

### 核心问题

- `retention_bytes` 只写入 metadata,没有参与裁剪或 compaction。
- `mac.lab` 当前约 716 MiB,是配置 50 MiB 的约 14.3 倍。
- `selectors.jsonl` 和 `ref_cache.jsonl` 各有 463,808 行;`index.json` 约 20 MiB,每次 observation 都会完整重写。

### 为什么重要

- 这不是理论上的 O(n) 争论,而是已发生的资源边界失效。
- 一旦 `index.json` 损坏,replay 会重新读取约 624 MiB 的 selector JSONL;启动延迟和峰值内存风险会突然暴露。

### 当前结论

- 已验证:磁盘上限没有执行,日志没有 compaction,index 包含 6,320 条已不属于 retained observations 的 stable selector 记录。
- 未验证:当前用户可感知 latency 中有多少由 index rewrite 或逐条 append 导致,正式修复前仍需 Rust benchmark。

### 后续讨论入口

- 先读 `src/control_observation/durable.rs:218-270,358-398,467-550` 和 `LATER_PLANS__complexity_audit.md` 的 P0/P1。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 主题更新: byte cap 已在代码层修复,现有历史尚未裁剪

### 已落地

- store open/record 都执行 byte cap检查,超限后原子替换 compact index/JSONL。
- synthetic cap、重开和 corrupt-index replay测试通过。

### 仍然重要的操作边界

- 当前 `mac.lab` 仍是 733,588 KiB、931,768 条 JSONL records。
- 第一次用新 binary 打开该 store 会删除超出 50 MiB policy 的旧历史。这是配置要求的行为,但也是不可逆历史裁剪。
- 本轮只实施并验证代码,没有主动启动 daemon 改写用户数据。
