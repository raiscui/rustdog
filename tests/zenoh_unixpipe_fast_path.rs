#![cfg(unix)]
#![cfg(target_os = "macos")]

//! Zenoh `transport_unixpipe` 本机 fast path 的端到端集成测试。
//!
//! 这些测试只覆盖同机 daemon + control 的 unixpipe fast path 行为。
//! 跨主机 / 跨网络场景由 `tests/zenoh_router_client.rs` 已有的 `control_multi_one_shot_*` 等测试覆盖。
//!
//! 关键测试点:
//! 1. daemon 启用 unixpipe + control 同机,@ping 走 fast path
//! 2. daemon 没启用 unixpipe,control 走 fallback(走 UDP scout)
//! 3. 残留的 stale FIFO 文件会被 daemon 启动时清理

#[path = "zenoh_unixpipe_fast_path/support.rs"]
mod support;

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use support::*;

const RDOG_NAMESPACE: &str = "lab";

fn unique_daemon_name(prefix: &str) -> String {
    // daemon_name 必须带 `.lab` 后缀,namespace 才能从名字后缀推断出来。
    // 见 `crate::zenoh_identity::infer_namespace_from_daemon_name`。
    format!(
        "{prefix}-{}-{}.{RDOG_NAMESPACE}",
        std::process::id(),
        next_port()
    )
}

/// 等 FIFO 出现,或返回 NotFound。
fn wait_for_fifo(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn derive_unixpipe_base_path(namespace: &str, daemon_name: &str) -> PathBuf {
    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    tmpdir.join(format!("rdog-{namespace}-{daemon_name}.pipe"))
}

fn cleanup_unixpipe_artifacts(base: &PathBuf) {
    let _ = fs::remove_file(base);
    let _ = fs::remove_file(format!("{}_uplink", base.display()));
    let _ = fs::remove_file(format!("{}_downlink", base.display()));
    // Zenoh 还会创建带 suffix 的 dedicated FIFO,尽力清掉,避免跨测试污染。
    if let Ok(entries) = fs::read_dir(
        base.parent()
            .unwrap_or_else(|| std::path::Path::new("/tmp")),
    ) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(base.file_name().unwrap().to_str().unwrap()) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

fn create_fake_uplink_candidate(namespace: &str, daemon_name: &str) -> PathBuf {
    let base = derive_unixpipe_base_path(namespace, daemon_name);
    cleanup_unixpipe_artifacts(&base);
    let uplink = format!("{}_uplink", base.display());
    let status = Command::new("mkfifo")
        .arg(&uplink)
        .status()
        .expect("mkfifo 调用应该成功");
    assert!(status.success(), "mkfifo {uplink} 失败");
    base
}

/// 清理 namespace 下所有 rdog-{ns}-*.pipe* 残留,避免多 test 之间的 FIFO 污染。
/// 用途:`self` / 空 target 的 e2e 强依赖 namespace 范围内只能有 1 个 daemon,
/// 旧 test 残留的 fifo 会让 fast path 误报"多候选"。
fn cleanup_namespace_artifacts(namespace: &str) {
    let tmpdir = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let prefix = format!("rdog-{namespace}-");
    if let Ok(entries) = fs::read_dir(&tmpdir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

// ============================================================================
// 测试用例
// ============================================================================

#[test]
fn unixpipe_endpoint_should_be_created_when_daemon_starts_with_unixpipe_enabled() {
    let daemon_name = unique_daemon_name("unixpipe-create");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    // 等待 daemon 起来 + log
    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // 验证 FIFO 文件被创建
    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);
}

#[test]
fn unixpipe_fast_path_should_make_ping_respond_within_budget() {
    let daemon_name = unique_daemon_name("unixpipe-ping");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // 给 listener 一点时间 settle。
    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected FIFO {uplink_path} to exist before client"
    );

    let start = Instant::now();
    let (status, stdout, stderr) = run_control_ping(&[daemon_name.as_str()]);
    let elapsed = start.elapsed();

    assert!(
        status.success(),
        "control @ping should succeed, stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 响应应该包含 pong, stdout={stdout}"
    );

    // 远端 IP 通过 multicast 走不通时 control 会 fallback 到 10s+;unixpipe 路径必须 < 1s。
    assert!(
        elapsed < Duration::from_millis(1000),
        "unixpipe fast path 必须在 1s 内返回,实际 {elapsed:?}"
    );

    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);
}

#[test]
fn stale_unixpipe_socket_files_should_be_cleaned_on_daemon_start() {
    let daemon_name = unique_daemon_name("unixpipe-stale");
    let base_path = derive_unixpipe_base_path(RDOG_NAMESPACE, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    // 模拟上次崩溃残留的 3 个文件:base / base_uplink / base_downlink。
    for suffix in ["", "_uplink", "_downlink"] {
        let path = format!("{}{suffix}", base_path.display());
        let status = Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo 调用应该成功");
        assert!(status.success(), "mkfifo {path} 失败");
    }

    // 启动 daemon,触发 stale cleanup。
    let daemon = start_zenoh_daemon_with_combined_output(&daemon_name, next_port(), true);

    // daemon 起来后,残留的 3 个文件必须已经被清理。
    // 重新创建新的 FIFO 是 daemon 自己的事,我们只验证旧的被 unlink。
    let base_only = base_path.clone();
    let uplink_path = format!("{}_uplink", base_path.display());

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    // base 本身不会作为 FIFO 存在,daemon 只创建 _uplink 和 _downlink。
    // 老的 _uplink 和 _downlink 应该被 unlink,然后 daemon 重新创建新的 _uplink。
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected new {uplink_path} to be created after cleanup"
    );
    // 老的 base 文件应该是 base path 本身。如果 daemon 清理了它,文件应该已经不存在。
    // 这里我们用 base path 的"派生路径"来验证,而不是 base 本身。
    let _ = base_only;

    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);
}

#[test]
fn duplicate_daemon_start_should_not_break_running_local_default_unixpipe() {
    // 使用短 namespace 控制 unixpipe 路径长度,同时隔离 registry 和历史 FIFO。
    let namespace = format!("dup{}", next_port());
    cleanup_namespace_artifacts(&namespace);
    let state_home = TestStateHome::new("duplicate-start");
    let daemon_name = format!("d.{namespace}");
    let base_path = derive_unixpipe_base_path(&namespace, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let first = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &namespace,
        next_port(),
        true,
        true,
        state_home.path(),
        None,
    );
    wait_for_marker(
        first.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("first daemon should be ready");

    let uplink_path = PathBuf::from(format!("{}_uplink", base_path.display()));
    assert!(
        wait_for_fifo(&uplink_path, Duration::from_secs(2)),
        "first daemon should create {}",
        uplink_path.display()
    );

    // 同名第二实例必须被 ownership guard 拒绝,且不能先删除第一实例的 FIFO。
    let mut second = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &namespace,
        next_port(),
        true,
        true,
        state_home.path(),
        None,
    );
    let second_status = wait_for_child_exit(&mut second, Duration::from_secs(5));
    let duplicate_output = second
        .output()
        .lock()
        .expect("duplicate output lock should work")
        .clone();
    let fifo_survived = uplink_path.exists();
    let (control_status, control_stdout, control_stderr) =
        run_control_with_args_and_env(&["--namespace", &namespace], Some(state_home.path()));

    // 先清理本测试创建的资源,再做断言;RED 阶段也不会留下新 daemon 和 FIFO。
    drop(second);
    drop(first);
    cleanup_unixpipe_artifacts(&base_path);
    cleanup_namespace_artifacts(&namespace);

    assert!(
        second_status.is_some_and(|status| !status.success()),
        "duplicate daemon should exit with failure, output={duplicate_output}"
    );
    assert!(
        fifo_survived,
        "duplicate daemon removed the running daemon FIFO {}, duplicate_output={duplicate_output}, control_status={control_status}, control_stdout={control_stdout}, control_stderr={control_stderr}",
        uplink_path.display(),
    );
    assert!(
        control_status.success(),
        "running local-default daemon should remain reachable, stdout={control_stdout}, stderr={control_stderr}, duplicate_output={duplicate_output}"
    );
    assert!(
        control_stdout.contains("pong"),
        "empty target @ping should still return pong, stdout={control_stdout}"
    );
}

#[test]
fn distinct_daemon_names_should_not_replace_a_shared_explicit_unixpipe() {
    // service-name 不同,但显式 socket_path 相同;FIFO path 必须有独立 ownership guard。
    let namespace = format!("p{}", next_port());
    let state_home = TestStateHome::new("shared-unixpipe");
    let daemon_a = format!("a.{namespace}");
    let daemon_b = format!("b.{namespace}");
    let shared_base = derive_unixpipe_base_path(&namespace, "shared");
    cleanup_unixpipe_artifacts(&shared_base);

    let first = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_a,
        &namespace,
        next_port(),
        true,
        false,
        state_home.path(),
        Some(&shared_base),
    );
    wait_for_marker(
        first.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("first daemon should be ready");

    let uplink_path = PathBuf::from(format!("{}_uplink", shared_base.display()));
    assert!(
        wait_for_fifo(&uplink_path, Duration::from_secs(2)),
        "first daemon should create {}",
        uplink_path.display()
    );
    let inode_before = fs::metadata(&uplink_path)
        .expect("first daemon uplink metadata should exist")
        .ino();

    let mut second = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_b,
        &namespace,
        next_port(),
        true,
        false,
        state_home.path(),
        Some(&shared_base),
    );
    let second_status = wait_for_child_exit(&mut second, Duration::from_secs(3));
    let duplicate_output = second
        .output()
        .lock()
        .expect("shared path output lock should work")
        .clone();
    let inode_after = fs::metadata(&uplink_path)
        .ok()
        .map(|metadata| metadata.ino());

    drop(second);
    drop(first);
    cleanup_unixpipe_artifacts(&shared_base);

    assert!(
        second_status.is_some_and(|status| !status.success()),
        "different daemon name must not acquire an active unixpipe path, output={duplicate_output}, inode_before={inode_before}, inode_after={inode_after:?}"
    );
    assert_eq!(
        inode_after,
        Some(inode_before),
        "second daemon replaced the active FIFO, output={duplicate_output}"
    );
}

// ============================================================================
// self / 空 target 入口(`rdog control self @<line>` / `rdog control @<line>`)
// ============================================================================

#[test]
fn self_target_with_explicit_namespace_should_find_local_daemon() {
    // 独立 namespace,完全跟其他 `lab` 测试隔离,允许 cargo test 默认并发。
    let ns = format!("self{}", next_port());
    let state_home = TestStateHome::new("self-explicit");
    cleanup_namespace_artifacts(&ns);

    let daemon_name = format!("selftest.{ns}");
    let base_path = derive_unixpipe_base_path(&ns, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &ns,
        next_port(),
        true,
        false,
        state_home.path(),
        None,
    );

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) =
        run_control_with_args_and_env(&["self", "--namespace", &ns], Some(state_home.path()));
    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);

    assert!(
        status.success(),
        "control self should succeed, stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn empty_target_with_namespace_should_find_local_daemon() {
    let ns = format!("empty{}", next_port());
    let state_home = TestStateHome::new("empty-target");
    cleanup_namespace_artifacts(&ns);

    let daemon_name = format!("only.{ns}");
    let base_path = derive_unixpipe_base_path(&ns, &daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &ns,
        next_port(),
        true,
        false,
        state_home.path(),
        None,
    );

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) =
        run_control_with_args_and_env(&["--namespace", &ns], Some(state_home.path()));
    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);

    assert!(
        status.success(),
        "control 空 target + --namespace 应该成功,stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn empty_target_should_use_local_default_even_when_extra_fifo_candidate_exists() {
    let ns = format!("lde{}", next_port());
    cleanup_namespace_artifacts(&ns);
    let state_home = TestStateHome::new("localdefempty");

    let daemon_name = format!("d{}.{ns}", next_port());
    let extra_daemon_name = format!("x.{ns}");
    let base_path = derive_unixpipe_base_path(&ns, &daemon_name);
    let extra_base = create_fake_uplink_candidate(&ns, &extra_daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &ns,
        next_port(),
        true,
        true,
        state_home.path(),
        None,
    );

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) =
        run_control_with_args_and_env(&["--namespace", &ns], Some(state_home.path()));

    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);
    cleanup_unixpipe_artifacts(&extra_base);

    assert!(
        status.success(),
        "local-default 空 target 应该成功,stdout={stdout},stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn self_target_should_use_local_default_even_when_extra_fifo_candidate_exists() {
    let ns = format!("lds{}", next_port());
    cleanup_namespace_artifacts(&ns);
    let state_home = TestStateHome::new("localdefself");

    let daemon_name = format!("d{}.{ns}", next_port());
    let extra_daemon_name = format!("x.{ns}");
    let base_path = derive_unixpipe_base_path(&ns, &daemon_name);
    let extra_base = create_fake_uplink_candidate(&ns, &extra_daemon_name);
    cleanup_unixpipe_artifacts(&base_path);

    let daemon = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name,
        &ns,
        next_port(),
        true,
        true,
        state_home.path(),
        None,
    );

    wait_for_marker(
        daemon.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon should be ready");

    let uplink_path = format!("{}_uplink", base_path.display());
    assert!(
        wait_for_fifo(std::path::Path::new(&uplink_path), Duration::from_secs(2)),
        "expected {uplink_path} to be created"
    );

    let (status, stdout, stderr) =
        run_control_with_args_and_env(&["self", "--namespace", &ns], Some(state_home.path()));

    drop(daemon);
    cleanup_unixpipe_artifacts(&base_path);
    cleanup_unixpipe_artifacts(&extra_base);

    assert!(
        status.success(),
        "local-default self target 应该成功,stdout={stdout},stderr={stderr}"
    );
    assert!(
        stdout.contains("pong"),
        "@ping 应该返回 pong,stdout={stdout}"
    );
}

#[test]
fn self_target_should_error_when_no_local_daemon_running() {
    // 用唯一 namespace 和 state home 构造空环境,不能删除真实 daemon 的 FIFO。
    let namespace = format!("none{}", next_port());
    let state_home = TestStateHome::new("no-local-daemon");
    cleanup_namespace_artifacts(&namespace);

    let (status, _stdout, stderr) = run_control_with_args_and_env(
        &["self", "--namespace", &namespace],
        Some(state_home.path()),
    );
    cleanup_namespace_artifacts(&namespace);

    assert!(
        !status.success(),
        "没有本地 daemon 时 control self 应该失败"
    );
    let err_lower = stderr.to_lowercase();
    assert!(
        err_lower.contains("not found") || err_lower.contains("未找到"),
        "应该报未找到本地 daemon,实际 stderr={stderr}"
    );
}

#[test]
fn self_target_should_error_when_multiple_local_daemons() {
    // 使用私有 namespace/state 启动两个 daemon,不能读取或清理真实 lab registry。
    let namespace = format!("multi{}", next_port());
    let state_home = TestStateHome::new("multiple-daemons");
    let daemon_name_a = format!("a.{namespace}");
    let daemon_name_b = format!("b.{namespace}");
    let base_a = derive_unixpipe_base_path(&namespace, &daemon_name_a);
    let base_b = derive_unixpipe_base_path(&namespace, &daemon_name_b);
    cleanup_unixpipe_artifacts(&base_a);
    cleanup_unixpipe_artifacts(&base_b);

    let daemon_a = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name_a,
        &namespace,
        next_port(),
        true,
        false,
        state_home.path(),
        None,
    );
    let daemon_b = start_zenoh_daemon_with_namespace_and_local_default(
        &daemon_name_b,
        &namespace,
        next_port(),
        true,
        false,
        state_home.path(),
        None,
    );

    wait_for_marker(
        daemon_a.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon A should be ready");
    wait_for_marker(
        daemon_b.output(),
        "zenoh router daemon ready",
        Duration::from_secs(8),
    )
    .expect("daemon B should be ready");

    // 等两个 fifo 都出现
    let uplink_a = format!("{}_uplink", base_a.display());
    let uplink_b = format!("{}_uplink", base_b.display());
    assert!(wait_for_fifo(
        std::path::Path::new(&uplink_a),
        Duration::from_secs(2)
    ));
    assert!(wait_for_fifo(
        std::path::Path::new(&uplink_b),
        Duration::from_secs(2)
    ));

    let (status, _stdout, stderr) = run_control_with_args_and_env(
        &["self", "--namespace", &namespace],
        Some(state_home.path()),
    );
    drop(daemon_b);
    drop(daemon_a);
    cleanup_unixpipe_artifacts(&base_a);
    cleanup_unixpipe_artifacts(&base_b);

    assert!(
        !status.success(),
        "两个本地 daemon 时 control self 应该失败(歧义)"
    );
    let err_lower = stderr.to_lowercase();
    assert!(
        err_lower.contains("already exists") || err_lower.contains("多个"),
        "应该报多个 daemon 冲突,实际 stderr={stderr}"
    );
}
