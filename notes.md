# 架构摩擦点探索笔记

## [2026-08-20 12:30:00] 初步探索发现

### 代码库规模
- 137 个 Rust 源文件
- 最大文件: control_ax.rs (122K), control_window.rs (76K), control_flow.rs (64K)
- control_computer_act 模块总计约 5000 行代码

### 热点模块初步观察

#### 1. control_ax.rs (122K) - AX 查询与操作
**表面职责**:
- AX tree capture/query
- AX action execution (press/scroll/focus/set_value)
- 缓存 AX snapshot (observation-scoped)

**观察到的边界**:
- 既有 tree capture (`capture_ax_find_snapshot`, `capture_current_ax_window_snapshot`)
- 又有 action execution (`perform_default_ax_press`, `perform_default_ax_action`)
- 还有 cache 管理 (`AxObservationCache`, `ax_observation_cache()`)
- 还有 parser (`parse_ax_tree_payload`, `parse_ax_press_payload`)

**初步问题**:
这是一个"胖模块"还是"深模块"? 需要看 interface complexity vs implementation complexity。

#### 2. control_observation.rs (53K) - Observation 生命周期
**表面职责**:
- 内存 ObservationStore (TTL-based, LRU eviction)
- Durable observation store (JSONL 落盘)
- Ref registry (observation-local ref ↔ backend id)
- Resource epoch snapshot (capture-start 版本)

**观察到的边界**:
- ObservationStore (内存) + JsonlDurableObservationStore (磁盘) 是两套真相源
- resource epoch 的 **capture-start snapshot** 存在 ObservationStore
- 但 resource epoch 的 **全局可变版本** 在 control_resource_lane.rs

**初步问题**:
- ObservationStore 保存 `resource_epochs: HashMap<String, ResourceEpochSnapshot>` (line 99)
- control_resource_lane.rs 保存 `epochs: Mutex<HashMap<String, u64>>` (全局版本)
- 为什么 epoch 真相源分散在两个模块?

#### 3. control_resource_lane.rs (短文件) - Resource 串行化
**表面职责**:
- 同 PID 资源的 mutation 串行执行
- dispatch 前/后双递增 write epoch (奇数=进行中, 偶数=完成)
- stale epoch 拒绝

**观察到的设计**:
```rust
struct ResourceCoordinator {
    lanes: Mutex<HashMap<String, Arc<Mutex<()>>>>,  // per-resource lock
    epochs: Mutex<HashMap<String, u64>>,            // 全局版本
}
```

**初步问题**:
- `epochs` 是单一真相源，但为什么 ObservationStore 也要保存 `resource_epochs`?
- ObservationStore 的 epoch 是 **capture-start 时的快照**，用于后续 stale 校验
- 这个设计看起来合理，但跨模块的 epoch 语义需要理解成本

#### 4. control_computer_act/mod.rs (44K) - Meta-command dispatcher
**表面职责**:
- 13 个 Mano-CUA action 路由到底层 primitive
- implicit_observe (ticket 11)
- verify 三档 (ticket 12-14)
- error envelope E2 (ticket 15)
- timeout/density/trace (ticket 16-18)

**观察到的结构**:
```
mod.rs (44K)
  ├─ implicit_observe.rs (26K)
  ├─ verify.rs (32K)
  ├─ error_envelope.rs (19K)
  ├─ timeout.rs (8K)
  ├─ density.rs (7K)
  ├─ trace.rs (11K)
  └─ outcome.rs (8K)
```

**初步问题**:
- mod.rs 仍然 44K，即使拆出了 7 个子模块
- 这是"真的复杂"还是"还可以进一步拆分"?

#### 5. ax_diff/ 模块 - AX snapshot diff
**表面职责**:
- 规范化 AX snapshot (移除 observation/ref/ax_path 噪音)
- 结构化 diff (window + element 两阶段配对)
- changes_first.rs - trusted changes decision (Phase Pi 借鉴)

**观察到的设计**:
- `compute_diff()` 在 diff.rs
- `trusted_changes_decision()` 在 changes_first.rs
- 两者都依赖 normalize.rs 的规范化

**初步问题**:
- changes_first 是 "diff 的变体" 还是 "diff 的消费者"?
- 如果是变体，为什么不是 diff.rs 的一个函数?
- 如果是消费者，为什么在同一个模块?

### 下一步探索方向
1. **deletion test**: 如果删除 AxObservationCache，复杂度去哪了?
2. **测试覆盖**: control_ax.rs 的 action execution 是否容易测试?
3. **模块边界**: observation epoch 分散在 ObservationStore 和 ResourceCoordinator，是必然还是泄漏?

## [2026-08-20 12:45:00] 深入分析：识别到的摩擦点

### 摩擦点 1: control_ax.rs - 浅模块警告
**位置**: `src/control_ax.rs` (122K, 53 个公开函数)

**摩擦类型**: 浅模块 - interface 复杂度接近 implementation 复杂度

**具体问题**:
1. **过多的公开 parse 函数** (17+):
   - `parse_ax_tree_payload()`
   - `parse_ax_press_payload()`
   - `parse_ax_press_sequence_payload()`
   - `parse_ax_action_payload()`
   - `parse_ax_set_value_payload()`
   - `parse_ax_focus_payload()`
   - `parse_ax_scroll_payload()`
   - `parse_type_text_payload()`
   - ...

2. **过多的公开 perform 函数** (10+):
   - `perform_default_ax_press()`
   - `perform_default_ax_press_with_postcondition()`
   - `perform_default_ax_press_sequence()`
   - `perform_default_ax_action()`
   - `perform_default_ax_set_value()`
   - `perform_default_ax_focus()`
   - `perform_default_ax_scroll()`
   - `perform_default_key_delivery()`
   - `perform_default_type_text()`
   - ...

3. **过多的公开 capture 函数** (5+):
   - `capture_ax_find_snapshot()`
   - `capture_current_ax_subtree()`
   - `capture_current_ax_window_snapshot()`
   - `capture_default_ax_snapshot()`
   - `capture_semantic_target_snapshot()`

4. **过多的公开 resolve 函数** (3+):
   - `resolve_cached_ax_tree()`
   - `resolve_cached_ax_get()`
   - `resolve_current_ax_target_rect()`

**影响**:
- 调用方需要知道 "哪个 parse 对应哪个 perform"
- 调用方需要知道 "什么时候用 capture_default 还是 capture_current_window"
- 调用方需要知道 "什么时候用 cached 还是直接 capture"
- 53 个公开函数意味着理解成本高，测试矩阵大

**深化机会**:
将 control_ax 拆分为 3 个独立模块：
1. **ax_query** - 只负责 AX tree capture/query/cache (10 functions)
2. **ax_action** - 只负责 AX action execution (10 functions)  
3. **ax_input** - 只负责 keyboard/type input (5 functions)

或者更激进：提供统一入口
```rust
pub fn execute_ax_command(command: AxCommand) -> io::Result<AxResult>
```
内部根据 `AxCommand` 枚举分发，而不是暴露 53 个独立函数。

### 摩擦点 2: Observation Epoch 的双重真相源
**位置**: 
- `src/control_observation.rs::ObservationStore` (line 99)
- `src/control_resource_lane.rs::ResourceCoordinator` (line 45)

**摩擦类型**: 紧耦合泄漏 - 职责边界不清晰

**具体问题**:
观察到两套 epoch 存储：

**ResourceCoordinator** (全局可变版本):
```rust
struct ResourceCoordinator {
    epochs: Mutex<HashMap<String, u64>>,  // 单一真相源
}
```

**ObservationStore** (capture-start 快照):
```rust
struct StoredObservation {
    resource_epochs: HashMap<String, ResourceEpochSnapshot>,  // 快照副本
}
```

还有第三个缓存层：
**AxObservationCache** (line 54 in control_ax.rs):
```rust
struct AxObservationCacheEntry {
    resource_epochs: HashMap<String, u64>,  // 又一个快照副本
    snapshot: AxSnapshot,
}
```

**为什么这是摩擦**:
1. 开发者需要理解 3 个地方的 epoch 语义差异：
   - ResourceCoordinator: 全局当前版本
   - ObservationStore: observation 创建时的版本
   - AxObservationCache: AX snapshot 创建时的版本

2. 注释说 "observation store 才是资源 epoch 的单一真相源" (control_ax.rs line 75)
   但实际 ResourceCoordinator 才是真正的全局版本

3. 跨模块调用链：
   ```
   capture_resource_epochs()  // control_resource_lane
     → record_observation()   // control_observation
       → insert(snapshot)     // control_ax AxObservationCache
   ```

**影响**:
- 理解 observation 生命周期需要跨 3 个模块
- 新开发者容易混淆 "当前 epoch" vs "capture-start epoch"
- 测试需要同步 3 个地方的 epoch 状态

**深化机会**:
将 epoch 管理集中到 ResourceCoordinator，其他地方只保存 `EpochSnapshot { resource_key, epoch, captured_at }` 不可变快照。

或者：将 ObservationStore 的 `resource_epochs` 字段改为 `EpochCapture` 类型（opaque），不暴露内部 HashMap，强制通过 ResourceCoordinator API 查询。

### 摩擦点 3: 两套 Observation Cache (implicit vs explicit)
**位置**:
- `src/control_computer_act/implicit_observe.rs::ComputerActObservationCache`
- `src/control_ax.rs::AxObservationCache`

**摩擦类型**: 缺少 locality - 相似功能分散

**具体问题**:

**ComputerActObservationCache** (implicit_observe.rs):
- TTL: 5 秒
- 用途: `@computer-act` 的 implicit observe
- 存储: `observation_id → (ref_id, created_at_ms)`
- 容量: 64 条

**AxObservationCache** (control_ax.rs):
- TTL: 无明确 TTL (依赖 ObservationStore 的 300s?)
- 用途: `@ax-tree` / `@ax-get` 的缓存查询
- 存储: `observation_id → (snapshot, resource_epochs, content_hash)`
- 容量: 64 条

**为什么这是摩擦**:
1. 两套缓存都用 `observation_id` 作为 key
2. 两套缓存都有 64 条容量上限
3. 两套缓存都有 FIFO eviction
4. 但 TTL 不同（5s vs 300s）
5. 用途重叠：都是为了避免重复 capture

**影响**:
- 开发者需要知道 "什么时候用哪个 cache"
- `@computer-act` 用 implicit cache，`@ax-tree` 用 explicit cache
- 如果 implicit observe 生成的 observation_id 也想 cache AX snapshot，需要同时写两个 cache

**深化机会**:
统一为一个 `ObservationCache`，支持多种 TTL policy：
```rust
enum CachePolicy {
    ComputerAct { ttl_ms: 5000 },
    Progressive { ttl_ms: 300000 },
}

struct ObservationCache {
    entries: HashMap<String, CacheEntry>,
    policies: HashMap<String, CachePolicy>,
}
```

或者：让 ComputerActObservationCache 只管理 observation_id 的生命周期，AX snapshot 统一走 AxObservationCache。

### 摩擦点 4: control_computer_act/mod.rs - 膨胀的 dispatcher
**位置**: `src/control_computer_act/mod.rs` (44K)

**摩擦类型**: 浅模块 + 难以测试

**具体问题**:
尽管已经拆出 7 个子模块，mod.rs 仍有 44K 代码：
- `implicit_observe.rs` (26K)
- `verify.rs` (32K)  
- `error_envelope.rs` (19K)
- 其他子模块 (34K)
- 但 `mod.rs` 本身仍有 44K

mod.rs 包含：
1. 13 个 `route_*` 函数 (click/hover/type/hotkey/scroll/drag/wait...)
2. `parse_start_box`, `parse_ref_target`, `parse_text` 等解析逻辑
3. `execute_computer_act` 主入口
4. `apply_implicit_observe_to_args` 协调逻辑
5. timeout/density/trace/verification 的组装逻辑

**为什么这是摩擦**:
1. **routing 表散落在多个 `route_*` 函数中**，没有集中的数据结构
2. **测试困难**: 要测 `route_click` 需要构造完整的 `serde_json::Value` args
3. **职责混杂**: routing + parsing + execution + verification 都在一个文件

**影响**:
- 新增 action 需要理解整个 44K 文件的结构
- routing 逻辑分散，无法一眼看出 "支持哪 13 个 action"
- 单元测试只有 `tests.rs` (29K)，主要测 error envelope，routing 依赖集成测试

**深化机会**:
1. 将 routing 表数据化：
```rust
struct ActionRoute {
    action: &'static str,
    parser: fn(&Value) -> Result<ControlCommand, RouteError>,
    timeout_ms: u64,
}

const ROUTES: &[ActionRoute] = &[
    ActionRoute { action: "click", parser: parse_click, timeout_ms: 5000 },
    ActionRoute { action: "type", parser: parse_type, timeout_ms: 10000 },
    // ...
];
```

2. 将 execution 逻辑移到 `execute.rs`，mod.rs 只保留 routing 表

### 总结：四类摩擦的共同模式

**共同特征**:
1. **接口爆炸**: control_ax 53 函数，control_computer_act 44K routing
2. **状态分散**: epoch 在 3 个地方，cache 在 2 个地方
3. **测试困难**: 需要构造复杂 JSON/跨多个模块才能测试单个功能
4. **理解成本高**: 新开发者需要跨 3-4 个模块才能理解一个完整流程

**不是摩擦的地方**:
- `control_resource_lane.rs` 设计清晰：单一职责，接口简洁
- `ax_diff/` 模块职责明确：只做 diff，不混入 observation 管理
- ADR 文档完善，领域术语清晰（CONTEXT.md）

**推荐深化方向**:
1. **优先级 1**: 拆分 control_ax.rs 为 3 个模块（query/action/input）
2. **优先级 2**: 统一 observation cache 策略，消除 implicit vs explicit 双轨
3. **优先级 3**: 将 control_computer_act routing 表数据化
4. **优先级 4**: 明确 epoch 职责边界，减少跨模块理解成本
## [2026-08-20 16:24:00] [Session ID: omx-1787115582924-n1rbi7] 笔记: #54 successor changes 接入证据

### 现象
- `execute_computer_act` 已有唯一 pre/successor snapshot 路径,successor 同时服务 postcondition、verification 和 successor response。
- pre snapshot 只在 `verify != none` 时采集,且没有 observation header。
- `build_successor_target` 当前把 `ObservationHeader.created_at_unix_ms` 写入 `epoch`。

### 已验证结论
- #53 的 identity gate 要求 before/after 都包含 `rdog.ax.v1` schema、observation id、完整 capture 和 granted permission。现有 pre snapshot 直接进入 gate 会被判为 `missing_identity_metadata`。
- 下一次 mutation 的并发保护读取 `resolve_observation_resource_epoch(observation_id, ref_id)`,因此 successor target 必须返回该值,不能返回 observation 创建时间。
- 统一 output budget 已在 `ControlExecutionOutcome::from_response_line` 的 `control_frames::bound_response_line` 路径执行。本轮扩展 structured payload 即可,无需新增预算实现。

### 最小实现
- 让现有 pre capture 同样调用 `with_observation`,不增加第二次 capture。
- 在 `changes_first` 复用现有 `ChangesSummary`,只补 unavailable constructor 和明确 fallback reason。
- 提取一个很小的 successor response 装配 seam,让 executor 与 contract test 共用,不引入 trait、factory 或新依赖。

## [2026-08-20 18:05:00] [Session ID: omx-1787115582924-n1rbi7] 笔记: #55 对抗式验证覆盖

### 静态与动态覆盖

- resource lane: capture-during-dispatch stale、failed dispatch invalidation、same-PID single writer、different-PID parallel 已有动态测试。
- cached AX: cache hit 不走 live capture、wrong epoch、unknown observation、missing ref、expired observation 已在 executor/helper 层覆盖。
- changes-first: stable 100%、exact 75%、74% fallback、duplicate id、root replacement、window/resource drift、permission、unsupported、truncation、unknown identity 已有 fixture。
- output: exact/over byte boundary、exact/over line boundary、multibyte UTF-8 structured preview、oversized response/error path 已覆盖。

### 本轮修复

- resolve_cached_ax_get 以前直接透传 observation store 的 STALE_REF / OBSERVATION_EXPIRED,上层无法依赖统一 reason code。
- 现在 STALE_REF -> target_not_found,其余 observation 生命周期错误 -> stale_observation_cache;生产查询入口保持单一归一化路径。

### Canary blocker

- 评测 runner unit tests 通过,但真实 runner 在校验配置时失败: /Users/cuiluming/Library/pnpm/pi 不存在。
- .envrc 经 direnv exec . 可提供 DashScope key,所以凭据不是已验证 blocker;缺失的是 upstream Pi binary。
- 历史 successor policy 2-case x 5-model artifact 仅作背景,不能宣称本轮 current-binary canary。
