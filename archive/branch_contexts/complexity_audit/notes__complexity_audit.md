## [2026-07-30 01:17:23] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 笔记: 基线与首轮扫描

## 来源

### skill scanner

- 命令: `python3 /Users/cuiluming/.codex/skills/complexity-optimizer/scripts/analyze_complexity.py <repo> --format markdown`。
- scanner 只命中 `tests/fixtures/macos_display_aware_fixture.swift:155` 的嵌套迭代,以及 `scripts/bench_computer_act_density.py:190` 的循环内 membership。
- skill 的语言清单没有 Rust。两个命中都不是 Rust 生产路径,因此 scanner 输出只能作为待复核线索,不能作为“生产代码无热点”的结论。

### 仓库规模

- Rust 生产源码约 62,490 行。
- 最大模块包括 `src/control_ax.rs` 3,577 行、`src/control_window/macos.rs` 2,714 行、`src/control_window.rs` 2,147 行、`src/control_ax/macos.rs` 1,847 行。
- 仓库没有 `benches/` 基准文件,也没有 Criterion、pprof 或 samply 配置命中。
- 现有动态性能入口主要是 `@gui-bench` 和 `scripts/bench_computer_act_density.py`,它们测请求密度,不是算法微基准。

## 初步判断

- 当前最高价值的人工复核范围是 AX/Web/observation 查询,因为它们处理递归树、候选排序、selector/refind 和跨窗口枚举,输入规模可能随真实 UI 树增长。
- control protocol parser、flow 和 session frame 路径也要检查,但其输入通常受单帧/单脚本边界限制,形式上的线性扫描未必值得优化。
- screenshot 像素合成天然是 O(total pixels),除非存在重复整图扫描或逐像素高常数操作,否则不应仅因循环而列为问题。

## [2026-07-30 01:25:19] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 笔记: Rust 热路径复核结论

## 已验证高影响问题 1: macOS window discovery 的 N+1 osascript

- 调用路径: `find()` -> `enumerate_candidates()` -> `list_running_apps()` -> `running_app_from_value()` -> `bundle_id_for_pid()` -> `run_jxa_script()`。
- 静态证据: `list_running_apps()` 已用一次 JXA 取得 app 列表,但 `running_app_from_value()` 对每个 pid 再启动一次 `osascript` 查询 bundle id。复杂度是 1+A 次外部进程启动。
- 动态证据:源码等价列表当前返回 22 个 app。当前做法三次 warm 测量总耗时为 1887.2/1661.4/1656.3 ms,中位 1661.4 ms;其中 N+1 bundle 查询中位 827.4 ms。
- 最小可证伪替代:同一条 JXA 在 app map 内通过 `NSRunningApplication` 一次返回 `bundle_id`。三次为 1819.7/855.3/820.7 ms,中位 855.3 ms;冷首轮后 warm 时间约减半。
- 等价性检查:22 个 app 的 batched 结果与逐 pid 查询相比,missing=0,bundle id mismatch=0。
- 推荐:改良现有 JXA payload,不要新增 native AppKit wrapper 或缓存层。

## 已验证高影响问题 2: durable retention_bytes 未执行,日志与 index 增长失控

- 配置与模板默认 `retention_observations=256`,`retention_bytes=50 MiB`,`write_ref_cache=true`。
- 代码证据:`retention_bytes` 只保存在 struct 并写入 `meta.json`,没有用于 prune/compaction。现有测试还明确验证“prune index without compacting JSONL”。
- 动态证据:`mac.lab` 当前有 4,152 observation records、463,808 selector records、463,808 ref-cache records。
- 文件大小:selectors 624,442,770 bytes,ref cache 104,633,483 bytes,index 20,642,726 bytes,observations 1,463,595 bytes,合计约 716 MiB,超过 50 MiB 配置约 14.3 倍。
- index 组成:256 retained observations、10,191 selector rows、6,350 unique selector ids;其中 6,320 selector rows 已不属于 retained observations。每次新 observation 仍会 pretty-serialize 并原子重写整个约 20 MiB index。
- 结论:磁盘增长已动态确认。单次写入延迟没有直接 benchmark,因此“它是当前 latency 根因”仍不能下结论。
- 推荐:在现有 store 内执行 byte cap 和 JSONL compaction,并把 index 改为 compact JSON。不要新增数据库。

## 高置信静态问题 3: durable record 的 O(refs * selectors) 匹配和逐记录 open/flush

- `record_observation()` 对每个 ref 调用 `selectors.iter().find(...)`;AX `with_observation()` 为每个 window/element 同时生成一条 ref 和 selector,所以 R 约等于 S。
- 当前复杂度 O(R*S),可用一次 `HashMap<&str,&str>` 构建改为 O(R+S)。
- `append_jsonl()` 每条 selector/ref cache 都重新 open、serialize、write newline、flush。可在一次 observation 内各打开一个 `BufWriter`,把文件 open/flush 次数从 O(R+S) 降为 O(1)。
- 当前本机 retained observation 的 selector_count 最大 218;协议 capture 上限可达 5,000,而 u16 输入理论上可到 65,535。
- 这项尚无实际 Rust micro-benchmark,应在实施前增加 1/200/2000 selector 的 record benchmark。

## 中影响静态问题 4: Web 树遍历为每个节点复制祖先路径

- `collect_web_matches()` 每个节点都执行 `ancestors.to_vec()`,再 push 当前节点。时间和临时分配为 O(N*D),退化链式树为 O(N^2)。
- 默认 Web capture 为 N<=2000,D<=8,但 parser 接受任意 u16/u8 正值。
- 函数实际只需要“最近的 actionable ancestor”,不需要完整 ancestor Vec。传递 `Option<&AxElement>` 可把遍历降为 O(N*R),R 是通常很小的 roles 数量,并消除逐节点路径分配。
- 现有 `web_find_should_promote_text_child_to_actionable_ancestor` 和 deep refresh test 可锁定行为。

## 已证伪或不建议优化

- scanner 的 Swift 命中是两个并列 `map`,不是嵌套循环。
- scanner 的 Python 命中是在 120 次 readiness poll 内检查两个固定字符串,不是随数据增长的 membership 集合。
- screenshot 合成是 O(total pixels),属于输出规模下界;没有发现重复整图算法。
- flow 最多 256 步,artifact/request-id 的 Vec membership 不值得新增 set。
- macOS AX window 与 visible-window 的 O(A*V) 内存扫描存在,但 app/window 数小,且已测的外部进程 N+1 更昂贵。先修 N+1,之后只有 profile 仍指向该扫描才考虑 pid 分组。
- AX/Web 大小写 contains 会重复 lowercase needle,属于常数项分配。只有修完前四项后 profile 仍显示字符串匹配热点才值得处理。

## [2026-07-30 02:04:41] [Session ID: 019faedb-9cfc-7321-ac54-73789a40b8d8] 笔记: 复杂度优化实施证据

### macOS window discovery

- `bundle_id` 已并入 `list_running_apps()` 和 `running_app_for_pid()` 的既有 JXA payload。
- 删除逐 pid `bundle_id_for_pid()` helper,window list 从 1+A 次 `osascript` 降为 1 次。
- 新实现 3 轮:1163.8/882.5/839.8 ms,中位 882.5 ms;审计基线中位 1661.4 ms,降低约 46.9%。
- 当前 22 个 app 与逐 pid查询相比 bundle-id mismatch=0。

### durable observation

- `open()` 与每次 `record_observation()` 后都会检查 observations/selectors/ref_cache/index 的总 byte 数。
- 超限时只保留 index 仍引用的记录,优先淘汰 retained observations 之外最老 stable selector,仍超限才淘汰最老 observation。
- index 先原子替换,旧 JSONL 此时仍是新 index 的超集;随后 3 个 JSONL 各自 temp+rename。
- duplicate selector key 使用计数表而非 HashSet,compaction 不改变 retained 记录基数。
- ref-cache selector 关联使用首个匹配 HashMap,从 O(R*S) 降为 O(R+S)。selector/ref-cache 每 observation 各一次 open/flush。
- 2000 pairs、byte cap、重开、corrupt-index replay、duplicate first-match/compaction、latest stable record 均有测试。

### Web traversal

- `collect_web_matches()` 现在只传 `Option<&AxElement>` 的最近 actionable ancestor。
- 删除每节点 `ancestors.to_vec()` 和反向 ancestor scan。当前 element 仍优先,现有 promotion/deep refresh tests 通过。

### 验证边界

- 全 bin nextest:649 passed,1 skipped。
- `computer_use_density`:3 passed。
- `control_lanes`:15 passed,1 skipped。
- live `control_window_e2e` / `control_display_aware_e2e` / `control_ax_e2e`:15 tests 全由环境 gate 跳过,nextest 返回 no-tests code 4;不能声称 live E2E 通过。
- all-target cargo check:0 errors,11 个既有 dirty-worktree warnings,没有来自本次文件的新 warning。
- scoped rustfmt check、git diff check和删除模式检查均通过。
