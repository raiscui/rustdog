// 本模块的用例针对 unixpipe (FIFO) 语义: base 路径推导、probe 超时、compose
// 注入 unixpipe endpoint, 依赖外部 `mkfifo` 与真实 FIFO 文件。被测对象
// (unixpipe_socket_path / try_unixpipe_probe / compose_listen_endpoints 的 unix
// 分支) 在 unixpipe.rs 里全部带 #[cfg(unix)], 因此整块只在 unix 目标编译。
// 落盘布局与 lock/meta 生命周期由 `crate::local_run` 的用例覆盖。
//
// 为什么门加在这里而不是父模块的 `mod tests;` 上: 同 local_default/tests.rs ——
// 本文件是该测试唯一的引入点, 门内聚在测试文件里, 父模块保持 `#[cfg(test)]`。
#![cfg(unix)]

use super::*;
use crate::zenoh_runtime::test_support::{env_test_guard, unique_test_dir};
use std::{env, time::Instant};

#[test]
fn unixpipe_socket_path_should_live_in_the_local_run_dir() {
    let path = unixpipe_socket_path("lab", "mac.lab").expect("路径推导应该成功");
    let run_dir = crate::local_run::run_dir().expect("运行目录应该能解析");

    assert_eq!(path.parent(), Some(run_dir.as_path()));
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("lab__mac.lab.pipe")
    );
}

#[test]
fn unixpipe_socket_path_should_ignore_tmpdir() {
    let _guard = env_test_guard();
    // 旧版本的落盘位置来自 $TMPDIR(per-session,重启即换);现在必须完全不看它。
    let prev = env::var_os("TMPDIR");
    unsafe { env::set_var("TMPDIR", "/tmp/rdog-tmpdir-should-be-ignored") };
    let result = unixpipe_socket_path("lab", "mac.lab");
    match prev {
        Some(v) => unsafe { env::set_var("TMPDIR", v) },
        None => unsafe { env::remove_var("TMPDIR") },
    }

    let path = result.expect("路径推导应该成功");
    assert!(
        !path.starts_with("/tmp/rdog-tmpdir-should-be-ignored"),
        "本机 fast path 不能再跟随 $TMPDIR,实际 {}",
        path.display()
    );
}

#[test]
fn unixpipe_socket_path_should_reject_components_with_slash_or_whitespace() {
    assert!(unixpipe_socket_path("la/b", "mac.lab").is_err());
    assert!(unixpipe_socket_path("lab", "mac lab").is_err());
    assert!(unixpipe_socket_path("", "mac.lab").is_err());
    assert!(unixpipe_socket_path("lab", "").is_err());
}

#[test]
fn unixpipe_socket_path_should_compress_oversized_combination() {
    // 92 字节的 namespace + "mac.lab" 组合会让可读名超出 95 字节上限,
    // 现在必须压缩成 `<slug>-<8位哈希>` 而不是直接失败。
    let big_ns: String = std::iter::repeat('a').take(92).collect();
    let path = unixpipe_socket_path(&big_ns, "mac.lab").expect("超长名字应压缩成哈希名");

    assert!(
        path.as_os_str().as_encoded_bytes().len() <= crate::config::UNIXPIPE_SOCKET_PATH_MAX_BYTES,
        "压缩后的 base 必须在上限内: {}",
        path.display()
    );
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .expect("base 应有文件名主干");
    assert!(stem.starts_with("aaaa"), "应保留可读前缀: {stem}");
    assert_eq!(
        stem.rsplit_once('-').map(|(_, hash)| hash.len()),
        Some(8),
        "应带 8 位哈希后缀: {stem}"
    );
}

#[test]
fn unixpipe_locator_should_format_as_protocol_prefix_and_path() {
    let path = PathBuf::from("/tmp/rdog-lab-mac.lab.pipe");
    assert_eq!(
        unixpipe_locator(&path),
        "unixpipe//tmp/rdog-lab-mac.lab.pipe"
    );
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
    assert!(composed.listen_endpoints[0].contains("lab__mac.lab.pipe"));
    assert_eq!(composed.listen_endpoints[1], "udp/0.0.0.0:7447");
    assert!(composed
        .unixpipe_base_path
        .expect("unixpipe base should be resolved")
        .ends_with("lab__mac.lab.pipe"));
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
fn prepare_unixpipe_listener_should_reclaim_stale_lock_and_fifo_files() {
    // `unique_test_dir` 会读取进程级 TMPDIR,必须与其他环境变量测试串行。
    let _guard = env_test_guard();
    let dir = unique_test_dir("unixpipe-stale-owner");
    let base = dir.join("shared.pipe");
    let lock_path = dir.join("shared.lock");
    let meta_path = dir.join("shared.meta");

    // PID 0 永远不会被识别为活跃进程,用于模拟 daemon 被 kill -9 后的 lock。
    fs::write(
        &lock_path,
        r#"{"schema":"rdog.unixpipe-lock.v1","pid":0,"started_at_unix_ms":1}"#,
    )
    .expect("stale lock should be created");
    for suffix in ["", "_uplink", "_downlink"] {
        fs::write(format!("{}{suffix}", base.display()), "stale")
            .expect("stale unixpipe artifact should be created");
    }

    let guard = prepare_unixpipe_listener(&base, "lab", "stale.lab", true)
        .expect("stale lock and unixpipe files should be recoverable");
    for suffix in ["", "_uplink", "_downlink"] {
        let path = PathBuf::from(format!("{}{suffix}", base.display()));
        assert!(
            !path.exists(),
            "stale file should be removed: {}",
            path.display()
        );
    }
    let lock_contents = fs::read_to_string(&lock_path).expect("new lock should exist");
    assert!(
        lock_contents.contains(&std::process::id().to_string()),
        "新 lock 必须记录当前 pid: {lock_contents}"
    );
    assert!(meta_path.exists(), "meta 应记录完整身份");

    // 正常退出把 pipe/lock/meta 一起清掉,目录不留生命周期碎片。
    drop(guard);
    assert!(!lock_path.exists(), "owner 退出后 lock 应被删除");
    assert!(!meta_path.exists(), "owner 退出后 meta 应被删除");

    let next_guard = prepare_unixpipe_listener(&base, "lab", "stale.lab", true)
        .expect("released lock 不应阻止下一次接管");
    drop(next_guard);
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

// -----------------------------------------------------------------
// 已有单测
// -----------------------------------------------------------------
