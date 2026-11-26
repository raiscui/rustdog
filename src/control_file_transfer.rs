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
    // 准入必须早于任何落盘动作. 放在 remove_file 之后就是"拒绝带副作用" ——
    // 一次被拒的重试会把上一次收到的字节抹掉, 而那正是本模块要消除的行为.
    //
    // 同 id 的情况分两种, 不能一拒了之: 断线重连后客户端就是拿同一个 id + resume:true 回来的
    // (specs/file-transfer-control-spec.md 的"传输 SHALL 可续传"就是这条), 拒掉它等于把续传
    // 这个能力本身拒掉 —— 而且错误信息让人去 @file-cancel, cancel 又会删半成品, 等于绕一圈
    // 回到从 0 开始.
    if let Some(existing) = sessions.get_mut(&transfer_id) {
        // 与下面的路径占用检查用同一个比较口径: 两处若对"同一个文件"判断不一致,
        // ./a.bin 与 a.bin 这种同义写法就会一边算续传、另一边算换目标.
        let same_target = temp_path_matches(existing.temp_path.as_deref(), &temp_path);
        if resume && same_target {
            // 续传式重开: 会话继续用, 只刷新声明与活跃时刻, 已收字节(真相源是半成品长度)不动
            existing.size = size;
            existing.expected_sha256 = expected_sha256;
            existing.last_activity_ms = now_ms();
            let written = existing.written;
            // 只有真的从非 0 接着写才算 resumed: 会话存在但一个字节都没收到时, 报 "resumed" 是
            // 在撒谎 —— 客户端会以为续上了. resume_honored 就是这个事实的判定位.
            let honored = written > 0;
            return Ok(json!({
                "kind": "file-transfer",
                "state": if honored { "resumed" } else { "open" },
                "transfer_id": transfer_id,
                "written": written,
                "next_offset": written,
                "resume_honored": honored,
            }));
        }
        return Err(std::io::Error::other(format!(
            "file_transfer_id_in_use: transfer_id {transfer_id} 已有活跃会话 (已收 {} 字节, 目标 {}{}); 续传请用 resume:true, 换目标请先 @file-cancel",
            existing.written,
            existing.path.display(),
            if same_target { "" } else { ", 与本次请求不同" }
        )));
    }
    // 别的 id 已持有同一暂存路径: 拒绝. 暂存文件名从**目标路径**推导, 两个 id 指向同一目标
    // 就共用一个文件, 交错写会让先到的 @file-end 把混合字节的哈希算过、把损坏的文件 rename
    // 到目标上, 全程没有一处报错.
    if let Some(holder) = sessions
        .iter()
        .find(|(_, other)| temp_path_matches(other.temp_path.as_deref(), &temp_path))
        .map(|(other_id, _)| other_id.clone())
    {
        return Err(std::io::Error::other(format!(
            "file_transfer_path_in_use: 暂存路径 {} 已被活跃会话 {holder} 持有; 等它完成或先 @file-cancel 它",
            temp_path.display()
        )));
    }

    let written = if resume {
        // 续传: 保留已有 partial 的进度; 无 partial 从 0 起
        std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0)
    } else {
        // 重开: 清掉旧 partial (半损坏文件不留盘, NSIS 教训同族)
        let _ = std::fs::remove_file(&temp_path);
        0
    };
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
    // resume:true 但半成品不存在 (从没上传过, 或已被 sweep 按窗口删掉) 时起始 offset 是 0.
    // 这时报 "resumed" 会把"重来了"伪装成"续上了", 而后者正是本模块反复要防的静默重传.
    let resume_honored = resume && written > 0;
    Ok(json!({
        "kind": "file-transfer",
        "state": if resume_honored { "resumed" } else { "open" },
        "resume_honored": resume_honored,
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

    // 整个提交过程都持着会话表锁: 校验、哈希、rename、退役必须在同一个临界区里.
    // 中间放锁会留下一条真实的错配路径 —— 客户端在 @file-end 超时后重试, 并发发现是
    // 取消 + 立即以同 id 同目标重开: 取消把暂存文件 unlink, 重开在同一条暂存路径上建新文件,
    // 然后本次 @file-end 继续跑, 把**新会话的半成品** rename 到目标上, 最后按 id 退役掉新会话.
    // 结果是: 第一个请求回 state:"committed" 且带着旧内容的 sha, 而盘上是第二次上传的前缀.
    // 代价是哈希期间其他传输的 chunk 写入会排队 —— 用一次串行换掉一条无损却错得很安静的路径.
    let mut sessions = transfers().lock().expect("transfer table poisoned");
    let Some(session) = sessions.get(&transfer_id) else {
        return Err(std::io::Error::other(format!(
            "@file-end 未知 transfer_id: {transfer_id}"
        )));
    };
    if !session.is_upload {
        return Err(std::io::Error::other("@file-end 只用于上传会话"));
    }
    let final_path = session.path.clone();
    let temp_path = session.temp_path.clone().expect("upload 会话必有暂存路径");
    let size = session.size;
    let written = session.written;
    let declared_in_put = session.expected_sha256.clone();

    // 双校验: 字节数先查 (便宜), 摘要后算 (贵)
    if written != size {
        let _ = std::fs::remove_file(&temp_path);
        sessions.remove(&transfer_id);
        return Err(std::io::Error::other(format!(
            "@file-end 字节数不符: 已写 {written}, 声明 {size} — 半成品已清理"
        )));
    }
    let actual_sha256 = sha256_file(&temp_path)?;
    if !actual_sha256.eq_ignore_ascii_case(&declared_sha256) {
        let _ = std::fs::remove_file(&temp_path);
        sessions.remove(&transfer_id);
        return Err(std::io::Error::other(format!(
            "@file-end 摘要不符: 声明 {declared_sha256}, 实际 {actual_sha256} — 半成品已清理"
        )));
    }
    // 会话里声明的 sha256 (若有) 也要一致 — 防控制端两次声明漂移
    if let Some(expected) = &declared_in_put {
        if !actual_sha256.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&temp_path);
            sessions.remove(&transfer_id);
            return Err(std::io::Error::other(
                "@file-end 与 @put-file 声明摘要不一致 — 半成品已清理",
            ));
        }
    }

    // 提交: 原子改名到目标 (同目录 rename, 失败回退为复制)
    if std::fs::rename(&temp_path, &final_path).is_err() {
        std::fs::copy(&temp_path, &final_path).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            sessions.remove(&transfer_id);
            std::io::Error::other(format!("提交文件失败 (rename 与 copy 均失败): {e}"))
        })?;
        let _ = std::fs::remove_file(&temp_path);
    }
    // 提交成功即退役会话. 不退的话, 这条已完成的条目会在 TTL 到期时被 sweep 拿去"回收"它的
    // 暂存路径 —— 而那个路径是按目标文件推导的, 同一目标的**下一次**上传正用着它.
    sessions.remove(&transfer_id);
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

/// 暂存路径是否指同一个文件。
///
/// 直接比 `PathBuf` 是**文本相等**, 挡不住 `a.bin` 与 `./a.bin` 这类同义写法; 在 Windows
/// (大小写不敏感卷) 上也挡不住 `Report.pdf` 与 `report.pdf`. 这里先做词法规范化, 再按平台
/// 决定是否折叠大小写 —— 与 `validate_transfer_path` 的口径一致: 词法层面尽力, 不做
/// `canonicalize` (它要求路径存在, 而暂存文件此刻可能还没创建).
///
/// 已知未覆盖: macOS 默认的 APFS 也是大小写不敏感, 但 `cfg!(windows)` 判不出来 —— 那一侧
/// 残留一个"同目标、仅字母大小写不同、并发上传"的窗口. 触发它需要客户端自己制造这种请求,
/// 本规格把它记为已知边界而不是假装已解决.
fn temp_path_matches(a: Option<&Path>, b: &Path) -> bool {
    let Some(a) = a else {
        return false;
    };
    let normalize = |path: &Path| {
        let joined = path
            .components()
            .filter(|component| !matches!(component, std::path::Component::CurDir))
            .collect::<PathBuf>();
        if cfg!(windows) {
            joined.to_string_lossy().to_lowercase()
        } else {
            joined.to_string_lossy().into_owned()
        }
    };
    normalize(a) == normalize(b)
}

/// 删一个暂存文件。NotFound 视为已删 (幂等), 其他错误记 warn 但不算失败 ——
/// 回收是尽力而为, 它不该让取消 / 过期本身失败。
fn reap_file(temp_path: &Path) -> bool {
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

/// 摘掉一条会话并回收它的暂存文件, 返回 (会话, 是否真的删掉了半成品)。
/// **调用方必须持有会话表锁。**
///
/// 两个不变量都靠"持锁"成立, 缺一个就会踩掉别人正在写的字节:
///
/// 1. **摘表与 unlink 之间不能放锁.** 否则并发的 `@put-file resume:true` 会在这两步之间
///    读走这个半成品的长度、登记成新会话, 随后文件被删 —— 新会话带着旧 offset 继续写,
///    前缀丢了, 最后摘要校验失败。
/// 2. **暂存路径可能被另一条活跃会话持有.** 暂存文件名从目标路径推导
///    (`<target>.<ext>.yino-partial`), 指向同一目标的两条上传共用同一个文件; 别的会话
///    还拿着它的时候不能删, 否则会毁掉那条正在推进的传输。
fn remove_session_locked(
    sessions: &mut HashMap<String, TransferSession>,
    transfer_id: &str,
) -> Option<(TransferSession, bool)> {
    let session = sessions.remove(transfer_id)?;
    let partial_removed = match session.temp_path.as_deref() {
        Some(temp_path) => {
            let held_by_other = sessions
                .values()
                .any(|other| other.temp_path.as_deref() == Some(temp_path));
            if held_by_other {
                log::warn!(
                    "文件传输半成品仍被其他活跃会话持有, 跳过删除: path={}, transfer_id={transfer_id}",
                    temp_path.display()
                );
                false
            } else {
                reap_file(temp_path)
            }
        }
        None => false,
    };
    Some((session, partial_removed))
}

/// `@file-cancel`: 主动取消一次传输 (上传或下载), 语义与空闲过期完全相同 ——
/// 移除会话条目 + 删半成品。只删暂存文件, **不碰**目标路径上已有的文件。
/// 幂等: 未知 transfer_id 返回 `state:"absent"`, 不算错误 (重复取消是正常调用顺序)。
pub fn execute_file_cancel(payload: &str) -> std::io::Result<Value> {
    const VERB: &str = "file-cancel";
    let value = parse_object(payload, VERB)?;
    let transfer_id = require_string(&value, "transfer_id", VERB)?.to_owned();

    let (state, partial_removed) = {
        let mut sessions = transfers().lock().expect("transfer table poisoned");
        match remove_session_locked(&mut sessions, &transfer_id) {
            Some((_, partial_removed)) => ("cancelled", partial_removed),
            None => ("absent", false),
        }
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
///
/// 今天所有中止原因都是终态的 (显式取消、摘要不符、门禁在 open 拒绝), 所以删半成品一律成立。
/// RAI-367 落地"中途路径丢失"这个**可续传**中止后, 这里必须按规格 §2.5 分开: 可续传的半成品
/// 留到 `bulk.resume_window_ms`, 否则续传 offset (真相源就是半成品的文件长度) 会静默归零。
pub(crate) fn reap_idle_transfers(now_ms: u64, idle_timeout_ms: u64) -> (usize, usize) {
    let mut reaped = 0;
    let mut partials = 0;
    {
        let mut sessions = transfers().lock().expect("transfer table poisoned");
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| {
                now_ms.saturating_sub(session.last_activity_ms) >= idle_timeout_ms
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired_ids {
            match remove_session_locked(&mut sessions, &id) {
                Some((_, partial_removed)) => {
                    reaped += 1;
                    if partial_removed {
                        partials += 1;
                    }
                }
                None => {}
            }
        }
    }
    (reaped, partials)
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

    #[test]
    fn committed_session_is_retired_so_a_later_upload_keeps_its_partial() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-retire-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("shared-target.bin");
        let target_json = escape_json(&target.display().to_string());

        // A: 完整上传到目标路径并提交
        let first = unique_id("first");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{first}","path":"{target_json}","size":5}}"#
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"AAAAA");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{first}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let sha = format!("{:x}", Sha256::digest(b"AAAAA"));
        execute_file_end(&format!(
            r#"{{"transfer_id":"{first}","sha256":"{sha}"}}"#
        ))
        .unwrap();

        // 提交成功后会话必须退役: 留着它, TTL 到期时 sweep 会把**同一个目标路径**推导出的
        // 暂存文件拿去"回收", 而下面 B 正用着那个文件。
        assert!(
            !active_ids().iter().any(|id| id == &first),
            "已提交的会话不得留在注册表里"
        );

        // B: 不同 id, 同一目标路径, 正在推进
        let second = unique_id("second");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{second}","path":"{target_json}","size":5}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{second}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = dir.join("shared-target.bin.yino-partial");
        assert!(partial.exists());

        // 拿 A 的 id 去取消 (客户端重试的常见形态) 不得伤到 B
        let cancelled = execute_file_cancel(&format!(r#"{{"transfer_id":"{first}"}}"#)).unwrap();
        assert_eq!(cancelled["state"], "absent");
        assert!(partial.exists(), "无权取消的 id 不得删掉别人的半成品");

        execute_file_cancel(&format!(r#"{{"transfer_id":"{second}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reclaim_skips_a_partial_still_held_by_another_session() {
        // open 准入已经让"两条活跃会话共用同一暂存路径"不可能发生, 所以这条按白盒构造状态:
        // 它测的是回收路径上最后一道防线 —— 假如将来有人动了 open 的准入顺序, 这里仍然不许
        // 删掉别人正在写的文件. 静默毁掉一条进行中的传输是不可恢复的, 值得留一道冗余检查.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-shared-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let partial = dir.join("contended.bin.yino-partial");
        std::fs::write(&partial, b"xxxxx").unwrap();

        let holder = unique_id("holder");
        let victim = unique_id("victim");
        let session = |id: &str| TransferSession {
            path: dir.join("contended.bin"),
            size: 10,
            expected_sha256: None,
            written: 5,
            temp_path: Some(partial.clone()),
            is_upload: true,
            last_activity_ms: now_ms(),
            };
        {
            let mut sessions = transfers().lock().unwrap();
            sessions.insert(holder.clone(), session(&holder));
            sessions.insert(victim.clone(), session(&victim));
        }

        let removed = {
            let mut sessions = transfers().lock().unwrap();
            remove_session_locked(&mut sessions, &victim)
        };
        let (_, partial_removed) = removed.expect("victim 应在表里");
        assert_eq!(partial_removed, false);
        assert!(partial.exists(), "仍被其他活跃会话持有的半成品不得被回收");

        let removed = {
            let mut sessions = transfers().lock().unwrap();
            remove_session_locked(&mut sessions, &holder)
        };
        let (_, partial_removed) = removed.expect("holder 应在表里");
        assert_eq!(partial_removed, true);
        assert!(!partial.exists(), "最后一个持有者摘除时才删");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_is_refused_when_another_session_holds_the_same_partial_path() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-inuse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("contended-open.bin");
        let target_json = escape_json(&target.display().to_string());
        let data = BASE64_STANDARD.encode(b"xxxxx");

        let holder = unique_id("holder");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{holder}","path":"{target_json}","size":10}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{holder}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = dir.join("contended-open.bin.yino-partial");
        let before = std::fs::metadata(&partial).unwrap().len();

        // 另一个 id 指向同一目标: 必须拒绝, 且**不能**先把 holder 的半成品删掉再拒绝
        let intruder = unique_id("intruder");
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{intruder}","path":"{target_json}","size":10}}"#
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_path_in_use"),
            "应拒绝并给出稳定码: {err}"
        );
        assert_eq!(
            std::fs::metadata(&partial).unwrap().len(),
            before,
            "拒绝不得动 holder 的半成品"
        );

        // holder 自己仍可继续写
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{holder}","offset":5,"data":"{data}"}}"#
        ))
        .unwrap();
        execute_file_cancel(&format!(r#"{{"transfer_id":"{holder}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_cancel_executes_from_the_compact_object_syntax() {
        // 只断言"解析成了 FileCancel 变体"是不够的: 行协议面向模型, 模型会写
        // `@file-cancel:{transfer_id:"x"}` 这种不带引号的键. 这条从解析一路走到执行,
        // 证明那个写法真的能取消掉一条传输, 而不是在 serde_json 那里就断了.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-compact-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("compact.bin");
        let tid = unique_id("compact");

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
        let partial = dir.join("compact.bin.yino-partial");
        assert!(partial.exists());

        let line = format!(r#"@file-cancel:{{transfer_id:"{tid}"}}"#);
        let parsed = crate::control_protocol::parse_control_line(&line).unwrap();
        let crate::control_protocol::ControlParseResult::Control(request) = parsed else {
            panic!("应解析为控制请求");
        };
        let crate::control_protocol::ControlCommand::FileCancel(payload) = request.command else {
            panic!("应解析为 FileCancel");
        };
        let cancelled = execute_file_cancel(&payload).unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        assert_eq!(cancelled["partial_removed"], true);
        assert!(!partial.exists());

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
    fn resume_accepts_a_differently_spelled_but_identical_target() {
        // ./a.bin 与 a.bin 在 Path 组件层面不相等, 但指的是同一个文件. 若续传判定用纯 Path
        // 相等, 客户端换个写法重连就会被拒成"换目标", 而错误信息只给 @file-cancel —— 那会
        // 删掉半成品, 又回到从 0 开始. 两处判定必须用同一个比较口径.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-alias-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("alias.bin");
        let dotted = dir.join(".").join("alias.bin");
        let tid = unique_id("alias");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10}}"#,
            escape_json(&plain.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();

        let resumed = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&dotted.display().to_string())
        ))
        .unwrap();
        assert_eq!(resumed["state"], "resumed");
        assert_eq!(resumed["written"], 5);

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_without_a_partial_reports_open_not_resumed() {
        // resume:true 但盘上没有半成品 (从没传过, 或已被窗口 sweep 删掉) 时起始 offset 是 0.
        // 报 "resumed" 会把"重来了"伪装成"续上了" —— 静默重传正是这条 lane 反复要防的东西.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-nopartial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("nopartial.bin");
        let tid = unique_id("nopartial");

        let opened = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        assert_eq!(opened["state"], "open");
        assert_eq!(opened["resume_honored"], false);
        assert_eq!(opened["written"], 0);

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_reuses_the_live_session_instead_of_being_refused() {
        // 断线重连后客户端就是拿同一个 id + resume:true 回来的 —— 这是
        // specs/file-transfer-control-spec.md 的"传输 SHALL 可续传"本身. 准入若把它一并拒掉,
        // 续传这个能力就等于不存在.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("attach.bin");
        let target_json = escape_json(&target.display().to_string());
        let tid = unique_id("attach");
        let first = BASE64_STANDARD.encode(b"AAAAA");
        let second = BASE64_STANDARD.encode(b"BBBBB");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{first}"}}"#
        ))
        .unwrap();

        // 重连: 同一个 id + resume:true → 接着用, 不从 0 重来, 也不报错
        let resumed = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":true}}"#
        ))
        .unwrap();
        assert_eq!(resumed["state"], "resumed");
        assert_eq!(resumed["written"], 5);
        assert_eq!(resumed["next_offset"], 5);

        // 续传之后继续写, 并正常提交
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":5,"data":"{second}"}}"#
        ))
        .unwrap();
        let sha = format!("{:x}", Sha256::digest(b"AAAAABBBBB"));
        let done = execute_file_end(&format!(
            r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#
        ))
        .unwrap();
        assert_eq!(done["state"], "committed");
        assert_eq!(std::fs::read(&target).unwrap(), b"AAAAABBBBB");

        // 活跃会话上换目标路径仍必须拒绝: 复用 id 去写另一个文件是错误, 不是续传
        let other = dir.join("other.bin");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10}}"#
        ))
        .unwrap();
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&other.display().to_string())
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_id_in_use"),
            "换目标不应被当成续传: {err}"
        );
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_is_refused_while_the_same_id_has_a_live_session() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idlive-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("live.bin");
        let target_json = escape_json(&target.display().to_string());
        let tid = unique_id("live");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = dir.join("live.bin.yino-partial");
        let before = std::fs::metadata(&partial).unwrap().len();

        // 同一 id 再来一次必须拒绝: 否则这次 open 会替换掉活跃会话, resume:false 还会把
        // 它正在写的文件删掉
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10}}"#
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_id_in_use"),
            "应拒绝并给稳定码: {err}"
        );
        assert_eq!(
            std::fs::metadata(&partial).unwrap().len(),
            before,
            "拒绝不得动活跃会话的半成品"
        );
        // 活跃会话仍在表里, 进度没丢
        let listed = execute_file_list().unwrap();
        let entry = listed["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["transfer_id"] == tid.as_str())
            .cloned()
            .expect("活跃会话应仍在表里");
        assert_eq!(entry["written"], before);

        // 取消之后同 id 可以正常重开 (新的一次传输)
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10}}"#
        ))
        .unwrap();
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

}
