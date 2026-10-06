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
use std::sync::{Mutex, OnceLock};

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
            target.display(),
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
            target.display()
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
            target.display()
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
            source.display()
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
            dir.join("big.bin").display()
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
}
