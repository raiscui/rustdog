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
/// ownership lock 被并发替换时的重抢次数上限。
#[cfg(unix)]
const LOCK_ACQUIRE_ATTEMPTS: usize = 4;

// ============================================================================
// 运行目录
// ============================================================================

/// 本机 fast path 的运行目录(可能还不存在)。
#[cfg(unix)]
pub(crate) fn run_dir() -> io::Result<PathBuf> {
    #[cfg(all(test, unix))]
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

/// 准备 base 的父目录。
///
/// 只有**专用运行目录**才收紧到 0700:显式 `socket_path` 的父目录可能是 `/tmp`
/// 或用户自建的共享目录,给它 chmod 会直接打死别人的访问(以 root 跑时甚至会把
/// `/tmp` 变成 0700)。其余父目录只保证存在,权限留给目录所有者。
#[cfg(unix)]
fn ensure_base_parent_dir(dir: &Path) -> io::Result<()> {
    let is_run_dir = run_dir().is_ok_and(|run_dir| run_dir == dir);
    if !is_run_dir {
        return fs::create_dir_all(dir);
    }

    fs::create_dir_all(dir)?;
    // 目录已经存在时(可能是旧版本/手工创建)也要收紧权限。
    fs::set_permissions(dir, Permissions::from_mode(0o700))
}

#[cfg(all(test, unix))]
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

#[cfg(all(test, unix))]
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

/// 用显式 base 路径(用户配置的 `socket_path` / 显式 listen endpoint)派生 lock/meta。
///
/// **base 原样保留**:显式路径可以是 `.../control.sock` 这类历史写法,listener、
/// local-default registry、`compose_listen_endpoints` 用的都是它;这里改写扩展名会
/// 让清理与 chmod 落到另一批文件上,残留 FIFO 反而永远清不掉。
/// lock/meta 与 base 同目录、同 stem,所以显式路径与自动推导路径共用一套生命周期。
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
    Ok(UnixpipeRunPaths {
        base: base.to_path_buf(),
        lock: dir.join(format!("{stem}{LOCK_SUFFIX}")),
        meta: dir.join(format!("{stem}{META_SUFFIX}")),
    })
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
    /// 旧二进制只认 `<base>.rdog-owner.pid`,所以新 owner 也用它声明占用。
    /// 少了这把锁,升级窗口里"旧二进制启动到新 daemon 正在用的显式 base"会
    /// 无条件 unlink 新 daemon 的 FIFO(反向互斥)。
    _legacy_guard: File,
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
            ensure_base_parent_dir(dir)?;
        }

        // 新格式锁先抢: 同一版本的两个 daemon 竞争时,这里给出的错误最准确。
        let Some((mut lock_file, previous)) = acquire_owner_lock(&paths.lock)? else {
            let holder = read_lock_at(&paths.lock)
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
        };

        // 跨版本互斥必须**双向**: 旧二进制用 `<base>.rdog-owner.pid` 表达占用,
        // 新二进制只看 `<stem>.lock` 的话,两个方向的升级窗口都会互相删 FIFO。
        // 所以新 owner 自己也在这把锁上 flock + 写 pid —— 旧 owner 还活着就抢不到
        // (直接失败),我们活着时旧二进制也会因 pid 存活而拒绝接管。
        let legacy_guard = match acquire_flock(&legacy_guard_path(&paths.base), true)? {
            Some(file) => file,
            None => {
                let holder = legacy_guard_holder(&paths.base)?
                    .map(|holder| holder.describe())
                    .unwrap_or_default();
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "unixpipe pipe 已被旧版本 daemon 占用: base={}, legacy_guard={}{holder}",
                        paths.base.display(),
                        legacy_guard_path(&paths.base).display()
                    ),
                ));
            }
        };
        write_legacy_guard_pid(&legacy_guard, std::process::id())?;

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

        // 只清旧 FIFO: 旧格式守卫现在是我们自己持有的,不能一起删掉。
        remove_stale_pipe_files(&paths.base)?;

        Ok(Self {
            _lock_file: lock_file,
            _legacy_guard: legacy_guard,
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
        if let Err(err) = remove_stale_pipe_files(&self.paths.base) {
            log::warn!(
                "清理 unixpipe FIFO 失败: base={}, error={err}",
                self.paths.base.display()
            );
        }
        for path in [
            &self.paths.lock,
            &self.paths.meta,
            &legacy_guard_path(&self.paths.base),
        ] {
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

/// 取得 `<stem>.lock` 的 exclusive ownership。
///
/// 返回 `Ok(None)` 表示已被活跃 owner 持有;`Ok(Some((file, previous)))` 表示本次
/// 抢到了锁,`previous` 是上一个 owner 留下的记录(可能为空)。
///
/// 为什么锁文件可以删却不会出现"两个 owner"(`rdog local` 要求退出后不留 lock,
/// 而旧 ProcessLease 是永久保留 inode): 删掉 lock 后,已经打开旧 inode 的竞争者
/// 仍可能在旧 inode 上抢到锁。这里在拿到锁后复核**路径当前指向的 inode** 是否
/// 就是自己锁住的 inode;不一致说明文件已被前一个 owner unlink 或被别人替换,
/// 于是重开重抢。最多重试 `LOCK_ACQUIRE_ATTEMPTS` 次。
#[cfg(unix)]
fn acquire_owner_lock(lock: &Path) -> io::Result<Option<(File, Option<RunLock>)>> {
    let Some(mut file) = acquire_flock(lock, true)? else {
        return Ok(None);
    };
    let previous = read_lock(&mut file)?;
    Ok(Some((file, previous)))
}

/// 在 `path` 上抢一把 exclusive flock;抢到就返回持有句柄。
///
/// 覆盖两种锁文件: 新格式 `<stem>.lock`(JSON)与旧格式 `<base>.rdog-owner.pid`(纯 pid)。
/// 约束:
/// - `create = false` 时不创建文件(只读探测/清理时不该凭空造出锁文件)。
/// - 抢到后复核"路径当前指向的 inode 是否就是我锁住的 inode";不一致说明文件已被
///   前一个 owner unlink 或被替换,释放后重抢,避免在旧 inode 上与新建 inode 的
///   owner 并存。
/// - `Ok(None)` = 已被别的进程持有。
#[cfg(unix)]
fn acquire_flock(path: &Path, create: bool) -> io::Result<Option<File>> {
    for _ in 0..LOCK_ACQUIRE_ATTEMPTS {
        let file = if create {
            open_lock_file(path)?
        } else {
            match open_existing_lock_file(path)? {
                Some(file) => file,
                None => return Ok(None),
            }
        };

        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(err)) => return Err(err),
        }

        if path_matches_locked_inode(path, &file)? {
            return Ok(Some(file));
        }
        drop(file);
    }

    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "unixpipe ownership lock 在 {LOCK_ACQUIRE_ATTEMPTS} 次尝试内始终被替换: {}",
            path.display()
        ),
    ))
}

/// 路径当前指向的 inode 是否就是 `file` 锁住的那个。
#[cfg(unix)]
fn path_matches_locked_inode(path: &Path, file: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let on_disk = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    Ok(on_disk.ino() == file.metadata()?.ino())
}

/// 旧版本 daemon 的路径守卫文件名。
#[cfg(unix)]
fn legacy_guard_path(base: &Path) -> PathBuf {
    PathBuf::from(format!("{}.rdog-owner.pid", base.display()))
}

/// 把当前 pid 写进旧格式守卫文件(纯 pid 文本,与旧 ProcessLease 的兼容格式一致)。
#[cfg(unix)]
fn write_legacy_guard_pid(file: &File, pid: u32) -> io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};

    let mut handle = file;
    handle.set_len(0)?;
    handle.seek(SeekFrom::Start(0))?;
    handle.write_all(pid.to_string().as_bytes())?;
    handle.sync_data()
}

/// 仍持有旧版本路径守卫的 owner。空结构表示"持有但读不到 pid"。
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct LegacyHolder {
    pid: Option<u32>,
}

#[cfg(unix)]
impl LegacyHolder {
    fn describe(self) -> String {
        match self.pid {
            Some(pid) => format!(", pid={pid}"),
            None => String::new(),
        }
    }
}

/// 旧版本 owner 是否仍活着(用旧守卫文件上的 flock 判定,不信任 PID)。
#[cfg(unix)]
fn legacy_guard_holder(base: &Path) -> io::Result<Option<LegacyHolder>> {
    let guard = legacy_guard_path(base);
    match process_lease::probe_activity(&guard) {
        Ok(process_lease::LeaseActivity::Active { pid }) => Ok(Some(LegacyHolder { pid })),
        Ok(process_lease::LeaseActivity::Inactive) => Ok(None),
        // 探测失败不能当作"确认已死"来授权删除。
        Err(err) => Err(io::Error::new(
            err.kind(),
            format!("探测旧版本 unixpipe 守卫 {} 失败: {err}", guard.display()),
        )),
    }
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
///
/// 注意: 接管路径的 daemon **不能**用它清 stale —— 它必须在持有旧格式守卫之后
/// 只清 FIFO(`remove_stale_pipe_files`),否则会把自己刚拿到的守卫文件删掉。
#[cfg(unix)]
pub(crate) fn remove_unixpipe_artifacts(base: &Path) -> io::Result<()> {
    remove_stale_pipe_files(base)?;

    let base_owner = legacy_guard_path(base);
    let base_owner_lease = crate::zenoh_runtime::process_lease::metadata_path_for_lock(&base_owner);
    for candidate in [base_owner, base_owner_lease] {
        remove_artifact_file(&candidate)?;
    }
    Ok(())
}

/// 只删 base 与其派生 FIFO(不碰任何守卫文件)。
///
/// 用于"已经拿到 ownership、准备自己建 FIFO"的接管路径。
#[cfg(unix)]
fn remove_stale_pipe_files(base: &Path) -> io::Result<()> {
    let fifos = fifo_paths(base);
    for candidate in [base.to_path_buf(), fifos[0].clone(), fifos[1].clone()] {
        remove_artifact_file(&candidate)?;
    }
    Ok(())
}

/// 删一个残留文件: 不存在算成功,是目录报错(绝不删用户目录)。
#[cfg(unix)]
fn remove_artifact_file(candidate: &Path) -> io::Result<()> {
    {
        match fs::metadata(candidate) {
            Ok(metadata) if metadata.is_dir() => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unixpipe 路径 {} 是目录而不是 FIFO 文件,拒绝清理",
                    candidate.display()
                ),
            )),
            Ok(_) => match fs::remove_file(candidate) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(io::Error::new(
                    err.kind(),
                    format!(
                        "清理 stale unixpipe 文件 {} 失败: {err}",
                        candidate.display()
                    ),
                )),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(io::Error::new(
                err.kind(),
                format!(
                    "检查 unixpipe 路径 {} 元数据失败: {err}",
                    candidate.display()
                ),
            )),
        }
    }
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
        let (lock_alive, lock_pid) = probe_lock_owner(&paths.lock)?;
        // 跨版本: 旧二进制只用 `<base>.rdog-owner.pid` 表达 ownership,
        // 漏掉它会把还在跑的老 daemon 报成 stale,进而被 `clean --force` 删除。
        let legacy_holder = legacy_guard_holder(&paths.base)?;
        let owner_alive = lock_alive || legacy_holder.is_some();
        let pid = lock_pid
            .or(meta.as_ref().map(|meta| meta.pid))
            .or_else(|| legacy_holder.and_then(|holder| holder.pid));
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
                .or_else(|| read_lock_at(&paths.lock).map(|lock| lock.started_at_unix_ms)),
        });
    }
    Ok(entries)
}

/// 读某个 lock 文件里上一个 owner 的记录(不存在或格式坏都给 `None`)。
#[cfg(unix)]
fn read_lock_at(lock: &Path) -> Option<RunLock> {
    let mut file = open_existing_lock_file(lock).ok()??;
    read_lock(&mut file).ok().flatten()
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
    use crate::zenoh_runtime::process_lease::LeaseActivity;

    let Some(tmpdir) = legacy_tmpdir() else {
        return Vec::new();
    };
    let Ok(read_dir) = fs::read_dir(&tmpdir) else {
        return Vec::new();
    };

    // 旧版本每个 base 会留下 `_uplink` / `_downlink` 与 `<base>.rdog-owner.pid`,
    // 三条都归属同一个 base。**先按 base 去重、再统一探一次守卫**: 如果逐条记录
    // 各探一次,liveness 证据会挂在 `.rdog-owner.pid` 那条上,而按 base 排序去重时
    // 先到的 FIFO 记录(owner_alive=false)可能把活 owner 的记录挤掉 —— 结果是
    // `local list` 把还在跑的老 daemon 报成 stale,`clean --force` 就把它的 FIFO 删了。
    let mut base_paths: Vec<PathBuf> = Vec::new();
    for entry in read_dir.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with("rdog-") {
            continue;
        }

        let base_name = if let Some(rest) = name.strip_suffix(FIFO_SUFFIXES[0]) {
            rest.to_string()
        } else if let Some(rest) = name.strip_suffix(FIFO_SUFFIXES[1]) {
            rest.to_string()
        } else if let Some(rest) = name.strip_suffix(".rdog-owner.pid") {
            rest.to_string()
        } else {
            continue;
        };
        base_paths.push(tmpdir.join(base_name));
    }

    base_paths.sort();
    base_paths.dedup();

    base_paths
        .into_iter()
        .map(|base| {
            let (owner_alive, pid) = match process_lease::probe_activity(&legacy_guard_path(&base))
            {
                Ok(LeaseActivity::Active { pid }) => (true, pid),
                // 探测失败不当作"确认已死"授权删除;clean 侧还会再复核一次。
                _ => (false, None),
            };
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
    #[cfg(all(test, unix))]
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
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct CleanReport {
    pub(crate) considered: usize,
    /// 被判定为残留的条目(dry-run 下就是"会删什么")。
    pub(crate) stale: Vec<RunEntry>,
    /// 删除期间发现已被新 owner 接管、因此主动放弃的条目。
    pub(crate) skipped: Vec<RunEntry>,
    pub(crate) removed_entries: usize,
    pub(crate) removed_files: usize,
}

/// 只回收死进程残留;存活 daemon 的 pipe 一律不动(即使 `--force`)。
///
/// 分类与删除之间存在时间窗:一个正在重启的 daemon 可能在"判定为残留"之后立刻
/// 拿到 ownership。所以删除前由 `remove_stale_entry` 重新抢锁,抢不到就跳过,
/// 而不是照着旧分类直接 unlink。
#[cfg(unix)]
fn clean_stale_entries(dry_run: bool) -> io::Result<CleanReport> {
    let entries = collect_entries()?;
    let mut report = CleanReport {
        considered: entries.len(),
        stale: Vec::new(),
        skipped: Vec::new(),
        removed_entries: 0,
        removed_files: 0,
    };

    for entry in entries.into_iter().filter(|entry| entry.is_stale()) {
        if dry_run {
            report.stale.push(entry);
            continue;
        }
        match remove_stale_entry(&entry)? {
            Some(files) => {
                report.removed_files += files;
                report.removed_entries += 1;
                report.stale.push(entry);
            }
            None => report.skipped.push(entry),
        }
    }
    Ok(report)
}

/// 在持有 ownership 的前提下删掉一条残留;拿不到 ownership 就返回 `None`。
///
/// 两条路径的 ownership 证据不同:
/// - 运行目录条目: 抢 `<stem>.lock`,抢到就说明没有活跃 owner,持锁期间删除。
/// - 旧 `$TMPDIR` 条目: 复核旧 `<base>.rdog-owner.pid` 的 flock,仍被持有就不删。
#[cfg(unix)]
fn remove_stale_entry(entry: &RunEntry) -> io::Result<Option<usize>> {
    // 旧格式守卫在新旧两条路径上都是 ownership 证据,所以两条路径都必须**持锁删除**:
    // 只探测不持锁的话,旧二进制可能恰好在这个窗口里拿到守卫并建出 FIFO,
    // 随后被我们连 FIFO 带守卫一起删掉。
    let Some(_legacy_guard) = acquire_flock(&legacy_guard_path(&entry.pipe_base), true)? else {
        return Ok(None);
    };
    if entry.legacy_location {
        return remove_entry_files(&entry.pipe_base).map(Some);
    }

    // 新格式 owner 还要再抢一次 `<stem>.lock`: 两个方向都锁住才敢删。
    let paths = paths_for_base(&entry.pipe_base)?;
    let Some((_owner_lock, _previous)) = acquire_owner_lock(&paths.lock)? else {
        return Ok(None);
    };

    remove_entry_files(&entry.pipe_base).map(Some)
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
        "{} entries={} stale={} dry_run={} removed={} files={} skipped={}",
        run_dir_display(),
        report.considered,
        report.stale.len(),
        !force,
        report.removed_entries,
        report.removed_files,
        report.skipped.len(),
    );
    for (index, entry) in report.stale.iter().enumerate() {
        println!("{}", describe_entry(index + 1, entry));
    }
    for entry in &report.skipped {
        println!(
            "跳过 pipe={} reason=owner-became-active",
            entry.pipe_base.display()
        );
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
