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

/// 单端证据快照的 schema 名 (§4.3)。写与读共用这一个常量。
pub(crate) const BULK_PATH_SCHEMA: &str = "rdog.bulk-path.v1";
/// 两端 join 之后的报告 schema 名 (§4.3 的 join 段)。
pub(crate) const BULK_PATH_JOIN_SCHEMA: &str = "rdog.bulk-path-join.v1";

// join 的原因码 (§4.3 join 段)。它们**不是**控制面稳定码: 只有验收工具与人在读,
// 不进 §2.2 的码表。命名空间仍是 `bulk_path_*`, 语义是"这两半为什么没有组成结论"。
pub(crate) const BULK_PATH_SCHEMA_MISMATCH: &str = "bulk_path_schema_mismatch";
pub(crate) const BULK_PATH_ENDPOINT_MISSING: &str = "bulk_path_endpoint_missing";
pub(crate) const BULK_PATH_ENDPOINT_DUPLICATED: &str = "bulk_path_endpoint_duplicated";
pub(crate) const BULK_PATH_ATTEMPT_MISMATCH: &str = "bulk_path_attempt_mismatch";
/// §4.2: `concurrency_suspected:true` 的那一半**不得单独用于**"relay 字节为 0"的结论。
pub(crate) const BULK_PATH_CONCURRENCY_SUSPECTED: &str = "bulk_path_concurrency_suspected";
/// 两端配上了、但没有任何一端给出可用结论时的前缀 (后面接 `<verdict>[+<verdict>]`)。
pub(crate) const BULK_PATH_INCONCLUSIVE: &str = "bulk_path_inconclusive";

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
    /// `put | get`。**类型是 `String` 而不是 `&'static str`**: join 要从 JSON 读回对端的快照,
    /// 而"写出去的形状"和"读回来的形状"是同一个结构才不会各自漂移 (见 `from_json`)。
    pub(crate) direction: String,
    pub(crate) capability: PathCapability,
    pub(crate) path_class: PathClass,
    pub(crate) direct_addr: Option<String>,
    pub(crate) bulk_bytes: u64,
    pub(crate) wire_bytes: u64,
    pub(crate) chunks: u64,
    /// `null` = 本端没有检测并发流量的能力 (今天所有端都是), 不是"没有并发"。
    pub(crate) concurrency_suspected: Option<bool>,
    /// `client | daemon` —— 这一半是谁测的。join 靠它分左右, 不靠文件顺序。
    pub(crate) endpoint: String,
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
            "schema": BULK_PATH_SCHEMA,
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
            "relay_sample_available": self.relay_sample_available(),
            "aborted": self.aborted,
            "verdict": self.verdict(),
        })
    }

    /// 从一份 `rdog.bulk-path.v1` JSON 读回快照 (join 用)。
    ///
    /// 严格读: `schema` 必须是本版, 数字字段必须是整数, **四个计数可以缺失为 `null`, 但不能缺键**
    /// —— "没有这一栏"和"这一栏写着不知道"是两件事, 前者是别人写的另一份格式。
    /// 读不出就是 `Err(原因码)`, 不许用默认值把它凑成一份看起来正常的快照。
    pub(crate) fn from_json(value: &Value) -> Result<Self, &'static str> {
        if value.get("schema").and_then(Value::as_str) != Some(BULK_PATH_SCHEMA) {
            return Err(BULK_PATH_SCHEMA_MISMATCH);
        }
        let text = |key: &str| -> Result<String, &'static str> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(BULK_PATH_SCHEMA_MISMATCH)
        };
        let flag = |key: &str| -> Result<bool, &'static str> {
            value
                .get(key)
                .and_then(Value::as_bool)
                .ok_or(BULK_PATH_SCHEMA_MISMATCH)
        };
        let count = |key: &str| -> Result<u64, &'static str> {
            value
                .get(key)
                .and_then(Value::as_u64)
                .ok_or(BULK_PATH_SCHEMA_MISMATCH)
        };
        // 键必须在, 值可以是 null —— 那是"未采样", 不是缺失。
        let optional_count = |key: &str| -> Result<Option<u64>, &'static str> {
            match value.get(key) {
                None => Err(BULK_PATH_SCHEMA_MISMATCH),
                Some(Value::Null) => Ok(None),
                Some(number) => number.as_u64().map(Some).ok_or(BULK_PATH_SCHEMA_MISMATCH),
            }
        };
        let path_class = match text("path_class")?.as_str() {            "direct" => PathClass::Direct,
            "relay" => PathClass::Relay,
            "unknown" => PathClass::Unknown,
            "not_applicable" => PathClass::NotApplicable,
            _ => return Err(BULK_PATH_SCHEMA_MISMATCH),
        };
        let endpoint = text("endpoint")?;
        if !matches!(endpoint.as_str(), "client" | "daemon") {
            return Err(BULK_PATH_SCHEMA_MISMATCH);
        }
        let direction = text("direction")?;
        if !matches!(direction.as_str(), "put" | "get") {
            return Err(BULK_PATH_SCHEMA_MISMATCH);
        }
        let snapshot = Self {
            attempt_id: text("attempt_id")?,
            transfer_id: text("transfer_id")?,
            direction,
            capability: PathCapability {
                path_aware: flag("path_aware")?,
                bulk_path_pin: flag("bulk_path_pin")?,
                bulk_path_checked_per_chunk: flag("bulk_path_checked_per_chunk")?,
            },
            path_class,
            direct_addr: match value.get("direct_addr") {
                None => return Err(BULK_PATH_SCHEMA_MISMATCH),
                Some(Value::Null) => None,
                Some(address) => Some(
                    address
                        .as_str()
                        .ok_or(BULK_PATH_SCHEMA_MISMATCH)?
                        .to_owned(),
                ),
            },
            bulk_bytes: count("bulk_bytes")?,
            wire_bytes: count("wire_bytes")?,
            chunks: count("chunks")?,
            concurrency_suspected: match value.get("concurrency_suspected") {
                None => return Err(BULK_PATH_SCHEMA_MISMATCH),
                Some(Value::Null) => None,
                Some(flag) => Some(flag.as_bool().ok_or(BULK_PATH_SCHEMA_MISMATCH)?),
            },
            endpoint,
            relay_tx_before: optional_count("relay_tx_before")?,
            relay_tx_after: optional_count("relay_tx_after")?,
            relay_rx_before: optional_count("relay_rx_before")?,
            relay_rx_after: optional_count("relay_rx_after")?,
            aborted: flag("aborted")?,
        };
        // `relay_sample_available` 是派生字段, 但被写进了文件 —— 它与四个计数不一致时,
        // 这份快照是被人改过的 (或由另一版写出来的), 不是"读不出来", 也不是"零"。
        let declared_available = flag("relay_sample_available")?;
        if declared_available != snapshot.relay_sample_available() {
            return Err(BULK_PATH_SCHEMA_MISMATCH);
        }
        Ok(snapshot)
    }

    /// 四个计数是否都采到了 —— JSON 里 `relay_sample_available` 的真相源。
    pub(crate) fn relay_sample_available(&self) -> bool {
        self.relay_tx_delta().is_some() && self.relay_rx_delta().is_some()
    }
}

/// join 报告里的一端: **只讲这一端自己测到的**, 不带对端的任何数字。
///
/// `verdict` 与 `relay_bytes_zero_own_half` 都直接取对这一半调用的结果 —— 报告不重新判一遍,
/// 否则 join 与单端判定会出现两套口径。
#[derive(Debug, Clone)]
pub(crate) struct HalfReport {
    pub(crate) endpoint: String,
    pub(crate) verdict: &'static str,
    pub(crate) relay_bytes_zero_own_half: bool,
    pub(crate) relay_sample_available: bool,
    pub(crate) path_class: &'static str,
    pub(crate) aborted: bool,
    pub(crate) concurrency_suspected: Option<bool>,
}

impl HalfReport {
    fn of(snapshot: &BulkPathSnapshot) -> Self {
        Self {
            endpoint: snapshot.endpoint.clone(),
            verdict: snapshot.verdict(),
            relay_bytes_zero_own_half: snapshot.relay_bytes_zero_own_half(),
            relay_sample_available: snapshot.relay_sample_available(),
            path_class: snapshot.path_class.as_str(),
            aborted: snapshot.aborted,
            concurrency_suspected: snapshot.concurrency_suspected,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "endpoint": self.endpoint,
            "verdict": self.verdict,
            "relay_bytes_zero_own_half": self.relay_bytes_zero_own_half,
            "relay_sample_available": self.relay_sample_available,
            "path_class": self.path_class,
            "aborted": self.aborted,
            "concurrency_suspected": self.concurrency_suspected,
        })
    }
}

/// 同一个 `transfer_id` 上两端快照的合订本 (`rdog.bulk-path-join.v1`)。
///
/// `relay_bytes_zero` 是**唯一允许的跨端结论**, 且只在 `status == "joined"` 时非 `null`:
/// `relay_bytes_zero` 是**三态**, 不是布尔:
///
/// | 值 | 什么时候 | 读法 |
/// | --- | --- | --- |
/// | `true` | 两端 `relay_bytes_zero_own_half()` 都为真 | 两端各证了自己那一半 |
/// | `false` | **至少一端** `verdict() == "relay_bytes_nonzero"` | 真的测到了 relay 上有字节 |
/// | `null` | 其余全部 (任一端 `no_bulk_bytes` / `relay_sample_unavailable` / `not_applicable` / `aborted`) | **没证出来**, 不是证否 |
///
/// 把第三格压成 `false` 是我们把"未采样 ≠ 0"做对之后**反向**漏掉的同一类谎: 读到 `false` 的人会以为
/// "测到了 relay 流量", 而真相是"这次没有结论"。没组成结论 (缺一端 / schema 不符) 同样是 `null`,
/// 但那种情况另有 `status:"unknown"` 区分 —— 两种 `null` 不是一件事。
#[derive(Debug, Clone)]
pub(crate) struct JoinReport {
    pub(crate) transfer_id: Option<String>,
    pub(crate) attempt_id: Option<String>,
    pub(crate) status: &'static str,
    /// 固定码 (`bulk_path_*`) 或 `joined` 且无结论时拼出来的 `bulk_path_inconclusive_<verdict>[+<verdict>]`。
    pub(crate) reason: Option<String>,
    pub(crate) client: Option<HalfReport>,
    pub(crate) daemon: Option<HalfReport>,
    pub(crate) relay_bytes_zero: Option<bool>,
}

impl JoinReport {
    fn unknown(transfer_id: Option<String>, reason: &'static str) -> Self {
        Self {
            transfer_id,
            attempt_id: None,
            status: "unknown",
            reason: Some(reason.to_owned()),
            client: None,
            daemon: None,
            relay_bytes_zero: None,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "schema": BULK_PATH_JOIN_SCHEMA,
            "transfer_id": self.transfer_id,
            "attempt_id": self.attempt_id,
            "status": self.status,
            "reason": self.reason,
            "client": self.client.as_ref().map(HalfReport::to_json),
            "daemon": self.daemon.as_ref().map(HalfReport::to_json),
            "relay_bytes_zero": self.relay_bytes_zero,
        })
    }
}

/// 把一组 `rdog.bulk-path.v1` 快照按 `transfer_id` 配对。
///
/// 配对**只按 `transfer_id`**: 左右由每一半自己的 `endpoint` 决定, 不看文件顺序、不看文件名 ——
/// 按顺序配会在"某人少给一个文件"时把两份客户端快照当成两端, 而那种错配恰好能凑出一个
/// 看起来完整的结论。
///
/// 任一端缺失、`attempt_id` 不一致、schema 不符、两端自称同一 `endpoint`, 都写 `unknown` + 原因码;
/// **绝不拿一端的数替另一端填** —— 只有 `status == "joined"` 的报告里才会出现跨端结论。
pub(crate) fn join_inputs(values: &[Value]) -> Vec<JoinReport> {
    let mut reports: Vec<JoinReport> = Vec::new();
    let mut groups: Vec<(String, Vec<BulkPathSnapshot>)> = Vec::new();
    for value in values {
        match BulkPathSnapshot::from_json(value) {
            Ok(snapshot) => {
                match groups.iter_mut().find(|(id, _)| *id == snapshot.transfer_id) {
                    Some((_, halves)) => halves.push(snapshot),
                    None => groups.push((snapshot.transfer_id.clone(), vec![snapshot])),
                }
            }
            Err(reason) => {
                // 读不出 schema 就不知道它属于哪一次尝试: 单独立一条 unknown, 用它自己声明的
                // `transfer_id` (若有) 标身份, 但**不并进**那个 id 的正常分组 —— 一份坏文件
                // 不能改变另两份的配对结果。
                reports.push(JoinReport::unknown(
                    value
                        .get("transfer_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    reason,
                ));
            }
        }
    }

    for (transfer_id, halves) in groups {
        reports.push(join_group(transfer_id, halves));
    }
    reports
}

fn join_group(transfer_id: String, halves: Vec<BulkPathSnapshot>) -> JoinReport {
    let mut client = None;
    let mut daemon = None;
    for half in halves {
        let slot = if half.endpoint == "client" {
            &mut client
        } else {
            &mut daemon
        };
        if slot.is_some() {
            return JoinReport::unknown(Some(transfer_id), BULK_PATH_ENDPOINT_DUPLICATED);
        }
        *slot = Some(half);
    }
    let (client, daemon) = match (client, daemon) {
        (Some(client), Some(daemon)) => (client, daemon),
        (client, daemon) => {
            let mut report = JoinReport::unknown(Some(transfer_id), BULK_PATH_ENDPOINT_MISSING);
            report.client = client.as_ref().map(HalfReport::of);
            report.daemon = daemon.as_ref().map(HalfReport::of);
            return report;
        }
    };
    if client.attempt_id != daemon.attempt_id {
        let mut report = JoinReport::unknown(Some(transfer_id), BULK_PATH_ATTEMPT_MISMATCH);
        report.client = Some(HalfReport::of(&client));
        report.daemon = Some(HalfReport::of(&daemon));
        return report;
    }

    let suspect = client.concurrency_suspected == Some(true)
        || daemon.concurrency_suspected == Some(true);
    let (relay_bytes_zero, reason) = if suspect {
        // §4.2: 有并发嫌疑的那条证据不得单独用于结论。
        (None, Some(BULK_PATH_CONCURRENCY_SUSPECTED.to_owned()))
    } else if client.relay_bytes_zero_own_half() && daemon.relay_bytes_zero_own_half() {
        // §4.3 的判定式, 逐字: 两端各自的 `relay_bytes_zero_own_half()` 都为真。
        (Some(true), None)
    } else if client.verdict() == "relay_bytes_nonzero" || daemon.verdict() == "relay_bytes_nonzero" {
        // 只有"真的量到非零"才是 `false`。其余一律没结论 —— 见 `JoinReport` 的三态表。
        (Some(false), None)
    } else {
        (None, Some(inconclusive_reason(&client, &daemon)))
    };
    JoinReport {
        transfer_id: Some(transfer_id),
        attempt_id: Some(client.attempt_id.clone()),
        status: "joined",
        reason,
        client: Some(HalfReport::of(&client)),
        daemon: Some(HalfReport::of(&daemon)),
        relay_bytes_zero,
    }
}

/// "配上了但没有结论"的原因码: `bulk_path_inconclusive_<verdict>`, 两端都给时按本端 → 对端用
/// `+` 串起来 (两端的 verdict 不同才需要)。它是**新的命名空间**, 不复用 `unknown` 那套码 ——
/// "没配上"和"配上了没结论"是两件事, 混用会让排障的人先去找缺失的文件。
fn inconclusive_reason(client: &BulkPathSnapshot, daemon: &BulkPathSnapshot) -> String {
    let mut verdicts: Vec<&'static str> = Vec::new();
    for verdict in [client.verdict(), daemon.verdict()] {
        // 自己那一半已经证到零的, 不构成"这条 join 没结论"的理由。
        if verdict == "relay_bytes_zero" || verdicts.contains(&verdict) {
            continue;
        }
        verdicts.push(verdict);
    }
    format!("{BULK_PATH_INCONCLUSIVE}_{}", verdicts.join("+"))
}

/// 读一组快照文件/目录, 按 `transfer_id` join, 打印报告并返回退出码。
///
/// 退出码: 0 = 每个分组都组成了结论 (joined 且 `relay_bytes_zero` 非 `null`); 1 = 至少一组没有结论
/// (缺一端 / schema 不符 / attempt 不一致 / 并发可疑); 2 由调用方在参数或 IO 错误时用。
pub(crate) fn run_join(inputs: &[std::path::PathBuf]) -> Result<i32, String> {
    let files = collect_snapshot_files(inputs)?;
    if files.is_empty() {
        return Err("没有找到任何 .json 快照".to_owned());
    }
    let mut values = Vec::with_capacity(files.len());
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|err| format!("读 {} 失败: {err}", file.display()))?;
        let value = serde_json::from_str::<Value>(&text)
            .map_err(|err| format!("{} 不是 JSON: {err}", file.display()))?;
        values.push(value);
    }
    let reports = join_inputs(&values);
    let conclusive = reports
        .iter()
        .all(|report| report.status == "joined" && report.relay_bytes_zero.is_some());
    let rendered = json!({
        "schema": BULK_PATH_JOIN_SCHEMA,
        "reports": reports.iter().map(JoinReport::to_json).collect::<Vec<_>>(),
    });
    println!("{rendered}");
    Ok(if conclusive { 0 } else { 1 })
}

/// 输入可以是文件, 也可以是目录; 目录只取 `*.json` (按名字排序, 让输出可重现)。
fn collect_snapshot_files(
    inputs: &[std::path::PathBuf],
) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    for input in inputs {
        let metadata = std::fs::metadata(input)
            .map_err(|err| format!("{} 不存在或不可读: {err}", input.display()))?;
        if metadata.is_dir() {
            let mut entries = std::fs::read_dir(input)
                .map_err(|err| format!("读目录 {} 失败: {err}", input.display()))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect::<Vec<_>>();
            entries.sort();
            files.extend(entries);
        } else {
            files.push(input.clone());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(capability: PathCapability, path_class: PathClass) -> BulkPathSnapshot {
        BulkPathSnapshot {
            attempt_id: "attempt".into(),
            transfer_id: "transfer".into(),
            direction: "put".into(),
            capability,
            path_class,
            direct_addr: None,
            bulk_bytes: 10,
            wire_bytes: 16,
            chunks: 1,
            concurrency_suspected: None,
            endpoint: "client".into(),
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

    /// 一份可以改成"另外一端"的快照: 客户端与 daemon 的差别只有 `endpoint` 与计数。
    fn half(endpoint: &str, relay_tx_after: Option<u64>) -> Value {
        let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        value.endpoint = endpoint.to_owned();
        value.relay_tx_after = relay_tx_after;
        value.to_json()
    }

    #[test]
    fn snapshot_json_round_trips_through_from_json() {
        let mut original = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        original.direct_addr = Some("10.0.0.2:4242".into());
        original.endpoint = "daemon".to_owned();
        let read_back = BulkPathSnapshot::from_json(&original.to_json())
            .expect("自己写出去的快照必须能读回来");
        assert_eq!(read_back.verdict(), original.verdict());
        assert_eq!(read_back.endpoint, "daemon");
        assert_eq!(read_back.direct_addr.as_deref(), Some("10.0.0.2:4242"));
        assert_eq!(read_back.transfer_id, original.transfer_id);
    }

    #[test]
    fn a_snapshot_of_another_shape_is_not_read_as_evidence() {
        // schema 不对 -> 不是本版; 计数缺键 -> 不是"未采样"而是"另一份格式"。
        let mut wrong_schema = half("client", Some(0));
        wrong_schema["schema"] = json!("rdog.bulk-path.v0");
        assert_eq!(
            BulkPathSnapshot::from_json(&wrong_schema).err(),
            Some(BULK_PATH_SCHEMA_MISMATCH)
        );

        let mut missing_counter = half("client", Some(0));
        missing_counter
            .as_object_mut()
            .expect("object")
            .remove("relay_rx_after");
        assert_eq!(
            BulkPathSnapshot::from_json(&missing_counter).err(),
            Some(BULK_PATH_SCHEMA_MISMATCH)
        );

        // `relay_sample_available` 与四个计数不一致: 被人改过, 不是零。
        let mut edited = half("client", None);
        edited["relay_sample_available"] = json!(true);
        assert_eq!(
            BulkPathSnapshot::from_json(&edited).err(),
            Some(BULK_PATH_SCHEMA_MISMATCH)
        );

        // 端点只认 client / daemon: 拿它当左右键, 就不能有第三种拼写。
        let mut foreign = half("middlebox", Some(0));
        foreign["endpoint"] = json!("middlebox");
        assert_eq!(
            BulkPathSnapshot::from_json(&foreign).err(),
            Some(BULK_PATH_SCHEMA_MISMATCH)
        );
    }

    #[test]
    fn join_needs_both_halves_and_never_fills_one_from_the_other() {
        // 只有客户端一半: 结论是 unknown, 且 daemon 那一栏是 null —— 不是拿客户端的数填过去。
        let reports = join_inputs(&[half("client", Some(0))]);
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.status, "unknown");
        assert_eq!(report.reason.as_deref(), Some(BULK_PATH_ENDPOINT_MISSING));
        assert_eq!(report.relay_bytes_zero, None);
        assert!(report.client.is_some(), "给了的那一半要照实列出来");
        assert!(report.daemon.is_none(), "没给的那一半必须是 null");
        let rendered = report.to_json();
        assert_eq!(rendered["schema"], BULK_PATH_JOIN_SCHEMA);
        assert_eq!(rendered["daemon"], Value::Null);
        assert_eq!(rendered["relay_bytes_zero"], Value::Null);
    }

    #[test]
    fn join_requires_the_same_attempt_id_on_both_ends() {
        let client = half("client", Some(0));
        let mut daemon = half("daemon", Some(0));
        daemon["attempt_id"] = json!("another-attempt");

        let reports = join_inputs(&[client, daemon]);
        assert_eq!(reports[0].status, "unknown");
        assert_eq!(
            reports[0].reason.as_deref(),
            Some(BULK_PATH_ATTEMPT_MISMATCH)
        );
        assert_eq!(reports[0].relay_bytes_zero, None);
        // 两半仍然各自列出: 它们确实存在, 只是不能组成一个结论。
        assert!(reports[0].client.is_some() && reports[0].daemon.is_some());
    }

    #[test]
    fn join_concludes_zero_only_when_both_halves_carry_their_own_zero() {
        let reports = join_inputs(&[half("client", Some(0)), half("daemon", Some(0))]);
        let report = &reports[0];
        assert_eq!(report.status, "joined");
        assert_eq!(report.reason, None);
        assert_eq!(report.relay_bytes_zero, Some(true));
        assert!(report
            .client
            .as_ref()
            .is_some_and(|half| half.relay_bytes_zero_own_half));
        assert!(report
            .daemon
            .as_ref()
            .is_some_and(|half| half.relay_bytes_zero_own_half));
    }

    #[test]
    fn one_half_with_unsampled_counters_voids_the_cross_end_conclusion() {
        // daemon 一半没采到计数: 它自己的 verdict 是 `relay_sample_unavailable`。跨端结论必须是
        // **`null`** —— "两端都证了零"不成立, 而"一端没采样"不是"测到非零"。
        let reports = join_inputs(&[half("client", Some(0)), half("daemon", None)]);
        let report = &reports[0];
        assert_eq!(report.status, "joined");
        assert_eq!(report.relay_bytes_zero, None);
        assert_eq!(
            report.reason.as_deref(),
            Some("bulk_path_inconclusive_relay_sample_unavailable")
        );
        let daemon = report.daemon.as_ref().expect("daemon half");
        assert_eq!(daemon.verdict, "relay_sample_unavailable");
        assert!(!daemon.relay_sample_available);
    }

    #[test]
    fn two_empty_halves_are_a_pair_without_a_conclusion() {
        // 两端都是空传输: 传输本身成功 (由正常回执表达), 但路径结论被扣掉 —— `null`, 不是 `false`。
        let empty = |endpoint: &str| {
            let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
            value.endpoint = endpoint.to_owned();
            value.chunks = 0;
            value.bulk_bytes = 0;
            value.wire_bytes = 0;
            value.to_json()
        };
        let reports = join_inputs(&[empty("client"), empty("daemon")]);
        let report = &reports[0];
        assert_eq!(report.status, "joined");
        assert_eq!(report.relay_bytes_zero, None);
        assert_eq!(
            report.reason.as_deref(),
            Some("bulk_path_inconclusive_no_bulk_bytes")
        );
        // 两端的 verdict 相同, 原因码里就不重复列两次。
        assert_eq!(report.reason.as_deref().expect("reason").matches('+').count(), 0);
    }

    #[test]
    fn an_aborted_half_makes_the_join_inconclusive_not_negative() {
        let mut aborted = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        aborted.endpoint = "daemon".to_owned();
        aborted.aborted = true;
        let reports = join_inputs(&[half("client", Some(0)), aborted.to_json()]);
        let report = &reports[0];
        assert_eq!(report.status, "joined");
        assert_eq!(report.relay_bytes_zero, None, "中止不是证否");
        assert_eq!(
            report.reason.as_deref(),
            Some("bulk_path_inconclusive_aborted")
        );
    }

    #[test]
    fn two_different_inconclusive_verdicts_are_both_named() {
        // 客户端那半没搬字节, daemon 那半没采到计数: 两个原因都要出现在码里, 别只报一个。
        let mut client = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
        client.chunks = 0;
        client.bulk_bytes = 0;
        client.wire_bytes = 0;
        let daemon = {
            let mut value = snapshot(PathCapability::PATH_AWARE_CHECKED_PER_CHUNK, PathClass::Direct);
            value.endpoint = "daemon".to_owned();
            value.relay_tx_after = None;
            value
        };
        let reports = join_inputs(&[client.to_json(), daemon.to_json()]);
        assert_eq!(reports[0].relay_bytes_zero, None);
        assert_eq!(
            reports[0].reason.as_deref(),
            Some("bulk_path_inconclusive_no_bulk_bytes+relay_sample_unavailable")
        );
    }

    #[test]
    fn a_relay_hit_on_either_end_is_not_a_zero() {
        let reports = join_inputs(&[half("client", Some(0)), half("daemon", Some(9))]);
        assert_eq!(reports[0].relay_bytes_zero, Some(false));
        assert_eq!(
            reports[0].daemon.as_ref().expect("daemon half").verdict,
            "relay_bytes_nonzero"
        );
    }

    #[test]
    fn join_groups_by_transfer_id_and_keeps_bad_files_out_of_good_groups() {
        let mut other = half("client", Some(0));
        other["transfer_id"] = json!("another-transfer");
        let mut broken = half("daemon", Some(0));
        broken["schema"] = json!("rdog.bulk-path.v0");
        broken["transfer_id"] = json!("transfer");

        let reports = join_inputs(&[half("client", Some(0)), half("daemon", Some(0)), other, broken]);
        // 两条分组各一条, 坏文件单独立一条 —— 它不能把 "transfer" 那组的配对结果改坏。
        assert_eq!(reports.len(), 3);
        let joined = reports
            .iter()
            .find(|report| report.transfer_id.as_deref() == Some("transfer") && report.status == "joined")
            .expect("正常的那一组必须组成结论");
        assert_eq!(joined.relay_bytes_zero, Some(true));
        let unknown = reports
            .iter()
            .find(|report| report.reason.as_deref() == Some(BULK_PATH_SCHEMA_MISMATCH))
            .expect("坏文件要单独报");
        assert_eq!(unknown.status, "unknown");
    }

    #[test]
    fn two_halves_claiming_the_same_endpoint_are_not_a_pair() {
        // 两份都是 client: 左右配不上, 不能"按顺序"凑成两端。
        let reports = join_inputs(&[half("client", Some(0)), half("client", Some(0))]);
        assert_eq!(reports[0].status, "unknown");
        assert_eq!(
            reports[0].reason.as_deref(),
            Some(BULK_PATH_ENDPOINT_DUPLICATED)
        );
        assert_eq!(reports[0].relay_bytes_zero, None);
    }

    #[test]
    fn suspect_concurrency_voids_the_conclusion_but_keeps_the_halves() {
        // §4.2: `concurrency_suspected:true` 的那条证据不得单独用于结论。
        let client = half("client", Some(0));
        let mut daemon = half("daemon", Some(0));
        daemon["concurrency_suspected"] = json!(true);
        let reports = join_inputs(&[client, daemon]);
        assert_eq!(reports[0].status, "joined");
        assert_eq!(
            reports[0].reason.as_deref(),
            Some(BULK_PATH_CONCURRENCY_SUSPECTED)
        );
        assert_eq!(reports[0].relay_bytes_zero, None);
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
