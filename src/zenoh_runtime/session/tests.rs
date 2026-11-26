use super::*;

#[test]
fn scout_interfaces_should_leave_auto_when_no_candidates_exist() {
    assert_eq!(select_scout_interfaces(None, Vec::new), None);
    let mut config = Config::default();
    let key = "scouting/multicast/interface";
    let original = config.get_json(key).ok();
    insert_scout_interfaces(&mut config, Some("  "), Vec::new).unwrap();
    assert_eq!(config.get_json(key).ok(), original);
}

#[test]
fn scout_interfaces_should_sort_and_deduplicate_non_loopback_unicast_ipv4() {
    let candidates = [
        "192.168.1.10",
        "127.0.0.1",
        "224.0.0.224",
        "10.0.0.2",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "192.168.1.10",
    ]
    .map(|ip| ip.parse().unwrap());
    let expected = Some("10.0.0.2,192.168.1.10".to_owned());
    assert_eq!(
        select_scout_interfaces(None, || candidates.to_vec()),
        expected
    );
    assert_eq!(
        select_scout_interfaces(None, || candidates.into_iter().rev().collect()),
        expected
    );
}

#[test]
fn scout_interfaces_should_use_explicit_override_without_enumerating() {
    for configured in ["Ethernet 2,192.168.1.10", "auto"] {
        assert_eq!(
            select_scout_interfaces(Some(configured), || panic!("must not enumerate")),
            Some(configured.to_owned())
        );
    }
}

#[test]
fn open_session_error_should_upgrade_windows_listen_access_denied() {
    let err = to_open_session_error(
        "拒绝访问。 (os error 5)",
        &[String::from("tcp/0.0.0.0:7447")],
    );

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("192.168.50.57:17447"));
    assert!(err.to_string().contains("0.0.0.0:7447"));
}

#[test]
fn locator_priority_should_prefer_preferred_tcp_over_link_local_and_serial() {
    let ordered = vec![
        "serial/COM3#baudrate=115200".to_string(),
        "tcp/169.254.105.229:7447".to_string(),
        "tcp/192.168.50.57:7447".to_string(),
        "tcp/127.0.0.1:7447".to_string(),
    ];

    let mut ordered = ordered;
    ordered.sort_by(|left, right| {
        locator_sort_key(left)
            .cmp(&locator_sort_key(right))
            .then_with(|| left.cmp(right))
    });

    assert_eq!(ordered[0], "tcp/127.0.0.1:7447");
    assert_eq!(ordered[1], "tcp/192.168.50.57:7447");
    assert_eq!(ordered[2], "tcp/169.254.105.229:7447");
    assert_eq!(ordered[3], "serial/COM3#baudrate=115200");
}

#[test]
fn parse_locator_socket_addr_should_ignore_metadata_suffix() {
    let addr = parse_locator_socket_addr("tcp/192.168.50.57:7447#so_sndbuf=65000").expect("addr");

    assert_eq!(addr, SocketAddr::from_str("192.168.50.57:7447").unwrap());
}
