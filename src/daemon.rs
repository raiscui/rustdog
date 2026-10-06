use crate::{
    config::{DaemonConfig, EndpointMode, InboundConfig, OutboundConfig},
    control_observation::initialize_durable_observation_state,
    control_recording::control_handler::{install_recording_handler, RecordingHandler},
    control_transport::ControlTransportKind,
    shell::{self, ShellMode},
};
use colored::Colorize;
use std::{
    io::{self, ErrorKind},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// accept 连续失败多少次才放弃当前 listener、转去重新 bind。
///
/// 退避期间 listener 保持 bound 是这次修复的全部要点:
/// listener 一旦被 drop,退避窗口内的连接就是 `Connection refused (os error 61)`,
/// 而进程其实一直活着 —— 客户端无从区分"daemon 没起来"和"daemon 正在退避"。
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u32 = 3;

/// 停止接受新连接后,留给在途 control 会话线程的收尾窗口。
const SESSION_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// 健康状态 (@ping 消费面)
// ---------------------------------------------------------------------------

/// 监听绑定状态机的取值。
const BIND_UNBOUND: u8 = 0;
const BIND_LISTENING: u8 = 1;
const BIND_BACKING_OFF: u8 = 2;

/// `bind_state()` 的真相源,供 `@ping` 汇报"进程活着但暂时聋了"这第三种状态。
///
/// 2026-10-05 实战里 "daemon 起来 5 秒就没了" 与 "@ping 频繁 Connection refused"
/// 是同一根因的两个症状:进程一直活着,只是那一刻正落在退避窗口里。
/// 状态显式化之后,客户端才有机会区分开。
static BIND_STATE: AtomicU8 = AtomicU8::new(BIND_UNBOUND);

/// 首次进入 `run()` 的时刻,`@ping` 用它回报 daemon 已存活多久。
static STARTED_AT: OnceLock<Instant> = OnceLock::new();

/// 当前 inbound listener 的绑定状态。
///
/// outbound-only 的 daemon 恒为 `"unbound"` —— 它确实没有 TCP 监听面,
/// 这不是降级信号,调用方要靠这个区分"没在听"和"听了但正在退避"。
pub fn bind_state() -> &'static str {
    bind_state_label(BIND_STATE.load(Ordering::Relaxed))
}

/// daemon 已运行时长。`run()` 尚未被调用过时返回 `Duration::ZERO`。
pub fn uptime() -> Duration {
    STARTED_AT
        .get()
        .map_or(Duration::ZERO, |started| started.elapsed())
}

/// 状态值到可上报字面量的映射。
///
/// 刻意留成纯函数:绑定状态是进程级全局量,直接对 `bind_state()` 断言
/// 会和并行跑的 worker 单测互相踩,纯函数才有稳定的回归防线。
fn bind_state_label(state: u8) -> &'static str {
    match state {
        BIND_LISTENING => "listening",
        BIND_BACKING_OFF => "backing_off",
        _ => "unbound",
    }
}

fn set_bind_state(state: u8) {
    BIND_STATE.store(state, Ordering::Relaxed);
}

/// 启动 daemon 模式并监督所有启用的 worker。
pub fn run(config: DaemonConfig) -> io::Result<()> {
    // 两端都关 = 一个 worker 都没有。历史行为是空 `handles` 直接 `Ok(())`,
    // 于是 main 拿到退出码 0 且日志一字未留 —— 配置一改就"静默启动成功又立刻退出"。
    if !config.inbound.enabled && !config.outbound.enabled {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "inbound 与 outbound 均已禁用,daemon 没有任何 worker 可运行; 请至少启用一端, 或改用 zenoh transport",
        ));
    }

    install_recording_stack();
    let retry_interval = config.retry_interval();
    let observation_daemon_name = tcp_observation_daemon_name(&config);
    initialize_durable_observation_state(&config.observation, None, &observation_daemon_name)?;

    // 停止标记必须跨 worker 共享。改造前 stop 在每个闭包里各建一份且从不置位,
    // 于是 `stop` 参数在生产路径上事实上是死代码,SIGINT 只能靠进程默认行为硬杀。
    let shutdown = Arc::new(AtomicBool::new(false));
    install_shutdown_handlers(Arc::clone(&shutdown))?;
    STARTED_AT.get_or_init(Instant::now);
    set_bind_state(BIND_UNBOUND);

    let mut handles = Vec::new();

    if config.outbound.enabled {
        let outbound = config.outbound.clone();
        let stop = Arc::clone(&shutdown);
        handles.push(thread::spawn(move || {
            run_outbound_worker_until_stopped(&outbound, retry_interval, stop.as_ref())
        }));
    }

    if config.inbound.enabled {
        let inbound = config.inbound.clone();
        let stop = Arc::clone(&shutdown);
        handles.push(thread::spawn(move || {
            run_inbound_worker_until_stopped(&inbound, retry_interval, stop.as_ref())
        }));
    }

    for handle in handles {
        join_worker(handle)?;
    }

    set_bind_state(BIND_UNBOUND);
    Ok(())
}

/// 运行 daemon 内嵌的 Zenoh router profile。
pub fn run_zenoh_router(
    mut config: DaemonConfig,
    namespace_override: Option<String>,
    daemon_name_override: Option<String>,
    entry_point_override: Vec<String>,
) -> io::Result<()> {
    install_recording_stack();
    if let Some(daemon_name) = daemon_name_override {
        config.zenoh.daemon_name = Some(daemon_name);
    }
    let daemon_name = config.zenoh.daemon_name.clone().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "Zenoh router daemon 缺少 daemon_name",
        )
    })?;

    // namespace 来源合并 (拍板 2026-10-06): 配置为主, 配置与 --namespace
    // 同给即报错; 合并后的显式值再交 resolve_namespace 做校验/后缀推断。
    let effective_override = crate::zenoh_identity::merge_config_namespace(
        &config.zenoh.namespace,
        namespace_override.as_deref(),
    )?;
    let resolved_namespace = crate::zenoh_identity::resolve_namespace(
        effective_override.as_deref(),
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
    let composed_listen = crate::zenoh_runtime::compose_listen_endpoints(
        &config.zenoh,
        &config.zenoh.namespace,
        &daemon_name,
    )?;

    // unixpipe 文件清理必须等 service-name guard 成功后再执行。
    // 这里仅解析启动参数,真正的 ownership-sensitive 操作交给 router runtime。
    #[cfg(unix)]
    let unixpipe_startup = composed_listen.unixpipe_base_path.map(|base_path| {
        crate::zenoh_control::ZenohUnixpipeStartupConfig {
            base_path,
            local_default: config.zenoh.unixpipe.local_default,
        }
    });

    let shell = default_control_shell();
    crate::zenoh_control::run_router_daemon(
        crate::zenoh_control::ZenohDaemonRuntimeConfig {
            namespace: config.zenoh.namespace,
            daemon_name,
            listen_endpoints: composed_listen.listen_endpoints,
            request_timeout_ms: config.zenoh.request_timeout_ms,
            startup_guard_window_ms: config.zenoh.startup_guard_window_ms,
            key_input_events: config.zenoh.key_input_events,
            observation: config.observation,
            key_delivery_backend: config.key.delivery_backend,
            auth: config.auth,
            tls: config.tls,
            #[cfg(unix)]
            unixpipe_startup,
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
                set_bind_state(BIND_LISTENING);
                log::info!(
                    "daemon inbound listening on {}:{}",
                    host.green(),
                    port.to_string().cyan()
                );

                listener.set_nonblocking(true)?;
                let mut sessions: Vec<JoinHandle<()>> = Vec::new();
                let mut consecutive_accept_errors: u32 = 0;

                loop {
                    if stop.load(Ordering::Relaxed) {
                        drain_sessions(&mut sessions);
                        set_bind_state(BIND_UNBOUND);
                        return Ok(());
                    }

                    match listener.accept() {
                        Ok((stream, remote_addr)) => {
                            consecutive_accept_errors = 0;
                            reap_finished_sessions(&mut sessions);

                            // inbound listener 为了可轮询退出保持 nonblocking。
                            // 但真正交给 shell 的会话 socket 仍然要恢复为阻塞模式,
                            // 否则“暂时还没数据”会被误判成 `WouldBlock` 错误。
                            stream.set_nonblocking(false)?;
                            log::info!("daemon inbound accepted connection from {}", remote_addr);

                            if mode == EndpointMode::Control {
                                let shell_name = shell_name.to_owned();
                                let host = host.to_owned();
                                let session = thread::spawn(move || {
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
                                sessions.push(session);
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
                        // EINTR 只是 accept 被信号打断,不代表 listener 坏了:
                        // 立刻重试,既不退避也不计入失败预算。
                        Err(err) if err.kind() == ErrorKind::Interrupted => {
                            log::debug!(
                                "daemon inbound accept interrupted on {}:{}, 立即重试: {}",
                                host,
                                port,
                                err
                            );
                        }
                        Err(err) => {
                            // accept 失败不等于端口该关。历史实现在这里直接 break:
                            // listener 被 drop → 端口立即关闭 → 睡满 retry_seconds(默认 5 秒)
                            // 才重新 bind,窗口内的连接全是 Connection refused,
                            // 而进程一直活着。现在保留 listener 原地退避重试,
                            // 只有连续失败到阈值才真正放弃这个 listener。
                            consecutive_accept_errors += 1;

                            if consecutive_accept_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                                log::error!(
                                    "daemon inbound accept on {}:{} 连续失败 {} 次,放弃当前 listener 转去重新 bind: {}",
                                    host,
                                    port,
                                    consecutive_accept_errors,
                                    err
                                );
                                break;
                            }

                            set_bind_state(BIND_BACKING_OFF);
                            log::warn!(
                                "daemon inbound accept failed on {}:{} (第 {}/{} 次), listener 保持 bound,retry_in {} 秒: {}",
                                host,
                                port,
                                consecutive_accept_errors,
                                MAX_CONSECUTIVE_ACCEPT_ERRORS,
                                retry_interval.as_secs(),
                                err
                            );

                            if wait_before_retry(stop, retry_interval) {
                                drain_sessions(&mut sessions);
                                set_bind_state(BIND_UNBOUND);
                                return Ok(());
                            }

                            set_bind_state(BIND_LISTENING);
                            log::info!("daemon inbound 恢复监听 on {}:{}", host, port);
                        }
                    }
                }

                // 走到这里 = 连续 accept 失败到阈值,当前 listener 作废。
                // 这里刻意**不**收尾在途会话: 它们各自持有已建立的 socket,
                // 与 listener 无关; 等它们只会把重新 bind 的时间再往后推,
                // 正好复刻这次要消灭的"端口关着还慢吞吞"窗口。
                if !sessions.is_empty() {
                    log::warn!(
                        "daemon inbound 放弃旧 listener,{} 个在途会话转为游离",
                        sessions.len()
                    );
                }
                set_bind_state(BIND_BACKING_OFF);
            }
            Err(err) => {
                set_bind_state(BIND_BACKING_OFF);
                log::error!("daemon inbound bind failed on {}:{}: {}", host, port, err);
            }
        }

        if wait_before_retry(stop, retry_interval) {
            break;
        }
    }

    set_bind_state(BIND_UNBOUND);
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

/// 把 unix 停止信号接到共享 shutdown 标志上。
///
/// 单独起一个线程消费 `Signals`:signal-hook 的 iterator 会一直阻塞,
/// 收到第一个信号就 break 并关闭 handle,不留悬挂线程。
///
/// 覆盖范围:unix 侧 SIGINT / SIGTERM 走优雅退出;
/// Windows 的 CTRL_CLOSE_EVENT 需要 console control handler(另走 Win32 消息泵),
/// 这里不覆盖,Windows 侧仍用 taskkill 结束进程 —— 那种终止是硬杀,不保证收尾。
#[cfg(unix)]
fn install_shutdown_handlers(shutdown: Arc<AtomicBool>) -> io::Result<()> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals =
        Signals::new([SIGINT, SIGTERM]).map_err(|err| io::Error::other(err.to_string()))?;
    let handle = signals.handle();

    thread::spawn(move || {
        if let Some(signal) = signals.forever().next() {
            log::info!("daemon 收到 {signal}, 停止接受新连接并等待在途会话收尾");
            shutdown.store(true, Ordering::SeqCst);
        }
        handle.close();
    });

    Ok(())
}

#[cfg(not(unix))]
fn install_shutdown_handlers(_shutdown: Arc<AtomicBool>) -> io::Result<()> {
    Ok(())
}

/// 回收已结束的会话线程,只留还在跑的。
///
/// JoinHandle 本身很轻,但 8 小时长跑会攒下几百个死句柄;
/// 不回收的话停机时要 join 的全是早已结束的线程,徒增噪音。
fn reap_finished_sessions(sessions: &mut Vec<JoinHandle<()>>) {
    sessions.retain(|handle| !handle.is_finished());
}

/// 停止接受新连接后,给在途会话一个**有限**的收尾窗口。
///
/// 无界 join 会让一次 SIGTERM 卡死在一个挂着的会话上 —— 宁可超时退出,
/// 也不能把"干净退出"变成"永远退不出去"。窗口内结束的会话照常等待其结果。
fn drain_sessions(sessions: &mut Vec<JoinHandle<()>>) {
    if sessions.is_empty() {
        return;
    }

    log::info!(
        "daemon inbound 等待 {} 个在途会话收尾,最多 {} 秒",
        sessions.len(),
        SESSION_DRAIN_TIMEOUT.as_secs()
    );

    let deadline = Instant::now() + SESSION_DRAIN_TIMEOUT;

    while Instant::now() < deadline {
        reap_finished_sessions(sessions);
        if sessions.is_empty() {
            return;
        }
        thread::sleep(ACCEPT_POLL_INTERVAL);
    }

    if !sessions.is_empty() {
        log::warn!(
            "daemon inbound 仍有 {} 个会话未收尾,放弃等待",
            sessions.len()
        );
    }
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

/// 健康状态与启动前置校验的回归防线。
///
/// 刻意开在 unix 门之外: 这批改动来自 Windows 真机事故,
/// 两条断言不能只在 unix 上跑。
#[cfg(test)]
mod health_state {
    use super::*;

    #[test]
    fn bind_state_label_should_cover_the_whole_state_machine() {
        assert_eq!(bind_state_label(BIND_LISTENING), "listening");
        assert_eq!(bind_state_label(BIND_BACKING_OFF), "backing_off");
        assert_eq!(bind_state_label(BIND_UNBOUND), "unbound");
        // 未知取值不得漏出契约外的字面量 —— @ping 的消费面只认这三个
        assert_eq!(bind_state_label(u8::MAX), "unbound");
    }

    #[test]
    fn public_health_api_should_stay_inside_the_ping_contract() {
        assert!(matches!(
            bind_state(),
            "listening" | "backing_off" | "unbound"
        ));

        let first = uptime();
        let second = uptime();
        // 单调: run() 未启动时恒为 Duration::ZERO,启动后随墙上时钟增长
        assert!(second >= first);
    }

    #[test]
    fn run_should_reject_config_with_both_endpoints_disabled() {
        let err = run(DaemonConfig::default()).expect_err("两端都禁用时必须报错,不能静默 Ok 退出");

        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(err.to_string().contains("inbound 与 outbound"));
    }
}

/// Install the global `RecordingHandler` so the daemon can accept
/// `@record-*` line-control commands. Idempotent: only the first call
/// takes effect; subsequent calls are no-ops. Per issue #23 E2E smoke.
fn install_recording_stack() {
    let journal = default_recording_dir("journal");
    let bundle = default_recording_dir("bundle");
    let _ = std::fs::create_dir_all(&journal);
    let _ = std::fs::create_dir_all(&bundle);
    let handler = RecordingHandler::new(journal, bundle);
    let _ = install_recording_handler(handler);
}

fn default_recording_dir(sub: &str) -> std::path::PathBuf {
    // Per XDG / macOS conventions; override via `RDOG__RECORDING__DIR`
    // (not yet implemented) if a deployment needs a different path.
    let base = std::env::var_os("RDOG_RECORDING_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|p| p.join(".cache").join("rdog").join("recording"))
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rdog-recording"))
        });
    base.join(sub)
}
