use std::{error::Error, net::SocketAddr, time::Duration};

use iroh::{
    endpoint::{presets, Connection, ConnectionError, PortmapperConfig, VarInt},
    Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey, TransportAddr,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

const ALPN: &[u8] = b"rustdog-test/rai-137-ping/1";
const PING: &[u8] = b"@ping#42\n";
const PONG: &[u8] = b"@response {\"id\":42,\"value\":{\"pong\":true}}\n";
const DENIED: &[u8] = b"test_identity_not_allowed";
const DENIED_CODE: u32 = 42;

async fn endpoint() -> TestResult<Endpoint> {
    Ok(Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")?
        .bind()
        .await?)
}

fn bound(endpoint: &Endpoint) -> SocketAddr {
    let sockets = endpoint.bound_sockets();
    assert_eq!(sockets.len(), 1);
    assert_eq!(sockets[0].ip(), std::net::Ipv4Addr::LOCALHOST);
    sockets[0]
}

fn assert_direct(connection: &Connection, remote: SocketAddr) {
    let paths = connection.paths();
    let selected = paths
        .iter()
        .find(|path| path.is_selected())
        .expect("selected QUIC path");
    assert!(!selected.is_relay());
    assert_eq!(selected.remote_addr(), &TransportAddr::Ip(remote));
}

async fn respond_to_ping(
    connection: &Connection,
    handler_entries: &mut Vec<EndpointId>,
) -> TestResult<()> {
    handler_entries.push(connection.remote_id());
    let (mut reply, mut request) = connection.accept_bi().await?;
    assert_eq!(request.read_to_end(PING.len()).await?, PING);
    reply.write_all(PONG).await?;
    reply.finish()?;
    Ok(())
}

#[test]
fn authenticated_iroh_loopback_admission() -> TestResult<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let (target, allowed, denied) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::try_join!(endpoint(), endpoint(), endpoint())
            })
            .await??;
            assert_ne!(allowed.id(), denied.id());
            assert_ne!(target.id(), allowed.id());
            assert_ne!(target.id(), denied.id());
            let target_socket = bound(&target);
            let target_addr =
                EndpointAddr::from_parts(target.id(), [TransportAddr::Ip(target_socket)]);
            let allowlist = [allowed.id()];
            let mut handler_entries = Vec::new();
            let scenarios = [
                ("allowed", &allowed, true),
                ("denied", &denied, false),
                ("allowed_after_denial", &allowed, true),
            ];

            let probe = tokio::time::timeout(Duration::from_secs(12), async {
                for (label, client, expect_allowed) in scenarios {
                    let server = async {
                        let incoming = target.accept().await.ok_or("target endpoint closed")?;
                        Ok::<_, Box<dyn Error>>(incoming.await?)
                    };
                    let connect = async { Ok::<_, Box<dyn Error>>(client.connect(target_addr.clone(), ALPN).await?) };
                    let (client_conn, server_conn) = tokio::try_join!(connect, server)?;
                    assert_eq!(client_conn.remote_id(), target.id());
                    assert_eq!(server_conn.remote_id(), client.id());
                    assert_eq!(server_conn.alpn(), ALPN);
                    assert_direct(&client_conn, target_socket);
                    assert_direct(&server_conn, bound(client));

                    let (mut request, mut response) = client_conn.open_bi().await?;
                    request.write_all(PING).await?;
                    request.finish()?;

                    let admitted = allowlist.contains(&server_conn.remote_id());
                    assert_eq!(admitted, expect_allowed);
                    let before = handler_entries.len();
                    if admitted {
                        respond_to_ping(&server_conn, &mut handler_entries).await?;
                        let bytes = response.read_to_end(96).await?;
                        let value: serde_json::Value = serde_json::from_slice(
                            bytes.strip_prefix(b"@response ").ok_or("missing response frame")?,
                        )?;
                        assert_eq!(value, serde_json::json!({"id":42,"value":{"pong":true}}));
                        assert_eq!(handler_entries.len() - before, 1);
                        client_conn.close(0u8.into(), b"test_complete");
                        println!("{label}: iroh_quic=direct_ipv4_loopback admission=allow handler_delta=1 id=42 pong=true");
                    } else {
                        server_conn.close(VarInt::from_u32(DENIED_CODE), DENIED);
                        match client_conn.closed().await {
                            ConnectionError::ApplicationClosed(close) => {
                                assert_eq!(close.error_code, VarInt::from_u32(DENIED_CODE));
                                assert_eq!(close.reason.as_ref(), DENIED);
                            }
                            other => panic!("expected explicit application rejection, got {other}"),
                        }
                        assert!(response.read_to_end(96).await.is_err(), "unexpected reply on denied stream");
                        assert_eq!(handler_entries.len(), before);
                        println!("{label}: iroh_quic=direct_ipv4_loopback admission=deny close_code={DENIED_CODE} reason=test_identity_not_allowed handler_delta=0 no_success_pong=true");
                    }
                }
                assert_eq!(handler_entries, [allowed.id(), allowed.id()]);
                Ok::<_, Box<dyn Error>>(())
            })
            .await;

            tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(target.close(), allowed.close(), denied.close());
            })
            .await?;
            println!("cleanup: all three ephemeral iroh endpoints closed");
            probe??;
            Ok(())
        })
}
