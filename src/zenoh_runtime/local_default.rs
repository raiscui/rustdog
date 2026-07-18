//! 本机默认daemon registry、managed owner校验与空target解析。

use std::io;

#[cfg(unix)]
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use super::{
    process_lease,
    unixpipe::{unixpipe_base_path_alive, validate_unixpipe_component},
};

#[cfg(unix)]
const LOCAL_DEFAULT_SCHEMA: &str = "rdog.local-default.v1";
#[cfg(unix)]
const LOCAL_DEFAULT_STARTUP_GRACE_MS: u128 = 10_000;

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
mod tests;
