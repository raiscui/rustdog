//! bulk 的直连强制判定与请求级证据快照
//! (specs/rdog-bulk-direct-only-spec.md §3 判定表 / §4 观测口径)。
//!
//! # 三格的语义 (这里就是定义, 别处只引用)
//!
//! | 字段 | 是谁的事实 | 读它的层 | 读到时的动作 |
//! | --- | --- | --- | --- |
//! | `path_aware` | **传输适配器**的拓扑属性: 这条链路上到底有没有 relay 跳 | 门禁 (`decide`) 与快照 | `false` ⇒ 路径判定不适用, bulk **允许**, 快照记 `not_applicable` |
//! | `bulk_path_pin` | **传输适配器**能否把 bulk 连接钉在直连路径上 (禁止迁移、排队与重传不得改走 relay) | 门禁 | `true` 配 `path_aware=true` 时才 `false` 有意义: 有 relay 跳却钉不住 ⇒ **拒绝** |
//! | `path_class` | **本次尝试此刻**观测到的路径 (`direct`/`relay`/`unknown`) | 门禁 (open 与每个 chunk 边界) | `direct` 放行; `relay` 拒 (`iroh_direct_path_unavailable`); 读不出 ⇒ `unknown` ⇒ **拒** (`iroh_path_unknown`, fail-closed) |
//!
//! 三格互相不能顶替: `path_aware=false` 说的是"拓扑里没有 relay 跳", `path_aware=true && bulk_path_pin=false`
//! 说的是"有 relay 跳但钉不住"。把后者当前者, 就得到一条"看起来有门禁、其实没有"的路径。
//!
//! `not_applicable` 是**声明出来的能力缺失**, 不是"没读到"; 后者一律归 `unknown` 并 fail-closed。

use serde_json::{json, Value};

/// 一条链路的路径能力 (由传输适配器声明, 不由配置推断)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PathCapability {
    pub(crate) path_aware: bool,
    pub(crate) bulk_path_pin: bool,
}

impl PathCapability {
    /// 拓扑里没有 relay 跳 (TCP / WebSocket / Zenoh): 路径判定不适用。
    pub(crate) const NOT_PATH_AWARE: Self = Self {
        path_aware: false,
        bulk_path_pin: false,
    };

    /// 有 relay 跳但**钉不住** bulk 连接: 该传输不得承载 bulk。
    ///
    /// 这是 iroh lane 今天的形态 —— 它只服务 `@ping` / `@pty`, 没有 bulk 的数据面, 也就谈不上
    /// 把 bulk 钉在直连上。宁可不接受 bulk, 也不接受一个"大致直连"。
    pub(crate) const PATH_AWARE_UNPINNED: Self = Self {
        path_aware: true,
        bulk_path_pin: false,
    };

    /// 有 relay 跳且能钉住 (适配器补上 pin 之后的形态)。
    pub(crate) const PATH_AWARE_PINNED: Self = Self {
        path_aware: true,
        bulk_path_pin: true,
    };
}

/// 一次尝试此刻观测到的路径类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathClass {
    Direct,
    Relay,
    /// 读不出 / 采样缺失 / 解析失败 —— 与"读到了但不是直连"同等对待, 一律 fail-closed。
    Unknown,
    /// 声明出来的能力缺失 (适配器自己说 `path_aware=false`), 不是"没读到"。
    NotApplicable,
}

impl PathClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// 门禁结论。`Refused` 带的是 §2.2 稳定码总表里登记过的码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BulkDecision {
    Allowed,
    Refused(&'static str),
}

pub(crate) const IROH_DIRECT_PATH_UNAVAILABLE: &str = "iroh_direct_path_unavailable";
pub(crate) const IROH_DIRECT_PATH_LOST: &str = "iroh_direct_path_lost";
pub(crate) const IROH_PATH_UNKNOWN: &str = "iroh_path_unknown";
pub(crate) const IROH_BULK_PATH_PIN_UNAVAILABLE: &str = "iroh_bulk_path_pin_unavailable";

/// 判定表 (§3.1 的穷举实现)。
///
/// `bulk_path_pin` 那一行必须**先于**路径类别判: 钉不住的传输上, 采样到 `direct` 也不代表后续
/// 排队/重传的字节不会改走 relay —— 那是应用层采样证明不了的部分 (§3.5)。
pub(crate) fn decide(
    capability: PathCapability,
    path_class: PathClass,
    established_direct: bool,
) -> BulkDecision {
    if !capability.path_aware {
        return BulkDecision::Allowed;
    }
    if !capability.bulk_path_pin {
        return BulkDecision::Refused(IROH_BULK_PATH_PIN_UNAVAILABLE);
    }
    match path_class {
        PathClass::Direct | PathClass::NotApplicable => BulkDecision::Allowed,
        PathClass::Relay => {
            if established_direct {
                // 曾经直连、中途丢: 已收字节仍有效, 调用方按 §2.5 B3 保留半成品后重试。
                BulkDecision::Refused(IROH_DIRECT_PATH_LOST)
            } else {
                BulkDecision::Refused(IROH_DIRECT_PATH_UNAVAILABLE)
            }
        }
        PathClass::Unknown => BulkDecision::Refused(IROH_PATH_UNKNOWN),
    }
}

/// 一次 bulk 尝试在**一端**采集到的路径与字节证据 (`rdog.bulk-path.v1`)。
///
/// 两端各发自己那一半, 由验收工具 join; **任何一端都不得替对端填数** —— 所以这个结构里只有
/// 本端测到的量, 没有"对端字段"。
#[derive(Debug, Clone)]
pub(crate) struct BulkPathSnapshot {
    pub(crate) attempt_id: String,
    pub(crate) transfer_id: String,
    pub(crate) direction: &'static str,
    pub(crate) capability: PathCapability,
    pub(crate) path_class: PathClass,
    pub(crate) direct_addr: Option<String>,
    pub(crate) bulk_bytes: u64,
    pub(crate) wire_bytes: u64,
    pub(crate) chunks: u64,
    pub(crate) concurrency_suspected: bool,
    pub(crate) endpoint: &'static str,
    pub(crate) relay_tx_before: u64,
    pub(crate) relay_tx_after: u64,
    pub(crate) relay_rx_before: u64,
    pub(crate) relay_rx_after: u64,
    pub(crate) aborted: bool,
}

impl BulkPathSnapshot {
    /// 本端的 relay 字节归属 = `after - before` (括号采样, §4.2)。
    pub(crate) fn relay_tx_delta(&self) -> u64 {
        self.relay_tx_after.saturating_sub(self.relay_tx_before)
    }

    pub(crate) fn relay_rx_delta(&self) -> u64 {
        self.relay_rx_after.saturating_sub(self.relay_rx_before)
    }

    /// `relay_bytes_zero` 的判定式 (§4.3) —— **逐字可执行**, 不做主观解读。
    ///
    /// 三个必要条件缺一不可:
    /// 1. `path_class == "direct"` —— `not_applicable` 下这个判定不适用 (空真), `relay` / `unknown` 直接否;
    /// 2. 本端 `relay_tx` delta 为 0;
    /// 3. 本端 `relay_rx` delta 为 0。
    ///
    /// 单端只能证自己那一半, 所以这个函数**不是**"整个请求 relay 字节为零"的结论 —— 结论要两端
    /// 各自的 `relay_bytes_zero_own_half()` 都为真才能下 (§4.3 要求 join)。
    pub(crate) fn relay_bytes_zero_own_half(&self) -> bool {
        !self.aborted
            && self.path_class == PathClass::Direct
            && self.relay_tx_delta() == 0
            && self.relay_rx_delta() == 0
    }

    /// 快照里的 `verdict` 字段。
    pub(crate) fn verdict(&self) -> &'static str {
        if self.aborted {
            return "aborted";
        }
        if self.path_class == PathClass::NotApplicable {
            // 不适用, 不是"通过": 没有 relay 跳的链路上"relay 字节为 0"是空真 (§2.1)。
            return "not_applicable";
        }
        if self.relay_bytes_zero_own_half() {
            "relay_bytes_zero"
        } else {
            "relay_bytes_nonzero"
        }
    }

    /// 正对照是否成立 (§4.4): 同链路控制面请求的 relay delta 必须**严格大于 0**。
    ///
    /// 严格不等式是刻意的: `>= 0` 在计数器根本没接上时照样通过, 等于没有正对照。
    pub(crate) fn positive_control_holds(control_probe: &BulkPathSnapshot) -> bool {
        control_probe.relay_tx_delta() > 0 || control_probe.relay_rx_delta() > 0
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "schema": "rdog.bulk-path.v1",
            "attempt_id": self.attempt_id,
            "transfer_id": self.transfer_id,
            "direction": self.direction,
            "path_aware": self.capability.path_aware,
            "bulk_path_pin": self.capability.bulk_path_pin,
            "path_class": self.path_class.as_str(),
            "direct_addr": self.direct_addr,
            "bulk_bytes": self.bulk_bytes,
            "wire_bytes": self.wire_bytes,
            "chunks": self.chunks,
            "concurrency_suspected": self.concurrency_suspected,
            "endpoint": self.endpoint,
            "relay_tx_before": self.relay_tx_before,
            "relay_tx_after": self.relay_tx_after,
            "relay_rx_before": self.relay_rx_before,
            "relay_rx_after": self.relay_rx_after,
            "verdict": self.verdict(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(capability: PathCapability, path_class: PathClass) -> BulkPathSnapshot {
        BulkPathSnapshot {
            attempt_id: "attempt".into(),
            transfer_id: "transfer".into(),
            direction: "put",
            capability,
            path_class,
            direct_addr: None,
            bulk_bytes: 10,
            wire_bytes: 16,
            chunks: 1,
            concurrency_suspected: false,
            endpoint: "client",
            relay_tx_before: 0,
            relay_tx_after: 0,
            relay_rx_before: 0,
            relay_rx_after: 0,
            aborted: false,
        }
    }

    #[test]
    fn decision_table_is_exhaustive_and_fails_closed() {
        // 不路径感知: 与路径类别无关, 一律允许 (快照记 not_applicable)
        for class in [
            PathClass::Direct,
            PathClass::Relay,
            PathClass::Unknown,
            PathClass::NotApplicable,
        ] {
            assert_eq!(
                decide(PathCapability::NOT_PATH_AWARE, class, false),
                BulkDecision::Allowed
            );
        }
        // 有 relay 跳但钉不住: 采样到 direct 也必须拒
        for class in [PathClass::Direct, PathClass::Relay, PathClass::Unknown] {
            assert_eq!(
                decide(PathCapability::PATH_AWARE_UNPINNED, class, false),
                BulkDecision::Refused(IROH_BULK_PATH_PIN_UNAVAILABLE)
            );
        }
        // 钉得住: 只有 direct 放行
        assert_eq!(
            decide(PathCapability::PATH_AWARE_PINNED, PathClass::Direct, false),
            BulkDecision::Allowed
        );
        assert_eq!(
            decide(PathCapability::PATH_AWARE_PINNED, PathClass::Relay, false),
            BulkDecision::Refused(IROH_DIRECT_PATH_UNAVAILABLE)
        );
        assert_eq!(
            decide(PathCapability::PATH_AWARE_PINNED, PathClass::Relay, true),
            BulkDecision::Refused(IROH_DIRECT_PATH_LOST)
        );
        // 读不出路径类别: fail-closed, 永远不许强转 direct
        assert_eq!(
            decide(PathCapability::PATH_AWARE_PINNED, PathClass::Unknown, false),
            BulkDecision::Refused(IROH_PATH_UNKNOWN)
        );
    }

    #[test]
    fn not_applicable_is_not_a_pass() {
        // 没有 relay 跳的链路上"relay 字节为 0"是空真: 判定不适用, 不能当成通过。
        let value = snapshot(PathCapability::NOT_PATH_AWARE, PathClass::NotApplicable);
        assert_eq!(value.verdict(), "not_applicable");
        assert!(!value.relay_bytes_zero_own_half());
    }

    #[test]
    fn relay_bytes_zero_requires_direct_and_both_counters_flat() {
        let mut direct = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Direct);
        assert_eq!(direct.verdict(), "relay_bytes_zero");

        direct.relay_tx_after = 1;
        assert_eq!(direct.verdict(), "relay_bytes_nonzero");
        direct.relay_tx_after = 0;

        direct.relay_rx_after = 1;
        assert_eq!(direct.verdict(), "relay_bytes_nonzero");
        direct.relay_rx_after = 0;

        // 路径不是 direct 时, 计数器再干净也不构成结论
        let relay = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Relay);
        assert_eq!(relay.verdict(), "relay_bytes_nonzero");

        let aborted = {
            let mut value = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Direct);
            value.aborted = true;
            value
        };
        assert_eq!(aborted.verdict(), "aborted");
    }

    #[test]
    fn positive_control_uses_a_strict_inequality() {
        // 全零的探针不许通过 —— 那正是"计数器根本没接上"的读数。
        let broken = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Relay);
        assert!(!BulkPathSnapshot::positive_control_holds(&broken));

        let mut healthy = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Relay);
        healthy.relay_tx_after = 42;
        assert!(BulkPathSnapshot::positive_control_holds(&healthy));

        let mut healthy_rx = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Relay);
        healthy_rx.relay_rx_after = 7;
        assert!(BulkPathSnapshot::positive_control_holds(&healthy_rx));
    }

    #[test]
    fn snapshot_json_carries_the_schema_and_both_byte_units() {
        let mut value = snapshot(PathCapability::PATH_AWARE_PINNED, PathClass::Direct);
        value.direct_addr = Some("10.0.0.2:4242".into());
        let json = value.to_json();
        assert_eq!(json["schema"], "rdog.bulk-path.v1");
        assert_eq!(json["path_class"], "direct");
        assert_eq!(json["bulk_bytes"], 10);
        assert_eq!(json["wire_bytes"], 16);
        assert_eq!(json["verdict"], "relay_bytes_zero");
        assert_eq!(json["endpoint"], "client");
    }
}
