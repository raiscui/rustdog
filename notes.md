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

## [2026-08-21 13:55:00] [Session ID: current] 阶段 2 探索: ax_action 模块分析

### 当前结构分析

#### control_ax.rs 文件信息
- **总行数**: 3634 行
- **大小**: 122KB（从架构报告）
- **核心 action 函数**: 5 个 perform + 6 个 parse

#### 核心 Action 函数

**Perform 函数** (执行层):
1. `perform_default_ax_press` (line 1093)
2. `perform_default_ax_press_with_postcondition` (line 1133)
3. `perform_default_ax_press_sequence` (line 1324)
4. `perform_default_ax_action` (line 1443)
5. `perform_default_ax_set_value` (line 1447)
6. `perform_default_ax_focus` (line 1451)
7. `perform_default_ax_scroll` (line 1455)

**Parse 函数** (协议层):
1. `parse_ax_press_payload` (line 1641)
2. `parse_ax_press_sequence_payload` (line 1738)
3. `parse_ax_action_payload` (line 1793)
4. `parse_ax_set_value_payload` (line 1833)
5. `parse_ax_focus_payload` (line 1881)
6. `parse_ax_scroll_payload` (line 1938)

#### 调用方分析

**主要调用方** (2 个文件):
1. `src/control_actions.rs` - 10+ 处调用
2. `src/control_web/act.rs` - 1 处调用

#### 现有子模块

```
src/control_ax/
├── input.rs      3.1K  (已迁移到 ax_input)
├── macos.rs      67KB  (平台实现，需要移动)
├── query.rs      45.6KB (AX tree query，阶段 3)
├── tree.rs       15.1KB (AX tree 结构)
└── types.rs      14.1KB (类型定义)
```

### 拆分策略

#### 目标结构

```
src/ax_action/
├── mod.rs           # 统一入口 + 数据化 routing 表
├── protocol.rs      # Parse 层 (6 个 parse 函数)
├── execute.rs       # Execution 层 (7 个 perform 函数)
├── types.rs         # Re-export from control_ax::types
├── platform/
│   └── macos.rs     # 移动自 control_ax/macos.rs
└── tests.rs         # 单元测试
```

#### 关键设计决策

**1. Routing 表数据化** (解决摩擦点 #4)
```rust
struct ActionRoute {
    name: &'static str,
    parser: fn(Value) -> io::Result<ActionRequest>,
    executor: fn(ActionRequest) -> io::Result<ActionResult>,
    timeout_ms: u64,
}

const ROUTES: &[ActionRoute] = &[
    ActionRoute { name: "press", parser: parse_press, executor: perform_press, timeout_ms: 5000 },
    ActionRoute { name: "action", parser: parse_action, executor: perform_action, timeout_ms: 5000 },
    // ... 一眼可见所有 action
];
```

**2. 统一入口函数**
```rust
pub fn execute_ax_action(action: &str, payload: Value) -> io::Result<ActionResult>
```

**3. Deprecated Facade**
```rust
#[deprecated(since = "0.9.0", note = "use ax_action::execute_ax_action")]
pub fn perform_default_ax_press(req: &AxPressRequest) -> io::Result<AxActionReport> {
    ax_action::execute_ax_action("press", serde_json::to_value(req)?)
}
```

### 下一步行动

- [ ] 步骤 2.1: 创建 `src/ax_action/` 目录结构
- [ ] 步骤 2.2: 实现 `mod.rs` (routing 表 + 统一入口)
- [ ] 步骤 2.3: 实现 `protocol.rs` (移动 6 个 parse 函数)
- [ ] 步骤 2.4: 实现 `execute.rs` (移动 7 个 perform 函数)
- [ ] 步骤 2.5: 考虑是否移动 `macos.rs` (67KB，可能延后到阶段 3)


### 第一轮决策（已确认）

**Q1 - 双 API 策略**: ✅
- 动态入口: `execute_ax_action(action: &str, payload: Value)` (RPC 用)
- 强类型函数: `execute_press(req: &AxPressRequest)` 等 (内部调用)
- Routing 表服务动态路径，内部调用强类型函数

**Q2 - postcondition 合并**: ✅
- `press_with_postcondition` 合并到 `press` action
- `AxPressRequest` 添加 `Option<Postcondition>` 字段
- Routing 表只需一个 `"press"` entry

**Q3 - press_sequence 独立**: ✅
- 暴露为独立 action `"press_sequence"`
- 保持原子性（全部成功或全部回滚）

**Q4 - macos.rs 暂不动**: ✅
- 保持在 `control_ax/macos.rs` 作为共享平台层
- 等阶段 3 (query 拆分) 后再统一评估

**Q5 - types 不 re-export**: ✅
- 不在 `ax_action` 创建 `types.rs`
- 所有类型保持在 `control_ax::types`
- 新模块直接 `use control_ax::types::*`

**Q6 - Facade 完整代理**: ✅
- Deprecated facade 必须完整代理旧签名
- 负责参数转换（如果新旧 API 参数结构不同）

**Q7 - 分阶段迁移**: ✅
- 立即迁移 `control_actions.rs` 到强类型 API
- 保留 `control_web/act.rs` 用 facade（RPC 边界）


### 第二轮决策（已确认）

**Q8 - Postcondition 字段向后兼容**: ✅
- 修改 `AxPressRequest`，添加 `#[serde(default, skip_serializing_if = "Option::is_none")]`
- 旧 JSON payload 仍能解析（字段缺失 = None）
- 新 payload 可包含 postcondition

**Q9 - 特定错误类型**: ✅
- 返回 `ActionNotFound(String)` 错误
- 映射到 `io::ErrorKind::NotFound`
- RPC 层根据 ErrorKind 决定 HTTP 状态码

**Q10 - Routing 表统一签名**: ✅
- 统一签名: `fn(&Value) -> io::Result<Value>`
- Routing 表是 `const` 纯数据结构
- 内部: 反序列化 → 强类型函数 → 序列化

**Q11 - 简化命名**: ✅
- 新模块: `press()`, `action()`, `set_value()`, `focus()`, `scroll()`, `press_sequence()`
- 旧函数: `perform_default_*` 作为 deprecated facade

**Q12 - 分层边界**: ✅
- `protocol.rs`: 纯反序列化（JSON → struct），serde 自动校验必填字段
- `execute.rs`: 业务逻辑校验 + 执行，自包含

**Q13 - 分层测试**: ✅
- (1) Routing 表测试（action 名 → handler）
- (2) 强类型函数单元测试（mock 平台）
- 旧测试暂不迁移，先覆盖新 API

**Q14 - 增量迁移**: ✅
- 先迁移 `press`，验证通过后批量迁移其他
- Import: `use ax_action::{press, action, ...}` (显式导入)


### 第三轮决策（已确认）

**Q15 - 增量实施**: ✅
- 先完成 `press` action 端到端（parse + execute + routing + API + test）
- 验证通过后批量添加其他 6 个 action
- 最快发现设计问题

**Q16 - 循环依赖处理**: ✅
- Facade 调用 `ax_action::execute_ax_action` 字符串 API
- 避免导入所有强类型函数
- 模块级 `pub use` 是安全的

**Q17 - 兼容性测试**: ✅
- 添加 `test_parse_ax_press_backward_compatible()`
- 用旧格式 JSON（无 `postcondition`）测试
- 验证能解析且字段为 `None`

**Q18 - Timeout 作为元数据**: ✅
- `timeout_ms` 暂时只作为元数据
- 不自动应用超时（保持 sync）
- 调用方自己决定是否超时

**Q19 - 测试策略**: ✅
- 单元测试：只测 protocol 层（parse 逻辑）
- Execute 层依赖集成测试（真实 AX 环境）
- 必要时用 `#[cfg(test)]` fake 实现

**Q20 - 三层验收**: ✅
1. 单元测试：新模块测试全部通过
2. 集成测试：`cargo test` 全量测试（883 个）
3. 冒烟测试：手动验证 press/type/scroll

**Q21 - Deprecated 删除时间线**: ✅
- 阶段 2 完成后迁移 `control_actions.rs`
- 阶段 3 完成后迁移 `control_web/act.rs`
- 0.10.0 或 0.11.0 删除所有 deprecated 函数


## 设计共识完成 ✅

经过 3 轮 21 个问题的 grilling，我们达成了完整的设计树：

### 核心架构决策

**模块结构**:
```
src/ax_action/
├── mod.rs          # Routing 表 + 统一入口 execute_ax_action()
├── protocol.rs     # 6 个 parse 函数（纯反序列化）
├── execute.rs      # 6 个强类型函数: press(), action(), set_value()...
└── tests.rs        # 单元测试（protocol + routing）
```

**双 API 设计**:
- 动态 API: `execute_ax_action(action: &str, payload: Value)` (RPC 用)
- 强类型 API: `press(req: &AxPressRequest)` 等（内部调用）
- Routing 表: `const` 数据结构，统一签名 `fn(&Value) -> io::Result<Value>`

**关键合并**:
- `press_with_postcondition` 合并到 `press`
- `AxPressRequest` 添加 `postcondition: Option<Postcondition>` 字段
- `press_sequence` 保持独立（原子性）

**依赖关系**:
- `ax_action` → `control_ax::types` (类型定义)
- `ax_action` → `control_ax::macos` (平台实现)
- `control_ax.rs` → `ax_action` (deprecated facade)
- 不创建 `ax_action/types.rs`，避免路径混乱

**迁移路径**:
1. 增量实施：先完成 `press` action 端到端
2. 立即迁移 `control_actions.rs` 到强类型 API
3. 保留 `control_web/act.rs` 用 facade
4. 阶段 3 后删除 deprecated 函数

**测试策略**:
- 单元测试：protocol 层 + routing 表
- 集成测试：全量 `cargo test`
- 兼容性测试：旧 JSON 格式仍能解析
- 冒烟测试：手动验证核心 action

### Alternatives Considered

#### Why not 立即移动 macos.rs？
- 67KB 平台代码可能被 query 和 action 共同依赖
- 等阶段 3 拆分 query 后再统一评估
- 避免过早抽象

#### Why not 引入 PlatformAx trait？
- 增加复杂度，但单元测试收益不大
- Execute 层依赖集成测试更真实
- 必要时用 `#[cfg(test)]` fake 实现

#### Why not 自动应用 timeout？
- 需要 async runtime，增加复杂度
- 调用方已有自己的超时机制
- 暂作为元数据，未来可扩展

#### Why not 批量实施所有 action？
- 风险高，难以定位问题
- 增量式可最快验证设计
- 先完成 `press` 建立模板


## [2026-08-28 09:30:00] [Session ID: current] 笔记: 阶段 3 (ax_query) 现状研究

### 来源: 代码勘察 (control_ax.rs / tree.rs / query.rs / control_observation / scratch tickets 07-11)

### 关键发现 (ADR 设想 vs #51/#54/#55 之后的现实)

1. **ticket 08 (AxSnapshotCache 迁移) 大部分已被现实超越**:
   - epoch 单一真相源已落地: 真源在 observation store / control_resource_lane,
     AX_OBSERVATION_CACHE 只存 capture 时快照 + 读取时向 resource lane 校验
     (control_ax.rs:46-232, #54/#55 加固过, 有对抗性测试 7/7 + 4/4)
   - 现缓存无 TTL policy, 纯 epoch 校验; ADR 设想的 CachePolicy
     (ImplicitObserve 5s / Progressive 300s) 对应的是另一个独立缓存
     COMPUTER_ACT_OBSERVATION_CACHE (control_computer_act/implicit_observe.rs)
   - 迁移/重构缓存的剩余收益只是物理位置, 风险却是动 #55 刚验证过的路径
2. **query.rs 不是纯查询引擎**: 1267 行里是 @ax-find/@ax-get 的完整 verb 实现
   (compact/对象协议解析 + observation ref 解析 + display scope + screenshot 摘要)。
   原样搬入 ax_query 会让"无状态模块"目标当场破产
3. **tree.rs 是混合体**: ~200 行纯 capture/匹配 helper (零 observation 依赖)
   + selector draft 构造 (只被 control_ax.rs 的 AxSnapshot::with_observation 用)
   + target 解析 (direct_ax_target_id / resolve_target_id_in_snapshot 依赖 observation ref)
4. **循环依赖现状**: control_ax→control_observation (selector/header/ref/注册) 与
   observation→control_ax (capture 入口 + AX 类型) 双向仍在;
   ax_action→control_ax 单向 (阶段 2 成果)
5. **capture 函数的消费面极广**: capture_default_ax_snapshot 有 8 个模块消费
   (observation/screenshot/actions/web/computer_act), 全部从 122KB 的 control_ax 导入

### 调用方映射 (tree.rs 函数 -> 外部消费者)
- capture_default_ax_snapshot: control_observation(.rs+producer), screenshot, control_actions, control_web capture+act, computer_act/verify
- capture_current_ax_window_snapshot: ax_action, control_actions, control_web capture+act, computer_act/verify
- capture_ax_find_snapshot / capture_current_ax_subtree: control_actions / control_web
- current_ax_platform: screenshot, observation/producer
- materialize_app_window_target(+_with) / ax_snapshot_status_error: ax_action/execute
- resolve_current_ax_target_rect: query.rs, screenshot, control_mouse/target
- selector drafts (collect_element_refs 等): 仅 control_ax.rs 的 with_observation

### 综合结论
ADR 阶段 3 的真目标 (无状态查询核心 + 单向依赖 + 甩掉 verb 大杂烩) 仍成立,
但实现切分必须按现实重划: ax_query 只收零 observation 依赖的纯 capture/匹配核心,
query.rs (verb) 与 selector 富化留在 control_ax 侧。缓存不动。

## [2026-08-28 15:30:00] [Session ID: current] 笔记: PR #62/#63 CI 失败根因分析

### PR #62 ubuntu Build 修复链 (已验证)

- 15309e5: ci.yml 装 xcap linux deps → wayland-sys build.rs panic 消失 (Build 首次通过)
- 10d495e: E0425 import 修复 (platform_unsupported_envelope_json cfg 门控)
- 94be124: 补 libgbm-dev/libdrm-dev → -lgbm 链接错误消失, rustdog 本体首次在 ubuntu 编译通过
- 结论: ubuntu Build 层完全修通, 上游 xcap README Ubuntu 清单缺 libgbm-dev (Alpine 侧对应
  mesa-dev 才全), 已实测 /usr/bin/kill 参数歧义另见下文

### PR #62 ubuntu unit tests 4 个失败 (存量平台缺口, ubuntu 首次跑到 unit tests)

1. execute_open_app_emits_{ok,app_not_found,permission_denied}_* 3 个:
   - 现象: error_code 断言 left="platform_unsupported" right="app_not_found" 等
   - 机制: open_app_payload_for_current_platform 在非 macOS 直接返回
     platform_unsupported envelope, 完全绕过 mock 注入; 这 3 个测试断言的是
     run_open_app_on_macos 分支行为, 缺 #[cfg(target_os = "macos")] 门控
   - 修复: 测试加 cfg 门控 (macOS 行为测试); linux 侧 platform_unsupported
     envelope 可另补独立断言测试
2. shell_lane_should_mark_timeout_and_continue_to_expect:
   - 现象: duration_ms=2001, timed_out=true, exit_code=None
   - 已验证机制 (docker ubuntu 24.04 + python 复刻 process.rs):
     TOTAL duration=2.008s, stdout_join_wait=1.937s, rc=-9
     → 50ms 时 sh(dash) 被 child.kill() 杀, sleep 成孤儿持有 stdout 管道写端,
     join_stream_reader 阻塞到 sleep 2s 自然退出
   - 未解疑点: kill -TERM -<pgid> exit=0 但组内 sh/sleep 均存活 0.5s+,
     strace 追查中 (第一遍因 killpg 非法 syscall 名失败, 已修正重跑)
   - 修复方向: 进程组终止信号路径需要修正 (kill 参数歧义 或 改用进程内 syscall),
     另外管道 join 应有兜底超时

### PR #63 macos unit tests 抽签 flake 矩阵 (与 main 同轮比对, 全部存量)

| 测试 | main 03:27 | main 03:49 | #63 ab4aceb | #63 07576b9 | #63 rerun |
|------|-----------|-----------|-------------|-------------|-----------|
| screenshot::tests 4 个 | 过 | 挂 | 挂 | 过 | 挂 |
| status_remaining_ms_clamped | 挂 | 过 | 过 | 过 | 挂 |
| recording_manual_cancel | - | - | - | 挂(仅e2e步) | 过 |
| auto_stop_continues_owner_disconnect | - | - | - | 过 | 过(#62轮挂) |
| metadata_publish_failure | - | - | 挂 | 过 | 过(#62轮挂) |
| unixpipe stale_owner | - | - | 挂 | 过 | 过 |

- 结论: macos runner 每轮随机挂 1-5 个时序/资源类测试, main 与两个 PR 分支
  同样抽签, 与 Phase 1 改动无关
- PR #63 本地验证: recording_e2e 6 轮 30/30 全过 (worktree 07576b9)

### 合并顺序决定

#62 先 (CI 修通 + 4 个测试平台适配) → #63 后 (update branch 后 ubuntu 首次
可验证)。#63 的 control_actions.rs import 块与 #62 10d495e 同内容, 收敛无冲突。

## [2026-09-02 17:08:30] [Session ID: sess_68ea20f1-1318-4541-85bc-08d0ca1ddbd8] 笔记: overlay onscreen=false 假设链 (读码阶段, 未验证)

### 现象 (来自 #102 comment, 已验证事实)
- 面板在 CGWindowList 注册正确 (bounds 400x300, layer 3) 但 onscreen=false
- runMode_beforeDate 冲刷后 frame 从 0x0 -> 400x300 (说明 runloop 冲刷有效果, 但订购仍未上屏)
- finishLaunching / orderFrontRegardless / display() / wantsLayer+CGColor 均无效

### 候选假设 (按优先)
1. **CATransaction 提交路径缺失**: orderFront 的窗口合成事务靠 runloop before-waiting observer 提交; 空 runloop 上 runMode_beforeDate 立即返回, observer 可能未触发 -> 窗口停在"已注册未合成"状态。此假设与"注册正确但 onscreen=false"精确吻合。
   - 证伪实验: swift 对照 (同款结构不跑 app.run 只跑 RunLoop.main.run(before:)); 若 swift 同样不可见 => 架构问题, 正解是跑完整 app.run() + NSTimer poll channel; 若 swift 可见 => rust 绑定用法问题
2. **borderless NSPanel + Accessory + ignoresMouseEvents 组合被合成器拒绝** (无内容 => 无 backing store)
   - 证伪实验: 普通 NSWindow + 标题栏样式对照
3. 代码笔误: finishLaunching 连调两次 (control_overlay.rs:140-142), 待清理; 是否参与失败路径未知
### 反证检查
- 若假设1成立, 为何 runMode 冲刷让 frame 从 0x0 变正确? => frame 更新走 CGSSetWindowBounds (AppKit 直接调用) 与 CATransaction commit 是两条路径, 吻合
- 推翻条件: swift 对照可见 + rust 在同一 runloop 结构下不可见 => 假设1不成立
