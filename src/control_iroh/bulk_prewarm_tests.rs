use super::*;
use iroh::endpoint::IncomingAddr;
use std::net::UdpSocket;

const LOOPBACK: &str = "127.0.0.1:0";

fn config(key_file: PathBuf) -> ClientConfig {
    ClientConfig {
        key_file,
        directory: Directory {
            id: SecretKey::generate().public(),
            addr: None,
            via_relay: false,
            relay: None,
        },
        relay: Relay {
            url: "https://127.0.0.1:1".parse().unwrap(),
            qad_port: 1,
            root_cert: PathBuf::new(),
        },
        path: Route::Direct,
        probe_direct_via_relay: false,
        bulk_prewarm: true,
        direct_connect_timeout_ms: None,
        routable: false,
        bind: None,
        lease_ttl_ms: None,
        frame_timeout_ms: None,
    }
}

async fn fixture_probe() -> (SecretKey, Endpoint, Endpoint, BulkPrewarmTuple) {
    let bind = Some(LOOPBACK.parse().unwrap());
    let key = SecretKey::generate();
    let data = endpoint(key.clone(), bind, DIRECT_PROBE_RETURN_ALPN, None)
        .await
        .unwrap();
    let daemon = endpoint_on(
        SecretKey::generate(),
        bind,
        &[DIRECT_PROBE_ALPN, CONTROL_ALPN],
        None,
        false,
    )
    .await
    .unwrap();
    let (forward, incoming) = timeout(IO, async {
        tokio::join!(
            async {
                data.connect(daemon.addr(), DIRECT_PROBE_ALPN)
                    .await
                    .unwrap()
            },
            async { daemon.accept().await.unwrap().await.unwrap() },
        )
    })
    .await
    .unwrap();
    await_direct(&forward, false, IO).await.unwrap();
    let (remote, local) = selected_direct_path(&forward, false).unwrap();
    let tuple = observed_bulk_tuple(&data, forward.remote_id(), remote, local).unwrap();
    assert_eq!(tuple.local, data.bound_sockets()[0]);
    assert_eq!(tuple.remote, daemon.bound_sockets()[0]);
    assert_eq!(tuple.peer, daemon.id());
    drop((forward, incoming));
    (key, data, daemon, tuple)
}

#[tokio::test]
async fn prewarmed_bulk_rebinds_the_exact_tuple_and_authenticates_the_peer() {
    let (key, probe, daemon, tuple) = fixture_probe().await;
    let local = tuple.local;
    let remote = tuple.remote;
    let peer = tuple.peer;
    close_direct_probe(probe, peer).await;
    let cfg = config(PathBuf::new());
    let (dial, incoming) = timeout(IO, async {
        tokio::join!(dial_prewarmed_bulk(&key, &cfg, tuple, IO), async {
            let incoming = daemon.accept().await.unwrap();
            assert_eq!(incoming.remote_addr(), IncomingAddr::Ip(local));
            incoming.await.unwrap()
        },)
    })
    .await
    .unwrap();
    let (data, conn, actual) = dial.unwrap();
    assert!(actual == Route::Direct);
    assert_eq!(conn.remote_id(), peer);
    assert_eq!(incoming.alpn(), CONTROL_ALPN);
    assert_eq!(selected_direct_path(&conn, false).unwrap().0, remote);
    assert_eq!(data.bound_sockets(), vec![local]);
    assert!(data.addr().relay_urls().next().is_none());
    tokio::join!(data.close(), daemon.close());
}

#[tokio::test]
async fn closing_the_probe_releases_the_kernel_socket_before_rebind() {
    let (_, probe, daemon, tuple) = fixture_probe().await;
    assert!(UdpSocket::bind(tuple.local).is_err());
    let closed = probe.closed();
    close_direct_probe(probe, tuple.peer).await;
    timeout(IO, closed).await.unwrap();
    let socket = UdpSocket::bind(tuple.local).expect("probe socket is still bound");
    assert_eq!(socket.local_addr().unwrap(), tuple.local);
    daemon.close().await;
}

#[tokio::test]
async fn busy_port_is_a_rebind_failure_and_does_not_start_a_bulk_dial() {
    let (key, probe, daemon, tuple) = fixture_probe().await;
    close_direct_probe(probe, tuple.peer).await;
    let _owner = UdpSocket::bind(tuple.local).unwrap();
    let error = dial_prewarmed_bulk(&key, &config(PathBuf::new()), tuple, IO)
        .await
        .err()
        .unwrap();
    assert!(
        error.to_string().starts_with(BULK_PREWARM_REBIND_FAILED),
        "{error}"
    );
    assert!(timeout(Duration::from_millis(100), daemon.accept())
        .await
        .is_err());
    daemon.close().await;
}

#[tokio::test]
async fn a_rebound_socket_without_a_direct_peer_cannot_open_a_bulk_lane() {
    let (key, probe, daemon, mut tuple) = fixture_probe().await;
    close_direct_probe(probe, tuple.peer).await;
    let blackhole = UdpSocket::bind(LOOPBACK).unwrap();
    tuple.remote = blackhole.local_addr().unwrap();
    let error = dial_prewarmed_bulk(
        &key,
        &config(PathBuf::new()),
        tuple,
        Duration::from_millis(100),
    )
    .await
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .starts_with(BULK_PREWARM_DIRECT_UNAVAILABLE),
        "{error}"
    );
    assert!(timeout(Duration::from_millis(100), daemon.accept())
        .await
        .is_err());
    daemon.close().await;
}

#[test]
fn unavailable_coordination_does_not_enter_the_bulk_handler() {
    let rt = runtime().unwrap();
    let daemon = rt
        .block_on(endpoint(
            SecretKey::generate(),
            Some(LOOPBACK.parse().unwrap()),
            CONTROL_ALPN,
            None,
        ))
        .unwrap();
    let path = std::env::temp_dir().join(format!("rdog-bulk-prewarm-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, SecretKey::generate().to_bytes()).unwrap();
    let mut cfg = config(path.clone());
    cfg.routable = true;
    let result = with_bulk_lane_at(cfg, daemon.addr(), |_| {
        panic!("missing coordination candidate reached the bulk handler");
    });
    std::fs::remove_file(path).unwrap();
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(BULK_PREWARM_TUPLE_UNAVAILABLE),
        "{error}"
    );
    rt.block_on(async {
        assert!(timeout(Duration::from_millis(100), daemon.accept())
            .await
            .is_err());
        daemon.close().await;
    });
}
