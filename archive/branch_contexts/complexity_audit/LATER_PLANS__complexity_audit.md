## [2026-07-30 01:28:06] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 后续计划: 按收益排序实施复杂度优化

### P0: 合并 macOS app metadata 查询

- 在现有 `list_running_apps()` 和 `running_app_for_pid()` JXA payload 中直接返回 `bundle_id`。
- 删除列表路径里的逐 pid `bundle_id_for_pid()` 外部进程调用,不新增缓存或 native wrapper。
- 保留 batched 与逐 pid bundle-id 等价测试,并复跑 3 轮 wall-time probe。

### P0: 让 durable retention_bytes 成为真实硬上限

- 在现有 `JsonlDurableObservationStore` 内实施 byte-aware compaction,同时覆盖 `selectors.jsonl`、`ref_cache.jsonl`、`observations.jsonl` 和 `index.json`。
- 明确 stable selector 历史的保留优先级,避免压缩时破坏 selector-get/refind 语义。
- 增加超过 byte cap 后文件总量回落、corrupt-index replay 和原子替换中断恢复测试。

### P1: 消除 observation 写入的二次复杂度和逐条文件打开

- 先建立 `ref_id -> selector_id` 借用索引,把 O(R*S) 改为 O(R+S)。
- 一次 observation 内对 selector/ref-cache 各使用一个 `BufWriter`,只 open/flush 一次。
- 增加 1/200/2000 selector Criterion benchmark;没有测量收益前不继续引入额外索引层。

### P2: 收敛 Web actionable ancestor 遍历

- 用最近 actionable ancestor 代替完整 `ancestors: Vec` 复制。
- 保留 ancestor promotion、去重、match_count、limit 和 deep refresh 行为。
- 只有真实 Web AX 树 profile 仍显示字符串匹配为热点时,再处理 lowercase 常数项。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 状态更新: 原优化计划已实现

- [x] P0 macOS app metadata batching 已实现并完成 3 轮 probe。
- [x] P0 durable byte cap/compaction 已实现并通过重开与 corrupt replay 测试。
- [x] P1 O(R+S) map和批量 JSONL writer 已实现,2000 pairs测试通过。
- [x] P2 Web actionable ancestor traversal 已实现,19 个 Web tests通过。

### 仍需未来执行

- [ ] 真实 GUI E2E 需要配置 live fixture 环境后再跑;本轮 15 个 live tests 被 gate 跳过。
- [ ] 本机 `mac.lab` 仍约 716 MiB。本轮没有擅自执行会删除历史记录的实际 compaction;新代码会在使用新 binary 的下一次 durable store open 时按 50 MiB 配置收敛。若历史必须保留,应先备份该目录。
