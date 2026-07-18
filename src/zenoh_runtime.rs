use std::{
    fs, io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use zenoh::{config::WhatAmI, scouting::Hello, Config, Session, Wait};

use crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES;

#[cfg(unix)]
pub(crate) mod process_lease;

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
        if let Some((namespace, target_name)) =
            unixpipe_probe.namespace.zip(unixpipe_probe.target_name)
        {
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
#[cfg(unix)]
const LOCAL_DEFAULT_SCHEMA: &str = "rdog.local-default.v1";
#[cfg(unix)]
const LOCAL_DEFAULT_STARTUP_GRACE_MS: u128 = 10_000;

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
            format!("unixpipe 路径组件 `{field}` 不能包含 `/` 或空白字符(实际: {value:?})"),
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

#[cfg(unix)]
#[derive(Debug)]
pub struct LocalDefaultDaemonGuard {
    _lease: process_lease::ProcessLease,
}

#[cfg(unix)]
#[derive(Debug, Clone, Deserialize, Serialize)]
struct LocalDefaultDaemonRecord {
    schema: String,
    namespace: String,
    daemon_name: String,
    pid: u32,
    unixpipe_base: PathBuf,
    created_at_unix_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_resource_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_resource_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lease_created_at_unix_ms: Option<u128>,
}

#[cfg(unix)]
pub fn register_local_default_daemon(
    namespace: &str,
    daemon_name: &str,
    unixpipe_base: &Path,
) -> io::Result<LocalDefaultDaemonGuard> {
    validate_unixpipe_component("namespace", namespace)?;
    validate_unixpipe_component("daemon_name", daemon_name)?;

    let dir = local_default_daemon_dir()?;
    fs::create_dir_all(&dir)?;
    let record_path = local_default_daemon_record_path(namespace)?;
    let guard_path = local_default_daemon_guard_path(namespace)?;
    let metadata_path = process_lease::metadata_path_for_lock(&guard_path);
    let mut lease = process_lease::ProcessLease::acquire(
        guard_path.clone(),
        metadata_path,
        "local-default",
        namespace,
    )
    .map_err(|err| {
        if err.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "本机默认 daemon 已存在: namespace={namespace}, local_default_guard={}",
                    guard_path.display()
                ),
            )
        } else {
            err
        }
    })?;
    lease.publish_metadata()?;
    let lease_metadata = lease.metadata();

    let record = LocalDefaultDaemonRecord {
        schema: LOCAL_DEFAULT_SCHEMA.to_string(),
        namespace: namespace.to_string(),
        daemon_name: daemon_name.to_string(),
        pid: lease_metadata.pid,
        unixpipe_base: unixpipe_base.to_path_buf(),
        created_at_unix_ms: unix_timestamp_ms(),
        lease_schema: Some(lease_metadata.lease_schema.clone()),
        lease_id: Some(lease_metadata.lease_id.clone()),
        lease_resource_kind: Some(lease_metadata.lease_resource_kind.clone()),
        lease_resource_key: Some(lease_metadata.lease_resource_key.clone()),
        lease_created_at_unix_ms: Some(lease_metadata.lease_created_at_unix_ms),
    };
    write_local_default_daemon_record(&record_path, &record)?;

    Ok(LocalDefaultDaemonGuard { _lease: lease })
}

#[cfg(unix)]
fn find_valid_local_default_daemons(
    namespace_filter: Option<&str>,
) -> io::Result<Vec<LocalDefaultDaemonRecord>> {
    let dir = local_default_daemon_dir()?;
    let mut records = Vec::new();
    if let Some(namespace) = namespace_filter {
        let record_path = local_default_daemon_record_path(namespace)?;
        if let Some(record) = load_valid_local_default_record(&record_path, namespace_filter)? {
            records.push(record);
        }
        return Ok(records);
    }

    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(records),
        Err(err) => {
            return Err(io::Error::new(
                err.kind(),
                format!(
                    "扫描本机默认 daemon registry 目录 {} 失败: {err}",
                    dir.display()
                ),
            ))
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if let Some(record) = load_valid_local_default_record(&path, None)? {
            records.push(record);
        }
    }
    records.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then(left.daemon_name.cmp(&right.daemon_name))
    });
    records.dedup_by(|left, right| {
        left.namespace == right.namespace && left.daemon_name == right.daemon_name
    });
    Ok(records)
}

#[cfg(unix)]
fn load_valid_local_default_record(
    record_path: &Path,
    namespace_filter: Option<&str>,
) -> io::Result<Option<LocalDefaultDaemonRecord>> {
    let record = match read_local_default_daemon_record(record_path) {
        Ok(record) => record,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Ok(None),
    };

    if record.is_valid_for(namespace_filter)? {
        return Ok(Some(record));
    }

    if record.should_keep_during_startup(namespace_filter)? {
        return Ok(None);
    }

    Ok(None)
}

#[cfg(unix)]
impl LocalDefaultDaemonRecord {
    fn is_valid_for(&self, namespace_filter: Option<&str>) -> io::Result<bool> {
        if !self.identity_is_valid(namespace_filter) || !self.owner_is_active()? {
            return Ok(false);
        }
        Ok(unixpipe_base_path_alive(&self.unixpipe_base))
    }

    fn should_keep_during_startup(&self, namespace_filter: Option<&str>) -> io::Result<bool> {
        if !self.identity_is_valid(namespace_filter) || !self.owner_is_active()? {
            return Ok(false);
        }
        if unixpipe_base_path_alive(&self.unixpipe_base) {
            return Ok(false);
        }

        Ok(unix_timestamp_ms().saturating_sub(self.created_at_unix_ms)
            <= LOCAL_DEFAULT_STARTUP_GRACE_MS)
    }

    fn identity_is_valid(&self, namespace_filter: Option<&str>) -> bool {
        self.schema == LOCAL_DEFAULT_SCHEMA
            && !namespace_filter.is_some_and(|namespace| namespace != self.namespace)
            && validate_unixpipe_component("namespace", &self.namespace).is_ok()
            && validate_unixpipe_component("daemon_name", &self.daemon_name).is_ok()
    }

    fn owner_is_active(&self) -> io::Result<bool> {
        let Some(metadata) = self.lease_metadata() else {
            // client只接受完整managed lease作为运行态owner证据。
            // 纯v1 PID记录和部分managed记录都只属于升级输入,不能用于正常发现。
            return Ok(false);
        };

        let guard_path = local_default_daemon_guard_path(&self.namespace)?;
        process_lease::managed_lease_is_active(&guard_path, &metadata)
    }

    fn lease_metadata(&self) -> Option<process_lease::LeaseMetadata> {
        let metadata = process_lease::LeaseMetadata {
            lease_schema: self.lease_schema.clone()?,
            lease_id: self.lease_id.clone()?,
            lease_resource_kind: self.lease_resource_kind.clone()?,
            lease_resource_key: self.lease_resource_key.clone()?,
            lease_created_at_unix_ms: self.lease_created_at_unix_ms?,
            pid: self.pid,
        };
        (metadata.lease_schema == process_lease::PROCESS_LEASE_SCHEMA
            && !metadata.lease_id.is_empty()
            && metadata.lease_resource_kind == "local-default"
            && metadata.lease_resource_key == self.namespace)
            .then_some(metadata)
    }
}

#[cfg(unix)]
fn read_local_default_daemon_record(path: &Path) -> io::Result<LocalDefaultDaemonRecord> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "解析本机默认 daemon registry {} 失败: {err}",
                path.display()
            ),
        )
    })
}

#[cfg(unix)]
fn write_local_default_daemon_record(
    path: &Path,
    record: &LocalDefaultDaemonRecord,
) -> io::Result<()> {
    process_lease::write_json_atomically(path, record)
}

#[cfg(unix)]
fn local_default_daemon_dir() -> io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = local_default_daemon_test_dir() {
        return Ok(dir);
    }

    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home)
            .join("rustdog")
            .join("local-default"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("rustdog")
            .join("local-default"));
    }

    Ok(std::env::temp_dir().join("rustdog").join("local-default"))
}

#[cfg(all(test, unix))]
thread_local! {
    static LOCAL_DEFAULT_DAEMON_TEST_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
fn set_local_default_daemon_test_dir(path: Option<PathBuf>) {
    LOCAL_DEFAULT_DAEMON_TEST_DIR.with(|slot| {
        *slot.borrow_mut() = path;
    });
}

#[cfg(all(test, unix))]
fn local_default_daemon_test_dir() -> Option<PathBuf> {
    LOCAL_DEFAULT_DAEMON_TEST_DIR.with(|slot| slot.borrow().clone())
}

#[cfg(unix)]
fn local_default_daemon_record_path(namespace: &str) -> io::Result<PathBuf> {
    validate_unixpipe_component("namespace", namespace)?;
    Ok(local_default_daemon_dir()?.join(format!("{namespace}.json")))
}

#[cfg(unix)]
fn local_default_daemon_guard_path(namespace: &str) -> io::Result<PathBuf> {
    validate_unixpipe_component("namespace", namespace)?;
    Ok(local_default_daemon_dir()?.join(format!("{namespace}.pid")))
}

#[cfg(unix)]
fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
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

/// 为`self`/空target解析唯一的managed local-default daemon。
///
/// active managed registry是owner身份的唯一真相源。registry不可用时仍扫描
/// `$TMPDIR`(或`/tmp`fallback)下的FIFO,但只用于生成升级诊断,不再自动选择daemon。
///
/// 关键实现细节:
/// - Zenoh 1.8.0 `transport_unixpipe` listener 实际只创建 `<base>_uplink` 和 `<base>_downlink`
///   两个 FIFO 文件,`<base>`(=`rdog-{ns}-{name}.pipe`)本身不一定存在。
/// - 因此扫描对象是 `*.pipe_uplink`,不是 `*.pipe`。同名 daemon 的 `<base>_downlink`
///   也存在,但只看 `_uplink` 就足够,避免双倍计数。
/// - 候选 base 路径必须以 `rdog-` 开头,中间段 `{ns}-{name}` 用第一个 `-` 切分。
/// - 任何FIFO候选都不能代替managed registry;显式target仍可用于升级排障。
#[cfg(unix)]
pub fn find_local_daemon_name(namespace_filter: Option<&str>) -> io::Result<String> {
    let local_defaults = find_valid_local_default_daemons(namespace_filter)?;
    match local_defaults.len() {
        0 => {}
        1 => return Ok(local_defaults[0].daemon_name.clone()),
        _ => {
            let instances = local_defaults
                .iter()
                .map(|record| {
                    format!(
                        "`{}`/`{}`(pid={})",
                        record.namespace, record.daemon_name, record.pid
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "本机发现多个 local-default daemon registry: [{instances}];请使用 `--namespace` 或显式 target name"
                ),
            ));
        }
    }

    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let prefix = "rdog-";
    let uplink_suffix = ".pipe_uplink";

    let entries = match fs::read_dir(&tmpdir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(no_local_daemon_error(namespace_filter));
        }
        Err(err) => {
            return Err(io::Error::new(
                err.kind(),
                format!("扫描 {tmpdir:?} 失败: {err}"),
            ));
        }
    };

    let mut candidates: Vec<String> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_name = match entry.file_name().to_str() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // 只看 `<base>_uplink` 文件,base = `rdog-{ns}-{name}.pipe`
        if !file_name.starts_with(prefix) || !file_name.ends_with(uplink_suffix) {
            continue;
        }

        // 中间段 = "{ns}-{name}",找第一个 `-` 作为分隔
        let middle = &file_name[prefix.len()..file_name.len() - uplink_suffix.len()];
        let Some(dash_idx) = middle.find('-') else {
            continue;
        };
        let ns = &middle[..dash_idx];
        let name = &middle[dash_idx + 1..];
        if ns.is_empty() || name.is_empty() {
            continue;
        }

        // namespace 过滤
        if let Some(filter) = namespace_filter {
            if ns != filter {
                continue;
            }
        }

        candidates.push(name.to_string());
    }

    candidates.sort();
    candidates.dedup();

    match candidates.len() {
        0 => Err(no_local_daemon_error(namespace_filter)),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "本机没有可用的active managed local-default registry;检测到未托管的 unixpipe FIFO 候选: [{}],但FIFO自动选择已退役;请显式指定 target name(例如 `rdog control <name> @<line>`),或在 daemon 配置中设置 `[zenoh.unixpipe] local_default = true`",
                candidates
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

#[cfg(not(unix))]
pub fn find_local_daemon_name(namespace_filter: Option<&str>) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "当前平台不支持 unixpipe 本机 fast path;请显式指定 target name。namespace={namespace_filter:?}"
        ),
    ))
}

#[cfg(unix)]
fn no_local_daemon_error(namespace_filter: Option<&str>) -> io::Error {
    let scope = match namespace_filter {
        Some(ns) => format!("namespace={ns} 的"),
        None => String::new(),
    };
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "未找到{scope}active managed local-default registry;请确保daemon配置了 `[zenoh.unixpipe] local_default = true`并已启动,或显式指定 target name(例如 `rdog control <name> @<line>`)"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        sync::{Mutex, MutexGuard, OnceLock},
    };

    // -----------------------------------------------------------------
    // unixpipe path derivation / cleanup / probe / compose_listen
    // -----------------------------------------------------------------

    fn env_test_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn unixpipe_socket_path_should_respect_tmpdir_env() {
        let _guard = env_test_guard();
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
        assert_eq!(
            path,
            PathBuf::from("/tmp/rdog-tmpdir-test/rdog-lab-mac.lab.pipe")
        );
    }

    #[test]
    fn unixpipe_socket_path_should_fallback_to_slash_tmp_when_tmpdir_unset() {
        let _guard = env_test_guard();
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
        let _guard = env_test_guard();
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

    // -----------------------------------------------------------------
    // find_local_daemon_name(rdog control self / 空 target 用)
    // -----------------------------------------------------------------

    fn make_mock_unixpipe(namespace: &str, daemon_name: &str) -> PathBuf {
        // 模拟 daemon 写出的 <base>_uplink FIFO,让 find_local_daemon_name 把它认作真 daemon。
        // 注意:base 本身不创建(Zenoh 1.8.0 不创建 base 文件),只创建 _uplink。
        let tmpdir = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let base = tmpdir.join(format!("rdog-{namespace}-{daemon_name}.pipe"));
        let uplink = base.with_file_name(format!(
            "{}_uplink",
            base.file_name().unwrap().to_str().unwrap()
        ));
        let status = std::process::Command::new("mkfifo")
            .arg(&uplink)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success());
        base
    }

    fn cleanup_mock_unixpipe(base: &Path) {
        let uplink = base.with_file_name(format!(
            "{}_uplink",
            base.file_name().unwrap().to_str().unwrap()
        ));
        let _ = fs::remove_file(&uplink);
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rdog-{prefix}-{}-{}",
            std::process::id(),
            unix_timestamp_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("测试临时目录应该能创建");
        dir
    }

    fn with_local_default_test_dir<R>(prefix: &str, f: impl FnOnce(&Path) -> R) -> R {
        let dir = unique_test_dir(prefix);
        set_local_default_daemon_test_dir(Some(dir.clone()));
        let result = f(&dir);
        set_local_default_daemon_test_dir(None);
        let _ = fs::remove_dir_all(&dir);
        result
    }

    fn with_tmpdir_test_dir<R>(prefix: &str, f: impl FnOnce(&Path) -> R) -> R {
        let dir = unique_test_dir(prefix);
        let prev = env::var_os("TMPDIR");
        unsafe { env::set_var("TMPDIR", &dir) };
        let result = f(&dir);
        match prev {
            Some(value) => unsafe { env::set_var("TMPDIR", value) },
            None => unsafe { env::remove_var("TMPDIR") },
        }
        let _ = fs::remove_dir_all(&dir);
        result
    }

    fn mock_unixpipe_base_in(dir: &Path, namespace: &str, daemon_name: &str) -> PathBuf {
        let base = dir.join(format!("rdog-{namespace}-{daemon_name}.pipe"));
        let uplink = base.with_file_name(format!(
            "{}_uplink",
            base.file_name().unwrap().to_str().unwrap()
        ));
        let status = std::process::Command::new("mkfifo")
            .arg(&uplink)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success(), "mkfifo 应该成功: {}", uplink.display());
        base
    }

    fn write_local_default_record_for_test(
        namespace: &str,
        daemon_name: &str,
        pid: u32,
        unixpipe_base: PathBuf,
        created_at_unix_ms: u128,
    ) {
        let record_path =
            local_default_daemon_record_path(namespace).expect("registry path 应该可推导");
        fs::create_dir_all(record_path.parent().expect("registry path 应该有 parent"))
            .expect("registry dir 应该能创建");
        let record = LocalDefaultDaemonRecord {
            schema: LOCAL_DEFAULT_SCHEMA.to_string(),
            namespace: namespace.to_string(),
            daemon_name: daemon_name.to_string(),
            pid,
            unixpipe_base,
            created_at_unix_ms,
            lease_schema: None,
            lease_id: None,
            lease_resource_kind: None,
            lease_resource_key: None,
            lease_created_at_unix_ms: None,
        };
        write_local_default_daemon_record(&record_path, &record)
            .expect("registry record 应该能写入");
    }

    #[test]
    fn find_local_daemon_name_should_reject_legacy_registry_even_with_matching_fifo() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-legacy", |registry_dir| {
            with_tmpdir_test_dir("local-default-legacy-fifo", |fifo_dir| {
                let ns = "ldlegacy";
                let legacy_base = mock_unixpipe_base_in(fifo_dir, ns, "legacy.ldlegacy");
                write_local_default_record_for_test(
                    ns,
                    "legacy.ldlegacy",
                    std::process::id(),
                    legacy_base,
                    unix_timestamp_ms(),
                );

                let err = find_local_daemon_name(Some(ns)).unwrap_err();

                let _ = fs::remove_dir_all(registry_dir);
                let msg = err.to_string();
                assert_eq!(err.kind(), io::ErrorKind::NotFound);
                assert!(
                    msg.contains("自动选择已退役"),
                    "应说明legacy registry/FIFO不能再自动选择: {msg}"
                );
                assert!(
                    msg.contains("显式指定 target name") && msg.contains("local_default = true"),
                    "应给出显式target和managed local-default恢复路径: {msg}"
                );
            });
        });
    }

    #[test]
    fn find_local_daemon_name_should_ignore_but_preserve_stale_local_default_lease() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-stale-pid", |registry_dir| {
            with_tmpdir_test_dir("local-default-stale-pid-fifo", |fifo_dir| {
                let ns = "ldstalepid";
                let stale_base = fifo_dir.join(format!("rdog-{ns}-stale.ldstalepid.pipe"));
                let fallback_base = make_mock_unixpipe(ns, "fallback.ldstalepid");
                write_local_default_record_for_test(
                    ns,
                    "stale.ldstalepid",
                    u32::MAX,
                    stale_base,
                    unix_timestamp_ms().saturating_sub(LOCAL_DEFAULT_STARTUP_GRACE_MS + 1),
                );
                let record_path = local_default_daemon_record_path(ns).expect("path");

                let err = find_local_daemon_name(Some(ns)).unwrap_err();

                cleanup_mock_unixpipe(&fallback_base);
                assert!(
                    record_path.exists(),
                    "client只能忽略stale registry,不能删除稳定lease状态"
                );
                let _ = fs::remove_dir_all(registry_dir);
                let msg = err.to_string();
                assert_eq!(err.kind(), io::ErrorKind::NotFound);
                assert!(
                    msg.contains("fallback.ldstalepid") && msg.contains("自动选择已退役"),
                    "stale legacy registry后只能报告unmanaged FIFO诊断: {msg}"
                );
            });
        });
    }

    #[test]
    fn find_local_daemon_name_should_ignore_registry_when_uplink_missing() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-missing-uplink", |registry_dir| {
            with_tmpdir_test_dir("local-default-missing-uplink-fifo", |fifo_dir| {
                let ns = "ldmissup";
                let missing_base = fifo_dir.join(format!("rdog-{ns}-missing.ldmissup.pipe"));
                let fallback_base = make_mock_unixpipe(ns, "fallback.ldmissup");
                let lease_guard =
                    register_local_default_daemon(ns, "missing.ldmissup", &missing_base)
                        .expect("managed local-default owner should register");
                let record_path = local_default_daemon_record_path(ns).expect("path");
                let mut record = read_local_default_daemon_record(&record_path)
                    .expect("managed registry should be readable");
                record.created_at_unix_ms =
                    unix_timestamp_ms().saturating_sub(LOCAL_DEFAULT_STARTUP_GRACE_MS + 1);
                write_local_default_daemon_record(&record_path, &record)
                    .expect("aged managed registry should be written");

                let err = find_local_daemon_name(Some(ns)).unwrap_err();

                cleanup_mock_unixpipe(&fallback_base);
                assert!(
                    record_path.exists(),
                    "缺失uplink时只能忽略registry,不能与新owner并发删除"
                );
                drop(lease_guard);
                let _ = fs::remove_dir_all(registry_dir);
                let msg = err.to_string();
                assert_eq!(err.kind(), io::ErrorKind::NotFound);
                assert!(
                    msg.contains("fallback.ldmissup") && msg.contains("自动选择已退役"),
                    "缺失uplink后只能报告unmanaged FIFO诊断: {msg}"
                );
            });
        });
    }

    #[test]
    fn find_local_daemon_name_should_keep_starting_registry_when_uplink_missing_briefly() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-starting", |registry_dir| {
            with_tmpdir_test_dir("local-default-starting-fifo", |fifo_dir| {
                let ns = "ldstarting";
                let missing_base = fifo_dir.join(format!("rdog-{ns}-starting.ldstarting.pipe"));
                let lease_guard =
                    register_local_default_daemon(ns, "starting.ldstarting", &missing_base)
                        .expect("starting managed owner should register");
                let record_path = local_default_daemon_record_path(ns).expect("path");
                let record = read_local_default_daemon_record(&record_path)
                    .expect("starting managed registry should be readable");

                assert!(
                    record
                        .should_keep_during_startup(Some(ns))
                        .expect("startup grace probe should work"),
                    "active managed owner在短暂缺失uplink时应进入启动宽限期"
                );

                let result = find_local_daemon_name(Some(ns));

                assert!(record_path.exists(), "启动宽限期内 registry 不应被清理");
                drop(lease_guard);
                let _ = fs::remove_dir_all(registry_dir);
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
            });
        });
    }

    #[test]
    fn find_local_daemon_name_should_error_when_multiple_valid_local_defaults_without_namespace() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-multiple", |registry_dir| {
            with_tmpdir_test_dir("local-default-multiple-fifo", |fifo_dir| {
                let base_a = mock_unixpipe_base_in(fifo_dir, "ldmulti1", "one.ldmulti1");
                let base_b = mock_unixpipe_base_in(fifo_dir, "ldmulti2", "two.ldmulti2");
                let guard_a = register_local_default_daemon("ldmulti1", "one.ldmulti1", &base_a)
                    .expect("first managed local-default should register");
                let guard_b = register_local_default_daemon("ldmulti2", "two.ldmulti2", &base_b)
                    .expect("second managed local-default should register");

                let err = find_local_daemon_name(None).unwrap_err();

                drop(guard_b);
                drop(guard_a);
                let _ = fs::remove_dir_all(registry_dir);
                let msg = err.to_string();
                assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
                assert!(msg.contains("local-default"), "应说明 registry 冲突: {msg}");
                assert!(msg.contains("one.ldmulti1"), "应列出第一个默认: {msg}");
                assert!(msg.contains("two.ldmulti2"), "应列出第二个默认: {msg}");
            });
        });
    }

    #[test]
    fn register_local_default_daemon_should_fail_when_same_namespace_guard_is_alive() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-guard", |registry_dir| {
            with_tmpdir_test_dir("local-default-guard-fifo", |fifo_dir| {
                let ns = "ldguard";
                let base = mock_unixpipe_base_in(fifo_dir, ns, "first.ldguard");
                let first_guard =
                    register_local_default_daemon(ns, "first.ldguard", &base).expect("first guard");

                assert_eq!(
                    find_local_daemon_name(Some(ns))
                        .expect("shared probe应该识别active managed lease"),
                    "first.ldguard"
                );

                let err = register_local_default_daemon(ns, "second.ldguard", &base).unwrap_err();
                assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
                assert!(err.to_string().contains("本机默认 daemon 已存在"));

                let guard_path = local_default_daemon_guard_path(ns).expect("guard path");
                let record_path = local_default_daemon_record_path(ns).expect("record path");
                drop(first_guard);

                // lease文件是稳定inode,owner退出只释放lock,不能删除路径。
                assert!(guard_path.exists(), "namespace lease file应该保留");
                assert!(record_path.exists(), "registry metadata应该保留");
                let second_guard = register_local_default_daemon(ns, "second.ldguard", &base)
                    .expect("released managed lease应该允许新owner接管");
                drop(second_guard);

                let _ = fs::remove_dir_all(registry_dir);
            });
        });
    }

    #[test]
    fn managed_local_default_record_should_require_matching_lease_id() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-lease-id", |registry_dir| {
            with_tmpdir_test_dir("local-default-lease-id-fifo", |fifo_dir| {
                let namespace = "ldleaseid";
                let base = mock_unixpipe_base_in(fifo_dir, namespace, "old.ldleaseid");
                let first_guard = register_local_default_daemon(namespace, "old.ldleaseid", &base)
                    .expect("first local-default owner should register");
                let record_path = local_default_daemon_record_path(namespace).expect("record path");
                let stale_record = read_local_default_daemon_record(&record_path)
                    .expect("first managed record should be readable");
                let guard_path = local_default_daemon_guard_path(namespace).expect("guard path");
                let metadata_path = process_lease::metadata_path_for_lock(&guard_path);
                drop(first_guard);

                // 模拟同PID的新lease已经持锁并发布不同lease ID,但registry尚未覆盖的窗口。
                let replacement_metadata = process_lease::LeaseMetadata {
                    lease_schema: process_lease::PROCESS_LEASE_SCHEMA.to_owned(),
                    lease_id: uuid::Uuid::new_v4().to_string(),
                    lease_resource_kind: "local-default".to_owned(),
                    lease_resource_key: namespace.to_owned(),
                    lease_created_at_unix_ms: unix_timestamp_ms(),
                    pid: std::process::id(),
                };
                process_lease::write_json_atomically(&metadata_path, &replacement_metadata)
                    .expect("replacement lease metadata should publish");
                let lock_file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&guard_path)
                    .expect("stable lease file should open");
                lock_file
                    .try_lock()
                    .expect("released namespace lease should be lockable");

                assert!(
                    !stale_record
                        .owner_is_active()
                        .expect("managed owner probe should work"),
                    "旧registry的lease ID不能冒充当前active lease"
                );

                drop(lock_file);
                cleanup_mock_unixpipe(&base);
                let _ = fs::remove_dir_all(registry_dir);
            });
        });
    }

    #[test]
    fn partial_managed_local_default_record_should_not_fallback_to_legacy_pid() {
        let _guard = env_test_guard();
        with_local_default_test_dir("local-default-partial-lease", |registry_dir| {
            with_tmpdir_test_dir("local-default-partial-lease-fifo", |fifo_dir| {
                let namespace = "ldpartial";
                let base = mock_unixpipe_base_in(fifo_dir, namespace, "partial.ldpartial");
                let lease_guard =
                    register_local_default_daemon(namespace, "partial.ldpartial", &base)
                        .expect("managed local-default owner should register");
                let record_path = local_default_daemon_record_path(namespace).expect("record path");
                let mut partial_record = read_local_default_daemon_record(&record_path)
                    .expect("managed record should be readable");

                // 任一lease字段存在就表明这是managed记录。字段缺失时必须判invalid,
                // 不能回退到只看PID的legacy路径。
                partial_record.lease_id = None;
                assert!(
                    !partial_record
                        .owner_is_active()
                        .expect("partial managed owner probe should work"),
                    "部分managed字段不能降级为legacy PID owner"
                );

                drop(lease_guard);
                cleanup_mock_unixpipe(&base);
                let _ = fs::remove_dir_all(registry_dir);
            });
        });
    }

    #[test]
    fn find_local_daemon_name_should_reject_unique_unmanaged_fifo() {
        let _guard = env_test_guard();
        with_tmpdir_test_dir("find-unique", |_| {
            let base = make_mock_unixpipe("rdogfindunique", "findme.findunique");

            let err = find_local_daemon_name(Some("rdogfindunique")).unwrap_err();
            cleanup_mock_unixpipe(&base);

            let msg = err.to_string();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
            assert!(msg.contains("findme.findunique"), "应列出诊断候选: {msg}");
            assert!(
                msg.contains("自动选择已退役"),
                "唯一unmanaged FIFO也不能作为owner真相源: {msg}"
            );
        });
    }

    #[test]
    fn find_local_daemon_name_should_filter_unmanaged_fifo_diagnostics_by_namespace() {
        let _guard = env_test_guard();
        with_tmpdir_test_dir("find-filter", |_| {
            let base_keep = make_mock_unixpipe("rdogkeepns", "keep.keepns");
            let base_skip = make_mock_unixpipe("rdogotherns", "skip.otherns");

            let err = find_local_daemon_name(Some("rdogkeepns")).unwrap_err();
            cleanup_mock_unixpipe(&base_keep);
            cleanup_mock_unixpipe(&base_skip);

            let msg = err.to_string();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
            assert!(
                msg.contains("keep.keepns"),
                "应保留目标namespace候选: {msg}"
            );
            assert!(
                !msg.contains("skip.otherns"),
                "不能泄漏其他namespace候选: {msg}"
            );
        });
    }

    #[test]
    fn find_local_daemon_name_should_error_when_no_match() {
        let _guard = env_test_guard();
        with_tmpdir_test_dir("find-no-match", |_| {
            let result = find_local_daemon_name(Some("rdog-nonexistent-ns-for-test-12345"));
            let err = result.unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
            assert!(err.to_string().contains("未找到"));
        });
    }

    #[test]
    fn find_local_daemon_name_should_report_multiple_unmanaged_fifo_candidates() {
        let _guard = env_test_guard();
        with_tmpdir_test_dir("find-multiple", |_| {
            // 多个FIFO只作为诊断信息,不能恢复旧的候选选择逻辑。
            let base1 = make_mock_unixpipe("rdogmulti", "first.multi");
            let base2 = make_mock_unixpipe("rdogmulti", "second.multi");

            let result = find_local_daemon_name(Some("rdogmulti"));
            cleanup_mock_unixpipe(&base1);
            cleanup_mock_unixpipe(&base2);

            let err = result.unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
            let msg = err.to_string();
            assert!(msg.contains("first.multi"), "应列出 first.multi: {msg}");
            assert!(msg.contains("second.multi"), "应列出 second.multi: {msg}");
            assert!(msg.contains("自动选择已退役"), "应说明退役契约: {msg}");
        });
    }

    #[test]
    fn find_local_daemon_name_should_skip_files_without_uplink_sibling() {
        let _guard = env_test_guard();
        with_tmpdir_test_dir("find-skip-no-uplink", |tmpdir| {
            // 创建一个文件,名字像 rdog-lab-fake.pipe 但没有 _uplink 兄弟
            // find_local_daemon_name 必须跳过它
            let base = tmpdir.join("rdog-rdogfakens-fake.pipe");
            let _ = fs::remove_file(&base);
            fs::write(&base, b"not a fifo").expect("写入 fake 文件");

            let result = find_local_daemon_name(Some("rdogfakens"));
            let _ = fs::remove_file(&base);

            // 没有 _uplink 兄弟,不能算 daemon
            let err = result.unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
        });
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
        assert!(
            elapsed >= Duration::from_millis(140),
            "应该在 timeout 之后返回"
        );

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
        let _guard = env_test_guard();
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.unixpipe.socket_path = None;
        cfg.listen_endpoints = vec!["udp/0.0.0.0:7447".to_string()];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        assert_eq!(composed.listen_endpoints.len(), 2);
        assert!(composed.listen_endpoints[0].starts_with("unixpipe/"));
        assert!(composed.listen_endpoints[0].contains("rdog-lab-mac.lab.pipe"));
        assert_eq!(composed.listen_endpoints[1], "udp/0.0.0.0:7447");
        assert!(composed
            .unixpipe_base_path
            .expect("unixpipe base should be resolved")
            .ends_with("rdog-lab-mac.lab.pipe"));
    }

    #[test]
    fn compose_listen_endpoints_should_not_inject_when_disabled() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = false;
        cfg.listen_endpoints = vec!["udp/0.0.0.0:7447".to_string()];

        let composed = compose_listen_endpoints(&cfg, "lab", "mac.lab").expect("ok");
        assert_eq!(
            composed.listen_endpoints,
            vec!["udp/0.0.0.0:7447".to_string()]
        );
        assert!(composed.unixpipe_base_path.is_none());
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
            composed.listen_endpoints,
            vec![
                "unixpipe//tmp/explicit.pipe".to_string(),
                "udp/0.0.0.0:7447".to_string(),
            ]
        );
        assert_eq!(
            composed.unixpipe_base_path,
            Some(PathBuf::from("/tmp/explicit.pipe"))
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
        assert_eq!(
            composed.listen_endpoints[0],
            "unixpipe//tmp/explicit-socket.pipe"
        );
        assert_eq!(composed.listen_endpoints[1], "udp/0.0.0.0:7447");
        assert_eq!(
            composed.unixpipe_base_path,
            Some(PathBuf::from("/tmp/explicit-socket.pipe"))
        );
    }

    #[test]
    fn compose_listen_endpoints_should_reject_conflicting_explicit_paths() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.unixpipe.socket_path = Some(PathBuf::from("/tmp/socket-path.pipe"));
        cfg.listen_endpoints = vec!["unixpipe//tmp/listen-endpoint.pipe".to_string()];

        let err = compose_listen_endpoints(&cfg, "lab", "mac.lab").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("不一致"));
    }

    #[test]
    fn compose_listen_endpoints_should_reject_multiple_explicit_unixpipe_endpoints() {
        use crate::config::ZenohConfig;
        let mut cfg = ZenohConfig::default();
        cfg.unixpipe.enabled = true;
        cfg.listen_endpoints = vec![
            "unixpipe//tmp/first.pipe".to_string(),
            "unixpipe//tmp/second.pipe".to_string(),
        ];

        let err = compose_listen_endpoints(&cfg, "lab", "mac.lab").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("最多只能包含一个"));
    }

    #[test]
    fn prepare_unixpipe_listener_should_recover_stale_owner_guard_and_files() {
        let dir = unique_test_dir("unixpipe-stale-owner");
        let base = dir.join("shared.pipe");
        let owner_guard = PathBuf::from(format!("{}.rdog-owner.pid", base.display()));

        // PID 0 永远不会被识别为活跃进程,用于模拟 daemon 崩溃后的 sidecar。
        fs::write(&owner_guard, "0").expect("stale owner guard should be created");
        for suffix in ["", "_uplink", "_downlink"] {
            fs::write(format!("{}{suffix}", base.display()), "stale")
                .expect("stale unixpipe artifact should be created");
        }

        let guard = prepare_unixpipe_listener(&base)
            .expect("stale owner and unixpipe files should be recoverable");
        assert_eq!(
            fs::read_to_string(&owner_guard)
                .expect("new owner guard should exist")
                .trim(),
            std::process::id().to_string()
        );
        for suffix in ["", "_uplink", "_downlink"] {
            let path = PathBuf::from(format!("{}{suffix}", base.display()));
            assert!(
                !path.exists(),
                "stale file should be removed: {}",
                path.display()
            );
        }

        // 正常退出只释放lock,稳定inode必须保留并允许下一轮接管。
        drop(guard);
        assert!(owner_guard.exists(), "owner lease file应该永久保留");
        let next_guard = prepare_unixpipe_listener(&base)
            .expect("released managed lease不应因旧PID仍存活而拒绝接管");
        drop(next_guard);
        assert!(owner_guard.exists(), "重复接管后lease file仍应保留");
        fs::remove_dir_all(dir).expect("test directory should be removed");
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
