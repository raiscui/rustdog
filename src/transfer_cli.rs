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
//! # 门禁位置
//!
//! bulk 字节从**控制端**发出, 所以"直连不可得就拒发"必须落在这里 (规格 §3.4)。当前 iroh lane
//! 只服务 `@ping` / `@pty`, bulk 动词进不去, 因此能力的拒绝发生在 channel 层; 阶段 3 接上
//! `path_class` 之后, 这里再加每个 chunk 发送前的复查。

use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
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
};

/// 一行请求 → 一行结构化回执。
///
/// 抽成 trait 的唯一理由是**可测**: 驱动是个状态机 (续传 offset、分块边界、失败即取消、下载摘要
/// 比对), 用真的协议实现 + 假传输就能把状态机测全, 不必起 daemon 也不必绑网络。
pub(crate) trait LineExchange {
    fn request(&self, line: &str) -> Result<Value, String>;

    /// 每块装载多少字节。默认取协议上限; 测试用小值才能在几条用例里跑完多块循环。
    fn chunk_bytes(&self) -> usize {
        chunk_payload_bytes()
    }
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
        format!(
            "{} ok bytes={} sha256={}{}",
            self.direction, self.bytes, self.sha256, resume
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

/// 本端这一半的 `rdog.bulk-path.v1` 证据 (规格 §4.3: 两端各发一半, 不替对端填数)。
///
/// 客户端今天只走 TCP / WebSocket / Zenoh, 这三条链路上没有 relay 跳, 所以 `path_class` 恒为
/// `not_applicable`、`verdict` 也必须是 `not_applicable` —— 不是"通过"。等 CLI 能走 iroh 之后,
/// 这里的采样才会变成 `direct` / `relay`。
pub(crate) struct BulkPathRecorder {
    pub(crate) attempt_id: String,
    pub(crate) capability: PathCapability,
    pub(crate) out: Option<std::path::PathBuf>,
    bulk_bytes: u64,
    wire_bytes: u64,
    chunks: u64,
    path_class: PathClass,
    aborted: bool,
    /// 失败路径上写快照时用的 id (成功路径用收据里的那个)。
    last_transfer_id: String,
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
            aborted: false,
            last_transfer_id: String::new(),
        }
    }

    /// 门禁 (§3.1): open 前一次。
    pub(crate) fn admit(&self) -> Result<(), String> {
        // `established_direct` 今天**没有生产者**: 判"中途 direct→relay"需要适配器在读采样的同时
        // 记住"本次是否曾观测到 direct", 而今天没有承载 bulk 数据面的适配器 (RAI-476)。所以这里
        // 恒传 `false` —— 它是被声明的缺口, 不是被假装解决的分支。`iroh_direct_path_lost` 因此只有
        // 单测覆盖, 不得据此主张"中途路径丢失已处理"。
        match decide(self.capability, self.path_class, false) {
            BulkDecision::Allowed => Ok(()),
            BulkDecision::Refused(code) => Err(refuse_message(code)),
        }
    }

    /// 门禁复查: **每个 chunk 边界都要再来一次**。
    ///
    /// 只在 open 判一次会漏掉中途 `direct → relay` 的切换 —— 那恰好是 `iroh_direct_path_lost`
    /// 存在的理由。少了这一处复查, 那个分支在生产里永远没有调用者。
    pub(crate) fn recheck(&self) -> Result<(), String> {
        self.admit()
    }

    /// 记一块**已被对端确认**的字节。必须在 `request` 成功之后调: 记账在发送之前会把从未被
    /// 确认的字节算进 `bulk_bytes`, 两端 join 就对不上账。
    fn note_confirmed_chunk(&mut self, bulk: u64, wire: u64) {
        self.bulk_bytes += bulk;
        self.wire_bytes += wire;
        self.chunks += 1;
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
        let snapshot = BulkPathSnapshot {
            attempt_id: self.attempt_id.clone(),
            transfer_id: transfer_id.to_string(),
            direction: direction.to_owned(),
            capability: self.capability,
            path_class: self.path_class,
            direct_addr: None,
            bulk_bytes: self.bulk_bytes,
            wire_bytes: self.wire_bytes,
            chunks: self.chunks,
            // 客户端没有并发检测能力: `null`, 不是 `false`。
            concurrency_suspected: None,
            endpoint: "client".to_owned(),
            // 客户端驱动没有 relay 句柄可读 —— 四个计数是 `None` (未采样), 不是 0。
            relay_tx_before: None,
            relay_tx_after: None,
            relay_rx_before: None,
            relay_rx_after: None,
            aborted: self.aborted,
        };
        if let Err(err) = std::fs::write(out, snapshot.to_json().to_string()) {
            eprintln!("warning: 证据快照写不进 {out:?}: {err}");
        }
    }
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
    fn request(&self, line: &str) -> Result<Value, String> {
        exchange(self, line)
    }
}

fn exchange(options: &TransportExchange, line: &str) -> Result<Value, String> {
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
        Ok(Err(err)) => return Err(format!("控制通道失败: {err}")),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(format!(
                "等待回执超时 ({}ms): {}",
                options.chunk_ack_timeout_ms,
                redact_payload(&line)
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => return Err("控制通道提前关闭".to_string()),
    };

    let first = exchanges
        .into_iter()
        .next()
        .ok_or_else(|| "控制端没有返回任何回执".to_string())?;
    let payload = first
        .response_line
        .as_deref()
        .and_then(parse_response_payload)
        .ok_or_else(|| {
            format!(
                "回执不可解析: {:?}",
                first.response_line.as_deref().unwrap_or("<none>")
            )
        })?;
    if let Some(error) = response_error(&payload) {
        return Err(error);
    }
    Ok(payload)
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
    local: &Path,
    remote: &str,
    restart: bool,
) -> Result<TransferReceipt, String> {
    // 任何一条失败路径都必须留下快照: 门禁拒绝、open 失败、分块失败、`@file-end` 失败。
    // 只在两条路上写会让"这次尝试到底发生了什么"在证据里缺一块 —— 而缺的那块最可能是
    // 失败原因本身。
    match run_put_inner(exchange_peer, recorder, local, remote, restart) {
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
    local: &Path,
    remote: &str,
    restart: bool,
) -> Result<TransferReceipt, String> {
    let metadata = std::fs::metadata(local).map_err(|err| format!("读取 {local:?} 失败: {err}"))?;
    if metadata.is_dir() {
        return Err(format!(
            "{local:?} 是目录: 这条 lane 只传单文件 (目录打包不在规格内)"
        ));
    }
    let size = metadata.len();
    let sha256 = sha256_of_file(local)?;
    let transfer_id = new_transfer_id("put");
    recorder.remember_transfer(&transfer_id);
    // 门禁在 transfer_id 之后就判: 被拒的尝试也要在证据里带自己的 id, 否则按 id join 时
    // 多次被拒会并成一条。
    recorder.admit()?;

    let open_line = serde_json::json!({
        "transfer_id": transfer_id,
        // 两端快照要按 `attempt_id` 对齐 (§4.3): 客户端把自己这一半的身份交给 daemon,
        // 否则 daemon 只能按 `transfer_id` 派生一个, join 会永远是 `attempt_mismatch`。
        "attempt_id": recorder.attempt_id,
        "path": remote,
        "size": size,
        "sha256": sha256,
        "resume": !restart,
    });
    let opened = exchange_peer.request(&format!("@put-file:{}", open_line))?;
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
        // 门禁复查: 每个 chunk 边界都再来一次 (§3.1)。只在 open 判一次会漏掉中途
        // direct → relay 的切换 —— 那正是 iroh_direct_path_lost 存在的理由。
        recorder.recheck()?;
        let accepted = match exchange_peer.request(&format!("@file-chunk:{chunk_line}")) {
            Ok(value) => value,
            Err(err) => {
                recorder.note_aborted();
                recorder.write(&transfer_id, "put");
                // 失败路径必须显式取消: 否则半成品和会话都留在盘上等 TTL, 而调用方已经放弃了。
                let _ = exchange_peer.request(&format!(
                    "@file-cancel:{{\"transfer_id\":\"{transfer_id}\"}}"
                ));
                return Err(format!("@{offset} 处发送分块失败: {err}"));
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
    }

    let end_line = serde_json::json!({
        "transfer_id": transfer_id,
        "sha256": sha256,
    });
    let committed = exchange_peer.request(&format!("@file-end:{end_line}"))?;
    match committed.get("state").and_then(Value::as_str) {
        Some("committed") => {}
        other => {
            return Err(format!(
                "@file-end 未提交: state={other:?} (回执 {committed})"
            ))
        }
    }
    recorder.write(&transfer_id, "put");
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
    match run_get_inner(exchange_peer, recorder, remote, local) {
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
    recorder.admit()?;
    let open_line = serde_json::json!({
        "transfer_id": transfer_id,
        // 下载方向同样要交身份: 两端快照按 `attempt_id` 对齐 (§4.3)。
        "attempt_id": recorder.attempt_id,
        "path": remote,
    });
    let opened = exchange_peer.request(&format!("@get-file:{open_line}"))?;
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
        // `output_budget_exceeded` 而拿不到数据。所以每块按响应预算切, 比上传块小一个量级。
        let chunk_bytes = (exchange_peer.chunk_bytes() as u64).min(response_payload_bytes());
        let mut offset = 0u64;
        loop {
            let want = (size - offset).min(chunk_bytes).max(1);
            let read_line = serde_json::json!({
                "transfer_id": transfer_id,
                "offset": offset,
                "length": want,
            });
            let read = exchange_peer.request(&format!("@file-read:{read_line}"))?;
            let data = read.get("data").and_then(Value::as_str).unwrap_or_default();
            let bytes = BASE64_STANDARD
                .decode(data)
                .map_err(|err| format!("分块解码失败: {err}"))?;
            file.write_all(&bytes)
                .map_err(|err| format!("写入 {staging:?} 失败: {err}"))?;
            recorder.note_confirmed_chunk(bytes.len() as u64, data.len() as u64);
            offset += bytes.len() as u64;
            let eof = read.get("eof").and_then(Value::as_bool).unwrap_or(false);
            if eof || offset >= size {
                break;
            }
            if bytes.is_empty() {
                return Err(format!("@{offset} 处读到空块且未到 EOF (回执 {read})"));
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
            recorder.note_aborted();
            recorder.write(&transfer_id, "get");
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
    recorder.write(&transfer_id, "get");
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
    }

    impl LoopbackPeer {
        fn new(chunk_bytes: usize) -> Self {
            Self {
                chunk_bytes,
                fail_chunk_index: None,
                seen_lines: std::sync::Mutex::new(Vec::new()),
                download_sha_override: None,
                download_fault: None,
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
        fn request(&self, line: &str) -> Result<Value, String> {
            self.seen_lines.lock().unwrap().push(line.to_string());
            if line.starts_with("@file-chunk:") {
                if let Some(index) = self.fail_chunk_index {
                    if self.chunk_count() > index {
                        return Err("模拟的对端故障".to_string());
                    }
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
                            "empty" => Ok(serde_json::json!({"data": "", "eof": false})),
                            _ => Err("模拟的读取故障".to_string()),
                        };
                    }
                }
            }
            let (verb, payload) = line
                .trim_start_matches('@')
                .split_once(':')
                .ok_or_else(|| format!("缺少载荷: {line}"))?;
            let result = match verb {
                "put-file" => execute_put_file(payload),
                "file-chunk" => execute_file_chunk(payload),
                "file-end" => execute_file_end(payload),
                "file-cancel" => execute_file_cancel(payload),
                "get-file" => execute_get_file(payload),
                "file-read" => execute_file_read(payload),
                other => return Err(format!("未知动词: {other}")),
            };
            let mut value = result.map_err(|err| err.to_string())?;
            if verb == "get-file" {
                if let Some(sha) = &self.download_sha_override {
                    value["sha256"] = Value::String(sha.clone());
                }
            }
            Ok(value)
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
        let receipt = run_put(
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
        let receipt = run_put(
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

    #[test]
    fn put_cancels_the_session_when_a_chunk_fails() {
        let dir = scratch_dir("put-cancel");
        let local = dir.join("source.bin");
        std::fs::write(&local, vec![7u8; 40]).unwrap();
        let remote = dir.join("landed.bin");

        let mut peer = LoopbackPeer::new(7);
        peer.fail_chunk_index = Some(1);
        let mut recorder = test_recorder(None);
        let err = run_put(
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
        run_put(
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
        let err = run_put(
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
        let recorder = BulkPathRecorder::new(
            "attempt".to_string(),
            PathCapability::PATH_AWARE_PINNED,
            None,
        );
        let err = recorder.admit().unwrap_err();
        assert!(err.contains(crate::bulk_path::IROH_PATH_UNKNOWN), "{err}");

        let unpinned = BulkPathRecorder::new(
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
}
