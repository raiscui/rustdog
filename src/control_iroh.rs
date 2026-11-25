use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use iroh::{
    endpoint::{presets, Connection, ConnectionError, PortmapperConfig, RecvStream, SendStream},
    Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr,
};
use iroh_relay::{server, tls::CaTlsConfig, RelayConfig, RelayMap, RelayQuicConfig};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    runtime::{Builder, Handle, Runtime},
    task::JoinSet,
    time::timeout,
};

use crate::{
    control_core::execute_ping,
    control_protocol::{parse_control_line, ControlCommand, ControlParseResult, ControlRequest},
    control_session::ControlPeerSession,
    control_transport::ControlTransport,
};

#[cfg(all(test, target_os = "linux"))]
mod admission_tests;

#[cfg(test)]
mod readonly_tests {
    #[test]
    fn default_iroh_gate_rejects_pty() {
        assert!(super::ping_only("@ping").is_ok());
        let open = crate::pty_control::render_pty_open_line(&["python3".into()], 80, 24).unwrap();
        let err = super::ping_only(&open).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(err.to_string(), "iroh_readonly_ping_required");
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const DIRECTORY_ALPN: &[u8] = b"rdog/iroh-directory/1";
const CONTROL_ALPN: &[u8] = b"rdog/iroh-readonly/1";
const IO: Duration = Duration::from_secs(3);
const CONNECT: Duration = Duration::from_secs(7);
const LEASE: Duration = Duration::from_secs(15);
const RENEW: Duration = Duration::from_secs(5);
const MAX_LINE: usize = 1024;
const MAX_DIRECTORY: usize = 2048;
const MAX_SESSIONS: usize = 16;
const DENIED: &str = "iroh_target_denied";
const DENIED_CODE: u32 = 43;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RendezvousConfig {
    key_file: PathBuf,
    bind: SocketAddr,
    relay_http: SocketAddr,
    relay_https: SocketAddr,
    qad: SocketAddr,
    relay_cert: PathBuf,
    relay_key: PathBuf,
    registrations: HashMap<String, EndpointId>,
    lookups: Vec<EndpointId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    id: EndpointId,
    addr: SocketAddr,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relay {
    url: RelayUrl,
    qad_port: u16,
    root_cert: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DaemonConfig {
    key_file: PathBuf,
    directory: Directory,
    relay: Relay,
    name: String,
    allow: Vec<EndpointId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientConfig {
    key_file: PathBuf,
    directory: Directory,
    relay: Relay,
    path: Route,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Route {
    Direct,
    Relay,
}

impl Route {
    fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
enum DirectoryRequest {
    Register { name: String, addr: EndpointAddr },
    Lookup { name: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "status", rename_all = "snake_case")]
enum DirectoryReply {
    Registered,
    Found { addr: EndpointAddr },
    Rejected { reason: String },
}

fn profile<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("iroh profile exceeds 64 KiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid iroh JSON profile".into())
}

fn identity(path: &Path) -> Result<SecretKey> {
    let mut bytes = Vec::new();
    File::open(path)?.take(33).read_to_end(&mut bytes)?;
    let fixed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "invalid iroh identity material")?;
    Ok(SecretKey::from_bytes(&fixed))
}

fn loopback(addr: SocketAddr) -> Result<SocketAddr> {
    if !addr.ip().is_loopback() || !addr.is_ipv4() {
        return Err("P1 iroh requires IPv4 loopback".into());
    }
    Ok(addr)
}

fn name_valid(name: &str) -> bool {
    let Some((namespace, daemon)) = name.split_once('/') else {
        return false;
    };
    [namespace, daemon].into_iter().all(|part| {
        (1..=64).contains(&part.len())
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    })
}

fn ping_only(line: &str) -> io::Result<ControlRequest> {
    match parse_control_line(line)? {
        ControlParseResult::Control(request) if matches!(request.command, ControlCommand::Ping) => {
            Ok(request)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "iroh_readonly_ping_required",
        )),
    }
}

fn certificates(path: &Path) -> io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    rustls_pemfile::certs(&mut BufReader::new(File::open(path)?)).collect()
}

fn runtime() -> io::Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

async fn endpoint(
    key: SecretKey,
    bind: Option<SocketAddr>,
    alpn: &[u8],
    relay: Option<&Relay>,
) -> Result<Endpoint> {
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .alpns(vec![alpn.to_vec()])
        .clear_ip_transports()
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .relay_mode(RelayMode::Disabled);
    if let Some(bind) = bind {
        builder = builder.bind_addr(loopback(bind)?)?;
    }
    if let Some(r) = relay {
        if r.url.scheme() != "https"
            || r.url.host_str() != Some("127.0.0.1")
            || r.url.port().is_none()
            || r.qad_port == 0
        {
            return Err("P1 iroh requires an explicit HTTPS loopback relay and QAD port".into());
        }
        builder = builder
            .ca_tls_config(CaTlsConfig::custom_roots(certificates(&r.root_cert)?))
            .relay_mode(RelayMode::Custom(RelayMap::from_iter([RelayConfig::new(
                r.url.clone(),
                Some(RelayQuicConfig::new(r.qad_port)),
            )])));
    }
    Ok(builder.bind().await?)
}

async fn frame_get<T: DeserializeOwned>(read: &mut RecvStream) -> Result<T> {
    let bytes = timeout(IO, read.read_to_end(MAX_DIRECTORY)).await??;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn frame_put<T: Serialize>(write: &mut SendStream, value: &T) -> Result<()> {
    let buf = serde_json::to_vec(value)?;
    if buf.len() > MAX_DIRECTORY {
        return Err("iroh directory frame too large".into());
    }
    timeout(IO, write.write_all(&buf)).await??;
    write.finish()?;
    Ok(())
}

async fn directory_call(
    ep: &Endpoint,
    directory: &Directory,
    request: DirectoryRequest,
) -> Result<DirectoryReply> {
    let addr =
        EndpointAddr::from_parts(directory.id, [TransportAddr::Ip(loopback(directory.addr)?)]);
    let conn = timeout(CONNECT, ep.connect(addr, DIRECTORY_ALPN)).await??;
    let (mut send, mut read) = timeout(IO, conn.open_bi()).await??;
    frame_put(&mut send, &request).await?;
    match frame_get(&mut read).await? {
        DirectoryReply::Rejected { reason } => Err(reason.into()),
        reply => Ok(reply),
    }
}

struct Registry {
    owners: HashMap<String, EndpointId>,
    readers: Vec<EndpointId>,
    leases: HashMap<String, (EndpointAddr, Instant)>,
    relay: RelayUrl,
}

impl Registry {
    fn request(&mut self, peer: EndpointId, request: DirectoryRequest) -> DirectoryReply {
        let reject = |reason: &str| DirectoryReply::Rejected {
            reason: reason.into(),
        };
        match request {
            DirectoryRequest::Register { name, addr } => {
                if self.owners.get(&name) != Some(&peer)
                    || addr.id != peer
                    || addr.addrs.len() != 2
                    || addr.ip_addrs().count() != 1
                    || addr
                        .ip_addrs()
                        .any(|a| loopback(*a).is_err() || a.port() == 0)
                    || addr.relay_urls().collect::<Vec<_>>() != [&self.relay]
                {
                    return reject("iroh_registration_denied");
                }
                self.leases.insert(name, (addr, Instant::now() + LEASE));
                DirectoryReply::Registered
            }
            DirectoryRequest::Lookup { name } => {
                if !self.readers.contains(&peer) {
                    return reject("iroh_lookup_denied");
                }
                match self.leases.get(&name) {
                    Some((addr, expires)) if *expires > Instant::now() => {
                        DirectoryReply::Found { addr: addr.clone() }
                    }
                    _ => reject("iroh_target_not_registered"),
                }
            }
        }
    }
}

#[derive(Debug)]
struct RelayAccess(Vec<EndpointId>);

impl server::AccessControl for RelayAccess {
    async fn on_connect(&self, request: &server::ClientRequest) -> server::Access {
        if self.0.contains(&request.endpoint_id()) {
            server::Access::Allow
        } else {
            server::Access::Deny { reason: None }
        }
    }
}

pub(crate) fn run_rendezvous(path: &Path) -> Result<()> {
    let cfg: RendezvousConfig = profile(path)?;
    if cfg.registrations.is_empty() || !cfg.registrations.keys().all(|name| name_valid(name)) {
        return Err("iroh registration configuration requires NAMESPACE/NAME keys".into());
    }
    let key = identity(&cfg.key_file)?;
    let relay_key = rustls_pemfile::private_key(&mut BufReader::new(File::open(&cfg.relay_key)?))?
        .ok_or("missing P1 relay TLS key")?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certificates(&cfg.relay_cert)?, relay_key)?;
    let mut relay_config = server::RelayConfig::new(loopback(cfg.relay_http)?);
    relay_config.access = Arc::new(RelayAccess(
        cfg.lookups
            .iter()
            .chain(cfg.registrations.values())
            .copied()
            .collect(),
    ));
    relay_config.tls = Some(server::TlsConfig::new(
        loopback(cfg.relay_https)?,
        server::CertConfig::Manual { server_config: tls },
    ));
    let mut config = server::ServerConfig::default();
    config.relay = Some(relay_config);
    config.quic = Some(server::QuicConfig::new(loopback(cfg.qad)?));
    runtime()?.block_on(async {
        let directory = endpoint(key, Some(cfg.bind), DIRECTORY_ALPN, None).await?;
        let relay = server::Server::spawn(config).await?;
        let tls_addr = relay.https_addr().ok_or("missing P1 HTTPS relay")?;
        let qad = relay.quic_addr().ok_or("missing P1 QAD listener")?;
        let registry = Arc::new(Mutex::new(Registry {
            owners: cfg.registrations, readers: cfg.lookups, leases: HashMap::new(),
            relay: format!("https://{tls_addr}").parse()?,
        }));
        log::info!("iroh rendezvous ready relay_https={tls_addr} qad={qad}");
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                incoming = directory.accept(), if tasks.len() < MAX_SESSIONS => {
                    let incoming = incoming.ok_or("iroh directory stopped")?;
                    let registry = registry.clone();
                    tasks.spawn(async move {
                        let outcome = async {
                            let conn = timeout(IO, incoming).await??;
                            let (mut send, mut read) = timeout(IO, conn.accept_bi()).await??;
                            let request = frame_get(&mut read).await?;
                            let reply = registry.lock().map_err(|_| "iroh directory lock failed")?.request(conn.remote_id(), request);
                            frame_put(&mut send, &reply).await?;
                            timeout(IO, send.stopped()).await??;
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                        }.await;
                        if outcome.is_err() { log::debug!("iroh directory request ended without reply"); }
                    });
                }
            }
        }
    })
}

async fn register(ep: &Endpoint, cfg: &DaemonConfig, data: &Endpoint) -> Result<Instant> {
    let ip = *data
        .bound_sockets()
        .first()
        .ok_or("missing daemon IP candidate")?;
    let addr = EndpointAddr::from_parts(
        data.id(),
        [
            TransportAddr::Ip(ip),
            TransportAddr::Relay(cfg.relay.url.clone()),
        ],
    );
    let expires = Instant::now() + LEASE;
    match directory_call(
        ep,
        &cfg.directory,
        DirectoryRequest::Register {
            name: cfg.name.clone(),
            addr,
        },
    )
    .await?
    {
        DirectoryReply::Registered => Ok(expires),
        _ => Err("unexpected iroh directory registration response".into()),
    }
}

fn route(conn: &Connection) -> Result<Route> {
    let paths = conn.paths();
    let observed = paths.iter().find(|path| path.is_selected());
    let observed = observed.as_ref().map(|path| path.remote_addr());
    match observed {
        Some(TransportAddr::Relay(_)) => Ok(Route::Relay),
        Some(TransportAddr::Ip(ip)) if loopback(*ip).is_ok() => Ok(Route::Direct),
        _ => Err("iroh has no usable loopback path".into()),
    }
}

pub(crate) fn run_daemon(path: &Path) -> Result<()> {
    let cfg: DaemonConfig = profile(path)?;
    if !name_valid(&cfg.name) || cfg.allow.is_empty() {
        return Err("iroh daemon requires a name and an explicit ping allowlist".into());
    }
    let key = identity(&cfg.key_file)?;
    runtime()?.block_on(async {
        let bind = "127.0.0.1:0".parse()?;
        let directory = endpoint(key.clone(), Some(bind), DIRECTORY_ALPN, None).await?;
        let data = endpoint(key, Some(bind), CONTROL_ALPN, Some(&cfg.relay)).await?;
        timeout(CONNECT, data.online()).await?;
        let deadline = register(&directory, &cfg, &data).await?;
        let allow = Arc::new(cfg.allow.clone());
        let expires = Arc::new(Mutex::new(deadline));
        let renewal_expires = expires.clone();
        let renewal_data = data.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(RENEW).await;
                if let Ok(deadline) = register(&directory, &cfg, &renewal_data).await {
                    if let Ok(mut when) = renewal_expires.lock() { *when = deadline; }
                }
            }
        });
        log::info!("iroh daemon registered");
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                incoming = data.accept(), if tasks.len() < MAX_SESSIONS => {
                    let incoming = incoming.ok_or("iroh daemon stopped")?;
                    let allow = allow.clone();
                    let expires = expires.clone();
                    tasks.spawn(async move {
                        let outcome = async {
                            let conn = timeout(IO, incoming).await??;
                            let (send, read) = timeout(IO, conn.accept_bi()).await??;
                            let route = route(&conn)?;
                            let live = expires.lock().ok().is_some_and(|at| *at > Instant::now());
                            if !live || !allow.contains(&conn.remote_id()) {
                                log::info!("iroh admission deny path={} handler_entries=0", route.label());
                                conn.close(DENIED_CODE.into(), DENIED.as_bytes());
                                return Ok(());
                            }
                            let transport = ControlTransport::Iroh(IrohLineTransport::new(send, read));
                            tokio::task::spawn_blocking(move || serve_ping(transport, route)).await??;
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                        }.await;
                        if outcome.is_err() { log::debug!("iroh control connection ended without a ping response"); }
                    });
                }
            }
        }
    })
}

fn serve_ping(mut transport: ControlTransport, route: Route) -> io::Result<()> {
    let line = transport
        .read_message()?
        .ok_or(io::ErrorKind::UnexpectedEof)?;
    let request = ping_only(&line)?;
    let outcome = execute_ping(request.request_id, "");
    if let Some(id) = request.request_id {
        log::info!("iroh ping dispatch path={} id={id}", route.label());
    }
    ControlPeerSession::new("iroh-readonly").dispatch_outcome(outcome, &mut transport)?;
    transport.close()
}

pub(crate) fn run_control(path: &Path, name: &str, line: &str) -> Result<()> {
    let request = ping_only(line)?;
    if !name_valid(name) {
        return Err("iroh target must be NAMESPACE/NAME".into());
    }
    let cfg: ClientConfig = profile(path)?;
    let key = identity(&cfg.key_file)?;
    runtime()?.block_on(async {
        let bind = "127.0.0.1:0".parse()?;
        let directory = endpoint(key.clone(), Some(bind), DIRECTORY_ALPN, None).await?;
        let lookup = directory_call(&directory, &cfg.directory, DirectoryRequest::Lookup { name: name.to_owned() }).await;
        directory.close().await;
        let DirectoryReply::Found { addr } = lookup? else { return Err("unexpected iroh directory lookup response".into()); };
        let data = match cfg.path {
            Route::Direct => endpoint(key, Some(bind), CONTROL_ALPN, None).await?,
            Route::Relay => endpoint(key, None, CONTROL_ALPN, Some(&cfg.relay)).await?,
        };
        let addrs = match cfg.path {
            Route::Direct => vec![TransportAddr::Ip(loopback(*addr.ip_addrs().next().ok_or("no iroh direct candidate")?)?)],
            Route::Relay if addr.relay_urls().any(|url| *url == cfg.relay.url) => vec![TransportAddr::Relay(cfg.relay.url)],
            Route::Relay => return Err("iroh directory has no configured relay candidate".into()),
        };
        let conn = timeout(CONNECT, data.connect(EndpointAddr::from_parts(addr.id, addrs), CONTROL_ALPN)).await??;
        let actual = route(&conn)?;
        if actual != cfg.path { return Err("iroh connection selected a prohibited route".into()); }
        log::info!("iroh_path={}", actual.label());
        let streams = timeout(IO, conn.open_bi()).await;
        let reply = match streams {
            Ok(Ok((send, read))) => {
                let mut transport = ControlTransport::Iroh(IrohLineTransport::new(send, read));
                let line = line.to_owned();
                tokio::task::spawn_blocking(move || {
                    transport.write_message(&line)?;
                    transport.read_message()?.ok_or(io::ErrorKind::UnexpectedEof.into())
                }).await?
            }
            Ok(Err(err)) => Err(io::Error::other(err)),
            Err(err) => Err(io::Error::other(err)),
        };
        let denied = matches!(conn.close_reason(),
            Some(ConnectionError::ApplicationClosed(close))
            if close.error_code.into_inner() == u64::from(DENIED_CODE) && close.reason.as_ref() == DENIED.as_bytes());
        data.close().await;
        if denied { return Err(DENIED.into()); }
        let reply = reply?;
        let body: serde_json::Value = serde_json::from_str(reply.strip_prefix("@response ").ok_or("invalid iroh ping response")?)?;
        let value = if let Some(id) = request.request_id {
            if body["id"].as_u64() != Some(id) { return Err("iroh ping response id mismatch".into()); }
            &body["value"]
        } else { &body };
        if value["pong"].as_bool() != Some(true) { return Err("invalid iroh pong".into()); }
        println!("{reply}");
        Ok(())
    })
}

pub(crate) struct IrohLineTransport {
    handle: Handle,
    send: SendStream,
    read: RecvStream,
    read_timeout: Duration,
    consumed: bool,
}

impl IrohLineTransport {
    fn new(send: SendStream, read: RecvStream) -> Self {
        Self {
            handle: Handle::current(),
            send,
            read,
            read_timeout: IO,
            consumed: false,
        }
    }

    pub(crate) fn read_message(&mut self) -> io::Result<Option<String>> {
        if std::mem::replace(&mut self.consumed, true) {
            return Ok(None);
        }
        let bytes = self
            .handle
            .block_on(async { timeout(self.read_timeout, self.read.read_to_end(MAX_LINE)).await })
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        let data = bytes
            .strip_suffix(b"\n")
            .filter(|line| !line.contains(&b'\n'))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid iroh line frame"))?;
        Ok(Some(
            String::from_utf8(data.to_vec()).map_err(io::Error::other)?,
        ))
    }

    pub(crate) fn write_message(&mut self, message: &str) -> io::Result<()> {
        if message.len() >= MAX_LINE || message.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid iroh line frame",
            ));
        }
        self.handle
            .block_on(async {
                timeout(IO, self.send.write_all(format!("{message}\n").as_bytes())).await
            })
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        self.send.finish().map_err(io::Error::other)
    }

    pub(crate) fn set_read_timeout(&mut self, duration: Option<Duration>) {
        self.read_timeout = duration.unwrap_or(IO).min(IO);
    }

    pub(crate) fn close(self) -> io::Result<()> {
        let stopped = self
            .handle
            .block_on(async { timeout(IO, self.send.stopped()).await })
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        if stopped.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "iroh peer stopped the line stream",
            ));
        }
        Ok(())
    }
}
