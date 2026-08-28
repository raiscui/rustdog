# ADR-0008: Split control_ax into ax_query, ax_action, ax_input modules

## Status

Accepted (2026-08-20); amended 2026-08-27 (动态 routing 层移除) and 2026-08-28 (阶段 3 按现实切分)

## Amendment (2026-08-27): 动态 routing 层按 as-built 移除

阶段 1 (ax_input) 与阶段 2 (ax_action) 已通过 Tickets #01-#11 完整落地后, 本 ADR 中
"ax_action 统一入口 `execute_ax_action(action, payload)` + 数据化 routing 表" 的设计
被实施事实推翻并移除 (commit 2e8239e):

- 全仓库所有边界 (compact 行协议 / ui_script / web RPC) 在迁移中均选择强类型路径,
  `ControlCommand` 到达分发点时已是 typed request, 动态层零生产消费者。
- 接入它需要人为 Value 往返序列化, 无能力收益; 保留则为死代码。
- as-built 的 ax_action 入口是 7 个强类型函数: `press` / `press_with_postcondition` /
  `press_sequence` / `perform_action` / `set_value` / `focus` / `scroll`;
  protocol 层职责由 control_protocol 的 compact 行解析承担, ax_action/protocol.rs 已删除。
- ax_input 同理收敛为单一 `type_text_with_config` / `send_key_with_config` 完整配置 API,
  80/20 简单包装层 (零调用) 一并删除。

阶段 3 (ax_query + cache migration) 于 2026-08-28 落地, 同样按 as-built 修正 (见下方
Amendment 2)。完整决策记录与备选方案见 task_plan.md 对应日期条目。

## Amendment 2 (2026-08-28): 阶段 3 ax_query 按现实切分线落地

原阶段 3 设计 (ObservationCapture adapter struct / AxSnapshotCache 多 TTL policy 迁移 /
query.rs 整体搬入 ax_query) 经现状核实后按以下修正实施:

- **ax_query 是无状态捕获核心** (~370 行, mod.rs + capture.rs), 收纳 tree.rs 中零
  observation/protocol 依赖的 capture / 查找 / target 物化函数与 6 个路由测试。
  纯度契约写入模块文档并可用 grep 断言: 禁止 import control_observation /
  control_protocol, 无 static 状态。
- **query.rs 保留在 control_ax**: 它是 @ax-find/@ax-get 的 verb 实现
  (协议解析 + observation ref 解析 + display scope), 不是纯查询引擎,
  原样搬入会破坏 ax_query 的无状态目标。纯查询原语 (collect_ax_role_values 等)
  按需进 ax_query。
- **缓存不迁移 (ticket 08 superseded)**: epoch 真相源分离已由 #51/#54/#55 落地
  (真相源在 observation store / control_resource_lane, 缓存仅存捕获时快照并校验),
  多 TTL policy 场景实际由 computer-act 的 implicit observe 缓存独立承担。
  迁移只剩物理位置收益, 不值得动刚对抗性验证过的路径。
- **ObservationCapture adapter (ticket 07) superseded**: as-built 的富化 seam 是
  `AxSnapshot::with_observation()`, selector draft 构造留在 control_ax/tree.rs,
  observation 侧入口是 record_observation_with_selectors_from_capture。
- **循环依赖 (ticket 10) 部分收敛**: capture 消费方 (observation/screenshot/web/
  computer_act/mouse/ax_action) 全部改从 ax_query 导入, control_ax 不再是
  capture 入口的 hub; verb 层与 observation 的双向编排边保留
  (with_observation 注册与缓存校验属于 verb 层职责)。
- **macos.rs 未搬入 ax_action/platform/** (grilling Q4 决策延续): macos.rs 保持
  在 control_ax 作为共享平台层; R3 后 type-text 投递策略已上移 ax_input,
  macos.rs 只剩平台原语, 未来多平台需求出现时再评估独立 ax_platform。
- **收尾阶段 (2026-08-28 下午) 两处 API as-built 分歧** (完整决策见 task_plan):
  - R1: `press(target: &AxTarget)` 取代 "postcondition 合并进 press 单入口" 的
    原设计 -- routing 表删除后双分支失去存在理由, postcondition 在类型层
    不可表示, guarded press 一律走 press_with_postcondition。
  - R3: ax_input 升级为真执行模块 -- 原 "80/20 简单 API 分层" 已删除,
    新增 execute.rs 承载 type-text 投递策略 (模式分发 + Auto 回退链 +
    错误命名), 平台路径注入; AxBackend trait 移除 type_text 方法。

## Context

control_ax.rs is a 122KB file with 53+ public functions, exhibiting shallow module characteristics where interface complexity nearly equals implementation complexity. The architecture review identified it as the top priority friction point:


- **17+ parse functions**: parse_ax_tree_payload, parse_ax_press_payload, etc.
- **10+ perform functions**: perform_default_ax_press, perform_default_ax_action, etc.
- **5+ capture functions**: capture_ax_find_snapshot, capture_current_ax_subtree, etc.
- **3+ resolve functions**: resolve_cached_ax_tree, resolve_current_ax_target_rect, etc.

Callers need to understand all 53 functions to use the module correctly. The module also has a circular dependency with control_observation (bidirectional calls).

### Deletion Test Result

If we delete control_ax, complexity would not concentrate but scatter to callers — they would need to know "which parse corresponds to which perform", "when to use cached vs current", increasing cognitive load.

### Module Size Breakdown

- `control_ax.rs`: 3611 lines (main file)
- `macos.rs`: 67KB (platform implementation)
- `query.rs`: 45.6KB (query logic)
- `tree.rs`: 15.1KB (tree capture)
- `types.rs`: 14.1KB (type definitions)
- `input.rs`: 2.8KB (input handling)

Total: 4298 lines

## Decision

Split control_ax into three focused modules with deep interfaces:

### 1. ax_query/ — AX tree capture & query (~47KB)

**Responsibility**: Capture AX trees, execute queries, provide snapshots.

**Public Interface** (deep, ~5 functions):
```rust
pub fn capture_window_snapshot(window_id: &str) -> io::Result<AxSnapshot>;
pub fn capture_tree(query: &AxQuery) -> io::Result<AxSnapshot>;
pub fn find_element(snapshot: &AxSnapshot, selector: &Selector) -> Option<AxElement>;
pub fn query_cached(observation_id: &str) -> Option<&AxSnapshot>;
```

**Internal Structure**:
- `mod.rs`: Public interface + re-exports
- `capture.rs`: Tree capture logic (from tree.rs)
- `query.rs`: Query execution (45.6KB, unchanged)
- `types.rs`: Query-specific types

**Key Design**: Stateless module. Does not own cache; cache logic moves to control_observation.

### 2. ax_action/ — AX action execution (~70KB)

**Responsibility**: Parse action payloads, execute actions via platform APIs, manage routing.

**Public Interface** (deep, unified entry):
```rust
pub fn execute_ax_action(action: &str, payload: Value) -> io::Result<ActionResult>;
```

**Internal Structure**:
- `mod.rs`: Unified entry + data-driven routing table
- `protocol.rs`: 7 parse_* functions (protocol layer)
- `execute.rs`: 7 perform_* functions (execution layer)
- `platform/macos.rs`: 67KB macOS implementation (unchanged)
- `types.rs`: Action-specific types

**Routing Table** (data-driven, solves Friction #4):
```rust
struct ActionRoute {
    name: &'static str,
    parser: fn(Value) -> io::Result<AnyActionRequest>,
    executor: fn(AnyActionRequest) -> io::Result<ActionResult>,
}

const ROUTES: &[ActionRoute] = &[
    ActionRoute { name: "press", parser: parse_press, executor: execute_press },
    // ... 13 actions visible at a glance
];
```

**TODO**: When multi-platform support is needed, extract `platform/` into a separate ax_platform module. Currently only macOS is implemented (YAGNI).

### 3. ax_input/ — Text & keyboard input (~3KB)

**Responsibility**: High-level text input and key delivery.

**Public Interface** (layered, 80/20 split):
```rust
// Simple API (80% use cases)
pub fn type_text(content: &str, mode: TypeMode) -> io::Result<TypeReport>;
pub fn send_key(key: Key, modifiers: &[Modifier]) -> io::Result<KeyReport>;

// Advanced API (20% use cases)
pub fn type_text_with_config(request: TypeTextRequest) -> io::Result<TypeReport>;
pub fn send_key_with_config(request: KeyRequest) -> io::Result<KeyReport>;
```

**Internal Structure**:
- `mod.rs`: High-level interface (hides Request types)
- `input.rs`: 2.8KB implementation (unchanged)
- `types.rs`: Input-specific types

**Key Design**: Simple API hides `TypeTextRequest` complexity (delivery, target_window, verification) with sensible defaults. Advanced API exposes full control for special cases.

### 4. Breaking the Circular Dependency

**Problem**: control_ax calls control_observation (4 functions), control_observation calls control_ax (capture functions).

**Solution**: Introduce `ObservationCapture` adapter in control_observation.

```rust
// In control_observation
pub struct ObservationCapture {
    // Internal: wraps ax_query calls
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

**Rationale**:
- ObservationCapture is a **long-term seam**, not temporary adapter
- It encapsulates "capture for observation" semantics (returns snapshot + selectors)
- Maintains synchronous call path (required by ADR-0005 for implicit_observe)
- Added to CONTEXT.md as a domain concept

### 5. AxObservationCache Migration

**Current**: Defined in control_ax.rs (internal static singleton)

**New Location**: control_observation (owned by ObservationStore)

```rust
// In control_observation
pub struct ObservationStore {
    observations: HashMap<String, StoredObservation>,
    ax_snapshot_cache: AxSnapshotCache,  // New
}

pub struct AxSnapshotCache {
    entries: HashMap<String, AxSnapshotCacheEntry>,
    order: VecDeque<String>,
}

pub struct AxSnapshotCacheEntry {
    snapshot: AxSnapshot,
    epochs: HashMap<String, u64>,  // Immutable snapshot, not truth source
    policy: CachePolicy,            // New
}

pub enum CachePolicy {
    ImplicitObserve { ttl_ms: 5000 },    // ADR-0005 requirement
    Progressive { ttl_ms: 300000 },
}
```

**Rationale**:
- Solves Friction #2 (three truth sources for epoch)
- ObservationStore remains the single truth source for resource epochs
- AxSnapshotCache is an "acceleration layer" that validates against ObservationStore
- Solves Friction #3 (dual cache) by supporting multiple TTL policies per entry

**Caller-specified Policy**:
```rust
observation_store.cache_ax_snapshot(
    observation_id,
    snapshot,
    CachePolicy::ImplicitObserve,  // Caller decides
);
```

## Implementation Plan

### Phase 1: ax_input (Smallest, Template for Process)

1. Create `ax_input/` directory structure
2. Implement high-level interface in `mod.rs`
3. Move `input.rs` (2.8KB unchanged)
4. Add unit tests for high-level interface
5. Migrate control_actions.rs input calls
6. Mark old control_ax input functions as `#[deprecated]`
7. Delete deprecated functions after migration

### Phase 2: ax_action (Largest, Core Logic)

1. Create `ax_action/` directory structure
2. Implement data-driven routing table in `mod.rs`
3. Move parse_* to `protocol.rs`
4. Move perform_* to `execute.rs`
5. Move `platform/macos.rs` (67KB unchanged)
6. Add unit tests for parse functions (easy wins)
7. Add integration tests for routing table
8. Migrate control_actions.rs action calls
9. Delete deprecated functions

### Phase 3: ax_query + Cache Migration (Most Complex)

1. Create `ax_query/` directory structure
2. Move capture logic from `tree.rs` to `capture.rs`
3. Keep `query.rs` (45.6KB unchanged)
4. Introduce `ObservationCapture` adapter in control_observation
5. Define `AxSnapshotCache` with multi-policy support
6. Add unit tests for cache validation logic (Priority #1 from Q13)
7. Move AxObservationCache to control_observation
8. Migrate all capture calls to use `ObservationCapture`
9. Delete deprecated functions

### Phase 4: Internal Caller Migration (Incremental)

**Scope**: Only internal callers (control_actions.rs, control_computer_act/, control_flow/)

**Timeline**: Immediately after each phase completes

**External API**: control_protocol commands (@computer-act, @ax-tree) unchanged

**Facade Lifetime**:
- Marked `#[deprecated]` immediately
- Removed after all internal callers migrated
- Only preserved for potential internal test tools (minimal scope)

## Testing Strategy

### Priority Order (Q13-C: Critical Path First)

1. **Cache validation logic** (AxObservationCache → ObservationStore migration)
   - Tests epoch validation
   - Tests TTL policy enforcement
   - Validates ObservationCapture adapter design

2. **Parse logic** (Easy wins, pure functions)
   - Tests all 7 parse_* functions
   - JSON → struct conversion

3. **Perform logic** (Last, requires platform mock)
   - Integration tests with macOS AX API mocks

### Test-Driven Split

- Write tests for new interfaces **before** moving code
- Ensure tests pass on both old and new paths during transition
- Remove old tests only after migration complete

## Consequences

### Positive

- **Interface Depth**: 53 functions → ~15 high-level functions across 3 modules
- **Locality**: Understanding AX query only requires reading ax_query/, not entire control_ax
- **Leverage**: Each module provides 5-10 high-level functions hiding internal complexity
- **Testability**: Can test ax_query independently (mock ObservationStore via adapter)
- **Maintainability**: Adding new action = update ROUTES table, not write new function
- **Solves Frictions #2, #3, #4**: Epoch truth source, dual cache, routing table

### Negative

- **Migration Cost**: 3 major调用方需要迁移 (control_actions, control_computer_act, control_flow)
- **Learning Curve**: Developers need to learn new module boundaries
- **Import Verbosity**: Callers import from 3 modules instead of 1 (mitigated by clear module names)

### Mitigations

- **Incremental Migration**: One module at a time, each independently reviewable
- **Clear Documentation**: Update CONTEXT.md with new module definitions
- **Facade Layer**: Temporary `#[deprecated]` re-exports for gradual migration
- **Test Coverage**: Comprehensive tests ensure no regressions

## Alternatives Considered

### A. Single unified command: execute_ax_command(AxCommand)

**Rejected**: Too aggressive. Requires all 53 functions to become enum variants. High migration cost without proportional benefit.

### B. Extract only platform code (ax_platform)

**Rejected**: Solves platform abstraction but not interface complexity. 53 functions would remain in control_ax.

### C. Keep control_ax, add facade modules

**Rejected**: Facade without moving implementation = fake modularity. Does not reduce actual complexity.

### D. Four modules: ax_query, ax_action, ax_input, ax_platform

**Deferred**: ax_platform extraction deferred until multi-platform support is actually needed (YAGNI). macos.rs remains in ax_action with TODO comment.

## ADR Compatibility Check

### ADR-0005 (Lifecycle)

✅ **Compatible**: ObservationCapture maintains synchronous call path for implicit_observe. The 5-second TTL reuse contract is preserved via CachePolicy::ImplicitObserve.

### ADR-0006 (Integration & Observability)

✅ **Compatible**: @flow embedding of @computer-act continues to work. Internal @flow dispatch will call new paths directly, bypassing facade to ensure density/trace_summary/verification fields are present.

### Other ADRs

✅ **No conflicts** with ADR-0001 through ADR-0004.

## References

- Architecture Review Report: `/var/folders/.../architecture-review-20260820.html`
- Exploration Data: Agent af29a0a9335f08314 (68053 tokens, 26 tool uses)
- Grilling Session: This ADR documents all 21 design questions answered in 3 rounds
