//! 本机 daemon 控制 lane 登记表 —— 本机 fast path 的「谁在这台机器上、它的控制面落在哪」真相源。
//!
//! 为什么需要它: unix 本机 fast path 靠 unixpipe FIFO (`zenoh_runtime::local_default`),
//! 文件在不在就是 liveness 证据。Windows 的 zenoh 构建没有 unixpipe transport,daemon
//! 以 zenoh 模式启动后本机只剩一条 zenoh listen endpoint(可能绑在 `0.0.0.0` 上),
//! 客户端因此既不知道 daemon 叫什么,也无从判断哪个端口说 zenoh 协议而不是 line-control。
//! 于是 daemon 启动时把身份与本机落点写进 `~/.rdog/run/local-daemon/<ns>__<name>.json`,
//! `rdog control self` / `rdog control <port>` 读它就能零配置自连。
//!
//! liveness 的真相源是 lock 文件上的 OS advisory lock,不是 PID: 进程无论怎么死,
//! 内核都会释放 lock,而 PID 会被别的进程复用。记录里的 pid 只是与 lock 内容对齐的 identity。
//!
//! 平台分工: unix 继续走 unixpipe lane(不写这份登记,避免两套本机 fast path 并存造成
//! "self 到底选了谁"二义); 非 unix 的 zenoh daemon 是这里的唯一写入方。

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub(crate) const LOCAL_DAEMON_SCHEMA: &str = "rdog.local-daemon.v1";

/// daemon 暴露给本机客户端的控制 lane。
///
/// 这里刻意只描述"本机怎么连上它",不含网络可达性: 外部入口仍由 daemon 配置的
/// listen endpoint 决定,本表不参与跨机寻址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum LocalControlLane {
    /// daemon 是 zenoh router;`connect_endpoints` 是回环化后的 connect endpoint。
    /// 空表示"由 zenoh 层自己决定"(unix 上由 unixpipe 探测接手)。
    Zenoh {
        #[serde(default)]
        connect_endpoints: Vec<String>,
    },
    /// daemon 是 line-control TCP 接收端(zenoh 关闭或纯 TCP 模式)。
    LineTcp { host: String, port: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LocalDaemonRecord {
    pub(crate) schema: String,
    pub(crate) namespace: String,
    pub(crate) daemon_name: String,
    pub(crate) pid: u32,
    pub(crate) lane: LocalControlLane,
    pub(crate) created_at_unix_ms: u128,
}

impl LocalDaemonRecord {
    fn matches_namespace(&self, namespace_filter: Option<&str>) -> bool {
        self.schema == LOCAL_DAEMON_SCHEMA
            && namespace_filter.is_none_or(|namespace| namespace == self.namespace)
    }

    /// 这个记录是否在回环 **TCP** 上提供控制面。
    ///
    /// 客户端只看得到 "127.0.0.1:<port>" 这个 TCP 目标,看不到对端跑什么协议;
    /// 这里就是"该端口属于我这台机器上哪个已登记 daemon"的唯一判据。
    ///
    /// 只认 tcp/: daemon 的 zenoh listen endpoint 里常同时有 `udp/0.0.0.0:7447`,
    /// 若按端口号匹配,一个恰好监听 TCP 7447 的 line-control 服务会被误认领到那个
    /// udp lane 上,请求就打到别的服务去了。
    pub(crate) fn lane_serves_tcp_port(&self, port: u16) -> bool {
        match &self.lane {
            LocalControlLane::LineTcp { port: lane_port, .. } => *lane_port == port,
            LocalControlLane::Zenoh {
                connect_endpoints, ..
            } => connect_endpoints
                .iter()
                .any(|endpoint| is_loopback_tcp_endpoint(endpoint, port)),
        }
    }
}

/// `tcp/127.0.0.1:17480` 是否就是"回环 TCP 的 <port>"。
fn is_loopback_tcp_endpoint(endpoint: &str, port: u16) -> bool {
    let Some((scheme, address)) = endpoint.split_once('/') else {
        return false;
    };
    if scheme != "tcp" {
        return false;
    }
    let Some((host, endpoint_port)) = address.rsplit_once(':') else {
        return false;
    };
    let Ok(endpoint_port) = endpoint_port.parse::<u16>() else {
        return false;
    };
    endpoint_port == port
        && matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// daemon 生命周期内持有的登记 guard;drop 时撤销记录并释放 lock。
#[derive(Debug)]
pub(crate) struct LocalDaemonGuard {
    record_path: PathBuf,
    lock_path: PathBuf,
    _lock: File,
}

impl Drop for LocalDaemonGuard {
    fn drop(&mut self) {
        // 只撤销记录,保留 lock 路径: 删除被独占锁住的路径会让并发注册者拿到
        // 另一个 inode 上的锁,互斥就失效了。残留 lock 文件不构成 liveness。
        let _ = fs::remove_file(&self.record_path);
    }
}

/// 登记本机 daemon 的控制 lane。
///
/// 同一 (namespace, daemon_name) 已被活跃 owner 持有时报错: 那是 daemon 身份重复,
/// 让后启动的一方悄悄覆盖会让客户端连到错误的 daemon。
pub(crate) fn register_local_daemon(
    namespace: &str,
    daemon_name: &str,
    lane: LocalControlLane,
) -> io::Result<LocalDaemonGuard> {
    let dir = local_daemon_dir()?;
    fs::create_dir_all(&dir)?;
    let record_path = dir.join(record_file_name(namespace, daemon_name));
    let lock_path = dir.join(lock_file_name(namespace, daemon_name));

    let mut lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "本机已存在活跃的 daemon 登记: namespace={namespace}, daemon_name={daemon_name}, lock={}",
                    lock_path.display()
                ),
            ));
        }
        Err(TryLockError::Error(err)) => return Err(err),
    }

    let record = LocalDaemonRecord {
        schema: LOCAL_DAEMON_SCHEMA.to_string(),
        namespace: namespace.to_string(),
        daemon_name: daemon_name.to_string(),
        pid: std::process::id(),
        lane,
        created_at_unix_ms: unix_timestamp_ms(),
    };

    write_json_atomically(&record_path, &record)?;

    Ok(LocalDaemonGuard {
        record_path,
        lock_path,
        _lock: lock,
    })
}

/// 解析本机唯一的已登记 daemon。
pub(crate) fn find_local_daemon(namespace_filter: Option<&str>) -> io::Result<LocalDaemonRecord> {
    let records = live_local_daemons(namespace_filter)?;
    match records.len() {
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "本机没有正在运行的 daemon 登记记录 ({})请确认 daemon 已启动,或显式指定 target name(例如 `rdog control <name> @<line>`)",
                namespace_scope_hint(namespace_filter)
            ),
        )),
        1 => Ok(records.into_iter().next().expect("length checked above")),
        _ => {
            let instances = records
                .iter()
                .map(|record| format!("`{}`/`{}`(pid={})", record.namespace, record.daemon_name, record.pid))
                .collect::<Vec<_>>()
                .join(", ");
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "本机登记了多个 daemon: [{instances}];请用 `--namespace` 或显式 target name 指定"
                ),
            ))
        }
    }
}

/// 找出把控制面落在 `port` 上的本机 daemon。
///
/// 客户端手里只有 "127.0.0.1:<port>" 时,这张表是判断"该端口说哪种协议"的唯一依据;
/// 没登记的一律交给 line-control TCP,保持老行为。
pub(crate) fn find_local_daemon_by_port(port: u16) -> io::Result<Option<LocalDaemonRecord>> {
    let records = live_local_daemons(None)?;
    let mut matched = records
        .into_iter()
        .filter(|record| record.lane_serves_tcp_port(port))
        .collect::<Vec<_>>();
    match matched.len() {
        0 => Ok(None),
        1 => Ok(matched.pop()),
        _ => {
            let instances = matched
                .iter()
                .map(|record| format!("`{}`/`{}`", record.namespace, record.daemon_name))
                .collect::<Vec<_>>()
                .join(", ");
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("本机多个 daemon 的登记都落在端口 {port} 上: [{instances}];请显式指定 target name"),
            ))
        }
    }
}

/// 把 daemon 的 listen endpoint 列表映射成本机回环 connect endpoint 列表。
///
/// `0.0.0.0` / `[::]` 是 listen 地址,不是可达的 connect 地址 —— 拿它去连在 Windows 上
/// 直接 `os error 10049`。这里只保留 tcp/udp 两类(serial 是硬件口,tls 需要证书协商,
/// unixpipe 只在 unix 存在),序列端点不进入本机控制 lane。
pub(crate) fn loopback_zenoh_endpoints(listen_endpoints: &[String]) -> Vec<String> {
    listen_endpoints
        .iter()
        .filter_map(|endpoint| {
            let (scheme, address) = endpoint.split_once('/')?;
            if !matches!(scheme, "tcp" | "udp") {
                return None;
            }
            let (host, port) = address.rsplit_once(':')?;
            let host = match host {
                "0.0.0.0" => "127.0.0.1".to_string(),
                "[::]" | "::" | "[::0]" | "::0" => "[::1]".to_string(),
                other => other.to_string(),
            };
            Some(format!("{scheme}/{host}:{port}"))
        })
        .collect()
}

fn live_local_daemons(namespace_filter: Option<&str>) -> io::Result<Vec<LocalDaemonRecord>> {
    let dir = local_daemon_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(io::Error::new(
                err.kind(),
                format!("扫描本机 daemon 登记目录 {} 失败: {err}", dir.display()),
            ))
        }
    };

    let mut records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(record) = read_record(&path) else {
            continue;
        };
        if !record.matches_namespace(namespace_filter) {
            continue;
        }
        let lock_path = path.with_extension("lock");
        match lock_owner_state(&lock_path) {
            Ok(LockOwnerState::Held) => records.push(record),
            // 记录还在但没人持锁: 清理残留,别让它继续干扰下一次查找。
            Ok(LockOwnerState::Free) => sweep_stale_record(&path, &lock_path, &record),
            // 探测本身失败不构成"这是残留"的证据,更不能据此删别人的记录。
            Err(_) => continue,
        }
    }
    records.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then(left.daemon_name.cmp(&right.daemon_name))
    });
    Ok(records)
}

fn read_record(path: &Path) -> Option<LocalDaemonRecord> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// lock 文件上是否有人持有独占锁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockOwnerState {
    /// 有 owner 持锁: 这就是 liveness 的真相源。
    Held,
    /// 拿得到 shared lock: 没有 owner,记录属于已退出的进程。
    Free,
}

/// 只判断"锁有没有人持有",**不读被独占锁住的文件内容**。
///
/// Windows 上 `File::try_lock` 走 LockFileEx(强制字节范围锁): owner 持锁期间,
/// 另一个句柄读同一范围会直接失败。若把"读不到 pid"当成"owner 已死",活着的 daemon
/// 会被判成残留并删掉它的登记 —— 所以这里不再从 lock 里读身份,pid 只留在记录里做诊断。
fn lock_owner_state(lock_path: &Path) -> io::Result<LockOwnerState> {
    let file = match OpenOptions::new().read(true).write(true).open(lock_path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(LockOwnerState::Free),
        Err(err) => return Err(err),
    };

    match file.try_lock_shared() {
        Ok(()) => Ok(LockOwnerState::Free),
        Err(TryLockError::WouldBlock) => Ok(LockOwnerState::Held),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

/// 在独占锁的保护下撤销残留记录。
///
/// 观察"锁是空的"与执行删除之间,重启中的 daemon 可能已经接管这把锁并写好新记录;
/// 拿不到独占锁就说明有人刚接手,必须放手。拿到锁后重读一次,只删与观察到的一致的那条。
fn sweep_stale_record(record_path: &Path, lock_path: &Path, observed: &LocalDaemonRecord) {
    // 没有 lock 文件(旧记录或手工写的记录)时也要在同一把锁上完成撤销判断,
    // 否则"观察到空"和"真正删除"之间仍然没有互斥。
    let Ok(file) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
    else {
        return;
    };
    if !matches!(file.try_lock(), Ok(())) {
        return;
    }
    if read_record(record_path).as_ref() != Some(observed) {
        return;
    }
    let _ = fs::remove_file(record_path);
}

fn record_file_name(namespace: &str, daemon_name: &str) -> String {
    format!("{namespace}__{daemon_name}.json")
}

fn lock_file_name(namespace: &str, daemon_name: &str) -> String {
    format!("{namespace}__{daemon_name}.lock")
}

fn namespace_scope_hint(namespace_filter: Option<&str>) -> String {
    match namespace_filter {
        Some(namespace) => format!("namespace={namespace}) "),
        None => "namespace=<任意>) ".to_string(),
    }
}

fn local_daemon_dir() -> io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = local_daemon_test_dir() {
        return Ok(dir);
    }

    match crate::config::resolve_user_config_dir() {
        Some(dir) => Ok(dir.join("run").join("local-daemon")),
        None => Ok(std::env::temp_dir()
            .join("rustdog")
            .join("local-daemon")),
    }
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| io::Error::other(format!("序列化 daemon 登记失败: {err}")))?;
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    static LOCAL_DAEMON_TEST_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_local_daemon_test_dir(path: Option<PathBuf>) {
    LOCAL_DAEMON_TEST_DIR.with(|slot| {
        *slot.borrow_mut() = path;
    });
}

#[cfg(test)]
fn local_daemon_test_dir() -> Option<PathBuf> {
    LOCAL_DAEMON_TEST_DIR.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests;
