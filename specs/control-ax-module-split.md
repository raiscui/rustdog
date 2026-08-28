# Spec: control_ax 模块拆分为 ax_query、ax_action、ax_input

> Companion to ADR-0008. This document is the **what**; ADR-0008 is the **why**. 
> Read this for build scope; read the ADR for design rationale.

> **As-built 增补 (2026-08-28 更新)**: 三个阶段已全部落地, 与本 spec 的分歧有
> ① `execute_ax_action` 统一字符串入口、数据化 routing 表与 `ax_action/protocol.rs`
> parse 层按实施结论移除 (零生产消费者, ADR-0008 Amendment);
> ② ax_query 按现实切分只收纳无状态捕获核心, query.rs 保留在 control_ax 作为
> verb 层, AxSnapshotCache 不迁移 (epoch 真相源分离已由 #51/#54/#55 落地,
> ADR-0008 Amendment 2)。
> 下文保留原始规划内容作为决策历史, 不代表当前架构。

## Problem Statement

control_ax.rs 是一个 122KB 的文件，包含 53+ 个公开函数，表现出典型的"浅模块"特征——接口复杂度几乎等于实现复杂度。调用方需要理解所有 53 个函数才能正确使用该模块。此外，该模块还存在以下问题：

1. **循环依赖**: control_ax ↔ control_observation 双向调用
2. **双重缓存**: AxObservationCache (control_ax) 和 ObservationStore (control_observation) 都持有 resource epoch 快照
3. **Truth Source 混乱**: resource epoch 的真相源在三个位置同时存在
4. **路由表缺失**: 13 个 action 的 parse 和 perform 函数分散，新增 action 需要修改多处

**Deletion Test 结果**: 如果删除 control_ax，复杂度不会集中而是分散到调用方——他们需要知道"哪个 parse 对应哪个 perform"、"何时使用 cached vs current"，认知负担反而增加。

**模块规模分解**:
- `control_ax.rs`: 3611 行 (主文件)
- `macos.rs`: 67KB (平台实现)
- `query.rs`: 45.6KB (查询逻辑)
- `tree.rs`: 15.1KB (树捕获)
- `types.rs`: 14.1KB (类型定义)
- `input.rs`: 2.8KB (输入处理)

总计: 4298 行

## Solution

将 control_ax 拆分为三个具有深接口的聚焦模块，每个模块提供 5-10 个高级函数，隐藏内部复杂性：

### 1. ax_query/ — AX 树捕获与查询 (~47KB)

**职责**: 捕获 AX 树、执行查询、提供快照。

**公开接口** (深接口，~5 个函数):
```rust
pub fn capture_window_snapshot(window_id: &str) -> io::Result<AxSnapshot>;
pub fn capture_tree(query: &AxQuery) -> io::Result<AxSnapshot>;
pub fn find_element(snapshot: &AxSnapshot, selector: &Selector) -> Option<AxElement>;
pub fn query_cached(observation_id: &str) -> Option<&AxSnapshot>;
```

**内部结构**:
- `mod.rs`: 公开接口 + 重导出
- `capture.rs`: 树捕获逻辑 (来自 tree.rs)
- `query.rs`: 查询执行 (45.6KB, 保持不变)
- `types.rs`: 查询专用类型

**关键设计**: 无状态模块。不拥有 cache；cache 逻辑移至 control_observation。

### 2. ax_action/ — AX 动作执行 (~70KB)

**职责**: 解析 action payloads、通过平台 API 执行动作、管理路由。

**公开接口** (深接口，统一入口):
```rust
pub fn execute_ax_action(action: &str, payload: Value) -> io::Result<ActionResult>;
```

**内部结构**:
- `mod.rs`: 统一入口 + 数据驱动的路由表
- `protocol.rs`: 7 个 parse_* 函数 (协议层)
- `execute.rs`: 7 个 perform_* 函数 (执行层)
- `platform/macos.rs`: 67KB macOS 实现 (保持不变)
- `types.rs`: Action 专用类型

**路由表** (数据驱动，解决 Friction #4):
```rust
struct ActionRoute {
    name: &'static str,
    parser: fn(Value) -> io::Result<AnyActionRequest>,
    executor: fn(AnyActionRequest) -> io::Result<ActionResult>,
}

const ROUTES: &[ActionRoute] = &[
    ActionRoute { name: "press", parser: parse_press, executor: execute_press },
    ActionRoute { name: "click", parser: parse_click, executor: execute_click },
    ActionRoute { name: "type", parser: parse_type, executor: execute_type },
    ActionRoute { name: "scroll", parser: parse_scroll, executor: execute_scroll },
    ActionRoute { name: "drag", parser: parse_drag, executor: execute_drag },
    ActionRoute { name: "hover", parser: parse_hover, executor: execute_hover },
    ActionRoute { name: "set_value", parser: parse_set_value, executor: execute_set_value },
    // ... 13 actions 一目了然
];
```

**TODO**: 当多平台支持需要时，将 `platform/` 提取为独立的 ax_platform 模块。目前仅实现 macOS (YAGNI)。

### 3. ax_input/ — 文本与键盘输入 (~3KB)

**职责**: 高级文本输入和按键传递。

**公开接口** (分层，80/20 分离):
```rust
// 简单 API (80% 用例)
pub fn type_text(content: &str, mode: TypeMode) -> io::Result<TypeReport>;
pub fn send_key(key: Key, modifiers: &[Modifier]) -> io::Result<KeyReport>;

// 高级 API (20% 用例)
pub fn type_text_with_config(request: TypeTextRequest) -> io::Result<TypeReport>;
pub fn send_key_with_config(request: KeyRequest) -> io::Result<KeyReport>;
```

**内部结构**:
- `mod.rs`: 高级接口 (隐藏 Request 类型)
- `input.rs`: 2.8KB 实现 (保持不变)
- `types.rs`: 输入专用类型

**关键设计**: 简单 API 通过合理的默认值隐藏 `TypeTextRequest` 的复杂性 (delivery, target_window, verification)。高级 API 暴露完全控制用于特殊场景。

### 4. 打破循环依赖

**问题**: control_ax 调用 control_observation (4 个函数)，control_observation 调用 control_ax (capture 函数)。

**解决方案**: 在 control_observation 中引入 `ObservationCapture` adapter。

```rust
// 在 control_observation 中
pub struct ObservationCapture {
    // 内部: 包装 ax_query 调用
}

impl ObservationCapture {
    pub fn capture_for_observation(
        &self, 
        window_id: &str
    ) -> io::Result<(AxSnapshot, Selectors)> {
        let snapshot = ax_query::capture_window_snapshot(window_id)?;
        let selectors = self.build_selectors(&snapshot);
        Ok((snapshot, selectors))
    }
}
```

**理由**:
- ObservationCapture 是一个**长期 seam**，而非临时 adapter
- 它封装了"为 observation 捕获"的语义 (返回 snapshot + selectors)
- 维护同步调用路径 (ADR-0005 对 implicit_observe 的要求)
- 已添加到 CONTEXT.md 作为领域概念

### 5. AX Snapshot Cache 迁移

**当前状态**: 定义在 control_ax.rs (内部 static singleton)

**新位置**: control_observation (由 ObservationStore 持有)

```rust
// 在 control_observation 中
pub struct ObservationStore {
    observations: HashMap<String, StoredObservation>,
    ax_snapshot_cache: AxSnapshotCache,  // 新增
}

pub struct AxSnapshotCache {
    entries: HashMap<String, AxSnapshotCacheEntry>,
    order: VecDeque<String>,
}

pub struct AxSnapshotCacheEntry {
    snapshot: AxSnapshot,
    epochs: HashMap<String, u64>,  // 不可变快照，非真相源
    policy: CachePolicy,            // 新增
}

pub enum CachePolicy {
    ImplicitObserve { ttl_ms: 5000 },    // ADR-0005 要求
    Progressive { ttl_ms: 300000 },
}
```

**理由**:
- 解决 Friction #2 (epoch 的三个真相源)
- ObservationStore 仍然是 resource epoch 的单一真相源
- AxSnapshotCache 是"加速层"，针对 ObservationStore 进行验证
- 解决 Friction #3 (双重缓存)，通过支持每个条目的多种 TTL policy

**调用方指定 Policy**:
```rust
observation_store.cache_ax_snapshot(
    observation_id,
    snapshot,
    CachePolicy::ImplicitObserve,  // 调用方决定
);
```

## User Stories

### 接口简化与深度
1. 作为 control_actions.rs 的维护者，我希望只需理解 5 个 ax_query 函数而非 53 个 control_ax 函数，这样我就能快速定位 AX 查询问题。
2. 作为新加入的开发者，我希望看到 `execute_ax_action` 这个统一入口，这样我就知道所有 action 都通过同一个路径执行，而不需要猜测"这个 action 用哪个函数"。
3. 作为 @computer-act 的实现者，我希望 ax_input 提供 `type_text(content, mode)` 这样的简单接口，这样 80% 的场景我不需要构造 `TypeTextRequest`。
4. 作为 ax_action 的维护者，我希望新增一个 action 只需要在 ROUTES 表中增加一行，而不是在 parse、perform、dispatch 三处分别添加代码。
5. 作为 control_observation 的维护者，我希望 ax_query 不持有任何 cache，这样我就能在 ObservationStore 中集中管理所有 epoch 和 TTL 逻辑。

### 循环依赖解决
6. 作为测试工程师，我希望能够 mock ObservationCapture 来独立测试 ax_query，而不需要启动完整的 ObservationStore。
7. 作为 control_observation 的维护者，我希望通过 ObservationCapture 调用 ax_query，这样依赖方向始终是 observation → query，而非循环依赖。
8. 作为架构审查者，我希望 ObservationCapture 是一个显式的、有文档的长期 seam，这样团队成员理解它是设计的一部分，而非技术债。

### Cache 与 Epoch 管理
9. 作为 @computer-act 的实现者，我希望 implicit_observe 能复用 5 秒内的 snapshot (ADR-0005 要求)，这样高频 action 序列不会重复捕获 AX 树。
10. 作为 Progressive Query 的实现者，我希望能为 snapshot 指定 300 秒 TTL，这样长时间的渐进式查询可以复用同一个 snapshot。
11. 作为 ObservationStore 的维护者，我希望 AxSnapshotCache 验证 epoch 时只读取 ObservationStore，这样 resource epoch 的真相源始终唯一。
12. 作为调试工程师，我希望 cache hit/miss 日志清楚地显示 policy (ImplicitObserve vs Progressive)，这样我能判断是否使用了正确的 TTL。
13. 作为测试工程师，我希望能独立测试 cache 验证逻辑，验证"stale epoch 导致 cache miss"这一关键路径。

### 路由表与可维护性
14. 作为 ax_action 的维护者，我希望看到一个包含所有 13 个 action 的 ROUTES 表，这样我能一目了然地检查是否遗漏了某个 action。
15. 作为代码审查者，我希望新增 action 的 PR 只修改 ROUTES 表和对应的 parse/execute 函数，这样我能快速验证改动范围。
16. 作为性能分析者，我希望 execute_ax_action 在日志中记录路由表查找时间和执行时间，这样我能识别瓶颈。

### 测试与验证
17. 作为测试工程师，我希望能为 parse_* 函数编写纯函数单元测试，这样我就不需要 mock 平台 API 来验证协议解析逻辑。
18. 作为集成测试维护者，我希望 execute_ax_action 的路由表测试能覆盖所有 13 个 action，这样我能确保没有遗漏的路由。
19. 作为 CI 维护者，我希望 ax_query 的测试不依赖 control_observation，这样单元测试和集成测试可以并行运行。

### 迁移与兼容性
20. 作为内部调用方迁移的执行者，我希望看到清晰的迁移顺序 (ax_input → ax_action → ax_query)，这样我能逐步完成迁移而不影响生产环境。
21. 作为 @computer-act 的维护者，我希望外部 API (@computer-act, @ax-tree 命令) 保持不变，这样客户端无需修改。
22. 作为 Facade 维护者，我希望旧的 control_ax 函数立即标记为 `#[deprecated]`，这样新代码不会继续使用旧接口。

## Implementation Decisions

(所有决策详细记录在 ADR-0008 中。以下为摘要。)

### 模块边界与接口设计

**决策 1**: 三个模块而非四个 (ax_platform 延迟)。
- **理由**: YAGNI。当前仅支持 macOS，提前抽象 platform 层没有价值。macos.rs 保留在 ax_action/platform/ 并添加 TODO 注释。

**决策 2**: ax_query 采用无状态设计，不持有 cache。
- **理由**: cache 是"谁需要加速就由谁持有"的逻辑。ObservationStore 需要 cache，因此它持有；ax_query 只是能力提供方，不应管理缓存生命周期。

**决策 3**: ax_action 提供统一入口 `execute_ax_action`，而非多个独立的 perform_* 函数。
- **理由**: 解决 Friction #4。统一入口使得调用方只需知道"action 字符串 + payload"，路由表内部处理 parse 和 execute 的映射。

**决策 4**: ax_input 分层设计——简单 API (type_text) + 高级 API (type_text_with_config)。
- **理由**: 80/20 原则。大部分场景使用默认配置，少数场景需要显式控制 delivery、target_window、verification。隐藏 Request 类型降低学习曲线。

### 循环依赖解决

**决策 5**: ObservationCapture 是长期 seam，而非临时 adapter。
- **理由**: 它封装了"为 observation 捕获"这一领域语义 (返回 snapshot + selectors)，不仅是技术适配。添加到 CONTEXT.md 确立其领域地位。

**决策 6**: ObservationCapture 维护同步调用路径。
- **理由**: ADR-0005 要求 implicit_observe 在 5 秒内复用 snapshot，必须是同步路径。引入异步会破坏该契约。

**决策 7**: ObservationCapture 调用 ax_query 的 capture 函数，而非反向。
- **理由**: 依赖方向统一为 observation → query，消除循环。query 层不知道 observation 的存在。

### Cache 与 Epoch 管理

**决策 8**: AxSnapshotCache 从 control_ax 移至 control_observation 的 ObservationStore。
- **理由**: 解决 Friction #2 (epoch 的三个真相源)。ObservationStore 是 resource epoch 的单一真相源，cache 只是加速层。

**决策 9**: AxSnapshotCache 支持多种 TTL policy (ImplicitObserve 5s, Progressive 300s)。
- **理由**: 解决 Friction #3 (双重缓存)。不同场景需要不同 TTL，通过 CachePolicy 枚举统一管理。

**决策 10**: Cache policy 由调用方指定，而非由 cache 自动推断。
- **理由**: 显式优于隐式。调用方知道自己是 implicit_observe (5s) 还是 progressive query (300s)，cache 只负责存储和验证。

**决策 11**: AxSnapshotCacheEntry 存储 epoch 快照，但不是真相源。
- **理由**: Cache 条目是不可变的。验证时读取 ObservationStore 的当前 epoch，mismatch 则 invalidate。

### 路由表设计

**决策 12**: 数据驱动的路由表，而非 match 语句。
- **理由**: 解决 Friction #4。新增 action 只需在 ROUTES 表中添加一行，所有 action 一目了然，易于维护和审查。

**决策 13**: 路由表包含 name、parser、executor 三个字段。
- **理由**: 协议层 (parse) 和执行层 (perform) 职责分离。路由表是它们之间的桥梁。

**决策 14**: 使用函数指针而非 trait object。
- **理由**: 零成本抽象。编译时已知所有 action，无需动态派发。

### 测试策略

**决策 15**: 关键路径优先——cache 验证逻辑是 Phase 3 的 Priority #1。
- **理由**: Q13-C 确认。Cache 是 observation 和 query 之间的唯一接口，验证逻辑错误会导致 stale ref 被复用，这是高风险路径。

**决策 16**: 测试驱动拆分——写测试 → 移代码 → 验证测试通过。
- **理由**: 确保新旧路径都通过相同测试，避免回归。只在迁移完成后删除旧测试。

**决策 17**: Parse 函数测试优先——易赢，纯函数，无需 mock。
- **理由**: 快速建立信心。parse_* 函数只做 JSON → struct 转换，单元测试覆盖率高且执行快。

**决策 18**: Perform 函数测试最后——需要平台 mock，复杂度高。
- **理由**: 平台 API (macOS AX) 难以 mock。优先验证 parse 和 routing，perform 通过集成测试覆盖。

### 迁移计划

**决策 19**: 增量式迁移——ax_input (最小) → ax_action (最大) → ax_query (最复杂)。
- **理由**: 
  - ax_input 最小 (2.8KB)，作为流程模板。
  - ax_action 最大但逻辑独立，路由表是核心。
  - ax_query 最复杂 (涉及 cache 迁移和循环依赖)，最后处理。

**决策 20**: 只迁移内部调用方 (control_actions, control_computer_act, control_flow)。
- **理由**: 外部 API (@computer-act, @ax-tree 命令) 保持不变，客户端无感知。内部调用方数量有限 (3 个)，可控。

**决策 21**: Facade 立即标记 `#[deprecated]`，迁移完成后删除。
- **理由**: 防止新代码继续使用旧接口。Facade 不是长期方案，仅在过渡期保留。

## Testing Decisions

### 测试 Seam (3 个关键接口)

#### 1. ObservationCapture (observation ↔ query 唯一接口)

**测试目标**:
- 验证 `capture_for_observation` 返回 (snapshot, selectors)
- 验证调用路径是同步的 (无 async)
- 验证 selector 生成逻辑 (基于 snapshot)

**测试策略**:
```rust
#[test]
fn test_observation_capture_returns_snapshot_and_selectors() {
    let capture = ObservationCapture::new();
    let result = capture.capture_for_observation("window_123");
    assert!(result.is_ok());
    let (snapshot, selectors) = result.unwrap();
    assert!(!snapshot.elements.is_empty());
    assert!(!selectors.is_empty());
}

#[test]
fn test_observation_capture_is_synchronous() {
    // 确保 capture_for_observation 不引入 async
    let capture = ObservationCapture::new();
    let start = Instant::now();
    let _ = capture.capture_for_observation("window_123");
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(1)); // 同步调用应快速完成
}
```

**Mock 策略**:
- 在 control_observation 的测试中，mock ax_query::capture_window_snapshot
- 在 ax_query 的测试中，不依赖 ObservationCapture (无状态模块)

#### 2. execute_ax_action (action 执行唯一入口)

**测试目标**:
- 验证所有 13 个 action 都有路由条目
- 验证路由表查找正确 (action 字符串 → parser/executor)
- 验证未知 action 返回错误

**测试策略**:
```rust
#[test]
fn test_all_actions_have_routes() {
    let expected_actions = vec![
        "press", "click", "type", "scroll", "drag", 
        "hover", "set_value", "mouse_move", "wheel", 
        "double_click", "right_click", "middle_click", "action"
    ];
    for action in expected_actions {
        let route = ROUTES.iter().find(|r| r.name == action);
        assert!(route.is_some(), "Missing route for action: {}", action);
    }
}

#[test]
fn test_unknown_action_returns_error() {
    let payload = json!({});
    let result = execute_ax_action("unknown_action", payload);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown action"));
}

#[test]
fn test_routing_table_integration() {
    // 集成测试: 验证 parse → execute 流程
    let payload = json!({"key": "a", "modifiers": []});
    let result = execute_ax_action("press", payload);
    // 不验证 macOS API 结果，只验证路由表能正确调用 parser + executor
    assert!(result.is_ok() || result.unwrap_err().to_string().contains("platform"));
}
```

#### 3. AxSnapshotCache (cache 验证唯一逻辑)

**测试目标** (Priority #1 from Q13):
- 验证 epoch mismatch 导致 cache miss
- 验证 TTL 过期导致 cache miss
- 验证 policy (ImplicitObserve 5s vs Progressive 300s)
- 验证 cache 不是 epoch 真相源 (读取 ObservationStore)

**测试策略**:
```rust
#[test]
fn test_stale_epoch_invalidates_cache() {
    let mut store = ObservationStore::new();
    let obs_id = "obs_1";
    let snapshot = create_test_snapshot();
    
    // 缓存 snapshot，epoch = 100
    store.cache_ax_snapshot(obs_id, snapshot.clone(), CachePolicy::ImplicitObserve);
    
    // 更新 ObservationStore 的 epoch = 101
    store.update_resource_epoch("pid_123", 101);
    
    // cache 查询应失败 (epoch mismatch)
    let cached = store.query_cached_snapshot(obs_id);
    assert!(cached.is_none());
}

#[test]
fn test_implicit_observe_ttl_5s() {
    let mut store = ObservationStore::new();
    let obs_id = "obs_1";
    let snapshot = create_test_snapshot();
    
    store.cache_ax_snapshot(obs_id, snapshot.clone(), CachePolicy::ImplicitObserve);
    
    // 4 秒内应 hit
    std::thread::sleep(Duration::from_secs(4));
    assert!(store.query_cached_snapshot(obs_id).is_some());
    
    // 6 秒后应 miss
    std::thread::sleep(Duration::from_secs(2));
    assert!(store.query_cached_snapshot(obs_id).is_none());
}

#[test]
fn test_progressive_ttl_300s() {
    let mut store = ObservationStore::new();
    let obs_id = "obs_1";
    let snapshot = create_test_snapshot();
    
    store.cache_ax_snapshot(obs_id, snapshot.clone(), CachePolicy::Progressive);
    
    // 299 秒内应 hit (用 mock time 加速)
    store.advance_time(Duration::from_secs(299));
    assert!(store.query_cached_snapshot(obs_id).is_some());
    
    // 301 秒后应 miss
    store.advance_time(Duration::from_secs(2));
    assert!(store.query_cached_snapshot(obs_id).is_none());
}

#[test]
fn test_cache_validates_against_observation_store() {
    let mut store = ObservationStore::new();
    let obs_id = "obs_1";
    let snapshot = create_test_snapshot();
    
    // 缓存 snapshot
    store.cache_ax_snapshot(obs_id, snapshot.clone(), CachePolicy::ImplicitObserve);
    
    // 修改 ObservationStore 的 truth (不通过 cache)
    store.direct_update_epoch("pid_123", 999);
    
    // cache 查询应失败，即使 TTL 未过期
    let cached = store.query_cached_snapshot(obs_id);
    assert!(cached.is_none());
}
```

### 单元测试覆盖

**Parse 函数** (易赢，Priority #2):
```rust
#[test]
fn test_parse_press_payload() {
    let payload = json!({"key": "a", "modifiers": ["shift"]});
    let request = parse_press(payload).unwrap();
    assert_eq!(request.key, "a");
    assert_eq!(request.modifiers, vec!["shift"]);
}

// 为所有 7 个 parse_* 函数添加类似测试
```

**Perform 函数** (最后，需要平台 mock):
```rust
#[test]
#[ignore] // 需要 macOS 环境或 mock
fn test_execute_press() {
    let request = PressRequest { key: "a", modifiers: vec![] };
    let result = execute_press(request);
    // 验证平台 API 调用 (需要 mock 框架)
}
```

### 集成测试覆盖

**路由表完整性**:
```rust
#[test]
fn test_end_to_end_action_routing() {
    for (action, payload) in get_test_cases() {
        let result = execute_ax_action(action, payload);
        assert!(result.is_ok() || is_expected_error(&result));
    }
}
```

**ObservationCapture 集成**:
```rust
#[test]
fn test_observation_to_query_integration() {
    let store = ObservationStore::new();
    let capture = ObservationCapture::new();
    
    // 捕获 observation
    let (snapshot, selectors) = capture.capture_for_observation("window_123").unwrap();
    
    // 缓存到 store
    let obs_id = store.register_observation(snapshot, selectors);
    
    // 查询 cache
    let cached = store.query_cached_snapshot(&obs_id);
    assert!(cached.is_some());
}
```

## Implementation Plan

### Phase 1: ax_input (最小，流程模板)

**目标**: 建立拆分流程，快速获得反馈。

**步骤**:
1. 创建 `ax_input/` 目录结构
2. 在 `mod.rs` 中实现高级接口
3. 移动 `input.rs` (2.8KB 保持不变)
4. 为高级接口添加单元测试
5. 迁移 control_actions.rs 的 input 调用
6. 标记旧 control_ax input 函数为 `#[deprecated]`
7. 迁移完成后删除 deprecated 函数

**验收标准**:
- ✅ `type_text(content, mode)` 通过 20 个单元测试
- ✅ `send_key(key, modifiers)` 通过 15 个单元测试
- ✅ control_actions.rs 所有 input 调用迁移完成
- ✅ `cargo clippy` 无 deprecated 警告

**时间估计**: 2-3 天

### Phase 2: ax_action (最大，核心逻辑)

**目标**: 实现统一入口和路由表，解决 Friction #4。

**步骤**:
1. 创建 `ax_action/` 目录结构
2. 在 `mod.rs` 中实现数据驱动的路由表
3. 移动 parse_* 到 `protocol.rs`
4. 移动 perform_* 到 `execute.rs`
5. 移动 `platform/macos.rs` (67KB 保持不变)
6. 为 parse 函数添加单元测试 (易赢)
7. 为路由表添加集成测试
8. 迁移 control_actions.rs 的 action 调用
9. 删除 deprecated 函数

**验收标准**:
- ✅ ROUTES 表包含所有 13 个 action
- ✅ 所有 parse_* 函数通过单元测试
- ✅ `execute_ax_action` 通过路由表集成测试
- ✅ control_actions.rs 所有 action 调用迁移完成

**时间估计**: 5-7 天

### Phase 3: ax_query + Cache 迁移 (最复杂)

**目标**: 实现无状态 query 模块，迁移 cache 到 ObservationStore，打破循环依赖。

**步骤**:
1. 创建 `ax_query/` 目录结构
2. 移动 capture 逻辑从 `tree.rs` 到 `capture.rs`
3. 保持 `query.rs` (45.6KB 不变)
4. 在 control_observation 中引入 `ObservationCapture` adapter
5. 定义 `AxSnapshotCache` 并支持多 policy
6. **为 cache 验证逻辑添加单元测试 (Priority #1)**
7. 迁移 AxObservationCache 到 control_observation
8. 迁移所有 capture 调用使用 `ObservationCapture`
9. 删除 deprecated 函数

**验收标准**:
- ✅ ObservationCapture 通过 15 个单元测试
- ✅ AxSnapshotCache 验证逻辑通过 20 个单元测试
- ✅ epoch mismatch 测试覆盖所有场景
- ✅ TTL policy 测试覆盖 ImplicitObserve 和 Progressive
- ✅ 循环依赖消除 (cargo 依赖图验证)
- ✅ 所有 capture 调用迁移完成

**时间估计**: 7-10 天

### Phase 4: 内部调用方迁移 (增量)

**范围**: 仅内部调用方 (control_actions.rs, control_computer_act/, control_flow/)

**时间线**: 每个 Phase 完成后立即进行

**外部 API**: control_protocol 命令 (@computer-act, @ax-tree) 保持不变

**Facade 生命周期**:
- 立即标记 `#[deprecated]`
- 所有内部调用方迁移后删除
- 仅为潜在的内部测试工具保留 (最小范围)

## Consequences

### Positive

- **接口深度**: 53 个函数 → 3 个模块约 15 个高级函数
- **局部性**: 理解 AX query 只需阅读 ax_query/，而非整个 control_ax
- **杠杆作用**: 每个模块提供 5-10 个高级函数，隐藏内部复杂性
- **可测试性**: 可以独立测试 ax_query (通过 mock ObservationStore 的 adapter)
- **可维护性**: 新增 action = 更新 ROUTES 表，而非编写新函数
- **解决 Frictions #2, #3, #4**: Epoch 真相源、双重缓存、路由表

### Negative

- **迁移成本**: 3 个主要调用方需要迁移 (control_actions, control_computer_act, control_flow)
- **学习曲线**: 开发者需要学习新的模块边界
- **Import 冗长**: 调用方从 3 个模块导入而非 1 个 (通过清晰的模块名缓解)

### Mitigations

- **增量迁移**: 一次一个模块，每个模块独立可审查
- **清晰文档**: 更新 CONTEXT.md 添加新模块定义
- **Facade 层**: 临时 `#[deprecated]` 重导出用于渐进式迁移
- **测试覆盖**: 全面的测试确保无回归

## Alternatives Considered

### A. 单一统一命令: execute_ax_command(AxCommand)

**拒绝**: 过于激进。需要将所有 53 个函数变为枚举变体。高迁移成本，收益不成比例。

### B. 仅提取平台代码 (ax_platform)

**拒绝**: 解决平台抽象但不解决接口复杂度。53 个函数仍保留在 control_ax 中。

### C. 保留 control_ax，添加 facade 模块

**拒绝**: Facade 而不移动实现 = 虚假的模块化。不会减少实际复杂度。

### D. 四个模块: ax_query, ax_action, ax_input, ax_platform

**延迟**: ax_platform 提取延迟到实际需要多平台支持时 (YAGNI)。macos.rs 保留在 ax_action 并添加 TODO 注释。

## ADR Compatibility Check

### ADR-0005 (Lifecycle)

✅ **兼容**: ObservationCapture 维护同步调用路径用于 implicit_observe。5 秒 TTL 复用契约通过 CachePolicy::ImplicitObserve 保留。

### ADR-0006 (Integration & Observability)

✅ **兼容**: @flow 嵌入 @computer-act 继续工作。内部 @flow dispatch 直接调用新路径，绕过 facade 确保 density/trace_summary/verification 字段存在。

### Other ADRs

✅ **无冲突** 与 ADR-0001 到 ADR-0004。

## References

- Architecture Review Report: `/var/folders/.../architecture-review-20260820.html`
- Exploration Data: Agent af29a0a9335f08314 (68053 tokens, 26 tool uses)
- Grilling Session: ADR-0008 记录了 3 轮中回答的所有 21 个设计问题
- ADR-0008: `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/docs/adr/0008-control-ax-module-split.md`
- CONTEXT.md: `/Users/cuiluming/local_doc/l_dev/my/rust/rustdog/CONTEXT.md`

## Glossary

- **Observation Capture**: 为 Recording Session 或 Replay 捕获 AX tree 的轻量级适配器，封装了"为 observation 生成 snapshot + selectors"的语义。
- **AX Snapshot Cache**: 缓存已捕获的 AX tree snapshot 和对应的资源 epoch 快照，用于避免重复 observation 注册。支持多种 TTL policy。
- **Successor Observation**: 资源 lane 完成 mutation 并提交稳定 write epoch 后，针对原目标窗口生成的新 observation。
- **Canonical Mutation Path**: Agent 执行带有 ref、observation_id 和 epoch 的状态变更时，统一使用 @computer-act 的结构化 mutation 路径。
- **Ref-backed Type Mutation**: 带有 observation-local ref、observation_id 和 epoch 的 @computer-act action:"type"。
