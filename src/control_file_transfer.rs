//! 双向分块文件传输 (specs/file-transfer-control-spec.md, 决策已定)
//!
//! # 协议形态 (与 spec 建议的一处偏差, 见"为什么拉取式")
//!
//! 上传 (控制端 → 被控端):
//! ```text
//! @put-file:{transfer_id, path, size, sha256?, resume?}   → {state:"open", written}
//! @file-chunk:{transfer_id, offset, data(base64)}         → {state:"accepting", written}
//! @file-end:{transfer_id, sha256}                         → {state:"committed"}
//! ```
//! 下载 (被控端 → 控制端, **拉取式**):
//! ```text
//! @get-file:{transfer_id, path}                           → {state:"reading", size, sha256}
//! @file-read:{transfer_id, offset, length}                → {offset, data, eof}
//! ```
//!
//! ## 为什么拉取式 (规格偏差记录)
//!
//! spec 建议被控端推送 `@file-chunk` 流。推送需要多帧 outcome — 而现有
//! `into_single_response_line` 有 Phase 1 单帧假设 (zenoh 兼容桥按单响应
//! 消费), 推送式要么破假设要么引新帧型。拉取式 (`@file-read` 循环) 在现有
//! 单响应模型上零改动落地, 与上传方向对称, 天然支持断点 (offset 由控制端
//! 控制)。SHALL 四条 (双向/有界/续传/摘要+路径) 全部满足。
//!
//! # 决策落地
//!
//! - base64 编码 (决策 1): 与 SaveFile 同构, 纯文本帧
//! - chunk 上限 (决策 2): ≤ 帧上限的 80% (8MiB × 0.8 ≈ 6.29MiB 载荷),
//!   超限的 @file-read length 请求被钳制而不是撑爆响应
//! - 路径穿越拒绝 (决策 3): normalize 后含 `..` 组件即拒绝
//! - 目录打包不做 (决策 4): 协议按单文件设计, 未来组包在上层加

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 单个 @file-chunk data 载荷的字节上限 (解码前 base64 文本的字符数上限)。
/// 取帧上限 8MiB 的 80% — 决策 2 的落地: chunk 永远塞得进一行。
const CHUNK_BASE64_LIMIT: usize = (MAX_CONTROL_MESSAGE_BYTES * 4) / 5;

use crate::control_line_read::MAX_CONTROL_MESSAGE_BYTES;

/// 传输会话表。OnceLock + Mutex 与 task_control 的 TASKS 同模式。
static TRANSFERS: OnceLock<Mutex<HashMap<String, TransferSession>>> = OnceLock::new();

fn transfers() -> &'static Mutex<HashMap<String, TransferSession>> {
    TRANSFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 一条进行中的传输。上传会话在 @put-file 建立、@file-end 提交后移除;
/// 下载会话只读, @get-file 建立、读完即移除 (无状态残留)。
#[derive(Debug, Clone, PartialEq)]
struct TransferSession {
    /// 目标/源文件路径 (已规范化)
    path: PathBuf,
    /// 声明的文件大小 (上传 = 预期写入总量; 下载 = 实际文件大小)
    size: u64,
    /// 期望摘要 (上传: 完成校验; 下载: 源文件摘要)
    expected_sha256: Option<String>,
    /// 已持久写入的字节数 (上传续传的断点)
    written: u64,
    /// 上传 = 暂存文件路径; 下载 = None (直接读源文件)
    temp_path: Option<PathBuf>,
    /// 上传方向?
    is_upload: bool,
    /// 最近一次有进展的时刻 (epoch ms)。空闲过期 (reap_idle_transfers) 据此判定。
    last_activity_ms: u64,
}

/// 当前 epoch 毫秒。过期判定收 now 参数而不是内部取时钟, 让单测能确定性地构造时间。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================
// 请求模型与解析 (错误统一 io::Error, 与项目 parse 惯例一致)
// ============================================================

fn parse_object(payload: &str, verb: &str) -> Result<Value, std::io::Error> {
    serde_json::from_str(payload)
        .map_err(|e| std::io::Error::other(format!("@{verb} payload 必须是 JSON 对象: {e}")))
}

fn require_string<'a>(value: &'a Value, key: &str, verb: &str) -> Result<&'a str, std::io::Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| std::io::Error::other(format!("@{verb} 缺 {key}")))
}

fn require_u64(value: &Value, key: &str, verb: &str) -> Result<u64, std::io::Error> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| std::io::Error::other(format!("@{verb} 缺 {key} (u64)")))
}

// ============================================================
// 路径安全 (决策 3)
// ============================================================

/// 规范化并校验目标路径: 拒绝 `..` 穿越 (含符号链接语义上等价的逃逸方向)。
///
/// 不用 `canonicalize`: 它要求路径存在且会**解析符号链接** — 后者对
/// "写入前校验"是竞态面; 这里做纯词法校验, 与实际写入目录之间由 OS
/// open 语义兜底。绝对路径允许 (与 @cmd 的文件写入权限同量级)。
fn validate_transfer_path(raw: &str, verb: &str) -> Result<PathBuf, std::io::Error> {
    let path = Path::new(raw);
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(std::io::Error::other(format!(
                "@{verb} 路径拒绝 .. 穿越: {raw}"
            )));
        }
    }
    if raw.trim().is_empty() {
        return Err(std::io::Error::other(format!("@{verb} 路径为空")));
    }
    Ok(path.to_path_buf())
}

// ============================================================
// 上传: put-file / file-chunk / file-end
// ============================================================

pub fn execute_put_file(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "put-file";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();
    let path = validate_transfer_path(require_string(&value, "path", VERB)?, VERB)?;
    let size = require_u64(&value, "size", VERB)?;
    let expected_sha256 = value
        .get("sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let resume = value.get("resume").and_then(Value::as_bool).unwrap_or(false);

    let temp_path = path.with_extension(format!(
        "{}.yino-partial",
        path.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));

    let mut sessions = transfers().lock().expect("transfer table poisoned");
    let previous = sessions.remove(&transfer_id);
    let written = if resume {
        // 续传: 保留已有 partial 的进度; 无 partial 从 0 起
        std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0)
    } else {
        // 重开: 清掉旧 partial (半损坏文件不留盘, NSIS 教训同族)
        let _ = std::fs::remove_file(&temp_path);
        0
    };
    // 同 id 重开但换了一条暂存路径时, 旧会话的半成品不在新路径上, 没人会清理它
    if let Some(previous) = previous {
        if previous.temp_path.as_deref() != Some(temp_path.as_path()) {
            reap_partial(&previous);
        }
    }
    sessions.insert(
        transfer_id.clone(),
        TransferSession {
            path,
            size,
            expected_sha256,
            written,
            temp_path: Some(temp_path),
            is_upload: true,
            last_activity_ms: now_ms(),
        },
    );
    Ok(json!({
        "kind": "file-transfer",
        "state": if resume { "resumed" } else { "open" },
        "transfer_id": transfer_id,
        "written": written,
        "next_offset": written,
    }))
}

pub fn execute_file_chunk(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "file-chunk";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();
    let offset = require_u64(&value, "offset", VERB)?;
    let data_b64 = require_string(&value, "data", VERB)?;

    if data_b64.len() > CHUNK_BASE64_LIMIT {
        return Err(std::io::Error::other(format!(
            "@file-chunk 载荷超过上限: {} > {CHUNK_BASE64_LIMIT} (base64 字符数)",
            data_b64.len()
        )));
    }
    let bytes = BASE64_STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| std::io::Error::other(format!("@file-chunk data 不是合法 base64: {e}")))?;

    let mut sessions = transfers().lock().expect("transfer table poisoned");
    let Some(session) = sessions.get_mut(&transfer_id) else {
        return Err(std::io::Error::other(format!(
            "@file-chunk 未知 transfer_id: {transfer_id} (先 @put-file)"
        )));
    };
    if !session.is_upload {
        return Err(std::io::Error::other(
            "@file-chunk 只用于上传会话 (下载用 @file-read)",
        ));
    }
    // 顺序写: offset 必须等于已写长度 — 不连续即协议错 (静默留洞比报错危险)
    if offset != session.written {
        return Err(std::io::Error::other(format!(
            "@file-chunk offset 不连续: 期望 {}, 收到 {}",
            session.written, offset
        )));
    }
    let Some(temp_path) = session.temp_path.clone() else {
        return Err(std::io::Error::other("@file-chunk 会话缺少暂存路径"));
    };
    // 追加写 (open_append 保证 O_APPEND 语义, 与 session.written 一致)
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
    }
    session.written += bytes.len() as u64;
    session.last_activity_ms = now_ms();
    Ok(json!({
        "kind": "file-transfer",
        "state": "accepting",
        "transfer_id": transfer_id,
        "written": session.written,
        "next_offset": session.written,
    }))
}

pub fn execute_file_end(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "file-end";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();
    let declared_sha256 = require_string(&value, "sha256", VERB)?.to_owned();

    let (final_path, temp_path, size, written, declared_in_put) = {
        let mut sessions = transfers().lock().expect("transfer table poisoned");
        let Some(session) = sessions.get_mut(&transfer_id) else {
            return Err(std::io::Error::other(format!(
                "@file-end 未知 transfer_id: {transfer_id}"
            )));
        };
        if !session.is_upload {
            return Err(std::io::Error::other("@file-end 只用于上传会话"));
        }
        (
            session.path.clone(),
            session.temp_path.clone().expect("upload 会话必有暂存路径"),
            session.size,
            session.written,
            session.expected_sha256.clone(),
        )
    };

    // 双校验: 字节数先查 (便宜), 摘要后算 (贵)
    if written != size {
        let _ = std::fs::remove_file(&temp_path);
        transfers()
            .lock()
            .expect("transfer table poisoned")
            .remove(&transfer_id);
        return Err(std::io::Error::other(format!(
            "@file-end 字节数不符: 已写 {written}, 声明 {size} — 半成品已清理"
        )));
    }
    let actual_sha256 = sha256_file(&temp_path)?;
    if !actual_sha256.eq_ignore_ascii_case(&declared_sha256) {
        let _ = std::fs::remove_file(&temp_path);
        transfers()
            .lock()
            .expect("transfer table poisoned")
            .remove(&transfer_id);
        return Err(std::io::Error::other(format!(
            "@file-end 摘要不符: 声明 {declared_sha256}, 实际 {actual_sha256} — 半成品已清理"
        )));
    }
    // 会话里声明的 sha256 (若有) 也要一致 — 防控制端两次声明漂移
    if let Some(expected) = &declared_in_put {
        if !actual_sha256.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&temp_path);
            transfers()
                .lock()
                .expect("transfer table poisoned")
                .remove(&transfer_id);
            return Err(std::io::Error::other(
                "@file-end 与 @put-file 声明摘要不一致 — 半成品已清理",
            ));
        }
    }

    // 提交: 原子改名到目标 (同目录 rename, 失败回退为复制)
    if std::fs::rename(&temp_path, &final_path).is_err() {
        std::fs::copy(&temp_path, &final_path)
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp_path);
                std::io::Error::other(format!("提交文件失败 (rename 与 copy 均失败): {e}"))
            })?;
        let _ = std::fs::remove_file(&temp_path);
    }
    Ok(json!({
        "kind": "file-transfer",
        "state": "committed",
        "transfer_id": transfer_id,
        "path": final_path,
        "size": written,
        "sha256": actual_sha256,
    }))
}

// ============================================================
// 下载: get-file / file-read (拉取式, 见模块头注)
// ============================================================

pub fn execute_get_file(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "get-file";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();
    let path = validate_transfer_path(require_string(&value, "path", VERB)?, VERB)?;

    let metadata = std::fs::metadata(&path)
        .map_err(|e| std::io::Error::other(format!("@get-file 打开失败: {e}")))?;
    if metadata.is_dir() {
        return Err(std::io::Error::other("@get-file 不支持目录 (单文件协议)"));
    }
    let sha256 = sha256_file(&path)?;
    let mut sessions = transfers().lock().expect("transfer table poisoned");
    sessions.insert(
        transfer_id.clone(),
        TransferSession {
            path,
            size: metadata.len(),
            expected_sha256: Some(sha256.clone()),
            written: 0,
            temp_path: None,
            is_upload: false,
            last_activity_ms: now_ms(),
        },
    );
    Ok(json!({
        "kind": "file-transfer",
        "state": "reading",
        "transfer_id": transfer_id,
        "size": metadata.len(),
        "sha256": sha256,
    }))
}

pub fn execute_file_read(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "file-read";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();
    let offset = require_u64(&value, "offset", VERB)?;
    let length = require_u64(&value, "length", VERB)?;

    // 钳制: 响应也要塞得进一行 (决策 2 的对称落地)
    let length = length.min((CHUNK_BASE64_LIMIT / 4) as u64);

    let (path, total, expected_sha256) = {
        let mut sessions = transfers().lock().expect("transfer table poisoned");
        let Some(session) = sessions.get_mut(&transfer_id) else {
            return Err(std::io::Error::other(format!(
                "@file-read 未知 transfer_id: {transfer_id} (先 @get-file)"
            )));
        };
        if session.is_upload {
            return Err(std::io::Error::other("@file-read 只用于下载会话"));
        }
        session.last_activity_ms = now_ms();
        (
            session.path.clone(),
            session.size,
            session.expected_sha256.clone(),
        )
    };

    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(&path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0u8; length as usize];
    let n = file.read(&mut buffer)?;
    buffer.truncate(n);
    let eof = offset + n as u64 >= total;
    let data = BASE64_STANDARD.encode(&buffer);

    // 读到 EOF 且校验通过则移除下载会话 (无状态残留); 校验由控制端比对
    // @get-file 返回的 sha256 — 被控端已在打开时算过, 这里不重复算
    if eof {
        transfers()
            .lock()
            .expect("transfer table poisoned")
            .remove(&transfer_id);
    }
    Ok(json!({
        "kind": "file-transfer",
        "transfer_id": transfer_id,
        "offset": offset,
        "length": n,
        "data": data,
        "eof": eof,
        "sha256": expected_sha256,
    }))
}

/// 回收一条会话的半成品文件。取消、空闲过期、同 id 换路径重开三处共用这条路径 ——
/// 回收语义只写一次, 免得某一条将来漏掉盘上的残留。返回是否真的删掉了文件。
fn reap_partial(session: &TransferSession) -> bool {
    let Some(temp_path) = session.temp_path.as_deref() else {
        return false;
    };
    match std::fs::remove_file(temp_path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            log::warn!(
                "文件传输半成品删除失败: path={}, error={err}",
                temp_path.display()
            );
            false
        }
    }
}

/// `@file-cancel`: 主动取消一次传输 (上传或下载), 语义与空闲过期完全相同 ——
/// 移除会话条目 + 删半成品。只删暂存文件, **不碰**目标路径上已有的文件。
/// 幂等: 未知 transfer_id 返回 `state:"absent"`, 不算错误 (重复取消是正常调用顺序)。
pub fn execute_file_cancel(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "file-cancel";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();

    let removed = transfers()
        .lock()
        .expect("transfer table poisoned")
        .remove(&transfer_id);
    let (state, partial_removed) = match removed {
        Some(session) => ("cancelled", reap_partial(&session)),
        None => ("absent", false),
    };
    Ok(json!({
        "kind": "file-transfer",
        "state": state,
        "transfer_id": transfer_id,
        "partial_removed": partial_removed,
    }))
}

/// `@file-list`: 回读当前活跃的传输会话。
///
/// 存在的理由是"被拒绝/被取消后注册表无残留"这件事必须能**直接读**, 而不是靠"再取消一次看它
/// 回 absent"来推断. 只列元信息, 不读文件内容.
pub fn execute_file_list() -> std::io::Result<Value> {
    let sessions = transfers().lock().expect("transfer table poisoned");
    let mut entries: Vec<Value> = sessions
        .iter()
        .map(|(transfer_id, session)| {
            json!({
                "transfer_id": transfer_id,
                "direction": if session.is_upload { "put" } else { "get" },
                "path": session.path,
                "size": session.size,
                "written": session.written,
                "last_activity_ms": session.last_activity_ms,
            })
        })
        .collect();
    // 顺序稳定: 回执要对账, HashMap 的迭代顺序不能进证据
    entries.sort_by(|a, b| {
        a["transfer_id"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["transfer_id"].as_str().unwrap_or_default())
    });
    Ok(json!({
        "kind": "file-transfer",
        "state": "listed",
        "active": entries.len(),
        "transfers": entries,
    }))
}

/// 空闲过期: 超过 `idle_timeout_ms` 没有进展的会话走与 `@file-cancel` 相同的回收路径。
///
/// 收 now 参数而不是内部取时钟, 单测才能确定性地构造过期而不用真睡。
/// 返回 (回收的会话数, 删掉的半成品数)。
pub(crate) fn reap_idle_transfers(now_ms: u64, idle_timeout_ms: u64) -> (usize, usize) {
    let expired: Vec<TransferSession> = {
        let mut sessions = transfers().lock().expect("transfer table poisoned");
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| {
                now_ms.saturating_sub(session.last_activity_ms) >= idle_timeout_ms
            })
            .map(|(id, _)| id.clone())
            .collect();
        expired_ids
            .into_iter()
            .filter_map(|id| sessions.remove(&id))
            .collect()
    };
    let partials = expired
        .iter()
        .filter(|session| reap_partial(session))
        .count();
    (expired.len(), partials)
}

/// 测试专用: 把一条会话的"最近进展"时刻改成指定值。
///
/// 过期判定必须能区分"同一次调用里, 一条过期一条没过期", 而两条会话若都在同一毫秒创建,
/// 真实时钟做不到这个区分 —— 与其让测试真睡, 不如把时刻写死。
#[cfg(test)]
fn stamp_last_activity_for_tests(transfer_id: &str, last_activity_ms: u64) {
    let mut sessions = transfers().lock().expect("transfer table poisoned");
    if let Some(session) = sessions.get_mut(transfer_id) {
        session.last_activity_ms = last_activity_ms;
    }
}

static TRANSFER_MAINTENANCE: OnceLock<Mutex<Option<TransferMaintenance>>> = OnceLock::new();

/// 文件传输会话的空闲过期 worker。
///
/// 为什么必须是后台线程而不是"下次请求时顺手扫一遍": 残留的正是**没有后续请求**的场景 ——
/// 传输被拒或中断后 daemon 安静下来, 半成品就在盘上一直躺着。惰性扫描只能保证
/// "有人再来时会被清", 保证不了"盘上没有残留"。
pub struct TransferMaintenance {
    stop: Arc<(Mutex<bool>, Condvar)>,
    join: Option<thread::JoinHandle<()>>,
}

impl TransferMaintenance {
    pub fn start(interval: Duration, idle_timeout_ms: u64) -> Self {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = Arc::clone(&stop);
        let join = thread::spawn(move || loop {
            let (reaped, partials) = reap_idle_transfers(now_ms(), idle_timeout_ms);
            if reaped > 0 {
                log::info!(
                    "文件传输空闲过期: 回收会话={reaped}, 删除半成品={partials}"
                );
            }

            let (lock, wake) = &*worker_stop;
            let Ok(stopped) = lock.lock() else {
                return;
            };
            if *stopped {
                return;
            }
            let Ok((stopped, _)) = wake.wait_timeout(stopped, interval) else {
                return;
            };
            if *stopped {
                return;
            }
        });

        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for TransferMaintenance {
    fn drop(&mut self) {
        let (lock, wake) = &*self.stop;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            wake.notify_all();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 启动文件传输的空闲过期 worker。跟 `initialize_durable_observation_state` 同模式:
/// 幂等, 重复调用会停掉旧 worker 再起新的。
pub fn initialize_file_transfer_maintenance(
    config: &crate::config::FileTransferConfig,
) -> std::io::Result<()> {
    let maintenance = TransferMaintenance::start(
        Duration::from_millis(config.sweep_interval_ms),
        config.idle_timeout_ms,
    );
    let slot = TRANSFER_MAINTENANCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| std::io::Error::other("文件传输 worker 锁已中毒"))?;
    *guard = Some(maintenance);
    Ok(())
}

/// 文件摘要 (流式 8KB 读, 不整载内存)。
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_id(tag: &str) -> String {
        format!("{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
    }

    /// 把路径转义成能塞进 JSON 字符串字面量的形态。
    ///
    /// 这批用例刻意走**手拼 JSON 文本** (而非 `json!`) 打真实文本协议入口,
    /// 代价是路径必须自己转义: Windows 的 `C:\Users\...` 里那个裸反斜杠是
    /// 非法 JSON 转义, 直接插进 payload 会被产品侧正确拒成 "invalid escape"
    /// —— 产品没错, 是测试拼串时漏了转义。转义规则与 `control_flow/tests.rs`
    /// 的同名助手保持一致。
    fn escape_json(input: &str) -> String {
        input.replace('\\', "\\\\").replace('"', "\\\"")
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        for bad in ["../../etc/passwd", "a/b/../../../x", "../evil"] {
            assert!(
                validate_transfer_path(bad, "put-file").is_err(),
                "应拒绝穿越: {bad}"
            );
        }
    }

    #[test]
    fn accepts_plain_and_absolute_paths() {
        assert!(validate_transfer_path("out/file.bin", "put-file").is_ok());
        assert!(validate_transfer_path("/tmp/yino-test/x.bin", "put-file").is_ok());
        // 单个 ".." 文件名? — Path::components 里 ".." 是 ParentDir, 同样拒
        assert!(validate_transfer_path("..", "put-file").is_err());
    }

    #[test]
    fn full_upload_flow_commits_and_verifies() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("upload-ok.bin");
        let tid = unique_id("up");

        let payload = br"hello file transfer";
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let sha = format!("{:x}", hasher.finalize());

        let open = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":{},"sha256":"{sha}"}}"#,
            escape_json(&target.display().to_string()),
            payload.len()
        ))
        .unwrap();
        assert_eq!(open["state"], "open");

        let data = BASE64_STANDARD.encode(payload);
        let chunk = execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        assert_eq!(chunk["written"], payload.len() as u64);

        let end = execute_file_end(&format!(
            r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#
        ))
        .unwrap();
        assert_eq!(end["state"], "committed");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            payload,
            "提交后目标文件内容一致"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn digest_mismatch_cleans_up_partial() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-mismatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("upload-bad.bin");
        let tid = unique_id("bad");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":5}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"abcde");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let wrong_sha = "0".repeat(64);
        let err = execute_file_end(&format!(
            r#"{{"transfer_id":"{tid}","sha256":"{wrong_sha}"}}"#
        ))
        .unwrap_err();
        assert!(err.to_string().contains("摘要不符"));
        // 半成品已清理 (NSIS 教训同族: 坏文件不留盘)
        assert!(!target.exists(), "坏文件不得留在目标路径");
        assert!(!dir.join("upload-bad.yino-partial").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_order_chunk_is_rejected() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-ooo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ooo.bin");
        let tid = unique_id("ooo");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"xxxxx");
        let err = execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":5,"data":"{data}"}}"#
        ))
        .unwrap_err();
        assert!(err.to_string().contains("offset 不连续"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_roundtrip_matches_upload() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.bin");
        std::fs::write(&source, b"download me roundtrip").unwrap();

        let tid = unique_id("dl");
        let meta = execute_get_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}"}}"#,
            escape_json(&source.display().to_string())
        ))
        .unwrap();
        let size = meta["size"].as_u64().unwrap();
        let expected_sha = meta["sha256"].as_str().unwrap();

        // 分两次读 (offset 推进), 验证流式与拼装
        let part1 =
            execute_file_read(&format!(r#"{{"transfer_id":"{tid}","offset":0,"length":10}}"#))
                .unwrap();
        assert_eq!(part1["length"], 10);
        assert_eq!(part1["eof"], false);
        let part2 =
            execute_file_read(&format!(r#"{{"transfer_id":"{tid}","offset":10,"length":11}}"#))
                .unwrap();
        assert_eq!(part2["eof"], true);

        // 拼回内容比对
        let mut raw = Vec::new();
        for part in [&part1, &part2] {
            let data = part["data"].as_str().unwrap();
            raw.extend_from_slice(&BASE64_STANDARD.decode(data).unwrap());
        }
        assert_eq!(raw, b"download me roundtrip");
        assert_eq!(size, raw.len() as u64);
        assert_eq!(expected_sha, format!("{:x}", Sha256::digest(&raw)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversize_chunk_is_rejected() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tid = unique_id("big");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":1}}"#,
            escape_json(&dir.join("big.bin").display().to_string())
        ))
        .unwrap();
        let big = "A".repeat(CHUNK_BASE64_LIMIT + 1);
        let err = execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{big}"}}"#
        ))
        .unwrap_err();
        assert!(err.to_string().contains("载荷超过上限"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 会话表是进程级单例: 所有读表或改表的用例串行化。锁放模块顶层唯一定义,
    /// 不在每个用例里各建一把 (同 docs/solutions/best-practices/
    /// parallel-test-global-state-single-lock.md 的四例收敛纪律)。
    static TRANSFER_TABLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 只在"我这条用例的 id"上做断言, 不断言整表条数 —— 同进程并行跑的其他用例
    /// 随时可能在表里放着它们的会话, 整表计数必然 flaky。
    fn active_ids() -> Vec<String> {
        execute_file_list().unwrap()["transfers"]
            .as_array()
            .expect("transfers 必须是数组")
            .iter()
            .filter_map(|entry| entry["transfer_id"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn cancel_drops_registry_entry_and_partial() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("cancel-me.bin");
        let tid = unique_id("cancel");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"xxxxx");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = dir.join("cancel-me.bin.yino-partial");
        assert!(partial.exists(), "写入后应留下半成品");
        assert_eq!(active_ids().iter().any(|id| id == &tid), true);

        let cancelled = execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        assert_eq!(cancelled["partial_removed"], true);
        assert!(!partial.exists(), "取消后半成品必须消失");
        assert!(!target.exists(), "取消不得凭空造出目标文件");
        assert!(
            !active_ids().iter().any(|id| id == &tid),
            "取消后注册表不得残留该 id"
        );

        // 幂等: 重复取消是正常调用顺序 (客户端可能重试), 不是错误
        let again = execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        assert_eq!(again["state"], "absent");
        assert_eq!(again["partial_removed"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_leaves_existing_target_file_untouched() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-keep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("existing.bin");
        std::fs::write(&target, b"do not delete me").unwrap();
        let tid = unique_id("keep");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":5}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"abcde");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();

        // 回收只针对半成品。取消一次上传不得动目标路径上原有的文件。
        assert_eq!(std::fs::read(&target).unwrap(), b"do not delete me");
        assert!(!dir.join("existing.bin.yino-partial").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_of_download_session_drops_entry_without_touching_source() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-dlcancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.bin");
        std::fs::write(&source, b"read only source").unwrap();
        let tid = unique_id("dlcancel");

        execute_get_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}"}}"#,
            escape_json(&source.display().to_string())
        ))
        .unwrap();
        assert!(active_ids().iter().any(|id| id == &tid));

        let cancelled = execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        // 下载会话没有半成品, 被取消的也不该删源文件
        assert_eq!(cancelled["partial_removed"], false);
        assert!(!active_ids().iter().any(|id| id == &tid));
        assert_eq!(std::fs::read(&source).unwrap(), b"read only source");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_reap_reclaims_stale_session_and_keeps_progressing_one() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stale = unique_id("stale");
        let fresh = unique_id("fresh");

        for (id, name) in [(&stale, "stale.bin"), (&fresh, "fresh.bin")] {
            execute_put_file(&format!(
                r#"{{"transfer_id":"{id}","path":"{}","size":10}}"#,
                escape_json(&dir.join(name).display().to_string())
            ))
            .unwrap();
            let data = BASE64_STANDARD.encode(b"xxxxx");
            execute_file_chunk(&format!(
                r#"{{"transfer_id":"{id}","offset":0,"data":"{data}"}}"#
            ))
            .unwrap();
        }

        // 用一个小数值的假时钟: 只可能命中本用例自己写死的两条会话, 别处用真实
        // epoch 毫秒戳的会话在 saturating_sub 下差值归零, 不会被误回收。
        stamp_last_activity_for_tests(&stale, 1_000);
        stamp_last_activity_for_tests(&fresh, 10_000);

        let (reaped, partials) = reap_idle_transfers(10_500, 5_000);
        assert_eq!(reaped, 1, "只应回收 stale 那条");
        assert_eq!(partials, 1, "stale 的半成品应被删掉");
        assert!(!dir.join("stale.bin.yino-partial").exists());
        assert!(dir.join("fresh.bin.yino-partial").exists(), "在推进的会话不受影响");

        let ids = active_ids();
        assert!(!ids.iter().any(|id| id == &stale), "过期会话应离开注册表");
        assert!(ids.iter().any(|id| id == &fresh), "未过期会话应仍在注册表");

        // 收尾: 把 fresh 也回收掉, 别把会话留在表里影响后续用例
        execute_file_cancel(&format!(r#"{{"transfer_id":"{fresh}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopening_same_id_on_another_path_reclaims_the_old_partial() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-reopen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tid = unique_id("reopen");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10}}"#,
            escape_json(&dir.join("first.bin").display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        assert!(dir.join("first.bin.yino-partial").exists());

        // 同 id 换一条路径重开: 旧半成品不在新路径上, 没有别的代码会清理它
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10}}"#,
            escape_json(&dir.join("second.bin").display().to_string())
        ))
        .unwrap();
        assert!(!dir.join("first.bin.yino-partial").exists(), "旧路径的半成品应被回收");
        // 同 id 但换了路径, 进度不继承 —— 新会话从 0 起
        let reopened = execute_file_list().unwrap();
        let entry = reopened["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["transfer_id"] == tid.as_str())
            .cloned()
            .expect("新会话应在表里");
        assert_eq!(entry["written"], 0);

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
