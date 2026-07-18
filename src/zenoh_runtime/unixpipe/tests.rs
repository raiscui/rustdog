use super::*;
use crate::zenoh_runtime::test_support::{env_test_guard, unique_test_dir};
use std::{env, time::Instant};

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
    // `unique_test_dir`会读取进程级TMPDIR,必须与其他环境变量测试串行。
    let _guard = env_test_guard();
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
