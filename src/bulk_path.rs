//! bulk 的直连强制判定与请求级证据快照
//! (specs/rdog-bulk-direct-only-spec.md §3 判定表 / §4 观测口径)。
//!
//! # 三格的语义 (这里就是定义, 别处只引用)
//!
//! | 字段 | 是谁的事实 | 读它的层 | 读到时的动作 |
//! | --- | --- | --- | --- |
//! | `path_aware` | **传输适配器**的拓扑属性: 这条链路上到底有没有 relay 跳 | 门禁 (`decide`) 与快照 | `false` ⇒ 路径判定不适用, bulk **允许**, 快照记 `not_applicable` |
//! | `bulk_path_pin` | **传输层**能否把 bulk 连接钉在直连路径上 (禁止迁移、排队与重传不得改走 relay) | 门禁 | 与下面一格合起来才决定"该传输能不能承载 bulk" |
//! | `bulk_path_checked_per_chunk` | **应用层**会不会在每次 chunk 边界采样路径, 读到非 `direct` 即拒 | 门禁 | **不等于** pin: 采样看不到排队与重传, 也禁不了迁移 (§3.5) |
//! | `path_class` | **本次尝试此刻**观测到的路径 (`direct`/`relay`/`unknown`) | 门禁 (open 与每个 chunk 边界) | `direct` 放行; `relay` 拒 (`iroh_direct_path_unavailable`); 读不出 ⇒ `unknown` ⇒ **拒** (`iroh_path_unknown`, fail-closed) |
//!
//! 四格互相不能顶替: `path_aware=false` 说的是"拓扑里没有 relay 跳"; `bulk_path_pin=false &&
//! bulk_path_checked_per_chunk=false` 说的是"有 relay 跳, 既钉不住也不查"; 而只做了逐 chunk 检查
//! (`checked_per_chunk=true` / `pin=false`) 说的是"每次边界都查, 但边界之间仍可能改走 relay"。
//! 把最后一种当第一种, 就得到一条"看起来有门禁、其实只在边界上有"的路径; 当第二种, 则白扔了它
//! 确实做到的那一半。
//!
//! `not_applicable` 是**声明出来的能力缺失**, 不是"没读到"; 后者一律归 `unknown` 并 fail-closed。

use serde_json::{json, Value};

/// 一条链路的路径能力 (由传输适配器声明, 不由配置推断)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PathCapability {
    pub(crate) path_aware: bool,
    /// 传输层钉住: 禁止迁移, 排队与重传的字节不得改走 relay (§3.5)。
    pub(crate) bulk_path_pin: bool,
    /// 应用层逐 chunk 检查: 每次 chunk 边界采样, 读到非 `direct` 即拒。
    ///
    /// **这不是 pin 的替代品**, 是独立的一格: 边界之间的排队与重传它看不到 (§3.5)。
    pub(crate) bulk_path_checked_per_chunk: bool,
}

impl PathCapability {
    /// 拓扑里没有 relay 跳 (TCP / WebSocket / Zenoh): 路径判定不适用。
    pub(crate) const NOT_PATH_AWARE: Self = Self {
        path_aware: false,
        bulk_path_pin: false,
        bulk_path_checked_per_chunk: false,
    };

    /// 有 relay 跳, 但**既钉不住也不逐 chunk 检查**: 该传输不得承载 bulk (该拒绝, 而不是
    /// "大致直连")。
    ///
    /// 今天**没有任何适配器处于这个状态**: 未开准入策略的 iroh lane 用它来表达"这条入口
    /// 不承载 bulk 数据面"(RAI-476 裁定 F), 其余传输没有 relay 跳。
    pub(crate) const PATH_AWARE_UNPINNED: Self = Self {
        path_aware: true,
        bulk_path_pin: false,
        bulk_path_checked_per_chunk: false,
    };

    /// 有 relay 跳, 传输层**钉不住**, 但应用层在 open 与每个 chunk 边界都检查一次路径
    /// (读到非 `direct` 即拒)。
    ///
    /// 这是 iroh 准入 lane 今天的形态 (RAI-476 裁定 A)。它守住的是"每个边界上路径是直连",
    /// **不包括**"边界之间不会改走 relay": 传输层 pin 是另一件事, 本版没做
    /// (`iroh_bulk_path_pin_unavailable` 留给真做 pin 的那天)。
    pub(crate) const PATH_AWARE_CHECKED_PER_CHUNK: Self = Self {
        path_aware: true,
        bulk_path_pin: false,
        bulk_path_checked_per_chunk: true,
    };

    /// 有 relay 跳且**传输层**能钉住 —— 今天没有任何适配器处于这个状态, 所以它只在测试里构造。
    /// 适配器真补上 pin 的那天它会成为生产路径上的第四种能力。
    #[cfg(test)]
    pub(crate) const PATH_AWARE_PINNED: Self = Self {
        path_aware: true,
        bulk_path_pin: true,
        bulk_path_checked_per_chunk: true,
    };
}

/// 一次尝试此刻观测到的路径类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathClass {
    Direct,
    /// 生产者是 iroh lane 的路径采样 (`PathSampler::Lane`); 其余传输没有 relay 跳。
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
#[cfg(test)]
pub(crate) const IROH_BULK_PATH_PIN_UNPINNED_ALIAS: &str = IROH_BULK_PATH_PIN_UNAVAILABLE;

/// 判定表 (§3.1 的穷举实现)。
///
/// 第一道判据必须**先于**路径类别判: 既钉不住、又不逐 chunk 检查的传输上, 采样到 `direct`
/// 也不代表后续排队/重传的字节不会改走 relay —— 那是应用层采样证明不了的部分 (§3.5), 而且
/// 它连边界都没查。
pub(crate) fn decide(
    capability: PathCapability,
    path_class: PathClass,
    established_direct: bool,
) -> BulkDecision {
    if !capability.path_aware {
        return BulkDecision::Allowed;
    }
    if !capability.bulk_path_pin && !capability.bulk_path_checked_per_chunk {
        return BulkDecision::Refused(IROH_BULK_PATH_PIN_UNAVAILABLE);
    }
    match path_class {
        PathClass::NotApplicable => {
            // `not_applicable` 是 `path_aware=false` 的**后果**, 不是路径感知传输上的合法输入。
            // 走到这里说明"读不出被错标成了不适用" —— 按 fail-closed 拒, 不借它放行。
            BulkDecision::Refused(IROH_PATH_UNKNOWN)
        }
        PathClass::Direct => BulkDecision::Allowed,
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
    /// `null` = 本端没有检测并发流量的能力 (今天所有端都是), 不是"没有并发"。
    pub(crate) concurrency_suspected: Option<bool>,
    pub(crate) endpoint: &'static str,
    /// 四个计数都是 `Option`: `None` = **本端没有 relay 句柄可读**, `Some(0)` = 真的读到了 0。
    ///
    /// 这一层类型区分是刻意的。用 `u64` 加一个 `relay_sample_available: bool` 也"能表达",
    /// 但它把"未采样"与"测到 0"交给调用方记得同时设两个字段; 类型分开以后, 想写出一个
    /// "没采到却看起来像零"的快照, 得先显式填一个 0 —— 那是主动说谎, 不是疏忽。
    pub(crate) relay_tx_before: Option<u64>,
    pub(crate) relay_tx_after: Option<u64>,
    pub(crate) relay_rx_before: Option<u64>,
    pub(crate) relay_rx_after: Option<u64>,
    pub(crate) aborted: bool,
}

impl BulkPathSnapshot {
    /// 本端的 relay 字节归属 = `after - before` (括号采样, §4.2)。
    ///
    /// 任一端计数缺失就是 `None` —— "不知道", 不是 0。
    pub(crate) fn relay_tx_delta(&self) -> Option<u64> {
        Some(self.relay_tx_after?.saturating_sub(self.relay_tx_before?))
    }

    pub(crate) fn relay_rx_delta(&self) -> Option<u64> {
        Some(self.relay_rx_after?.saturating_sub(self.relay_rx_before?))
    }

    /// 本次尝试到底搬没搬字节。
    ///
    /// 这条是防**最纯的空真**: 一次"续传时半成品已经完整"的尝试一个字节都不用发, 计数当然是零,
    /// 而它会被读成"bulk 没走 relay"的正面证据 —— 不需要任何失败路径参与。
    pub(crate) fn moved_bytes(&self) -> bool {
        self.chunks > 0 || self.bulk_bytes > 0
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
            && self.moved_bytes()
            && self.path_class == PathClass::Direct
            && self.relay_tx_delta() == Some(0)
            && self.relay_rx_delta() == Some(0)
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
        if self.relay_tx_delta().is_none() || self.relay_rx_delta().is_none() {
            // 采不到计数: 既不能报零, 也不能报非零 —— 这是一条"无结论"。
            return "relay_sample_unavailable";
        }
        if !self.moved_bytes() {
            // 一个字节都没搬 (续传已完成等): 这份快照不构成"bulk 没走 relay"的证据。
            return "no_bulk_bytes";
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
    ///
    /// **今天没有生产消费者**: 正对照由验收脚本 (与跑批单) 执行, 仓库里只有它的判据与用例。
    /// 所以这里 `cfg(test)`: 一个只有测试能走到的"能力"会让读者以为它已经接上了 —— 等 RAI-476
    /// 的计数接线之后去掉这行, 它会成为验收工具的一部分。
    #[cfg(test)]
    pub(crate) fn positive_control_holds(control_probe: &BulkPathSnapshot) -> bool {
        control_probe.relay_tx_delta().unwrap_or(0) > 0
            || control_probe.relay_rx_delta().unwrap_or(0) > 0
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "schema": "rdog.bulk-path.v1",
            "attempt_id": self.attempt_id,
            "transfer_id": self.transfer_id,
            "direction": self.direction,
            "path_aware": self.capability.path_aware,
            "bulk_path_pin": self.capability.bulk_path_pin,
            "bulk_path_checked_per_chunk": self.capability.bulk_path_checked_per_chunk,
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
            // null = 未采样。四个计数缺一端就是 null, 验收工具必须把它读成"不知道"。
            "relay_sample_available": self.relay_tx_delta().is_some() && self.relay_rx_delta().is_some(),
            "aborted": self.aborted,
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
            concurrency_suspected: None,
            endpoint: "client",
            relay_tx_before: Some(0),
            relay_tx_after: Some(0),
            relay_rx_before: Some(0),
            relay_rx_after: Some(0),
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
        // 有 relay 跳但既不钉也不查: 采样到 direct 也必须拒
        for class in [PathClass::Direct, PathClass::Relay, PathClass::Unknown] {
            assert_eq!(
                decide(PathCapability::PATH_AWARE_UNPINNED, class, false),
                BulkDecision::Refused(IROH_BULK_PATH_PIN_UNAVAILABLE)
            );
        }
        // 逐 chunk 检查 (钉不住) 与传输层钉住, 路径判定必须完全一致: 差别在那两条 pin 义务上
        // (§3.5), 不在判定表里 —— 两个能力都按 direct/relay/unknown 分支。
        for capability in [
            PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
            PathCapability::PATH_AWARE_PINNED,
        ] {
            assert_eq!(
                decide(capability, PathClass::Direct, false),
                BulkDecision::Allowed
            );
            assert_eq!(
                decide(capability, PathClass::Relay, false),
                BulkDecision::Refused(IROH_DIRECT_PATH_UNAVAILABLE)
            );
            assert_eq!(
                decide(capability, PathClass::Relay, true),
                BulkDecision::Refused(IROH_DIRECT_PATH_LOST)
            );
            // 读不出路径类别: fail-closed, 永远不许强转 direct
            assert_eq!(
                decide(capability, PathClass::Unknown, false),
                BulkDecision::Refused(IROH_PATH_UNKNOWN)
            );
        }
        // 逐 chunk 检查 ≠ 钉住: 能力声明本身必须把这两件事分开, 否则 §3.5 的两条义务
        // 会被一句"我们检查过了"顶替。
        assert!(
            PathCapability::PATH_AWARE_CHECKED_PER_CHUNK.path_aware
                && PathCapability::PATH_AWARE_CHECKED_PER_CHUNK.bulk_path_checked_per_chunk
                && !PathCapability::PATH_AWARE_CHECKED_PER_CHUNK.bulk_path_pin
        );
        // `not_applicable` 是 `path_aware=false` 的**后果**, 在路径感知传输上不是合法输入:
        // 它只可能是"读不出被错标", 同样必须拒。这一格以前被漏在"穷举"之外。
        for capability in [
            PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
            PathCapability::PATH_AWARE_PINNED,
        ] {
            assert_eq!(
                decide(capability, PathClass::NotApplicable, false),
                BulkDecision::Refused(IROH_PATH_UNKNOWN)
            );
        }
        assert_eq!(
            decide(
                PathCapability::PATH_AWARE_UNPINNED,
                PathClass::NotApplicable,
                false
            ),
            BulkDecision::Refused(IROH_BULK_PATH_PIN_UNAVAILABLE)
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
        let mut direct = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        assert_eq!(direct.verdict(), "relay_bytes_zero");

        direct.relay_tx_after = Some(1);
        assert_eq!(direct.verdict(), "relay_bytes_nonzero");
        direct.relay_tx_after = Some(0);

        direct.relay_rx_after = Some(1);
        assert_eq!(direct.verdict(), "relay_bytes_nonzero");
        direct.relay_rx_after = Some(0);

        // 路径不是 direct 时, 计数器再干净也不构成结论
        let relay = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Relay);
        assert_eq!(relay.verdict(), "relay_bytes_nonzero");

        let aborted = {
            let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
            value.aborted = true;
            value
        };
        assert_eq!(aborted.verdict(), "aborted");
    }

    #[test]
    fn unsampled_counters_never_read_as_zero() {
        // 计数器没接上时 relay_tx/rx 也是 0, 与"一字节没走 relay"数值上完全一样。
        // 这一格与"永远为假"的判定式一起, 保证那种零只能读成"不知道"。
        let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        // 只有一端有计数 = 未采样: 类型上就是 None, 不是 0。
        value.relay_tx_after = None;
        assert_eq!(value.verdict(), "relay_sample_unavailable");
        assert!(!value.relay_bytes_zero_own_half());
        assert_eq!(value.to_json()["relay_sample_available"], false);
        assert_eq!(value.to_json()["relay_tx_after"], serde_json::Value::Null);

        // 采样可用 && direct && 两个 delta 都 0 时, 才是真正的零
        value.relay_tx_after = Some(0);
        assert_eq!(value.verdict(), "relay_bytes_zero");
    }

    #[test]
    fn a_transfer_that_moved_nothing_is_not_evidence() {
        // 一次"续传时半成品已经完整"的尝试一个字节都不用发 —— 它的 relay 计数当然是零, 而这
        // 与"bulk 没走 relay"在数值上完全一样。这条不依赖任何失败路径, 是最纯的空真。
        let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        value.chunks = 0;
        value.bulk_bytes = 0;
        value.wire_bytes = 0;
        assert!(!value.moved_bytes());
        assert_eq!(value.verdict(), "no_bulk_bytes");
        assert!(!value.relay_bytes_zero_own_half());

        // 有字节移动才算证据
        value.chunks = 1;
        value.bulk_bytes = 10;
        assert_eq!(value.verdict(), "relay_bytes_zero");
    }

    #[test]
    fn positive_control_uses_a_strict_inequality() {
        // 全零的探针不许通过 —— 那正是"计数器根本没接上"的读数。
        let broken = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Relay);
        assert!(!BulkPathSnapshot::positive_control_holds(&broken));

        let mut healthy = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Relay);
        healthy.relay_tx_after = Some(42);
        assert!(BulkPathSnapshot::positive_control_holds(&healthy));

        let mut healthy_rx = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Relay);
        healthy_rx.relay_rx_after = Some(7);
        assert!(BulkPathSnapshot::positive_control_holds(&healthy_rx));
    }

    #[test]
    fn snapshot_json_carries_the_schema_and_both_byte_units() {
        let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        value.direct_addr = Some("10.0.0.2:4242".into());
        let json = value.to_json();
        assert_eq!(json["schema"], "rdog.bulk-path.v1");
        assert_eq!(json["path_class"], "direct");
        assert_eq!(json["bulk_path_pin"], false);
        assert_eq!(json["bulk_path_checked_per_chunk"], true);
        assert_eq!(json["bulk_bytes"], 10);
        assert_eq!(json["wire_bytes"], 16);
        assert_eq!(json["verdict"], "relay_bytes_zero");
        assert_eq!(json["endpoint"], "client");
    }
}
