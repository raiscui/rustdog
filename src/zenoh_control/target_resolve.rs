use std::{fs, io, path::PathBuf, time::Duration};

#[cfg(not(unix))]
use std::{
    fs::OpenOptions,
    io::Write,
    process::{Command, Stdio},
};

use zenoh::Wait;

use crate::zenoh_identity::{
    build_alive_key, build_alive_key_with_root, build_control_key_with_root, KEYEXPR_ROOT,
    LEGACY_KEYEXPR_ROOT,
};

#[derive(Debug, Clone)]
pub(super) struct ResolvedTarget {
    pub(super) daemon_name: String,
    pub(super) control_key: String,
    pub(super) keyexpr_root: String,
}

#[derive(Debug)]
pub(super) struct DaemonNameGuard {
    #[cfg(unix)]
    _lease: crate::zenoh_runtime::process_lease::ProcessLease,
    #[cfg(not(unix))]
    path: PathBuf,
}

#[cfg(not(unix))]
impl Drop for DaemonNameGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn log_target_if_changed(
    current_target: &ResolvedTarget,
    refreshed_target: &ResolvedTarget,
) {
    if current_target.control_key != refreshed_target.control_key {
        log::info!(
            "zenoh control target selected: service_name(daemon_name)={}, member_id={}, control_key={}",
            refreshed_target.daemon_name,
            crate::zenoh_identity::member_id_from_daemon_name(&refreshed_target.daemon_name),
            refreshed_target.control_key
        );
    }
}

pub(super) fn ensure_unique_daemon_name(
    session: &zenoh::Session,
    namespace: &str,
    daemon_name: &str,
    timeout: Duration,
) -> io::Result<()> {
    let selector = build_alive_key(namespace, daemon_name);
    let replies = session
        .liveliness()
        .get(&selector)
        .timeout(timeout)
        .wait()
        .map_err(to_io_error)?;

    if let Ok(reply) = replies.recv() {
        if let Ok(sample) = reply.result() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "发现重复 service_name 活跃 member: namespace={namespace}, service_name={daemon_name}, remote_key={}",
                    sample.key_expr()
                ),
            ));
        }
    }

    Ok(())
}

pub(super) fn acquire_daemon_name_guard(
    namespace: &str,
    daemon_name: &str,
) -> io::Result<DaemonNameGuard> {
    let lock_dir = zenoh_guard_dir()?;
    fs::create_dir_all(&lock_dir)?;
    let path = lock_dir.join(format!("{namespace}__{daemon_name}.pid"));

    #[cfg(unix)]
    {
        let metadata_path = crate::zenoh_runtime::process_lease::metadata_path_for_lock(&path);
        let resource_key = format!("{namespace}/{daemon_name}");
        let mut lease = crate::zenoh_runtime::process_lease::ProcessLease::acquire(
            path.clone(),
            metadata_path,
            "service-name",
            &resource_key,
        )
        .map_err(|err| {
            if err.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "发现重复 service_name 活跃 member: namespace={namespace}, service_name={daemon_name}, local_guard={}",
                        path.display()
                    ),
                )
            } else {
                err
            }
        })?;
        lease.publish_metadata()?;
        return Ok(DaemonNameGuard { _lease: lease });
    }

    #[cfg(not(unix))]
    {
        let pid = std::process::id().to_string();
        loop {
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(pid.as_bytes())?;
                    return Ok(DaemonNameGuard { path });
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    let existing = fs::read_to_string(&path).unwrap_or_default();
                    let existing_pid = existing.trim().parse::<u32>().ok();
                    if existing_pid.is_some_and(process_exists) {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!(
                                "发现重复 service_name 活跃 member: namespace={namespace}, service_name={daemon_name}, local_guard={}",
                                path.display()
                            ),
                        ));
                    }

                    match fs::remove_file(&path) {
                        Ok(()) => continue,
                        Err(remove_err) if remove_err.kind() == io::ErrorKind::NotFound => continue,
                        Err(remove_err) => return Err(remove_err),
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }
}

pub(super) fn resolve_target(
    session: &zenoh::Session,
    namespace: &str,
    target_name: Option<&str>,
    timeout: Duration,
) -> io::Result<ResolvedTarget> {
    let mut candidates = Vec::new();
    for keyexpr_root in [KEYEXPR_ROOT, LEGACY_KEYEXPR_ROOT] {
        let selector = match target_name {
            Some(target_name) => build_alive_key_with_root(keyexpr_root, namespace, target_name),
            None => format!("{keyexpr_root}/{namespace}/daemon/*/alive"),
        };
        let replies = session
            .liveliness()
            .get(&selector)
            .timeout(timeout)
            .wait()
            .map_err(to_io_error)?;

        while let Ok(reply) = replies.recv() {
            let Ok(sample) = reply.result() else {
                continue;
            };
            if let Some(candidate) = parse_liveliness_candidate(sample.key_expr().as_str()) {
                candidates.push(candidate);
            }
        }

        // 新 root 是默认真相源。
        // 只有新 root 完全没有命中时,才继续尝试 legacy root。
        if !candidates.is_empty() {
            break;
        }
    }

    if candidates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "未找到目标 service: namespace={}, target_name={}",
                namespace,
                target_name.unwrap_or("<auto>")
            ),
        ));
    }

    if target_name.is_none() {
        candidates.sort_by(|left, right| left.daemon_name.cmp(&right.daemon_name));
        candidates.dedup_by(|left, right| left.daemon_name == right.daemon_name);
    }

    if candidates.len() > 1 {
        let instances = candidates
            .iter()
            .map(|candidate| candidate.daemon_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("目标 service 冲突,命中的 member_id: {instances}"),
        ));
    }

    Ok(candidates.remove(0))
}

pub(super) fn parse_liveliness_candidate(key: &str) -> Option<ResolvedTarget> {
    let parts = key.split('/').collect::<Vec<_>>();
    if parts.len() != 7 {
        return None;
    }
    if !matches!(parts[0], KEYEXPR_ROOT | LEGACY_KEYEXPR_ROOT)
        || parts[2] != "daemon"
        || parts[4] != "member"
        || parts[6] != "alive"
    {
        return None;
    }

    let namespace = parts[1];
    let daemon_name = parts[3];
    let member_id = parts[5];
    if member_id != daemon_name {
        return None;
    }
    Some(ResolvedTarget {
        daemon_name: daemon_name.to_string(),
        control_key: build_control_key_with_root(parts[0], namespace, daemon_name),
        keyexpr_root: parts[0].to_string(),
    })
}

fn zenoh_guard_dir() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data)
                .join("rustdog")
                .join("zenoh-guards"));
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(state_home)
                .join("rustdog")
                .join("zenoh-guards"));
        }

        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("rustdog")
                .join("zenoh-guards"));
        }
    }

    Ok(std::env::temp_dir().join("rustdog").join("zenoh-guards"))
}

#[cfg(not(unix))]
fn process_exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        if pid == 0 {
            return false;
        }

        let filter = format!("PID eq {pid}");
        return Command::new("tasklist")
            .args(["/FI", &filter])
            .output()
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
            });
    }

    #[cfg(not(windows))]
    {
        if pid == 0 {
            return false;
        }

        // 这里只是做本地 pid 存活探测。
        // stale pid 是正常清理路径的一部分,不应该把 `kill -0` 的 stderr
        // 直接泄漏到 daemon 启动输出里,否则用户会误以为启动失败了。
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .is_some_and(|status| status.success())
    }
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveliness_key_should_parse_back_to_control_target() {
        let target =
            parse_liveliness_candidate("rdog/lab/daemon/mini-a.lab/member/mini-a.lab/alive")
                .expect("candidate should parse");

        assert_eq!(target.daemon_name, "mini-a.lab");
        assert_eq!(
            target.control_key,
            "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/control"
        );
        assert_eq!(target.keyexpr_root, "rdog");
    }

    #[test]
    fn legacy_liveliness_key_should_parse_back_to_legacy_control_target() {
        let target =
            parse_liveliness_candidate("rcat/lab/daemon/mini-a.lab/member/mini-a.lab/alive")
                .expect("legacy candidate should parse");

        assert_eq!(target.daemon_name, "mini-a.lab");
        assert_eq!(
            target.control_key,
            "rcat/lab/daemon/mini-a.lab/member/mini-a.lab/control"
        );
        assert_eq!(target.keyexpr_root, "rcat");
    }
}
