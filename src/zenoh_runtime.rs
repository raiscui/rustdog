use std::{
    fs,
    io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use zenoh::{config::WhatAmI, scouting::Hello, Config, Session, Wait};

use crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES;

pub fn open_router_session(listen_endpoints: &[String]) -> io::Result<Session> {
    open_session("router", &[], listen_endpoints)
}

pub fn open_client_session(connect_endpoints: &[String]) -> io::Result<Session> {
    open_session("client", connect_endpoints, &[])
}

pub fn resolve_client_connect_endpoints(
    connect_endpoints: &[String],
    discovery_timeout: Duration,
    unixpipe_probe: UnixpipeClientProbe<'_>,
) -> io::Result<Vec<String>> {
    if !connect_endpoints.is_empty() {
        // 用户显式给了 entry-point,尊重用户选择,不再尝试 unixpipe fast path。
        // 防止"显式给 udp/远端-host"却被本机 unixpipe 误抢先"的混淆。
        return Ok(connect_endpoints.to_vec());
    }

    // 本机 fast path:同机 daemon 通常会开 unixpipe endpoint。
    //
    // 实现要点:不能主动 open FIFO 探活(那会让 daemon 的 request channel 看到 EOF
    // 并破坏后续 client),只能用 `Path::exists` 做"FIFO 文件在不在"的轻量检查。
    // 如果 FIFO 在,直接把 unixpipe locator 作为唯一 connect endpoint 交给 zenoh::open。
    // zenoh::open 内部如果 unixpipe 不可达,会返回 Err,调用方在 `open_client_session`
    // 那一步会拿到错误并决定如何 fallback。
    #[cfg(unix)]
    {
        if let Some((namespace, target_name)) = unixpipe_probe.namespace.zip(unixpipe_probe.target_name) {
            if let Ok(base_path) = unixpipe_socket_path(namespace, target_name) {
                if unixpipe_base_path_alive(&base_path) {
                    log::info!(
                        "unixpipe endpoint detected, taking fast path (path: {})",
                        base_path.display()
                    );
                    return Ok(vec![unixpipe_locator(&base_path)]);
                }
            }
        }
    }

    autodiscover_router_endpoints(discovery_timeout)
}

/// 轻量检查 unixpipe base 路径对应的 FIFO 文件是否存在。
///
/// 注意:这里只做 `Path::exists`,**不**主动 open FIFO 探活。
/// 原因: 主动 open FIFO 写端后会立即关闭,daemon 端的 request channel 会看到 EOF
/// 并影响后续 client 的正常 connect 流程。我们只关心"daemon 留没留这个文件",
/// 真正的连接性由 zenoh::open 内部处理。
#[cfg(unix)]
fn unixpipe_base_path_alive(base: &Path) -> bool {
    let uplinks = unixpipe_fifo_paths(base);
    uplinks.iter().any(|path| path.exists())
}

/// 客户端 unixpipe fast path 提示。
///
/// 当 `namespace` 和 `target_name` 都是 `Some` 时,会触发 unixpipe 存在性检查并
/// 把对应的 locator 作为唯一 connect endpoint 返回,跳过 UDP scout。
/// `None` 任意一个都走老 autodiscover 路径(用于 `rdog control` 没指定 target 的场景)。
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixpipeClientProbe<'a> {
    pub namespace: Option<&'a str>,
    pub target_name: Option<&'a str>,
}

impl<'a> UnixpipeClientProbe<'a> {
    pub fn new(namespace: Option<&'a str>, target_name: Option<&'a str>) -> Self {
        Self {
            namespace,
            target_name,
        }
    }
}

fn open_session(
    mode: &str,
    connect_endpoints: &[String],
    listen_endpoints: &[String],
) -> io::Result<Session> {
    let mut config = Config::default();
    config
        .insert_json5("mode", &format!("\"{mode}\""))
        .map_err(to_io_error)?;

    if !connect_endpoints.is_empty() {
        let value = json_string_list(connect_endpoints);
        config
            .insert_json5("connect/endpoints", &value)
            .map_err(to_io_error)?;
    }

    if !listen_endpoints.is_empty() {
        let value = json_string_list(listen_endpoints);
        config
            .insert_json5("listen/endpoints", &value)
            .map_err(to_io_error)?;
    }

    zenoh::open(config)
        .wait()
        .map_err(|err| to_open_session_error(err, listen_endpoints))
}

fn autodiscover_router_endpoints(discovery_timeout: Duration) -> io::Result<Vec<String>> {
    // ------------------------------------------------------------
    // 这里不再把“发现 router”与“按 Hello 原始顺序逐个连接 locator”
    // 完全交给 zenoh::open() 内部处理。
    //
    // 原因是 Windows 多网卡现场里,Hello 可能先列出多个 169.254.* 死地址,
    // 3 秒 scouting 窗口会先被这些慢连接耗尽,还没轮到真正可达的 LAN IP。
    // 因此我们先自己 scout 一次,把 locator 排序后再显式 open。
    // ------------------------------------------------------------
    let mut config = Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .map_err(to_io_error)?;

    let scout = zenoh::scout(WhatAmI::Router, config)
        .wait()
        .map_err(to_io_error)?;
    let deadline = Instant::now() + discovery_timeout;

    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "Zenoh autodiscovery 在 {}ms 内未找到可连接的 router locator",
                        discovery_timeout.as_millis()
                    ),
                )
            })?;

        match scout.recv_timeout(remaining) {
            Ok(Some(hello)) => {
                let endpoints = prioritize_hello_locators(&hello);
                if !endpoints.is_empty() {
                    return Ok(endpoints);
                }
            }
            Ok(None) => continue,
            Err(err) => {
                let kind = if Instant::now() >= deadline {
                    io::ErrorKind::TimedOut
                } else {
                    io::ErrorKind::Other
                };
                let message = if kind == io::ErrorKind::TimedOut {
                    format!(
                        "Zenoh autodiscovery 在 {}ms 内未找到可连接的 router locator",
                        discovery_timeout.as_millis()
                    )
                } else {
                    format!("Zenoh autodiscovery scout 提前结束: {err}")
                };
                return Err(io::Error::new(kind, message));
            }
        }
    }
}

fn prioritize_hello_locators(hello: &Hello) -> Vec<String> {
    let mut locators = hello
        .locators()
        .iter()
        .map(ToString::to_string)
        .filter(|locator| !is_serial_locator(locator))
        .collect::<Vec<_>>();

    locators.sort_by(|left, right| {
        locator_sort_key(left)
            .cmp(&locator_sort_key(right))
            .then_with(|| left.cmp(right))
    });
    locators.dedup();
    locators
}

fn locator_sort_key(locator: &str) -> (u8, &str) {
    match parse_locator_socket_addr(locator) {
        Some(addr) if addr.ip().is_loopback() => (0, locator),
        Some(addr) if is_link_local_ip(addr.ip()) => (2, locator),
        Some(_) => (1, locator),
        None => (3, locator),
    }
}

fn parse_locator_socket_addr(locator: &str) -> Option<SocketAddr> {
    let (_, address) = locator.split_once('/')?;
    let address = address.split(['#', '?']).next()?;
    SocketAddr::from_str(address).ok()
}

fn is_serial_locator(locator: &str) -> bool {
    locator
        .split_once('/')
        .map(|(scheme, _)| scheme.eq_ignore_ascii_case("serial"))
        .unwrap_or(false)
}

fn is_link_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 169 && octets[1] == 254
        }
        IpAddr::V6(v6) => v6.is_unicast_link_local(),
    }
}

fn json_string_list(values: &[String]) -> String {
    let joined = values
        .iter()
        .map(|value| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{joined}]")
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

fn to_open_session_error(err: impl std::fmt::Display, listen_endpoints: &[String]) -> io::Error {
    let message = err.to_string();

    if looks_like_windows_listen_access_denied(&message) && !listen_endpoints.is_empty() {
        let endpoints = listen_endpoints.join(", ");
        return io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{message}. 当前 Windows 现场对 listen_endpoints={endpoints} 的绑定被拒绝。请优先改用具体网卡 IP + 高位端口,例如 tcp/192.168.50.57:17447,不要先用 tcp/0.0.0.0:7447。"
            ),
        );
    }

    io::Error::other(message)
}

fn looks_like_windows_listen_access_denied(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("access is denied") || lower.contains("拒绝访问") || lower.contains("os error 5")
}

// ============================================================================
// unixpipe (named pipe / FIFO) 本机 fast path 辅助函数
//
// Zenoh 1.8.0 的 `transport_unixpipe` 实际是 named pipe (FIFO),不是 Unix domain socket。
// 它会从 base 路径派生 `<base>_uplink` / `<base>_downlink` 两条 FIFO 文件。
// macOS `sun_path` 限制 104 字节,base 必须 ≤ 95 字节(< 104 - len("_downlink"))。
//
// 性能收益:避免 UDP loopback 上的 Zenoh link 协议栈开销,本机 round-trip 期望 2~5x 提速。
// ============================================================================

/// Zenoh 1.8.0 unixpipe 用的 locator 前缀。
pub(crate) const UNIXPIPE_LOCATOR_PREFIX: &str = "unixpipe";

/// 根据 (namespace, daemon_name) 推导 base 路径,daemon 和 control 端用同一份规则。
///
/// 路径模板: `{tmpdir}/rdog-{namespace}-{daemon_name}.pipe`
/// - `tmpdir` 优先级: `$TMPDIR` > `/tmp`。
/// - macOS 的 `$TMPDIR` 是 per-user(例如 `/var/folders/xx/yy/T/`),自然有权限隔离。
/// - 扩展名 `.pipe` 表明这是 FIFO(named pipe),不是 Unix domain socket,避免后人误以为是 socket。
#[cfg(unix)]
pub fn unixpipe_socket_path(namespace: &str, daemon_name: &str) -> io::Result<PathBuf> {
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
fn validate_unixpipe_component(field: &str, value: &str) -> io::Result<()> {
    if value.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unixpipe 路径组件 `{field}` 不能为空"),
        ));
    }
    if value.contains('/') || value.contains(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unixpipe 路径组件 `{field}` 不能包含 `/` 或空白字符(实际: {value:?})"
            ),
        ));
    }
    Ok(())
}

/// 把 base 路径格式化成 Zenoh 1.8.0 能识别的 locator 字符串。
///
/// 输出形如 `unixpipe/<base>`。Zenoh 会在 base 上派生 `<base>_uplink` 和 `<base>_downlink`。
#[cfg(unix)]
pub fn unixpipe_locator(path: &Path) -> String {
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

/// 清理 stale FIFO 文件。
///
/// Zenoh 1.8.0 listener 在 `mkfifo` 失败 EEXIST 时会直接报错,不会自动清理。
/// 因此 daemon 重启前必须 unlink 任何残留的 `<base>` / `<base>_uplink` / `<base>_downlink`。
///
/// 返回 Ok(()) 即视为"路径已干净(本来就干净 或 已被本调用清理)"。
/// 文件存在但是是目录(不是 FIFO)才会返回错误,避免误删用户的目录。
#[cfg(unix)]
pub fn cleanup_stale_unixpipe_socket(base: &Path) -> io::Result<()> {
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
                            format!("清理 stale unixpipe 文件 {} 失败: {err}", candidate.display()),
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
                    format!("检查 unixpipe 路径 {} 元数据失败: {err}", candidate.display()),
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
pub fn try_unixpipe_probe(base: &Path, timeout: Duration) -> io::Result<()> {
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

/// 把 unixpipe endpoint 自动注入到 listen_endpoints 列表里。
///
/// 规则:
/// 1. 如果 `config.zenoh.unixpipe.enabled == false`,返回原列表不动。
/// 2. 如果 `config.zenoh.unixpipe.socket_path` 是 None,用 `(namespace, daemon_name)` 自动推导。
/// 3. 如果推导或显式给的路径超过 `UNIXPIPE_SOCKET_PATH_MAX_BYTES`,返回 InvalidInput 错误。
/// 4. 如果 listen_endpoints 列表里已经包含 `unixpipe/...`,跳过注入(用户显式控制时优先)。
/// 5. 注入的 endpoint 放在最前,这样即使同机 fast path 不可达,后续 UDP 也能 fallback。
#[cfg(unix)]
pub fn compose_listen_endpoints(
    config: &crate::config::ZenohConfig,
    namespace: &str,
    daemon_name: &str,
) -> io::Result<Vec<String>> {
    if !config.unixpipe.enabled {
        return Ok(config.listen_endpoints.clone());
    }

    let base_path = match config.unixpipe.socket_path.as_ref() {
        Some(explicit) => {
            let explicit_str = explicit.as_os_str();
            if explicit_str.len() > UNIXPIPE_SOCKET_PATH_MAX_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "显式 unixpipe.socket_path 太长: {} 字节,上限 {} 字节",
                        explicit_str.len(),
                        UNIXPIPE_SOCKET_PATH_MAX_BYTES
                    ),
                ));
            }
            explicit.clone()
        }
        None => unixpipe_socket_path(namespace, daemon_name)?,
    };

    let locator = unixpipe_locator(&base_path);

    if config
        .listen_endpoints
        .iter()
        .any(|endpoint| endpoint.starts_with(UNIXPIPE_LOCATOR_PREFIX))
    {
        // 用户在 listen_endpoints 里显式给了 unixpipe,尊重用户选择,不再注入。
        return Ok(config.listen_endpoints.clone());
    }

    let mut composed = Vec::with_capacity(config.listen_endpoints.len() + 1);
    composed.push(locator);
    composed.extend(config.listen_endpoints.iter().cloned());
    Ok(composed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // -----------------------------------------------------------------
    // unixpipe path derivation / cleanup / probe / compose_listen
    // -----------------------------------------------------------------

    #[test]
    fn unixpipe_socket_path_should_respect_tmpdir_env() {
        // 临时覆盖 TMPDIR,确认派生路径使用它。
        let prev = env::var_os("TMPDIR");
        // SAFETY: 在测试里改环境变量是常见模式,后续立即恢复。
        unsafe { env::set_var("TMPDIR", "/tmp/rdog-tmpdir-test") };
        let result = unixpipe_socket_path("lab", "mac.lab");
        match prev {
            Some(v) => unsafe { env::set_var("TMPDIR", v) },
            None => unsafe { env::remove_var("TMPDIR") },
        }
        let path = result.expect("路径推导应该成功");
        assert_eq!(path, PathBuf::from("/tmp/rdog-tmpdir-test/rdog-lab-mac.lab.pipe"));
    }

    #[test]
    fn unixpipe_socket_path_should_fallback_to_slash_tmp_when_tmpdir_unset() {
        let prev = env::var_os("TMPDIR");
        unsafe { env::remove_var("TMPDIR") };
        let result = unixpipe_socket_path("lab", "mac.lab");
        match prev {
            Some(v) => unsafe { env::set_var("TMPDIR", v) },
            None => unsafe { env::remove_var("TMPDIR") },
        }
        let path = result.expect("fallback 应该成功");
        assert_eq!(path, PathBuf::from("/tmp/rdog-lab-mac.lab.pipe"));
    }

    #[test]
    fn unixpipe_socket_path_should_reject_components_with_slash_or_whitespace() {
        assert!(unixpipe_socket_path("la/b", "mac.lab").is_err());
        assert!(unixpipe_socket_path("lab", "mac lab").is_err());
        assert!(unixpipe_socket_path("", "mac.lab").is_err());
        assert!(unixpipe_socket_path("lab", "").is_err());
    }

    #[test]
    fn unixpipe_socket_path_should_reject_oversized_combination() {
        // 92 字节的 namespace + "mac.lab" 组合会让最终路径超过 95 字节上限。
        let big_ns: String = std::iter::repeat('a').take(92).collect();
        let err = unixpipe_socket_path(&big_ns, "mac.lab").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("unixpipe base 路径太长"));
    }

    #[test]
    fn unixpipe_locator_should_format_as_protocol_prefix_and_path() {
        let path = PathBuf::from("/tmp/rdog-lab-mac.lab.pipe");
        assert_eq!(
            unixpipe_locator(&path),
            "unixpipe//tmp/rdog-lab-mac.lab.pipe"
        );
    }

    #[test]
    fn cleanup_stale_unixpipe_socket_should_remove_existing_pipe_files() {
        // 模拟 daemon 崩溃后残留的 3 个文件。
        let base = PathBuf::from("/tmp/rdog-cleanup-test.pipe");
        let _ = fs::remove_file(&base);
        let _ = fs::remove_file(format!("{}_uplink", base.display()));
        let _ = fs::remove_file(format!("{}_downlink", base.display()));

        for suffix in ["", "_uplink", "_downlink"] {
            let path = format!("/tmp/rdog-cleanup-test.pipe{suffix}");
            let status = std::process::Command::new("mkfifo")
                .arg(&path)
                .status()
                .expect("mkfifo 调用应该成功");
            assert!(status.success(), "mkfifo 应该成功");
        }

        cleanup_stale_unixpipe_socket(&base).expect("清理应该成功");

        for suffix in ["", "_uplink", "_downlink"] {
            let path = format!("/tmp/rdog-cleanup-test.pipe{suffix}");
            assert!(!Path::new(&path).exists(), "{path} 应该已被清理");
        }
    }

    #[test]
    fn cleanup_stale_unixpipe_socket_should_succeed_when_files_missing() {
        let base = PathBuf::from("/tmp/rdog-cleanup-missing.pipe");
        let _ = fs::remove_file(&base);
        cleanup_stale_unixpipe_socket(&base).expect("文件不存在时必须能直接通过");
    }

    #[test]
    fn cleanup_stale_unixpipe_socket_should_reject_when_path_is_directory() {
        // 如果路径是目录而不是 FIFO 文件,必须报错避免误删用户目录。
        let base = PathBuf::from("/tmp/rdog-cleanup-dir-test.pipe");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("create_dir_all 应该成功");

        let err = cleanup_stale_unixpipe_socket(&base).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn try_unixpipe_probe_should_return_not_found_when_fifo_missing() {
        let base = PathBuf::from("/tmp/rdog-probe-missing.pipe");
        let _ = fs::remove_file(&base);
        let _ = fs::remove_file(format!("{}_uplink", base.display()));
        let _ = fs::remove_file(format!("{}_downlink", base.display()));

        let err = try_unixpipe_probe(&base, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn try_unixpipe_probe_should_return_timeout_when_fifo_exists_without_reader() {
        // 创建 FIFO 但不打开读端,probe 必须在 timeout 内返回 TimedOut。
        let base = PathBuf::from("/tmp/rdog-probe-no-reader.pipe");
        let _ = fs::remove_file(&base);
        let _ = fs::remove_file(format!("{}_uplink", base.display()));
        let uplink = format!("{}_uplink", base.display());
        let status = std::process::Command::new("mkfifo")
            .arg(&uplink)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success(), "mkfifo 应该成功");

        let start = Instant::now();
        let err = try_unixpipe_probe(&base, Duration::from_millis(150)).unwrap_err();
        let elapsed = start.elapsed();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(elapsed >= Duration::from_millis(140), "应该在 timeout 之后返回");

        let _ = fs::remove_file(&uplink);
    }

    #[test]
    fn try_unixpipe_probe_should_succeed_when_reader_is_alive() {
        // 创建 FIFO,后台开读端,然后 probe 必须成功。
        let base = PathBuf::from("/tmp/rdog-probe-with-reader.pipe");
        let _ = fs::remove_file(&base);
        let _ = fs::remove_file(format!("{}_uplink", base.display()));
        let uplink = format!("{}_uplink", base.display());
        let status = std::process::Command::new("mkfifo")
            .arg(&uplink)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success(), "mkfifo 应该成功");

        // 后台持有读端,模拟 daemon 在监听。
        let uplink_clone = uplink.clone();
        let _reader = thread::spawn(move || {
            let _f = fs::OpenOptions::new()
                .read(true)
                .open(&uplink_clone)
                .expect("reader 应该能开");
            thread::sleep(Duration::from_millis(500));
        });

        // 给 reader 一点时间起来。
        thread::sleep(Duration::from_millis(50));

        let result = try_unixpipe_probe(&base, Duration::from_millis(500));
        let _ = fs::remove_file(&uplink);
        result.expect("有 reader 时 probe 应该成功");
    }

    #[test]
    fn compose_listen_endpoints_should_inject_unixpipe_when_enabled_and_not_present() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.unixpipe.socket_path = None;
        cfg.listen_endpoints = vec!["udp/0.0.0.0:7447".to_string()];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        assert_eq!(composed.len(), 2);
        assert!(composed[0].starts_with("unixpipe/"));
        assert!(composed[0].contains("rdog-lab-mac.lab.pipe"));
        assert_eq!(composed[1], "udp/0.0.0.0:7447");
    }

    #[test]
    fn compose_listen_endpoints_should_not_inject_when_disabled() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = false;
        cfg.listen_endpoints = vec!["udp/0.0.0.0:7447".to_string()];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        assert_eq!(composed, vec!["udp/0.0.0.0:7447".to_string()]);
    }

    #[test]
    fn compose_listen_endpoints_should_not_override_explicit_unixpipe() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.unixpipe.socket_path = None;
        cfg.listen_endpoints = vec![
            "unixpipe//tmp/explicit.pipe".to_string(),
            "udp/0.0.0.0:7447".to_string(),
        ];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        // 用户的显式 unixpipe 必须保留,不能被自动推导覆盖。
        assert_eq!(
            composed,
            vec![
                "unixpipe//tmp/explicit.pipe".to_string(),
                "udp/0.0.0.0:7447".to_string(),
            ]
        );
    }

    #[test]
    fn compose_listen_endpoints_should_use_explicit_socket_path() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.unixpipe.socket_path = Some(PathBuf::from("/tmp/explicit-socket.pipe"));
        cfg.listen_endpoints = vec!["udp/0.0.0.0:7447".to_string()];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        assert_eq!(composed[0], "unixpipe//tmp/explicit-socket.pipe");
        assert_eq!(composed[1], "udp/0.0.0.0:7447");
    }

    // -----------------------------------------------------------------
    // 已有单测
    // -----------------------------------------------------------------

    #[test]
    fn open_session_error_should_upgrade_windows_listen_access_denied() {
        let err = to_open_session_error(
            "拒绝访问。 (os error 5)",
            &[String::from("tcp/0.0.0.0:7447")],
        );

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("192.168.50.57:17447"));
        assert!(err.to_string().contains("0.0.0.0:7447"));
    }

    #[test]
    fn locator_priority_should_prefer_preferred_tcp_over_link_local_and_serial() {
        let ordered = vec![
            "serial/COM3#baudrate=115200".to_string(),
            "tcp/169.254.105.229:7447".to_string(),
            "tcp/192.168.50.57:7447".to_string(),
            "tcp/127.0.0.1:7447".to_string(),
        ];

        let mut ordered = ordered;
        ordered.sort_by(|left, right| {
            locator_sort_key(left)
                .cmp(&locator_sort_key(right))
                .then_with(|| left.cmp(right))
        });

        assert_eq!(ordered[0], "tcp/127.0.0.1:7447");
        assert_eq!(ordered[1], "tcp/192.168.50.57:7447");
        assert_eq!(ordered[2], "tcp/169.254.105.229:7447");
        assert_eq!(ordered[3], "serial/COM3#baudrate=115200");
    }

    #[test]
    fn parse_locator_socket_addr_should_ignore_metadata_suffix() {
        let addr =
            parse_locator_socket_addr("tcp/192.168.50.57:7447#so_sndbuf=65000").expect("addr");

        assert_eq!(addr, SocketAddr::from_str("192.168.50.57:7447").unwrap());
    }
}
