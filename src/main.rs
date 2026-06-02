use clap::Parser;
use fern::colors::{Color, ColoredLevelConfig};
use fern::Dispatch;
use std::{io::stdout, path::PathBuf, process::exit};

use crate::input::{Command, ConfigCommand, Transport};
use crate::listener::{listen, Mode, Opts};

mod config;
mod control_actions;
mod control_ax;
mod control_bootstrap;
mod control_capabilities;
mod control_client_input;
mod control_core;
mod control_display;
mod control_frames;
mod control_gui_bench;
mod control_mouse;
mod control_observation;
mod control_protocol;
mod control_session;
mod control_transport;
mod control_web;
mod control_window;
mod daemon;
mod hidden_mode;
mod input;
mod listener;
mod pty_control;
mod screenshot;
mod shell;
mod zenoh_control;
mod zenoh_identity;
mod zenoh_runtime;

#[cfg(unix)]
mod unixshell;

#[cfg(windows)]
mod winshell;

const LEGACY_ZENOH_PEER_TRANSPORT_ERROR: &str =
    "旧 transport `zenoh-peer` 已废弃; 请改用 `--transport zenoh`。`rdog control` 默认会自动发现 router，必要时再补 `--entry-point tcp/<router-host>:<port>`";

fn host_from_opts(host: Vec<String>) -> Result<(String, String), String> {
    let fixed_host = if host.len() == 1 {
        ("0.0.0.0".to_string(), host.get(0).unwrap().to_string()) // Safe to unwrap here
    } else if let [host, port] = &host[..] {
        (host.to_string(), port.to_string())
    } else {
        return Err("Missing host".to_string());
    };

    Ok(fixed_host)
}

#[derive(Debug, Eq, PartialEq)]
enum ControlInvocation {
    Tcp {
        host: String,
        port: String,
    },
    WebSocket {
        url: String,
    },
    Zenoh {
        namespace: Option<String>,
        target_name: Option<String>,
        entry_point: Vec<String>,
    },
}

fn resolve_control_invocation(
    transport: Option<Transport>,
    url: Option<String>,
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    positional: Vec<String>,
) -> Result<ControlInvocation, String> {
    match transport {
        Some(Transport::Tcp) => {
            resolve_explicit_tcp_control(url, namespace, target_name, entry_point, positional)
        }
        Some(Transport::Zenoh) => {
            resolve_zenoh_control(url, namespace, target_name, entry_point, positional)
        }
        Some(Transport::ZenohPeerLegacy) => Err(LEGACY_ZENOH_PEER_TRANSPORT_ERROR.to_string()),
        None => resolve_inferred_control(url, namespace, target_name, entry_point, positional),
    }
}

fn resolve_explicit_tcp_control(
    url: Option<String>,
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    positional: Vec<String>,
) -> Result<ControlInvocation, String> {
    reject_zenoh_options_for_tcp(namespace, target_name, entry_point)?;

    if let Some(url) = url {
        return Ok(ControlInvocation::WebSocket { url });
    }

    let (host, port) = host_from_opts(positional)?;
    Ok(ControlInvocation::Tcp { host, port })
}

fn resolve_inferred_control(
    url: Option<String>,
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    positional: Vec<String>,
) -> Result<ControlInvocation, String> {
    if let Some(url) = url {
        reject_zenoh_options_for_url(namespace, target_name, entry_point)?;
        return Ok(ControlInvocation::WebSocket { url });
    }

    let has_zenoh_options = namespace.is_some() || target_name.is_some() || !entry_point.is_empty();
    if has_zenoh_options {
        return resolve_zenoh_control(None, namespace, target_name, entry_point, positional);
    }

    match positional.as_slice() {
        [] => Err(
            "缺少 control 目标: TCP 请写 `rdog control HOST PORT`,Zenoh 请写 `rdog control <target-name>`"
                .to_string(),
        ),
        [single] if single.parse::<u16>().is_ok() => Ok(ControlInvocation::Tcp {
            host: "0.0.0.0".to_string(),
            port: single.to_string(),
        }),
        [single] if looks_like_ipv4_address(single) => Err(format!(
            "单个 IPv4 地址 `{single}` 缺少端口; TCP 请写 `rdog control {single} PORT`,Zenoh 目标请使用非 IP 的 daemon 名"
        )),
        [single] => Ok(ControlInvocation::Zenoh {
            namespace,
            target_name: Some(single.to_string()),
            entry_point,
        }),
        [_, _] => {
            let (host, port) = host_from_opts(positional)?;
            Ok(ControlInvocation::Tcp { host, port })
        }
        _ => unreachable!("clap already limits control positional arguments to at most two"),
    }
}

fn resolve_zenoh_control(
    url: Option<String>,
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    positional: Vec<String>,
) -> Result<ControlInvocation, String> {
    if url.is_some() {
        return Err("`--transport zenoh` 不能和 `--url` 同时传入".to_string());
    }

    let target_name = merge_zenoh_target_name(target_name, positional)?;
    Ok(ControlInvocation::Zenoh {
        namespace,
        target_name,
        entry_point,
    })
}

fn merge_zenoh_target_name(
    target_name: Option<String>,
    positional: Vec<String>,
) -> Result<Option<String>, String> {
    match positional.as_slice() {
        [] => Ok(target_name),
        [target] if target_name.is_none() => Ok(Some(target.to_string())),
        [_] => Err(
            "`rdog control <target-name>` 不能和 `--target-name` 同时传入; 请只保留一个目标名"
                .to_string(),
        ),
        [_, _] => Err(
            "Zenoh control 只接受一个位置参数作为 target-name; TCP host/port 请显式使用 `--transport tcp HOST PORT`"
                .to_string(),
        ),
        _ => unreachable!("clap already limits control positional arguments to at most two"),
    }
}

fn reject_zenoh_options_for_tcp(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
) -> Result<(), String> {
    if namespace.is_some() || target_name.is_some() || !entry_point.is_empty() {
        return Err(
            "`--namespace`、`--target-name` 和 `--entry-point` 只能用于 Zenoh control".to_string(),
        );
    }

    Ok(())
}

fn reject_zenoh_options_for_url(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
) -> Result<(), String> {
    if namespace.is_some() || target_name.is_some() || !entry_point.is_empty() {
        return Err("`--url` 不能和 Zenoh control 选项同时传入".to_string());
    }

    Ok(())
}

fn looks_like_ipv4_address(value: &str) -> bool {
    let labels = value.split('.').collect::<Vec<_>>();
    labels.len() == 4 && labels.iter().all(|label| label.parse::<u8>().is_ok())
}

fn main() {
    let opts = input::Opts::parse();

    // 先初始化日志,后续所有错误都统一走顶层退出码。
    if let Err(err) = init_logger(&opts.command) {
        eprintln!("Failed to initialize logger: {err}");
        exit(1);
    }

    if let Err(err) = run(opts) {
        log::error!("{err}");
        exit(1);
    }
}

fn init_logger(command: &Command) -> Result<(), String> {
    let level = std::env::var("RDOG_LOG_LEVEL")
        .ok()
        .as_deref()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "error" => Some(log::LevelFilter::Error),
            "warn" | "warning" => Some(log::LevelFilter::Warn),
            "info" => Some(log::LevelFilter::Info),
            "debug" => Some(log::LevelFilter::Debug),
            "trace" => Some(log::LevelFilter::Trace),
            _ => None,
        })
        .unwrap_or(log::LevelFilter::Info);

    let dispatch = Dispatch::new()
        .format(|out, message, record| {
            let colors = ColoredLevelConfig::new()
                .warn(Color::Yellow)
                .info(Color::BrightGreen)
                .debug(Color::BrightBlue)
                .trace(Color::Magenta)
                .error(Color::Red);

            out.finish(format_args!(
                "{}{} {}",
                colors.color(record.level()).to_string().to_lowercase(),
                ":",
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .level(level);

    match hidden_mode::log_target_for_command(command) {
        hidden_mode::LogTarget::Stdout => dispatch
            .chain(stdout())
            .apply()
            .map_err(|err| err.to_string()),
        hidden_mode::LogTarget::File(path) => {
            let file = fern::log_file(&path).map_err(|err| {
                format!("failed to open hidden log file {}: {err}", path.display())
            })?;
            dispatch.chain(file).apply().map_err(|err| err.to_string())
        }
    }
}

fn run(opts: input::Opts) -> Result<(), String> {
    match opts.command {
        Command::Listen {
            interactive,
            block_signals,
            local_interactive,
            exec,
            host,
        } => {
            let (host, port) = match host_from_opts(host) {
                Ok(value) => value,
                Err(err) => return Err(err),
            };

            let opts = Opts {
                host,
                port,
                exec,
                block_signals,
                mode: if interactive {
                    Mode::Interactive
                } else if local_interactive {
                    Mode::LocalInteractive
                } else {
                    Mode::Normal
                },
            };

            listen(&opts).map_err(|err| err.to_string())?;
        }
        Command::Connect { shell, mode, host } => {
            let (host, port) = match host_from_opts(host) {
                Ok(value) => value,
                Err(err) => return Err(err),
            };

            let port = parse_port(&port)?;
            shell::connect_and_run_shell(&host, port, &shell, mode)
                .map_err(|err| err.to_string())?;
        }
        Command::Control {
            url,
            transport,
            namespace,
            target_name,
            entry_point,
            pty,
            pty_close,
            pty_detach,
            pty_attach,
            host,
            pty_command,
        } => {
            if pty && pty_command.is_empty() {
                return Err("`rdog control --pty` 需要在 `--` 后提供远端命令".to_string());
            }

            let invocation = resolve_control_invocation(
                transport,
                url,
                namespace,
                target_name,
                entry_point,
                host,
            )?;

            match invocation {
                ControlInvocation::Tcp { host, port } => {
                    let port = parse_port(&port)?;
                    if pty {
                        shell::control_remote_pty(&host, port, &pty_command)
                            .map_err(|err| err.to_string())?;
                    } else if let Some(session_id) = pty_close {
                        send_single_control_line_tcp(
                            &host,
                            port,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        send_single_control_line_tcp(
                            &host,
                            port,
                            &pty_control::render_pty_detach_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_attach {
                        let (cols, rows) = pty_control::default_terminal_size();
                        shell::control_remote_attach(&host, port, &session_id, cols, rows)
                            .map_err(|err| err.to_string())?;
                    } else {
                        shell::control_remote(&host, port).map_err(|err| err.to_string())?;
                    }
                }
                ControlInvocation::WebSocket { url } => {
                    if pty {
                        shell::control_remote_url_pty(&url, &pty_command)
                            .map_err(|err| err.to_string())?;
                    } else if let Some(session_id) = pty_close {
                        send_single_control_line_websocket(
                            &url,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        send_single_control_line_websocket(
                            &url,
                            &pty_control::render_pty_detach_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_attach {
                        let (cols, rows) = pty_control::default_terminal_size();
                        shell::control_remote_url_attach(&url, &session_id, cols, rows)
                            .map_err(|err| err.to_string())?;
                    } else {
                        shell::control_remote_url(&url).map_err(|err| err.to_string())?;
                    }
                }
                ControlInvocation::Zenoh {
                    namespace,
                    target_name,
                    entry_point,
                } => {
                    if pty {
                        shell::control_remote_zenoh_pty(
                            namespace,
                            target_name,
                            entry_point,
                            &pty_command,
                        )
                        .map_err(|err| err.to_string())?;
                    } else if let Some(session_id) = pty_close {
                        send_single_control_line_zenoh(
                            namespace,
                            target_name,
                            entry_point,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        send_single_control_line_zenoh(
                            namespace,
                            target_name,
                            entry_point,
                            &pty_control::render_pty_detach_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_attach {
                        let (cols, rows) = pty_control::default_terminal_size();
                        shell::control_remote_zenoh_attach(
                            namespace,
                            target_name,
                            entry_point,
                            &session_id,
                            cols,
                            rows,
                        )
                        .map_err(|err| err.to_string())?;
                    } else {
                        shell::control_remote_zenoh(namespace, target_name, entry_point)
                            .map_err(|err| err.to_string())?;
                    }
                }
            }
        }
        Command::Daemon {
            config,
            transport,
            namespace,
            daemon_name,
            entry_point,
        } => {
            let explicit_config_path = config.is_some();
            let daemon_config = config::load_daemon_config_unvalidated(config.as_deref())
                .map_err(|err| err.to_string())?;
            let transport =
                resolve_daemon_transport(transport, explicit_config_path, &daemon_config);

            match transport {
                Transport::Tcp => {
                    config::validate_tcp_daemon_profile(&daemon_config)
                        .map_err(|err| err.to_string())?;
                    daemon::run(daemon_config).map_err(|err| err.to_string())?;
                }
                Transport::Zenoh => {
                    daemon::run_zenoh_router(daemon_config, namespace, daemon_name, entry_point)
                        .map_err(|err| err.to_string())?;
                }
                Transport::ZenohPeerLegacy => {
                    return Err(LEGACY_ZENOH_PEER_TRANSPORT_ERROR.to_string())
                }
            }
        }
        Command::HiddenDaemon {
            config,
            child,
            log_file,
        } => {
            run_hidden_daemon(config, child, log_file)?;
        }
        Command::Config { command } => match command {
            ConfigCommand::Init { force } => {
                let paths =
                    config::write_example_configs_in_place(force).map_err(|err| err.to_string())?;

                for path in paths {
                    log::info!("已生成示例配置: {}", path.display());
                }
            }
        },
    }

    Ok(())
}

fn resolve_daemon_transport(
    requested_transport: Option<Transport>,
    explicit_config_path: bool,
    config: &config::DaemonConfig,
) -> Transport {
    requested_transport.unwrap_or_else(|| {
        if explicit_config_path && config.zenoh.enabled {
            Transport::Zenoh
        } else {
            Transport::Tcp
        }
    })
}

fn run_hidden_daemon(
    config_path: Option<PathBuf>,
    child: bool,
    log_file: Option<PathBuf>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        if child {
            // ------------------------------------------------------------
            // 隐藏子进程入口: 这里只做一次模式置位,随后仍然复用
            // 现有 daemon 配置加载与生命周期逻辑。
            // ------------------------------------------------------------
            hidden_mode::enable_hidden_session();
            let daemon_config = config::load_daemon_config(config_path.as_deref())
                .map_err(|err| err.to_string())?;

            // ------------------------------------------------------------
            // 这里额外校验一次内部传下来的日志路径,避免 parent/child
            // 之间参数漂移时静默退回 stdout。
            // ------------------------------------------------------------
            let Some(resolved_log_file) = log_file else {
                return Err("hidden child is missing internal --log-file".to_string());
            };

            if resolved_log_file != daemon_config.hidden.log_file {
                return Err(format!(
                    "hidden child log path mismatch: cli={}, config={}",
                    resolved_log_file.display(),
                    daemon_config.hidden.log_file.display()
                ));
            }

            daemon::run(daemon_config).map_err(|err| err.to_string())?;
            return Ok(());
        }

        let daemon_config =
            config::load_daemon_config(config_path.as_deref()).map_err(|err| err.to_string())?;

        hidden_mode::spawn_hidden_daemon_process(
            config_path.as_deref(),
            &daemon_config.hidden.log_file,
        )
        .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let _ = (config_path, child, log_file);
        Err("hidden-daemon is only supported on Windows".to_string())
    }
}

fn parse_port(port: &str) -> Result<u16, String> {
    port.parse::<u16>()
        .map_err(|err| format!("Invalid port `{port}`: {err}"))
}

fn send_single_control_line_tcp(host: &str, port: u16, line: &str) -> Result<(), String> {
    let mut transport = control_transport::ControlTransport::connect_tcp(host, port)
        .map_err(|err| err.to_string())?;
    send_single_control_line_transport(&mut transport, line)
}

fn send_single_control_line_websocket(url: &str, line: &str) -> Result<(), String> {
    let mut transport = control_transport::ControlTransport::connect_websocket(url)
        .map_err(|err| err.to_string())?;
    send_single_control_line_transport(&mut transport, line)
}

fn send_single_control_line_transport(
    transport: &mut control_transport::ControlTransport,
    line: &str,
) -> Result<(), String> {
    transport
        .write_message(line)
        .map_err(|err| err.to_string())?;
    if let Some(response) = transport.read_message().map_err(|err| err.to_string())? {
        println!("{response}");
    }
    Ok(())
}

fn send_single_control_line_zenoh(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    line: &str,
) -> Result<(), String> {
    zenoh_control::send_single_control_line(namespace, target_name, entry_point, 3_000, line)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_port, resolve_control_invocation, resolve_daemon_transport, ControlInvocation,
    };
    use crate::{
        config::{DaemonConfig, ZenohConfig},
        input::Transport,
    };

    #[test]
    fn control_invocation_should_treat_single_name_as_zenoh_target() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["mac.lab".to_string()],
        )
        .expect("single daemon name should resolve to zenoh");

        assert_eq!(
            invocation,
            ControlInvocation::Zenoh {
                namespace: None,
                target_name: Some("mac.lab".to_string()),
                entry_point: Vec::new()
            }
        );
    }

    #[test]
    fn control_invocation_should_keep_single_port_as_tcp_shorthand() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["5555".to_string()],
        )
        .expect("single numeric positional should stay tcp port shorthand");

        assert_eq!(
            invocation,
            ControlInvocation::Tcp {
                host: "0.0.0.0".to_string(),
                port: "5555".to_string()
            }
        );
    }

    #[test]
    fn control_invocation_should_keep_host_port_as_tcp() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["127.0.0.1".to_string(), "5555".to_string()],
        )
        .expect("two positional arguments should stay tcp host port");

        assert_eq!(
            invocation,
            ControlInvocation::Tcp {
                host: "127.0.0.1".to_string(),
                port: "5555".to_string()
            }
        );
    }

    #[test]
    fn control_invocation_should_infer_zenoh_from_target_name_flag() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            Some("mac.lab".to_string()),
            Vec::new(),
            Vec::new(),
        )
        .expect("target-name flag should imply zenoh");

        assert_eq!(
            invocation,
            ControlInvocation::Zenoh {
                namespace: None,
                target_name: Some("mac.lab".to_string()),
                entry_point: Vec::new()
            }
        );
    }

    #[test]
    fn control_invocation_should_reject_single_ipv4_without_port() {
        let err = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["127.0.0.1".to_string()],
        )
        .expect_err("single IPv4 positional should not be silently treated as target name");

        assert!(err.contains("缺少端口"));
    }

    #[test]
    fn control_invocation_should_keep_entrypoint_with_positional_zenoh_target() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            None,
            vec!["tcp/127.0.0.1:7447".to_string()],
            vec!["mac.lab".to_string()],
        )
        .expect("entrypoint plus single name should imply zenoh target");

        assert_eq!(
            invocation,
            ControlInvocation::Zenoh {
                namespace: None,
                target_name: Some("mac.lab".to_string()),
                entry_point: vec!["tcp/127.0.0.1:7447".to_string()]
            }
        );
    }

    #[test]
    fn control_invocation_should_reject_tcp_with_zenoh_options() {
        let err = resolve_control_invocation(
            Some(Transport::Tcp),
            None,
            None,
            Some("mac.lab".to_string()),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("explicit tcp should reject zenoh-only options");

        assert!(err.contains("只能用于 Zenoh control"));
    }

    #[test]
    fn parse_port_should_reject_invalid_port_numbers() {
        let err = parse_port("420692223").unwrap_err();

        assert!(err.contains("Invalid port"));
    }

    #[test]
    fn resolve_daemon_transport_should_infer_zenoh_from_config_when_flag_is_missing() {
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        assert_eq!(
            resolve_daemon_transport(None, true, &config),
            Transport::Zenoh
        );
    }

    #[test]
    fn resolve_daemon_transport_should_keep_explicit_transport_choice() {
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        assert_eq!(
            resolve_daemon_transport(Some(Transport::Tcp), true, &config),
            Transport::Tcp
        );
    }

    #[test]
    fn resolve_daemon_transport_should_keep_default_daemon_on_tcp_without_explicit_config() {
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        assert_eq!(
            resolve_daemon_transport(None, false, &config),
            Transport::Tcp
        );
    }
}
