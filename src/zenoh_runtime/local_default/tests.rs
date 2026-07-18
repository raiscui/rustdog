use super::*;
use crate::zenoh_runtime::test_support::{env_test_guard, unique_test_dir};
use std::env;

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
    write_local_default_daemon_record(&record_path, &record).expect("registry record 应该能写入");
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
            let lease_guard = register_local_default_daemon(ns, "missing.ldmissup", &missing_base)
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
                find_local_daemon_name(Some(ns)).expect("shared probe应该识别active managed lease"),
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
            let lease_guard = register_local_default_daemon(namespace, "partial.ldpartial", &base)
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
