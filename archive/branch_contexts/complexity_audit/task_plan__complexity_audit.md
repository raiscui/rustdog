# 任务计划: Rustdog 算法复杂度只读审计

## [2026-07-30 01:16:01] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [计划]: 建立审计基线

## 目标

对当前 working tree 做完整的算法复杂度和性能热点审计。最终报告必须给出真实源码位置、调用语境、复杂度估计、建议、风险和验证方法。除支线上下文日志外,不修改任何生产文件。

## 成功标准与停止条件

- 扫描器候选已经过源码上下文复核,不把模式命中直接写成已确认问题。
- 重点覆盖控制协议、GUI/AX/Web 查询、observation/refmap、flow、Zenoh、截图和录制相关路径。
- 每项结论区分算法阶数问题、常数项问题和仅在规模增长后才成立的风险。
- 报告明确工作树状态、验证命令、未运行的动态基准及残余不确定性。
- 所有阶段完成后停止,不擅自实施优化。

## 两个方向

1. 不惜代价,最佳方案:运行首轮扫描,沿调用链复核候选,检查测试与输入规模,必要时做只读构建/测试证据,形成完整排序报告。
2. 先能用,后面再优雅:只输出扫描器命中和明显嵌套循环,速度快,但对 Rust 支持弱,误报和漏报都较高。

## 做出的决定

- [x] 采用方向 1。用户显式调用完整 complexity audit workflow,而 scanner 本身不支持 Rust,必须人工复核真实路径。
- [x] Ponytail full 生效。只建议能证明值得存在的优化;小输入、冷路径和一次性初始化不会因为形式上的 O(n²) 就自动列为高优先级。
- [x] 当前工作树包含大量既有未提交改动。审计以当前 working tree 为快照,不回退、不格式化、不覆盖用户改动。

## 阶段

- [x] 阶段 1: 读取仓库上下文、技能规则、工作树状态和构建入口。
- [ ] 阶段 2: 运行首轮扫描并生成候选热点清单。
- [ ] 阶段 3: 沿真实源码路径复核复杂度、输入规模、调用频率和既有测试。
- [ ] 阶段 4: 汇总排序报告,完成只读验证和交付。

## 已遇到错误

- [x] CodeGraph MCP 首次调用返回 `unsupported call: mcp__codegraph__codegraph_explore`。本次不使用该索引作为证据,改用 scanner、`rg` 和定点源码阅读。
- [x] `rtk find` 对组合参数的压缩输出不可用于文件规模统计。后续改用 `rtk proxy find` 获取原始证据。

## 当前状态

**阶段 2 进行中**: 运行 skill 自带 scanner,同时枚举 Rust 源码规模、测试入口和可能的迭代/搜索热点。

## [2026-07-30 01:17:23] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [阶段完成]: 首轮扫描已完成

- [x] 阶段 2: skill scanner 已运行并记录原始命中。
- [x] 已确认 scanner 不覆盖 Rust 生产代码,两个命中只保留为待证伪线索。
- [x] 已完成 Rust 源码规模和 benchmark/test 入口盘点。
- [ ] 阶段 3: 复核 AX/Web/observation 树查询、window 枚举、flow/parser/session 和 screenshot 路径。

**阶段 3 进行中**: 先检查 scanner 两个命中是否成立,再沿真实 Rust 热路径定位重复扫描、嵌套候选匹配和不必要排序。

## [2026-07-30 01:25:19] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [阶段完成]: Rust 热路径复核完成

- [x] 阶段 3: 已覆盖 AX/Web/observation/window/flow/parser/frame/screenshot/Zenoh 相关迭代路径。
- [x] 已确认 2 个带动态证据的高影响问题,以及 2 个需要 benchmark 的静态复杂度问题。
- [x] 已回滚不等价的 133-app 初始探针口径,最终只采用源码等价的 22-app 测量。
- [x] 已修正 durable state 总量口径:加入 `ref_cache.jsonl` 后约 716 MiB,不再使用早先漏项的约 646 MiB。
- [ ] 阶段 4: 运行 baseline targeted tests/build,核对工作树只增加支线上下文,整理最终报告。

**阶段 4 进行中**: 用现有测试证明当前行为基线仍通过,再交付按影响排序的只读报告。

## [2026-07-30 01:28:06] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [任务完成]: 复杂度审计已验证收口

- [x] 阶段 1: 基线、范围、技能和 dirty worktree 边界已确认。
- [x] 阶段 2: scanner 已运行,两个误报已证伪。
- [x] 阶段 3: Rust 热路径已复核,4 项 findings 已按影响排序。
- [x] 阶段 4: 54 个 targeted tests 通过,`cargo check` 0 errors,上下文 diff check 通过。
- [x] 已记录 6 条既有 compiler warnings,未在只读审计中修改用户生产代码。
- [x] 后续实施项已写入 `LATER_PLANS__complexity_audit.md`,重大 durable 风险已写入 `EPIPHANY_LOG__complexity_audit.md`。

**状态: complete**。停止条件已满足,交付完整报告。

## [2026-07-30 01:29:49] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [验证修正]: 重跑 untracked context diff check

- [x] 错误:首个批量检查脚本把 zsh 只读变量 `status` 当作普通变量,命令退出 1。
- [x] 回滚口径:此前“上下文 diff check 通过”只包含 tracked `task_plan.md`,不能覆盖新增 untracked 支线文件。
- [ ] 改用变量 `rc` 重跑 5 个 untracked 文件的 `git diff --no-index --check`,再检查 tracked 主计划。

**状态: final verification retry**。

## [2026-07-30 01:30:25] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [验证完成]: context diff check 重试通过

- [x] 5 个 untracked 支线上下文均通过 `git diff --no-index --check`。
- [x] tracked `task_plan.md` 通过 `git diff --check`。
- [x] shell 脚本错误已纠正,最终验证证据有效。

**状态: complete**。

## [2026-07-30 01:40:58] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [实施计划]: 按审计建议依次优化

### 目标与停止条件

- [ ] P0-A:macOS app metadata 改为单次 JXA 批量返回,删除 window list 路径的 N+1 `osascript`。
- [ ] P0-B:durable state 实施 byte cap 和原子 compaction,不再无限增长。
- [ ] P1:durable ref->selector 匹配改为 O(R+S),JSONL 改为每批一次 open/flush。
- [ ] P2:Web traversal 不再为每个节点复制 ancestor Vec。
- [ ] 每项都有行为回归测试;macOS 路径复跑 3 轮 probe,durable 增加规模与 cap 测试。
- [ ] 最终 targeted nextest、`cargo check`、`cargo fmt --check` 和 scoped diff review 全部通过。

### 所有权与并行边界

- 当前主线程只修改 `src/control_window/macos.rs`,负责 JXA batching、测试和 live probe。
- durable 执行 lane 只修改 `src/control_observation/durable.rs`。
- Web 执行 lane 只修改 `src/control_web.rs`、`src/control_web/tests.rs`。
- 所有 lane 必须保留当前 dirty worktree 里的既有改动,禁止回退或格式化无关代码。

### 实施原则

- [x] 不新增依赖、缓存层、数据库或 native AppKit abstraction。
- [x] duplicate ref id 保留原先第一个 selector 获胜的语义。
- [x] compaction 必须通过同目录临时文件 + rename,禁止先删旧日志。
- [x] Web 仍选择最近的 actionable ancestor,并保留去重、limit 和 match_count。

**状态: implementation in progress**。主线程先做 P0-A,其他两个独立 lane 并行。

## [2026-07-30 01:52:00] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [阶段进展]: P0-A 与 durable lane 已实现

- [x] P0-A 代码完成:`bundle_id` 已进入现有 JXA 批量 payload,逐 pid helper 已删除。
- [x] P0-A 2 个单测通过;3 轮 live probe 留到最终统一复测。
- [x] P0-B 代码完成:open/record 后执行 byte-aware compaction,index 和 JSONL 使用同目录 temp + rename。
- [x] P1 代码完成:首个 duplicate ref 语义保持,O(R+S) map 和 batch `BufWriter` 已落地。
- [x] durable tests 8 passed,包含 2000 pairs、byte cap、重开、corrupt-index replay、duplicate first-match。
- [x] `src/control_observation/durable.rs` 测试迁到 `src/control_observation/durable/tests.rs`,生产文件 907 行。
- [ ] P2 Web traversal 尚未实施。
- [ ] durable compaction 仍需 scoped review 和全量相关测试。

### 已处理执行错误

- [x] Native subagent 两个 spawn 均返回 `unsupported call`;没有生成或修改任何 agent workspace,主线程接管全部 lane。
- [x] `cargo fmt -- src/control_window/macos.rs` 意外格式化 3 个所有权外文件;diff 证明仅格式变化后,已只恢复这 3 个此前 clean 的文件。
- [x] 后续一律使用直接 `rustfmt --edition 2021 <exact files>` 格式化目标文件。

**状态: P2 in progress**。下一步只改现有 Web traversal 参数和最小回归测试。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [实施完成]: 4 项复杂度优化已收口

- [x] P0-A macOS metadata batching完成,3 轮 probe 中位 882.5 ms,旧基线中位 1661.4 ms。
- [x] P0-B durable byte cap/atomic compaction完成。
- [x] P1 ref-selector O(R+S)和 batch JSONL完成。
- [x] P2 Web nearest actionable ancestor完成。
- [x] 行为验证:649 bin passed/1 skipped,density 3 passed,control lanes 15 passed/1 skipped。
- [x] 静态验证:all-target check 0 errors,scoped rustfmt/diff checks通过。
- [x] 证据边界:15 个 live GUI E2E被环境 gate跳过,未声称通过。
- [x] 数据边界:没有主动裁剪现有 716 MiB `mac.lab`;首次新 store open才会执行不可逆 compaction。

**状态: complete**。所有可安全自动执行的实施和验证步骤均已完成。

## [2026-07-30 09:58:12] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [验证轮次]: verification-before-completion fresh gate

- [ ] 完整 bin `cargo nextest run --package rustdog --bin rdog`。
- [ ] 非 live integration: `computer_use_density`、`control_lanes`。
- [ ] `cargo check --package rustdog --all-targets --quiet`。
- [ ] scoped `rustfmt --check`、tracked/untracked `git diff --check`。
- [ ] 重新读取输出并区分 passed、skipped、warnings、blocked,不引用旧轮次结果。

**状态: fresh verification in progress**。只有本轮命令证据支持后才更新 completion claim。

## [2026-07-30 10:01:50] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] [验证完成]: fresh verification-before-completion gate

- [x] 完整 bin nextest fresh: `649 passed, 1 skipped`,退出 0。
- [x] 非 live integration fresh: `18 passed, 1 skipped`,退出 0。
- [x] all-target cargo check fresh:退出 0,`0 errors,11 warnings`。warning 全部来自既有 dirty-worktree 文件,本次没有声称 warning-free。
- [x] scoped rustfmt fresh:退出 0。
- [x] tracked/untracked scoped diff check fresh:通过。
- [x] removed-pattern fresh check:通过,旧 N+1 helper、ancestor Vec copy 和旧 resolver 均不存在。
- [x] live GUI E2E fresh: `0 passed,15 skipped`,实际 nextest exit 4(`no tests to run`),已作为环境缺口保留,未计入通过数。

**状态: verified with explicit live-E2E gap**。没有新的代码改动发生在本轮 fresh gate 之后。
