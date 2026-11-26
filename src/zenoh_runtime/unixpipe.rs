//! Zenoh unixpipe路径、ownership、探测与listener composition。

use std::{io, path::PathBuf};

#[cfg(unix)]
use std::{fs, path::Path, sync::mpsc, thread, time::Duration};

#[cfg(unix)]
use crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES;

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
/// 目录、文件名与超长名字压缩的规则收敛在 `crate::local_run`,这里只取 base 路径:
/// - 目录: `$XDG_RUNTIME_DIR/rdog/` 否则 `~/.rdog/run/`(0700)
/// - 文件名: `<namespace>__<daemon_name>.pipe`,超 95 字节预算时压缩成 `<slug>-<8位哈希>.pipe`
/// - 扩展名 `.pipe` 表明这是 FIFO(named pipe),不是 Unix domain socket,避免后人误以为是 socket。
#[cfg(unix)]
pub(super) fn unixpipe_socket_path(namespace: &str, daemon_name: &str) -> io::Result<PathBuf> {
    Ok(crate::local_run::unixpipe_run_paths(namespace, daemon_name)?.base)
}

/// 路径组件校验的唯一定义在 `crate::local_run`,local-default registry 沿用同一套口径。
#[cfg(unix)]
pub(super) use crate::local_run::validate_unixpipe_component;

/// 把 base 路径格式化成 Zenoh 1.8.0 能识别的 locator 字符串。
///
/// 输出形如 `unixpipe/<base>`。Zenoh 会在 base 上派生 `<base>_uplink` 和 `<base>_downlink`。
#[cfg(unix)]
pub(super) fn unixpipe_locator(path: &Path) -> String {
    format!("{UNIXPIPE_LOCATOR_PREFIX}/{}", path.display())
}

/// Zenoh 1.8.0 派生出的两条 FIFO 路径(定义在 `crate::local_run`,与生命周期清理同源)。
#[cfg(unix)]
fn unixpipe_fifo_paths(base: &Path) -> [PathBuf; 2] {
    crate::local_run::fifo_paths(base)
}

/// unixpipe base path 的跨进程 ownership guard。
///
/// lock/meta/FIFO 的生命周期收敛在 `crate::local_run::UnixpipeRunGuard`:
/// 启动时持 `<stem>.lock` 的 flock、写 `<stem>.meta`,正常退出删 FIFO 与 lock/meta。
#[cfg(unix)]
pub type UnixpipePathGuard = crate::local_run::UnixpipeRunGuard;

/// 获取 base path ownership,随后清理崩溃残留 FIFO。
///
/// 返回的 guard 必须覆盖 Zenoh listener 生命周期。第二实例在 ownership 检查失败时
/// 不会执行任何 destructive cleanup;拿不到 lock 说明 owner 已死(哪怕是被 kill -9),
/// 才进入残留清理。
#[cfg(unix)]
pub fn prepare_unixpipe_listener(
    base: &Path,
    namespace: &str,
    daemon_name: &str,
    shared: bool,
) -> io::Result<UnixpipePathGuard> {
    let mut paths = crate::local_run::paths_for_base(base)?;
    // 来源由 `compose_listen_endpoints` 判定后传进来: 只有显式配置的 base
    // 才可能被旧二进制用同一路径撞上,因而需要旧格式兼容守卫。
    // 不能按路径值判定 —— 用户可能把 `rdog local list` 打印的路径抄进 `socket_path`。
    paths.shared = shared;
    crate::local_run::UnixpipeRunGuard::acquire(paths, namespace, daemon_name)
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
    /// base 是否来自用户显式配置(`socket_path` 或显式 `unixpipe/` listen endpoint)。
    ///
    /// 来源只能在这里判定 —— 只看路径值无法区分"显式配的"与"按身份推导的",
    /// 而用户完全可能把 `rdog local list` 打印的路径抄进 `socket_path`。
    pub unixpipe_base_explicit: bool,
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
            unixpipe_base_explicit: true,
        });
    }

    if !config.unixpipe.enabled {
        return Ok(ComposedListenEndpoints {
            listen_endpoints: config.listen_endpoints.clone(),
            unixpipe_base_path: None,
            unixpipe_base_explicit: false,
        });
    }

    let (base_path, explicit) = match config.unixpipe.socket_path.as_ref() {
        Some(explicit) => (explicit.clone(), true),
        None => (unixpipe_socket_path(namespace, daemon_name)?, false),
    };
    validate_composed_unixpipe_base_path(&base_path, "unixpipe base path")?;

    let mut listen_endpoints = Vec::with_capacity(config.listen_endpoints.len() + 1);
    listen_endpoints.push(unixpipe_locator(&base_path));
    listen_endpoints.extend(config.listen_endpoints.iter().cloned());
    Ok(ComposedListenEndpoints {
        listen_endpoints,
        unixpipe_base_path: Some(base_path),
        unixpipe_base_explicit: explicit,
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
        unixpipe_base_explicit: false,
    })
}

#[cfg(test)]
mod tests;
