use clap::Parser;
use fern::colors::{Color, ColoredLevelConfig};
use fern::Dispatch;
use std::{io::stderr, path::PathBuf, process::exit};

use crate::input::{Command, ConfigCommand, Transport};
use crate::listener::{listen, Mode, Opts};

mod ax_diff;
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

/// 把 host 末尾连续以 `@` 开头的一组元素抽出当 one-shot line 列表。
///
/// 这是 `rdog control <target> @<line> [@<line> ...]` 这种无状态 CLI 入口的
/// 核心分流步骤。抽出来变成纯函数,方便单测覆盖:
/// - 空 host
/// - 末尾一个 `@` 元素
/// - 末尾 N 个 `@` 元素(单 line 形式就是 N=1 的特例)
/// - 末尾不是 `@` 开头(返回空 Vec,沿用旧 stdio 桥接)
/// - 多个元素、中间夹着非 `@` 时,只 pop 末尾连续 `@` 段,中间那一个留给后续校验报错
fn extract_one_shot_lines(host: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut host = host;
    let mut lines = Vec::new();
    while let Some(last) = host.last() {
        if last.starts_with('@') {
            // safe unwrap: last() returned Some in this branch
            lines.push(host.pop().unwrap());
        } else {
            break;
        }
    }
    // 保持用户输入顺序,不是 pop 出来的反序
    lines.reverse();
    (host, lines)
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
    /// 本机 fast path:用 `rdog control self @<line>` 或空 target 触发,
    /// 通过扫描 $TMPDIR/rdog-*.pipe_uplink 找唯一 unixpipe daemon。
    ZenohLocal {
        namespace: Option<String>,
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
        // --url 和非空 host 位置参数同时传入是真冲突。
        // 注:one-shot 入口 (`rdog control --url ws://... @<line>`) 已经在
        // main.rs 里把 `@<line>` 从 host 末尾剥出来了,这里看到的 host 一定不含
        // `@<line>`,所以非空就是真正的冲突,直接报错。
        if !positional.is_empty() {
            return Err(
                "`--url` 不能和位置参数 (target / host port) 同时传入;one-shot `@<line>` 只能跟在 URL 之后"
                    .to_string(),
            );
        }
        reject_zenoh_options_for_url(namespace, target_name, entry_point)?;
        return Ok(ControlInvocation::WebSocket { url });
    }

    let has_zenoh_options = namespace.is_some() || target_name.is_some() || !entry_point.is_empty();

    // `rdog control self @<line>` / `rdog control @<line>` 这种"省掉 target 名"的快捷入口。
    // 不允许和 --target-name 或 --entry-point 一起用(避免歧义)。
    if positional.as_slice() == ["self"] {
        if target_name.is_some() {
            return Err(
                "`rdog control self` 不能和 `--target-name` 同时传入;两者只能选一个".to_string(),
            );
        }
        if !entry_point.is_empty() {
            return Err(
                "`rdog control self` 不能和 `--entry-point` 同时传入;--entry-point 必须指定明确 target"
                    .to_string(),
            );
        }
        return Ok(ControlInvocation::ZenohLocal { namespace });
    }

    if has_zenoh_options {
        return resolve_zenoh_control(None, namespace, target_name, entry_point, positional);
    }

    match positional.as_slice() {
        // 空 target + 无 --namespace: 走 ZenohLocal 本机 fast path。
        // 跟 `self` 关键字路径一样,只是更简洁(`rdog control @<line>`)。
        [] => {
            if has_zenoh_options {
                // has_zenoh_options 已被前面的 if 拦截,这里走不到。
                unreachable!("空 positional + zenoh_options 已被前面 has_zenoh_options 分支接走");
            }
            return Ok(ControlInvocation::ZenohLocal { namespace: None });
        }
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
        // host: num_args = 0..=3 时,3 个非 `@` 位置参数是用户错误。
        // 3 个位置参数 + 1 个 trailing `@<line>` 会被 clap 在 num_args 处拦下,
        // 不会到这一步;到这里一定是 3 个非 `@` 元素,直接报错。
        _ => Err(format!(
            "control 位置参数最多 2 个 (target / host port);one-shot `@<line>` 必须放在最后;收到 {} 个位置参数 {:?}",
            positional.len(),
            positional
        )),
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

    // 没有 target_name 也没有 --entry-point → 本机 fast path。
    // 这种情况通常是 `rdog control --namespace lab @<line>`(空 target + 只有 namespace)。
    if target_name.is_none() && entry_point.is_empty() {
        return Ok(ControlInvocation::ZenohLocal { namespace });
    }

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
        // ------------------------------------------------------------
        // 非 hidden 命令的日志走 stderr(Unix 习惯:错误/警告不应混入 stdout,
        // 否则 agent 走 pipe / redirect 解析 stdout 时会被噪音打断)。
        // hidden 子进程走 file 不变,保持 Windows 隐藏 resident 模式契约。
        // ------------------------------------------------------------
        hidden_mode::LogTarget::Stderr => dispatch
            .chain(stderr())
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
            // ------------------------------------------------------------
            // one-shot 入口:把 `rdog control <target> @<line> [@<line> ...]`
            // 这种无状态形式替代 `printf ... | rdog control <target>`。
            //
            // clap 端 `host: num_args = 0..=32` 已经把 1..N 个 `@<line>`
            // 收进 host 末尾,这里 pop 出来后按输入顺序串行执行,
            // 共享同一条 transport(TCP / WebSocket / Zenoh session bridge)。
            // ------------------------------------------------------------
            let (host, one_shot_lines) = extract_one_shot_lines(host);
            if !one_shot_lines.is_empty() {
                if one_shot_lines.iter().any(|line| line.is_empty()) {
                    return Err("one-shot line 不能为空".to_string());
                }
                if host.iter().any(|item| item.starts_with('@')) {
                    return Err(
                        "one-shot 模式只支持尾部连续 `@<line> [@<line> ...]`;前面位置参数不应以 `@` 开头"
                            .to_string(),
                    );
                }
                if pty {
                    return Err("`rdog control <target> @<line> ...` 与 `--pty` 互斥".to_string());
                }
                if pty_close.is_some() {
                    return Err(
                        "`rdog control <target> @<line> ...` 与 `--pty-close` 互斥".to_string()
                    );
                }
                if pty_detach.is_some() {
                    return Err(
                        "`rdog control <target> @<line> ...` 与 `--pty-detach` 互斥".to_string()
                    );
                }
                if pty_attach.is_some() {
                    return Err(
                        "`rdog control <target> @<line> ...` 与 `--pty-attach` 互斥".to_string()
                    );
                }
                // one-shot line 不再前置拦截:空 target + 无 namespace
                // 会进入 ZenohLocal dispatch,让 find_local_daemon_name(None) 扫本地 daemon,
                // 找不到再返回清晰错误(避免和 self 路径语义不一致)。
            }

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
                    if !one_shot_lines.is_empty() {
                        send_control_lines_tcp(&host, port, &one_shot_lines)?;
                        return Ok(());
                    }
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
                    if !one_shot_lines.is_empty() {
                        send_control_lines_websocket(&url, &one_shot_lines)?;
                        return Ok(());
                    }
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
                    if !one_shot_lines.is_empty() {
                        send_control_lines_zenoh(
                            namespace,
                            target_name,
                            entry_point,
                            &one_shot_lines,
                        )?;
                        return Ok(());
                    }
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
                ControlInvocation::ZenohLocal { namespace } => {
                    // `rdog control self @<line>` / 空 target 的本机 fast path。
                    // PTY 不支持(one-shot 支持,直接走 send_control_lines_zenoh 复用同 session)。
                    if pty || pty_close.is_some() || pty_detach.is_some() || pty_attach.is_some() {
                        return Err(
                            "`rdog control self` / 空 target 不支持 PTY 操作,请显式指定 target name"
                                .to_string(),
                        );
                    }

                    // 扫描 $TMPDIR/rdog-{ns}-*.pipe_uplink 找唯一 daemon。
                    let target_name = zenoh_runtime::find_local_daemon_name(namespace.as_deref())
                        .map_err(|err| err.to_string())?;

                    // 推断 namespace(从 daemon_name 的点后缀),显式给的优先。
                    let resolved_namespace = namespace.clone().or_else(|| {
                        crate::zenoh_identity::infer_namespace_from_daemon_name(&target_name)
                    });

                    // 找不到 namespace 的两种情况:
                    // 1. 用户没传 --namespace 且 daemon_name 没点后缀(无法推断)
                    // 2. 用户传了 --namespace 但 daemon 不存在
                    // 这两种都属于用户配置错,统一报"需要 --namespace"。
                    let resolved_namespace = match resolved_namespace {
                        Some(ns) => ns,
                        None => {
                            return Err(format!(
                                "`rdog control self` 找不到 namespace;请传 `--namespace`(例如 `--namespace lab`)。daemon_name={target_name:?} 没有可推断的 namespace 后缀"
                            ));
                        }
                    };

                    // one-shot 走 send_control_lines_zenoh(单 session 串行);
                    // 否则走 control_remote_zenoh(交互式 stdin/stdout)。
                    if !one_shot_lines.is_empty() {
                        send_control_lines_zenoh(
                            Some(resolved_namespace),
                            Some(target_name),
                            vec![],
                            &one_shot_lines,
                        )?;
                        return Ok(());
                    }
                    shell::control_remote_zenoh(
                        Some(resolved_namespace),
                        Some(target_name),
                        vec![],
                    )
                    .map_err(|err| err.to_string())?;
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
        Command::AxDiff {
            before,
            after,
            format,
            quiet,
            top_changes,
            max_depth,
        } => {
            // 把 clap 解析的 Option<PathBuf> / Option<String> 装成 ax_diff 自己的 argv
            // 形态,这样子模块可以独立 --help / 测试,不需要把 clap 类型泄漏到 ax_diff 内部。
            let mut argv: Vec<String> = Vec::new();
            if let Some(b) = before {
                argv.push("--before".to_string());
                argv.push(b.display().to_string());
            }
            if let Some(a) = after {
                argv.push("--after".to_string());
                argv.push(a.display().to_string());
            }
            if let Some(f) = format {
                argv.push("--format".to_string());
                argv.push(f);
            }
            if quiet {
                argv.push("--quiet".to_string());
            }
            if let Some(n) = top_changes {
                argv.push("--top-changes".to_string());
                argv.push(n.to_string());
            }
            argv.push("--max-depth".to_string());
            argv.push(max_depth.to_string());
            // 退出码约定: 0=相同, 1=有差异, 2=用法错误, 3=JSON 解析失败。
            // 这里不走 main.rs 通用 Err 路径, 因为 main 通用路径会把任意
            // Err 变成 exit 1, 会让 ax_diff 的 2/3 退出码被吞掉。
            match ax_diff::parse_options(&argv) {
                Ok(opts) => {
                    let code = ax_diff::run(opts);
                    if code != 0 {
                        std::process::exit(code);
                    }
                }
                Err(err) => {
                    eprintln!("rdog ax-diff 参数错误: {err}");
                    std::process::exit(2);
                }
            }
        }
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

/// TCP 多 line one-shot 入口:一次性发一组 `@<line>`,共享同一条 TCP 连接。
///
/// 与 `send_single_control_line_tcp` 的区别:
/// - 走完整 frame 收口循环,能正确处理 `@screenshot` 这种 `@savefile` 多 frame 场景
/// - 一次 connect,不再每条重连
/// - 任一行失败整组退出
fn send_control_lines_tcp(host: &str, port: u16, lines: &[String]) -> Result<(), String> {
    let mut transport = control_transport::ControlTransport::connect_tcp(host, port)
        .map_err(|err| err.to_string())?;
    shell::run_line_control_lines(&mut transport, lines).map_err(|err| err.to_string())
}

/// WebSocket 多 line one-shot 入口,语义同 `send_control_lines_tcp`。
fn send_control_lines_websocket(url: &str, lines: &[String]) -> Result<(), String> {
    let mut transport = control_transport::ControlTransport::connect_websocket(url)
        .map_err(|err| err.to_string())?;
    shell::run_line_control_lines(&mut transport, lines).map_err(|err| err.to_string())
}

/// Zenoh 多 line one-shot 入口:复用一条 session bridge 串行执行一组 `@<line>`。
///
/// 任一行失败整组退出,不做行级重试(避免半成功半失败状态)。
fn send_control_lines_zenoh(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    lines: &[String],
) -> Result<(), String> {
    zenoh_control::send_control_lines(namespace, target_name, entry_point, 3_000, lines)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        extract_one_shot_lines, parse_port, resolve_control_invocation, resolve_daemon_transport,
        ControlInvocation,
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

    // ------------------------------------------------------------
    // extract_one_shot_lines 单元测试
    // ------------------------------------------------------------

    #[test]
    fn extract_one_shot_lines_should_return_empty_vec_when_host_is_empty() {
        let (host, lines) = extract_one_shot_lines(Vec::new());
        assert!(host.is_empty());
        assert!(lines.is_empty());
    }

    #[test]
    fn extract_one_shot_lines_should_leave_non_at_tail_untouched() {
        let (host, lines) = extract_one_shot_lines(vec!["mac.lab".to_string()]);
        assert_eq!(host, vec!["mac.lab".to_string()]);
        assert!(lines.is_empty());
    }

    #[test]
    fn extract_one_shot_lines_should_leave_host_port_untouched() {
        let (host, lines) =
            extract_one_shot_lines(vec!["127.0.0.1".to_string(), "5555".to_string()]);
        assert_eq!(host, vec!["127.0.0.1".to_string(), "5555".to_string()]);
        assert!(lines.is_empty());
    }

    #[test]
    fn extract_one_shot_lines_should_pop_single_trailing_at_line_after_target() {
        let (host, lines) =
            extract_one_shot_lines(vec!["mac.lab".to_string(), "@ping".to_string()]);
        assert_eq!(host, vec!["mac.lab".to_string()]);
        assert_eq!(lines, vec!["@ping".to_string()]);
    }

    #[test]
    fn extract_one_shot_lines_should_pop_single_trailing_at_line_after_host_port() {
        let (host, lines) = extract_one_shot_lines(vec![
            "127.0.0.1".to_string(),
            "5555".to_string(),
            "@capabilities#1".to_string(),
        ]);
        assert_eq!(host, vec!["127.0.0.1".to_string(), "5555".to_string()]);
        assert_eq!(lines, vec!["@capabilities#1".to_string()]);
    }

    #[test]
    fn extract_one_shot_lines_should_pop_consecutive_at_lines_in_input_order() {
        // 多个连续 `@` 起始 token 都要 pop,且按用户输入顺序返回
        let (host, lines) = extract_one_shot_lines(vec![
            "mac.lab".to_string(),
            "@ping".to_string(),
            "@capabilities#1".to_string(),
            "@observe#3".to_string(),
        ]);
        assert_eq!(host, vec!["mac.lab".to_string()]);
        assert_eq!(
            lines,
            vec![
                "@ping".to_string(),
                "@capabilities#1".to_string(),
                "@observe#3".to_string(),
            ]
        );
    }

    #[test]
    fn extract_one_shot_lines_should_stop_popping_at_non_at_element() {
        // 末尾非 `@` 时,前面所有 `@` 都不动
        let (host, lines) = extract_one_shot_lines(vec![
            "mac.lab".to_string(),
            "@ping".to_string(),
            "extra".to_string(),
        ]);
        assert_eq!(
            host,
            vec![
                "mac.lab".to_string(),
                "@ping".to_string(),
                "extra".to_string()
            ]
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn extract_one_shot_lines_should_keep_object_payload_intact() {
        // 对象 payload 整段保留
        let payload = r#"@key#7:{key:"right-control",hold_ms:200}"#;
        let (host, lines) =
            extract_one_shot_lines(vec!["mac.lab".to_string(), payload.to_string()]);
        assert_eq!(host, vec!["mac.lab".to_string()]);
        assert_eq!(lines, vec![payload.to_string()]);
    }
}
