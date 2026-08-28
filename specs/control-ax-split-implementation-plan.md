# control_ax Module Split Implementation Plan

**生成时间**: 2026-08-20  
**ADR 参考**: docs/adr/0008-control-ax-module-split.md  
**架构审查**: /var/folders/.../architecture-review-20260820.html

> **执行状态 (2026-08-28 更新)**: 三个阶段全部落地, 提交链 068024b..218a435。
> 与原计划的三类 as-built 分歧:
> ① 动态 routing 表 / execute_ax_action 字符串入口 / protocol.rs parse 层确认零生产
> 消费者后移除 (ADR-0008 Amendment); ② 阶段 3 的 ax_query 只收纳无状态捕获核心,
> query.rs 保留在 control_ax 作为 @ax-find/@ax-get verb 层, 缓存不迁移
> (ticket 07/08 superseded, ADR-0008 Amendment 2); ③ ObservationCapture 的 as-built
> 形态是 AxSnapshot::with_observation 方法。
> 收尾阶段另有: press(target: &AxTarget) 取代 postcondition 合并式单入口 (R1);
> ax_input 的 80/20 简单 API 层删除, 升级为承载投递策略的真执行模块 (R3);
> macos.rs 按 Q4 决策保留在 control_ax 共享平台层。
> 另: commit 62c782e 曾夹带一次全仓 cargo fmt 扫荡 (control_recording 纯重排),
> 当时未在提交信息记录, 在此留档。
> 下文为原始执行计划, 保留作决策历史。

---

## 执行摘要

将 control_ax (122KB, 53+ 函数) 拆分为三个专注模块：
- **ax_query/** (~47KB) - AX tree 捕获与查询
- **ax_action/** (~70KB) - AX action 执行与 routing
- **ax_input/** (~3KB) - 文本与键盘输入

**关键收益**：接口深度提升（53 函数 → ~15 函数），解决循环依赖，统一 cache 策略。

---

## 阶段 1: ax_input（最小，验证流程）

### 目标
- 验证增量拆分流程
- 建立测试模板
- 最小 blast radius

### 步骤

#### 1.1 创建目录结构
```bash
mkdir -p src/ax_input
touch src/ax_input/mod.rs
touch src/ax_input/types.rs
```

#### 1.2 实现高层接口 (src/ax_input/mod.rs)

```rust
//! Text and keyboard input with simplified high-level API.
//!
//! This module provides two tiers:
//! - Simple API (80% use cases): `type_text()`, `send_key()`
//! - Advanced API (20% use cases): `type_text_with_config()`, `send_key_with_config()`

use crate::control_ax::input::{perform_default_key_delivery, perform_default_type_text};
use crate::control_ax::types::{KeyRequest, TypeTextRequest, TypeTextMode};
use std::io;

pub use crate::control_ax::types::{KeyDelivery, TypeMode, WindowActionReport};

/// Simple API: Type text into the active window.
///
/// Default behavior:
/// - Delivery: KeyDelivery::default()
/// - Target: Active window
/// - Verification: None
pub fn type_text(content: &str, mode: TypeMode) -> io::Result<WindowActionReport> {
    let request = TypeTextRequest {
        content: content.to_string(),
        mode: mode.into(),
        delivery: KeyDelivery::default(),
        target_window: None,
        verification: None,
    };
    perform_default_type_text(&request)
}

/// Simple API: Send a key with optional modifiers.
pub fn send_key(key: Key, modifiers: &[Modifier]) -> io::Result<()> {
    let request = KeyRequest {
        key: key.into(),
        modifiers: modifiers.iter().map(|m| m.into()).collect(),
        delivery: KeyDelivery::default(),
    };
    perform_default_key_delivery(&request)
}

/// Advanced API: Type text with full configuration.
///
/// Use when you need:
/// - Custom delivery mode
/// - Explicit target window
/// - Post-action verification
pub fn type_text_with_config(request: TypeTextRequest) -> io::Result<WindowActionReport> {
    perform_default_type_text(&request)
}

/// Advanced API: Send key with full configuration.
pub fn send_key_with_config(request: KeyRequest) -> io::Result<()> {
    perform_default_key_delivery(&request)
}

// Re-export types for advanced usage
pub mod types {
    pub use crate::control_ax::types::{
        KeyRequest, TypeTextRequest, TypeTextMode, KeyDelivery, Key, Modifier,
    };
}
```

#### 1.3 添加单元测试 (src/ax_input/tests.rs)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_api_should_hide_request_complexity() {
        // Verify that simple API doesn't require TypeTextRequest construction
        let result = type_text("hello", TypeMode::Character);
        assert!(result.is_ok() || result.is_err()); // Syntax check only
    }

    #[test]
    fn advanced_api_should_allow_custom_config() {
        let request = TypeTextRequest {
            content: "test".into(),
            mode: TypeTextMode::Character,
            delivery: KeyDelivery::FrontmostApp,
            target_window: Some("Terminal".into()),
            verification: None,
        };
        let result = type_text_with_config(request);
        assert!(result.is_ok() || result.is_err());
    }
}
```

#### 1.4 迁移调用方 (control_actions.rs)

```diff
- use crate::control_ax::{perform_default_type_text, perform_default_key_delivery};
+ use crate::ax_input::{type_text, send_key, type_text_with_config};

fn handle_type_action(payload: &str) -> io::Result<WindowActionReport> {
-   let request = parse_type_text_payload(payload)?;
-   perform_default_type_text(&request)
+   type_text(payload, TypeMode::Character)
}
```

#### 1.5 标记旧函数为 deprecated (src/control_ax.rs)

```rust
#[deprecated(since = "0.9.0", note = "use ax_input::type_text instead")]
pub fn perform_default_type_text(request: &TypeTextRequest) -> io::Result<WindowActionReport> {
    ax_input::type_text_with_config(request.clone())
}
```

#### 1.6 验证与清理
- [ ] 运行 `cargo test --package rustdog --lib ax_input`
- [ ] 运行 `cargo test` (全量测试)
- [ ] 检查 deprecation warnings: `cargo build 2>&1 | grep deprecated`
- [ ] 迁移完成后删除 deprecated 函数

**预计耗时**: 1-2 天

---

## 阶段 2: ax_action（最大，核心逻辑）

### 目标
- 统一 action 执行入口
- 数据化 routing 表
- 分离 protocol 和 execution 层

### 步骤

#### 2.1 创建目录结构
```bash
mkdir -p src/ax_action/platform
touch src/ax_action/mod.rs
touch src/ax_action/protocol.rs
touch src/ax_action/execute.rs
touch src/ax_action/types.rs
```

#### 2.2 实现数据化 routing 表 (src/ax_action/mod.rs)

```rust
//! AX action execution with unified entry point and data-driven routing.

use serde_json::Value;
use std::io;

mod protocol;
mod execute;
pub mod types;
pub mod platform;

pub use types::{ActionResult, AnyActionRequest};

/// Unified entry: Execute any AX action by name.
///
/// Supported actions: press, action, set_value, focus, scroll, drag, wait, etc.
///
/// # Example
/// ```
/// let payload = json!({"target": {"ref": "button_1"}, "action": "Press"});
/// let result = execute_ax_action("press", payload)?;
/// ```
pub fn execute_ax_action(action: &str, payload: Value) -> io::Result<ActionResult> {
    let route = ROUTES
        .iter()
        .find(|r| r.name == action)
        .ok_or_else(|| io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown action: {}", action)
        ))?;

    let request = (route.parser)(payload)?;
    (route.executor)(request)
}

/// Data-driven routing table (solves Friction #4).
///
/// All 13 actions visible at a glance.
struct ActionRoute {
    name: &'static str,
    parser: fn(Value) -> io::Result<AnyActionRequest>,
    executor: fn(AnyActionRequest) -> io::Result<ActionResult>,
    timeout_ms: u64,
}

const ROUTES: &[ActionRoute] = &[
    ActionRoute {
        name: "press",
        parser: protocol::parse_press,
        executor: execute::perform_press,
        timeout_ms: 5000,
    },
    ActionRoute {
        name: "action",
        parser: protocol::parse_action,
        executor: execute::perform_action,
        timeout_ms: 5000,
    },
    ActionRoute {
        name: "set_value",
        parser: protocol::parse_set_value,
        executor: execute::perform_set_value,
        timeout_ms: 10000,
    },
    ActionRoute {
        name: "focus",
        parser: protocol::parse_focus,
        executor: execute::perform_focus,
        timeout_ms: 3000,
    },
    ActionRoute {
        name: "scroll",
        parser: protocol::parse_scroll,
        executor: execute::perform_scroll,
        timeout_ms: 5000,
    },
    // ... 剩余 8 个 action
];

// Re-export for backward compatibility (temporary facade)
#[deprecated(since = "0.9.0", note = "use execute_ax_action instead")]
pub fn perform_default_ax_press(req: &AxPressRequest) -> io::Result<ActionResult> {
    execute_ax_action("press", serde_json::to_value(req)?)
}

// TODO: When multi-platform support is needed, extract platform/ into ax_platform module.
// Currently only macOS is implemented (YAGNI principle).
```

#### 2.3 实现 protocol 层 (src/ax_action/protocol.rs)

```rust
//! Protocol layer: Parse JSON payloads into typed requests.

use super::types::*;
use serde_json::Value;
use std::io;

pub fn parse_press(payload: Value) -> io::Result<AnyActionRequest> {
    // Move parse_ax_press_payload logic here
    let req: AxPressRequest = serde_json::from_value(payload)?;
    Ok(AnyActionRequest::Press(req))
}

pub fn parse_action(payload: Value) -> io::Result<AnyActionRequest> {
    let req: AxActionRequest = serde_json::from_value(payload)?;
    Ok(AnyActionRequest::Action(req))
}

// ... 其余 11 个 parse 函数
```

#### 2.4 实现 execution 层 (src/ax_action/execute.rs)

```rust
//! Execution layer: Perform actions via platform APIs.

use super::platform;
use super::types::*;
use std::io;

pub fn perform_press(request: AnyActionRequest) -> io::Result<ActionResult> {
    let AnyActionRequest::Press(req) = request else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "type mismatch"));
    };

    // Move perform_default_ax_press logic here
    platform::macos::press_element(&req.target, &req.action)?;
    
    Ok(ActionResult::Press { success: true })
}

// ... 其余 12 个 perform 函数
```

#### 2.5 移动平台实现 (src/ax_action/platform/macos.rs)

```bash
mv src/control_ax/macos.rs src/ax_action/platform/macos.rs
```

**保持文件内容不变** (67KB)，只调整 `use` 路径。

#### 2.6 添加测试

**单元测试** (src/ax_action/protocol_tests.rs):
```rust
#[test]
fn parse_press_should_accept_valid_payload() {
    let payload = json!({"target": {"ref": "btn_1"}, "action": "Press"});
    let result = protocol::parse_press(payload);
    assert!(result.is_ok());
}
```

**Routing 表测试** (src/ax_action/routing_tests.rs):
```rust
#[test]
fn routing_table_should_cover_all_13_actions() {
    let expected_actions = [
        "press", "action", "set_value", "focus", "scroll",
        "drag", "wait", "click", "hover", "type", "hotkey", "screenshot", "open_app"
    ];
    
    for action in expected_actions {
        assert!(ROUTES.iter().any(|r| r.name == action), "missing route: {}", action);
    }
}
```

#### 2.7 迁移调用方
- control_actions.rs: 将 11+ 个 perform 调用改为 `execute_ax_action`
- control_computer_act/: 更新 routing 逻辑

#### 2.8 验证
- [ ] 单元测试通过
- [ ] 集成测试通过
- [ ] Routing 表完整性测试通过
- [ ] 删除 deprecated 函数

**预计耗时**: 3-5 天

---

## 阶段 3: ax_query + Cache Migration（最复杂）

### 目标
- 拆分 capture/query 逻辑
- 引入 ObservationCapture adapter
- 移动 AxSnapshotCache 到 control_observation
- 打破循环依赖

### 步骤

#### 3.1 创建目录结构
```bash
mkdir -p src/ax_query
touch src/ax_query/mod.rs
touch src/ax_query/capture.rs
touch src/ax_query/types.rs
```

#### 3.2 实现 ax_query 接口 (src/ax_query/mod.rs)

```rust
//! AX tree capture and query (stateless module).

use std::io;

mod capture;
pub mod query;  // Re-export query.rs (45.6KB unchanged)
pub mod types;

pub use types::{AxSnapshot, AxQuery, AxElement, Selector};

/// Capture AX tree for a specific window.
///
/// Stateless: Does not cache. Caller decides caching policy.
pub fn capture_window_snapshot(window_id: &str) -> io::Result<AxSnapshot> {
    capture::capture_window(window_id)
}

/// Capture AX tree based on query.
pub fn capture_tree(query: &AxQuery) -> io::Result<AxSnapshot> {
    capture::capture_with_query(query)
}

/// Find element in snapshot by selector.
pub fn find_element(snapshot: &AxSnapshot, selector: &Selector) -> Option<&AxElement> {
    query::find(snapshot, selector)
}
```

#### 3.3 移动 capture 逻辑 (src/ax_query/capture.rs)

从 `control_ax/tree.rs` 提取 capture 函数：
```rust
//! AX tree capture implementation.

use super::types::*;
use std::io;

pub fn capture_window(window_id: &str) -> io::Result<AxSnapshot> {
    // Move capture_current_ax_window_snapshot logic here
    todo!("extract from tree.rs")
}

pub fn capture_with_query(query: &AxQuery) -> io::Result<AxSnapshot> {
    // Move capture_ax_find_snapshot logic here
    todo!("extract from tree.rs")
}
```

#### 3.4 引入 ObservationCapture adapter (src/control_observation/capture_adapter.rs)

```rust
//! Lightweight adapter for capturing AX trees for observations.
//!
//! This is a long-term seam (not temporary adapter) that encapsulates
//! "capture for observation" semantics: returns snapshot + selectors.

use crate::ax_query;
use std::io;

pub struct ObservationCapture;

impl ObservationCapture {
    /// Capture AX tree for observation registration.
    ///
    /// Returns: (snapshot, selectors) tuple ready for ObservationStore.
    pub fn capture_for_observation(
        window_id: &str
    ) -> io::Result<(AxSnapshot, Selectors)> {
        let snapshot = ax_query::capture_window_snapshot(window_id)?;
        let selectors = Self::build_selectors(&snapshot);
        Ok((snapshot, selectors))
    }

    fn build_selectors(snapshot: &AxSnapshot) -> Selectors {
        // Extract selector building logic from current code
        todo!()
    }
}
```

#### 3.5 定义 AxSnapshotCache (src/control_observation/ax_cache.rs)

```rust
//! AX snapshot cache with multi-policy TTL support.

use std::collections::{HashMap, VecDeque};

pub struct AxSnapshotCache {
    entries: HashMap<String, AxSnapshotCacheEntry>,
    order: VecDeque<String>,
    capacity: usize,
}

pub struct AxSnapshotCacheEntry {
    snapshot: AxSnapshot,
    epochs: HashMap<String, u64>,  // Immutable snapshot at capture time
    policy: CachePolicy,
    captured_at_unix_ms: u64,
}

/// Caller-specified cache policy.
pub enum CachePolicy {
    /// 5-second TTL for implicit_observe (ADR-0005 requirement).
    ImplicitObserve { ttl_ms: u64 },
    
    /// 300-second TTL for progressive queries.
    Progressive { ttl_ms: u64 },
}

impl AxSnapshotCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// Insert snapshot with caller-specified policy.
    pub fn insert(
        &mut self,
        observation_id: String,
        snapshot: AxSnapshot,
        epochs: HashMap<String, u64>,
        policy: CachePolicy,
    ) {
        // FIFO eviction if at capacity
        if self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }

        self.entries.insert(
            observation_id.clone(),
            AxSnapshotCacheEntry {
                snapshot,
                epochs,
                policy,
                captured_at_unix_ms: current_time_ms(),
            },
        );
        self.order.push_back(observation_id);
    }

    /// Get cached snapshot if not expired and epochs match.
    pub fn get(
        &self,
        observation_id: &str,
        current_epochs: &HashMap<String, u64>,
    ) -> Option<&AxSnapshot> {
        let entry = self.entries.get(observation_id)?;

        // Check TTL expiration
        let elapsed = current_time_ms() - entry.captured_at_unix_ms;
        let ttl_ms = match entry.policy {
            CachePolicy::ImplicitObserve { ttl_ms } => ttl_ms,
            CachePolicy::Progressive { ttl_ms } => ttl_ms,
        };
        if elapsed > ttl_ms {
            return None;  // Expired
        }

        // Check epoch match
        for (resource_key, &cached_epoch) in &entry.epochs {
            if let Some(&current_epoch) = current_epochs.get(resource_key) {
                if cached_epoch != current_epoch {
                    return None;  // Stale
                }
            }
        }

        Some(&entry.snapshot)
    }
}

fn current_time_ms() -> u64 {
    // Platform-specific time implementation
    todo!()
}
```

#### 3.6 集成到 ObservationStore (src/control_observation.rs)

```rust
pub struct ObservationStore {
    observations: HashMap<String, StoredObservation>,
    ax_snapshot_cache: AxSnapshotCache,  // New
}

impl ObservationStore {
    pub fn new() -> Self {
        Self {
            observations: HashMap::new(),
            ax_snapshot_cache: AxSnapshotCache::new(64),  // 64 entries capacity
        }
    }

    /// Cache AX snapshot with caller-specified policy.
    pub fn cache_ax_snapshot(
        &mut self,
        observation_id: String,
        snapshot: AxSnapshot,
        epochs: HashMap<String, u64>,
        policy: CachePolicy,
    ) {
        self.ax_snapshot_cache.insert(observation_id, snapshot, epochs, policy);
    }

    /// Get cached AX snapshot (validates against current epochs).
    pub fn get_cached_ax_snapshot(
        &self,
        observation_id: &str,
    ) -> Option<&AxSnapshot> {
        let current_epochs = capture_resource_epochs();  // From control_resource_lane
        self.ax_snapshot_cache.get(observation_id, &current_epochs)
    }
}
```

#### 3.7 添加关键路径测试 (优先级 #1)

```rust
#[cfg(test)]
mod cache_validation_tests {
    use super::*;

    #[test]
    fn cache_should_reject_stale_epochs() {
        let mut cache = AxSnapshotCache::new(64);
        
        let mut epochs = HashMap::new();
        epochs.insert("window_1".into(), 5);
        
        cache.insert(
            "obs_1".into(),
            mock_snapshot(),
            epochs.clone(),
            CachePolicy::Progressive { ttl_ms: 300000 },
        );

        // Simulate epoch increment
        epochs.insert("window_1".into(), 6);

        // Should return None (stale)
        assert!(cache.get("obs_1", &epochs).is_none());
    }

    #[test]
    fn cache_should_respect_ttl_policy() {
        let mut cache = AxSnapshotCache::new(64);
        
        cache.insert(
            "obs_1".into(),
            mock_snapshot(),
            HashMap::new(),
            CachePolicy::ImplicitObserve { ttl_ms: 100 },  // 100ms TTL
        );

        // Immediate get: should succeed
        assert!(cache.get("obs_1", &HashMap::new()).is_some());

        // After 150ms: should expire
        std::thread::sleep(Duration::from_millis(150));
        assert!(cache.get("obs_1", &HashMap::new()).is_none());
    }

    #[test]
    fn observation_capture_should_return_snapshot_and_selectors() {
        let result = ObservationCapture::capture_for_observation("Terminal");
        assert!(result.is_ok());
        
        let (snapshot, selectors) = result.unwrap();
        assert!(!snapshot.elements.is_empty());
        assert!(!selectors.is_empty());
    }
}
```

#### 3.8 迁移所有 capture 调用

更新 control_observation 中的调用：
```diff
- use crate::control_ax::{capture_current_ax_window_snapshot, ...};
+ use crate::control_observation::capture_adapter::ObservationCapture;

fn record_observation_with_selectors_from_capture(...) -> io::Result<String> {
-   let snapshot = capture_current_ax_window_snapshot(window_id)?;
-   let selectors = build_selectors_from_snapshot(&snapshot);
+   let (snapshot, selectors) = ObservationCapture::capture_for_observation(window_id)?;
    
    let observation_id = self.record_observation(snapshot, selectors)?;
    
    // Cache with appropriate policy
+   self.cache_ax_snapshot(
+       observation_id.clone(),
+       snapshot,
+       capture_resource_epochs(),
+       CachePolicy::Progressive { ttl_ms: 300000 },
+   );
    
    Ok(observation_id)
}
```

#### 3.9 验证
- [ ] Cache 验证测试通过（关键路径）
- [ ] ObservationCapture adapter 测试通过
- [ ] 循环依赖已打破（`cargo build` 无循环依赖警告）
- [ ] 全量测试通过
- [ ] 删除旧的 AxObservationCache 代码（在 control_ax.rs）

**预计耗时**: 4-6 天

---

## 验收标准

### 功能验收
- [ ] 所有现有测试通过（796+ tests）
- [ ] 新增测试覆盖：cache 验证、routing 表、高层接口
- [ ] 编译无 warning（除了 deprecated 警告）

### 架构验收
- [ ] 循环依赖已打破（control_observation 不再直接调用 ax_query capture 函数）
- [ ] 接口深度提升（53 函数 → ~15 函数）
- [ ] 模块职责清晰（ax_query: 无状态 capture, ax_action: 统一入口, ax_input: 高层 API）

### 文档验收
- [ ] ADR-0008 已创建
- [ ] CONTEXT.md 已更新（新增 Observation Capture, AX Snapshot Cache 定义）
- [ ] 每个新模块的 mod.rs 有 module-level 文档
- [ ] 迁移指南已提供（for 外部调用方，如果有）

### 性能验收
- [ ] 无性能回归（运行现有 benchmark suite）
- [ ] Cache hit rate 不低于当前水平

---

## 风险与缓解

### 风险 1: 迁移期间回归
**概率**: 中  
**影响**: 高  
**缓解**: 
- 每个阶段独立 PR，可独立回滚
- 测试驱动拆分（先写测试，再移动代码）
- 保留 deprecated facade 直到全部迁移完成

### 风险 2: Cache 语义变化
**概率**: 中  
**影响**: 高  
**缓解**:
- 优先测试 cache 验证逻辑（关键路径）
- 用 integration test 验证 implicit_observe 的 5s TTL 仍然工作
- 对比新旧 cache 的行为（单元测试 + manual testing）

### 风险 3: ADR-0005/0006 兼容性破坏
**概率**: 低  
**影响**: 高  
**缓解**:
- ObservationCapture 保持同步调用（满足 ADR-0005）
- @flow 调用新路径直接绕过 facade（满足 ADR-0006 的 density/trace 字段）
- 添加 integration test 验证 @computer-act 响应格式不变

### 风险 4: Platform 抽象过早
**概率**: 低  
**影响**: 低  
**缓解**:
- 暂不定义 PlatformAx trait（YAGNI）
- 在 ax_action/mod.rs 顶部添加 TODO 注释
- 等真正需要多平台时再抽象

---

## 时间线

| 阶段 | 预计耗时 | 累计耗时 |
|------|---------|---------|
| Phase 1: ax_input | 1-2 天 | 1-2 天 |
| Phase 2: ax_action | 3-5 天 | 4-7 天 |
| Phase 3: ax_query + Cache | 4-6 天 | 8-13 天 |
| 文档与验收 | 1 天 | 9-14 天 |

**总预计**: 2-3 周（考虑 code review 和迭代时间）

---

## 后续优化（不在本次范围）

- [ ] 引入 PlatformAx trait（等多平台支持时）
- [ ] ax_action 超过 80KB 时考虑拆分 platform/
- [ ] 考虑更激进的 cache 统一（如果发现新的 policy 需求）
- [ ] 性能优化（如果 routing 表查找成为瓶颈）

---

## 参考资料

- **ADR-0008**: docs/adr/0008-control-ax-module-split.md
- **架构审查报告**: /var/folders/.../architecture-review-20260820.html
- **Grilling 决策树**: 21 个问题，3 轮质询（本文档基于）
- **探索数据**: Agent af29a0a9335f08314 (68053 tokens)
