use clap::Parser;
use fern::colors::{Color, ColoredLevelConfig};
use fern::Dispatch;
use std::{
    io::stderr,
    path::{Path, PathBuf},
    process::exit,
};
use tracing_subscriber::{filter::LevelFilter as TracingLevelFilter, fmt::writer::BoxMakeWriter};

use crate::control_recording::cli::{self as record_cli, RecordCommand};
use crate::input::{Command, ConfigCommand, RecordCommandShared, RecordSubcommand, Transport};
use crate::listener::{listen, Mode, Opts};

mod ax_action;
mod ax_diff;
mod ax_input;
mod ax_query;
mod cancellation;
mod config;
mod control_actions;
mod control_ax;
mod control_bootstrap;
mod control_capabilities;
mod control_client_input;
mod control_computer_act;
mod control_core;
mod control_display;
mod control_display_scope;
mod control_flow;
mod control_frames;
mod control_gui_bench;
mod control_invocation;
mod control_mouse;
mod control_observation;
mod control_protocol;
mod control_recording;
mod control_resource_lane;
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
// UI script runner 复用现有 line-control transport。
// 这里保持 CLI-side orchestration,不新增 daemon-side UI 协议。
mod ui_script;
mod ui_script_runner;
mod zenoh_control;
mod zenoh_identity;
mod zenoh_runtime;

#[cfg(unix)]
mod unixshell;

#[cfg(windows)]
mod winshell;

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

    let log_target = hidden_mode::log_target_for_command(command);
    init_tracing(level, &log_target)?;

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

    match log_target {
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

/// 为新增的结构化诊断事件安装 subscriber,但不接管既有 `log` / `fern` 调用。
///
/// Zenoh 也依赖 `tracing-subscriber`,Cargo feature 会全局合并,因此不能仅依赖
/// 本 crate 的 `default-features = false` 来阻止 `tracing-log`。这里直接安装
/// tracing subscriber,不调用 `try_init()`,让下方 `fern::Dispatch::apply` 始终是
/// `log` 的唯一全局 logger。两套事件共用同一日志等级和目标。
fn init_tracing(level: log::LevelFilter, target: &hidden_mode::LogTarget) -> Result<(), String> {
    let writer = match target {
        hidden_mode::LogTarget::Stderr => BoxMakeWriter::new(stderr),
        hidden_mode::LogTarget::File(path) => {
            let file = fern::log_file(path).map_err(|err| {
                format!(
                    "failed to open hidden tracing log file {}: {err}",
                    path.display()
                )
            })?;
            BoxMakeWriter::new(file)
        }
    };

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing_level_filter(level))
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))
}

/// `RDOG_LOG_LEVEL` 是唯一的运行时日志等级来源。这里仅把既有 log filter 映射到
/// tracing filter,避免两个日志系统因同一环境变量而出现不同的可见性。
fn tracing_level_filter(level: log::LevelFilter) -> TracingLevelFilter {
    match level {
        log::LevelFilter::Off => TracingLevelFilter::OFF,
        log::LevelFilter::Error => TracingLevelFilter::ERROR,
        log::LevelFilter::Warn => TracingLevelFilter::WARN,
        log::LevelFilter::Info => TracingLevelFilter::INFO,
        log::LevelFilter::Debug => TracingLevelFilter::DEBUG,
        log::LevelFilter::Trace => TracingLevelFilter::TRACE,
    }
}

fn run(opts: input::Opts) -> Result<(), String> {
    match opts.command {
        Command::Record {
            subcommand,
            url,
            transport,
            namespace,
            target_name,
            entry_point,
            host,
        } => {
            let record_subcommand = match subcommand {
                RecordSubcommand::Start { profile, duration } => RecordCommand::Start {
                    profile,
                    duration_ms: duration,
                },
                RecordSubcommand::Status => RecordCommand::Status,
                RecordSubcommand::Mark {
                    label,
                    redaction_active,
                } => RecordCommand::Mark {
                    label,
                    redaction_active,
                },
                RecordSubcommand::Stop => RecordCommand::Stop,
                RecordSubcommand::Cancel => RecordCommand::Cancel,
            };
            let shared = RecordCommandShared {
                url,
                transport,
                namespace,
                target_name,
                entry_point,
                host,
            };
            record_cli::run(
                record_subcommand,
                &shared,
                &record_cli::default_artifacts_dir(),
            )?;
        }
        Command::Listen {
            interactive,
            block_signals,
            local_interactive,
            exec,
            host,
        } => {
            let (host, port) = match control_invocation::host_from_opts(host) {
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
            let (host, port) = match control_invocation::host_from_opts(host) {
                Ok(value) => value,
                Err(err) => return Err(err),
            };

            let port = control_invocation::parse_port(&port)?;
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
            ui_script,
            dry_run,
            trace_dir,
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
            let (host, one_shot_lines) = control_invocation::extract_one_shot_lines(host);
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

            if let Some(script_path) = ui_script {
                if !one_shot_lines.is_empty() {
                    return Err(
                        "`rdog control --ui-script` 不能和尾部 `@<line>` one-shot 同时使用"
                            .to_string(),
                    );
                }
                if pty || pty_close.is_some() || pty_detach.is_some() || pty_attach.is_some() {
                    return Err("`rdog control --ui-script` 不能和 PTY 操作同时使用".to_string());
                }
                if !pty_command.is_empty() {
                    return Err(
                        "`rdog control --ui-script` 不接受 `--` 后的远端 PTY 命令".to_string()
                    );
                }

                let mut positional = host;
                positional.push(script_path.to_string_lossy().into_owned());
                ui_script_runner::run(ui_script_runner::UiScriptRunOptions {
                    dry_run,
                    url,
                    transport,
                    namespace,
                    target_name,
                    entry_point,
                    trace_dir,
                    positional,
                })?;
                return Ok(());
            }

            if pty && pty_command.is_empty() {
                return Err("`rdog control --pty` 需要在 `--` 后提供远端命令".to_string());
            }

            let invocation = control_invocation::resolve_control_invocation(
                transport,
                url,
                namespace,
                target_name,
                entry_point,
                host,
            )?;

            if !one_shot_lines.is_empty() {
                control_invocation::send_control_lines_for_invocation(
                    &invocation,
                    &one_shot_lines,
                    Path::new("rdog_downloads"),
                )?;
                return Ok(());
            }

            match invocation {
                control_invocation::ControlInvocation::Tcp { host, port } => {
                    let port = control_invocation::parse_port(&port)?;
                    if pty {
                        shell::control_remote_pty(&host, port, &pty_command)
                            .map_err(|err| err.to_string())?;
                    } else if let Some(session_id) = pty_close {
                        control_invocation::send_single_control_line_tcp(
                            &host,
                            port,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        control_invocation::send_single_control_line_tcp(
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
                control_invocation::ControlInvocation::WebSocket { url } => {
                    if pty {
                        shell::control_remote_url_pty(&url, &pty_command)
                            .map_err(|err| err.to_string())?;
                    } else if let Some(session_id) = pty_close {
                        control_invocation::send_single_control_line_websocket(
                            &url,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        control_invocation::send_single_control_line_websocket(
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
                control_invocation::ControlInvocation::Zenoh {
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
                        control_invocation::send_single_control_line_zenoh(
                            namespace,
                            target_name,
                            entry_point,
                            &pty_control::render_pty_close_line(&session_id)
                                .map_err(|err| err.to_string())?,
                        )?;
                    } else if let Some(session_id) = pty_detach {
                        control_invocation::send_single_control_line_zenoh(
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
                control_invocation::ControlInvocation::ZenohLocal { namespace } => {
                    // `rdog control self @<line>` / 空 target 的本机 fast path。
                    // PTY 不支持(one-shot 支持,直接走 send_control_lines_zenoh 复用同 session)。
                    if pty || pty_close.is_some() || pty_detach.is_some() || pty_attach.is_some() {
                        return Err(
                            "`rdog control self` / 空 target 不支持 PTY 操作,请显式指定 target name"
                                .to_string(),
                        );
                    }

                    // 本机默认选择由runtime层统一处理:
                    // 只接受active managed local-default registry,FIFO扫描仅用于升级诊断。
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

                    // one-shot 已在进入 match 前统一处理。
                    // 这里仅保留本机默认 daemon 的交互式 stdin/stdout 路径。
                    shell::control_remote_zenoh(
                        Some(resolved_namespace),
                        Some(target_name),
                        vec![],
                    )
                    .map_err(|err| err.to_string())?;
                }
            }
        }
        Command::UiScript {
            command:
                input::UiScriptCommand::Run {
                    dry_run,
                    url,
                    transport,
                    namespace,
                    target_name,
                    entry_point,
                    trace_dir,
                    positional,
                },
        } => {
            ui_script_runner::run(ui_script_runner::UiScriptRunOptions {
                dry_run,
                url,
                transport,
                namespace,
                target_name,
                entry_point,
                trace_dir,
                positional,
            })?;
        }
        Command::Daemon {
            config,
            transport,
            namespace,
            daemon_name,
            entry_point,
        } => {
            let daemon_config = config::load_daemon_config_unvalidated(config.as_deref())
                .map_err(|err| err.to_string())?;
            let transport = resolve_daemon_transport(transport, &daemon_config);

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
                    return Err(control_invocation::LEGACY_ZENOH_PEER_TRANSPORT_ERROR.to_string())
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
    config: &config::DaemonConfig,
) -> Transport {
    // 显式 `--transport` 最高优先;否则跟随合并后配置的 zenoh.enabled。
    // 用户级配置目录 (`~/.rdog`) 引入后,无 `--config` 时也可能加载到 zenoh profile,
    // 因此 transport 推断必须只看最终配置,不再要求 `--config` 显式传入。
    requested_transport.unwrap_or_else(|| {
        if config.zenoh.enabled {
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

#[cfg(test)]
mod tests {
    use super::resolve_daemon_transport;
    use crate::{
        config::{DaemonConfig, ZenohConfig},
        input::Transport,
    };

    #[test]
    fn resolve_daemon_transport_should_infer_zenoh_from_config_when_flag_is_missing() {
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        assert_eq!(resolve_daemon_transport(None, &config), Transport::Zenoh);
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
            resolve_daemon_transport(Some(Transport::Tcp), &config),
            Transport::Tcp
        );
    }

    #[test]
    fn resolve_daemon_transport_should_keep_tcp_when_zenoh_is_disabled_in_config() {
        let config = DaemonConfig::default();

        assert_eq!(resolve_daemon_transport(None, &config), Transport::Tcp);
    }

    #[test]
    fn resolve_daemon_transport_should_infer_zenoh_without_explicit_config_path() {
        // 用户级配置目录场景: 无 `--config`,但合并后配置 zenoh.enabled=true。
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        assert_eq!(resolve_daemon_transport(None, &config), Transport::Zenoh);
    }
}
