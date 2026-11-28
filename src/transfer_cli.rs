//! `rdog control <target> --put / --get` 的流式驱动
//! (specs/rdog-bulk-direct-only-spec.md §1.3 G1)。
//!
//! # 为什么需要它
//!
//! 协议在 daemon 侧早就完整, 但没有任何 CLI 能驱动 bulk: 今天唯一的用法是把 chunk 塞进命令行参数
//! (撞 `E2BIG`), 而 `send_control_lines_for_invocation` 收的是预构造的 `&[String]` —— 即"整文件先进
//! 内存再发"。178 MB 的文件 base64 后约 238 MB 全部驻留, 与分块的初衷相反.
//!
//! 这里按**一次交换一行**驱动: 每次只把一个 chunk 读进内存、发出去、收回应, 所以峰值内存是一个
//! chunk 的大小, 与文件大小无关。
//!
//! # 为什么不做长连接会话
//!
//! 传输会话的身份是 `transfer_id`, 不是连接 —— daemon 侧的会话表不绑定连接。所以逐行交换在第
//! 一版里是正确且最简单的形态: 不必给四种 transport 各加一套流式会话 API。代价是每块一次建链
//! (TCP) / 一次查询 (Zenoh); 对分块传输是常数级开销, 换来的是不需要改传输层。
//!
//! # 两条出口
//!
//! - TCP / WebSocket / Zenoh: 每条请求独立建链 (`TransportExchange`), 链路上没有 relay 跳,
//!   本端证据的 `path_class` 恒为 `not_applicable` (判定不适用, 不是"通过")。
//! - iroh 准入 lane (`--transport iroh`): 一条长连接承载整次尝试 (`IrohExchange`), 每个边界都能
//!   读到路径与 relay 计数, 所以 `path_class` 是真的 `direct` / `relay` / `unknown`。
//!
//! # 门禁位置
//!
//! bulk 字节从**控制端**发出, 所以"直连不可得就拒发"必须落在这里 (规格 §3.4): open 前判一次,
//! 每个 chunk 边界各再判一次, 判定用边界上的即时采样 —— 只在 open 判一次会漏掉中途
//! `direct → relay` 的切换, 那正是 `iroh_direct_path_lost` 存在的理由。

use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    bulk_path::{decide, BulkDecision, BulkPathSnapshot, PathCapability, PathClass},
    control_file_transfer::chunk_payload_bytes,
    control_invocation::{send_control_lines_for_invocation, ControlInvocation},
    control_iroh::{PathObservation, RelayBracket, RelayReading},
};

/// 一行请求 → 一行结构化回执。
///
/// 抽成 trait 的唯一理由是**可测**: 驱动是个状态机 (续传 offset、分块边界、失败即取消、下载摘要
/// 比对), 用真的协议实现 + 假传输就能把状态机测全, 不必起 daemon 也不必绑网络。
pub(crate) trait LineExchange {
    fn request(&self, line: &str) -> Result<Reply, ExchangeError>;

    /// 每块装载多少字节。默认取协议上限; 测试用小值才能在几条用例里跑完多块循环。
    fn chunk_bytes(&self) -> usize {
        chunk_payload_bytes()
    }

    /// 本端此刻的路径观测。
    ///
    /// `None` = 这条链路上没有 relay 跳 (能力声明已经是 `path_aware=false`), 判定不适用;
    /// 有 relay 跳的传输**必须**给观测, 给不出就是 `unknown` 并 fail-closed —— 两者不能混。
    fn observe_path(&self) -> Option<PathObservation> {
        None
    }
}

/// 一条控制出口的稳定身份, 用来给 `--put` 的 transfer_id 分桶 (见 `TransferIdStore`)。
///
/// 同一个本地文件发往两台不同主机是两次传输: 只按路径分桶会让第二次撞上"同 id 换目标"。
pub(crate) fn target_label(invocation: &ControlInvocation) -> String {
    match invocation {
        ControlInvocation::Tcp { host, port } => format!("tcp:{host}:{port}"),
        ControlInvocation::WebSocket { url } => format!("ws:{url}"),
        ControlInvocation::Zenoh {
            namespace,
            target_name,
            ..
        } => format!(
            "zenoh:{}/{}",
            namespace.as_deref().unwrap_or(""),
            target_name.as_deref().unwrap_or("")
        ),
        ControlInvocation::ZenohLocal { namespace } => {
            format!("zenoh-local:{}", namespace.as_deref().unwrap_or(""))
        }
    }
}

/// 一次失败的交换。
///
/// `evidence` 是这条**错误回执**上挂的 daemon 半快照 —— 门禁拒绝与中止的终态同样带证据
/// (§4.3), 在这里把它丢掉, 跨端 join 就只能落到 `endpoint_missing`。
#[derive(Debug)]
pub(crate) struct ExchangeError {
    message: String,
    evidence: Option<String>,
    /// 失败来源。重放决策只读它, 不读 `message` —— 文案会被改写, 来源不会。
    kind: FailureKind,
}

/// 一次尝试为什么失败。只用于"要不要重放"的判断, 不用于给用户看的文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureKind {
    /// 路径类别读不出 (`iroh_path_unknown`): 重拨本身就是修路径的手段。
    PathUnknown,
    /// 门禁明确拒绝 (走 relay / 钉不住): 换个时刻重拨也不该放行, 不重拨。
    PathRefused,
    /// 读回执超时、那一刻连接仍开着: 对端可能已执行, 本端只是没收到答案。
    AckUnansweredLive,
    /// 本机原因 (写/读失败、协议错、文件动作失败) 或对端已关。
    Local,
}

impl ExchangeError {
    fn without_evidence(message: String) -> Self {
        Self {
            message,
            evidence: None,
            kind: FailureKind::Local,
        }
    }

    fn with_kind(message: String, kind: FailureKind) -> Self {
        Self {
            message,
            evidence: None,
            kind,
        }
    }
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// 给 `control_iroh` 的真流回归用例用的构造器: 把一条真 lane 包成驱动能吃的交换器。
///
/// 只给测试: 生产路径由 `run_iroh_bulk` 自己建 lane 与交换器。
#[cfg(test)]
pub(crate) fn iroh_exchange(
    lane: &mut crate::control_iroh::IrohBulkLane,
    chunk_ack_timeout_ms: u64,
) -> impl LineExchange + '_ {
    IrohExchange::new(lane, chunk_ack_timeout_ms)
}

/// 一条回执: 结构化载荷 + daemon 半快照的**原文**。
///
/// `evidence` 是 daemon 在终态响应里挂的那份 `rdog.bulk-path.v1` (§4.3)。存原文而不是解析后的
/// `Value`: 跨端 join 读的是 daemon 自己那半, 重排一遍字段就等于把它换成了本端的转述。
pub(crate) struct Reply {
    payload: Value,
    evidence: Option<String>,
}

impl Reply {
    fn get(&self, key: &str) -> Option<&Value> {
        self.payload.get(key)
    }
}

/// `@response` 行 → 载荷 + daemon 半快照原文。
fn parse_reply(line: &str) -> Result<Reply, ExchangeError> {
    let payload = parse_response_payload(line)
        .ok_or_else(|| ExchangeError::without_evidence(format!("回执不可解析: {line:?}")))?;
    let evidence = raw_evidence(&payload, line);
    Ok(Reply { payload, evidence })
}

/// 回执是错误信封时: 错误文本 + daemon 半快照 (若有)。
fn error_reply(reply: Reply, error: String, kind: FailureKind) -> ExchangeError {
    ExchangeError {
        message: error,
        evidence: reply.evidence,
        kind,
    }
}

/// 稳定码 → 重放决策用的类别。读的是**结构化的 `error_code` 字段**, 不是文案: 文案会被改写,
/// 而"这条 rerun 得通不通"取决于码。
fn kind_for_code(code: Option<&str>) -> FailureKind {
    match code {
        Some(crate::bulk_path::IROH_PATH_UNKNOWN) => FailureKind::PathUnknown,
        Some(crate::bulk_path::IROH_DIRECT_PATH_UNAVAILABLE)
        | Some(crate::bulk_path::IROH_DIRECT_PATH_LOST)
        | Some(crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE) => FailureKind::PathRefused,
        _ => FailureKind::Local,
    }
}

/// 回执里的**机器码** (不是文案)。两种信封都认: `error_code` 字段, 以及门禁拒绝那种把码直接
/// 放在 `error` 字段里的形状 (`{"code":65,"error":"iroh_path_unknown"}`)。
///
/// 匹配用**全等**而不是子串: `error` 也可能是 file-transfer 的自然语言错误, 全等才不会把
/// "某句话里出现这个码"误当成路径拒绝。
fn failure_code(payload: &Value) -> Option<&str> {
    let envelope = payload.get("value").unwrap_or(payload);
    envelope
        .get("error_code")
        .and_then(Value::as_str)
        .or_else(|| envelope.get("error").and_then(Value::as_str))
}

/// daemon 在响应里挂的半快照原文。
///
/// 只有信封确实带 `evidence` 时才取: 从 `"evidence"` 后的冒号起, 用流式反序列化量出这个值占
/// 多少字节, 原样切出来 —— 解析再序列化会改键序与空白, 那就不再是 daemon 给我的那份。
fn raw_evidence(payload: &Value, line: &str) -> Option<String> {
    payload.get("value").unwrap_or(payload).get("evidence")?;
    let at = line.find("\"evidence\"")?;
    let colon = at + line[at..].find(':')?;
    let rest = &line[colon + 1..];
    let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
    stream.next()?.ok()?;
    Some(rest[..stream.byte_offset()].to_owned())
}

/// 走真实控制通道的交换: 每次请求独立建链 (TCP) 或独立查询 (Zenoh)。
pub(crate) struct TransportExchange {
    invocation: ControlInvocation,
    /// 单块应答超时。超时即失败并取消, 不让客户端无限等一个已经不响应的对端 (规格 §2.4)。
    chunk_ack_timeout_ms: u64,
}

impl TransportExchange {
    pub(crate) fn new(invocation: ControlInvocation, chunk_ack_timeout_ms: u64) -> Self {
        Self {
            invocation,
            chunk_ack_timeout_ms,
        }
    }
}

/// 上传/下载完成后的回执 (给 CLI 打印, 也给验收留证据)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferReceipt {
    pub(crate) transfer_id: String,
    pub(crate) direction: &'static str,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    /// 只有上传会出现: 本次是否真的从半成品接着传 (daemon 的 `resume_honored`)。
    pub(crate) resume_honored: Option<bool>,
}

impl TransferReceipt {
    pub(crate) fn render(&self) -> String {
        let resume = match self.resume_honored {
            Some(true) => " resume=honored",
            Some(false) => " resume=restarted",
            None => "",
        };
        // 尾列不能被新字段打断: put 的最后一列仍是 `resume=…`, get 的最后一列仍是 `sha256=…`,
        // 所以 `build=` 插在 `sha256=` **之前**。
        format!(
            "{} ok bytes={} {} sha256={}{}",
            self.direction,
            self.bytes,
            crate::build_info::build_tag(),
            self.sha256,
            resume
        )
    }
}

/// 单块应答超时: 优先用配置文件里的 `[file_transfer].chunk_ack_timeout_ms`,
/// 读不到配置就用内置默认值。
///
/// 客户端不该因为"没有配置文件"就不能传文件 —— 目标/命名空间都由 flag 给出, 配置在这里只提供
/// 一个超时值, 所以读失败按默认值走, 而不是报错。
pub(crate) fn chunk_ack_timeout_ms_from_config() -> u64 {
    crate::config::load_daemon_config_unvalidated(None)
        .map(|config| config.file_transfer.chunk_ack_timeout_ms)
        .unwrap_or(DEFAULT_CHUNK_ACK_TIMEOUT_MS)
}

/// 单个响应里能装下的**载荷字节**上限。
///
/// 从 `CONTROL_RESPONSE_MAX_BYTES` 推导 (不手抄数值): 载荷经 base64 膨胀 4/3, 再留 1 KiB 给
/// JSON 字段与转义。超过它的请求会被包成 `output_budget_exceeded`, 拿不到数据。
fn response_payload_bytes() -> u64 {
    let budget = crate::control_frames::CONTROL_RESPONSE_MAX_BYTES.saturating_sub(1024) as u64;
    budget / 4 * 3
}

const DEFAULT_CHUNK_ACK_TIMEOUT_MS: u64 = 30 * 1000;

/// daemon 那一半的证据文件名, 落在 `--bulk-path-json` 的同目录 (§4.3 的跨端 join 输入)。
pub(crate) const DAEMON_HALF_FILE: &str = "bulk-path.daemon.json";

/// `--put` 的 transfer_id 复用表。
///
/// # 为什么需要
///
/// lane 层失败 (帧写超时、读超时) 之后 daemon 的会话还在, 它按目标路径推导出来的暂存名被占着。
/// 下一次 `--put` 换一个新 id 会被 `file_transfer_path_in_use` 拒到续传窗口过期; 而每次都新生成
/// id 的 CLI 永远走不到"同 id + resume"那条路。按 (目标, 本地路径, 远端路径) 复用 id, 就把
/// "挡住下一次"变成"续上"。
///
/// # 存储形态: 一个键一个文件
///
/// 目录 `<rdog 用户目录>/bulk-transfer-ids/`, 每个键一份 `<FNV-1a 64 十六进制>.json`, 内容是
/// `{"key": <真实键>, "transfer_id": <id>}`。
///
/// 为什么不共用一张表: `load → insert → save` 之间没有互斥, 两条并发的传输会各自拿旧快照把整表
/// 覆盖掉 —— 200 轮实测丢一条键 144 次。临时文件 + rename 只保证"读者看不到半张表", 挡不住
/// "读-改-写丢更新"。键本身是天然可分的, 一个键一个文件之后 `put` / `remove` 只碰自己那份,
/// **没有读-改-写窗口, 不需要锁, 也不会丢别人的键**。
///
/// 单文件仍然 temp + rename: 那才是它该保的东西 —— 一个键的文件要么是旧的完整内容, 要么是新的
/// 完整内容。读不出来 (截断/坏 JSON) 时按"该键没有 id"处理并 warn, **不删文件** (留给排障);
/// 最坏只坏这一个键, 其余键不受影响。
///
/// 只记 `--put`: 下载方向 daemon 没有"未完成的下载"这种状态, 同 id 换不来续传, 记了也只是垃圾。
///
/// 旧的单文件 `<rdog 用户目录>/bulk-transfer-ids.json` 不迁移也不读: 这个 store 是同一个 PR 里
/// 引入的, 从未随发布版本出去, 盘上可能存在的那份只是本分支早期构建留下的。它原样留着不动。
///
/// # 已知边界: 没有在途 owner
///
/// 键是 `(target, local, remote)`, **不含进程身份, 也没有"谁在用这条 id"的守卫**。于是同一键上
/// 有另一个进程正在传时: 它写的 id 会覆盖这条, 而 `--restart` 读到那个 id 后发的 `@file-cancel`
/// 会把**那条在途会话**一起取消 (daemon 的 cancel 只认 `transfer_id`, 不认发起者)。
///
/// 规避是"同一键不要并发"; 根治要给 daemon 侧的会话加 owner 租约 (RAI-526, Backlog/low)。
#[derive(Clone)]
pub(crate) struct TransferIdStore {
    /// 存储目录; `None` = 不持久化 (`$HOME` 缺失)。
    dir: Option<PathBuf>,
}

impl TransferIdStore {
    /// 用户级默认存储。`$HOME` 缺失时退化为不持久化 (读不到用户目录不是传输失败的理由)。
    pub(crate) fn user_default() -> Self {
        Self {
            dir: crate::config::resolve_user_config_dir()
                .map(|dir| dir.join("bulk-transfer-ids")),
        }
    }

    #[cfg(test)]
    pub(crate) fn at(dir: PathBuf) -> Self {
        Self { dir: Some(dir) }
    }

    /// 测试专用: 预置一条"上一次尝试用过这个 id"的记录。
    #[cfg(test)]
    pub(crate) fn seed(&self, target: &str, local: &Path, remote: &str, transfer_id: &str) {
        self.put(target, local, remote, transfer_id);
    }

    /// 键是 (目标, 本地路径, 远端路径): 同一个本地文件发往两台不同主机是两次传输, 吃同一个
    /// transfer_id 会让第二次的 `@put-file` 在 daemon 侧撞上"同 id 换目标"那条分支。
    fn key(target: &str, local: &Path, remote: &str) -> String {
        format!("{target}\0{}\0{remote}", local.display())
    }

    /// 键 → 文件名: FNV-1a 64 的十六进制。**确定性、无依赖**, 同一个键在任何机器上都落到同一个
    /// 文件名上; 文件名只承载哈希, 真实键写在文件内容里, 便于人读与排障。
    fn file_name(key: &str) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in key.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}.json")
    }

    fn entry_path(&self, key: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(Self::file_name(key)))
    }

    fn read(&self, key: &str) -> Option<String> {
        let path = self.entry_path(key)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
            Err(err) => {
                eprintln!("warning: 读不到 {}: {err}; 该键按无 id 处理", path.display());
                return None;
            }
        };
        match serde_json::from_slice::<StoredTransferId>(&bytes) {
            Ok(stored) if stored.key == key => Some(stored.transfer_id),
            // 哈希撞车: 文件里存的是**另一个键**。FNV-1a 64 在真实量级上撞的概率可忽略, 但真撞了
            // 就会让两条传输静默共用一个 id —— 那正是这一轮在消灭的那类"看起来对、其实丢了一条"。
            // 所以读取路径校验一次: 键不一致按**缺失**处理 (fail-closed), 绝不把别人的 id 当本键返回。
            Ok(stored) => {
                eprintln!(
                    "warning: {} 里记的键与请求的键不同 (文件 {:?} / 请求 {:?}); 该键按无 id 处理",
                    path.display(),
                    stored.key,
                    key
                );
                None
            }
            Err(err) => {
                eprintln!(
                    "warning: 读不懂 {}: {err}; 该键按无 id 处理",
                    path.display()
                );
                None
            }
        }
    }

    /// 一个键一个文件, 所以这里只写自己那一份: 没有读-改-写窗口, 不会丢掉别人的键。
    fn write(&self, key: &str, transfer_id: &str) {
        let Some(path) = self.entry_path(key) else {
            return;
        };
        if let Some(dir) = self.dir.as_ref() {
            if let Err(err) = std::fs::create_dir_all(dir) {
                eprintln!("warning: 建不出 {}: {err}", dir.display());
                return;
            }
        }
        let bytes = match serde_json::to_vec(&StoredTransferId {
            key: key.to_string(),
            transfer_id: transfer_id.to_string(),
        }) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("warning: 序列化 transfer_id 条目失败: {err}");
                return;
            }
        };
        // 单文件的完整性: 读者只会看到旧的完整内容或新的完整内容。
        //
        // 临时名必须**唯一**: 只带 pid 时同 pid 的两个并发写 (同一进程的两个线程) 共用一个暂存
        // 文件, 能把写了一半的内容 rename 到位 (实测出现 `trailing characters` 与 `No such file`).
        // 后缀用进程级原子计数器: pid 区分进程, 计数器区分线程与同一线程的先后。
        let unique = TEMP_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let staging = path.with_extension(format!(
            "{}.{unique}.tmp",
            std::process::id()
        ));
        if let Err(err) = std::fs::write(&staging, bytes) {
            eprintln!("warning: 写不进 {staging:?}: {err}");
            return;
        }
        if let Err(err) = std::fs::rename(&staging, &path) {
            eprintln!("warning: 换不上 {path:?}: {err}");
            let _ = std::fs::remove_file(&staging);
        }
    }

    fn get(&self, target: &str, local: &Path, remote: &str) -> Option<String> {
        self.read(&Self::key(target, local, remote))
    }

    fn put(&self, target: &str, local: &Path, remote: &str, transfer_id: &str) {
        self.write(&Self::key(target, local, remote), transfer_id);
    }

    fn remove(&self, target: &str, local: &Path, remote: &str) {
        let Some(path) = self.entry_path(&Self::key(target, local, remote)) else {
            return;
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("warning: 删不掉 {:?}: {err}", path),
        }
    }
}

/// 暂存文件名的唯一后缀: 只靠 pid 会让同一进程的两个并发写撞在同一个暂存名上。
static TEMP_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 一个键的文件内容。真实键写在里面: 文件名是哈希, 而排障时人要能看出这是哪条传输。
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredTransferId {
    key: String,
    transfer_id: String,
}

/// 本端这一半的 `rdog.bulk-path.v1` 证据 (规格 §4.3: 两端各发一半, 不替对端填数)。
///
/// 能力由出口声明: 没有 relay 跳的链路 (`path_aware=false`) 记 `not_applicable`, 不是"通过";
/// iroh 准入 lane 记 `PATH_AWARE_CHECKED_PER_CHUNK` —— 每个边界都采样, 但**不是**传输层 pin。
pub(crate) struct BulkPathRecorder {
    pub(crate) attempt_id: String,
    pub(crate) capability: PathCapability,
    pub(crate) out: Option<std::path::PathBuf>,
    bulk_bytes: u64,
    wire_bytes: u64,
    chunks: u64,
    path_class: PathClass,
    direct_addr: Option<String>,
    /// 每条尝试是否**曾观测到** direct, 键是 `transfer_id` (open 复位)。
    ///
    /// 只有"曾直连"与"从未直连"分成两份状态, 才能把"中途掉 relay" (`iroh_direct_path_lost`)
    /// 与"从来没有直连" (`iroh_direct_path_unavailable`) 判成两个码。daemon 侧
    /// (`control_iroh::BulkPathGate`) 记的是同一件事。
    established: std::collections::HashMap<String, bool>,
    /// 括号: open 前的基线 + 沿途见过的所有 relay path 的并集 (§4.2)。
    bracket: RelayBracket,
    /// 括号右端: 终态之后那一次读数。
    relay_after: Option<RelayReading>,
    /// daemon 那一半的原文, 从终态响应的 `evidence` 里收下来 (§4.3)。
    daemon_half: Option<String>,
    aborted: bool,
    /// 失败路径上写快照时用的 id (成功路径用收据里的那个)。
    last_transfer_id: String,
    /// 这一次尝试为什么失败 —— 重放决策读它。`None` = 还没失败。
    failure: Option<FailureKind>,
}

impl BulkPathRecorder {
    pub(crate) fn new(
        attempt_id: String,
        capability: PathCapability,
        out: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            attempt_id,
            capability,
            out,
            bulk_bytes: 0,
            wire_bytes: 0,
            chunks: 0,
            path_class: if capability.path_aware {
                PathClass::Unknown
            } else {
                PathClass::NotApplicable
            },
            direct_addr: None,
            established: std::collections::HashMap::new(),
            bracket: RelayBracket::new(None),
            relay_after: None,
            daemon_half: None,
            aborted: false,
            last_transfer_id: String::new(),
            failure: None,
        }
    }

    /// open 前的门禁与括号左端 (§3.1 / §4.2)。
    ///
    /// 顺序是硬要求: 复位这条尝试的"曾直连"、采样、取 relay 基线, 然后才判 —— 基线必须早于
    /// open 执行, 否则 open 自己的字节会被算进括号的左边; open 也必须无条件复位, 同 id 的
    /// 幂等重试是一次**新的**尝试。
    pub(crate) fn begin(&mut self, exchange_peer: &dyn LineExchange) -> Result<(), String> {
        self.established
            .insert(self.last_transfer_id.clone(), false);
        self.clear_daemon_half_file();
        let before = self.sample(exchange_peer).and_then(|seen| seen.relay);
        self.bracket = RelayBracket::new(before);
        self.admit()
    }

    /// 门禁复查: **每个 chunk 边界都要再来一次** (§3.1), 判定用这一刻的采样。
    pub(crate) fn recheck(&mut self, exchange_peer: &dyn LineExchange) -> Result<(), String> {
        self.sample(exchange_peer);
        self.admit()
    }

    /// 括号右端: 终态请求**之后**才取 (§4.2)。
    ///
    /// 失败路径同样要取: 一次被中止的尝试也有"结束时那一刻"的 relay 读数, 那是它自己的证据。
    pub(crate) fn close_bracket(&mut self, exchange_peer: &dyn LineExchange) {
        if !self.capability.path_aware {
            return;
        }
        let observation = exchange_peer.observe_path();
        // 终态前的这次读数同样并入并集: 它可能正是唯一一次见到 relay path 的采样。
        self.bracket
            .observe(observation.as_ref().and_then(|seen| seen.relay.as_ref()));
        self.relay_after = observation.and_then(|seen| seen.relay);
    }

    /// 门禁判定: 用当前采样与"这条尝试曾否直连"查表 (§3.1)。
    pub(crate) fn admit(&mut self) -> Result<(), String> {
        let established = self
            .established
            .get(&self.last_transfer_id)
            .copied()
            .unwrap_or(false);
        match decide(self.capability, self.path_class, established) {
            BulkDecision::Allowed => Ok(()),
            BulkDecision::Refused(code) => {
                self.failure = Some(kind_for_code(Some(code)));
                Err(refuse_message(code))
            }
        }
    }

    /// 记下这一次尝试的失败来源。文案还要往上走, 但"能不能重放"只看这里。
    pub(crate) fn note_failure(&mut self, kind: FailureKind) {
        self.failure = Some(kind);
    }

    pub(crate) fn failure(&self) -> Option<FailureKind> {
        self.failure
    }

    /// 一次边界采样: 更新路径类别与 `direct_addr`, 并把"曾直连"记到这条尝试上。
    ///
    /// 一条尝试只取**一次**采样, 判定与记账共用它 —— 各读一次会留下"拒了但记成 direct"这种
    /// daemon 侧明确排除掉的自相矛盾。
    fn sample(&mut self, exchange_peer: &dyn LineExchange) -> Option<PathObservation> {
        if !self.capability.path_aware {
            return None;
        }
        // 路径感知的传输给不出观测就是 `unknown` (fail-closed), 不是"当作没这回事"。
        let seen = exchange_peer
            .observe_path()
            .unwrap_or_else(PathObservation::unknown);
        if seen.class == PathClass::Direct {
            if self.direct_addr.is_none() {
                self.direct_addr = seen.direct_addr.clone();
            }
            self.established
                .insert(self.last_transfer_id.clone(), true);
        }
        self.path_class = seen.class;
        self.bracket.observe(seen.relay.as_ref());
        Some(seen)
    }

    /// 记一块**已被对端确认**的字节。必须在 `request` 成功之后调: 记账在发送之前会把从未被
    /// 确认的字节算进 `bulk_bytes`, 两端 join 就对不上账。
    fn note_confirmed_chunk(&mut self, bulk: u64, wire: u64) {
        self.bulk_bytes += bulk;
        self.wire_bytes += wire;
        self.chunks += 1;
    }

    /// 收下 daemon 在终态响应里挂的那半快照 (§4.3)。
    ///
    /// 只收原文: 跨端 join 要的是 daemon 自己那份, 本端不改一个字节。收不到时保持 `None`, 由
    /// [`Self::write`] 明确报告 —— 伪造一份会让 join 拿到一对看起来完整的两端。
    pub(crate) fn note_daemon_half(&mut self, evidence: Option<String>) {
        if evidence.is_some() {
            self.daemon_half = evidence;
        }
    }

    pub(crate) fn remember_transfer(&mut self, transfer_id: &str) {
        self.last_transfer_id = transfer_id.to_string();
    }

    pub(crate) fn transfer_id_or_empty(&self) -> &str {
        &self.last_transfer_id
    }

    fn note_aborted(&mut self) {
        self.aborted = true;
    }

    /// 落盘本端证据。没有 `out` 时什么都不做。
    ///
    /// 写失败只 warn: 证据是给验收用的旁路产物, 不该让一次已经成功的传输因为写不了报告而报错 ——
    /// 但也**不许静默**: 吞掉 IO 错误会让"没有快照"与"快照写了但没落盘"分不开。
    pub(crate) fn write(&self, transfer_id: &str, direction: &'static str) {
        let Some(out) = self.out.as_ref() else {
            return;
        };
        let bracket = self.bracket.close(self.relay_after.as_ref());
        let snapshot = BulkPathSnapshot {
            attempt_id: self.attempt_id.clone(),
            transfer_id: transfer_id.to_string(),
            direction: direction.to_owned(),
            capability: self.capability,
            path_class: self.path_class,
            direct_addr: self.direct_addr.clone(),
            bulk_bytes: self.bulk_bytes,
            wire_bytes: self.wire_bytes,
            chunks: self.chunks,
            // 客户端没有并发检测能力: `null`, 不是 `false`。
            concurrency_suspected: None,
            endpoint: "client".to_owned(),
            build: crate::build_info::BUILD_SHA.to_owned(),
            // 括号两端都读到了才有四个数; 读不到就是 `None` (未采样), 绝不写 0。
            relay_tx_before: bracket.tx_before,
            relay_tx_after: bracket.tx_after,
            relay_rx_before: bracket.rx_before,
            relay_rx_after: bracket.rx_after,
            aborted: self.aborted,
        };
        if let Err(err) = std::fs::write(out, snapshot.to_json().to_string()) {
            eprintln!("warning: 证据快照写不进 {out:?}: {err}");
        }
        self.write_daemon_half();
    }

    /// 一次新尝试开始前删掉上一次留下的 daemon 半快照。
    ///
    /// 不删的话, 这一次收不到证据时那份文件还在, join 会把**上一次**尝试的 daemon 半快照配到
    /// 这一次的客户端半快照上 —— 两份都"看起来正常", 结论却是错的。
    fn clear_daemon_half_file(&self) {
        if !self.capability.path_aware {
            return;
        }
        let Some(own) = self.out.as_ref() else {
            return;
        };
        let stale = own.with_file_name(DAEMON_HALF_FILE);
        match std::fs::remove_file(&stale) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("warning: 删不掉旧的 {stale:?}: {err}"),
        }
    }

    /// daemon 那一半落到本端快照的同目录 (`bulk-path.daemon.json`)。
    ///
    /// 只有有 relay 跳的出口才会有 daemon 半快照 (daemon 只在准入 lane 上产出它), 所以
    /// `path_aware=false` 的出口在这里什么都不做。
    fn write_daemon_half(&self) {
        if !self.capability.path_aware {
            return;
        }
        let Some(own) = self.out.as_ref() else {
            return;
        };
        let target = own.with_file_name(DAEMON_HALF_FILE);
        match self.daemon_half.as_ref() {
            Some(raw) => {
                if let Err(err) = std::fs::write(&target, raw) {
                    eprintln!("warning: daemon 半快照写不进 {target:?}: {err}");
                }
            }
            // 不伪造: 没有这一半, join 必须落到 `bulk_path_endpoint_missing`, 而不是被一份
            // 本端补出来的"完整证据"顶掉。
            None => eprintln!(
                "warning: 终态响应里没有 daemon 半快照 (evidence): 跨端 join 会因缺少 daemon 侧而失败 (bulk_path_endpoint_missing)"
            ),
        }
    }
}

/// 把一次失败的交换收成错误文本, 同时把它带的 daemon 半快照收进证据。
///
/// 门禁拒绝与被中止的终态同样挂着 `evidence` (§4.3): 在错误路径上丢掉它, 跨端 join 就只能落到
/// `bulk_path_endpoint_missing` —— 而"为什么这次没成"正是最该留证据的时刻。
fn note_error(
    recorder: &mut BulkPathRecorder,
    error: ExchangeError,
    transfer_id: &str,
    resumable: bool,
) -> String {
    recorder.note_daemon_half(error.evidence);
    recorder.note_failure(error.kind);
    if resumable {
        format!(
            "{} (transfer_id={transfer_id}; 同路径重跑会用它续传)",
            error.message
        )
    } else {
        format!("{} (transfer_id={transfer_id})", error.message)
    }
}

/// 进度步长: 最多 20 条报进度行 (至少 1 MiB 一条)。
///
/// 规格 §5 要求 stderr 有进度行, 而它同时也是**"慢"与"卡住"的唯一区分手段**: 没有它, 一次被外层
/// `timeout` 杀掉的传输在日志里和一个死锁长得一模一样。
fn progress_step(size: u64) -> u64 {
    (size / 20).max(1024 * 1024)
}

fn report_progress(direction: &str, offset: u64, size: u64) {
    let percent = if size == 0 {
        100
    } else {
        offset.saturating_mul(100) / size
    };
    eprintln!(
        "{direction} {offset}/{size} ({percent}%) {}",
        crate::build_info::build_tag()
    );
}

fn refuse_message(code: &str) -> String {
    format!("{code}: 当前传输不能承载 bulk 直连传输")
}

fn new_transfer_id(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("cli-{tag}-{nanos}")
}

/// 发一行控制指令并取回结构化回执。
///
/// 回执是 `@response <json>`; 缺行、不可解析、或信封里带错误都算失败 —— 静默把错误当成成功是
/// 这条 lane 最贵的失败模式 (客户端会以为传输完成了)。
impl LineExchange for TransportExchange {
    fn request(&self, line: &str) -> Result<Reply, ExchangeError> {
        exchange(self, line)
    }
}

/// iroh 准入 lane 上的一条长连接: 整次尝试共用它, 失败即整条流作废。
///
/// 与 `TransportExchange` 的差别不只是"不重建链": 它让驱动在每个边界读到路径与 relay 计数,
/// 所以这是唯一一个本端 `path_class` 会是 `direct` 的出口。
struct IrohExchange<'lane> {
    /// 驱动是单线程的: 这里只需要"共用一条连接", 不需要跨线程互斥。
    lane: std::cell::RefCell<&'lane mut crate::control_iroh::IrohBulkLane>,
    chunk_ack_timeout_ms: u64,
}

/// iroh lane 上单个 chunk 的**载荷**字节上限。
///
/// 为什么不能沿用 `chunk_payload_bytes()` (≈5.03 MB): base64 之后的整帧约 6.7 MB, 而 `noq`
/// 单流接收窗口只有 1.25 MB —— `write_all` 必须等对端边读边腾窗口, 帧越大等待越久; 默认 3 s
/// 的帧写预算在一条真实 relay 上(几十 Mbps 以下)根本不够, 首块就会把 lane 打成 poison。
///
/// 256 KiB 的依据: 帧 ≈ 341 KiB ≈ 1.25 MB 窗口的 27%, 一帧最多挡一两次窗口更新; 按 96 KiB/s
/// 的最慢目标链路 (见 `MIN_FRAME_BYTES_PER_SEC`) 也是 ~3.6 s, 与默认预算同量级。
///
/// **只约束上传方向**。下载方向受的是另一条预算: `@file-read` 的回执是**响应行**, 而响应行
/// 有统一的 model-visible 上限 (`CONTROL_RESPONSE_MAX_BYTES` = 48 KiB), 超了会被包成
/// `output_budget_exceeded`。所以 `run_get_inner` 每块自己按 `response_payload_bytes()` 切到
/// 36096 B (≈35 KiB) —— 与这里的 256 KiB 不是同一个值, 也不该是: 上传的帧是请求行, 没有这条上限。
const IROH_CHUNK_PAYLOAD_BYTES: usize = 256 * 1024;

impl<'lane> IrohExchange<'lane> {
    fn new(lane: &'lane mut crate::control_iroh::IrohBulkLane, chunk_ack_timeout_ms: u64) -> Self {
        Self {
            lane: std::cell::RefCell::new(lane),
            chunk_ack_timeout_ms,
        }
    }
}

impl LineExchange for IrohExchange<'_> {
    fn request(&self, line: &str) -> Result<Reply, ExchangeError> {
        let line_read = self
            .lane
            .borrow_mut()
            .request(
                line,
                Duration::from_millis(self.chunk_ack_timeout_ms.max(1)),
            )
            .map_err(|err| {
                // lane 已经分了类: "连接仍开着但没答案"与"对端没了"是两条不同的路, 不能都退化成
                // 一个字符串再靠子串猜回来。
                let kind = match err.kind {
                    crate::control_iroh::LaneFailure::AckUnansweredLive => {
                        FailureKind::AckUnansweredLive
                    }
                    crate::control_iroh::LaneFailure::AckUnansweredClosed
                    | crate::control_iroh::LaneFailure::PeerClosed
                    | crate::control_iroh::LaneFailure::Local => FailureKind::Local,
                };
                ExchangeError::with_kind(err.message, kind)
            })?;
        let reply = parse_reply(&line_read)?;
        if let Some(error) = response_error(&reply.payload) {
            let kind = kind_for_code(failure_code(&reply.payload));
            return Err(error_reply(reply, error, kind));
        }
        Ok(reply)
    }

    fn observe_path(&self) -> Option<PathObservation> {
        Some(self.lane.borrow().observe())
    }

    /// 上传方向的块大小: 单帧必须远小于流窗口 (见 `IROH_CHUNK_PAYLOAD_BYTES`)。
    ///
    /// 下载方向不吃这个值 —— `run_get_inner` 自己按响应预算切 (36 KiB)。
    fn chunk_bytes(&self) -> usize {
        chunk_payload_bytes().min(IROH_CHUNK_PAYLOAD_BYTES)
    }
}

/// `rdog control <target> --transport iroh --put/--get` 的一次尝试。
#[derive(Clone)]
pub(crate) enum IrohBulkRequest {
    Put {
        local: PathBuf,
        remote: String,
        restart: bool,
    },
    Get {
        remote: String,
        local: PathBuf,
    },
}

/// iroh 准入 lane 上的 `--put` / `--get`。
///
/// 能力声明与准入 lane 一致: `path_aware=true` / `bulk_path_pin=false` /
/// `bulk_path_checked_per_chunk=true`。`bulk_path_pin` 只能是 `false` —— 逐边界检查证明的是
/// "每个边界上直连", 不是"边界之间不会改走 relay" (§3.5); 真 pin 归 RAI-501。
pub(crate) fn run_iroh_bulk(
    path: &Path,
    name: &str,
    request: IrohBulkRequest,
    out: Option<PathBuf>,
) -> Result<TransferReceipt, String> {
    // 被中止的 lane 会在 daemon 侧留一条已死的连接: 它搬过字节、然后不再有任何帧, 而 iroh 按
    // endpoint id 管路径状态 —— 同一身份的下一条连接的路径会停在 `selected=false`, 门禁读到的
    // 就是 `iroh_path_unknown`。**重拨本身就是修路径的手段** (实测: 重拨在 8.16 s 拿到可用路径,
    // 不重试时那条连接要 30.0 s 才被传输层放掉), 不是"等它过期"。该码在 §2.2 里可重试, 所以
    // 这里按它重拨, 让使用者看到的第一次重跑就能续上。
    //
    // 回执没回来 (`bulk_chunk_ack_unanswered`) **不在这条重拨里**: 那个状态下本端不知道对端
    // 做了什么 (chunk 可能已落盘, `@file-end` 甚至可能已提交), 而 daemon 没有"已提交结果"可回放
    // —— 盲目重开会让一次已经提交的上传从 0 重传、把成品替换掉。所以它如实报错, 由使用者显式
    // 重跑 (那条路径上 daemon 的 `written` 与提交结果才是可核的)。
    // ID 在**第一个 open 之前**就定下来, 并在整条命令内固定 (RAI-711 判据 2): 若第一次 open 的
    // 回执丢了, 下一轮才知道该回到哪一个会话 —— 等回执再落盘会让重放拿不到同一个 id。
    let ids = TransferIdStore::user_default();
    let planned = match &request {
        IrohBulkRequest::Put {
            local,
            remote,
            restart,
        } => Some(plan_transfer_id(&ids, name, local, remote, *restart)),
        IrohBulkRequest::Get { .. } => None,
    };
    let mut waited = Duration::ZERO;
    loop {
        match run_iroh_bulk_once(path, name, request.clone(), out.clone(), &ids, planned.as_deref()) {

            // 只看**来源**: 路径读不出才重拨 (重拨自己就是修路径的手段)。文案里恰好出现
            // `iroh_path_unknown` 的别的失败不该被当成路径问题 —— 那条负对照有用例钉住。
            Err(failure) => match retry_step(&failure, waited) {
                Some(step) => {
                    std::thread::sleep(step);
                    waited += step;
                }
                None => return Err(failure.message),
            },
            Ok(receipt) => return Ok(receipt),
        }
    }
}

/// 一次尝试为什么失败: 给重放决策用的类别 + 给用户看的文案。
///
/// 两者分开是因为它们的读者不同 —— 决策要的是"来源", 用户要的是"发生了什么"; 用文案兼做判据
/// 会让任何一次措辞改动悄悄改变重试行为 (评审 Finding 5)。
struct AttemptFailure {
    kind: FailureKind,
    message: String,
}

/// 路径读不出时重拨的间隔与总量: 一次重拨通常就够 (重拨自己会修路径), 预算只防"旧连接还没被
/// 传输层放掉"的那几秒。
const IROH_PATH_RETRY_STEP: Duration = Duration::from_secs(2);
const IROH_PATH_RETRY_BUDGET: Duration = Duration::from_secs(18);

/// 这条命令要用哪个 `transfer_id` —— **在第一个 open 之前**算, 之后不再变 (RAI-711 判据 2)。
///
/// `--restart` 换新 id (旧会话由第一次尝试里的取消收掉); 否则复用 store 里的那个 —— 同一条
/// (目标, 本地, 远端) 上一次没走完时, 新 id 会撞 `file_transfer_path_in_use`。
fn plan_transfer_id(
    ids: &TransferIdStore,
    target: &str,
    local: &Path,
    remote: &str,
    restart: bool,
) -> String {
    if restart {
        return new_transfer_id("put");
    }
    ids.get(target, local, remote)
        .unwrap_or_else(|| new_transfer_id("put"))
}

/// 这次失败要不要重拨、隔多久 —— **只看 `kind`**, 文案里出现什么都不影响它。
///
/// `AckUnansweredLive` 也**不在这里**: 它意味着本端不知道对端做了什么 (对端可能已提交), 重开会让
/// 已提交的上传从 0 重传并替换成品。同 ID 幂等续传是 RAI-711, 那条要先把"已提交就返回原终态"
/// 做出来, 才轮得到自动重放。
fn retry_step(failure: &AttemptFailure, waited: Duration) -> Option<Duration> {
    if failure.kind == FailureKind::PathUnknown && waited < IROH_PATH_RETRY_BUDGET {
        Some(IROH_PATH_RETRY_STEP)
    } else {
        None
    }
}

fn run_iroh_bulk_once(
    path: &Path,
    name: &str,
    request: IrohBulkRequest,
    out: Option<PathBuf>,
    ids: &TransferIdStore,
    planned: Option<&str>,
) -> Result<TransferReceipt, AttemptFailure> {
    let chunk_ack_timeout_ms = chunk_ack_timeout_ms_from_config();
    let ids = ids.clone();
    let planned = planned.map(str::to_owned);
    let target = name.to_owned();
    crate::control_iroh::with_bulk_lane(path, name, move |lane| {
        let exchange_peer = IrohExchange::new(lane, chunk_ack_timeout_ms);
        let mut recorder = BulkPathRecorder::new(
            format!("cli-{}", std::process::id()),
            PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
            out,
        );
        let outcome = match request {
            IrohBulkRequest::Put {
                local,
                remote,
                restart,
            } => run_put(
                &exchange_peer,
                &mut recorder,
                &ids,
                &target,
                &local,
                &remote,
                restart,
                planned.as_deref(),
            ),
            IrohBulkRequest::Get { remote, local } => {
                run_get(&exchange_peer, &mut recorder, &remote, &local)
            }
        };
        // 失败来源由驱动里的门禁与交换器记进 recorder —— 这里是唯一还看得见它的地方。
        (outcome, recorder.failure())
    })
    .map_err(|err| AttemptFailure {
        kind: FailureKind::Local,
        message: err.to_string(),
    })
    .and_then(|(outcome, kind)| match outcome {
        Ok(receipt) => Ok(receipt),
        Err(message) => Err(AttemptFailure {
            kind: kind.unwrap_or(FailureKind::Local),
            message,
        }),
    })
}

fn exchange(options: &TransportExchange, line: &str) -> Result<Reply, ExchangeError> {
    let artifacts_dir = std::env::temp_dir().join("rdog-transfer-cli");
    let (sender, receiver) = mpsc::channel();
    let invocation = options.invocation.clone();
    let worker_line = line.to_string();
    std::thread::spawn(move || {
        let outcome = send_control_lines_for_invocation(
            &invocation,
            std::slice::from_ref(&worker_line),
            &artifacts_dir,
        );
        let _ = sender.send(outcome);
    });

    let timeout = Duration::from_millis(options.chunk_ack_timeout_ms.max(1));
    let exchanges = match receiver.recv_timeout(timeout) {
        Ok(Ok(exchanges)) => exchanges,
        Ok(Err(err)) => {
            return Err(ExchangeError::without_evidence(format!(
                "控制通道失败: {err}"
            )))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(ExchangeError::without_evidence(format!(
                "等待回执超时 ({}ms): {}",
                options.chunk_ack_timeout_ms,
                redact_payload(&line)
            )))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(ExchangeError::without_evidence(
                "控制通道提前关闭".to_string(),
            ))
        }
    };

    let first = exchanges
        .into_iter()
        .next()
        .ok_or_else(|| ExchangeError::without_evidence("控制端没有返回任何回执".to_string()))?;
    let reply = parse_reply(
        first
            .response_line
            .as_deref()
            .ok_or_else(|| ExchangeError::without_evidence("回执不可解析: <none>".to_string()))?,
    )?;
    if let Some(error) = response_error(&reply.payload) {
        let kind = kind_for_code(failure_code(&reply.payload));
        return Err(error_reply(reply, error, kind));
    }
    Ok(reply)
}

/// 回执行里的错误。`kind:"error"` 与 `error` / `error_code` 两种写法都要认:
/// 稳定码走 `error`, 通用失败走 `kind`.
fn response_error(payload: &Value) -> Option<String> {
    // 三种信封都要认: `{"kind":"error","error":…}` (动作失败)、`{"error_code":…}` (稳定码)、
    // 以及 `{"value":{"error_code":"output_budget_exceeded",…}}` —— 最后那种是**响应超出统一
    // model-visible 预算**时的包法, 认不出来就会被当成"这一块数据是空的", 把一个明确的
    // 预算错误伪装成内容错。
    let envelope = payload.get("value").unwrap_or(payload);
    if envelope.get("ok").and_then(Value::as_bool) == Some(false) {
        let code = envelope.get("error_code").and_then(Value::as_str);
        let message = envelope.get("error_message").and_then(Value::as_str);
        return Some(match (code, message) {
            (Some(code), Some(message)) => format!("{code}: {message}"),
            (Some(code), None) => code.to_string(),
            (None, Some(message)) => message.to_string(),
            (None, None) => envelope.to_string(),
        });
    }
    let kind = envelope.get("kind").and_then(Value::as_str);
    let error = envelope
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| envelope.get("error_code").and_then(Value::as_str));
    match (kind, error) {
        (Some("error"), Some(error)) => Some(error.to_string()),
        (Some("error"), None) => Some(envelope.to_string()),
        (_, Some(error)) if kind != Some("file-transfer") => Some(error.to_string()),
        _ => None,
    }
}

fn parse_response_payload(line: &str) -> Option<Value> {
    let payload = line.trim_start().strip_prefix("@response ")?;
    serde_json::from_str(payload.trim()).ok()
}

/// 超时/失败信息里不要带 chunk 载荷 (那是文件内容, 不该进日志)。
fn redact_payload(line: &str) -> String {
    match line.find("\"data\"") {
        Some(index) => format!("{}…", &line[..index]),
        None => line.to_string(),
    }
}

fn sha256_of_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("打开 {path:?} 失败: {err}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("读取 {path:?} 失败: {err}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 上传: `@put-file` → `@file-chunk*` → `@file-end`。
///
/// `restart=true` 显式从 0 重来 (并要求 daemon 删掉既有半成品); 否则一律 `resume:true` ——
/// 窗口内没有半成品时 daemon 会回报 `resume_honored:false` 与 `written:0`, 也就是"从头来"
/// 这件事由回执说清楚, 而不是由客户端猜 (规格 §2.5 D1 的 CLI 侧规则)。
pub(crate) fn run_put(
    exchange_peer: &dyn LineExchange,
    recorder: &mut BulkPathRecorder,
    ids: &TransferIdStore,
    target: &str,
    local: &Path,
    remote: &str,
    restart: bool,
    planned: Option<&str>,
) -> Result<TransferReceipt, String> {
    // 任何一条失败路径都必须留下快照: 门禁拒绝、open 失败、分块失败、`@file-end` 失败。
    // 只在两条路上写会让"这次尝试到底发生了什么"在证据里缺一块 —— 而缺的那块最可能是
    // 失败原因本身。成功路径只在括号右端取完之后写一次。
    let outcome = run_put_inner(
        exchange_peer,
        recorder,
        ids,
        target,
        local,
        remote,
        restart,
        planned,
    );
    recorder.close_bracket(exchange_peer);
    match outcome {
        Ok(receipt) => {
            recorder.write(&receipt.transfer_id, "put");
            Ok(receipt)
        }
        Err(err) => {
            recorder.note_aborted();
            recorder.write(recorder.transfer_id_or_empty(), "put");
            Err(err)
        }
    }
}

fn run_put_inner(
    exchange_peer: &dyn LineExchange,
    recorder: &mut BulkPathRecorder,
    ids: &TransferIdStore,
    target: &str,
    local: &Path,
    remote: &str,
    restart: bool,
    planned: Option<&str>,
) -> Result<TransferReceipt, String> {
    let metadata = std::fs::metadata(local).map_err(|err| format!("读取 {local:?} 失败: {err}"))?;
    if metadata.is_dir() {
        return Err(format!(
            "{local:?} 是目录: 这条 lane 只传单文件 (目录打包不在规格内)"
        ));
    }
    let size = metadata.len();
    let sha256 = sha256_of_file(local)?;
    // 同一条 (目标, 本地, 远端) 上一次没走完就复用它的 id: daemon 的会话还在, 新 id 会撞
    // `file_transfer_path_in_use` (那条检查先于 `resume:false` 的清理), 一直挡到续传窗口过期。
    let previous = ids.get(target, local, remote);
    // "上一次没干净收尾" ⇔ store 里还留着这条 (成功终态会把它删掉)。
    let had_previous = previous.is_some();
    let transfer_id = if restart {
        // `--restart` 要换新 id, 那就必须先把旧会话收掉 —— 否则"别的 id 持有同一暂存路径"
        // 会把它挡在门外, 而错误文本还会说"同路径重跑会用它续传"(那个 id 我们已经不用了)。
        if let Some(stale) = previous.as_ref() {
            let _ = exchange_peer.request(&format!(
                "@file-cancel:{{\"transfer_id\":\"{stale}\"}}"
            ));
            ids.remove(target, local, remote);
        }
        // 计划好的 id 优先: 它在第一个 open 之前就定了, 重放轮也必须用它 (RAI-711 判据 2/7)。
        planned
            .map(str::to_owned)
            .unwrap_or_else(|| new_transfer_id("put"))
    } else {
        planned
            .map(str::to_owned)
            .or(previous)
            .unwrap_or_else(|| new_transfer_id("put"))
    };
    recorder.remember_transfer(&transfer_id);
    // 门禁在 transfer_id 之后就判: 被拒的尝试也要在证据里带自己的 id, 否则按 id join 时
    // 多次被拒会并成一条。括号左端也在这里取 —— 它必须早于 open 执行 (§4.2)。
    recorder.begin(exchange_peer)?;

    // 在**发出 open 之前**落盘 (RAI-711 判据 2 / 评审 #205 finding 1): open 的回执一丢, 下一条命令
    // 就得知道该回到哪个 id —— "等回执再落盘"在那种情况下等于把 id 丢掉, 重放只能换新 id, 于是
    // 撞上暂存路径占用。代价是 open 被拒时 store 里会留一条指向"没有会话"的 id, 而那条下一轮会被
    // 当作续传候选: daemon 那边没有会话也没有半成品时它会如实报 `written:0` (从 0 开始)。
    ids.put(target, local, remote, &transfer_id);
    // 只在"上一次没干净收尾"时要求**本次发布 no-replace**: 正常首传不带, `--restart` 一律不带
    // (它是"显式同意覆盖"的唯一入口)。含义只有一个: 这次 open 的发布不许覆盖已有目标。
    let no_replace_publish = !restart && had_previous;
    let open_line = serde_json::json!({
        "transfer_id": transfer_id,
        "no_replace_publish": no_replace_publish,
        // 两端快照要按 `attempt_id` 对齐 (§4.3): 客户端把自己这一半的身份交给 daemon,
        // 否则 daemon 只能按 `transfer_id` 派生一个, join 会永远是 `attempt_mismatch`。
        "attempt_id": recorder.attempt_id,
        "path": remote,
        "size": size,
        "sha256": sha256,
        "resume": !restart,
    });
    let opened = exchange_peer
        .request(&format!("@put-file:{}", open_line))
        .map_err(|err| note_error(recorder, err, &transfer_id, false))?;
    let written = opened.get("written").and_then(Value::as_u64).unwrap_or(0);
    let resume_honored = opened
        .get("resume_honored")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if written > size {
        return Err(format!(
            "半成品 {written} 字节大于本次声明的 {size} 字节: 不是同一次传输, 先换 transfer_id 或显式重来"
        ));
    }

    let mut file =
        std::fs::File::open(local).map_err(|err| format!("打开 {local:?} 失败: {err}"))?;
    file.seek(SeekFrom::Start(written))
        .map_err(|err| format!("定位到 {written} 失败: {err}"))?;
    let chunk_bytes = exchange_peer.chunk_bytes();
    let mut buffer = vec![0u8; chunk_bytes];
    let mut offset = written;
    let step = progress_step(size);
    let mut next_report = offset + step;
    report_progress("put", offset, size);
    while offset < size {
        let want = (size - offset).min(chunk_bytes as u64) as usize;
        let read = file
            .read(&mut buffer[..want])
            .map_err(|err| format!("读取 {local:?} 失败: {err}"))?;
        if read == 0 {
            return Err(format!("文件在 {offset} 处提前结束 (声明 {size} 字节)"));
        }
        let chunk_line = serde_json::json!({
            "transfer_id": transfer_id,
            "offset": offset,
            "data": BASE64_STANDARD.encode(&buffer[..read]),
        });
        // 门禁复查: 每个 chunk 边界都再来一次 (§3.1), 用这一刻的采样。只在 open 判一次会
        // 漏掉中途 direct → relay 的切换 —— 那正是 iroh_direct_path_lost 存在的理由。
        recorder.recheck(exchange_peer)?;
        let accepted = match exchange_peer.request(&format!("@file-chunk:{chunk_line}")) {
            Ok(value) => value,
            Err(err) => {
                let reason = note_error(recorder, err, &transfer_id, true);
                // 失败路径必须显式取消 —— 但 lane 已经 poison 时取消也发不出去, 那时留着会话
                // 反而是对的: 下次同路径的 `--put` 复用同一个 id 接着传 (见 `TransferIdStore`)。
                let _ = exchange_peer.request(&format!(
                    "@file-cancel:{{\"transfer_id\":\"{transfer_id}\"}}"
                ));
                return Err(format!("@{offset} 处发送分块失败: {reason}"));
            }
        };
        // 只有被对端**确认**过的那部分才入账: 记账在发送之前会把从未落盘的字节算进
        // bulk_bytes, 两端 join 就对不上账。
        let confirmed = accepted
            .get("written")
            .and_then(Value::as_u64)
            .map(|written| written.saturating_sub(offset))
            .unwrap_or(read as u64);
        recorder.note_confirmed_chunk(
            confirmed,
            chunk_line["data"].as_str().map(str::len).unwrap_or(0) as u64,
        );
        offset = accepted
            .get("written")
            .and_then(Value::as_u64)
            .unwrap_or(offset + read as u64);
        if offset >= next_report || offset >= size {
            report_progress("put", offset, size);
            next_report = offset + step;
        }
    }

    let end_line = serde_json::json!({
        "transfer_id": transfer_id,
        "sha256": sha256,
    });
    let committed = exchange_peer
        .request(&format!("@file-end:{end_line}"))
        .map_err(|err| note_error(recorder, err, &transfer_id, true))?;
    // 上传的终态是 `@file-end`: daemon 那一半随这条响应到达 (§4.3)。
    recorder.note_daemon_half(committed.evidence.clone());
    match committed.get("state").and_then(Value::as_str) {
        Some("committed") => {}
        other => {
            return Err(format!(
                "@file-end 未提交: state={other:?} (回执 {})",
                committed.payload
            ))
        }
    }
    // 提交成功: 这条 (目标, 本地, 远端) 不再有未完成尝试, 下一次从新 id 开始。
    ids.remove(target, local, remote);
    Ok(TransferReceipt {
        transfer_id: transfer_id.clone(),
        direction: "put",
        bytes: size,
        sha256,
        resume_honored: Some(resume_honored),
    })
}

/// 下载: `@get-file` → `@file-read*`, 逐块落盘。
///
/// 拼装完必须自己比对 sha256: daemon 报的 `state` 是"我读完了", 不是"你拿到的字节是对的"
/// (规格 §2.3)。
pub(crate) fn run_get(
    exchange_peer: &dyn LineExchange,
    recorder: &mut BulkPathRecorder,
    remote: &str,
    local: &Path,
) -> Result<TransferReceipt, String> {
    let outcome = run_get_inner(exchange_peer, recorder, remote, local);
    recorder.close_bracket(exchange_peer);
    match outcome {
        Ok(receipt) => {
            recorder.write(&receipt.transfer_id, "get");
            Ok(receipt)
        }
        Err(err) => {
            recorder.note_aborted();
            recorder.write(recorder.transfer_id_or_empty(), "get");
            Err(err)
        }
    }
}

fn run_get_inner(
    exchange_peer: &dyn LineExchange,
    recorder: &mut BulkPathRecorder,
    remote: &str,
    local: &Path,
) -> Result<TransferReceipt, String> {
    let transfer_id = new_transfer_id("get");
    recorder.remember_transfer(&transfer_id);
    recorder.begin(exchange_peer)?;
    let open_line = serde_json::json!({
        "transfer_id": transfer_id,
        // 下载方向同样要交身份: 两端快照按 `attempt_id` 对齐 (§4.3)。
        "attempt_id": recorder.attempt_id,
        "path": remote,
    });
    let opened = exchange_peer
        .request(&format!("@get-file:{open_line}"))
        .map_err(|err| note_error(recorder, err, &transfer_id, false))?;
    let size = opened.get("size").and_then(Value::as_u64).unwrap_or(0);
    let declared_sha256 = opened
        .get("sha256")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default();

    // 先写临时名, 摘要校验通过后再 rename 到目标: 中途任何失败都不能毁掉本地原有文件,
    // 也不能在目标路径上留一个没有标记的残片 (上传侧对半成品做了那么多隔离, 下载侧直接覆盖
    // 真文件是同一个错误的反面)。
    let staging = local.with_file_name(format!(
        "{}.rdog-partial-download",
        local
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".to_string())
    ));
    // 整段落盘收在一个闭包里: **任何**一步失败 (读块超时、解码失败、写盘失败、空块、字节数
    // 对不上) 都走同一个清理点删掉临时名。逐条 `?` 提前 return 会把 `<local>.rdog-partial-download`
    // 留在目标目录里 —— 原文件是安全的, 但残片没人清。
    let written = (|| -> Result<u64, String> {
        let mut file = std::fs::File::create(&staging)
            .map_err(|err| format!("创建 {staging:?} 失败: {err}"))?;
        // 下载方向的上限不是行长度而是**响应预算**: 一个 `@file-read` 的回执超预算时会被包成
        // `output_budget_exceeded` 而拿不到数据。所以每块按 `response_payload_bytes()` 切到 36096 B
        // (≈35 KiB) —— 比上传块 (iroh lane 上是 256 KiB) 小一个量级, 因为两个方向撞的是两条
        // 不同的预算: 上传受帧写预算与流窗口约束, 下载受响应行预算约束。
        //
        // 代价是结构性的: 每块一次完整往返, 36 KiB/块在 100 ms RTT 下约 0.34 MB/s (1 GiB ≈ 50 min)。
        // 这是"每块一往返"这个设计的上限, 不是实测值。
        let chunk_bytes = (exchange_peer.chunk_bytes() as u64).min(response_payload_bytes());
        let mut offset = 0u64;
        let step = progress_step(size);
        let mut next_report = step;
        report_progress("get", 0, size);
        loop {
            let want = (size - offset).min(chunk_bytes).max(1);
            let read_line = serde_json::json!({
                "transfer_id": transfer_id,
                "offset": offset,
                "length": want,
            });
            // 下载方向同样每个块边界判一次 (§3.1): 只在 open 判一次会漏掉中途掉 relay。
            recorder.recheck(exchange_peer)?;
            let read = exchange_peer
                .request(&format!("@file-read:{read_line}"))
                .map_err(|err| note_error(recorder, err, &transfer_id, false))?;
            let data = read.get("data").and_then(Value::as_str).unwrap_or_default();
            let bytes = BASE64_STANDARD
                .decode(data)
                .map_err(|err| format!("分块解码失败: {err}"))?;
            file.write_all(&bytes)
                .map_err(|err| format!("写入 {staging:?} 失败: {err}"))?;
            recorder.note_confirmed_chunk(bytes.len() as u64, data.len() as u64);
            offset += bytes.len() as u64;
            if offset >= next_report || offset >= size {
                report_progress("get", offset, size);
                next_report = offset + step;
            }
            let eof = read.get("eof").and_then(Value::as_bool).unwrap_or(false);
            if eof {
                // 下载的终态是 `eof:true` 的那一帧 `@file-read`: daemon 那一半挂在这里 (§4.3)。
                recorder.note_daemon_half(read.evidence.clone());
            }
            if eof || offset >= size {
                break;
            }
            if bytes.is_empty() {
                return Err(format!("@{offset} 处读到空块且未到 EOF (回执 {})", read.payload));
            }
        }
        file.flush()
            .map_err(|err| format!("写入 {staging:?} 失败: {err}"))?;
        Ok(offset)
    })();
    let offset = match written {
        Ok(offset) => offset,
        Err(err) => {
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }
    };

    let actual = sha256_of_file(&staging)?;
    if !declared_sha256.is_empty() && !actual.eq_ignore_ascii_case(&declared_sha256) {
        let _ = std::fs::remove_file(&staging);
        return Err(format!(
            "下载摘要不符: 声明 {declared_sha256}, 实际 {actual} — 未落到 {local:?}, 临时文件已删"
        ));
    }
    std::fs::rename(&staging, local).map_err(|err| {
        let _ = std::fs::remove_file(&staging);
        format!("落盘 {local:?} 失败: {err}")
    })?;
    Ok(TransferReceipt {
        transfer_id: transfer_id.clone(),
        direction: "get",
        bytes: offset,
        sha256: actual,
        resume_honored: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_file_transfer::{
        execute_file_cancel, execute_file_chunk, execute_file_end, execute_file_read,
        execute_get_file, execute_put_file,
    };

    /// 把请求直接发给**真的** daemon 侧实现: 只把线缆换成函数调用。
    ///
    /// 这样测的是驱动的状态机 (续传 offset、分块边界、失败即取消、下载摘要比对) 与真实协议语义的
    /// 组合, 而不是一个自己编的协议替身。
    struct LoopbackPeer {
        chunk_bytes: usize,
        fail_chunk_index: Option<usize>,
        seen_lines: std::sync::Mutex<Vec<String>>,
        download_sha_override: Option<String>,
        /// 第 N 个 `@file-read` 之后的读取故障: `"empty"` 返回空块, `"error"` 直接失败。
        download_fault: Option<(usize, &'static str)>,
        /// 按顺序取的本端路径观测 (用完最后一个就一直用它); 空 = 这个"传输"没有 relay 跳。
        observations: std::sync::Mutex<std::collections::VecDeque<PathObservation>>,
        /// 终态响应里挂上 daemon 半快照 (只有 iroh 准入 lane 会这么做)。
        attach_evidence: Option<Value>,
        /// 从第 N 个 chunk 起, **所有**后续请求都失败 —— 用来复现"lane 已 poison, 连 cancel 都发不
        /// 出去"的那种失败, 而不是"某一块失败但通道还活着"。
        poison_after_chunk: Option<usize>,
        /// `@file-end` 这一步失败时挂的 daemon 半快照 —— **终态**才是 daemon 会带 `evidence`
        /// 的地方 (§4.3): chunk 失败是非终态, daemon 不在那条错误上挂证据。
        end_failure_evidence: Option<Value>,
        /// `@put-file` 直接失败 (open 没被接受)。
        fail_open: bool,
    }

    impl LoopbackPeer {
        fn new(chunk_bytes: usize) -> Self {
            Self {
                chunk_bytes,
                fail_chunk_index: None,
                seen_lines: std::sync::Mutex::new(Vec::new()),
                download_sha_override: None,
                download_fault: None,
                observations: std::sync::Mutex::new(std::collections::VecDeque::new()),
                attach_evidence: None,
                poison_after_chunk: None,
                end_failure_evidence: None,
                fail_open: false,
            }
        }

        fn with_observations(mut self, observations: Vec<PathObservation>) -> Self {
            self.observations = std::sync::Mutex::new(observations.into());
            self
        }

        fn with_evidence(mut self, evidence: Value) -> Self {
            self.attach_evidence = Some(evidence);
            self
        }

        fn reply(&self, mut value: Value) -> Reply {
            if value.get("state").and_then(Value::as_str) == Some("committed")
                || value.get("eof").and_then(Value::as_bool) == Some(true)
            {
                if let Some(evidence) = self.attach_evidence.as_ref() {
                    value["evidence"] = evidence.clone();
                }
            }
            let evidence = value.get("evidence").map(Value::to_string);
            Reply {
                payload: value,
                evidence,
            }
        }

        fn seen(&self) -> Vec<String> {
            self.seen_lines.lock().unwrap().clone()
        }

        fn chunk_count(&self) -> usize {
            self.seen()
                .iter()
                .filter(|line| line.starts_with("@file-chunk:"))
                .count()
        }
    }

    impl LineExchange for LoopbackPeer {
        fn request(&self, line: &str) -> Result<Reply, ExchangeError> {
            self.seen_lines.lock().unwrap().push(line.to_string());
            if line.starts_with("@file-chunk:") {
                if let Some(index) = self.fail_chunk_index {
                    if self.chunk_count() > index {
                        return Err(ExchangeError::without_evidence("模拟的对端故障".to_string()));
                    }
                }
            }
            if let Some(index) = self.poison_after_chunk {
                let sent = self
                    .seen()
                    .iter()
                    .filter(|seen| seen.starts_with("@file-chunk:"))
                    .count();
                // 第 `index+1` 块之后整条流不可用: 后续每一条 (含 `@file-cancel`) 都失败。
                if sent > index && (!line.starts_with("@file-chunk:") || sent > index + 1) {
                    return Err(ExchangeError::without_evidence(
                        "模拟的 lane 已 poison".to_string(),
                    ));
                }
            }
            if self.fail_open && line.starts_with("@put-file:") {
                return Err(ExchangeError::without_evidence(
                    "模拟的 open 被拒".to_string(),
                ));
            }
            if line.starts_with("@file-end:") {
                if let Some(evidence) = self.end_failure_evidence.as_ref() {
                    return Err(ExchangeError {
                        message: "模拟的终态失败".to_string(),
                        evidence: Some(evidence.to_string()),
                        kind: FailureKind::Local,
                    });
                }
            }
            if line.starts_with("@file-read:") {
                if let Some((after, kind)) = self.download_fault {
                    let reads = self
                        .seen()
                        .iter()
                        .filter(|seen| seen.starts_with("@file-read:"))
                        .count();
                    if reads > after {
                        return match kind {
                            "empty" => Ok(self.reply(serde_json::json!({"data": "", "eof": false}))),
                            _ => Err(ExchangeError::without_evidence("模拟的读取故障".to_string())),
                        };
                    }
                }
            }
            let (verb, payload) = line
                .trim_start_matches('@')
                .split_once(':')
                .ok_or_else(|| ExchangeError::without_evidence(format!("缺少载荷: {line}")))?;
            let result = match verb {
                "put-file" => execute_put_file(payload),
                "file-chunk" => execute_file_chunk(payload),
                "file-end" => execute_file_end(payload),
                "file-cancel" => execute_file_cancel(payload),
                "get-file" => execute_get_file(payload),
                "file-read" => execute_file_read(payload),
                other => return Err(ExchangeError::without_evidence(format!("未知动词: {other}"))),
            };
            let mut value = result.map_err(|err| ExchangeError::without_evidence(err.to_string()))?;
            if verb == "get-file" {
                if let Some(sha) = &self.download_sha_override {
                    value["sha256"] = Value::String(sha.clone());
                }
            }
            Ok(self.reply(value))
        }

        fn observe_path(&self) -> Option<PathObservation> {
            let mut observations = self.observations.lock().unwrap();
            if observations.len() > 1 {
                observations.pop_front()
            } else {
                observations.front().cloned()
            }
        }

        fn chunk_bytes(&self) -> usize {
            self.chunk_bytes
        }
    }

    fn test_recorder(out: Option<std::path::PathBuf>) -> BulkPathRecorder {
        BulkPathRecorder::new(
            "test-attempt".to_string(),
            PathCapability::NOT_PATH_AWARE,
            out,
        )
    }

    /// 用例用的 transfer_id 存储: 每次调用一个独立文件, 不碰真实的 `~/.rdog`。
    fn ids_for(tag: &str) -> TransferIdStore {
        let dir = std::env::temp_dir().join(format!("rdog-cli-ids-{tag}-{}", std::process::id()));
        TransferIdStore::at(dir)
    }

    /// 绝大多数用例只关心协议状态机, 不关心 id 复用: 给它们一份用完即弃的存储。
    fn run_put_default_ids(
        peer: &LoopbackPeer,
        recorder: &mut BulkPathRecorder,
        local: &Path,
        remote: &str,
        restart: bool,
    ) -> Result<TransferReceipt, String> {
        let ids = ids_for(&recorder.attempt_id.clone());
        run_put(peer, recorder, &ids, "test-target", local, remote, restart, None)
    }

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rdog-transfer-cli-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn put_streams_multi_chunk_file_and_reports_the_checksum() {
        let dir = scratch_dir("put");
        let local = dir.join("source.bin");
        let payload: Vec<u8> = (0..40u8).collect();
        std::fs::write(&local, &payload).unwrap();
        let remote = dir.join("landed.bin");

        let peer = LoopbackPeer::new(7);
        let mut recorder = test_recorder(None);
        let receipt = run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap();

        assert_eq!(receipt.bytes, payload.len() as u64);
        assert_eq!(receipt.resume_honored, Some(false));
        assert_eq!(std::fs::read(&remote).unwrap(), payload);
        assert!(
            peer.chunk_count() > 1,
            "40 字节按 7 字节切块必须有多块, 否则没测到循环"
        );
        assert_eq!(
            receipt.sha256,
            format!("{:x}", Sha256::digest(&payload)),
            "回执里的摘要必须是本地文件的真值"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_resumes_from_the_partial_length_after_a_restart() {
        let dir = scratch_dir("put-resume");
        let local = dir.join("source.bin");
        let payload: Vec<u8> = (0..40u8).collect();
        std::fs::write(&local, &payload).unwrap();
        let remote = dir.join("landed.bin");

        // 先造一个半成品: 前三块已落盘, 会话随后"丢失" (daemon 重启的形态)
        let partial_peer = LoopbackPeer::new(7);
        let open = serde_json::json!({
            "transfer_id": "cli-resume-seed",
            "path": remote.to_string_lossy(),
            "size": payload.len(),
            "resume": false,
        });
        partial_peer.request(&format!("@put-file:{open}")).unwrap();
        for (index, block) in payload[..21].chunks(7).enumerate() {
            let chunk = serde_json::json!({
                "transfer_id": "cli-resume-seed",
                "offset": index * 7,
                "data": BASE64_STANDARD.encode(block),
            });
            partial_peer
                .request(&format!("@file-chunk:{chunk}"))
                .unwrap();
        }
        crate::control_file_transfer::forget_sessions_for_tests(&["cli-resume-seed"]);

        let peer = LoopbackPeer::new(7);
        let mut recorder = test_recorder(None);
        let receipt = run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap();
        assert_eq!(
            receipt.resume_honored,
            Some(true),
            "窗口内的半成品必须接着传"
        );
        assert_eq!(std::fs::read(&remote).unwrap(), payload);
        // 续传不该从 0 重发: 21 字节已收, 剩下 19 字节按 7 切 = 3 块
        assert_eq!(peer.chunk_count(), 3, "只该发剩下的块");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 这条只覆盖"某一块失败但通道还活着" —— 取消发得出去, 会话随之清掉。lane 已 poison、
    /// 连 cancel 也发不出去的那种失败由 `a_poisoned_lane_hands_the_next_run_the_same_transfer_id`
    /// 覆盖 (那条走 id 复用/续传, 不靠取消)。
    #[test]
    fn put_cancels_the_session_when_a_chunk_fails() {
        let dir = scratch_dir("put-cancel");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![7u8; 40]).unwrap();
        let remote = dir.join("landed.bin");

        let mut peer = LoopbackPeer::new(7);
        peer.fail_chunk_index = Some(1);
        let mut recorder = test_recorder(None);
        let err = run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap_err();
        assert!(err.contains("模拟的对端故障"), "错误必须带原因: {err}");
        assert!(
            peer.seen()
                .iter()
                .any(|line| line.starts_with("@file-cancel:")),
            "失败路径必须显式取消, 不能把半成品留给 TTL"
        );
        assert!(!remote.exists(), "失败不得留下目标文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_file_records_not_applicable_not_a_pass() {
        // 客户端只走没有 relay 跳的链路: 证据必须是 "not_applicable" (判定不适用),
        // 而不是"relay 字节为 0" —— 后者在无 relay 的拓扑上是空真。
        let dir = scratch_dir("snapshot");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![5u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7);
        let mut recorder = test_recorder(Some(out.clone()));
        run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap();

        let raw = std::fs::read_to_string(&out).unwrap();
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["schema"], "rdog.bulk-path.v1");
        assert_eq!(value["path_class"], "not_applicable");
        assert_eq!(value["path_aware"], false);
        assert_eq!(value["verdict"], "not_applicable");
        assert_eq!(value["endpoint"], "client");
        assert!(value["bulk_bytes"].as_u64().unwrap() > 0);
        assert!(value["wire_bytes"].as_u64().unwrap() >= value["bulk_bytes"].as_u64().unwrap());
        // 未采样必须写成 null, 不许写成 0 —— 那正是"假零"的来源。
        assert_eq!(value["relay_sample_available"], false);
        assert!(value["relay_tx_after"].is_null());
        assert!(value["concurrency_suspected"].is_null());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_onset_still_leaves_evidence() {
        // 门禁拒绝是最该留证据的一种尝试 (它连 open 都没发出去), 而它过去不写快照。
        let dir = scratch_dir("gate-evidence");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![1u8; 8]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(4);
        let mut recorder = BulkPathRecorder::new(
            "attempt".to_string(),
            PathCapability::PATH_AWARE_UNPINNED,
            Some(out.clone()),
        );
        let err = run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap_err();
        assert!(
            err.contains(crate::bulk_path::IROH_BULK_PATH_PIN_UNPINNED_ALIAS),
            "{err}"
        );

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(value["aborted"], true);
        assert_eq!(value["bulk_bytes"], 0);
        assert_eq!(value["verdict"], "aborted");
        assert!(peer.seen().is_empty(), "门禁拒绝必须在任何一行发出之前发生");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_aware_transport_without_sampling_is_refused() {
        // 适配器一旦声明路径感知而调用方还没有采样, 就必须 fail-closed 成 iroh_path_unknown ——
        // 读不出路径类别永远不许被当成直连。
        let mut recorder = BulkPathRecorder::new(
            "attempt".to_string(),
            PathCapability::PATH_AWARE_PINNED,
            None,
        );
        let err = recorder.admit().unwrap_err();
        assert!(err.contains(crate::bulk_path::IROH_PATH_UNKNOWN), "{err}");

        let mut unpinned = BulkPathRecorder::new(
            "attempt".to_string(),
            PathCapability::PATH_AWARE_UNPINNED,
            None,
        );
        let err = unpinned.admit().unwrap_err();
        assert!(
            err.contains(crate::bulk_path::IROH_BULK_PATH_PIN_UNAVAILABLE),
            "{err}"
        );
    }

    #[test]
    fn get_writes_the_file_and_verifies_the_checksum() {
        let dir = scratch_dir("get");
        let remote = dir.join("remote.bin");
        let payload: Vec<u8> = (0..40u8).collect();
        std::fs::write(&remote, &payload).unwrap();
        let local = dir.join("landed.bin");

        let peer = LoopbackPeer::new(9);
        let mut recorder = test_recorder(None);
        let receipt = run_get(&peer, &mut recorder, &remote.to_string_lossy(), &local).unwrap();
        assert_eq!(receipt.bytes, payload.len() as u64);
        assert_eq!(std::fs::read(&local).unwrap(), payload);
        assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(&payload)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_failure_mid_stream_leaves_no_staging_residue() {
        // 读块超时/解码失败/写盘失败/空块走的是同一个清理点; 这里用"空块"和"读取直接失败"
        // 两种触发, 断言临时名一定被删掉 —— 残片没人清, 而它长得和正常下载的文件名只差后缀。
        let dir = scratch_dir("get-residue");
        let remote = dir.join("remote.bin");
        std::fs::write(&remote, vec![3u8; 40]).unwrap();
        let staging = dir.join("landed.bin.rdog-partial-download");

        for kind in ["empty", "error"] {
            let local = dir.join("landed.bin");
            let mut peer = LoopbackPeer::new(8);
            peer.download_fault = Some((1, kind));
            let mut recorder = test_recorder(None);
            let err = run_get(&peer, &mut recorder, &remote.to_string_lossy(), &local).unwrap_err();
            assert!(
                err.contains("空块") || err.contains("模拟的读取故障"),
                "{kind}: 错误要带上原因: {err}"
            );
            assert!(!staging.exists(), "{kind}: 失败后不得留下临时名残片");
            assert!(!local.exists(), "{kind}: 也不得落下目标文件");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_failure_leaves_an_existing_local_file_untouched() {
        // 上传侧对半成品做了那么多隔离, 下载侧若一开始就 File::create 目标路径, 中途失败就会
        // 既毁掉本地原文件、又在目标位置留一个没有标记的残片。
        let dir = scratch_dir("get-keep");
        let remote = dir.join("remote.bin");
        std::fs::write(&remote, vec![1u8; 32]).unwrap();
        let local = dir.join("existing.bin");
        std::fs::write(&local, b"original").unwrap();

        let mut peer = LoopbackPeer::new(8);
        peer.download_sha_override = Some("0".repeat(64));
        let mut recorder = test_recorder(None);
        let err = run_get(&peer, &mut recorder, &remote.to_string_lossy(), &local).unwrap_err();
        assert!(err.contains("下载摘要不符"), "{err}");
        assert_eq!(
            std::fs::read(&local).unwrap(),
            b"original",
            "失败的下载不得改动本地原有文件"
        );
        assert!(
            !dir.join("existing.bin.rdog-partial-download").exists(),
            "临时文件必须删掉, 不留在目标目录里"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_fails_when_the_landed_bytes_do_not_match_the_declared_checksum() {
        // daemon 报的摘要不是"你拿到的字节是对的"的证据 —— 客户端必须自己比对 (规格 §2.3)。
        let dir = scratch_dir("get-mismatch");
        let remote = dir.join("remote.bin");
        std::fs::write(&remote, vec![1u8; 32]).unwrap();
        let local = dir.join("landed.bin");

        let mut peer = LoopbackPeer::new(8);
        peer.download_sha_override = Some("0".repeat(64));
        let mut recorder = test_recorder(None);
        let err = run_get(&peer, &mut recorder, &remote.to_string_lossy(), &local).unwrap_err();
        assert!(err.contains("下载摘要不符"), "必须报摘要不符: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // iroh 准入 lane 上的客户端: 能力声明、括号采样、`iroh_direct_path_lost` 的生产者,
    // 以及 daemon 半快照落盘 (§3.1 / §4.2 / §4.3)
    // -----------------------------------------------------------------------

    fn direct() -> PathObservation {
        PathObservation {
            class: PathClass::Direct,
            direct_addr: Some("192.0.2.7:51000".to_string()),
            relay: None,
        }
    }

    fn relay() -> PathObservation {
        PathObservation {
            class: PathClass::Relay,
            direct_addr: None,
            relay: None,
        }
    }

    /// 一次 direct 观测 + 这一次读到的 relay 累计量。
    fn direct_with_relay(tx: u64, rx: u64, paths: &[&str]) -> PathObservation {
        PathObservation {
            class: PathClass::Direct,
            direct_addr: Some("192.0.2.7:51000".to_string()),
            relay: Some(RelayReading {
                tx,
                rx,
                relay_paths: paths.iter().map(|path| path.to_string()).collect(),
            }),
        }
    }

    fn iroh_recorder(out: Option<std::path::PathBuf>) -> BulkPathRecorder {
        BulkPathRecorder::new(
            "cli-test".to_string(),
            PathCapability::PATH_AWARE_CHECKED_PER_CHUNK,
            out,
        )
    }

    #[test]
    fn the_iroh_exit_declares_per_chunk_checks_and_no_pin() {
        // 客户端在 iroh 准入 lane 上与 daemon 声明同一格能力: 逐 chunk 检查, **不是** pin。
        // 把它写成 pin 就是拿"每个边界上直连"顶替"边界之间不会改路" (§3.5)。
        let dir = scratch_dir("iroh-capability");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![4u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(value["path_aware"], true);
        assert_eq!(value["bulk_path_pin"], false);
        assert_eq!(value["bulk_path_checked_per_chunk"], true);
        assert_eq!(value["path_class"], "direct");
        assert_eq!(value["endpoint"], "client");
        assert_eq!(value["direct_addr"], "192.0.2.7:51000");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unsampled_relay_counter_stays_null_not_zero() {
        // 路径读到了 direct, 但 relay 计数读不到 (没有 relay 句柄 / 一条 path 都没有):
        // 四个计数必须是 null, verdict 是 relay_sample_unavailable —— 不是"零增量"。
        let dir = scratch_dir("iroh-unsampled");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![4u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        for key in [
            "relay_tx_before",
            "relay_tx_after",
            "relay_rx_before",
            "relay_rx_after",
        ] {
            assert!(value[key].is_null(), "{key} 未采样必须是 null, 不是 0");
        }
        assert_eq!(value["relay_sample_available"], false);
        assert_eq!(value["verdict"], "relay_sample_unavailable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_client_bracket_reports_the_delta_between_its_own_two_readings() {
        // 括号两端都由**客户端**取: open 前一次、终态后一次。中间那次是 chunk 边界采样,
        // 不改变括号的端点 —— 端点错位会让 delta 把别的区间算进来。
        let dir = scratch_dir("iroh-bracket");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![9u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![
            direct_with_relay(100, 50, &["relay:https://relay.invalid"]),
            direct_with_relay(120, 60, &["relay:https://relay.invalid"]),
            direct_with_relay(140, 70, &["relay:https://relay.invalid"]),
        ]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(value["relay_tx_before"], 100);
        assert_eq!(value["relay_tx_after"], 140);
        assert_eq!(value["relay_rx_before"], 50);
        assert_eq!(value["relay_rx_after"], 70);
        assert_eq!(value["relay_sample_available"], true);
        // 40 / 20 的区间增量不为零: 判定不能落到 relay_bytes_zero。
        assert_eq!(value["verdict"], "relay_bytes_nonzero");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relay_path_closed_inside_the_client_bracket_yields_no_reading() {
        // `before` 时在场的 relay path 到 `after` 时没了: 关闭字节已不在快照里, 相减会得到
        // 一个假的零。这份读数整份作废, 与 daemon 侧逐字同一条规则 (共用 `relay_bracket`)。
        let dir = scratch_dir("iroh-bracket-closed");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![9u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(20).with_observations(vec![
            direct_with_relay(10, 0, &["relay:https://relay.invalid"]),
            direct_with_relay(10, 0, &[]),
        ]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert!(value["relay_tx_before"].is_null());
        assert!(value["relay_tx_after"].is_null());
        assert_eq!(value["relay_sample_available"], false);
        assert_eq!(value["verdict"], "relay_sample_unavailable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn losing_the_direct_path_mid_upload_is_reported_as_lost_not_unavailable() {
        // 客户端"曾观测到 direct"必须按 transfer_id 记, 并在 open 复位: 只有它能把
        // "中途掉 relay" (`iroh_direct_path_lost`) 与"从来没有直连"分成两个码。
        let dir = scratch_dir("iroh-lost");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![1u8; 40]).unwrap();
        let remote = dir.join("landed.bin");

        let peer = LoopbackPeer::new(7).with_observations(vec![direct(), relay()]);
        let mut recorder = iroh_recorder(None);
        let err = run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false)
            .unwrap_err();
        assert!(
            err.contains(crate::bulk_path::IROH_DIRECT_PATH_LOST),
            "中途掉 relay 必须是 lost: {err}"
        );
        // 门禁在块发出**之前**判: 掉 relay 的那个边界上的字节一个都没上线。
        assert_eq!(peer.chunk_count(), 0, "被拒的边界之后不得再发块");
        assert!(
            peer.seen()
                .iter()
                .any(|line| line.starts_with("@put-file:")),
            "open 已经发出, 失败的是随后的边界"
        );
        // 不走 `@file-cancel`: 中途掉 relay 按规格 §2.5 B3 保留半成品, 留给下一次续传。
        assert!(
            !peer.seen()
                .iter()
                .any(|line| line.starts_with("@file-cancel:")),
            "中途掉路径是保留半成品重试, 不是取消这条会话"
        );
        assert!(!remote.exists(), "被中止的上传不得落下目标文件");
        // 生产上这条会话要留着给续传用, 但会话表是**同进程全局**的: 别的用例会拿未来时刻扫
        // 这张表, 留下一个活跃会话会把它们的计数吹掉。用例自己按 id 收干净。
        let id = peer
            .seen()
            .iter()
            .find_map(|line| line.strip_prefix("@put-file:"))
            .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
            .and_then(|open| {
                open.get("transfer_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .expect("@put-file 已经发出, open 载荷里必有 transfer_id");
        crate::control_file_transfer::forget_sessions_for_tests(&[&id]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relay_only_lane_refuses_the_upload_before_any_line() {
        let dir = scratch_dir("iroh-relay-open");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![1u8; 40]).unwrap();
        let remote = dir.join("landed.bin");

        let peer = LoopbackPeer::new(7).with_observations(vec![relay()]);
        let mut recorder = iroh_recorder(None);
        let err = run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false)
            .unwrap_err();
        assert!(
            err.contains(crate::bulk_path::IROH_DIRECT_PATH_UNAVAILABLE),
            "{err}"
        );
        assert!(peer.seen().is_empty(), "open 前被拒: 一条线都不得发出");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_daemon_half_lands_next_to_the_client_half_verbatim() {
        // 跨端 join 的第二份输入: daemon 在 `@file-end` 收据里挂的 `evidence` 必须落盘,
        // 且是 daemon 给的那份原文 (本端不补字段、不改结构)。
        let dir = scratch_dir("daemon-half");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![2u8; 12]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");
        let evidence = serde_json::json!({
            "schema": "rdog.bulk-path.v1",
            "attempt_id": "cli-test",
            "transfer_id": "daemon-side",
            "direction": "put",
            "path_aware": true,
            "bulk_path_pin": false,
            "bulk_path_checked_per_chunk": true,
            "path_class": "direct",
            "direct_addr": null,
            "bulk_bytes": 12,
            "wire_bytes": 16,
            "chunks": 1,
            "concurrency_suspected": null,
            "endpoint": "daemon",
            "relay_tx_before": 100,
            "relay_tx_after": 100,
            "relay_rx_before": 50,
            "relay_rx_after": 50,
            "relay_sample_available": true,
            "aborted": false,
            "verdict": "relay_bytes_zero",
        });

        let peer = LoopbackPeer::new(7)
            .with_observations(vec![direct()])
            .with_evidence(evidence.clone());
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let daemon_half = dir.join(DAEMON_HALF_FILE);
        let raw = std::fs::read_to_string(&daemon_half).expect("daemon 半快照必须落盘");
        let written: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(written, evidence, "写下的必须是 daemon 给的那份, 不改字段");
        assert_eq!(written["endpoint"], "daemon");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_daemon_half_is_not_invented() {
        // 收不到就不写: 伪造一份会让 join 拿到一对看起来完整的两端, 而其中一半是假的。
        let dir = scratch_dir("daemon-half-missing");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![2u8; 12]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        assert!(out.exists(), "本端那一半照写");
        assert!(
            !dir.join(DAEMON_HALF_FILE).exists(),
            "没有收到 daemon 半快照就不得凭空写一份"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn transfer_id_of(peer: &LoopbackPeer) -> String {
        peer.seen()
            .iter()
            .find_map(|line| line.strip_prefix("@put-file:"))
            .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
            .and_then(|open| {
                open.get("transfer_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .expect("@put-file 已经发出")
    }

    #[test]
    fn a_relay_path_seen_only_at_a_boundary_voids_the_bracket() {
        // 评审 #2: `before` 只有直连时它的 relay 集合是**空的**, 空集合的 `all()` 真空成立 ——
        // 于是一次在边界上短暂 fallback 到 relay、走掉一段字节、终态前又只剩直连的尝试, 会被
        // 读成"四个 0"并判 relay_bytes_zero。对账必须用"本次见过的并集"。
        let dir = scratch_dir("bracket-union");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![3u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![
            direct_with_relay(100, 50, &[]),
            direct_with_relay(120, 60, &["PathId(9)@relay:https://relay.invalid/"]),
            direct_with_relay(140, 70, &[]),
        ]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        for key in [
            "relay_tx_before",
            "relay_tx_after",
            "relay_rx_before",
            "relay_rx_after",
        ] {
            assert!(value[key].is_null(), "{key} 必须作废, 不是 0");
        }
        assert_eq!(value["relay_sample_available"], false);
        assert_eq!(value["verdict"], "relay_sample_unavailable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_writers_on_different_keys_never_lose_each_other() {
        // 评审 D3 实测: 共用一张表时 `load → insert → save` 会互相覆盖, 200 轮丢一条键 144 次。
        // 一个键一个文件之后两条写入只碰自己那份, 所以每一轮两条都必须还在。
        //
        // 用**线程**而不是进程: store 的全部状态都在文件里, 没有进程内缓存, 两个独立实例从文件
        // 读、往文件写, 在文件系统层面与两个进程等价 (这一点写在这里, 免得读成"只测了线程")。
        let dir = std::env::temp_dir().join(format!("rdog-cli-ids-race-{}", std::process::id()));
        let local = std::path::PathBuf::from("/tmp/race.bin");

        for round in 0..200 {
            let remote_a = format!("/tmp/race-a-{round}.bin");
            let remote_b = format!("/tmp/race-b-{round}.bin");
            let dir_a = dir.clone();
            let dir_b = dir.clone();
            let remote_a2 = remote_a.clone();
            let remote_b2 = remote_b.clone();
            let local_a = local.clone();
            let local_b = local.clone();
            let start = std::sync::Arc::new(std::sync::Barrier::new(2));
            let start_a = start.clone();
            let start_b = start.clone();

            let writer_a = std::thread::spawn(move || {
                let store = TransferIdStore::at(dir_a);
                start_a.wait();
                store.put("t", &local_a, &remote_a2, "id-a");
            });
            let writer_b = std::thread::spawn(move || {
                let store = TransferIdStore::at(dir_b);
                start_b.wait();
                store.put("t", &local_b, &remote_b2, "id-b");
            });
            writer_a.join().unwrap();
            writer_b.join().unwrap();

            let reader = TransferIdStore::at(dir.clone());
            assert_eq!(
                reader.get("t", &local, &remote_a),
                Some("id-a".to_string()),
                "第 {round} 轮丢了 A 的条目"
            );
            assert_eq!(
                reader.get("t", &local, &remote_b),
                Some("id-b".to_string()),
                "第 {round} 轮丢了 B 的条目"
            );
        }

        // 第二轮: 两个线程各写 300 次 (同一 pid、同一目录)。共用暂存名时这一支出现过
        // `trailing characters` 与 `换不上 …: No such file or directory` —— 现在每个写都有自己的
        // 唯一临时名, 所以既不该读坏, 也不该留下 `.tmp` 残片。
        let remote_a = "/tmp/burst-a.bin".to_string();
        let remote_b = "/tmp/burst-b.bin".to_string();
        let burst = |dir: std::path::PathBuf, remote: String, id: &'static str| {
            std::thread::spawn(move || {
                let store = TransferIdStore::at(dir);
                let local = std::path::PathBuf::from("/tmp/race.bin");
                for _ in 0..300 {
                    store.put("t", &local, &remote, id);
                }
            })
        };
        burst(dir.clone(), remote_a.clone(), "id-a").join().unwrap();
        burst(dir.clone(), remote_b.clone(), "id-b").join().unwrap();
        let reader = TransferIdStore::at(dir.clone());
        assert_eq!(reader.get("t", &local, &remote_a), Some("id-a".to_string()));
        assert_eq!(reader.get("t", &local, &remote_b), Some("id-b".to_string()));
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不得留下暂存残片: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hash_collision_reads_as_missing_not_as_someone_elses_id() {
        // 文件名按哈希分, 一旦两个键撞到同一个文件名, 后写的会覆盖前一个。文件内容里存了真实键,
        // 所以读取时能看出来: 键不一致必须按**缺失**处理, 绝不把别人的 id 当本键的返回。
        let dir =
            std::env::temp_dir().join(format!("rdog-cli-ids-collide-{}", std::process::id()));
        let local = std::path::PathBuf::from("/tmp/collide.bin");
        let store = TransferIdStore::at(dir.clone());
        store.put("t", &local, "/tmp/other.bin", "id-other");

        // 手工造一个"文件名属于 A、内容里的键是 B"的文件 (模拟碰撞后的覆盖结果)。
        let squatter = dir.join(TransferIdStore::file_name(&TransferIdStore::key(
            "t",
            &local,
            "/tmp/mine.bin",
        )));
        std::fs::write(
            &squatter,
            serde_json::to_vec(&StoredTransferId {
                key: TransferIdStore::key("t", &local, "/tmp/other.bin"),
                transfer_id: "id-other".to_string(),
            })
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            store.get("t", &local, "/tmp/mine.bin"),
            None,
            "撞车必须读成缺失, 不是别人的 id"
        );
        assert_eq!(
            store.get("t", &local, "/tmp/other.bin"),
            Some("id-other".to_string()),
            "另一边仍可读"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_entry_only_loses_its_own_key() {
        // 一个键一个文件: 截断只坏这一个键, 别的键照常可读; 坏文件按"该键无 id"处理且**不删**,
        // 留给排障 (它是不是被谁写坏了, 只有文件还在才查得出来)。
        let dir = std::env::temp_dir().join(format!("rdog-cli-ids-corrupt-{}", std::process::id()));
        let local = std::path::PathBuf::from("/tmp/corrupt.bin");
        let store = TransferIdStore::at(dir.clone());
        store.put("t", &local, "/tmp/good.bin", "id-good");
        store.put("t", &local, "/tmp/broken.bin", "id-broken");

        let broken = dir.join(TransferIdStore::file_name(&TransferIdStore::key(
            "t",
            &local,
            "/tmp/broken.bin",
        )));
        std::fs::write(&broken, b"{\"key\":\"t\",\"transfer_id\":\"id-trunc").unwrap();

        assert_eq!(store.get("t", &local, "/tmp/broken.bin"), None);
        assert_eq!(
            store.get("t", &local, "/tmp/good.bin"),
            Some("id-good".to_string()),
            "坏的只该坏它自己"
        );
        assert!(broken.exists(), "坏文件不删, 留给排障");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_receipt_keeps_its_trailing_columns() {
        // 成功回执是 stdout 的契约行, 尾列不能被新字段打断:
        //   put 的最后一列是 `resume=…`, get 的最后一列是 `sha256=…`。
        let put = TransferReceipt {
            transfer_id: "t".into(),
            direction: "put",
            bytes: 10,
            sha256: "abc".into(),
            resume_honored: Some(true),
        }
        .render();
        assert!(put.ends_with("resume=honored"), "put 尾列必须是 resume: {put}");
        assert!(put.contains(" build="), "{put}");

        let get = TransferReceipt {
            transfer_id: "t".into(),
            direction: "get",
            bytes: 10,
            sha256: "abc".into(),
            resume_honored: None,
        }
        .render();
        assert!(
            get.ends_with("sha256=abc"),
            "get 尾列必须是 sha256: {get}"
        );
        assert!(get.contains(" build="), "{get}");
    }

    #[test]
    fn progress_is_reported_at_most_twenty_times() {
        assert_eq!(progress_step(20 * 1024 * 1024), 1024 * 1024);
        assert_eq!(progress_step(200 * 1024 * 1024), 10 * 1024 * 1024);
        assert_eq!(progress_step(1_000), 1024 * 1024, "下限 1 MiB, 小文件只报首尾");
    }

    #[test]
    fn a_bracket_that_never_saw_a_relay_path_still_reads_zero() {
        // 空的并集不是豁免: 两个计数都在、没有 relay path 可对账时, 增量为 0 才是真的 0。
        let dir = scratch_dir("bracket-empty-union");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![3u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");

        let peer = LoopbackPeer::new(7).with_observations(vec![
            direct_with_relay(0, 0, &[]),
            direct_with_relay(0, 0, &[]),
            direct_with_relay(0, 0, &[]),
        ]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(value["relay_tx_before"], 0);
        assert_eq!(value["relay_tx_after"], 0);
        assert_eq!(value["relay_sample_available"], true);
        assert_eq!(value["verdict"], "relay_bytes_zero");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_poisoned_lane_hands_the_next_run_the_same_transfer_id() {
        // 评审 #3: lane 层失败后 cancel 也发不出去, daemon 的会话还占着目标推导出来的暂存名。
        // 下一次换新 id 会被 `file_transfer_path_in_use` 挡到续传窗口过期; 复用 id 才能续上。
        let dir = scratch_dir("poison-resume");
        let local = dir.join("source.bin");
        let payload: Vec<u8> = (0..40u8).collect();
        std::fs::write(&local, &payload).unwrap();
        let remote = dir.join("landed.bin");
        let ids = ids_for("poison-resume-shared");

        let mut peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        peer.poison_after_chunk = Some(0);
        let mut recorder = iroh_recorder(None);
        let err = run_put(
            &peer,
            &mut recorder,
            &ids,
            "test-target",
            &local,
            &remote.to_string_lossy(),
            false,
            None,
        )
        .unwrap_err();
        assert!(err.contains("poison"), "错误要把 lane 的真实原因带出来: {err}");
        assert!(
            err.contains(&transfer_id_of(&peer)),
            "错误里要能看出下一步会复用哪个 id: {err}"
        );
        let first_id = transfer_id_of(&peer);

        // 第二次同路径: 必须复用同一个 id, 由 daemon 侧按 resume:true 接着传。
        let peer2 = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder2 = iroh_recorder(None);
        let receipt = run_put(
            &peer2,
            &mut recorder2,
            &ids,
            "test-target",
            &local,
            &remote.to_string_lossy(),
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            transfer_id_of(&peer2),
            first_id,
            "同一条 (本地, 目标, 远端) 必须复用 transfer_id"
        );
        assert_eq!(
            receipt.resume_honored,
            Some(true),
            "复用 id 的意义就是落到 resume, 不是从 0 重来"
        );
        assert_eq!(std::fs::read(&remote).unwrap(), payload);
        crate::control_file_transfer::forget_sessions_for_tests(&[&first_id]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_terminal_failure_still_lands_its_daemon_half() {
        // 评审 #4 的边界: daemon 只在**终态**错误的回执上挂 `evidence` (§4.3), 非终态的 chunk
        // 失败不带。所以这条用例失败在 `@file-end` (终态), 替身也只在这一条错误上挂证据 ——
        // 读成"daemon 在 chunk 失败时也给 evidence"会是错的。
        let dir = scratch_dir("error-evidence");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![4u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");
        let evidence = serde_json::json!({
            "schema": "rdog.bulk-path.v1",
            "attempt_id": "cli-test",
            "transfer_id": "daemon-refused",
            "direction": "put",
            "endpoint": "daemon",
            "aborted": true,
            "verdict": "aborted",
        });

        let mut peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        peer.end_failure_evidence = Some(evidence.clone());
        let mut recorder = iroh_recorder(Some(out.clone()));
        let err = run_put_default_ids(
            &peer,
            &mut recorder,
            &local,
            &remote.to_string_lossy(),
            false,
        )
        .unwrap_err();
        assert!(err.contains("终态失败"), "{err}");

        let raw = std::fs::read_to_string(dir.join(DAEMON_HALF_FILE))
            .expect("终态错误回执上的 daemon 半快照也要落盘");
        let written: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(written, evidence);
        let id = transfer_id_of(&peer);
        crate::control_file_transfer::forget_sessions_for_tests(&[&id]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_cancels_the_stale_session_so_the_next_open_is_not_blocked() {
        // 评审 D2: `--restart` 换新 id, 但 daemon 的"别的 id 持有同一暂存路径"检查先于
        // `resume:false` 的清理 —— 不先把旧会话 cancel 掉, 下一次 open 会被 `path_in_use` 挡到
        // 续传窗口过期, 而错误文本还在说"同路径重跑会用它续传"(那个 id 已经不用了)。
        let dir = scratch_dir("restart-cancel");
        let local = dir.join("source.bin");
        let payload: Vec<u8> = (0..40u8).collect();
        std::fs::write(&local, &payload).unwrap();
        let remote = dir.join("landed.bin");
        let ids = ids_for("restart-cancel-shared");

        // 第一次: 第一块之后整条流不可用, 会话留在 daemon 表里。
        let mut peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        peer.poison_after_chunk = Some(0);
        let mut recorder = iroh_recorder(None);
        let id_at_failure = {
            let err = run_put(
                &peer,
                &mut recorder,
                &ids,
                "test-target",
                &local,
                &remote.to_string_lossy(),
                false,
            None,
        )
            .unwrap_err();
            assert!(err.contains("poison"), "{err}");
            transfer_id_of(&peer)
        };

        // 第二次带 --restart: 必须先 cancel 掉旧会话, 再用新 id 从 0 开始。
        let peer2 = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder2 = iroh_recorder(None);
        let receipt = run_put(
            &peer2,
            &mut recorder2,
            &ids,
            "test-target",
            &local,
            &remote.to_string_lossy(),
            true,
            None,
        )
        .unwrap();
        assert!(
            peer2
                .seen()
                .iter()
                .any(|line| line.starts_with("@file-cancel:")),
            "--restart 必须先把旧会话收掉: {:?}",
            peer2.seen()
        );
        assert!(
            peer2
                .seen()
                .iter()
                .any(|line| line.contains(&format!("\"transfer_id\":\"{id_at_failure}\""))
                    && line.starts_with("@file-cancel:")),
            "cancel 的必须是 store 里那个旧 id"
        );
        assert_ne!(
            transfer_id_of(&peer2),
            id_at_failure,
            "--restart 要换新 id"
        );
        assert_eq!(receipt.resume_honored, Some(false), "restart 是从 0 开始");

        // 第三次不带 --restart: 不该再撞 `path_in_use` —— 旧会话已经收干净了。
        let peer3 = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder3 = iroh_recorder(None);
        let again = run_put(
            &peer3,
            &mut recorder3,
            &ids,
            "test-target",
            &local,
            &remote.to_string_lossy(),
            false,
            None,
        );
        assert!(
            again.is_ok(),
            "旧会话收掉之后同路径重跑不该再撞 `path_in_use`: {:?}",
            again.err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_that_never_happened_promises_no_resume() {
        // open 没被接受时 daemon 侧还没有会话: 错误文本不能说"会用它续传"。
        // id 本身**在发 open 之前**就落盘 (M2), 见本用例末尾那条断言与理由。
        let dir = scratch_dir("open-refused");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![4u8; 8]).unwrap();
        let remote = dir.join("landed.bin");
        let ids = ids_for("open-refused-shared");

        let mut peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        peer.fail_open = true;
        let mut recorder = iroh_recorder(None);
        let err = run_put(
            &peer,
            &mut recorder,
            &ids,
            "test-target",
            &local,
            &remote.to_string_lossy(),
            false,
            None,
        )
        .unwrap_err();
        assert!(err.contains("模拟的 open 被拒"), "{err}");
        // 文案仍不许许诺"会用它续传" —— 那一刻确实还没有会话。
        assert!(
            !err.contains("会用它续传"),
            "没有可续的会话就不许许诺: {err}"
        );
        // **契约变更 (RAI-711 M2)**: id 现在在发 open **之前**就落盘, 因为 open 的回执丢了之后
        // 下一条命令必须知道该回到哪个 id (评审 #205 finding 1)。落盘不等于"会话存在": 下一轮
        // 拿这个 id 去 open 时, daemon 没有会话也没有半成品就会如实报 `written:0`, 从 0 开始。
        let planned = ids
            .get("test-target", &local, &remote.to_string_lossy())
            .expect("id 必须在发 open 之前落盘");
        assert!(planned.starts_with("cli-put-"), "{planned}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_daemon_half_from_the_previous_attempt_is_removed() {
        // 评审 #5: 上一次留下的 daemon 半快照不清掉, 这一次收不到证据时 join 会把两次尝试的
        // 两半配在一起 —— 两份都"看起来正常", 结论却是错的。
        let dir = scratch_dir("stale-daemon-half");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![4u8; 20]).unwrap();
        let remote = dir.join("landed.bin");
        let out = dir.join("bulk-path.json");
        std::fs::write(dir.join(DAEMON_HALF_FILE), b"{\"schema\":\"old\"}").unwrap();

        let peer = LoopbackPeer::new(7).with_observations(vec![direct()]);
        let mut recorder = iroh_recorder(Some(out.clone()));
        run_put_default_ids(&peer, &mut recorder, &local, &remote.to_string_lossy(), false).unwrap();

        assert!(
            !dir.join(DAEMON_HALF_FILE).exists(),
            "旧的那半必须被清掉, 不能配到这一次上"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_download_path_is_gated_at_every_chunk_boundary_too() {
        // 评审 #6: §3.1 的"每个 chunk 边界再判一次"在上传和下载上是同一条要求。
        let dir = scratch_dir("get-gate");
        let remote = dir.join("remote.bin");
        std::fs::write(&remote, vec![1u8; 40]).unwrap();
        let local = dir.join("landed.bin");

        let peer = LoopbackPeer::new(7).with_observations(vec![direct(), relay()]);
        let mut recorder = iroh_recorder(None);
        let err = run_get(&peer, &mut recorder, &remote.to_string_lossy(), &local).unwrap_err();
        assert!(
            err.contains(crate::bulk_path::IROH_DIRECT_PATH_LOST),
            "下载方向中途掉 relay 也必须拒: {err}"
        );
        assert!(
            peer.seen()
                .iter()
                .filter(|line| line.starts_with("@file-read:"))
                .count()
                <= 1,
            "被拒的那个边界之后不得再读块: {:?}",
            peer.seen()
        );
        assert!(!local.exists(), "被中止的下载不得落下目标文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_iroh_exit_rejects_a_bad_target_before_touching_the_network() {
        let dir = scratch_dir("iroh-name");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![2u8; 4]).unwrap();
        let profile = dir.join("client.json");
        std::fs::write(&profile, b"{}").unwrap();

        let err = run_iroh_bulk(
            &profile,
            "no-namespace",
            IrohBulkRequest::Put {
                local: local.clone(),
                remote: "/tmp/landed.bin".to_string(),
                restart: false,
            },
            None,
        )
        .unwrap_err();
        assert!(err.contains("NAMESPACE/NAME"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_iroh_exit_reports_a_missing_profile_instead_of_starting_a_transfer() {
        // profile 缺失是最常见的"策略/身份没配好": 必须在发第一条请求前失败, 且不落半成品。
        let dir = scratch_dir("iroh-profile");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![2u8; 4]).unwrap();
        let target = dir.join("landed.bin");

        let err = run_iroh_bulk(
            &dir.join("absent.json"),
            "lab/daemon",
            IrohBulkRequest::Put {
                local,
                remote: target.to_string_lossy().into_owned(),
                restart: false,
            },
            None,
        )
        .unwrap_err();
        assert!(
            err.contains("absent.json"),
            "错误必须指向读不出来的那个 profile: {err}"
        );
        assert!(!target.exists(), "失败的尝试不得落下目标文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn failure(kind: FailureKind, message: &str) -> AttemptFailure {
        AttemptFailure {
            kind,
            message: message.to_string(),
        }
    }

    #[test]
    fn a_path_unknown_failure_gets_one_redial_within_budget() {
        let step = retry_step(
            &failure(FailureKind::PathUnknown, "iroh_path_unknown"),
            Duration::ZERO,
        );
        assert_eq!(step, Some(IROH_PATH_RETRY_STEP));
        // 预算用尽之后不再重拨: 上界是"间隔 × 次数", 不是无限。
        assert_eq!(
            retry_step(
                &failure(FailureKind::PathUnknown, "iroh_path_unknown"),
                IROH_PATH_RETRY_BUDGET
            ),
            None
        );
    }

    #[test]
    fn a_failure_that_only_mentions_the_path_code_in_its_text_is_not_retried() {
        // 负对照 (评审 Finding 5): 判据是来源, 不是文案。一个"文案里恰好带 iroh_path_unknown"的
        // 本机失败 (比如本地路径名里就有这串) 不得触发重拨 —— 否则这次改动只是把脆弱从字符串
        // 搬到了类型上, 而没证明搬对了。
        assert_eq!(
            retry_step(
                &failure(
                    FailureKind::Local,
                    "打开 \"/tmp/iroh_path_unknown-note.bin\" 失败: 没有那个文件"
                ),
                Duration::ZERO
            ),
            None
        );
    }

    #[test]
    fn an_unanswered_ack_is_not_in_the_retry_set() {
        // RAI-705 → RAI-711 的分界: "本端不知道对端做了什么"不许自动重放, 因为对端可能已经提交,
        // 重开会把成品从 0 重传并替换。同 ID 幂等续传要先做出"已提交就返回原终态"。
        for kind in [
            FailureKind::AckUnansweredLive,
            FailureKind::PathRefused,
            FailureKind::Local,
        ] {
            assert_eq!(
                retry_step(&failure(kind, "任意文案"), Duration::ZERO),
                None,
                "{kind:?} 不该被重拨"
            );
        }
    }

    #[test]
    fn a_gate_refusal_envelope_is_classified_by_its_code_not_by_prose() {
        // 门禁拒绝的信封是 `{"code":65,"error":"iroh_path_unknown"}` —— 码放在 `error` 字段里。
        // 只认 `error_code` 会让这种拒绝退化成 `Local`, 于是"路径读不出 ⇒ 重拨修路径"这条既有
        // 承诺静默失效 (2026-11-28 实测: 该误分类让 `a_put_killed_mid_upload_resumes_on_the_first_
        // retry` 直接变红)。
        let refused = serde_json::json!({"code": 65, "error": crate::bulk_path::IROH_PATH_UNKNOWN});
        assert_eq!(
            kind_for_code(failure_code(&refused)),
            FailureKind::PathUnknown,
            "门禁拒绝的码必须落到 PathUnknown"
        );
        // 自然语言的 file-transfer 错误: 全等匹配不成立 ⇒ Local (不会因为句子里出现某个码而重拨)。
        let prose = serde_json::json!({
            "error": "@file-chunk offset 不连续: 期望 100, 收到 90 (iroh_path_unknown 不是这里的原因)"
        });
        assert_eq!(kind_for_code(failure_code(&prose)), FailureKind::Local);
    }

    #[test]
    fn the_transfer_id_plan_reuses_a_stored_one_and_restart_picks_a_new_one() {
        // RAI-711 判据 2: id 必须在第一个 open **之前**定下来并在整条命令内固定 —— 否则第一次
        // open 的回执一丢, 重放就不知道回到哪个会话 (现场形态: 新 id 撞 `file_transfer_path_in_use`)。
        let dir = scratch_dir("plan-id");
        // 隔离到 scratch 目录: `user_default()` 会写真实 `$HOME/.rdog/bulk-transfer-ids`, 用例一旦
        // panic 就在用户的真状态里留一条记录 (同 docs/solutions 的测试隔离纪律)。
        let ids = TransferIdStore::at(dir.join("ids"));
        let local = dir.join("source.bin");
        let remote = "/tmp/landed.bin";
        let fresh = plan_transfer_id(&ids, "lab/daemon", &local, remote, false);
        assert!(!fresh.is_empty());
        // 同一条 (目标, 本地, 远端) 有旧 id 时复用它 —— 换新 id 会撞暂存路径占用。
        ids.put("lab/daemon", &local, remote, "cli-put-known");
        assert_eq!(
            plan_transfer_id(&ids, "lab/daemon", &local, remote, false),
            "cli-put-known"
        );
        // `--restart` 显式要换新 id, 不继承旧的那个。
        assert_ne!(
            plan_transfer_id(&ids, "lab/daemon", &local, remote, true),
            "cli-put-known"
        );
        ids.remove("lab/daemon", &local, remote);
        let _ = std::fs::remove_dir_all(&dir);
    }



    #[test]
    fn daemon_stable_codes_map_to_their_retry_kind() {
        // 稳定码总表 ↔ 实现的对应: 读的是结构化的 `error_code` 字段。
        use crate::bulk_path::{
            IROH_BULK_PATH_PIN_UNAVAILABLE, IROH_DIRECT_PATH_LOST, IROH_DIRECT_PATH_UNAVAILABLE,
            IROH_PATH_UNKNOWN,
        };
        assert_eq!(kind_for_code(Some(IROH_PATH_UNKNOWN)), FailureKind::PathUnknown);
        for code in [
            IROH_DIRECT_PATH_LOST,
            IROH_DIRECT_PATH_UNAVAILABLE,
            IROH_BULK_PATH_PIN_UNAVAILABLE,
        ] {
            assert_eq!(kind_for_code(Some(code)), FailureKind::PathRefused, "{code}");
        }
        assert_eq!(kind_for_code(None), FailureKind::Local);
        assert_eq!(kind_for_code(Some("file_transfer_id_in_use")), FailureKind::Local);
    }
}
