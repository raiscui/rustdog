use std::{io, path::PathBuf};

use super::{
    find_local_daemon, find_local_daemon_by_port, loopback_zenoh_endpoints, register_local_daemon,
    set_local_daemon_test_dir, LocalControlLane,
};

/// 测试用隔离目录: 登记表按 HOME 落盘,不隔离就会读到开发机上正在跑的 daemon。
struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rdog-local-daemon-{prefix}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("test registry dir should be created");
        set_local_daemon_test_dir(Some(path.clone()));
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        set_local_daemon_test_dir(None);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn zenoh_lane(port: u16) -> LocalControlLane {
    LocalControlLane::Zenoh {
        connect_endpoints: vec![format!("tcp/127.0.0.1:{port}")],
    }
}

#[test]
fn registered_daemon_should_be_found_and_removed_after_guard_drop() {
    let _dir = TestDir::new("roundtrip");

    let guard = register_local_daemon("lab", "win-test.lab", zenoh_lane(17480))
        .expect("registration should succeed");
    let record = find_local_daemon(None).expect("freshly registered daemon should be visible");
    assert_eq!(record.namespace, "lab");
    assert_eq!(record.daemon_name, "win-test.lab");
    assert_eq!(
        record.lane,
        LocalControlLane::Zenoh {
            connect_endpoints: vec!["tcp/127.0.0.1:17480".to_string()]
        }
    );

    drop(guard);

    let err = find_local_daemon(None).expect_err("guard drop should revoke the record");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn stale_record_left_by_dead_owner_should_be_ignored_and_cleaned() {
    let dir = TestDir::new("stale");

    // 模拟 kill -9: 只留记录文件,没有活着的 lock owner。
    let record_path = dir.path.join("lab__ghost.lab.json");
    std::fs::write(
        &record_path,
        serde_json::to_vec(&serde_json::json!({
            "schema": super::LOCAL_DAEMON_SCHEMA,
            "namespace": "lab",
            "daemon_name": "ghost.lab",
            "pid": 424242,
            "lane": {"kind": "line-tcp", "host": "127.0.0.1", "port": 5555},
            "created_at_unix_ms": 1,
        }))
        .expect("record should serialize"),
    )
    .expect("record should be written");

    let err = find_local_daemon(None).expect_err("dead owner must not be reported as a daemon");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(
        !record_path.exists(),
        "stale record should be pruned during lookup"
    );
}

#[test]
fn multiple_registered_daemons_should_report_ambiguity_instead_of_picking_one() {
    let _dir = TestDir::new("ambiguous");
    let _a = register_local_daemon("lab", "a.lab", zenoh_lane(17480)).expect("a should register");
    let _b = register_local_daemon("lab", "b.lab", zenoh_lane(17481)).expect("b should register");

    let err = find_local_daemon(None).expect_err("two daemons must be ambiguous");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    let message = err.to_string();
    assert!(message.contains("a.lab") && message.contains("b.lab"), "{message}");

    let filtered = find_local_daemon(Some("lab")).expect_err("namespace alone is still ambiguous");
    assert_eq!(filtered.kind(), io::ErrorKind::AlreadyExists);

    let other = find_local_daemon(Some("other")).expect_err("unknown namespace has no daemon");
    assert_eq!(other.kind(), io::ErrorKind::NotFound);
}

#[test]
fn duplicate_identity_registration_should_be_rejected() {
    let _dir = TestDir::new("duplicate");
    let _first = register_local_daemon("lab", "dup.lab", zenoh_lane(17480)).expect("first wins");

    let err = register_local_daemon("lab", "dup.lab", zenoh_lane(17481))
        .expect_err("duplicate identity must not silently overwrite");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn port_lookup_should_match_the_registered_lane_only() {
    let _dir = TestDir::new("by-port");
    let _daemon = register_local_daemon("lab", "win-test.lab", zenoh_lane(17480))
        .expect("registration should succeed");
    let _tcp_daemon = register_local_daemon(
        "lab",
        "tcp-test.lab",
        LocalControlLane::LineTcp {
            host: "127.0.0.1".to_string(),
            port: 5555,
        },
    )
    .expect("tcp registration should succeed");

    let zenoh = find_local_daemon_by_port(17480)
        .expect("lookup should succeed")
        .expect("zenoh lane port should match");
    assert_eq!(zenoh.daemon_name, "win-test.lab");

    let tcp = find_local_daemon_by_port(5555)
        .expect("lookup should succeed")
        .expect("line-tcp lane port should match");
    assert_eq!(tcp.daemon_name, "tcp-test.lab");

    assert!(
        find_local_daemon_by_port(17481)
            .expect("lookup should succeed")
            .is_none(),
        "unregistered port must not be claimed by any daemon"
    );
}

#[test]
fn loopback_endpoints_should_replace_bind_addresses_and_drop_non_ip_lanes() {
    let endpoints = vec![
        "tcp/0.0.0.0:17480".to_string(),
        "udp/0.0.0.0:7447".to_string(),
        "tcp/[::]:17481".to_string(),
        "tcp/192.168.50.57:17482".to_string(),
        "serial/COM3#baudrate=115200".to_string(),
        "tls/0.0.0.0:17483".to_string(),
        "unixpipe/~/.rdog/x.pipe".to_string(),
    ];

    assert_eq!(
        loopback_zenoh_endpoints(&endpoints),
        vec![
            "tcp/127.0.0.1:17480".to_string(),
            "udp/127.0.0.1:7447".to_string(),
            "tcp/[::1]:17481".to_string(),
            "tcp/192.168.50.57:17482".to_string(),
        ]
    );
}
