//! Zenoh本地运行态的跨进程lease.
//!
//! lock file使用稳定inode承载OS advisory lock,metadata使用原子rename发布。
//! 两者职责分离:lock是唯一liveness事实,JSON只提供identity与诊断信息。

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub(crate) const PROCESS_LEASE_SCHEMA: &str = "rdog.process-lease.v1";

/// 持久化metadata的公共lease字段。
///
/// local-default registry复用相同字段名,便于client关联sidecar identity。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct LeaseMetadata {
    pub(crate) lease_schema: String,
    pub(crate) lease_id: String,
    pub(crate) lease_resource_kind: String,
    pub(crate) lease_resource_key: String,
    pub(crate) lease_created_at_unix_ms: u128,
    pub(crate) pid: u32,
}

impl LeaseMetadata {
    fn new(resource_kind: &str, resource_key: &str) -> Self {
        Self {
            lease_schema: PROCESS_LEASE_SCHEMA.to_owned(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            lease_resource_kind: resource_kind.to_owned(),
            lease_resource_key: resource_key.to_owned(),
            lease_created_at_unix_ms: unix_timestamp_ms(),
            pid: std::process::id(),
        }
    }

    /// 判断已有metadata是否属于同一个受管资源和PID。
    fn matches_previous_owner(
        &self,
        resource_kind: &str,
        resource_key: &str,
        previous_pid: Option<u32>,
    ) -> bool {
        self.lease_schema == PROCESS_LEASE_SCHEMA
            && self.lease_resource_kind == resource_kind
            && self.lease_resource_key == resource_key
            && previous_pid == Some(self.pid)
    }
}

/// shared probe观察到的lease活跃状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseActivity {
    Active { pid: Option<u32> },
    Inactive,
}

/// 进程持有的exclusive lease。
///
/// `file`必须覆盖owner生命周期。Drop不删除任何路径,仅通过关闭descriptor释放lock。
#[derive(Debug)]
pub(crate) struct ProcessLease {
    file: File,
    metadata_path: PathBuf,
    metadata: LeaseMetadata,
    previous_lock_contents: Vec<u8>,
    metadata_published: bool,
}

impl ProcessLease {
    /// 尝试获取资源lease,不会阻塞等待另一个owner退出。
    ///
    /// metadata匹配时,lock可获取已经证明旧owner死亡,即使PID被复用也允许接管。
    /// metadata缺失或不匹配时按legacy PID guard处理,保护仍在运行的旧版本daemon。
    pub(crate) fn acquire(
        lock_path: PathBuf,
        metadata_path: PathBuf,
        resource_kind: &str,
        resource_key: &str,
    ) -> io::Result<Self> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // create(true)只保证路径存在,真正的互斥由同一inode上的exclusive lock提供。
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "process lease已被活跃owner持有: kind={resource_kind}, key={resource_key}, lock={}",
                        lock_path.display()
                    ),
                ));
            }
            Err(TryLockError::Error(err)) => return Err(err),
        }

        let previous_lock_contents = read_lock_contents(&mut file)?;
        let previous_pid = parse_compat_pid(&previous_lock_contents);
        let previous_metadata = read_json_if_valid::<LeaseMetadata>(&metadata_path);
        let previous_is_managed = previous_metadata.as_ref().is_some_and(|metadata| {
            metadata.matches_previous_owner(resource_kind, resource_key, previous_pid)
        });

        if !previous_is_managed && previous_pid.is_some_and(process_exists) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "发现仍存活的legacy PID guard: kind={resource_kind}, key={resource_key}, pid={}, lock={}",
                    previous_pid.expect("live legacy PID checked above"),
                    lock_path.display()
                ),
            ));
        }

        let metadata = LeaseMetadata::new(resource_kind, resource_key);
        write_compat_pid(&mut file, metadata.pid)?;
        Ok(Self {
            file,
            metadata_path,
            metadata,
            previous_lock_contents,
            metadata_published: false,
        })
    }

    /// 发布service/path等不带业务payload的通用metadata。
    pub(crate) fn publish_metadata(&mut self) -> io::Result<()> {
        let result = write_json_atomically(&self.metadata_path, &self.metadata);
        if result.is_ok()
            || read_json_if_valid::<LeaseMetadata>(&self.metadata_path).as_ref()
                == Some(&self.metadata)
        {
            // rename已经可见但目录sync失败时,metadata仍属于当前lease,不能回滚PID造成分裂。
            self.metadata_published = true;
        }
        result
    }

    pub(crate) fn metadata(&self) -> &LeaseMetadata {
        &self.metadata
    }
}

impl Drop for ProcessLease {
    fn drop(&mut self) {
        if self.metadata_published {
            return;
        }

        // 未提交lease不能把当前PID伪装成legacy owner。此时exclusive lock仍由本对象持有,
        // 因此恢复旧内容不会与另一个owner并发写stable lock file。
        if let Err(err) = write_lock_contents(&mut self.file, &self.previous_lock_contents) {
            log::warn!(
                "回滚未提交process lease PID失败: metadata={}, error={err}",
                self.metadata_path.display()
            );
        }
    }
}

/// 使用shared lock做无副作用活跃探测。
///
/// probe成功拿到shared lock说明没有exclusive owner。File离开作用域后立即释放shared lock。
pub(crate) fn probe_activity(lock_path: &Path) -> io::Result<LeaseActivity> {
    let mut file = match OpenOptions::new().read(true).write(true).open(lock_path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(LeaseActivity::Inactive),
        Err(err) => return Err(err),
    };

    match file.try_lock_shared() {
        Ok(()) => Ok(LeaseActivity::Inactive),
        Err(TryLockError::WouldBlock) => Ok(LeaseActivity::Active {
            pid: parse_compat_pid(&read_lock_contents(&mut file)?),
        }),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

/// 验证业务记录引用的lease是否正是当前exclusive owner。
///
/// identity先与独立sidecar完整比较,再探测stable lock。这样PID数值复用时,
/// 旧registry也不能冒充新owner。
pub(crate) fn managed_lease_is_active(
    lock_path: &Path,
    expected_metadata: &LeaseMetadata,
) -> io::Result<bool> {
    let metadata_path = metadata_path_for_lock(lock_path);
    if read_json_if_valid::<LeaseMetadata>(&metadata_path).as_ref() != Some(expected_metadata) {
        return Ok(false);
    }

    Ok(matches!(
        probe_activity(lock_path)?,
        LeaseActivity::Active { pid } if pid == Some(expected_metadata.pid)
    ))
}

/// 从lock路径推导通用metadata sidecar。
pub(crate) fn metadata_path_for_lock(lock_path: &Path) -> PathBuf {
    let mut path = lock_path.as_os_str().to_os_string();
    path.push(".lease.json");
    PathBuf::from(path)
}

/// 同目录临时文件 + sync + rename发布JSON,避免reader看到部分写入。
pub(crate) fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("lease metadata路径缺少parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;

    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("lease metadata路径缺少文件名: {}", path.display()),
        )
    })?;
    let temp_path = parent.join(format!(
        ".{}.{}.tmp",
        file_name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(to_io_error)?;

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn read_lock_contents(file: &mut File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

fn parse_compat_pid(contents: &[u8]) -> Option<u32> {
    std::str::from_utf8(contents).ok()?.trim().parse().ok()
}

fn write_compat_pid(file: &mut File, pid: u32) -> io::Result<()> {
    write_lock_contents(file, pid.to_string().as_bytes())
}

fn write_lock_contents(file: &mut File, contents: &[u8]) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(contents)?;
    file.sync_data()
}

fn read_json_if_valid<T: DeserializeOwned>(path: &Path) -> Option<T> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub(crate) fn process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zenoh_runtime::test_support::env_test_guard;

    fn test_paths(prefix: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "rdog-process-lease-{prefix}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("test lease dir should be created");
        (dir.clone(), dir.join("owner.pid"), dir.join("owner.json"))
    }

    #[test]
    fn active_lease_blocks_competitor_then_releases_without_unlink() {
        let _guard = env_test_guard();
        let (dir, lock_path, metadata_path) = test_paths("lifecycle");
        let mut first = ProcessLease::acquire(
            lock_path.clone(),
            metadata_path.clone(),
            "test",
            "same-resource",
        )
        .expect("first lease should be acquired");
        first.publish_metadata().expect("metadata should publish");

        let err = ProcessLease::acquire(
            lock_path.clone(),
            metadata_path.clone(),
            "test",
            "same-resource",
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            probe_activity(&lock_path).expect("active probe should work"),
            LeaseActivity::Active {
                pid: Some(std::process::id())
            }
        );

        drop(first);
        assert!(
            lock_path.exists(),
            "stable lock file must remain after Drop"
        );
        assert_eq!(
            probe_activity(&lock_path).expect("inactive probe should work"),
            LeaseActivity::Inactive
        );

        let second =
            ProcessLease::acquire(lock_path.clone(), metadata_path, "test", "same-resource")
                .expect("managed stale PID should not block takeover");
        drop(second);
        fs::remove_dir_all(dir).expect("test lease dir should be removed");
    }

    #[test]
    fn live_legacy_pid_without_managed_metadata_is_rejected() {
        let _guard = env_test_guard();
        let (dir, lock_path, metadata_path) = test_paths("legacy");
        fs::write(&lock_path, std::process::id().to_string())
            .expect("legacy PID guard should be written");

        let err =
            ProcessLease::acquire(lock_path, metadata_path, "test", "legacy-resource").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(err.to_string().contains("legacy PID guard"));
        fs::remove_dir_all(dir).expect("test lease dir should be removed");
    }

    #[test]
    fn metadata_publish_failure_should_not_leave_self_blocking_legacy_pid() {
        let _guard = env_test_guard();
        let (dir, lock_path, metadata_path) = test_paths("publish-rollback");
        let mut first = ProcessLease::acquire(
            lock_path.clone(),
            metadata_path.clone(),
            "test",
            "publish-rollback-resource",
        )
        .expect("lease should be acquired before publishing metadata");

        // 用同名目录稳定触发rename失败,模拟metadata发布阶段的I/O错误。
        fs::create_dir(&metadata_path).expect("blocking metadata directory should be created");
        first
            .publish_metadata()
            .expect_err("publishing metadata onto a directory should fail");
        drop(first);
        fs::remove_dir(&metadata_path).expect("blocking metadata directory should be removed");

        // 发布失败不代表资源仍被占用,同一进程必须能够立即重试。
        let retry = ProcessLease::acquire(
            lock_path,
            metadata_path,
            "test",
            "publish-rollback-resource",
        )
        .expect("failed metadata publish must not leave a self-blocking legacy PID");
        drop(retry);
        fs::remove_dir_all(dir).expect("test lease dir should be removed");
    }

    #[test]
    fn atomic_json_publish_replaces_complete_document() {
        let _guard = env_test_guard();
        let (dir, _lock_path, metadata_path) = test_paths("atomic-json");
        let first = LeaseMetadata::new("test", "first");
        let second = LeaseMetadata::new("test", "second");

        write_json_atomically(&metadata_path, &first).expect("first publish should work");
        write_json_atomically(&metadata_path, &second).expect("second publish should work");
        let actual: LeaseMetadata =
            serde_json::from_slice(&fs::read(&metadata_path).expect("metadata should be readable"))
                .expect("metadata should be valid JSON");

        assert_eq!(actual, second);
        let temp_count = fs::read_dir(&dir)
            .expect("test dir should be readable")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temp_count, 0, "atomic publish must not leave temp files");
        fs::remove_dir_all(dir).expect("test lease dir should be removed");
    }
}
