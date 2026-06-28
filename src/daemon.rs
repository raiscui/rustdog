use crate::{
    config::{DaemonConfig, EndpointMode, InboundConfig, OutboundConfig},
    control_observation::initialize_durable_observation_state,
    control_transport::ControlTransportKind,
    shell::{self, ShellMode},
};
use colored::Colorize;
use std::{
    io::{self, ErrorKind},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 启动 daemon 模式并监督所有启用的 worker。
pub fn run(config: DaemonConfig) -> io::Result<()> {
    let retry_interval = config.retry_interval();
    let observation_daemon_name = tcp_observation_daemon_name(&config);
    initialize_durable_observation_state(&config.observation, None, &observation_daemon_name)?;
    let mut handles = Vec::new();

    // 每个 worker 都持有自己的停止标记。
    // 生产环境下不会主动置位,但测试会复用同一套循环。
    if config.outbound.enabled {
        let outbound = config.outbound.clone();
        handles.push(thread::spawn(move || {
            let stop = Arc::new(AtomicBool::new(false));
            run_outbound_worker_until_stopped(&outbound, retry_interval, stop.as_ref())
        }));
    }

    if config.inbound.enabled {
        let inbound = config.inbound.clone();
        handles.push(thread::spawn(move || {
            let stop = Arc::new(AtomicBool::new(false));
            run_inbound_worker_until_stopped(&inbound, retry_interval, stop.as_ref())
        }));
    }

    for handle in handles {
        join_worker(handle)?;
    }

    Ok(())
}

/// 运行 daemon 内嵌的 Zenoh router profile。
pub fn run_zenoh_router(
    mut config: DaemonConfig,
    namespace_override: Option<String>,
    daemon_name_override: Option<String>,
    entry_point_override: Vec<String>,
) -> io::Result<()> {
    if let Some(daemon_name) = daemon_name_override {
        config.zenoh.daemon_name = Some(daemon_name);
    }
    let daemon_name = config.zenoh.daemon_name.clone().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "Zenoh router daemon 缺少 daemon_name",
        )
    })?;

    let resolved_namespace = crate::zenoh_identity::resolve_namespace(
        namespace_override.as_deref(),
        Some(&daemon_name),
    )?;

    config.zenoh.enabled = true;
    config.outbound.enabled = false;
    config.inbound.enabled = false;
    config.zenoh.namespace = resolved_namespace;

    if !entry_point_override.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "router 模式的 `rdog daemon` 不再接受 `--entry-point`; 请把入口写到 `[zenoh].listen_endpoints`",
        ));
    }

    crate::config::validate_zenoh_daemon_profile(&config)?;

    // ------------------------------------------------------------
    // 把 unixpipe endpoint 注入到 listen_endpoints 列表。
    // 规则由 `zenoh_runtime::compose_listen_endpoints` 统一管理:
    // - 用户显式禁 (`unixpipe.enabled = false`) → 不注入
    // - 用户已经在 listen_endpoints 里显式声明 unixpipe/... → 不覆盖
    // - 否则用 (namespace, daemon_name) 自动推导 FIFO base 路径并注入到最前
    // ------------------------------------------------------------
    let composed_listen_endpoints = crate::zenoh_runtime::compose_listen_endpoints(
        &config.zenoh,
        &config.zenoh.namespace,
        &daemon_name,
    )?;

    #[cfg(unix)]
    let unixpipe_base_path = if config.zenoh.unixpipe.enabled {
        Some(match config.zenoh.unixpipe.socket_path.as_ref() {
            Some(explicit) => explicit.clone(),
            None => {
                crate::zenoh_runtime::unixpipe_socket_path(&config.zenoh.namespace, &daemon_name)?
            }
        })
    } else {
        None
    };

    // ------------------------------------------------------------
    // 清理 stale FIFO 文件,避免 Zenoh listener `mkfifo` EEXIST 启动失败。
    // 如果当前 daemon 声明 local_default,同一个 base path 也会写入 registry,
    // 作为 `rdog control @<line>` / `self @<line>` 的本机默认目标来源。
    // ------------------------------------------------------------
    #[cfg(unix)]
    let _local_default_guard = if let Some(base_path) = unixpipe_base_path.as_ref() {
        crate::zenoh_runtime::cleanup_stale_unixpipe_socket(base_path)?;
        log::info!(
            "zenoh unixpipe fast path 启用: base={}",
            base_path.display()
        );

        if config.zenoh.unixpipe.local_default {
            let guard = crate::zenoh_runtime::register_local_default_daemon(
                &config.zenoh.namespace,
                &daemon_name,
                base_path,
            )?;
            log::info!(
                "zenoh unixpipe local-default 已注册: namespace={}, daemon_name={}",
                config.zenoh.namespace,
                daemon_name
            );
            Some(guard)
        } else {
            None
        }
    } else {
        None
    };

    let shell = default_control_shell();
    crate::zenoh_control::run_router_daemon(
        crate::zenoh_control::ZenohDaemonRuntimeConfig {
            namespace: config.zenoh.namespace,
            daemon_name,
            listen_endpoints: composed_listen_endpoints,
            request_timeout_ms: config.zenoh.request_timeout_ms,
            startup_guard_window_ms: config.zenoh.startup_guard_window_ms,
            key_input_events: config.zenoh.key_input_events,
            observation: config.observation,
        },
        shell,
    )
}

fn tcp_observation_daemon_name(config: &DaemonConfig) -> String {
    if config.inbound.enabled {
        if let (Some(host), Some(port)) = (&config.inbound.host, config.inbound.port) {
            return format!("tcp-inbound-{host}-{port}");
        }
    }

    if config.outbound.enabled {
        if let (Some(host), Some(port)) = (&config.outbound.host, config.outbound.port) {
            return format!("tcp-outbound-{host}-{port}");
        }
    }

    "tcp-local".to_owned()
}

fn default_control_shell() -> &'static str {
    #[cfg(windows)]
    {
        "cmd.exe"
    }

    #[cfg(not(windows))]
    {
        "/bin/sh"
    }
}

fn run_outbound_worker_until_stopped(
    config: &OutboundConfig,
    retry_interval: Duration,
    stop: &AtomicBool,
) -> io::Result<()> {
    let (host, port, shell_name, mode) = outbound_parts(config)?;

    log::info!(
        "daemon outbound worker started for {}:{}",
        host.green(),
        port.to_string().cyan()
    );

    while !stop.load(Ordering::Relaxed) {
        log::info!(
            "daemon outbound connecting to {}:{}",
            host.green(),
            port.to_string().cyan()
        );

        match shell::connect_and_run_shell(host, port, shell_name, shell_mode_from_endpoint(mode)) {
            Ok(()) => {
                log::warn!(
                    "daemon outbound session ended for {}:{}, {} 秒后重试",
                    host,
                    port,
                    retry_interval.as_secs()
                );
            }
            Err(err) => {
                log::error!(
                    "daemon outbound connect/session failed for {}:{}: {}",
                    host,
                    port,
                    err
                );
            }
        }

        if wait_before_retry(stop, retry_interval) {
            break;
        }
    }

    Ok(())
}

fn run_inbound_worker_until_stopped(
    config: &InboundConfig,
    retry_interval: Duration,
    stop: &AtomicBool,
) -> io::Result<()> {
    let (host, port, shell_name, mode, transport) = inbound_parts(config)?;
    let address = format!("{host}:{port}");

    log::info!(
        "daemon inbound worker started on {}:{}",
        host.green(),
        port.to_string().cyan()
    );

    while !stop.load(Ordering::Relaxed) {
        match TcpListener::bind(&address) {
            Ok(listener) => {
                log::info!(
                    "daemon inbound listening on {}:{}",
                    host.green(),
                    port.to_string().cyan()
                );

                listener.set_nonblocking(true)?;

                loop {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }

                    match listener.accept() {
                        Ok((stream, remote_addr)) => {
                            // inbound listener 为了可轮询退出保持 nonblocking。
                            // 但真正交给 shell 的会话 socket 仍然要恢复为阻塞模式,
                            // 否则“暂时还没数据”会被误判成 `WouldBlock` 错误。
                            stream.set_nonblocking(false)?;
                            log::info!("daemon inbound accepted connection from {}", remote_addr);

                            if mode == EndpointMode::Control {
                                let shell_name = shell_name.to_owned();
                                let host = host.to_owned();
                                thread::spawn(move || {
                                    let session_result = match transport {
                                        ControlTransportKind::Tcp => {
                                            shell::run_control_receiver_over_stream(
                                                stream,
                                                &shell_name,
                                            )
                                        }
                                        ControlTransportKind::WebSocket => {
                                            shell::run_control_receiver_over_websocket_stream(
                                                stream,
                                                &shell_name,
                                            )
                                        }
                                    };

                                    if let Err(err) = session_result {
                                        log::error!(
                                            "daemon inbound session failed for {}:{}: {}",
                                            host,
                                            port,
                                            err
                                        );
                                    } else {
                                        log::warn!(
                                            "daemon inbound session ended for {}:{}",
                                            host,
                                            port
                                        );
                                    }
                                });
                                continue;
                            }

                            let session_result = match mode {
                                EndpointMode::Interactive => shell::run_shell_over_stream(
                                    stream,
                                    shell_name,
                                    ShellMode::Interactive,
                                ),
                                EndpointMode::Control => match transport {
                                    ControlTransportKind::Tcp => {
                                        shell::run_control_receiver_over_stream(stream, shell_name)
                                    }
                                    ControlTransportKind::WebSocket => {
                                        shell::run_control_receiver_over_websocket_stream(
                                            stream, shell_name,
                                        )
                                    }
                                },
                            };

                            if let Err(err) = session_result {
                                log::error!(
                                    "daemon inbound session failed for {}:{}: {}",
                                    host,
                                    port,
                                    err
                                );
                            } else {
                                log::warn!("daemon inbound session ended for {}:{}", host, port);
                            }
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL_INTERVAL);
                        }
                        Err(err) => {
                            log::error!(
                                "daemon inbound listener failed on {}:{}: {}",
                                host,
                                port,
                                err
                            );
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                log::error!("daemon inbound bind failed on {}:{}: {}", host, port, err);
            }
        }

        if wait_before_retry(stop, retry_interval) {
            break;
        }
    }

    Ok(())
}

fn wait_before_retry(stop: &AtomicBool, retry_interval: Duration) -> bool {
    if stop.load(Ordering::Relaxed) {
        return true;
    }

    if retry_interval.is_zero() {
        return stop.load(Ordering::Relaxed);
    }

    log::info!(
        "daemon worker waiting {} 秒 before retry",
        retry_interval.as_secs()
    );
    thread::sleep(retry_interval);
    stop.load(Ordering::Relaxed)
}

fn join_worker(handle: JoinHandle<io::Result<()>>) -> io::Result<()> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("daemon worker thread panicked")),
    }
}

fn outbound_parts(config: &OutboundConfig) -> io::Result<(&str, u16, &str, EndpointMode)> {
    let Some(host) = config.host.as_deref() else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "outbound 已启用,但缺少 host",
        ));
    };
    let Some(port) = config.port else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "outbound 已启用,但缺少 port",
        ));
    };
    let Some(shell_name) = config.shell.as_deref() else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "outbound 已启用,但缺少 shell",
        ));
    };

    Ok((host, port, shell_name, config.mode))
}

fn inbound_parts(
    config: &InboundConfig,
) -> io::Result<(&str, u16, &str, EndpointMode, ControlTransportKind)> {
    let Some(host) = config.host.as_deref() else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "inbound 已启用,但缺少 host",
        ));
    };
    let Some(port) = config.port else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "inbound 已启用,但缺少 port",
        ));
    };
    let Some(shell_name) = config.shell.as_deref() else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "inbound 已启用,但缺少 shell",
        ));
    };

    Ok((host, port, shell_name, config.mode, config.transport))
}

fn shell_mode_from_endpoint(mode: EndpointMode) -> ShellMode {
    match mode {
        EndpointMode::Interactive => ShellMode::Interactive,
        EndpointMode::Control => ShellMode::Control,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{net::TcpStream, sync::atomic::AtomicUsize, time::Instant};

    const TEST_HOST: &str = "127.0.0.1";
    const INVALID_SHELL: &str = "__rdog_missing_shell__";

    mod daemon_workers {
        use super::*;

        #[test]
        fn outbound_worker_should_retry_after_session_failure() {
            let port = next_free_port();
            let accept_count = Arc::new(AtomicUsize::new(0));
            let server = spawn_counting_server(port, 2, Arc::clone(&accept_count));
            let stop = Arc::new(AtomicBool::new(false));
            let config = OutboundConfig {
                enabled: true,
                host: Some(TEST_HOST.to_owned()),
                port: Some(port),
                shell: Some(INVALID_SHELL.to_owned()),
                mode: EndpointMode::Interactive,
            };

            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_outbound_worker_until_stopped(
                    &config,
                    Duration::from_millis(10),
                    worker_stop.as_ref(),
                )
            });

            wait_until(
                || accept_count.load(Ordering::Relaxed) >= 2,
                Duration::from_secs(3),
            );
            stop.store(true, Ordering::Relaxed);

            server.join().expect("server thread should finish");
            worker.join().expect("worker thread should finish").unwrap();
        }

        #[test]
        fn inbound_worker_should_retry_after_bind_failure() {
            let port = next_free_port();
            let guard = TcpListener::bind((TEST_HOST, port)).expect("guard listener should bind");
            let stop = Arc::new(AtomicBool::new(false));
            let config = InboundConfig {
                enabled: true,
                host: Some(TEST_HOST.to_owned()),
                port: Some(port),
                shell: Some(INVALID_SHELL.to_owned()),
                mode: EndpointMode::Interactive,
                transport: ControlTransportKind::Tcp,
            };

            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_inbound_worker_until_stopped(
                    &config,
                    Duration::from_millis(10),
                    worker_stop.as_ref(),
                )
            });

            thread::sleep(Duration::from_millis(80));
            drop(guard);

            wait_until_connectable(port, Duration::from_secs(3));
            stop.store(true, Ordering::Relaxed);

            worker.join().expect("worker thread should finish").unwrap();
        }

        #[test]
        fn inbound_worker_should_keep_listening_after_session_failure() {
            let port = next_free_port();
            let stop = Arc::new(AtomicBool::new(false));
            let config = InboundConfig {
                enabled: true,
                host: Some(TEST_HOST.to_owned()),
                port: Some(port),
                shell: Some(INVALID_SHELL.to_owned()),
                mode: EndpointMode::Interactive,
                transport: ControlTransportKind::Tcp,
            };

            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || {
                run_inbound_worker_until_stopped(
                    &config,
                    Duration::from_millis(10),
                    worker_stop.as_ref(),
                )
            });

            wait_until_connectable(port, Duration::from_secs(3));
            wait_until_connectable(port, Duration::from_secs(3));
            stop.store(true, Ordering::Relaxed);

            worker.join().expect("worker thread should finish").unwrap();
        }
    }

    fn next_free_port() -> u16 {
        let listener = TcpListener::bind((TEST_HOST, 0)).expect("ephemeral listener should bind");
        listener
            .local_addr()
            .expect("listener should have local addr")
            .port()
    }

    fn spawn_counting_server(
        port: u16,
        expected_accepts: usize,
        accept_count: Arc<AtomicUsize>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            let listener =
                TcpListener::bind((TEST_HOST, port)).expect("counting server should bind");
            listener
                .set_nonblocking(true)
                .expect("counting server should become non-blocking");
            let deadline = Instant::now() + Duration::from_secs(3);

            while Instant::now() < deadline {
                if accept_count.load(Ordering::Relaxed) >= expected_accepts {
                    return;
                }

                match listener.accept() {
                    Ok((stream, _)) => {
                        accept_count.fetch_add(1, Ordering::Relaxed);
                        drop(stream);
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("counting server accept failed: {err}"),
                }
            }

            panic!("counting server did not observe enough accepts before timeout");
        })
    }

    fn wait_until_connectable(port: u16, timeout: Duration) {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            match TcpStream::connect((TEST_HOST, port)) {
                Ok(stream) => {
                    drop(stream);
                    return;
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }

        panic!("port {port} did not become connectable before timeout");
    }

    fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if predicate() {
                return;
            }

            thread::sleep(Duration::from_millis(10));
        }

        panic!("condition was not satisfied before timeout");
    }
}
