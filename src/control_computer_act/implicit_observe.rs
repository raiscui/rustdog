//! `@computer-act` 的 implicit_observe plumbing。
//!
//! 设计目标 (ticket 11 / ADR-0005 L3):
//! - `start_box` 路径: 触发内部 implicit_observe,记录 `observation_id`,供后续轮次复用
//! - `target.ref + target.observation_id` 路径: 校验 TTL,5s 内直接复用 (`fresh`),
//!   过期或缺失时自动 re-observe (`stale_re_observed`),把新 id 回填到 response
//! - TTL = 5000 ms,严格按 ADR-0005,跟全局 `ObservationStore` (300s) 解耦
//!
//! ticket 11 不依赖真实 AX/screenshot observe (那是 ticket 18 / Phase I 的工作)。
//! 本轮 cache 只维护 `observation_id → ref_id` 映射 + TTL,底层 dispatch 仍用
//! `MouseEndpoint::Coordinate`。后续 real observe 接入后,这里再加 `ref_id → backend`
//! 字段,把 endpoint 从 `Coordinate` 切到 `ObservationRef`。
//!
//! 单一真相源: 所有 freshness 判断都走 `cache.resolve_or_re_observe()`,不分散到
//! 多个 caller 各自判定 TTL。

use serde_json::Value;
use std::io;
use std::sync::{Mutex, OnceLock};

/// TTL: ADR-0005 L3 显式规定 5 秒。rdog 默认 `ObservationStore` 是 300s,本轮
/// `ComputerActObservationCache` 独立维护这个值,不跟全局 store 耦合。
pub const COMPUTER_ACT_OBSERVATION_TTL_MS: u64 = 5_000;

/// `observation_id → (ref_id, created_at_ms)` 记录。
///
/// `ref_id` 在 ticket 11 阶段是 synthetic `@e{seq}` (跟 Mano-CUA `@e1` 风格对齐),
/// 后续 real observe 接入后,这里换 AX backend 真实 ref。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImplicitObservationRecord {
    pub observation_id: String,
    pub ref_id: String,
    pub created_at_ms: u64,
}

/// `implicit_observe` 调用的产物。
///
/// - `Fresh`: 命中现存 observation 且未过期,不需要 re-observe
/// - `StaleReObserved`: 现存 observation 已过期或缺失,daemon 自动 re-observe
///   并把新 `observation_id` 写到 `record`
/// - `StaleFallbackToCoords`: 留接口给 real observe 阶段; ticket 11 不暴露,
///   留给 Phase I (后续 ticket 21) 真正对接 `@observe` 后再启用
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImplicitObserveOutcome {
    Fresh {
        record: ImplicitObservationRecord,
    },
    StaleReObserved {
        /// 可能为 `None` (client 只给 `target.ref` 但没给 `observation_id`,
        /// 或 `observation_id` 不在 cache 里)
        previous_observation_id: Option<String>,
        record: ImplicitObservationRecord,
    },
    StaleFallbackToCoords {
        reason: String,
        last_known_observation_id: Option<String>,
    },
}

/// 5 秒 TTL 的 `observation_id` 缓存。
///
/// 设计要点:
/// - `now_ms` 参数注入: 测试可用 mock clock; real path 喂 `unix_epoch_ms()`
/// - 按 `created_at_ms + ttl_ms` 严格判定过期,不用 wall clock
/// - 容量上限 64 条,超出按 FIFO evict (跟全局 `ObservationStore` 一致)
/// - 单线程调用方 (rdog dispatcher 当前是单 worker),无内部锁
#[derive(Debug, Clone)]
pub(crate) struct ComputerActObservationCache {
    ttl_ms: u64,
    max_records: usize,
    next_seq: u64,
    /// FIFO 顺序,头部最老,尾部最新
    order: Vec<String>,
    records: std::collections::HashMap<String, ImplicitObservationRecord>,
}

impl Default for ComputerActObservationCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputerActObservationCache {
    pub fn new() -> Self {
        Self::with_limits(COMPUTER_ACT_OBSERVATION_TTL_MS, 64)
    }

    /// 测试用: 调 TTL / 容量上限。production 也需要,因为 `new()` 调用它
    /// 一次性传入默认值。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_limits(ttl_ms: u64, max_records: usize) -> Self {
        Self {
            ttl_ms,
            max_records,
            next_seq: 1,
            order: Vec::new(),
            records: std::collections::HashMap::new(),
        }
    }

    /// 触发一次新的 implicit_observe,生成新 `observation_id` + synthetic ref_id。
    ///
    /// ticket 11 阶段: `ref_id = format!("@e{seq}")` (Mano-CUA 风格占位)。
    pub fn record_implicit(&mut self, now_ms: u64) -> ImplicitObservationRecord {
        self.evict_expired(now_ms);

        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let observation_id = format!("computer-act-obs-{now_ms}-{seq}");
        let ref_id = format!("@e{seq}");

        let record = ImplicitObservationRecord {
            observation_id: observation_id.clone(),
            ref_id,
            created_at_ms: now_ms,
        };
        self.order.push(observation_id.clone());
        self.records.insert(observation_id, record.clone());
        self.evict_over_capacity();
        record
    }

    /// 解析给定 `observation_id` 是否仍在 TTL 内。
    ///
    /// 返回:
    /// - `Ok(record)` → 在 TTL 内,可复用
    /// - `Err(NotFoundOrExpired)` → 不存在或已过期; 调用方决定 re-observe 还是 fallback
    pub fn resolve(&mut self, observation_id: &str, now_ms: u64) -> Result<ImplicitObservationRecord, ()> {
        self.evict_expired(now_ms);
        match self.records.get(observation_id) {
            Some(record) => Ok(record.clone()),
            None => Err(()),
        }
    }

    /// 包装 `route_computer_act_action` 之前的入口调用。
    ///
    /// 输入:
    /// - `args` 是 request `args` 字段 (rdog dict 已经 parse 成 `serde_json::Value`)
    /// - `now_ms` 是注入的时钟 (生产 = `unix_epoch_ms`,测试 = mock)
    ///
    /// 输出:
    /// - 返回 outcome + 修改后的 `args` (若 start_box 路径,args 不变; 若 stale 路径,
    ///   把新的 `observation_id` 透传给底层 dispatcher,后续 ticket 18 真实 observe
    ///   接入后,这里再把 `start_box` 替换为 `target.ref`)
    pub fn resolve_or_re_observe(
        &mut self,
        args: &Value,
        now_ms: u64,
    ) -> ImplicitObserveOutcome {
        // 1. 看 caller 是否给了 `target.ref + target.observation_id`
        let target = args.get("target").and_then(|v| v.as_object());
        if let Some(target) = target {
            let ref_id = target.get("ref").and_then(|v| v.as_str()).map(str::to_owned);
            let observation_id = target
                .get("observation_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned);

            if let Some(obs_id) = observation_id.as_deref() {
                // 客户端给了 observation_id → 校验 TTL
                match self.resolve(obs_id, now_ms) {
                    Ok(record) => return ImplicitObserveOutcome::Fresh { record },
                    Err(()) => {
                        // 过期或不存在 → re-observe,ref_id 用旧 target.ref (real observe
                        // 阶段会用 fresh AX ref; ticket 11 阶段沿用 stale ref_id)
                        let stale_record = self.record_implicit(now_ms);
                        let mut new_record = stale_record;
                        if let Some(prior_ref) = ref_id {
                            new_record.ref_id = prior_ref;
                        }
                        return ImplicitObserveOutcome::StaleReObserved {
                            previous_observation_id: Some(obs_id.to_string()),
                            record: new_record,
                        };
                    }
                }
            }

            // 客户端只给 target.ref 但没给 observation_id → 当作 stale,
            // 用 client 提供的 ref_id 作为新 ref 占位
            if let Some(prior_ref) = ref_id {
                let mut new_record = self.record_implicit(now_ms);
                new_record.ref_id = prior_ref;
                return ImplicitObserveOutcome::StaleReObserved {
                    previous_observation_id: None,
                    record: new_record,
                };
            }
        }

        // 2. start_box 路径 (或没 target 也没 start_box 的非鼠标动作):
        // - click/scroll/drag/hover 等鼠标动作: 一定有 start_box
        // - wait/open_app/open_url/type/hotkey 等非鼠标动作: 不需要 observe
        //
        // 简化决策: 只要有 `start_box`,就 re-observe (走 fresh 路径,而不是 stale)
        if args.get("start_box").is_some() {
            let record = self.record_implicit(now_ms);
            return ImplicitObserveOutcome::Fresh { record };
        }

        // 3. 既没 target 也没 start_box (非鼠标动作): 不需要 observation_id,
        // 返回 Fallback 占位 (caller 看到 `StaleFallbackToCoords` 时,不要写
        // observation_id 到 response)
        ImplicitObserveOutcome::StaleFallbackToCoords {
            reason: "non-mouse action without target ref; no implicit_observe needed"
                .to_string(),
            last_known_observation_id: None,
        }
    }

    fn evict_expired(&mut self, now_ms: u64) {
        // 收集过期 entries (避免在迭代时 mut borrow 自己)
        let expired: Vec<String> = self
            .order
            .iter()
            .filter(|id| {
                self.records
                    .get(*id)
                    .map(|r| now_ms.saturating_sub(r.created_at_ms) > self.ttl_ms)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        for id in expired {
            self.records.remove(&id);
            self.order.retain(|x| x != &id);
        }
    }

    fn evict_over_capacity(&mut self) {
        while self.order.len() > self.max_records {
            if let Some(oldest) = self.order.first().cloned() {
                self.records.remove(&oldest);
                self.order.remove(0);
            } else {
                break;
            }
        }
    }
}

/// Daemon 进程内的全局 `ComputerActObservationCache`,5 秒 TTL。
///
/// 单一真相源: 所有 `@computer-act` implicit_observe 都走这个 cache,
/// 避免每个 executor / 每个 call site 各自起一个独立 cache (那会让 freshness
/// 跨请求失效,违反 L3 设计)。
///
/// 用 `OnceLock<Mutex<...>>` 而不是 `Arc<Mutex<...>>` 注入:
/// - rdog 当前是单 dispatcher worker,没有跨 executor 共享需求
/// - 测试用 `initialize_for_tests` 替换,避免污染真实 daemon
static COMPUTER_ACT_OBSERVATION_CACHE: OnceLock<Mutex<ComputerActObservationCache>> =
    OnceLock::new();

fn global_cache() -> &'static Mutex<ComputerActObservationCache> {
    COMPUTER_ACT_OBSERVATION_CACHE.get_or_init(|| Mutex::new(ComputerActObservationCache::new()))
}

/// 真实 daemon 启动时调用一次,显式初始化 cache (用默认 TTL 5s)。
///
/// 跟 `initialize_durable_observation_state` 同模式;幂等。
#[cfg_attr(not(test), allow(dead_code))]
pub fn initialize_computer_act_observation_state() {
    let _ = COMPUTER_ACT_OBSERVATION_CACHE.get_or_init(|| Mutex::new(ComputerActObservationCache::new()));
}

/// 测试用:替换 cache 为可注入时钟的实例。
#[cfg(test)]
pub(crate) fn initialize_computer_act_observation_state_for_tests(
    cache: ComputerActObservationCache,
) {
    // 测试每次调用都强制覆盖,跟 `initialize_durable_observation_state_for_tests`
    // 模式一致。如果需要锁住后续调用,测试应在 `lock()` 之后 hold MutexGuard。
    let cache_for_init = cache.clone();
    let slot = COMPUTER_ACT_OBSERVATION_CACHE.get_or_init(|| Mutex::new(cache_for_init));
    *slot.lock().expect("computer-act observation cache poisoned") = cache;
}

/// 测试用:清空 cache,避免下一个测试看到上一次的 observation。
#[cfg(test)]
pub(crate) fn reset_computer_act_observation_state_for_tests() {
    if let Some(slot) = COMPUTER_ACT_OBSERVATION_CACHE.get() {
        *slot.lock().expect("computer-act observation cache poisoned") =
            ComputerActObservationCache::new();
    }
}

/// 真实 daemon 路径: 在 caller 拿不到 `now_ms` 注入时,用 wall clock。
pub(crate) fn resolve_or_re_observe_with_wall_clock(args: &Value) -> ImplicitObserveOutcome {
    let now_ms = unix_epoch_ms();
    let mut guard = global_cache()
        .lock()
        .expect("computer-act observation cache poisoned");
    guard.resolve_or_re_observe(args, now_ms)
}

/// 测试 / 单元路径: caller 提供 mock clock。预留 API 给后续 ticket 18 / Phase I
/// real observe 集成 (那时 caller 需要显式控制 now 而不靠 wall clock)。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_or_re_observe_at(args: &Value, now_ms: u64) -> ImplicitObserveOutcome {
    let mut guard = global_cache()
        .lock()
        .expect("computer-act observation cache poisoned");
    guard.resolve_or_re_observe(args, now_ms)
}

/// 注入时钟: `SystemTime::now() - UNIX_EPOCH` 的毫秒数。
///
/// 跟 `ObservationStore` 同模式 (real path 喂 unix epoch ms,测试 mock 不同时间)。
pub fn unix_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把 `ImplicitObserveOutcome` 转换成 response 的 `observation_used` 字段。
///
/// 单一真相源: caller 写 response 时只能通过这个 helper,不要自己判 outcome。
pub(crate) fn render_observation_used(outcome: &ImplicitObserveOutcome) -> Option<Value> {
    match outcome {
        ImplicitObserveOutcome::Fresh { record } => Some(serde_json::json!({
            "observation_id": &record.observation_id,
            "ref_id": &record.ref_id,
            "freshness": "fresh",
        })),
        ImplicitObserveOutcome::StaleReObserved {
            previous_observation_id,
            record,
        } => {
            let mut obj = serde_json::json!({
                "observation_id": &record.observation_id,
                "ref_id": &record.ref_id,
                "freshness": "stale_re_observed",
                "re_observe_id": &record.observation_id,
            });
            if let Some(prev) = previous_observation_id {
                obj["previous_observation_id"] = Value::String(prev.clone());
            }
            Some(obj)
        }
        ImplicitObserveOutcome::StaleFallbackToCoords { .. } => {
            // ticket 11 阶段: non-mouse actions 不暴露 observation_used
            None
        }
    }
}

/// 把 `ImplicitObserveOutcome` 里的 `observation_id` 提取出来给 response 顶层用。
///
/// 仅 `Fresh` / `StaleReObserved` 有 id; fallback 路径返回 `None`。
pub(crate) fn render_top_level_observation_id(outcome: &ImplicitObserveOutcome) -> Option<String> {
    match outcome {
        ImplicitObserveOutcome::Fresh { record } => Some(record.observation_id.clone()),
        ImplicitObserveOutcome::StaleReObserved { record, .. } => {
            Some(record.observation_id.clone())
        }
        ImplicitObserveOutcome::StaleFallbackToCoords { .. } => None,
    }
}

/// ticket 11 阶段: 底层 dispatch 仍用 `MouseEndpoint::Coordinate` (start_box 像素)。
/// 这里只保证 outcome 被记录,不动 start_box → ref 替换。
///
/// 后续 Phase I real observe 集成 (ticket 21+) 时,这一层再补
/// `replace_start_box_with_target_ref(args, &outcome)`: 把 start_box 拆掉,换
/// 成 `target.ref + target.observation_id`,让 click/hover/drag 走真实 ref 路径。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn apply_implicit_observe_to_args(
    _args: &mut Value,
    _outcome: &ImplicitObserveOutcome,
) -> io::Result<()> {
    Ok(())
}



#[cfg(test)]
mod tests {
    //! ticket 11 implicit_observe plumbing 单测。
    //!
    //! 覆盖:
    //! - cache 基础 CRUD (record / resolve / FIFO 容量)
    //! - TTL 边界 (clock injection)
    //! - `resolve_or_re_observe` 4 个分支
    //! - response envelope helpers

    use super::*;
    use serde_json::json;

    // --- ComputerActObservationCache ---

    #[test]
    fn record_implicit_returns_sequential_ids_and_refs() {
        let mut cache = ComputerActObservationCache::new();
        let r1 = cache.record_implicit(1_000);
        let r2 = cache.record_implicit(1_000);
        assert_eq!(r1.ref_id, "@e1");
        assert_eq!(r2.ref_id, "@e2");
        assert_ne!(r1.observation_id, r2.observation_id);
        assert!(r1.observation_id.contains("computer-act-obs-1000-1"));
        assert!(r2.observation_id.contains("computer-act-obs-1000-2"));
    }

    #[test]
    fn resolve_returns_record_within_ttl() {
        let mut cache = ComputerActObservationCache::new();
        let rec = cache.record_implicit(2_000);
        let got = cache.resolve(&rec.observation_id, 2_000 + 4_000).unwrap();
        assert_eq!(got.ref_id, rec.ref_id);
        assert_eq!(got.created_at_ms, 2_000);
    }

    #[test]
    fn resolve_returns_notfound_after_ttl_expires() {
        let mut cache = ComputerActObservationCache::new();
        let rec = cache.record_implicit(2_000);
        let got = cache.resolve(&rec.observation_id, 2_000 + 6_000);
        assert!(got.is_err(), "should be expired at t+6000");
    }

    #[test]
    fn resolve_returns_notfound_for_unknown_observation_id() {
        let mut cache = ComputerActObservationCache::new();
        let got = cache.resolve("computer-act-obs-9999-99", 1_000);
        assert!(got.is_err());
    }

    #[test]
    fn cache_evicts_oldest_over_capacity() {
        let mut cache = ComputerActObservationCache::with_limits(60_000, 3);
        let r1 = cache.record_implicit(1_000);
        let r2 = cache.record_implicit(1_001);
        let r3 = cache.record_implicit(1_002);
        let r4 = cache.record_implicit(1_003);
        assert!(cache.resolve(&r1.observation_id, 1_010).is_err(), "r1 evicted");
        assert!(cache.resolve(&r2.observation_id, 1_010).is_ok(), "r2 kept");
        assert!(cache.resolve(&r3.observation_id, 1_010).is_ok(), "r3 kept");
        assert!(cache.resolve(&r4.observation_id, 1_010).is_ok(), "r4 kept");
    }

    #[test]
    fn ttl_default_is_5_seconds_per_adr_0005() {
        assert_eq!(COMPUTER_ACT_OBSERVATION_TTL_MS, 5_000);
    }

    // --- resolve_or_re_observe 4 个分支 ---

    #[test]
    fn start_box_path_returns_fresh_observation() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"start_box": [100, 200]}), 1_000);
        match outcome {
            ImplicitObserveOutcome::Fresh { record } => {
                assert_eq!(record.ref_id, "@e1");
                assert_eq!(record.created_at_ms, 1_000);
            }
            _ => panic!("expected Fresh, got {outcome:?}"),
        }
    }

    #[test]
    fn target_ref_and_observation_id_within_ttl_returns_fresh() {
        let mut cache = ComputerActObservationCache::new();
        let _ = cache.resolve_or_re_observe(&json!({"start_box": [100, 200]}), 1_000);
        let obs_id_1 = match cache.resolve_or_re_observe(&json!({"start_box": [300, 400]}), 1_001) {
            ImplicitObserveOutcome::Fresh { record } => record.observation_id.clone(),
            _ => panic!("first call should produce Fresh"),
        };
        let args = json!({
            "target": {"ref": "@e1", "observation_id": obs_id_1.clone()}
        });
        let outcome = cache.resolve_or_re_observe(&args, 1_001 + 1_000);
        match outcome {
            ImplicitObserveOutcome::Fresh { record } => {
                assert_eq!(record.observation_id, obs_id_1);
            }
            _ => panic!("expected Fresh, got {outcome:?}"),
        }
    }

    #[test]
    fn target_ref_and_observation_id_expired_returns_stale_re_observed() {
        let mut cache = ComputerActObservationCache::new();
        let obs = cache.record_implicit(1_000);
        let prior_obs_id = obs.observation_id.clone();
        let args = json!({
            "target": {"ref": "@e1", "observation_id": obs.observation_id}
        });
        let outcome = cache.resolve_or_re_observe(&args, 1_000 + 6_000);
        match outcome {
            ImplicitObserveOutcome::StaleReObserved { previous_observation_id, record } => {
                assert_eq!(previous_observation_id, Some(prior_obs_id.clone()));
                assert_eq!(record.ref_id, "@e1");
                assert_ne!(record.observation_id, prior_obs_id);
            }
            _ => panic!("expected StaleReObserved, got {outcome:?}"),
        }
    }

    #[test]
    fn target_ref_only_without_observation_id_returns_stale_re_observed() {
        let mut cache = ComputerActObservationCache::new();
        let args = json!({"target": {"ref": "@e99"}});
        let outcome = cache.resolve_or_re_observe(&args, 1_000);
        match outcome {
            ImplicitObserveOutcome::StaleReObserved { previous_observation_id, record } => {
                assert_eq!(previous_observation_id, None);
                assert_eq!(record.ref_id, "@e99", "client 提供的 ref 应该保留");
                assert!(record.observation_id.contains("computer-act-obs-1000-"));
            }
            _ => panic!("expected StaleReObserved, got {outcome:?}"),
        }
    }

    #[test]
    fn non_mouse_action_without_target_or_start_box_returns_fallback() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"duration_ms": 500}), 1_000);
        assert!(matches!(outcome, ImplicitObserveOutcome::StaleFallbackToCoords { .. }));
    }

    #[test]
    fn unknown_observation_id_returns_stale_re_observed() {
        let mut cache = ComputerActObservationCache::new();
        let args = json!({
            "target": {"ref": "@e1", "observation_id": "computer-act-obs-0-0"}
        });
        let outcome = cache.resolve_or_re_observe(&args, 1_000);
        assert!(matches!(outcome, ImplicitObserveOutcome::StaleReObserved { .. }));
    }

    // --- response envelope helpers ---

    #[test]
    fn render_observation_used_fresh_shape() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"start_box": [10, 20]}), 1_000);
        let rendered = render_observation_used(&outcome).expect("fresh must produce value");
        assert_eq!(rendered["freshness"], "fresh");
        assert_eq!(rendered["ref_id"], "@e1");
        assert!(rendered["observation_id"].is_string());
    }

    #[test]
    fn render_observation_used_stale_shape_includes_re_observe_and_previous() {
        let mut cache = ComputerActObservationCache::new();
        let obs = cache.record_implicit(1_000);
        let prior_obs_id = obs.observation_id.clone();
        let args = json!({
            "target": {"ref": "@e1", "observation_id": obs.observation_id}
        });
        let outcome = cache.resolve_or_re_observe(&args, 1_000 + 6_000);
        let rendered = render_observation_used(&outcome).expect("stale must produce value");
        assert_eq!(rendered["freshness"], "stale_re_observed");
        assert_eq!(rendered["previous_observation_id"], prior_obs_id);
        assert_eq!(rendered["ref_id"], "@e1");
        // ADR-0005: `observation_id` (顶层) 和 `re_observe_id` 都是新生成的 id。
        // ticket 11 当前两个值都从 `record.observation_id` 取,等价。
        assert_eq!(rendered["observation_id"], rendered["re_observe_id"]);
        assert_ne!(rendered["observation_id"], prior_obs_id);
    }

    #[test]
    fn render_observation_used_stale_without_previous_obs_id_omits_field() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"target": {"ref": "@e1"}}), 1_000);
        let rendered = render_observation_used(&outcome).expect("stale ref-only must produce value");
        assert_eq!(rendered["freshness"], "stale_re_observed");
        assert!(rendered.get("previous_observation_id").is_none(),
            "no previous obs_id means omit field, not null");
    }

    #[test]
    fn render_observation_used_fallback_returns_none() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"duration_ms": 100}), 1_000);
        assert!(render_observation_used(&outcome).is_none(),
            "non-mouse fallback should not write observation_used field");
    }

    #[test]
    fn render_top_level_observation_id_returns_id_for_observable_outcomes() {
        let mut cache = ComputerActObservationCache::new();
        let fresh = cache.resolve_or_re_observe(&json!({"start_box": [1, 1]}), 1_000);
        let id1 = render_top_level_observation_id(&fresh).expect("fresh has id");
        let args = json!({"target": {"ref": "@e1", "observation_id": id1.clone()}});
        let still_fresh = cache.resolve_or_re_observe(&args, 4_000);
        assert_eq!(render_top_level_observation_id(&still_fresh), Some(id1.clone()));
        let stale = cache.resolve_or_re_observe(&args, 8_000);
        let id2 = render_top_level_observation_id(&stale).expect("stale has new id");
        assert_ne!(id1, id2, "stale re-observe must produce fresh id");
    }

    #[test]
    fn render_top_level_observation_id_returns_none_for_fallback() {
        let mut cache = ComputerActObservationCache::new();
        let outcome = cache.resolve_or_re_observe(&json!({"app_name": "X"}), 1_000);
        assert!(render_top_level_observation_id(&outcome).is_none());
    }

    #[test]
    fn global_cache_wall_clock_helper_produces_fresh_for_start_box() {
        super::initialize_computer_act_observation_state_for_tests(ComputerActObservationCache::new());
        let outcome = super::resolve_or_re_observe_with_wall_clock(&json!({"start_box": [50, 60]}));
        assert!(matches!(outcome, ImplicitObserveOutcome::Fresh { .. }));
        super::reset_computer_act_observation_state_for_tests();
    }
}
