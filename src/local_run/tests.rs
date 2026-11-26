// 本模块的用例全部围绕本机 fast path 的落盘布局与生命周期: 目录解析、名字派生
// (含超长名字压缩)、lock/meta 语义、死进程残留回收与 `rdog local` 的判定依据。
// 被测对象在 local_run.rs 里全部带 #[cfg(unix)],因此整块只在 unix 目标编译。
#![cfg(unix)]

use super::*;
use std::{
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// 每个用例独占的运行目录 + 旧 `$TMPDIR` 替身,避免并行测试互相污染。
struct TestDirs {
    root: PathBuf,
    run: PathBuf,
    legacy: PathBuf,
}

/// 8 位短随机后缀: 测试目录名要短,长 UUID 会吃掉 95 字节的 pipe 名预算。
fn short_id() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

impl TestDirs {
    fn new(prefix: &str) -> Self {
        // macOS 的 `env::temp_dir()` 是 `/var/folders/xx/.../T/`(约 50 字节),再塞一段
        // 测试目录名就把 95 字节预算吃光,`pipe_stem` 会直接报"预算不足"。这些用例要的
        // 是**短**目录而不是"系统临时目录",所以固定在 `/tmp` 下建;每个用例仍靠随机后缀独占。
        let root = PathBuf::from("/tmp").join(format!("rdog-lr-{prefix}-{}", short_id()));
        let run = root.join("run");
        let legacy = root.join("legacy");
        fs::create_dir_all(&run).expect("test run dir should be created");
        fs::create_dir_all(&legacy).expect("test legacy dir should be created");
        set_run_dir_test_override(Some(run.clone()));
        set_legacy_tmpdir_test_override(Some(legacy.clone()));
        Self { root, run, legacy }
    }
}

impl Drop for TestDirs {
    fn drop(&mut self) {
        set_run_dir_test_override(None);
        set_legacy_tmpdir_test_override(None);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn create_fake_fifos(base: &Path) {
    for path in fifo_paths(base) {
        fs::write(&path, b"fake").expect("fake FIFO should be created");
    }
}

/// PID 0 永远不会被识别为活跃进程,用来模拟 daemon 被 `kill -9` 后的 lock。
fn write_stale_lock(lock: &Path, pid: u32) {
    let contents = serde_json::to_vec(&RunLock {
        schema: LOCK_SCHEMA.to_string(),
        pid,
        started_at_unix_ms: 1,
    })
    .expect("lock should serialize");
    fs::write(lock, contents).expect("stale lock should be written");
}

fn dir_file_names(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .expect("dir should be readable")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

// -----------------------------------------------------------------
// 目录解析
// -----------------------------------------------------------------

#[test]
fn resolve_run_dir_should_prefer_xdg_runtime_dir() {
    let resolved = resolve_run_dir(
        Some(OsStr::new("/run/user/1000")),
        Some(OsStr::new("/home/u")),
        PathBuf::from("/tmp/rdog-run"),
    );
    assert_eq!(resolved, PathBuf::from("/run/user/1000/rdog"));
}

#[test]
fn resolve_run_dir_should_fall_back_to_home() {
    let resolved = resolve_run_dir(
        Some(OsStr::new("")),
        Some(OsStr::new("/home/u")),
        PathBuf::from("/tmp/rdog-run"),
    );
    assert_eq!(resolved, PathBuf::from("/home/u/.rdog/run"));
}

#[test]
fn resolve_run_dir_should_stay_stable_without_home() {
    let resolved = resolve_run_dir(None, None, PathBuf::from("/tmp/rdog-run"));
    assert_eq!(resolved, PathBuf::from("/tmp/rdog-run"));
}

// -----------------------------------------------------------------
// 名字派生
// -----------------------------------------------------------------

#[test]
fn short_identity_should_map_to_readable_stem() {
    let dirs = TestDirs::new("short");
    let paths = unixpipe_run_paths("lab", "mac.lab").expect("paths should resolve");

    assert_eq!(paths.base, dirs.run.join("lab__mac.lab.pipe"));
    assert_eq!(paths.lock, dirs.run.join("lab__mac.lab.lock"));
    assert_eq!(paths.meta, dirs.run.join("lab__mac.lab.meta"));
}

#[test]
fn long_identity_should_compress_and_keep_full_name_in_meta() {
    let _dirs = TestDirs::new("long");
    let namespace = "n".repeat(60);
    let daemon_name = format!("{}.lab", "d".repeat(60));

    let paths = unixpipe_run_paths(&namespace, &daemon_name).expect("long name should resolve");
    let base_bytes = paths.base.as_os_str().as_encoded_bytes().len();
    assert!(
        base_bytes <= crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES,
        "压缩后的 base 必须卡在路径上限内,实际 {base_bytes} 字节: {}",
        paths.base.display()
    );

    let stem = paths
        .base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .expect("base should have a stem");
    let (slug, hash) = stem.rsplit_once('-').expect("压缩名应带哈希后缀");
    assert_eq!(hash.len(), 8, "哈希应为 8 位十六进制: {stem}");
    assert!(hash.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(slug.starts_with("nnnn"), "slug 应保留可读前缀: {slug}");

    let guard = UnixpipeRunGuard::acquire(paths.clone(), &namespace, &daemon_name)
        .expect("compressed identity should acquire the pipe");
    let meta = read_meta(&paths.meta).expect("meta should describe full identity");
    assert_eq!(meta.namespace, namespace);
    assert_eq!(meta.daemon_name, daemon_name);
    assert_eq!(meta.pipe_base, paths.base);

    // `rdog local list` 的数据面必须还原完整名字,而不是只给出哈希。
    let entries = collect_entries().expect("entries should be collected");
    let entry = entries
        .iter()
        .find(|entry| entry.pipe_base == paths.base)
        .expect("compressed entry should be listed");
    assert_eq!(entry.namespace.as_deref(), Some(namespace.as_str()));
    assert_eq!(entry.daemon_name.as_deref(), Some(daemon_name.as_str()));
    drop(guard);
}

#[test]
fn distinct_long_identities_should_get_distinct_hashes() {
    let dirs = TestDirs::new("hash");
    let namespace = "n".repeat(70);
    let first = unixpipe_run_paths(&namespace, &format!("{}.lab", "a".repeat(40))).expect("first");
    let second =
        unixpipe_run_paths(&namespace, &format!("{}.lab", "b".repeat(40))).expect("second");

    assert_ne!(first.base, second.base);
    assert_eq!(first.base.parent(), Some(dirs.run.as_path()));
}

#[test]
fn non_ascii_identity_should_use_compressed_form() {
    let _dirs = TestDirs::new("non-ascii");
    let paths = unixpipe_run_paths("lab", "测试机.lab").expect("non-ascii daemon name should work");

    let stem = paths
        .base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .expect("base should have a stem");
    assert!(
        stem.contains('-'),
        "非 ASCII 名字应走 slug+哈希形态: {stem}"
    );

    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "测试机.lab").expect("acquire");
    let meta = read_meta(&paths.meta).expect("meta should be readable");
    assert_eq!(meta.daemon_name, "测试机.lab");
    drop(guard);
}

// -----------------------------------------------------------------
// 生命周期
// -----------------------------------------------------------------

#[test]
fn second_acquire_should_be_rejected_while_owner_is_alive() {
    let _dirs = TestDirs::new("duplicate");
    let paths = unixpipe_run_paths("lab", "dup.lab").expect("paths");

    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "dup.lab").expect("first acquire");
    let err = UnixpipeRunGuard::acquire(paths, "lab", "dup.lab")
        .expect_err("second daemon must be rejected");

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        err.to_string().contains("已被活跃 daemon 占用"),
        "second instance 应得到明确占用错误: {err}"
    );
    drop(guard);
}

#[test]
fn stale_lock_should_be_reclaimed_on_next_start() {
    let _dirs = TestDirs::new("stale");
    let paths = unixpipe_run_paths("lab", "crash.lab").expect("paths");
    create_fake_fifos(&paths.base);
    write_stale_lock(&paths.lock, 0);

    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "crash.lab")
        .expect("stale owner must be reclaimable");

    for path in fifo_paths(&paths.base) {
        assert!(!path.exists(), "残留 FIFO 应被清理: {}", path.display());
    }
    let lock = read_lock(&mut open_existing_lock_file(&paths.lock).unwrap().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(lock.pid, std::process::id());
    drop(guard);
}

#[test]
fn drop_should_remove_pipe_lock_and_meta() {
    let dirs = TestDirs::new("drop");
    let paths = unixpipe_run_paths("lab", "exit.lab").expect("paths");

    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "exit.lab").expect("acquire");
    create_fake_fifos(&paths.base);
    drop(guard);

    assert!(!paths.lock.exists(), "正常退出应删除 lock");
    assert!(!paths.meta.exists(), "正常退出应删除 meta");
    for path in fifo_paths(&paths.base) {
        assert!(!path.exists(), "正常退出应删除 FIFO: {}", path.display());
    }
    assert!(
        dir_file_names(&dirs.run).is_empty(),
        "正常退出后运行目录应为空: {:?}",
        dir_file_names(&dirs.run)
    );
}

#[test]
fn twenty_start_stop_cycles_should_not_accumulate_files() {
    let dirs = TestDirs::new("cycles");
    let paths = unixpipe_run_paths("lab", "cycle.lab").expect("paths");

    for round in 0..20 {
        let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "cycle.lab")
            .unwrap_or_else(|err| panic!("第 {round} 轮启动失败: {err}"));
        create_fake_fifos(&paths.base);
        assert_eq!(
            dir_file_names(&dirs.run),
            vec![
                "lab__cycle.lab.lock".to_string(),
                "lab__cycle.lab.meta".to_string(),
                "lab__cycle.lab.pipe_downlink".to_string(),
                "lab__cycle.lab.pipe_uplink".to_string(),
            ],
            "第 {round} 轮: 1 逻辑 pipe(Zenoh 派生 2 个 FIFO) + 1 lock + 1 meta,不能多也不能少"
        );
        drop(guard);
        assert!(
            dir_file_names(&dirs.run).is_empty(),
            "第 {round} 轮退出后不应留残留: {:?}",
            dir_file_names(&dirs.run)
        );
    }
}

#[test]
fn restrict_fifo_permissions_should_tighten_fifo_and_dir_modes() {
    let dirs = TestDirs::new("perms");
    let paths = unixpipe_run_paths("lab", "perm.lab").expect("paths");
    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "perm.lab").expect("acquire");
    create_fake_fifos(&paths.base);

    guard
        .restrict_fifo_permissions()
        .expect("chmod of existing FIFO should succeed");

    for path in fifo_paths(&paths.base) {
        let mode = fs::metadata(&path)
            .expect("fifo metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "FIFO 应为 0600: {}", path.display());
    }
    let dir_mode = fs::metadata(&dirs.run)
        .expect("run dir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700, "运行目录应为 0700");
    drop(guard);
}

#[test]
fn remove_unixpipe_artifacts_should_reject_directory() {
    let dirs = TestDirs::new("dir-guard");
    let base = dirs.run.join("as-directory.pipe");
    fs::create_dir_all(&base).expect("directory should be created");

    let err = remove_unixpipe_artifacts(&base).expect_err("目录不能被当残留删掉");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(base.is_dir(), "拒绝清理时目录必须原样保留");
}

// -----------------------------------------------------------------
// 观测与回收
// -----------------------------------------------------------------

#[test]
fn listing_should_not_create_files() {
    let dirs = TestDirs::new("readonly");
    let entries = collect_entries().expect("empty run dir should collect nothing");
    assert!(entries.is_empty());
    assert!(
        dir_file_names(&dirs.run).is_empty(),
        "`rdog local list` 不能在运行目录里留东西: {:?}",
        dir_file_names(&dirs.run)
    );
}

#[test]
fn live_entry_should_report_owner_alive_and_pid_alive() {
    let _dirs = TestDirs::new("verdict-live");
    let paths = unixpipe_run_paths("lab", "alive.lab").expect("paths");
    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "alive.lab").expect("acquire");
    create_fake_fifos(&paths.base);

    let entries = collect_entries().expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.pipe_base == paths.base)
        .expect("live entry should be listed");
    assert!(entry.owner_alive);
    assert_eq!(entry.pid, Some(std::process::id()));
    assert_eq!(entry.pid_alive, Some(true));
    assert_eq!(entry.fifo_files, 2);
    assert_eq!(entry.verdict(), "live");
    drop(guard);
}

#[test]
fn dead_pid_entry_should_report_pid_not_alive() {
    let _dirs = TestDirs::new("verdict-dead");
    let paths = unixpipe_run_paths("lab", "dead.lab").expect("paths");
    create_fake_fifos(&paths.base);
    write_stale_lock(&paths.lock, 0);

    let entries = collect_entries().expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.pipe_base == paths.base)
        .expect("stale entry should be listed");
    assert!(!entry.owner_alive);
    assert_eq!(entry.pid, Some(0));
    assert_eq!(
        entry.pid_alive,
        Some(false),
        "回执要给出 pid 不存在的判定依据"
    );
    assert_eq!(entry.verdict(), "stale-pid-dead");
}

#[test]
fn clean_should_only_remove_stale_entries() {
    let _dirs = TestDirs::new("clean");
    let live_paths = unixpipe_run_paths("lab", "alive.lab").expect("live paths");
    let live = UnixpipeRunGuard::acquire(live_paths.clone(), "lab", "alive.lab").expect("acquire");
    create_fake_fifos(&live_paths.base);

    let stale_paths = unixpipe_run_paths("lab", "dead.lab").expect("stale paths");
    create_fake_fifos(&stale_paths.base);
    write_stale_lock(&stale_paths.lock, 0);

    let dry = clean_stale_entries(true).expect("dry run should work");
    assert_eq!(dry.stale.len(), 1);
    assert_eq!(dry.removed_entries, 0);
    assert!(stale_paths.lock.exists(), "dry-run 不能删任何文件");

    let report = clean_stale_entries(false).expect("clean should work");
    assert_eq!(report.removed_entries, 1);
    assert!(report.removed_files >= 3, "应删掉 FIFO + lock: {report:?}");
    assert!(!stale_paths.lock.exists());
    for path in fifo_paths(&stale_paths.base) {
        assert!(!path.exists(), "残留 FIFO 应被删除: {}", path.display());
    }

    // 活 daemon 的 pipe 无论 dry-run 还是 --force 都不能动。
    assert!(live_paths.lock.exists());
    for path in fifo_paths(&live_paths.base) {
        assert!(
            path.exists(),
            "活 daemon 的 FIFO 不能删: {}",
            path.display()
        );
    }
    drop(live);
}

#[test]
fn legacy_tmpdir_artifacts_should_be_listed_and_cleanable() {
    let dirs = TestDirs::new("legacy");
    let legacy_base = dirs.legacy.join("rdog-lab-old.lab.pipe");
    create_fake_fifos(&legacy_base);

    let entries = collect_entries().expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.pipe_base == legacy_base)
        .expect("旧 TMPDIR 残留应被列出来");
    assert!(entry.legacy_location);
    assert_eq!(entry.namespace.as_deref(), Some("lab"));
    assert_eq!(entry.daemon_name.as_deref(), Some("old.lab"));
    assert_eq!(entry.verdict(), "legacy-stale");

    let report = clean_stale_entries(false).expect("clean should work");
    assert_eq!(report.removed_entries, 1);
    for path in fifo_paths(&legacy_base) {
        assert!(!path.exists(), "旧位置残留应被删除: {}", path.display());
    }
}

// -----------------------------------------------------------------
// 显式 base / 跨版本保护 / 竞态
// -----------------------------------------------------------------

#[test]
fn explicit_base_should_be_preserved_with_sidecars_beside_it() {
    let paths = paths_for_base(Path::new("/var/tmp/control.sock")).expect("paths");
    assert_eq!(
        paths.base,
        PathBuf::from("/var/tmp/control.sock"),
        "显式 base 必须原样保留: 改写扩展名会让清理落到另一批文件上"
    );
    assert_eq!(paths.lock, PathBuf::from("/var/tmp/control.lock"));
    assert_eq!(paths.meta, PathBuf::from("/var/tmp/control.meta"));
}

#[test]
fn explicit_base_should_not_tighten_shared_parent_permissions() {
    let dirs = TestDirs::new("shared");
    let shared = dirs.root.join("shared");
    fs::create_dir_all(&shared).expect("shared dir should be created");
    fs::set_permissions(&shared, Permissions::from_mode(0o755)).expect("chmod shared dir");

    let base = shared.join("control.sock");
    let guard =
        UnixpipeRunGuard::acquire(paths_for_base(&base).expect("paths"), "lab", "explicit.lab")
            .expect("explicit base should acquire");

    let mode = fs::metadata(&shared)
        .expect("shared dir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o755,
        "显式路径的父目录不是 rdog 运行目录,不能被收紧权限"
    );
    drop(guard);
}

#[test]
fn live_legacy_owner_should_block_new_acquisition_and_clean() {
    let dirs = TestDirs::new("legacy");
    let base = dirs.run.join("lab__legacy.lab.pipe");
    create_fake_fifos(&base);

    // 旧二进制持有的路径守卫: 同一套 flock 语义,只是文件名不同。
    let legacy_guard = legacy_guard_path(&base);
    let legacy_lease = process_lease::ProcessLease::acquire(
        legacy_guard.clone(),
        process_lease::metadata_path_for_lock(&legacy_guard),
        "unixpipe-path",
        "legacy",
    )
    .expect("legacy guard should be held");

    let entries = collect_entries().expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.pipe_base == base)
        .expect("legacy-owned entry should be listed");
    assert!(entry.owner_alive, "旧版本 owner 仍持锁时必须算活的");
    // 条目本身在运行目录里(新命名),所以 verdict 是 `live`;关键在于它算活的,
    // 不会被 `clean` 当残留删掉。
    assert_eq!(entry.verdict(), "live");

    let err = UnixpipeRunGuard::acquire(paths_for_base(&base).expect("paths"), "lab", "new.lab")
        .expect_err("旧版本 owner 还在跑时,新二进制不能接管同一个 base");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        err.to_string().contains("旧版本 daemon"),
        "错误要指明跨版本占用: {err}"
    );

    let report = clean_stale_entries(false).expect("clean should run");
    assert_eq!(report.removed_entries, 0, "活 owner 的残留不能删");
    for path in fifo_paths(&base) {
        assert!(
            path.exists(),
            "活 owner 的 FIFO 不能被删: {}",
            path.display()
        );
    }

    drop(legacy_lease);
}

#[test]
fn removal_should_be_skipped_when_a_new_owner_takes_the_lock() {
    let dirs = TestDirs::new("race");
    let base = dirs.run.join("lab__race.lab.pipe");
    create_fake_fifos(&base);
    let paths = paths_for_base(&base).expect("paths");
    write_stale_lock(&paths.lock, 0);

    // 模拟"分类为残留之后、删除之前,新 daemon 抢到了锁"。
    let owner = process_lease::ProcessLease::acquire(
        paths.lock.clone(),
        process_lease::metadata_path_for_lock(&paths.lock),
        "unixpipe-run",
        "race",
    )
    .expect("new owner should take the lock");

    let stale_entry = RunEntry {
        pipe_base: base.clone(),
        namespace: Some("lab".to_string()),
        daemon_name: Some("race.lab".to_string()),
        pid: Some(0),
        fifo_files: 2,
        owner_alive: false,
        pid_alive: Some(false),
        legacy_location: false,
        started_at_unix_ms: Some(1),
    };

    assert_eq!(
        remove_stale_entry(&stale_entry).expect("removal should not error"),
        None,
        "抢不到锁就必须放弃删除"
    );
    for path in fifo_paths(&base) {
        assert!(path.exists(), "新 owner 的 FIFO 不能删: {}", path.display());
    }

    drop(owner);
}

#[test]
fn new_owner_should_block_an_old_binary_on_the_same_base() {
    let dirs = TestDirs::new("compat");
    // 兼容守卫只作用于"共享显式 base"(运行目录之外): 派生路径旧二进制算不出来。
    let base = dirs.root.join("shared").join("control.sock");
    fs::create_dir_all(base.parent().expect("parent")).expect("parent dir");

    let guard =
        UnixpipeRunGuard::acquire(paths_for_base(&base).expect("paths"), "lab", "compat.lab")
            .expect("new owner should acquire");

    // 旧二进制会在同一个 `<base>.rdog-owner.pid` 上抢锁并读 pid:
    // 抢不到锁 + pid 存活 = 拒绝接管,不会去 unlink 新 daemon 的 FIFO。
    let legacy_guard = legacy_guard_path(&base);
    let err = process_lease::ProcessLease::acquire(
        legacy_guard.clone(),
        process_lease::metadata_path_for_lock(&legacy_guard),
        "unixpipe-path",
        "shared",
    )
    .expect_err("旧二进制不能在活着的新 owner 上抢到路径守卫");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        err.to_string().contains("已被活跃") || err.to_string().contains("legacy"),
        "旧二进制应看到活跃 owner: {err}"
    );

    drop(guard);
    // 退出后守卫文件不残留,旧二进制可以正常接管。
    assert!(!legacy_guard.exists(), "新 owner 退出后兼容守卫不该留下");
}

#[test]
fn derived_base_should_not_create_a_legacy_compat_guard() {
    let dirs = TestDirs::new("derived");
    let paths = unixpipe_run_paths("lab", "derived.lab").expect("paths");
    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "derived.lab").expect("acquire");

    assert!(
        !legacy_guard_path(&paths.base).exists(),
        "派生路径不该留旧格式守卫: 旧二进制推导不出这条路,没有跨版本冲突"
    );
    assert!(!base_is_shared(&paths.base));
    drop(guard);
    assert!(
        dir_file_names(&dirs.run).is_empty(),
        "派生路径退出后目录应为空: {:?}",
        dir_file_names(&dirs.run)
    );
}

#[test]
fn legacy_removal_should_be_skipped_while_old_guard_is_held() {
    let dirs = TestDirs::new("legacy-race");
    let legacy_base = dirs.legacy.join("rdog-lab-race.lab.pipe");
    create_fake_fifos(&legacy_base);
    let legacy_guard = legacy_guard_path(&legacy_base);

    let holder = process_lease::ProcessLease::acquire(
        legacy_guard.clone(),
        process_lease::metadata_path_for_lock(&legacy_guard),
        "unixpipe-path",
        "legacy-race",
    )
    .expect("old daemon takes the guard");

    let entry = RunEntry {
        pipe_base: legacy_base.clone(),
        namespace: Some("lab".to_string()),
        daemon_name: Some("race.lab".to_string()),
        pid: Some(std::process::id()),
        fifo_files: 2,
        owner_alive: false,
        pid_alive: Some(true),
        legacy_location: true,
        started_at_unix_ms: None,
    };
    assert_eq!(
        remove_stale_entry(&entry).expect("removal should not error"),
        None,
        "旧守卫被持有时必须放弃删除"
    );
    for path in fifo_paths(&legacy_base) {
        assert!(path.exists(), "活 owner 的 FIFO 不能删: {}", path.display());
    }

    drop(holder);
}

#[test]
fn unmanaged_candidates_should_report_identity_for_diagnostics() {
    let _dirs = TestDirs::new("candidates");
    let paths = unixpipe_run_paths("lab", "unmanaged.lab").expect("paths");
    let guard = UnixpipeRunGuard::acquire(paths.clone(), "lab", "unmanaged.lab").expect("acquire");
    create_fake_fifos(&paths.base);

    assert_eq!(
        unmanaged_pipe_candidates(Some("lab")).expect("candidates"),
        vec!["lab/unmanaged.lab".to_string()]
    );
    assert!(unmanaged_pipe_candidates(Some("other"))
        .expect("candidates")
        .is_empty());
    drop(guard);
}
