//! Zenoh unixpipe路径、ownership、探测与listener composition。

use std::{io, path::PathBuf};

#[cfg(unix)]
use std::{fs, path::Path, sync::mpsc, thread, time::Duration};

#[cfg(unix)]
use crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES;

#[cfg(unix)]
use super::process_lease;

// ============================================================================
// unixpipe (named pipe / FIFO) 本机 fast path辅助函数
//
// Zenoh 1.8.0 的`transport_unixpipe`实际是named pipe(FIFO),不是Unix domain socket。
// 它会从base路径派生`<base>_uplink` / `<base>_downlink`两条FIFO文件。
// macOS `sun_path`限制104字节,base必须不超过95字节。
// ============================================================================

/// Zenoh 1.8.0 unixpipe使用的locator前缀。
const UNIXPIPE_LOCATOR_PREFIX: &str = "unixpipe";

/// 轻量检查 unixpipe base 路径对应的 FIFO 文件是否存在。
///
/// 注意:这里只做 `Path::exists`,**不**主动 open FIFO 探活。
/// 原因: 主动 open FIFO 写端后会立即关闭,daemon 端的 request channel 会看到 EOF
/// 并影响后续 client 的正常 connect 流程。我们只关心"daemon 留没留这个文件",
/// 真正的连接性由 zenoh::open 内部处理。
#[cfg(unix)]
pub(super) fn unixpipe_base_path_alive(base: &Path) -> bool {
    let uplinks = unixpipe_fifo_paths(base);
    uplinks.iter().any(|path| path.exists())
}

/// 根据 (namespace, daemon_name) 推导 base 路径,daemon 和 control 端用同一份规则。
///
/// 路径模板: `{tmpdir}/rdog-{namespace}-{daemon_name}.pipe`
/// - `tmpdir` 优先级: `$TMPDIR` > `/tmp`。
/// - macOS 的 `$TMPDIR` 是 per-user(例如 `/var/folders/xx/yy/T/`),自然有权限隔离。
/// - 扩展名 `.pipe` 表明这是 FIFO(named pipe),不是 Unix domain socket,避免后人误以为是 socket。
#[cfg(unix)]
pub(super) fn unixpipe_socket_path(namespace: &str, daemon_name: &str) -> io::Result<PathBuf> {
    validate_unixpipe_component("namespace", namespace)?;
    validate_unixpipe_component("daemon_name", daemon_name)?;

    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let candidate = tmpdir.join(format!("rdog-{namespace}-{daemon_name}.pipe"));
    let path_str = candidate.as_os_str();

    if path_str.len() > UNIXPIPE_SOCKET_PATH_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "推导出的 unixpipe base 路径太长: {} 字节,上限 {} 字节(macOS sun_path 限制 104 字节,Zenoh unixpipe 会派生 _uplink/_downlink FIFO,留 9 字节容差)",
                path_str.len(),
                UNIXPIPE_SOCKET_PATH_MAX_BYTES
            ),
        ));
    }

    Ok(candidate)
}

#[cfg(unix)]
pub(super) fn validate_unixpipe_component(field: &str, value: &str) -> io::Result<()> {
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

/// 把 base 路径格式化成 Zenoh 1.8.0 能识别的 locator 字符串。
///
/// 输出形如 `unixpipe/<base>`。Zenoh 会在 base 上派生 `<base>_uplink` 和 `<base>_downlink`。
#[cfg(unix)]
pub(super) fn unixpipe_locator(path: &Path) -> String {
    format!("{UNIXPIPE_LOCATOR_PREFIX}/{}", path.display())
}

/// Zenoh 1.8.0 派生出的两条 FIFO 路径。
#[cfg(unix)]
fn unixpipe_fifo_paths(base: &Path) -> [PathBuf; 2] {
    let base_str = base.as_os_str().to_owned();
    [
        PathBuf::from(format!("{}_uplink", base_str.to_string_lossy())),
        PathBuf::from(format!("{}_downlink", base_str.to_string_lossy())),
    ]
}

/// unixpipe base path 的跨进程 ownership guard。
///
/// guard 与 base path 放在同一目录,确保不同 daemon identity 只要解析到同一 FIFO,
/// 就会竞争同一把锁。进程退出后OS释放lock,新 daemon可接管并执行 stale cleanup。
#[cfg(unix)]
#[derive(Debug)]
pub struct UnixpipePathGuard {
    _lease: process_lease::ProcessLease,
}

/// 获取 base path ownership,随后清理崩溃残留 FIFO。
///
/// 返回的 guard 必须覆盖 Zenoh listener 生命周期。这样第二实例在 ownership 检查失败时,
/// 不会执行任何 destructive cleanup。
#[cfg(unix)]
pub fn prepare_unixpipe_listener(base: &Path) -> io::Result<UnixpipePathGuard> {
    let guard = acquire_unixpipe_path_guard(base)?;
    cleanup_stale_unixpipe_socket(base)?;
    Ok(guard)
}

#[cfg(unix)]
fn acquire_unixpipe_path_guard(base: &Path) -> io::Result<UnixpipePathGuard> {
    let mut guard_name = base.as_os_str().to_os_string();
    guard_name.push(".rdog-owner.pid");
    let guard_path = PathBuf::from(guard_name);
    let metadata_path = process_lease::metadata_path_for_lock(&guard_path);
    let mut lease = process_lease::ProcessLease::acquire(
        guard_path.clone(),
        metadata_path,
        "unixpipe-path",
        &base.to_string_lossy(),
    )
    .map_err(|err| {
        if err.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "unixpipe FIFO base 已被活跃 daemon 占用: base={}, owner_guard={}",
                    base.display(),
                    guard_path.display()
                ),
            )
        } else {
            err
        }
    })?;
    lease.publish_metadata()?;
    Ok(UnixpipePathGuard { _lease: lease })
}

/// 清理 stale FIFO 文件。
///
/// Zenoh 1.8.0 listener 在 `mkfifo` 失败 EEXIST 时会直接报错,不会自动清理。
/// 因此 daemon 重启前必须 unlink 任何残留的 `<base>` / `<base>_uplink` / `<base>_downlink`。
///
/// 返回 Ok(()) 即视为"路径已干净(本来就干净 或 已被本调用清理)"。
/// 文件存在但是是目录(不是 FIFO)才会返回错误,避免误删用户的目录。
#[cfg(unix)]
fn cleanup_stale_unixpipe_socket(base: &Path) -> io::Result<()> {
    let candidates: [PathBuf; 3] = [
        base.to_path_buf(),
        unixpipe_fifo_paths(base)[0].clone(),
        unixpipe_fifo_paths(base)[1].clone(),
    ];

    for candidate in &candidates {
        match fs::metadata(candidate) {
            Ok(metadata) if metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "unixpipe 路径 {} 是目录而不是 FIFO 文件,拒绝清理",
                        candidate.display()
                    ),
                ));
            }
            Ok(_) => {
                if let Err(err) = fs::remove_file(candidate) {
                    if err.kind() != io::ErrorKind::NotFound {
                        return Err(io::Error::new(
                            err.kind(),
                            format!(
                                "清理 stale unixpipe 文件 {} 失败: {err}",
                                candidate.display()
                            ),
                        ));
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                // 文件不存在,本来就是干净状态,跳过。
            }
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!(
                        "检查 unixpipe 路径 {} 元数据失败: {err}",
                        candidate.display()
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// 客户端探测:base 路径对应的 FIFO 是否有一个 reader 在监听。
///
/// 用 mpsc + 后台线程 + 超时模拟"短超时 connect",避免依赖 `libc` 拿到 `O_NONBLOCK`。
///
/// 返回:
/// - `Ok(())` 表示 reader 在线(FIFO 存在且 daemon 在监听)。
/// - `Err(NotFound)` 表示 FIFO 文件不存在,daemon 没在跑。
/// - `Err(TimedOut)` 表示 FIFO 存在但 200ms 内没看到 reader。
/// - 其他错误透传底层 `OpenOptions::open` 的失败原因。
#[cfg(unix)]
#[allow(dead_code)]
fn try_unixpipe_probe(base: &Path, timeout: Duration) -> io::Result<()> {
    let fifo_path = unixpipe_fifo_paths(base)[0].clone();

    if !fifo_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("unixpipe FIFO {} 不存在", fifo_path.display()),
        ));
    }

    let fifo_path_for_thread = fifo_path.clone();
    let (tx, rx) = mpsc::sync_channel::<io::Result<()>>(1);
    thread::spawn(move || {
        // 打开 FIFO 写端:如果 daemon 在监听(已 open `<base>_uplink` for read),
        // open 立即成功;否则阻塞,直到 timeout 触发 channel 关闭后线程被丢弃。
        let result = fs::OpenOptions::new()
            .write(true)
            .open(&fifo_path_for_thread)
            .map(|_| ());
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "unixpipe FIFO {} 探测超时({}ms 内没看到 reader)",
                fifo_path.display(),
                timeout.as_millis()
            ),
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
            io::ErrorKind::Other,
            "unixpipe 探测线程异常断开",
        )),
    }
}

/// daemon 最终使用的 listen endpoints 与唯一 unixpipe base path。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedListenEndpoints {
    pub listen_endpoints: Vec<String>,
    pub unixpipe_base_path: Option<PathBuf>,
}

/// 把 unixpipe endpoint 自动注入到 listen_endpoints 列表里。
///
/// 规则:
/// 1. `listen_endpoints` 里的显式 unixpipe endpoint 优先,并成为 cleanup/registry/guard 的路径真相源。
/// 2. 显式 endpoint 最多一条;它与 `unixpipe.socket_path` 同时存在时必须指向同一路径。
/// 3. 没有显式 endpoint 且 `unixpipe.enabled == false` 时,返回原列表且不声明 unixpipe base。
/// 4. 其余情况使用 `socket_path`,或按 `(namespace, daemon_name)` 自动推导并注入到列表最前。
#[cfg(unix)]
pub fn compose_listen_endpoints(
    config: &crate::config::ZenohConfig,
    namespace: &str,
    daemon_name: &str,
) -> io::Result<ComposedListenEndpoints> {
    let explicit_unixpipe_paths = config
        .listen_endpoints
        .iter()
        .filter_map(|endpoint| endpoint.strip_prefix("unixpipe/"))
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    if explicit_unixpipe_paths.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zenoh.listen_endpoints 最多只能包含一个 unixpipe endpoint",
        ));
    }

    if let Some(explicit_base) = explicit_unixpipe_paths.into_iter().next() {
        validate_composed_unixpipe_base_path(&explicit_base, "显式 unixpipe listen endpoint")?;
        if config
            .unixpipe
            .socket_path
            .as_ref()
            .is_some_and(|socket_path| socket_path != &explicit_base)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "zenoh.unixpipe.socket_path 与显式 unixpipe listen endpoint 不一致: socket_path={}, endpoint_base={}",
                    config
                        .unixpipe
                        .socket_path
                        .as_ref()
                        .expect("socket_path checked above")
                        .display(),
                    explicit_base.display()
                ),
            ));
        }
        return Ok(ComposedListenEndpoints {
            listen_endpoints: config.listen_endpoints.clone(),
            unixpipe_base_path: Some(explicit_base),
        });
    }

    if !config.unixpipe.enabled {
        return Ok(ComposedListenEndpoints {
            listen_endpoints: config.listen_endpoints.clone(),
            unixpipe_base_path: None,
        });
    }

    let base_path = match config.unixpipe.socket_path.as_ref() {
        Some(explicit) => explicit.clone(),
        None => unixpipe_socket_path(namespace, daemon_name)?,
    };
    validate_composed_unixpipe_base_path(&base_path, "unixpipe base path")?;

    let mut listen_endpoints = Vec::with_capacity(config.listen_endpoints.len() + 1);
    listen_endpoints.push(unixpipe_locator(&base_path));
    listen_endpoints.extend(config.listen_endpoints.iter().cloned());
    Ok(ComposedListenEndpoints {
        listen_endpoints,
        unixpipe_base_path: Some(base_path),
    })
}

#[cfg(unix)]
fn validate_composed_unixpipe_base_path(path: &Path, source: &str) -> io::Result<()> {
    let path_str = path.as_os_str();
    if path_str.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{source} 不能为空"),
        ));
    }
    if path_str.len() > UNIXPIPE_SOCKET_PATH_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{source} 太长: {} 字节,上限 {} 字节",
                path_str.len(),
                UNIXPIPE_SOCKET_PATH_MAX_BYTES
            ),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn compose_listen_endpoints(
    config: &crate::config::ZenohConfig,
    _namespace: &str,
    _daemon_name: &str,
) -> io::Result<ComposedListenEndpoints> {
    Ok(ComposedListenEndpoints {
        listen_endpoints: config.listen_endpoints.clone(),
        unixpipe_base_path: None,
    })
}

#[cfg(test)]
mod tests;
