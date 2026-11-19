use super::*;

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

#[test]
fn recording_rejects_plaintext_empty_and_mixed_routes_before_connecting() {
    for endpoints in [
        vec![],
        vec!["tcp/127.0.0.1:7447".to_owned()],
        vec![
            "tls/127.0.0.1:7447".to_owned(),
            "tcp/127.0.0.1:7448".to_owned(),
        ],
    ] {
        let error = open_recording_client_session(&endpoints)
            .expect_err("录制不能经不受保护的链路认领owner");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}

#[test]
fn recording_routes_cannot_create_implicit_listeners_or_discovery_links() {
    for mode in ["router", "client"] {
        let mut config = Config::default();
        config.insert_json5("mode", &format!("\"{mode}\"")).unwrap();
        let endpoint = vec!["tls/127.0.0.1:7447".to_owned()];
        let (connect, listen) = if mode == "router" {
            (&[][..], endpoint.as_slice())
        } else {
            (endpoint.as_slice(), &[][..])
        };
        restrict_recording_links(&mut config, connect, listen).unwrap();
        for key in ["connect/endpoints", "listen/endpoints"] {
            let routes: Vec<String> = serde_json::from_str(&config.get_json(key).unwrap()).unwrap();
            assert!(routes.iter().all(|route| route.starts_with("tls/")));
            assert_eq!(
                routes.is_empty(),
                (mode == "router") == (key == "connect/endpoints")
            );
        }
        for key in [
            "scouting/multicast/enabled",
            "scouting/multicast/listen",
            "scouting/gossip/enabled",
        ] {
            assert_eq!(config.get_json(key).unwrap(), "false");
        }
        for key in [
            "scouting/multicast/autoconnect",
            "scouting/gossip/autoconnect",
        ] {
            let routes: serde_json::Value =
                serde_json::from_str(&config.get_json(key).unwrap()).unwrap();
            assert_eq!(routes, serde_json::json!([]));
        }
    }
}
