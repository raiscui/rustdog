//! 本机 fast path 的落盘目录、名字派生、生命周期锁与 `rdog local` 数据面。
//!
//! 目录与命名 (specs/zenoh-unixpipe-fast-path-plan.md):
//! - 目录: `$XDG_RUNTIME_DIR/rdog/`(设置了就优先,注销即清)否则 `~/.rdog/run/`,权限 0700。
//! - 名字: `<namespace>__<daemon_name>.pipe`;整条 base 路径超过 95 字节上限时压缩成
//!   `<slug>-<8位哈希>.pipe`,完整身份写进同目录 `<stem>.meta`。
//! - 生命周期: `<stem>.lock` 由 daemon 持 flock 并写 pid + 启动时刻;正常退出删
//!   pipe/lock/meta,`kill -9` 留下的残留由下一次启动或 `rdog local clean --force` 回收。
//!
//! liveness 的真相源是 lock 文件上的 OS advisory lock,不是 PID:进程无论怎么死,
//! 内核都会释放 lock,而 PID 会被别的进程复用 —— 只看 PID 会把残留误判成活 daemon。
//! PID 只作为给人看的判定依据 (`pid_alive=no`),`rdog local list` 会把它打出来。

use std::{fs, io};

#[cfg(unix)]
use std::{
    fs::{File, OpenOptions, Permissions, TryLockError},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use sha2::{Digest, Sha256};

#[cfg(unix)]
use crate::zenoh_runtime::process_lease;

/// 运行目录名: `~/.rdog/run`。
#[cfg(unix)]
pub(crate) const RUN_DIR_NAME: &str = "run";

/// Zenoh 1.8.0 会从 base 派生 `<base>_uplink` / `<base>_downlink` 两条 FIFO。
#[cfg(unix)]
pub(crate) const FIFO_SUFFIXES: [&str; 2] = ["_uplink", "_downlink"];

#[cfg(unix)]
const PIPE_SUFFIX: &str = ".pipe";
#[cfg(unix)]
const LOCK_SUFFIX: &str = ".lock";
#[cfg(unix)]
const META_SUFFIX: &str = ".meta";
#[cfg(unix)]
const LOCK_SCHEMA: &str = "rdog.unixpipe-lock.v1";
#[cfg(unix)]
const META_SCHEMA: &str = "rdog.unixpipe-run.v1";
/// Windows 命名管道实例名前缀 (`\\.\pipe\rdog-<namespace>-<name>`)。
#[cfg(windows)]
const WINDOWS_PIPE_PREFIX: &str = "rdog-";
/// 压缩名里的哈希只取前 8 位十六进制,够区分同目录内的身份,又不吃路径预算。
#[cfg(unix)]
const HASH_HEX_LEN: usize = 8;
/// 压缩名至少保留这么多字节的可读 slug,否则压缩出来的名字没人看得懂。
#[cfg(unix)]
const MIN_SLUG_BYTES: usize = 8;
/// 等 Zenoh 建出 FIFO 的上限与轮询间隔(`restrict_fifo_permissions`)。
#[cfg(unix)]
const FIFO_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(unix)]
const FIFO_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);

// ============================================================================
// 运行目录
// ============================================================================

/// 本机 fast path 的运行目录(可能还不存在)。
#[cfg(unix)]
pub(crate) fn run_dir() -> io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = run_dir_test_override() {
        return Ok(dir);
    }

    Ok(resolve_run_dir(
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        std::env::var_os("HOME").as_deref(),
        std::env::temp_dir().join("rdog-run"),
    ))
}

/// 目录解析的纯函数形态,便于不碰进程环境就能锁住三级优先级的语义。
#[cfg(unix)]
fn resolve_run_dir(
    xdg_runtime_dir: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    fallback: PathBuf,
) -> PathBuf {
    if let Some(runtime_dir) = xdg_runtime_dir.filter(|value| !value.is_empty()) {
        return PathBuf::from(runtime_dir).join("rdog");
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".rdog").join(RUN_DIR_NAME);
    }
    // 没有 HOME 的容器/沙箱兜底: 仍然给一个稳定目录,而不是退回 per-session 的 TMPDIR。
    fallback
}

/// 创建运行目录并固定成 0700。
#[cfg(unix)]
fn ensure_run_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    // 目录已经存在时(可能是旧版本/手工创建)也要收紧权限。
    fs::set_permissions(dir, Permissions::from_mode(0o700))
}

#[cfg(test)]
thread_local! {
    static RUN_DIR_TEST_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
pub(crate) fn set_run_dir_test_override(path: Option<PathBuf>) {
    RUN_DIR_TEST_OVERRIDE.with(|slot| {
        *slot.borrow_mut() = path;
    });
}

#[cfg(test)]
fn run_dir_test_override() -> Option<PathBuf> {
    RUN_DIR_TEST_OVERRIDE.with(|slot| slot.borrow().clone())
}

/// 旧 `$TMPDIR` 残留扫描的测试替身;与运行目录分开,避免测试读到真实 `/tmp`。
#[cfg(all(test, unix))]
thread_local! {
    static LEGACY_TMPDIR_TEST_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
pub(crate) fn set_legacy_tmpdir_test_override(path: Option<PathBuf>) {
    LEGACY_TMPDIR_TEST_OVERRIDE.with(|slot| {
        *slot.borrow_mut() = path;
    });
}

#[cfg(all(test, unix))]
fn legacy_tmpdir_test_override() -> Option<PathBuf> {
    LEGACY_TMPDIR_TEST_OVERRIDE.with(|slot| slot.borrow().clone())
}

// ============================================================================
// 名字派生
// ============================================================================

/// 一条 daemon 在本机 fall path 上的三个落盘路径。
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnixpipeRunPaths {
    /// FIFO base;Zenoh 在其上派生 `_uplink` / `_downlink`。
    pub(crate) base: PathBuf,
    /// 生命周期锁(flock + pid + 启动时刻)。
    pub(crate) lock: PathBuf,
    /// 完整身份记录,压缩名场景下是唯一能还原 `namespace/daemon_name` 的地方。
    pub(crate) meta: PathBuf,
}

/// 按 (namespace, daemon_name) 推导运行目录里的落盘路径。
#[cfg(unix)]
pub(crate) fn unixpipe_run_paths(
    namespace: &str,
    daemon_name: &str,
) -> io::Result<UnixpipeRunPaths> {
    validate_unixpipe_component("namespace", namespace)?;
    validate_unixpipe_component("daemon_name", daemon_name)?;
    let dir = run_dir()?;
    Ok(paths_for_identity(&dir, namespace, daemon_name)?)
}

/// 用显式 base 路径(用户配置的 `socket_path`)反推 lock/meta 路径。
///
/// lock/meta 始终与 base 同目录、同 stem,这样 `rdog local list` 只扫一个目录就能
/// 看到所有 daemon,显式路径与自动推导路径走同一套生命周期。
#[cfg(unix)]
pub(crate) fn paths_for_base(base: &Path) -> io::Result<UnixpipeRunPaths> {
    let stem = base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unixpipe base 路径缺少文件名: {}", base.display()),
            )
        })?;
    let dir = base.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unixpipe base 路径缺少父目录: {}", base.display()),
        )
    })?;
    Ok(paths_for_stem(&dir, &stem))
}

#[cfg(unix)]
fn paths_for_identity(
    dir: &Path,
    namespace: &str,
    daemon_name: &str,
) -> io::Result<UnixpipeRunPaths> {
    Ok(paths_for_stem(
        dir,
        &pipe_stem(dir, namespace, daemon_name)?,
    ))
}

#[cfg(unix)]
fn paths_for_stem(dir: &Path, stem: &str) -> UnixpipeRunPaths {
    UnixpipeRunPaths {
        base: dir.join(format!("{stem}{PIPE_SUFFIX}")),
        lock: dir.join(format!("{stem}{LOCK_SUFFIX}")),
        meta: dir.join(format!("{stem}{META_SUFFIX}")),
    }
}

/// 推导文件名主干: 优先人可读的 `<namespace>__<daemon_name>`,超预算才压缩。
#[cfg(unix)]
fn pipe_stem(dir: &Path, namespace: &str, daemon_name: &str) -> io::Result<String> {
    let full = format!("{namespace}__{daemon_name}");
    let budget = stem_byte_budget(dir);

    if full.len() <= budget && is_plain_file_component(&full) {
        return Ok(full);
    }

    let slug_budget = budget.saturating_sub(1 + HASH_HEX_LEN);
    if slug_budget < MIN_SLUG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "本机 fast path 运行目录太长,压缩后的 pipe 名仍超出 {len} 字节上限: dir={dir:?}(可用 {budget} 字节,压缩名至少需要 {need} 字节);请把 XDG_RUNTIME_DIR 指向更短的目录",
                len = crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES,
                dir = dir,
                budget = budget,
                need = MIN_SLUG_BYTES + 1 + HASH_HEX_LEN,
            ),
        ));
    }

    Ok(format!(
        "{}-{}",
        ascii_slug(&full, slug_budget),
        identity_hash(namespace, daemon_name)
    ))
}

/// base 文件名可用的字节数: 上限减去目录、分隔符与 `.pipe` 后缀。
#[cfg(unix)]
fn stem_byte_budget(dir: &Path) -> usize {
    let max = crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES;
    let mut dir_bytes = dir.as_os_str().as_encoded_bytes().len();
    if dir.as_os_str().as_encoded_bytes().ends_with(b"/") {
        dir_bytes -= 1;
    }
    max.saturating_sub(dir_bytes + 1 + PIPE_SUFFIX.len())
}

/// 文件名能否直接承载完整身份: 只用可移植字符,且不是隐藏文件。
#[cfg(unix)]
fn is_plain_file_component(value: &str) -> bool {
    !value.starts_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

/// 把 `<namespace>__<daemon_name>` 压成 ASCII slug,按字节预算截断且不切断 UTF-8。
#[cfg(unix)]
fn ascii_slug(value: &str, budget: usize) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            ch
        } else {
            '-'
        };
        let mut buf = [0_u8; 4];
        let encoded = mapped.encode_utf8(&mut buf);
        if slug.len() + encoded.len() > budget {
            break;
        }
        slug.push(mapped);
    }

    let trimmed = slug.trim_matches('.').trim_end_matches('-');
    if trimmed.is_empty() {
        // 预算极小时退化成一个固定的可读前缀,哈希仍然保证唯一性。
        "pipe".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(unix)]
fn identity_hash(namespace: &str, daemon_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update([0_u8]);
    hasher.update(daemon_name.as_bytes());
    hasher.finalize()[..HASH_HEX_LEN / 2]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Zenoh 1.8.0 从 base 派生的两条 FIFO。
#[cfg(unix)]
pub(crate) fn fifo_paths(base: &Path) -> [PathBuf; 2] {
    let base = base.as_os_str().to_string_lossy();
    [
        PathBuf::from(format!("{base}{}", FIFO_SUFFIXES[0])),
        PathBuf::from(format!("{base}{}", FIFO_SUFFIXES[1])),
    ]
}

#[cfg(unix)]
pub(crate) fn validate_unixpipe_component(field: &str, value: &str) -> io::Result<()> {
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unixpipe 路径组件 `{field}` 不能为空"),
        ));
    }
    if value.contains('/') || value.contains(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unixpipe 路径组件 `{field}` 不能包含 `/` 或空白字符(实际: {value:?})"),
        ));
    }
    Ok(())
}

// ============================================================================
// 生命周期: lock / meta / stale 清理
// ============================================================================

#[cfg(unix)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunLock {
    schema: String,
    pid: u32,
    started_at_unix_ms: u128,
}

#[cfg(unix)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunMeta {
    schema: String,
    namespace: String,
    daemon_name: String,
    pipe_base: PathBuf,
    pid: u32,
    created_at_unix_ms: u128,
}

/// daemon 生命周期内持有的 pipe ownership。
///
/// 覆盖范围必须是 listener 的整个生命周期: 持有期间同一 base 的第二实例会在
/// `try_lock` 上被拒绝且不会执行任何 destructive cleanup; Drop 时删除 FIFO 与
/// lock/meta,所以 SIGINT/SIGTERM 优雅退出后目录里不留东西。
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct UnixpipeRunGuard {
    _lock_file: File,
    paths: UnixpipeRunPaths,
}

#[cfg(unix)]
impl UnixpipeRunGuard {
    /// 接管 base 路径: 加锁 + 写 meta + 清理上一次崩溃留下的 FIFO。
    pub(crate) fn acquire(
        paths: UnixpipeRunPaths,
        namespace: &str,
        daemon_name: &str,
    ) -> io::Result<Self> {
        if let Some(dir) = paths.base.parent() {
            ensure_run_dir(dir)?;
        }

        let mut lock_file = open_lock_file(&paths.lock)?;
        let previous = read_lock(&mut lock_file)?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let holder = previous
                    .map(|lock| {
                        format!(
                            ", pid={}, started_at_ms={}",
                            lock.pid, lock.started_at_unix_ms
                        )
                    })
                    .unwrap_or_default();
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "unixpipe pipe 已被活跃 daemon 占用: base={}, lock={}{holder}",
                        paths.base.display(),
                        paths.lock.display()
                    ),
                ));
            }
            Err(TryLockError::Error(err)) => return Err(err),
        }

        // 拿到 exclusive lock 就证明上一个 owner 已经死了(哪怕它的 PID 被别的进程复用)。
        // 这里把判定依据写进日志,现场排查"到底是残留还是真活着"时不用再猜。
        if let Some(previous) = previous {
            log::info!(
                "回收 unixpipe 残留: base={}, previous_pid={}, previous_pid_alive={}, previous_started_at_ms={}",
                paths.base.display(),
                previous.pid,
                process_lease::process_exists(previous.pid),
                previous.started_at_unix_ms
            );
        }

        let now = unix_timestamp_ms();
        let lock = RunLock {
            schema: LOCK_SCHEMA.to_string(),
            pid: std::process::id(),
            started_at_unix_ms: now,
        };
        write_lock(&mut lock_file, &lock)?;

        let meta = RunMeta {
            schema: META_SCHEMA.to_string(),
            namespace: namespace.to_string(),
            daemon_name: daemon_name.to_string(),
            pipe_base: paths.base.clone(),
            pid: lock.pid,
            created_at_unix_ms: now,
        };
        process_lease::write_json_atomically(&paths.meta, &meta)?;
        tighten_file_permissions(&paths.meta)?;

        remove_unixpipe_artifacts(&paths.base)?;

        Ok(Self {
            _lock_file: lock_file,
            paths,
        })
    }

    #[cfg(test)]
    pub(crate) fn base(&self) -> &Path {
        &self.paths.base
    }

    /// 把已创建的 FIFO 收紧到 0600。
    ///
    /// FIFO 由 Zenoh 的 unixpipe link 在 listen 阶段创建,可能比 session open 稍晚,
    /// mode 也受 umask 影响(常见 0644),所以这里做一次有界重试再 chmod。
    /// 目录已经是 0700,这是第二道门;FIFO 一直不出现说明本次没走 unixpipe,不算错误。
    pub(crate) fn restrict_fifo_permissions(&self) -> io::Result<()> {
        let deadline = Instant::now() + FIFO_POLL_TIMEOUT;
        loop {
            let fifos = fifo_paths(&self.paths.base);
            let existing: Vec<&PathBuf> = fifos.iter().filter(|path| path.exists()).collect();
            if !existing.is_empty() {
                for path in existing {
                    match fs::set_permissions(path, Permissions::from_mode(0o600)) {
                        Ok(()) => {}
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                        Err(err) => {
                            return Err(io::Error::new(
                                err.kind(),
                                format!("收紧 unixpipe FIFO 权限失败 {}: {err}", path.display()),
                            ))
                        }
                    }
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(FIFO_POLL_INTERVAL);
        }
    }
}

#[cfg(unix)]
impl Drop for UnixpipeRunGuard {
    fn drop(&mut self) {
        if let Err(err) = remove_unixpipe_artifacts(&self.paths.base) {
            log::warn!(
                "清理 unixpipe FIFO 失败: base={}, error={err}",
                self.paths.base.display()
            );
        }
        for path in [&self.paths.lock, &self.paths.meta] {
            if let Err(err) = fs::remove_file(path) {
                if err.kind() != io::ErrorKind::NotFound {
                    log::warn!("清理 unixpipe 生命周期文件失败: {}: {err}", path.display());
                }
            }
        }
    }
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

/// 只读探测用: 不创建文件 —— `rdog local list` 不能靠"看一眼"就在运行目录里留东西。
#[cfg(unix)]
fn open_existing_lock_file(path: &Path) -> io::Result<Option<File>> {
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn read_lock(file: &mut File) -> io::Result<Option<RunLock>> {
    file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(serde_json::from_str::<RunLock>(contents.trim()).ok())
}

#[cfg(unix)]
fn write_lock(file: &mut File, lock: &RunLock) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(lock).map_err(io::Error::other)?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_data()
}

#[cfg(unix)]
fn tighten_file_permissions(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, Permissions::from_mode(0o600))
}

/// 删除 base 及其派生 FIFO,以及旧版本 `<base>.rdog-owner.pid*` 守卫文件。
///
/// 文件存在但**是目录**时报错而不是删,避免把用户的同名目录当残留清掉。
/// 返回 Ok(()) 表示路径已干净(本来就干净或已被本调用清理)。
#[cfg(unix)]
pub(crate) fn remove_unixpipe_artifacts(base: &Path) -> io::Result<()> {
    let fifos = fifo_paths(base);
    let base_owner = PathBuf::from(format!("{}.rdog-owner.pid", base.display()));
    let base_owner_lease = crate::zenoh_runtime::process_lease::metadata_path_for_lock(&base_owner);

    for candidate in [
        base.to_path_buf(),
        fifos[0].clone(),
        fifos[1].clone(),
        base_owner,
        base_owner_lease,
    ] {
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "unixpipe 路径 {} 是目录而不是 FIFO 文件,拒绝清理",
                        candidate.display()
                    ),
                ));
            }
            Ok(_) => match fs::remove_file(&candidate) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(io::Error::new(
                        err.kind(),
                        format!(
                            "清理 stale unixpipe 文件 {} 失败: {err}",
                            candidate.display()
                        ),
                    ))
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!(
                        "检查 unixpipe 路径 {} 元数据失败: {err}",
                        candidate.display()
                    ),
                ))
            }
        }
    }

    Ok(())
}

/// 读 `.meta` 里的完整身份;压缩名场景下这是唯一的身份来源。
#[cfg(unix)]
fn read_meta(path: &Path) -> Option<RunMeta> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<RunMeta>(&bytes)
        .ok()
        .filter(|meta| meta.schema == META_SCHEMA)
}

#[cfg(unix)]
fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

// ============================================================================
// 观测与回收 (`rdog local list` / `rdog local clean`)
// ============================================================================

/// `rdog local list` 里的一条记录。
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunEntry {
    pub(crate) pipe_base: PathBuf,
    pub(crate) namespace: Option<String>,
    pub(crate) daemon_name: Option<String>,
    pub(crate) pid: Option<u32>,
    pub(crate) fifo_files: usize,
    /// lock 上的 flock 是否仍被某个进程持有 —— liveness 的真相源。
    pub(crate) owner_alive: bool,
    /// lock 里那个 PID 是否还在(只作判定依据;PID 复用会给出 `no` + `owner_alive=true`)。
    pub(crate) pid_alive: Option<bool>,
    /// 停在旧 `$TMPDIR` 位置、不受新运行目录治理的残留。
    pub(crate) legacy_location: bool,
    pub(crate) started_at_unix_ms: Option<u128>,
}

#[cfg(unix)]
impl RunEntry {
    fn verdict(&self) -> &'static str {
        match (self.legacy_location, self.owner_alive, self.pid_alive) {
            (true, true, _) => "legacy-live",
            (true, false, _) => "legacy-stale",
            (false, true, _) => "live",
            // lock 能拿到说明 owner 已死;PID 仍存在 = 被别的进程复用了。
            (false, false, Some(true)) => "stale-pid-reused",
            (false, false, _) => "stale-pid-dead",
        }
    }

    fn is_stale(&self) -> bool {
        !self.owner_alive
    }
}

#[cfg(unix)]
fn probe_lock_owner(lock: &Path) -> io::Result<(bool, Option<u32>)> {
    let Some(mut file) = open_existing_lock_file(lock)? else {
        return Ok((false, None));
    };
    let recorded = read_lock(&mut file).ok().flatten();
    let pid = recorded.as_ref().map(|lock| lock.pid);

    match file.try_lock() {
        // 拿到 exclusive lock: 没有活跃 owner,立即释放并返回。
        Ok(()) => Ok((false, pid)),
        Err(TryLockError::WouldBlock) => Ok((true, pid)),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

/// 收集当前用户在本机 fast path 上的全部 pipe(运行目录 + 旧 TMPDIR 残留)。
#[cfg(unix)]
fn collect_entries() -> io::Result<Vec<RunEntry>> {
    let dir = run_dir()?;
    let mut entries = collect_run_dir_entries(&dir)?;
    entries.extend(collect_legacy_entries());
    entries.sort_by(|left, right| left.pipe_base.cmp(&right.pipe_base));
    Ok(entries)
}

#[cfg(unix)]
fn collect_run_dir_entries(dir: &Path) -> io::Result<Vec<RunEntry>> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(io::Error::new(
                err.kind(),
                format!("读取本机 fast path 运行目录 {} 失败: {err}", dir.display()),
            ))
        }
    };

    let mut stems = Vec::new();
    for entry in read_dir.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if let Some(stem) = pipe_stem_from_file_name(&name) {
            stems.push(stem);
        }
    }
    stems.sort();
    stems.dedup();

    let mut entries = Vec::new();
    for stem in stems {
        let paths = paths_for_stem(dir, &stem);
        let meta = read_meta(&paths.meta);
        let fallback_identity = meta
            .is_none()
            .then(|| stem.split_once("__"))
            .flatten()
            .map(|(namespace, daemon_name)| (namespace.to_string(), daemon_name.to_string()));
        let (owner_alive, pid) = probe_lock_owner(&paths.lock)?;
        let pid = pid.or(meta.as_ref().map(|meta| meta.pid));
        let fifo_files = fifo_paths(&paths.base)
            .iter()
            .filter(|path| path.exists())
            .count();
        entries.push(RunEntry {
            pipe_base: paths.base,
            namespace: meta
                .as_ref()
                .map(|meta| meta.namespace.clone())
                .or_else(|| {
                    fallback_identity
                        .as_ref()
                        .map(|(namespace, _)| namespace.clone())
                }),
            daemon_name: meta
                .as_ref()
                .map(|meta| meta.daemon_name.clone())
                .or_else(|| fallback_identity.map(|(_, daemon_name)| daemon_name)),
            pid,
            fifo_files,
            owner_alive,
            pid_alive: pid.map(crate::zenoh_runtime::process_lease::process_exists),
            legacy_location: false,
            started_at_unix_ms: meta
                .as_ref()
                .map(|meta| meta.created_at_unix_ms)
                .or_else(|| read_started_at(&paths.lock)),
        });
    }
    Ok(entries)
}

#[cfg(unix)]
fn read_started_at(lock: &Path) -> Option<u128> {
    let mut file = open_existing_lock_file(lock).ok()??;
    read_lock(&mut file)
        .ok()
        .flatten()
        .map(|lock| lock.started_at_unix_ms)
}

/// 从运行目录里的文件名还原 stem: `<stem>.pipe` / `<stem>.pipe_uplink|downlink` / `<stem>.lock` / `<stem>.meta`。
#[cfg(unix)]
fn pipe_stem_from_file_name(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_suffix(PIPE_SUFFIX) {
        return Some(rest.to_string());
    }
    for suffix in FIFO_SUFFIXES {
        if let Some(rest) = name.strip_suffix(suffix) {
            if let Some(stem) = rest.strip_suffix(PIPE_SUFFIX) {
                return Some(stem.to_string());
            }
        }
    }
    for suffix in [LOCK_SUFFIX, META_SUFFIX] {
        if let Some(rest) = name.strip_suffix(suffix) {
            return Some(rest.to_string());
        }
    }
    None
}

/// 旧版本把 pipe 放在 `$TMPDIR/rdog-<namespace>-<name>.pipe`,升级后不会自己消失。
///
/// 这里的 liveness 用旧守卫文件上的 flock 判断(`process_lease::probe_activity`),
/// 所以"老版本 daemon 仍在跑"不会被误删。
#[cfg(unix)]
fn collect_legacy_entries() -> Vec<RunEntry> {
    use crate::zenoh_runtime::process_lease::{probe_activity, LeaseActivity};

    let Some(tmpdir) = legacy_tmpdir() else {
        return Vec::new();
    };
    let Ok(read_dir) = fs::read_dir(&tmpdir) else {
        return Vec::new();
    };

    let mut bases: Vec<(PathBuf, Option<u32>, bool)> = Vec::new();
    for entry in read_dir.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with("rdog-") {
            continue;
        }

        // 旧版本每个 base 会留下 `_uplink` / `_downlink` 与 `<base>.rdog-owner.pid`,
        // 三条都归属同一个 base,统一折算成一条记录。
        let (base_name, owner_guard) = if let Some(rest) = name.strip_suffix(FIFO_SUFFIXES[0]) {
            (rest.to_string(), None)
        } else if let Some(rest) = name.strip_suffix(FIFO_SUFFIXES[1]) {
            (rest.to_string(), None)
        } else if let Some(rest) = name.strip_suffix(".rdog-owner.pid") {
            (
                rest.to_string(),
                Some(tmpdir.join(format!("{rest}.rdog-owner.pid"))),
            )
        } else {
            continue;
        };

        let base = tmpdir.join(&base_name);
        let activity = owner_guard.as_deref().map(probe_activity).transpose();
        let (owner_alive, pid) = match activity {
            Ok(Some(LeaseActivity::Active { pid })) => (true, pid),
            _ => (false, None),
        };
        bases.push((base, pid, owner_alive));
    }

    bases.sort_by(|left, right| left.0.cmp(&right.0));
    bases.dedup_by(|left, right| left.0 == right.0);

    bases
        .into_iter()
        .map(|(base, pid, owner_alive)| {
            let identity = legacy_identity_from_base(&base);
            RunEntry {
                namespace: identity.as_ref().map(|(namespace, _)| namespace.clone()),
                daemon_name: identity.map(|(_, daemon_name)| daemon_name),
                pid,
                fifo_files: fifo_paths(&base)
                    .iter()
                    .filter(|path| path.exists())
                    .count(),
                owner_alive,
                pid_alive: None,
                legacy_location: true,
                started_at_unix_ms: None,
                pipe_base: base,
            }
        })
        // base 本身不一定存在(Zenoh 只建 _uplink/_downlink),按 FIFO 判定"还在不在"。
        .filter(|entry| entry.fifo_files > 0)
        .collect()
}

/// 旧命名 `rdog-<namespace>-<daemon_name>.pipe` 的身份还原。
///
/// 旧规则用第一个 `-` 切分,namespace 里带 `-` 会切错 —— 这正是新命名改用
/// `<namespace>__<daemon_name>` + `.meta` 的原因。这里只服务升级诊断与清理。
#[cfg(unix)]
fn legacy_identity_from_base(base: &Path) -> Option<(String, String)> {
    let name = base.file_name()?.to_str()?.strip_suffix(PIPE_SUFFIX)?;
    let middle = name.strip_prefix("rdog-")?;
    let (namespace, daemon_name) = middle.split_once('-')?;
    (!namespace.is_empty() && !daemon_name.is_empty())
        .then(|| (namespace.to_string(), daemon_name.to_string()))
}

#[cfg(unix)]
fn legacy_tmpdir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = legacy_tmpdir_test_override() {
        return Some(dir);
    }

    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    // 新运行目录本身可能就在 /tmp 兜底分支下,别把同一批文件数两遍。
    let run_dir = run_dir().ok()?;
    (run_dir != tmpdir).then_some(tmpdir)
}

/// 未托管 unixpipe 候选的列出(只服务 `rdog control self` / 空 target 的升级诊断)。
///
/// 判据是"FIFO 还在",不看 liveness: 诊断要解释"为什么本机明明有 pipe 却选不出
/// daemon",死残留和活 daemon 都属于这个解释范围。
#[cfg(unix)]
pub(crate) fn unmanaged_pipe_candidates(namespace_filter: Option<&str>) -> io::Result<Vec<String>> {
    let mut candidates = Vec::new();
    for entry in collect_entries()? {
        if entry.fifo_files == 0 {
            continue;
        }
        let (Some(namespace), Some(daemon_name)) = (entry.namespace, entry.daemon_name) else {
            continue;
        };
        if namespace_filter.is_some_and(|filter| filter != namespace) {
            continue;
        }
        candidates.push(format!("{namespace}/{daemon_name}"));
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

/// `rdog local clean` 的结果。
#[derive(Debug, Default)]
pub(crate) struct CleanReport {
    pub(crate) considered: usize,
    pub(crate) stale: Vec<RunEntry>,
    pub(crate) removed_entries: usize,
    pub(crate) removed_files: usize,
}

/// 只回收死进程残留;存活 daemon 的 pipe 一律不动(即使 `--force`)。
#[cfg(unix)]
fn clean_stale_entries(dry_run: bool) -> io::Result<CleanReport> {
    let entries = collect_entries()?;
    let stale: Vec<RunEntry> = entries
        .iter()
        .filter(|entry| entry.is_stale())
        .cloned()
        .collect();
    let mut report = CleanReport {
        considered: entries.len(),
        stale,
        removed_entries: 0,
        removed_files: 0,
    };

    if dry_run {
        return Ok(report);
    }

    for entry in &report.stale {
        report.removed_files += remove_entry_files(&entry.pipe_base)?;
        report.removed_entries += 1;
    }
    Ok(report)
}

/// 删除一条 pipe 的全部落盘文件,返回删掉的个数。
#[cfg(unix)]
fn remove_entry_files(base: &Path) -> io::Result<usize> {
    let fifos = fifo_paths(base);
    let owner_guard = PathBuf::from(format!("{}.rdog-owner.pid", base.display()));
    let owner_lease = crate::zenoh_runtime::process_lease::metadata_path_for_lock(&owner_guard);
    let paths = paths_for_base(base)?;

    let mut candidates = vec![
        base.to_path_buf(),
        fifos[0].clone(),
        fifos[1].clone(),
        owner_guard,
        owner_lease,
        paths.lock,
        paths.meta,
    ];

    let mut removed = 0;
    for candidate in candidates {
        match fs::remove_file(&candidate) {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!("删除残留 {} 失败: {err}", candidate.display()),
                ))
            }
        }
    }
    Ok(removed)
}

// ============================================================================
// CLI 输出 (`rdog local list|clean`)
// ============================================================================

fn run_dir_display() -> String {
    #[cfg(unix)]
    {
        match run_dir() {
            Ok(dir) => match fs::metadata(&dir) {
                Ok(metadata) => format!(
                    "run_dir={} exists=yes mode={:04o}",
                    dir.display(),
                    metadata.permissions().mode() & 0o777
                ),
                Err(_) => format!("run_dir={} exists=no", dir.display()),
            },
            Err(err) => format!("run_dir=<unresolved: {err}>"),
        }
    }
    #[cfg(windows)]
    {
        r"run_dir=\\.\pipe exists=yes mode=-".to_string()
    }
}

#[cfg(unix)]
fn describe_entry(index: usize, entry: &RunEntry) -> String {
    let namespace = entry.namespace.as_deref().unwrap_or("?");
    let daemon_name = entry.daemon_name.as_deref().unwrap_or("?");
    let pid = entry
        .pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "?".to_string());
    let pid_alive = match entry.pid_alive {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    let age = entry
        .started_at_unix_ms
        .map(|started| {
            let elapsed_ms = unix_timestamp_ms_now().saturating_sub(started);
            format!(" age={}s", elapsed_ms / 1000)
        })
        .unwrap_or_default();

    format!(
        "[{index}] pipe={} namespace={namespace} daemon_name={daemon_name} pid={pid} fifo={} status={} pid_alive={pid_alive}{age}",
        entry.pipe_base.display(),
        entry.fifo_files,
        entry.verdict(),
    )
}

#[cfg(unix)]
fn unix_timestamp_ms_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// `rdog local list` 的 stdout 形态。
#[cfg(unix)]
pub(crate) fn print_list() -> Result<(), String> {
    let entries = collect_entries().map_err(|err| err.to_string())?;
    let live = entries.iter().filter(|entry| entry.owner_alive).count();
    let stale = entries.len() - live;

    println!(
        "{} entries={} live={live} stale={stale}",
        run_dir_display(),
        entries.len()
    );
    for (index, entry) in entries.iter().enumerate() {
        println!("{}", describe_entry(index + 1, entry));
    }
    if stale > 0 {
        println!("提示: {stale} 条残留未回收,`rdog local clean` 可预览,`rdog local clean --force` 才会真正删除");
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn print_clean(force: bool) -> Result<(), String> {
    let report = clean_stale_entries(!force).map_err(|err| err.to_string())?;

    println!(
        "{} entries={} stale={} dry_run={} removed={} files={}",
        run_dir_display(),
        report.considered,
        report.stale.len(),
        !force,
        report.removed_entries,
        report.removed_files,
    );
    for (index, entry) in report.stale.iter().enumerate() {
        println!("{}", describe_entry(index + 1, entry));
    }
    if !force && !report.stale.is_empty() {
        println!(
            "提示: 以上 {} 条会被 `rdog local clean --force` 删除(当前只读)",
            report.stale.len()
        );
    }
    Ok(())
}

/// Windows 命名管道实例枚举。
///
/// Windows fast path 本身由 RAI-385 落地;这里只提供观测面,把 `\\.\pipe\` 里
/// `rdog-` 前缀的实例如实列出来。命名管道不落盘,所以 `clean` 没有可删对象。
#[cfg(windows)]
fn windows_pipe_instances() -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(r"\\.\pipe\")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(WINDOWS_PIPE_PREFIX) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(windows)]
pub(crate) fn print_list() -> Result<(), String> {
    let instances = windows_pipe_instances().map_err(|err| err.to_string())?;
    println!(
        "{} entries={} live={} stale=0",
        run_dir_display(),
        instances.len(),
        instances.len()
    );
    for (index, name) in instances.iter().enumerate() {
        // Windows 命名管道实例不携带 namespace/daemon_name 元数据,只能列原名。
        println!(
            "[{}] pipe={name} namespace=? daemon_name=? pid=? fifo=1 status=live pid_alive=unknown note=windows-named-pipe",
            index + 1
        );
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn print_clean(_force: bool) -> Result<(), String> {
    println!(
        "{} entries=0 stale=0 dry_run=false removed=0 files=0 note=Windows 命名管道不落盘,无残留可清",
        run_dir_display()
    );
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn print_list() -> Result<(), String> {
    Err("当前平台没有本机 fast path 落盘位置".to_string())
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn print_clean(_force: bool) -> Result<(), String> {
    Err("当前平台没有本机 fast path 落盘位置".to_string())
}

#[cfg(all(test, unix))]
mod tests;
