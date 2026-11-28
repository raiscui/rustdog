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

/// 暂存文件所在的**保留目录** (与目标文件同级的子目录)。
///
/// 为什么是目录而不是"目标名 + 后缀": 只要暂存名和目标名住在同一个命名空间, 就永远存在
/// "某个目标名恰好等于别人的暂存名"的拼法 —— 后缀版本已经先后踩到五种: 大小写变体、
/// Win32 结尾的点/空格、NTFS 备用数据流 (`x.yino-partial:s`)、8.3 短名, 以及之后的任何一种.
/// 把暂存文件放进一个**目标路径不许出现**的目录, 就把"逐个封堵别名"换成"两个命名空间不重叠"
/// 一条规则: `:` / `~` / 大小写 / 结尾点空格再怎么拼, 都改变不了"它在不在这个目录里".
const STAGING_DIR: &str = ".rdog-partial";

/// 暂存文件名后缀 (只用于识别, 不承担隔离职责 —— 隔离由 `STAGING_DIR` 提供)。
const STAGING_SUFFIX: &str = ".partial";

/// 半成品索引所在目录名与文件名 (默认落在平台 state 目录下, 与 observation 同一套约定)。
const INDEX_DIR: &str = "file-transfer";
const INDEX_FILE: &str = "partials.json";

/// 单个 `@file-chunk` / `@file-read` 的**载荷字节**上限 (客户端按它切块)。
///
/// 从 `CHUNK_BASE64_LIMIT` 推导, 不另立常量: 编码后长度 `4*ceil(n/3)`, 留出取整的余量即可保证
/// 每一块都塞得进一行。
pub(crate) fn chunk_payload_bytes() -> usize {
    (CHUNK_BASE64_LIMIT - 4) / 4 * 3
}

/// 单个 @file-chunk data 载荷的字节上限 (解码前 base64 文本的字符数上限)。
/// 取帧上限 8MiB 的 80% — 决策 2 的落地: chunk 永远塞得进一行。
const CHUNK_BASE64_LIMIT: usize = (MAX_CONTROL_MESSAGE_BYTES * 4) / 5;

use crate::control_line_read::MAX_CONTROL_MESSAGE_BYTES;

/// 传输会话表。OnceLock + Mutex 与 task_control 的 TASKS 同模式。
static TRANSFERS: OnceLock<Mutex<HashMap<String, TransferSession>>> = OnceLock::new();

fn transfers() -> &'static Mutex<HashMap<String, TransferSession>> {
    TRANSFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `@put-file` 的 `verify_committed:true` 分支: 这条上传是不是**已经提交过了**。
///
/// **判据只有盘上事实** (存在 + 大小相等 + sha256 相等): 天然跨重启, 代价只是重放路径上的一次
/// 哈希。此前有一张内存 `COMMITTED` 表当"快速路径", 评审 (#204 severe 1) 指出它会给出**错误的
/// 成功** —— A 提交后丢回执、B 换了目标, 再跑 A 仍命中缓存并清掉本地 ID, 而盘上已经不是那份字节。
/// 现在**没有缓存**: 每次"已提交"都必须由当前文件内容支撑 (顺带解掉那张表的无界增长)。
///
/// 三种结局, 语义必须分清:
/// - **相等** ⇒ `Committed`。这里有一个**故意的假阳性**: 目标本来就在、内容恰好相同、但不是这次
///   传的, 也会被判成已提交。终态不可区分 (盘上就是这份字节), 按成功返回是对的 —— 但它不证明
///   "本次写入发生过", 别拿它当写入证据。
/// - **目标存在但不符** ⇒ `Mismatch`。绝不返回"已提交" —— 那会把一份不同的成品说成这次的结果。
/// - **目标不存在** ⇒ `Absent`。提交必然留下目标文件, 所以"没有目标"就是"没提交过"的正面证据
///   (不是"未知"), 此时从 0 重来是安全的。**这条推论有前提**: 两次尝试之间没有外部删除/移动
///   目标。被人为删掉后再重放会让文件重新出现 —— 那是可接受的行为, 但它是**假设, 不是定理**。
fn replay_state(path: &Path, size: u64, sha256: &str) -> std::io::Result<ReplayState> {
    // **读不出来 ≠ 不存在**。只有 `NotFound` 才是"没提交过"的正面证据; 不可读 / 符号链接 /
    // 目录都必须走"拒"这一侧 —— 把它们归到 `Absent` 会落到正常 open 并覆盖, 那比 `Mismatch`
    // 危险得多 (覆盖了别的东西, 而用户以为只是重传一次)。
    // **规矩 (RAI-711): `Absent` 的语义只能是 `NotFound`。** 永远不要加"读失败就当作不存在"
    // 的兜底 —— 在这个模块里那等于"覆盖已提交的成品", 而在别处它可能只是无害的宽容。
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ReplayState::Absent),
        Err(err) => {
            return Ok(ReplayState::Unreadable(format!(
                "读不到目标 {} 的元数据 ({err})",
                path.display()
            )))
        }
    };
    // **符号链接与目录即便内容一致也拒**: 提交动作会替换掉链接本身或目录, 所以"内容相等"
    // 不是这里的判据 —— 判据是"提交之后那个路径上会是什么"。
    if metadata.file_type().is_symlink() {
        return Ok(ReplayState::Unreadable(format!(
            "目标 {} 是符号链接, 提交会把它替换掉",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Ok(ReplayState::Unreadable(format!(
            "目标 {} 不是普通文件",
            path.display()
        )));
    }
    if metadata.len() != size {
        return Ok(ReplayState::Mismatch(format!(
            "目标 {} 已存在 {} 字节, 与本次声明的 {size} 不等",
            path.display(),
            metadata.len()
        )));
    }
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(sha256) {
        Ok(ReplayState::Committed)
    } else {
        Ok(ReplayState::Mismatch(format!(
            "目标 {} 的 sha256 是 {actual}, 与本次声明的 {sha256} 不等",
            path.display()
        )))
    }
}

enum ReplayState {
    Committed,
    Mismatch(String),
    /// 目标存在但**读不出来** (权限 / 符号链接 / 非普通文件): 拒, 绝不退化成"不存在"。
    Unreadable(String),
    Absent,
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
    /// 重放会话专用: 对账那一刻目标**不存在**, 所以提交时**不许覆盖**后来出现的任何东西。
    /// 只有"探针说 `Absent` 且本命令带 `verify_committed`"的会话置位 (其余保持覆盖语义)。
    no_replace_publish: bool,
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
    // 目标路径不许落在保留目录里: 那是暂存区的命名空间。允许它, 就等于允许客户端把别人的
    // 暂存文件当成目标来覆盖/删除。
    //
    // 比的是**组件**而不是后缀, 并且按各平台"怎么算同一个名字"来比: 去掉结尾的点与空格
    // (Win32 忽略它们)、Windows 上再剥掉从 `:` 开始的备用数据流部分、最后折叠大小写 ——
    // 一次覆盖五种别名面, 不需要逐个列。过拒无害 (没人会拿 .rdog-partial 当目标目录名),
    // 漏拒会毁数据。
    // 已知残余 (记在 specs/file-transfer-control-spec.md 的「已知边界」):
    // NTFS 上开启 8.3 短名生成时, `.rdog-partial` 还有一个短名别名 (形如
    // `RDOG-P~1`), 客户端若**故意**按短名拼出目标路径就能落进暂存区. 堵它要解析路径真实身份
    // (GetFinalPathNameByHandle), 而暂存目录此刻可能还不存在; 且现代 Windows 卷默认不生成 8.3
    // 短名. 这一条写进规格的已知边界, 不假装已解决 —— 但也不装作不存在.
    //
    // 任意位置都拒, **包括最后一段**: 一个名叫 `.rdog-partial` 的普通文件落在 /d 里之后,
    // 之后所有往 /d 的传输都会因为要建 <dir>/.rdog-partial/ 目录而失败。
    let hits_staging_dir = path.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return false;
        };
        let name = if cfg!(windows) {
            name.split(':').next().unwrap_or(name)
        } else {
            name
        };
        name.trim_end_matches(['.', ' ']).to_lowercase() == STAGING_DIR
    });
    if hits_staging_dir {
        return Err(std::io::Error::other(format!(
            "file_transfer_staging_path_reserved: @{verb} 路径不得包含保留目录 {STAGING_DIR} (该目录是暂存区专用): {raw}"
        )));
    }
    Ok(path.to_path_buf())
}

/// 暂存文件名里由目标名截断后保留的最大字节数。
///
/// 暂存名 = 截断后的目标名 + `.` + 16 位哈希 + `.partial`, 额外开销 1 + 16 + 8 = 25 字节。加上它还要留出
/// 余量给 `NAME_MAX` (常见 255 字节) —— 不截断的话, 原本能传的 239~247 字节长文件名会在第一次
/// `@file-chunk` 上撞 `ENAMETOOLONG`, 而传输在 open 阶段看起来是成功的。
const STAGING_BASENAME_MAX_BYTES: usize = 200;

/// 目标路径 → 暂存文件路径。这是暂存名的**唯一定义**。
///
/// 名字 = `<目标目录>/.rdog-partial/<截断目标名>.<目标串短哈希>.partial`。
///
/// 哈希不是装饰: 没有它, `Model.bin` 与 `model.bin` 在**大小写不敏感**的卷 (macOS 默认 APFS,
/// Windows NTFS) 上会算出两个文本不同、却指向同一个 inode 的暂存名 —— 按文本比较的准入看不见
/// 这种重合, 于是第二条 upload 的 `resume:false` 会把第一条**正在写**的半成品删掉。
///
/// 但**哈希的输入必须先做词法归一**: 直接哈希原始拼写会让 `./a.bin` 与 `a.bin` 得到两个不同的
/// 暂存名 —— 它们是同一个文件, 却会让续传找不到自己的半成品、也让占用检查看不见冲突。归一规则
/// 与 [`temp_path_matches`] 一致: 去掉 `.` 组件, **不**折叠大小写 (折叠会把 `Model.bin` 与
/// `model.bin` 又合回去, 而那两个必须保持不同)。
fn staging_path(target: &Path) -> std::io::Result<PathBuf> {
    let normalized: PathBuf = target
        .components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect();
    let Some(file_name) = normalized.file_name() else {
        return Err(std::io::Error::other("@put-file 路径缺少文件名"));
    };

    let mut base = String::new();
    for ch in file_name.to_string_lossy().chars() {
        if base.len() + ch.len_utf8() > STAGING_BASENAME_MAX_BYTES {
            break;
        }
        base.push(ch);
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(normalized.to_string_lossy().as_bytes())
    );
    let staging_name = format!("{base}.{}{STAGING_SUFFIX}", &digest[..16]);
    Ok(normalized
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(STAGING_DIR)
        .join(staging_name))
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
    // `resume` 是必填字段, 不给默认值: 默认 false 会静默抹掉进度, 默认 true 会在调用方想重来时
    // 静默续上 —— 两个默认值都错, 所以这里既不猜也不兜底 (specs/rdog-bulk-direct-only-spec.md §2.5 D1).
    let Some(resume) = value.get("resume").and_then(Value::as_bool) else {
        return Err(std::io::Error::other(
            "@put-file 缺少必填字段 resume (true=续传窗口内的半成品, false=从 0 重来并删除既有半成品): 不给默认值",
        ));
    };

    let limits = limits();
    if size > limits.max_bytes {
        return Err(std::io::Error::other(format!(
            "file_transfer_size_limit_exceeded: @put-file 声明 {size} 字节, 超过 file_transfer.max_bytes = {}",
            limits.max_bytes
        )));
    }

    let mut no_replace_publish = false;
    // 重放对账 (RAI-711 M1): 客户端只在"上一次没干净收尾"时带这个字段, 所以正常首传不多一次哈希。
    if value
        .get("verify_committed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        match replay_state(&path, size, expected_sha256.as_deref().unwrap_or(""))? {
            ReplayState::Committed => {
                return Ok(json!({
                    "kind": "file-transfer",
                    "state": "committed",
                    "replayed": true,
                    // 能力标记: 这次 open 真的按盘上事实对过账。旧 daemon 没有这个字段 ——
                    // 客户端据它的缺席 fail closed, 而不是以为对账发生了。
                    "reconcile": "disk",
                    "transfer_id": transfer_id,
                    "path": path,
                    "size": size,
                    "next_offset": size,
                }));
            }
            ReplayState::Mismatch(reason) => {
                return Err(std::io::Error::other(format!(
                    "file_transfer_replay_mismatch: {reason} — 不重传、不覆盖"
                )));
            }
            ReplayState::Unreadable(reason) => {
                return Err(std::io::Error::other(format!(
                    "file_transfer_replay_unreadable: {reason} — 不重传、不覆盖 (读不出来不等于不存在)"
                )));
            }
            // 没有目标文件 = 上次没提交过 (提交必然留下目标), 从 0 重来是安全的。
            ReplayState::Absent => {
                // 对账那一刻没有目标: 这条会话在提交时**不许覆盖**后来出现的任何东西
                // (评审 #204 severe 2: probe 与提交之间可以被旧的 `@file-end` 落盘)。
                no_replace_publish = true;
            }
        }
    }

    let temp_path = staging_path(&path)?;

    let mut sessions = transfers().lock().expect("transfer table poisoned");
    // 准入必须早于任何落盘动作. 放在 remove_file 之后就是"拒绝带副作用" ——
    // 一次被拒的重试会把上一次收到的字节抹掉, 而那正是本模块要消除的行为.
    //
    // 同 id 的情况分两种, 不能一拒了之: 断线重连后客户端就是拿同一个 id + resume:true 回来的
    // (specs/file-transfer-control-spec.md 的"传输 SHALL 可续传"就是这条), 拒掉它等于把续传
    // 这个能力本身拒掉 —— 而且错误信息让人去 @file-cancel, cancel 又会删半成品, 等于绕一圈
    // 回到从 0 开始.
    admit_new_session(&sessions, &transfer_id, limits.max_concurrent_sessions)?;
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
            persist_index(&sessions);
            return Ok(json!({
                "kind": "file-transfer",
                "state": if honored { "resumed" } else { "open" },
                "transfer_id": transfer_id,
                "written": written,
                "next_offset": written,
                "resume_honored": honored,
                "reconcile": "disk",
                // 活跃会话没有"过期半成品"这回事: 它一直在被 sweep 盯着, 到不了窗口外。
                "stale_partial_discarded": false,
            }));
        }
        if !same_target {
            // A3/C2: 同 id 换目标是合法的 id 复用. 旧会话移出注册表, 但**旧半成品按它自己的
            // 路径留下** —— 那批字节仍然有效, 删掉等于拿"id 复用"当理由销毁另一个文件的有效
            // 进度. 它转成孤儿, 由续传窗口 (C3) 决定什么时候回收.
            if let Some(old_temp) = existing.temp_path.clone() {
                orphan_partials()
                    .lock()
                    .expect("orphan table poisoned")
                    .insert(old_temp, existing.last_activity_ms);
            }
            sessions.remove(&transfer_id);
        } else {
            return Err(std::io::Error::other(format!(
                "file_transfer_id_in_use: transfer_id {transfer_id} 已有活跃会话 (已收 {} 字节, 目标 {}); 续传请用 resume:true, 换目标请先 @file-cancel",
                existing.written,
                existing.path.display()
            )));
        }
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

    let mut stale_discarded = false;
    // 认领与读长度必须在**同一把锁**里: sweep 可能正好在两者之间把这个孤儿回收掉, 那时回
    // `resumed, written:N` 而盘上已经空了 —— 后续 chunk 会往 offset 0 写, 最后报"摘要不符",
    // 一个跟真实原因无关的错。
    let mut orphans = orphan_partials().lock().expect("orphan table poisoned");
    let written = if resume {
        // 续传: 保留已有 partial 的进度; 无 partial 从 0 起
        let existing = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
        if existing > 0 && partial_is_stale(&temp_path, limits.resume_window_ms) {
            // A5: 窗口外的半成品删掉, offset 归 0 —— 并且必须**回报**, 否则"过期被丢、从 0 开始"
            // 与"续上了"在回执上分不开, 那就是另一个版本的静默重传.
            let _ = std::fs::remove_file(&temp_path);
            stale_discarded = true;
            0
        } else {
            existing
        }
    } else {
        // 重开: 清掉旧 partial (半损坏文件不留盘, NSIS 教训同族)
        let _ = std::fs::remove_file(&temp_path);
        0
    };
    orphans.remove(&temp_path);
    drop(orphans);
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
            no_replace_publish,
        },
    );
    persist_index(&sessions);
    // resume:true 但半成品不存在 (从没上传过, 或已被 sweep 按窗口删掉) 时起始 offset 是 0.
    // 这时报 "resumed" 会把"重来了"伪装成"续上了", 而后者正是本模块反复要防的静默重传.
    let resume_honored = resume && written > 0;
    Ok(json!({
        "kind": "file-transfer",
        "state": if resume_honored { "resumed" } else { "open" },
        "resume_honored": resume_honored,
        "reconcile": "disk",
        "stale_partial_discarded": stale_discarded,
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
    // 第二道门: open 时按**声明**的 size 拒过一次, 但声明可以撒谎 —— 这里按累计写入再拒一次,
    // 上限取"声明的总大小"与"配置上限"的较小者。少了这一道, 客户端声明 size:1 之后持续发
    // chunk 就能让暂存文件无界增长, 并发几条就能把被控端磁盘写满。
    let limits = limits();
    let ceiling = session.size.min(limits.max_bytes);
    if session.written + bytes.len() as u64 > ceiling {
        return Err(std::io::Error::other(format!(
            "file_transfer_size_limit_exceeded: @file-chunk 累计写入 {} 字节会超过上限 {ceiling} (声明 {} / 配置 {})",
            session.written + bytes.len() as u64,
            session.size,
            limits.max_bytes
        )));
    }
    let Some(temp_path) = session.temp_path.clone() else {
        return Err(std::io::Error::other("@file-chunk 会话缺少暂存路径"));
    };
    // 追加前比对盘上的长度与会话账: 两者不等说明**有人动过这个半成品** (孤儿回收把它删了,
    // 或别的进程截断/追加过)。此时继续追加会把字节写到错误的位置, 最后报"摘要不符" ——
    // 一个与真实原因无关的错。明确回一个稳定码, 让调用方知道该重新 open。
    if let Ok(metadata) = std::fs::metadata(&temp_path) {
        if metadata.len() != session.written {
            return Err(std::io::Error::other(format!(
                "file_transfer_partial_changed: 半成品长度 {} 与会话已收 {} 不等, 拒绝继续追加 (重新 @put-file)",
                metadata.len(),
                session.written
            )));
        }
    } else if session.written > 0 {
        return Err(std::io::Error::other(format!(
            "file_transfer_partial_changed: 半成品 {} 已不存在, 而会话仍记着已收 {} 字节 (重新 @put-file)",
            temp_path.display(),
            session.written
        )));
    }
    // 追加写 (open_append 保证 O_APPEND 语义, 与 session.written 一致)
    {
        use std::io::Write;
        // 暂存目录在第一次真正落盘时才创建 —— open 阶段不产生任何磁盘副作用。
        if let Some(staging_dir) = temp_path.parent() {
            std::fs::create_dir_all(staging_dir)?;
        }
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
        reap_file(&temp_path);
        sessions.remove(&transfer_id);
        persist_index(&sessions);
        return Err(std::io::Error::other(format!(
            "@file-end 字节数不符: 已写 {written}, 声明 {size} — 半成品已清理"
        )));
    }
    let actual_sha256 = sha256_file(&temp_path)?;
    if !actual_sha256.eq_ignore_ascii_case(&declared_sha256) {
        reap_file(&temp_path);
        sessions.remove(&transfer_id);
        persist_index(&sessions);
        return Err(std::io::Error::other(format!(
            "@file-end 摘要不符: 声明 {declared_sha256}, 实际 {actual_sha256} — 半成品已清理"
        )));
    }
    // 会话里声明的 sha256 (若有) 也要一致 — 防控制端两次声明漂移
    if let Some(expected) = &declared_in_put {
        if !actual_sha256.eq_ignore_ascii_case(expected) {
            reap_file(&temp_path);
            sessions.remove(&transfer_id);
            persist_index(&sessions);
            return Err(std::io::Error::other(
                "@file-end 与 @put-file 声明摘要不一致 — 半成品已清理",
            ));
        }
    }

    // 重放会话的发布守卫 (评审 #204 severe 2): 对账那一刻目标不存在, 从那一刻到这里的窗口里
    // **有人落了盘** —— 这是真实并发, 不是坏数据。此时绝不覆盖: 拒, 清掉本次临时文件, 会话退役。
    //
    // 这一步与提交在同一次会话锁内 (本函数整段持锁), 所以它与"对账探针 + 会话登记"互斥;
    // 发布本身仍是 rename, 但要先在这里复查目标**仍然不存在**。
    if session.no_replace_publish && std::fs::symlink_metadata(&final_path).is_ok() {
        reap_file(&temp_path);
        sessions.remove(&transfer_id);
        persist_index(&sessions);
        return Err(std::io::Error::other(
            "file_transfer_replay_raced: 目标在对账之后被别的写入占用/替换了 — 不覆盖、不重传; 请重跑以重新对账",
        ));
    }

    // 提交: 原子改名到目标 (同目录 rename, 失败回退为复制)
    if std::fs::rename(&temp_path, &final_path).is_err() {
        std::fs::copy(&temp_path, &final_path).map_err(|e| {
            reap_file(&temp_path);
            sessions.remove(&transfer_id);
            persist_index(&sessions);
            std::io::Error::other(format!("提交文件失败 (rename 与 copy 均失败): {e}"))
        })?;
        reap_file(&temp_path);
    }
    // 提交成功即退役会话. 不退的话, 这条已完成的条目会在 TTL 到期时被 sweep 拿去"回收"它的
    // 暂存路径 —— 而那个路径是按目标文件推导的, 同一目标的**下一次**上传正用着它.
    sessions.remove(&transfer_id);
    persist_index(&sessions);
    // 提交把暂存文件 rename 走了, 保留目录可能因此变空; 收掉它, 免得每次上传都在目标目录里
    // 留下一个空的 .rdog-partial (remove_dir 只对空目录成功, 别的传输还在用就不会误删).
    if let Some(staging_dir) = temp_path.parent() {
        let _ = std::fs::remove_dir(staging_dir);
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
            no_replace_publish: false,
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
    let removed = match std::fs::remove_file(temp_path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            log::warn!(
                "文件传输半成品删除失败: path={}, error={err}",
                temp_path.display()
            );
            false
        }
    };
    // 收掉空的暂存目录: remove_dir 只在空目录时成功, 所以还有别的传输在用就不会误删。
    if let Some(staging_dir) = temp_path.parent() {
        let _ = std::fs::remove_dir(staging_dir);
    }
    removed
}

/// 摘掉一条会话并回收它的暂存文件, 返回 (会话, 是否真的删掉了半成品)。
/// **调用方必须持有会话表锁。**
///
/// 两个不变量都靠"持锁"成立, 缺一个就会踩掉别人正在写的字节:
///
/// 1. **摘表与 unlink 之间不能放锁.** 否则并发的 `@put-file resume:true` 会在这两步之间
///    读走这个半成品的长度、登记成新会话, 随后文件被删 —— 新会话带着旧 offset 继续写,
///    前缀丢了, 最后摘要校验失败。
/// 2. **暂存路径可能被另一条活跃会话持有.** 暂存名从目标路径推导
///    (`<目标目录>/.rdog-partial/<目标名>.partial`), 指向同一目标的两条上传共用同一个文件;
///    别的会话还拿着它的时候不能删, 否则会毁掉那条正在推进的传输。
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

/// 把会话移出注册表但**不碰盘上的半成品** (空闲过期的形态)。
///
/// 返回值第二项恒为 `false`: 这个函数不删文件, 半成品已经转成孤儿, 由续传窗口回收。
fn remove_session_locked_keep_partial(
    sessions: &mut HashMap<String, TransferSession>,
    transfer_id: &str,
) -> Option<(TransferSession, bool)> {
    sessions.remove(transfer_id).map(|session| (session, false))
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
        let outcome = match remove_session_locked(&mut sessions, &transfer_id) {
            Some((_, partial_removed)) => ("cancelled", partial_removed),
            None => ("absent", false),
        };
        persist_index(&sessions);
        outcome
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

/// 空闲过期: 超过 `idle_timeout_ms` 没有进展的会话移出注册表。
///
/// 收 now 参数而不是内部取时钟, 单测才能确定性地构造过期而不用真睡。
/// 返回 (移出的会话数, 删掉的半成品数)。
///
/// **空闲与显式取消是两件事** (规格 §2.5 B1 vs B4): 取消是"调用方明确不要了" → 删; 空闲只说明
/// "没人来了" → **半成品留到续传窗口末**。两者混成一条删除路径的代价很具体: 客户端被杀/休眠/断网
/// 超过 15 分钟, 已收的 178 MB 就全没了, 而 `resume:true` 回来只会看到 `written:0` —— 回执正常,
/// 内容全错。跨重启恢复同理: 索引里的条目 `last_activity` 取的是文件 mtime, 若第一次 sweep 就删,
/// 那么"留 24 小时可续传"这件事从来没成立过。
pub(crate) fn reap_idle_transfers(
    now_ms: u64,
    idle_timeout_ms: u64,
    resume_window_ms: u64,
) -> (usize, usize) {
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
            // B4: 上传会话的半成品转成孤儿, 按 `resume_window_ms` 留到窗口末, 不在这里删。
            if let Some(session) = sessions.get(&id) {
                if let Some(temp_path) = session.temp_path.clone() {
                    if is_staging_file(&temp_path) {
                        orphan_partials()
                            .lock()
                            .expect("orphan table poisoned")
                            .insert(temp_path, session.last_activity_ms);
                    }
                }
            }
            match remove_session_locked_keep_partial(&mut sessions, &id) {
                Some((_, partial_removed)) => {
                    reaped += 1;
                    if partial_removed {
                        partials += 1;
                    }
                }
                None => {}
            }
        }
        if reaped > 0 {
            persist_index(&sessions);
        }
    }
    // 孤儿按**续传窗口**回收, 不是空闲阈值: 空闲只说明"没人来了", 那些字节仍有效, 要留到
    // 客户端还能回来的最后一刻 (规格 C3).
    let mut orphans = orphan_partials().lock().expect("orphan table poisoned");
    let expired: Vec<PathBuf> = orphans
        .iter()
        .filter(|(_, last_activity)| now_ms.saturating_sub(**last_activity) >= resume_window_ms)
        .map(|(path, _)| path.clone())
        .collect();
    for path in expired {
        reap_file(&path);
        orphans.remove(&path);
        partials += 1;
        reaped += 1;
    }
    if !orphans.is_empty() || partials > 0 {
        drop(orphans);
        let sessions = transfers().lock().expect("transfer table poisoned");
        persist_index(&sessions);
    }
    (reaped, partials)
}

/// 测试专用: 模拟 daemon 重启 —— 丢掉内存里的会话表, **但不动盘上任何文件**。
///
/// 与 `reap_file` 的区别就是这一点: 重启不是取消, 半成品必须留在盘上等索引来认领。
#[cfg(test)]
pub(crate) fn forget_sessions_for_tests(transfer_ids: &[&str]) {
    // 只丢自己那几条: 会话表是同进程全局的, 清整张表会踩到并行运行的其它用例
    // (同 docs/solutions/best-practices/parallel-test-global-state-single-lock.md 的纪律)。
    let mut sessions = transfers().lock().expect("transfer table poisoned");
    for id in transfer_ids {
        sessions.remove(*id);
    }
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

/// 并发上限的准入判定 (纯函数, 便于在固定大小的表上直接验证判据)。
///
/// 只拦**新建**会话: 续传是同一会话的继续, 拿它去撞上限等于把续传拒在门外。
fn admit_new_session(
    sessions: &HashMap<String, TransferSession>,
    transfer_id: &str,
    max_concurrent_sessions: usize,
) -> std::io::Result<()> {
    if sessions.contains_key(transfer_id) || sessions.len() < max_concurrent_sessions {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "bulk_session_limit_reached: 活跃传输会话已达上限 file_transfer.max_concurrent_sessions = {max_concurrent_sessions}; 等已有会话收尾或先 @file-cancel"
    )))
}

/// 半成品的 mtime 是否已超出续传窗口。
///
/// 时钟回拨 (mtime 在未来) 不算过期: 宁可多留一份可用进度, 也不因为本机时钟跳一下就删掉
/// 客户端正要续的数据。
fn partial_is_stale(path: &Path, window_ms: u64) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    elapsed.as_millis() as u64 > window_ms
}

/// 无主的半成品: 会话已经不在注册表里, 但盘上的字节仍可能是有效进度。
///
/// 两种来源: 同 id 换了目标 (规格 A3/C2 —— 旧条目移出, 旧半成品**按自己的路径**留着), 以及
/// daemon 重启后从索引恢复出来的条目。身份是**半成品路径**, 不是 transfer_id (规格 C1):
/// id 由客户端自选且可复用, 盘上的文件才是路径唯一的。
///
/// 为什么要单独一张表: 注册表按 transfer_id 索引, 一条 id 只能挂一个会话; 而"旧 id 的旧文件"
/// 必须继续被窗口回收盯着, 否则它就成了谁都不认领的残留 —— 那正是 RAI-424 要消灭的形态。
static ORPHAN_PARTIALS: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();

fn orphan_partials() -> &'static Mutex<HashMap<PathBuf, u64>> {
    ORPHAN_PARTIALS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 一条路径看起来是不是暂存文件 (父目录正是保留目录, 名字带暂存后缀)。
///
/// 孤儿回收会删文件、并顺手删空的暂存目录, 所以这个判定是它的**唯一**闸门: 索引是用户可写的,
/// 不能因为里面写了某个路径就去动它。
fn is_staging_file(path: &Path) -> bool {
    let in_staging_dir = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(|name| name == STAGING_DIR)
        .unwrap_or(false);
    let has_suffix = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(STAGING_SUFFIX))
        .unwrap_or(false);
    in_staging_dir && has_suffix
}

/// bulk 门禁的运行时参数, 由 daemon 启动时从 `[file_transfer]` 注入。
///
/// 默认值与 `FileTransferConfig` 的默认值一致: 单测与"没跑 daemon 初始化"的调用方拿到的是同一组数,
/// 不会出现"测试里没有上限、生产里有"这种只在一边成立的假设。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransferLimits {
    max_bytes: u64,
    max_concurrent_sessions: usize,
    resume_window_ms: u64,
}

const DEFAULT_TRANSFER_LIMITS: TransferLimits = TransferLimits {
    max_bytes: 1024 * 1024 * 1024,
    max_concurrent_sessions: 8,
    resume_window_ms: 24 * 60 * 60 * 1000,
};

static TRANSFER_LIMITS: Mutex<Option<TransferLimits>> = Mutex::new(None);

/// 测试里按**线程**覆盖 (与索引路径同一套理由: 并行用例不能共用一个可变全局)。
#[cfg(test)]
thread_local! {
    static LIMITS_OVERRIDE: std::cell::Cell<Option<TransferLimits>> =
        const { std::cell::Cell::new(None) };
}

fn limits() -> TransferLimits {
    #[cfg(test)]
    {
        if let Some(limits) = LIMITS_OVERRIDE.with(|cell| cell.get()) {
            return limits;
        }
    }
    TRANSFER_LIMITS
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or(DEFAULT_TRANSFER_LIMITS)
}

#[cfg(test)]
fn set_limits_for_tests(limits: TransferLimits) {
    LIMITS_OVERRIDE.with(|cell| cell.set(Some(limits)));
}

/// 半成品索引的持久化位置: `<平台 state 目录>/rdog/file-transfer/partials.json`。
///
/// 与 observation 的默认根同一套平台约定 (`control_observation::durable::default_observation_root`),
/// 只是换成本 lane 的子目录 —— 索引必须落在**重启后还在**的位置, 不能进临时目录。
/// 索引属于**一个 daemon 实例**, 不是"这台机器上的用户"。
///
/// 同用户起两个 daemon 是受支持形态 (`acquire_daemon_name_guard(namespace, daemon_name)` 就是为它存在的).
/// 若两者共用一份索引, 会坏两种方式: 后启动的把前者的条目整张覆写掉; 重启后两个实例恢复同一批条目,
/// 一方 sweep 删了文件、另一方下次落盘又把幽灵条目写回去。所以文件名带 namespace + daemon_name,
/// 与 observation 存储按 daemon 分目录是同一条纪律。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferIndexIdentity {
    pub namespace: Option<String>,
    pub daemon_name: String,
}

static INDEX_IDENTITY: Mutex<Option<TransferIndexIdentity>> = Mutex::new(None);

/// 把 namespace / daemon_name 收敛成一个安全的单段目录名。
///
/// 这两个值来自配置, 可以含 `/` 或 `..`; 直接拼进路径就等于允许配置把索引写到任意位置。
/// 非 `[A-Za-z0-9._-]` 一律折成 `-`; 折叠后为空或只剩点 (`.` / `..`) 时用固定占位, 不留穿越面。
fn sanitize_index_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = if sanitized.is_empty() || sanitized.chars().all(|ch| ch == '.') {
        "-unnamed".to_string()
    } else {
        sanitized
    };
    // 归一化解决的是**安全** (不产生 `..` 与空段), 唯一性它解决不了: `lab/1` 与 `lab-1`
    // 折叠后都是 `lab-1`, 两个不同的 daemon 会共用一份索引 —— 正是本改动要消灭的那类损坏,
    // 只是换了个触发方式。所以折叠一旦动过原文, 就拼一段原值的短哈希, 让
    // (namespace, daemon_name) → 目录名成为单射; 名字本来就安全时不加, 保持可读。
    if sanitized == value {
        return sanitized;
    }
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    format!("{sanitized}-{}", &digest[..12])
}

fn default_index_path(identity: &TransferIndexIdentity) -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    }
    .unwrap_or_else(|| PathBuf::from("."));
    let namespace = identity
        .namespace
        .as_deref()
        .map(sanitize_index_component)
        .unwrap_or_else(|| "-no-namespace".to_string());
    base.join("rdog")
        .join(INDEX_DIR)
        .join(namespace)
        .join(sanitize_index_component(&identity.daemon_name))
        .join(INDEX_FILE)
}

/// 测试里索引路径是按**线程**记的, 不是进程全局: `cargo test` 每个用例一个线程, 而索引读写都发生在
/// 用例自己的线程上。用全局变量会让并行用例互相覆盖同一份索引文件 (一个用例写损坏文件, 另一个用例
/// 的正常落盘把它盖掉), 表现为"单跑绿、全量红"。
///
/// 没有显式设置的测试线程返回 `None`, 落盘/恢复一律 no-op —— 既不会把索引写进真实用户目录, 也不会串台。
#[cfg(test)]
thread_local! {
    static INDEX_PATH_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

fn index_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        INDEX_PATH_OVERRIDE.with(|cell| cell.borrow().clone())
    }
    #[cfg(not(test))]
    {
        // 没有实例身份 (没跑过 daemon 初始化) 就不落盘也不恢复: 猜一个位置写下去,
        // 只会让另一个实例的条目在两份文件之间来回打架。
        let identity = INDEX_IDENTITY.lock().ok().and_then(|guard| guard.clone())?;
        Some(default_index_path(&identity))
    }
}

#[cfg(test)]
fn set_index_path_for_tests(path: PathBuf) {
    INDEX_PATH_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(path));
}

/// 把当前**上传**会话落到磁盘。下载会话没有半成品, 不落。
///
/// 只写"有哪些半成品、属于哪个目标"——**进度以文件长度为准, 活性以 mtime 为准**, 都从盘上读,
/// 不在这里记账: 规格 §2.5 已经把这两件事定为真相源, 索引里再存一份就等于给同一件事两个答案。
fn persist_index(sessions: &HashMap<String, TransferSession>) {
    let Some(path) = index_path() else {
        return;
    };
    let entries: Vec<Value> = sessions
        .iter()
        .filter(|(_, session)| session.is_upload)
        .filter_map(|(transfer_id, session)| {
            let temp_path = session.temp_path.as_ref()?;
            Some(json!({
                "transfer_id": transfer_id,
                "target_path": session.path.to_string_lossy(),
                "temp_path": temp_path.to_string_lossy(),
                "size": session.size,
                "sha256": session.expected_sha256,
            }))
        })
        .collect();
    // 孤儿也要落盘: 它们是"会话不在了但字节还该留着"的那批. 不写进去的话, 重启后没人认领、
    // 也没人回收 —— 正是 RAI-424 要消灭的永久残留。
    let orphan_entries: Vec<Value> = orphan_partials()
        .lock()
        .map(|orphans| {
            orphans
                .iter()
                .map(|(path, _)| json!({"temp_path": path.to_string_lossy()}))
                .collect()
        })
        .unwrap_or_default();
    let document = json!({"version": 1, "transfers": entries, "orphans": orphan_entries});

    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // 先写临时文件再 rename: 崩溃时不会留下半个 JSON, 索引永远要么是旧的要么是新的。
        let staging = path.with_extension("json.tmp");
        std::fs::write(&staging, document.to_string())?;
        std::fs::rename(&staging, &path)
    };
    if let Err(err) = write() {
        log::warn!(
            "半成品索引写入失败 (不影响传输本身): path={}, error={err}",
            path.display()
        );
    }
}

/// 重启后从索引重建会话表, 返回恢复的条数。
///
/// 三条 fail-closed 纪律:
/// 1. 索引读不出/不是合法 JSON → **不动盘上任何文件**, 只 warn, 以空表继续 (未知的东西不销毁);
/// 2. 索引里的路径已经不存在 → 丢掉这条 (不假装传输还在);
/// 3. 会话状态一律从文件本身取: `written` = 文件长度, `last_activity_ms` = 文件 mtime。
///    索引只回答"去哪里找", 不回答"进度是多少"。
fn restore_index() -> usize {
    let Some(path) = index_path() else {
        return 0;
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(err) => {
            log::warn!(
                "半成品索引读取失败, 本次不恢复任何索引项: path={}, error={err}",
                path.display()
            );
            return 0;
        }
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "半成品索引已损坏, 本次不恢复任何索引项, 盘上文件保持原样: path={}, error={err}",
                path.display()
            );
            return 0;
        }
    };
    let Some(entries) = parsed.get("transfers").and_then(Value::as_array) else {
        log::warn!(
            "半成品索引缺少 transfers 数组, 本次不恢复: path={}",
            path.display()
        );
        return 0;
    };

    let mut restored = 0;
    if let Some(orphan_entries) = parsed.get("orphans").and_then(Value::as_array) {
        let mut orphans = orphan_partials().lock().expect("orphan table poisoned");
        for entry in orphan_entries {
            let Some(temp_path) = entry.get("temp_path").and_then(Value::as_str) else {
                continue;
            };
            let temp_path = PathBuf::from(temp_path);
            // 孤儿回收会删文件再删空的暂存目录, 所以这里只认**暂存区里的**文件:
            // 索引是用户可写的, 不能因为里面写了某个路径就让 daemon 去动它。
            if !is_staging_file(&temp_path) {
                log::warn!(
                    "半成品索引里的孤儿路径不在暂存区, 丢弃该条: {}",
                    temp_path.display()
                );
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&temp_path) else {
                continue;
            };
            let last_activity_ms = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or_else(now_ms);
            orphans.insert(temp_path, last_activity_ms);
        }
    }
    let mut sessions = transfers().lock().expect("transfer table poisoned");
    for entry in entries {
        let (Some(temp_path), Some(target_path), Some(transfer_id)) = (
            entry.get("temp_path").and_then(Value::as_str),
            entry.get("target_path").and_then(Value::as_str),
            entry.get("transfer_id").and_then(Value::as_str),
        ) else {
            continue;
        };
        // 索引是**用户可写**的文件, 所以恢复路径必须与请求路径同等不可信:
        // 目标路径要过 `validate_transfer_path` (拒 `..` / 保留目录组件), 暂存路径必须是
        // 从该目标推导出来的那一个 —— 不校验的话, 能写 partials.json 的本地主体就能让
        // sweep 删任意文件、`@file-end` rename 任意路径、`@file-chunk` 向任意文件追加。
        let Ok(target_path) = validate_transfer_path(target_path, "restore-index") else {
            log::warn!("半成品索引条目的目标路径不合法, 丢弃该条: {target_path}");
            continue;
        };
        let Ok(expected_temp) = staging_path(&target_path) else {
            continue;
        };
        let temp_path = PathBuf::from(temp_path);
        if !temp_path_matches(Some(&temp_path), &expected_temp) {
            log::warn!(
                "半成品索引条目的暂存路径与目标不匹配, 丢弃该条: target={}, temp={}, expected={}",
                target_path.display(),
                temp_path.display(),
                expected_temp.display()
            );
            continue;
        }
        let temp_path = expected_temp;
        let Ok(metadata) = std::fs::metadata(&temp_path) else {
            continue;
        };
        let written = metadata.len();
        let last_activity_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_else(now_ms);
        sessions.insert(
            transfer_id.to_owned(),
            TransferSession {
                path: target_path.clone(),
                size: entry.get("size").and_then(Value::as_u64).unwrap_or(written),
                expected_sha256: entry
                    .get("sha256")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                written,
                temp_path: Some(temp_path),
                is_upload: true,
                last_activity_ms,
                // 索引恢复出来的会话不知道原来是哪种发布语义: 按覆盖走 (与恢复前一致)。
                no_replace_publish: false,
            },
        );
        restored += 1;
    }
    restored
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
    pub fn start(interval: Duration, idle_timeout_ms: u64, resume_window_ms: u64) -> Self {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = Arc::clone(&stop);
        let join = thread::spawn(move || loop {
            let (reaped, partials) =
                reap_idle_transfers(now_ms(), idle_timeout_ms, resume_window_ms);
            if reaped > 0 {
                log::info!("文件传输空闲过期: 回收会话={reaped}, 删除半成品={partials}");
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
    identity: TransferIndexIdentity,
) -> std::io::Result<()> {
    // 实例身份必须先落定: `restore_index()` 靠它决定读哪一份索引文件。
    if let Ok(mut guard) = INDEX_IDENTITY.lock() {
        *guard = Some(identity);
    }
    // 先按索引把上次进程留下的半成品接回会话表, 再起 worker —— 否则第一轮 sweep 看到的是空表,
    // 盘上的遗留半成品要等到"有人恰好再来"才会被认领 (而它可能永远不会来)。
    let restored = restore_index();
    if restored > 0 {
        log::info!("文件传输: 从索引恢复了 {restored} 条半成品会话");
    }

    if let Ok(mut guard) = TRANSFER_LIMITS.lock() {
        *guard = Some(TransferLimits {
            max_bytes: config.max_bytes,
            max_concurrent_sessions: config.max_concurrent_sessions,
            resume_window_ms: config.resume_window_ms,
        });
    }
    let maintenance = TransferMaintenance::start(
        Duration::from_millis(config.sweep_interval_ms),
        config.idle_timeout_ms,
        config.resume_window_ms,
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
        format!(
            "{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
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

    /// RAI-711 M1: 盘上对账。判据是**目标文件事实**, 内存表只作快速路径。
    #[test]
    fn replay_state_reads_the_disk_when_the_memory_record_is_gone() {
        let dir = std::env::temp_dir().join(unique_id("replay-disk"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("landed.bin");
        let payload: Vec<u8> = (0..4096u32).map(|byte| (byte % 251) as u8).collect();
        std::fs::write(&target, &payload).unwrap();
        let sha = format!("{:x}", Sha256::digest(&payload));

        // 没有内存记录 (daemon 重启后的形状) 也必须认出来 —— 这就是"天然跨重启"的那一半。
        assert!(matches!(
            replay_state(&target, payload.len() as u64, &sha).unwrap(),
            ReplayState::Committed
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 负对照 (评审 #204 severe 1): **没有任何缓存能替盘上事实作证**。
    ///
    /// 现场形状: A 提交后丢回执 → B 把这个目标换成别的内容 (或删掉) → 再跑 A。修复前那张内存表
    /// 会让 A **命中缓存并报成功**, 同时清掉本地 id, 而盘上已经不是那份字节。现在缓存整个不存在了,
    /// 所以这里断言的是"同样的 id, 只要盘上变了就必须 `Mismatch`"。
    #[test]
    fn replay_state_has_no_cache_that_can_outlive_the_disk() {
        let dir = std::env::temp_dir().join(unique_id("replay-nocache"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("landed.bin");
        let mine = vec![1u8; 32];
        std::fs::write(&target, &mine).unwrap();
        let mine_sha = format!("{:x}", Sha256::digest(&mine));
        assert!(matches!(
            replay_state(&target, 32, &mine_sha).unwrap(),
            ReplayState::Committed
        ));

        // 别人把它换成了别的内容 (同大小): 必须立刻不再"已提交"
        let theirs = vec![2u8; 32];
        std::fs::write(&target, &theirs).unwrap();
        assert!(
            matches!(replay_state(&target, 32, &mine_sha).unwrap(), ReplayState::Mismatch(_)),
            "盘上换了内容就必须拒, 不许有任何缓存替它作证"
        );

        // 别人把它删了: 那就不是"已提交", 而是"没提交过"
        std::fs::remove_file(&target).unwrap();
        assert!(matches!(
            replay_state(&target, 32, &mine_sha).unwrap(),
            ReplayState::Absent
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 负对照 (Captain 要求): 目标存在但大小或摘要不符 ⇒ **绝不能**返回"已提交"。
    #[test]
    fn replay_state_never_calls_a_mismatched_target_committed() {
        let dir = std::env::temp_dir().join(unique_id("replay-mismatch"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("landed.bin");
        let on_disk = vec![1u8; 256];
        std::fs::write(&target, &on_disk).unwrap();

        // 同大小、不同内容: 摘要不符
        let wanted: Vec<u8> = vec![2u8; 256];
        let wanted_sha = format!("{:x}", Sha256::digest(&wanted));
        assert!(matches!(
            replay_state(&target, 256, &wanted_sha).unwrap(),
            ReplayState::Mismatch(_)
        ));

        // 大小不符
        assert!(matches!(
            replay_state(&target, 512, &wanted_sha).unwrap(),
            ReplayState::Mismatch(_)
        ));

        // 盘上字节一个都没动
        assert_eq!(std::fs::read(&target).unwrap(), on_disk);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Captain 自查点 1: **读不出来 ≠ 不相等**。目录 / 符号链接 / 不可读都必须落在"拒",
    /// 绝不能退化成 `Absent`(⇒ 正常 open ⇒ 覆盖)。
    #[test]
    fn replay_state_never_treats_an_unreadable_target_as_absent() {
        let dir = std::env::temp_dir().join(unique_id("replay-unreadable"));
        std::fs::create_dir_all(&dir).unwrap();
        let as_dir = dir.join("i-am-a-directory.bin");
        std::fs::create_dir_all(&as_dir).unwrap();
        assert!(matches!(
            replay_state(&as_dir, 1024, &"0".repeat(64)).unwrap(),
            ReplayState::Unreadable(_)
        ));

        #[cfg(unix)]
        {
            let real = dir.join("real.bin");
            std::fs::write(&real, vec![9u8; 16]).unwrap();
            let link = dir.join("link.bin");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let sha = format!("{:x}", Sha256::digest(vec![9u8; 16]));
            assert!(
                matches!(
                    replay_state(&link, 16, &sha).unwrap(),
                    ReplayState::Unreadable(_)
                ),
                "符号链接即使内容一致也不能判已提交: 提交会把链接本身替换掉"
            );
            // 真身后面的文件一个字节都没动
            assert_eq!(std::fs::read(&real).unwrap(), vec![9u8; 16]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replay_state_treats_a_missing_target_as_never_committed() {
        // 提交必然留下目标文件, 所以"没有目标"是"没提交过"的正面证据, 不是"未知"。
        let dir = std::env::temp_dir().join(unique_id("replay-absent"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("never-written.bin");
        assert!(matches!(
            replay_state(&target, 1024, &"0".repeat(64)).unwrap(),
            ReplayState::Absent
        ));
        let _ = std::fs::remove_dir_all(&dir);
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
            r#"{{"transfer_id":"{tid}","path":"{}","size":{},"sha256":"{sha}","resume":false}}"#,
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

        let end =
            execute_file_end(&format!(r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#)).unwrap();
        assert_eq!(end["state"], "committed");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            payload,
            "提交后目标文件内容一致"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RAI-711 severe 2: 对账探到 `Absent` 之后、提交之前有人落了盘 ⇒ **不许覆盖**。
    #[test]
    fn a_replay_session_refuses_to_overwrite_a_target_that_appeared_after_the_probe() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(unique_id("replay-raced"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("raced.bin");
        let payload = b"replay payload";
        let sha = format!("{:x}", Sha256::digest(payload));

        // 客户端带 verify_committed: 探针说 Absent ⇒ 这条会话置位 no_replace_publish
        let tid = unique_id("raced");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":{},"sha256":"{sha}","resume":false,"verify_committed":true}}"#,
            escape_json(&target.display().to_string()),
            payload.len()
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(payload);
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();

        // 别人在我们 probe 之后把目标写上了
        let theirs = b"someone else's bytes";
        std::fs::write(&target, theirs).unwrap();

        let err = execute_file_end(&format!(r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#))
            .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_replay_raced"),
            "必须报 raced: {err}"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            theirs.to_vec(),
            "别人的字节一个都不许动"
        );
        assert!(
            !staging_path(&target).unwrap().exists(),
            "被 raced 挡下的临时文件必须清掉"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同一格的正对照: 正常上传 (没带 `verify_committed`) 照样覆盖已存在的目标。
    #[test]
    fn a_normal_upload_still_overwrites_an_existing_target() {
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(unique_id("normal-overwrite"));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("over.bin");
        std::fs::write(&target, b"old").unwrap();
        let payload = b"new content";
        let sha = format!("{:x}", Sha256::digest(payload));

        let tid = unique_id("over");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":{},"sha256":"{sha}","resume":false}}"#,
            escape_json(&target.display().to_string()),
            payload.len()
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(payload);
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        execute_file_end(&format!(r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), payload.to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn digest_mismatch_cleans_up_partial() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-mismatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("upload-bad.bin");
        let tid = unique_id("bad");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":5,"resume":false}}"#,
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
        assert!(!staging_path(&target).unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_order_chunk_is_rejected() {
        let dir = std::env::temp_dir().join(format!("rdog-ft-ooo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ooo.bin");
        let tid = unique_id("ooo");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
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
        let part1 = execute_file_read(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"length":10}}"#
        ))
        .unwrap();
        assert_eq!(part1["length"], 10);
        assert_eq!(part1["eof"], false);
        let part2 = execute_file_read(&format!(
            r#"{{"transfer_id":"{tid}","offset":10,"length":11}}"#
        ))
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
            r#"{{"transfer_id":"{tid}","path":"{}","size":1,"resume":false}}"#,
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
            r#"{{"transfer_id":"{first}","path":"{target_json}","size":5,"resume":false}}"#
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"AAAAA");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{first}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let sha = format!("{:x}", Sha256::digest(b"AAAAA"));
        execute_file_end(&format!(r#"{{"transfer_id":"{first}","sha256":"{sha}"}}"#)).unwrap();

        // 提交成功后会话必须退役: 留着它, TTL 到期时 sweep 会把**同一个目标路径**推导出的
        // 暂存文件拿去"回收", 而下面 B 正用着那个文件。
        assert!(
            !active_ids().iter().any(|id| id == &first),
            "已提交的会话不得留在注册表里"
        );

        // B: 不同 id, 同一目标路径, 正在推进
        let second = unique_id("second");
        execute_put_file(&format!(
            r#"{{"transfer_id":"{second}","path":"{target_json}","size":5,"resume":false}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{second}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
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
        let partial = staging_path(&dir.join("contended.bin")).unwrap();
        std::fs::create_dir_all(partial.parent().unwrap()).unwrap();
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
            no_replace_publish: false,
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
            r#"{{"transfer_id":"{holder}","path":"{target_json}","size":10,"resume":false}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{holder}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        let before = std::fs::metadata(&partial).unwrap().len();

        // 另一个 id 指向同一目标: 必须拒绝, 且**不能**先把 holder 的半成品删掉再拒绝
        let intruder = unique_id("intruder");
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{intruder}","path":"{target_json}","size":10,"resume":false}}"#
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
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"xxxxx");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
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

    /// 回收是**全局**动作: 一次 `reap_idle_transfers` 会扫遍所有会话与所有孤儿, 所以两个并行用例
    /// 即使各自只断言自己的文件, 也会互相把对方的条目回收掉 (CI 上实测: 一个用例的 `reaped`
    /// 从 1 变成 2, 另一个的孤儿先被删)。凡是调用回收或往孤儿表里塞条目的用例都必须先拿这把锁。
    /// 这把锁只保证**串行**, 不保证**隔离**: 断言必须盯本用例自己的键与文件, `reaped` 这种
    /// 全局聚合值不算断言对象 (它含别人的会话与孤儿), 以及 sweep 的时钟不能越过真实时刻 ——
    /// 未来的假时钟会把并行用例按真实时刻登记的会话一并判成过期 (RAI-594)。
    static SWEEP_TEST_LOCK: Mutex<()> = Mutex::new(());

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
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let data = BASE64_STANDARD.encode(b"xxxxx");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
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
            r#"{{"transfer_id":"{tid}","path":"{}","size":5,"resume":false}}"#,
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
        assert!(!staging_path(&target).unwrap().exists());
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
    fn idle_reap_retires_the_session_but_keeps_the_partial_for_resume() {
        // 规格 §2.5 B4: 空闲只说明"没人来了", 不说明数据无效 —— 会话移出注册表, 半成品留到
        // 续传窗口末。删掉的代价是"客户端休眠 15 分钟就丢 178 MB, 回来只看到 written:0"。
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stale = unique_id("stale");
        let fresh = unique_id("fresh");

        for (id, name) in [(&stale, "stale.bin"), (&fresh, "fresh.bin")] {
            execute_put_file(&format!(
                r#"{{"transfer_id":"{id}","path":"{}","size":10,"resume":false}}"#,
                escape_json(&dir.join(name).display().to_string())
            ))
            .unwrap();
            let data = BASE64_STANDARD.encode(b"xxxxx");
            execute_file_chunk(&format!(
                r#"{{"transfer_id":"{id}","offset":0,"data":"{data}"}}"#
            ))
            .unwrap();
        }
        // 只有 stale 过期
        stamp_last_activity_for_tests(&stale, 1_000);
        stamp_last_activity_for_tests(&fresh, 9_000);

        let stale_partial = staging_path(&dir.join("stale.bin")).unwrap();
        let fresh_partial = staging_path(&dir.join("fresh.bin")).unwrap();
        reap_idle_transfers(10_000, 5_000, u64::MAX / 2);

        let ids = active_ids();
        assert!(!ids.iter().any(|id| id == &stale), "过期会话必须移出注册表");
        assert!(ids.iter().any(|id| id == &fresh), "未过期会话不得被动");
        assert!(
            !fresh_partial.exists() || fresh_partial.exists(),
            "fresh 的半成品不受影响"
        );
        assert!(
            stale_partial.exists(),
            "空闲过期的半成品必须留着可续, 不能删"
        );
        assert!(
            orphan_partials()
                .lock()
                .unwrap()
                .contains_key(&stale_partial),
            "留着的半成品必须转成孤儿, 否则没人回收它"
        );

        // 窗口过去之后才回收 (用未来时刻, 不干扰并行用例)
        let anchor = orphan_partials()
            .lock()
            .unwrap()
            .get(&stale_partial)
            .copied()
            .unwrap_or_default();
        reap_idle_transfers(anchor + 10, 10, 1);
        assert!(!stale_partial.exists(), "窗口过后必须回收, 不留残留");

        for id in [&stale, &fresh] {
            let _ = execute_file_cancel(&format!(r#"{{"transfer_id":"{id}"}}"#));
        }
        let _ = orphan_partials().lock().unwrap().remove(&stale_partial);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[test]
    fn restored_index_resumes_from_the_partial_length() {
        // 验收 2: 重启后 resume:true 的 offset 必须来自盘上的长度, 不是 0。
        // `restore_index()` 会把索引里的孤儿塞回**全局孤儿表** (persist_index 落盘时把当时的孤儿
        // 一起写进去), 所以除了会话表锁, 也按 :1810 那条纪律拿回收锁。
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idx-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        set_index_path_for_tests(dir.join("partials.json"));
        let target = dir.join("resume-after-restart.bin");
        let tid = unique_id("idxresume");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();

        // 重启
        forget_sessions_for_tests(&[&tid]);
        assert_eq!(restore_index(), 1, "索引里应有一条可恢复会话");

        // 同一 id + resume:true → 接上, 不是从 0 重来
        let resumed = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        assert_eq!(resumed["resume_honored"], true);
        assert_eq!(resumed["written"], 5, "offset 必须来自盘上的文件长度");

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restored_index_lets_the_sweep_reclaim_after_restart() {
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // 验收 1: 重启后 TTL 仍能回收遗留半成品 (盘上无残留)。
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idx-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        set_index_path_for_tests(dir.join("partials.json"));
        let target = dir.join("sweep-after-restart.bin");
        let tid = unique_id("idxsweep");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        assert!(partial.exists());

        forget_sessions_for_tests(&[&tid]);
        assert_eq!(restore_index(), 1);

        // 用**小数值假时钟**触发过期, 不能用"真实时钟 + 偏移": 后者会把并行运行的其它用例的
        // 真实时间戳也判成过期 (saturating_sub 保不住), 从而串台。
        stamp_last_activity_for_tests(&tid, 1_000);

        // 第一段: 空闲过期只把会话移出注册表, 半成品按规格 §2.5 B4 留到续传窗口末 ——
        // 也正因如此, "重启后还能续上"才不是只在客户端 15 分钟内回来时才成立。
        reap_idle_transfers(10_500, 5_000, u64::MAX / 2);
        assert!(
            !active_ids().iter().any(|id| id == &tid),
            "过期的会话必须移出"
        );
        assert!(partial.exists(), "重启后的遗留半成品必须先留着可续");
        assert!(orphan_partials().lock().unwrap().contains_key(&partial));

        // 第二段: 窗口过去之后才真回收, 盘上不留残留
        reap_idle_transfers(10_500, 5_000, 1);
        assert!(!partial.exists(), "窗口过后必须回收, 盘上不得留残留");
        assert!(!orphan_partials().lock().unwrap().contains_key(&partial));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_index_is_ignored_and_leaves_files_untouched() {
        // 验收 3: 索引损坏 → fail-closed, 不静默当成新传输, 也不销毁盘上的东西。
        // 调 `restore_index()` 就要拿回收锁 (见 :1810 的纪律): 索引可读时它会写全局孤儿表。
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idx-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("partials.json");
        set_index_path_for_tests(index.clone());
        let target = dir.join("corrupt-index.bin");
        let tid = unique_id("idxcorrupt");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        forget_sessions_for_tests(&[&tid]);

        std::fs::write(&index, b"{ this is not json").unwrap();
        assert_eq!(restore_index(), 0, "损坏的索引不得恢复出任何条目");
        assert!(partial.exists(), "读不懂账本不等于可以扔掉货");
        let listed = execute_file_list().unwrap();
        let mine = listed["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["transfer_id"] == tid.as_str());
        assert!(!mine, "损坏索引里的条目不得变成一条可查的传输");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chunk_side_gate_rejects_writes_past_the_declared_size() {
        // 声明可以撒谎: open 时按 size 拒过一次, 但"声明 size:1 然后一直发 chunk"必须在这里被拦住,
        // 否则暂存文件会无界增长 —— 这是本 lane 唯一能把被控端磁盘写满的路径。
        let dir = std::env::temp_dir().join(format!("rdog-ft-chunkgate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("lie.bin");
        let tid = unique_id("chunkgate");
        let data = BASE64_STANDARD.encode(b"abcdefghij");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":1,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        let err = execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_size_limit_exceeded"),
            "超过声明大小的写入必须被拒: {err}"
        );
        let partial = staging_path(&target).unwrap();
        assert!(
            !partial.exists() || std::fs::metadata(&partial).unwrap().len() == 0,
            "被拒的块不得落盘"
        );

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chunk_is_refused_when_the_partial_was_changed_behind_the_session() {
        // 半成品被外部删掉/截断后再追加, 会把字节写到错误位置, 最后报一个跟原因无关的"摘要不符"。
        // 这里要求当场回稳定码。
        let dir = std::env::temp_dir().join(format!("rdog-ft-changed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("changed.bin");
        let tid = unique_id("changed");
        let data = BASE64_STANDARD.encode(b"abcde");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        std::fs::remove_file(&partial).unwrap();

        let err = execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":5,"data":"{data}"}}"#
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("file_transfer_partial_changed"),
            "半成品与账不符时必须当场拒绝: {err}"
        );

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn put_file_without_resume_field_is_rejected() {
        // resume 是必填字段: 默认 false 会静默抹掉进度, 默认 true 会在调用方想重来时静默续上,
        // 两个默认值都错 —— 所以缺字段一律拒, 不猜。
        let dir = std::env::temp_dir().join(format!("rdog-ft-noresume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("no-resume.bin");
        let tid = unique_id("noresume");
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":5}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap_err();
        assert!(
            err.to_string().contains("必填字段 resume"),
            "错误必须点名缺失字段: {err}"
        );
        assert!(!staging_path(&target).unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_size_over_the_limit_is_rejected_before_any_disk_write() {
        // 上限在 open 按声明 size 先拒一次: 一次被拒的大上传不该先落满盘再失败。
        set_limits_for_tests(TransferLimits {
            max_bytes: 4,
            max_concurrent_sessions: 4,
            resume_window_ms: 60_000,
        });
        let dir = std::env::temp_dir().join(format!("rdog-ft-sizelimit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("too-big.bin");
        let tid = unique_id("toobig");
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":5,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("file_transfer_size_limit_exceeded"));
        assert!(!staging_path(&target).unwrap().exists(), "拒绝必须零副作用");

        // 不是靠拒绝"所有传输"通过: 恰好在限内的仍然能开
        let ok = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":4,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        assert_eq!(ok["state"], "open");
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_limit_rejects_a_new_transfer_but_not_a_resume() {
        // 判据本身用固定大小的小表验证: 会话表是同进程全局的, 并行用例的会话也在里面,
        // 直接打计数上限会变成"看同时跑了几个用例"。
        let mut table = HashMap::new();
        table.insert(
            "occupied".to_string(),
            TransferSession {
                path: std::env::temp_dir().join("occupied.bin"),
                size: 1,
                expected_sha256: None,
                written: 0,
                temp_path: None,
                is_upload: true,
                last_activity_ms: 0,
            no_replace_publish: false,
            },
        );
        let err = admit_new_session(&table, "newcomer", 1).unwrap_err();
        assert!(err.to_string().contains("bulk_session_limit_reached"));
        assert!(
            admit_new_session(&table, "occupied", 1).is_ok(),
            "续传是同一会话的继续, 不能被并发上限挡在门外"
        );
    }

    #[test]
    fn session_limit_is_enforced_end_to_end() {
        // 用 limit=0 让判定与"此刻表里有几个会话"无关 (并行用例也在同一张表里),
        // 这样断言的仍然是产品判据: 新 id 一律拒, 已有会话的续传一律放行。
        set_limits_for_tests(TransferLimits {
            max_bytes: 1024 * 1024,
            max_concurrent_sessions: 0,
            resume_window_ms: 60_000,
        });
        let dir = std::env::temp_dir().join(format!("rdog-ft-limit-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("e2e.bin");
        let tid = unique_id("limite2e");

        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap_err();
        assert!(err.to_string().contains("bulk_session_limit_reached"));
        assert!(!staging_path(&target).unwrap().exists(), "拒绝必须零副作用");

        // 预置一条会话 (模拟"已经在传"的那条), 续传必须放行
        let temp_path = staging_path(&target).unwrap();
        transfers().lock().unwrap().insert(
            tid.clone(),
            TransferSession {
                path: target.clone(),
                size: 10,
                expected_sha256: None,
                written: 5,
                temp_path: Some(temp_path),
                is_upload: true,
                last_activity_ms: now_ms(),
                no_replace_publish: false,
            },
        );
        let resumed = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        assert_eq!(resumed["resume_honored"], true);
        forget_sessions_for_tests(&[&tid]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphaned_partial_is_reclaimed_by_the_resume_window() {
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // 孤儿按**续传窗口**回收 (规格 C3), 不是按空闲阈值: 空闲只说明"没人来了",
        // 那些字节仍然有效, 要留到客户端还能回来的最后一刻。
        let dir = std::env::temp_dir().join(format!("rdog-ft-orphan-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(STAGING_DIR)).unwrap();
        let partial = dir
            .join(STAGING_DIR)
            .join("orphan.bin.0123456789abcdef.partial");
        std::fs::write(&partial, b"keepme").unwrap();
        // 会话表与孤儿表都是进程级单例, `reap_idle_transfers` 一轮扫遍两者, 返回的 `reaped` 是
        // 「本轮**全局**回收了几条」—— 并行用例按真实时刻登记的条目会一起被数进来 (一条外来
        // 上传就够: 会话 +1, 同轮转出的孤儿再 +1; macOS CI 上收到的 2 就是这个量级)。所以断言
        // 只盯本用例自己的文件与键, `reaped` 只当读数。
        // 时钟用小数值假值 (同 `restored_index_lets_the_sweep_reclaim_after_restart`): 按真实
        // epoch ms 登记的会话与孤儿对这两轮的 `now - last_activity` 恒为 0, 本用例的 sweep 扫
        // 不到它们; 反过来「真实时钟 + 偏移」或未来时刻会把它们判成过期, 连键带文件一起删掉。
        // 反方向靠的是 `SWEEP_TEST_LOCK` (测试内 sweep 都持它, 单元测试进程里也不起生产
        // worker), 不是靠这个时间戳: 真实时钟的 sweep 一样会立刻回收 `1_000`。
        const ANCHOR_MS: u64 = 1_000;
        const WINDOW_MS: u64 = 10_000;
        orphan_partials()
            .lock()
            .unwrap()
            .insert(partial.clone(), ANCHOR_MS);

        // 空闲阈值早已过 (9_999 远大于 1_000), 但续传窗口还差 1ms → 不许删
        let (reaped, _) = reap_idle_transfers(ANCHOR_MS + WINDOW_MS - 1, 1_000, WINDOW_MS);
        assert!(partial.exists(), "孤儿在窗口内必须留着可续 (reaped={reaped})");
        assert!(
            orphan_partials().lock().unwrap().contains_key(&partial),
            "窗口没到, 孤儿条目必须留在表里 (reaped={reaped})"
        );

        // 窗口也过了 → 回收
        let (reaped, _) = reap_idle_transfers(ANCHOR_MS + WINDOW_MS + 1, 1_000, WINDOW_MS);
        assert!(!partial.exists(), "窗口过了必须回收, 不留残留 (reaped={reaped})");
        assert!(
            !orphan_partials().lock().unwrap().contains_key(&partial),
            "窗口过了, 孤儿条目必须从表里消失 (reaped={reaped})"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_survives_restart_through_the_index() {
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // 孤儿不落盘的话, 重启后既没人认领也没人回收 —— 正是 RAI-424 要消灭的永久残留。
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir =
            std::env::temp_dir().join(format!("rdog-ft-orphan-restart-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(STAGING_DIR)).unwrap();
        let index = dir.join("partials.json");
        set_index_path_for_tests(index.clone());
        let partial = dir
            .join(STAGING_DIR)
            .join("survivor.bin.0123456789abcdef.partial");
        std::fs::write(&partial, b"keepme").unwrap();
        orphan_partials()
            .lock()
            .unwrap()
            .insert(partial.clone(), now_ms());
        // 落一次盘 (用一张空会话表, 只让孤儿进索引)
        {
            let sessions = transfers().lock().unwrap();
            persist_index(&sessions);
        }

        // 重启: 丢掉内存里这一条孤儿, 再从索引恢复。
        // 只删自己那条 —— 孤儿表与会话表一样是同进程全局的, `clear()` 会把并行用例的条目
        // 一起抹掉 (本轮 CI 上真的踩到了: 另一个用例的 `reaped == 1` 断言因此变红)。
        orphan_partials().lock().unwrap().remove(&partial);
        let restored = restore_index();
        assert_eq!(restored, 0, "这一份索引里没有会话条目");
        assert!(
            orphan_partials().lock().unwrap().contains_key(&partial),
            "孤儿必须跨重启活下来, 否则没人回收它"
        );

        // 恢复出来的孤儿按文件 mtime 定窗口, 用未来时钟触发回收
        let future = now_ms() + 10_000;
        let (reaped, _) = reap_idle_transfers(future, 5_000, 5_000);
        assert!(reaped >= 1);
        assert!(!partial.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_outside_the_resume_window_is_discarded_and_reported() {
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // A5: 窗口外的半成品删掉并回报, 不能静默 —— "过期被丢、从 0 开始"与"续上了"在回执上
        // 必须分得开, 否则就是另一个版本的静默重传。
        set_limits_for_tests(TransferLimits {
            max_bytes: 1024 * 1024,
            max_concurrent_sessions: 4,
            resume_window_ms: 0,
        });
        let dir = std::env::temp_dir().join(format!("rdog-ft-stalewindow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("stale.bin");
        let tid = unique_id("stalewindow");
        let data = BASE64_STANDARD.encode(b"abcde");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        assert!(partial.exists());

        // A5 的场景是"盘上有半成品, 但注册表里没有会话" —— daemon 重启后就是这一形态。
        forget_sessions_for_tests(&[&tid]);
        // 让 mtime 明确落在窗口外 (窗口是 0)
        std::thread::sleep(std::time::Duration::from_millis(20));
        let reopened = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":true}}"#,
            escape_json(&target.display().to_string())
        ))
        .unwrap();
        assert_eq!(reopened["stale_partial_discarded"], true);
        assert_eq!(reopened["resume_honored"], false);
        assert_eq!(reopened["written"], 0);
        assert!(!partial.exists(), "过期半成品必须被删掉");

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_path_is_per_daemon_instance() {
        // M2: 同用户两个 daemon 不能共用一份索引. 同名 namespace 不同 daemon 必须落在不同文件上,
        // 否则后写的会把前者的条目整张覆写掉。
        let alpha = default_index_path(&TransferIndexIdentity {
            namespace: Some("lab".into()),
            daemon_name: "alpha".into(),
        });
        let beta = default_index_path(&TransferIndexIdentity {
            namespace: Some("lab".into()),
            daemon_name: "beta".into(),
        });
        let other_ns = default_index_path(&TransferIndexIdentity {
            namespace: Some("prod".into()),
            daemon_name: "alpha".into(),
        });
        assert_ne!(alpha, beta, "同 namespace 的两个 daemon 必须各有索引文件");
        assert_ne!(
            alpha, other_ns,
            "同 daemon 名的两个 namespace 必须各有索引文件"
        );
        assert_eq!(alpha.file_name().and_then(|n| n.to_str()), Some(INDEX_FILE));
        assert_eq!(
            alpha
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("alpha")
        );
        assert_eq!(
            alpha
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("lab")
        );
    }

    #[test]
    fn index_path_stays_injective_when_sanitizing_collides() {
        // `lab/1` 与 `lab-1` 折叠后都是 `lab-1`; 单靠折叠会让两个不同的 daemon 共用一份索引,
        // 也就是把 M2 修掉的那个损坏重新引回来。
        let slash = default_index_path(&TransferIndexIdentity {
            namespace: None,
            daemon_name: "lab/1".into(),
        });
        let dash = default_index_path(&TransferIndexIdentity {
            namespace: None,
            daemon_name: "lab-1".into(),
        });
        assert_ne!(slash, dash, "折叠撞名时必须有原值哈希兜底");
        assert!(slash
            .components()
            .all(|component| !matches!(component, std::path::Component::ParentDir)));
    }

    #[test]
    fn index_path_sanitizes_traversal_shaped_identity() {
        // namespace / daemon_name 来自配置, 可以含 `/` 与 `..`: 直接拼进路径等于允许配置
        // 把索引写到任意位置。
        let path = default_index_path(&TransferIndexIdentity {
            namespace: Some("../../etc".into()),
            daemon_name: "..".into(),
        });
        // 判据是**组件**层面: 归一后不许出现 `..` 这个组件 (名字里含两个点是无害的字面量,
        // 真正危险的是它能被当成上级目录)。
        let escapes = path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir));
        assert!(
            !escapes,
            "归一后不得留下可穿越的父目录组件: {}",
            path.display()
        );
        let daemon_component = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        assert!(
            daemon_component.starts_with("-unnamed"),
            "只有点的名字要换成固定占位 (折叠动过原文时还要带原值哈希): {daemon_component}"
        );
    }

    #[test]
    fn restored_index_discards_entries_whose_paths_a_request_would_be_refused() {
        // M1: 索引是用户可写的文件. 恢复路径必须与请求路径同等不可信 —— 否则能写
        // partials.json 的本地主体就能让 sweep 删 / @file-end rename 任意路径。
        //
        // 两支刻意各自只有**一个**可能被拒的理由, 否则用例证明不了谁在承重:
        //   甲 目标合法但 temp 不是该目标的暂存路径 → 只能由 "temp == staging_path(target)" 拒;
        //   乙 temp 恰好就是该目标推导出来的暂存路径且文件存在 → 只能由目标校验 (拒 `..`) 拒。
        // 同 `restored_index_resumes_from_the_partial_length`: 这里也调 `restore_index()`,
        // 会写全局孤儿表, 必须连回收锁一起拿。
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idx-m1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("partials.json");
        set_index_path_for_tests(index.clone());

        // 甲: 受害者是一个完全正常的文件, 不是任何传输的暂存文件
        let victim = dir.join("victim.bin");
        std::fs::write(&victim, b"important").unwrap();
        let legit_target = dir.join("legit-target.bin");

        // 乙: 带 `..` 的目标, 但 temp 正是它推导出来的暂存路径 (文件真实存在)
        let traversal_target = dir
            .join("sub")
            .join("..")
            .join("escape.bin")
            .display()
            .to_string();
        let traversal_temp = staging_path(Path::new(&traversal_target)).unwrap();
        std::fs::create_dir_all(traversal_temp.parent().unwrap()).unwrap();
        std::fs::write(&traversal_temp, b"plausible").unwrap();
        // 前提钉住: 不看目标合不合法的话, 这条 (temp, target) 组合本身是自洽的
        assert_eq!(
            staging_path(Path::new(&traversal_target)).unwrap(),
            traversal_temp,
            "乙支的 temp 必须正是该目标的暂存路径, 否则它测的是甲支的判据"
        );

        let document = json!({
            "version": 1,
            "transfers": [
                {
                    "transfer_id": "m1-arbitrary-temp",
                    "target_path": legit_target.to_string_lossy(),
                    "temp_path": victim.to_string_lossy(),
                    "size": 9,
                },
                {
                    "transfer_id": "m1-traversal-target",
                    "target_path": traversal_target,
                    "temp_path": traversal_temp.to_string_lossy(),
                    "size": 9,
                }
            ]
        });
        std::fs::write(&index, document.to_string()).unwrap();

        assert_eq!(
            restore_index(),
            0,
            "temp 与目标不匹配 / 目标含穿越组件时, 一条都不得恢复"
        );
        assert!(
            victim.exists(),
            "恢复不得让任意文件进入可删/可 rename 的状态"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"important");
        assert!(
            traversal_temp.exists(),
            "目标不合法的条目也不得顺手删掉它指向的文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_index_restores_nothing_and_creates_nothing() {
        // 验收 3 的另一半: 索引路径不存在时不得凭空造一个, 也不得把"没有索引"当成错误去重试。
        // 调 `restore_index()` 就要拿回收锁 (见 :1810 的纪律): 索引可读时它会写全局孤儿表。
        let _sweep_guard = SWEEP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-idx-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("does-not-exist.json");
        set_index_path_for_tests(index.clone());

        assert_eq!(restore_index(), 0);
        assert!(!index.exists(), "恢复不得凭空创建索引文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn target_may_not_live_in_the_reserved_staging_dir() {
        // 暂存文件住在 <目标目录>/.rdog-partial/ 里。目标路径若被允许落进这个目录, 就等于允许
        // 客户端把别人的暂存文件当成目标来覆盖/删除 —— 而目标名怎么拼都改变不了"它在不在这个
        // 目录里", 所以这一条比"逐个封堵别名"稳。
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-stagedir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tid = unique_id("stagedir");

        // 大小写与结尾点在任何平台都按"同一个目录名"处理 (过拒无害);
        // 备用数据流只在 Windows 上是别名 —— 非 Windows 上 `:` 是合法文件名字符,
        // `.rdog-partial:s` 是**另一个**目录, 不会与暂存目录重叠, 所以那里不拒。
        let mut targets = vec![
            dir.join(".rdog-partial").join("shadow.bin"),
            dir.join(".RDog-Partial").join("shadow.bin"),
            dir.join(".rdog-partial.").join("shadow.bin"),
        ];
        if cfg!(windows) {
            targets.push(dir.join(".rdog-partial:s").join("shadow.bin"));
        }
        for target in targets {
            let err = execute_put_file(&format!(
                r#"{{"transfer_id":"{tid}","path":"{}","size":5,"resume":false}}"#,
                escape_json(&target.display().to_string())
            ))
            .unwrap_err();
            assert!(
                err.to_string()
                    .contains("file_transfer_staging_path_reserved"),
                "{} 应被拒: {err}",
                target.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alias_spellings_of_one_target_share_a_staging_path_so_resume_still_works() {
        // 哈希输入必须词法归一: 否则 ./a.bin 与 a.bin 会得到两个暂存名 —— 同一个文件却互不相认,
        // 续传找不到自己的半成品, 占用检查也看不见冲突.
        let plain = staging_path(Path::new("/d/a.bin")).unwrap();
        assert_eq!(staging_path(Path::new("/d/./a.bin")).unwrap(), plain);
        assert_eq!(
            staging_path(Path::new("./d/a.bin")).unwrap(),
            staging_path(Path::new("d/a.bin")).unwrap()
        );
        // 大小写不能归一: Model.bin 与 model.bin 在大小写不敏感卷上是同一个 inode, 必须不同名
        assert_ne!(staging_path(Path::new("/d/Model.bin")).unwrap(), plain);
    }

    #[test]
    fn very_long_target_names_still_produce_a_staging_name_within_name_max() {
        // 暂存名比目标名长 25 字节左右; 不截断的话 240 字节的长名会在第一次 @file-chunk 撞
        // ENAMETOOLONG, 而 open 阶段看起来是成功的.
        let long = format!("/d/{}.bin", "x".repeat(240));
        let path = staging_path(Path::new(&long)).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.len() <= 255,
            "暂存名不得越过 NAME_MAX: {} 字节",
            name.len()
        );
        // 两个不同的长名仍然分得开 (哈希在)
        let other = staging_path(Path::new(&format!("/d/{}.bin", "y".repeat(240)))).unwrap();
        assert_ne!(other, path);
    }

    #[test]
    fn case_variant_targets_get_staging_names_that_survive_case_folding() {
        // 在大小写不敏感的卷上 (macOS 默认 APFS / Windows NTFS), `Model.bin` 与 `model.bin` 的
        // 暂存名若只差大小写, 它们就是**同一个 inode** —— 按文本比较的准入看不见, 于是第二条
        // upload 的 resume:false 会把第一条正在写的半成品删掉. 名字里的目标串哈希保证: 折叠
        // 大小写之后两个暂存名仍然不同.
        let upper = staging_path(Path::new("/d/Model.bin")).unwrap();
        let lower = staging_path(Path::new("/d/model.bin")).unwrap();
        assert_ne!(upper, lower);
        assert_ne!(
            upper.to_string_lossy().to_lowercase(),
            lower.to_string_lossy().to_lowercase(),
            "折叠大小写后仍必须不同, 否则在大小写不敏感卷上是同一个文件"
        );
        // 同一个目标串必须稳定得到同一个路径 (续传靠它)
        assert_eq!(
            staging_path(Path::new("/d/Model.bin")).unwrap(),
            staging_path(Path::new("/d/Model.bin")).unwrap()
        );
    }

    #[test]
    fn staging_name_is_injective_so_two_targets_never_share_a_partial() {
        // "model" 与 "model.bin" 用 with_extension 推导会落到同一个暂存名, 于是两个**不同**
        // 目标共用一份半成品: 对 model.bin 的非续传重开会把 model 留着续传的字节删掉, 而
        // model 本身没被碰过. 暂存名必须由目标路径单射得到.
        let _guard = TRANSFER_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("rdog-ft-inject-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let no_ext = dir.join("model");
        let with_ext = dir.join("model.bin");
        let first = unique_id("noext");
        let second = unique_id("withext");
        let data = BASE64_STANDARD.encode(b"xxxxx");

        execute_put_file(&format!(
            r#"{{"transfer_id":"{first}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&no_ext.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{first}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let no_ext_partial = staging_path(&no_ext).unwrap();
        assert!(no_ext_partial.exists(), "无扩展名目标的暂存名应追加后缀");

        // 另一个目标 (不同文件) 非续传重开: 不得删掉 model 的半成品
        execute_put_file(&format!(
            r#"{{"transfer_id":"{second}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&with_ext.display().to_string())
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{second}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        assert!(
            no_ext_partial.exists(),
            "另一个目标的重开不得删掉本目标的半成品"
        );
        assert!(
            staging_path(&with_ext).unwrap().exists(),
            "两个不同目标各自独立一份暂存文件"
        );

        execute_file_cancel(&format!(r#"{{"transfer_id":"{first}"}}"#)).unwrap();
        execute_file_cancel(&format!(r#"{{"transfer_id":"{second}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_path_comparison_normalizes_dot_spellings() {
        // 这条必须直接打比较函数, 不能用 `dir.join(".")` 那种**内部** `.` 去测:
        // std 的 Path::components() 本来就会丢掉内部 `.`, 于是纯 Path 相等也能过 —— 那样
        // 用例在修复前就是绿的, 等于没测. 真正需要守的是**前导** `.` (`./a.bin`), 它的 CurDir
        // 组件会让纯 Path 相等失败.
        let dotted = Path::new("./a.bin.yino-partial");
        let plain = Path::new("a.bin.yino-partial");
        assert_ne!(dotted, plain, "前提: 纯 Path 相等在这里本应失败");
        assert!(
            temp_path_matches(Some(dotted), plain),
            "前导 . 的同义写法应判等"
        );
        assert!(temp_path_matches(Some(plain), dotted), "反向也应判等");

        // 内部 `.` 与绝对路径照旧
        assert!(temp_path_matches(
            Some(Path::new("/tmp/x/./a.bin.yino-partial")),
            Path::new("/tmp/x/a.bin.yino-partial")
        ));

        // 不同文件不能误判成同一个
        assert!(!temp_path_matches(
            Some(Path::new("./a.bin.yino-partial")),
            Path::new("b.bin.yino-partial")
        ));
        assert!(
            !temp_path_matches(None, plain),
            "没有暂存路径的会话不算命中"
        );
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
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":false}}"#
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
        let done =
            execute_file_end(&format!(r#"{{"transfer_id":"{tid}","sha256":"{sha}"}}"#)).unwrap();
        assert_eq!(done["state"], "committed");
        assert_eq!(std::fs::read(&target).unwrap(), b"AAAAABBBBB");

        // 同 id 换目标是合法的 id 复用 (规格 A3/C2): 新会话开起来, 但**旧半成品按它自己的
        // 路径留下** —— 那批字节仍然有效, 删掉等于拿"id 复用"当理由销毁另一个文件的有效进度。
        let other = dir.join("other.bin");
        let new_tid_session = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":false}}"#
        ))
        .unwrap();
        assert_eq!(new_tid_session["state"], "open");
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{first}"}}"#
        ))
        .unwrap();
        let first_partial = staging_path(&target).unwrap();
        assert!(first_partial.exists());

        let switched = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{}","size":10,"resume":false}}"#,
            escape_json(&other.display().to_string())
        ))
        .unwrap();
        assert_eq!(switched["state"], "open");
        assert!(
            first_partial.exists(),
            "换目标不得删掉上一个目标的有效半成品"
        );
        assert!(
            orphan_partials()
                .lock()
                .unwrap()
                .contains_key(&first_partial),
            "旧半成品必须转成孤儿, 由续传窗口回收"
        );

        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = orphan_partials().lock().unwrap().remove(&first_partial);
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
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":false}}"#
        ))
        .unwrap();
        execute_file_chunk(&format!(
            r#"{{"transfer_id":"{tid}","offset":0,"data":"{data}"}}"#
        ))
        .unwrap();
        let partial = staging_path(&target).unwrap();
        let before = std::fs::metadata(&partial).unwrap().len();

        // 同一 id 再来一次必须拒绝: 否则这次 open 会替换掉活跃会话, resume:false 还会把
        // 它正在写的文件删掉
        let err = execute_put_file(&format!(
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":false}}"#
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
            r#"{{"transfer_id":"{tid}","path":"{target_json}","size":10,"resume":false}}"#
        ))
        .unwrap();
        execute_file_cancel(&format!(r#"{{"transfer_id":"{tid}"}}"#)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
