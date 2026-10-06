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

mod agent_messaging;
mod agent_runtime;
mod auth_credentials;
mod ax_action;
mod ax_diff;
mod ax_input;
mod ax_query;
mod cancellation;
mod config;
mod control_actions;
mod control_ax;
mod control_bootstrap;
mod control_cdp;
mod control_capabilities;
mod control_client_input;
mod control_computer_act;
mod control_core;
mod control_display;
mod control_display_scope;
mod control_file_transfer;
mod control_flow;
mod control_frames;
mod control_gui_bench;
mod control_invocation;
mod control_line_read;
mod control_mouse;
mod control_observation;
mod control_ocr;
mod control_overlay;

mod control_protocol;
mod control_recording;
mod control_resource_lane;
mod control_session;
mod control_transport;
mod control_web;
mod control_window;
mod daemon;
mod hidden_mode;
mod win_dpi;
mod input;
mod listener;
mod pty_control;
mod screenshot;
mod shell;
mod task_control;
mod tls_material;
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
    // DPI awareness 必须在任何窗口创建之前设置 (Windows 只接受第一次,
    // 首窗后锁定) — 因此放在绝对最前, 覆盖所有子命令含 hidden-daemon --child。
    win_dpi::enable_dpi_awareness();

    let opts = input::Opts::parse();

    // 日志目标在这里一次算清,init_logger 和下面的错误出口共用同一个判定结果,
    // 避免"日志写到哪"这件事在同一个进程里被推导两次而漂移。
    let log_target = hidden_mode::log_target_for_command(&opts.command);

    // 先初始化日志,后续所有错误都统一走顶层退出码。
    if let Err(err) = init_logger(&log_target) {
        eprintln!("Failed to initialize logger: {err}");
        exit(1);
    }

    if let Err(err) = run(opts) {
        log::error!("{err}");
        // ------------------------------------------------------------
        // 启动失败必须让人看见 —— 哪怕日志主体被 RDOG_LOG_FILE 改道到了文件。
        //
        // 2026-10-06 真机实测 (Windows 11): 同一份二进制、同一条
        // `rdog daemon -c bad.toml`, 唯一差别是设不设 RDOG_LOG_FILE:
        // 不设 → stderr 519 字节; 设了 → stderr 0 字节, 519 字节全进了文件,
        // 且没有任何一行提示说明日志去了哪。环境变量一旦被 setx 写进用户环境,
        // 后面每一条命令都走改道这条路, 现场据此把"日志去文件了"
        // 误判成"daemon 静默崩溃", 排查为此多绕了好几轮。
        //
        // 这里只对"文件背后还有操作者"的形态镜像一份: hidden child 的 stderr
        // 接的是 Stdio::null(), 通知没有收件人, 只会变成一次无意义的写入失败。
        // hidden child 仍然只有日志文件这一个现场, 契约不变。
        // ------------------------------------------------------------
        if let hidden_mode::LogTarget::File {
            console_observer: true,
            ..
        } = &log_target
        {
            eprintln!("error: {err}");
        }
        exit(1);
    }
}

fn init_logger(log_target: &hidden_mode::LogTarget) -> Result<(), String> {
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

    init_tracing(level, log_target)?;

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
        // hidden 子进程走 file 不变,保持 Windows 隐藏 resident 模式契约;
        // `rdog daemon` 也能用 RDOG_LOG_FILE 显式改道落盘(见 hidden_mode::LOG_FILE_ENV)。
        // ------------------------------------------------------------
        hidden_mode::LogTarget::Stderr => dispatch
            .chain(stderr())
            .apply()
            .map_err(|err| err.to_string()),
        hidden_mode::LogTarget::File { path, .. } => {
            let file = fern::log_file(&path)
                .map_err(|err| format!("failed to open log file {}: {err}", path.display()))?;
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
        hidden_mode::LogTarget::File { path, .. } => {
            let file = fern::log_file(path).map_err(|err| {
                format!("failed to open tracing log file {}: {err}", path.display())
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
        Command::Auth { command } => match command {
            input::AuthCommand::TlsInit => {
                let user_config_dir = config::resolve_user_config_dir()
                    .ok_or_else(|| "无法定位用户配置目录 (~/.rdog)".to_string())?;
                let report =
                    tls_material::tls_init(&user_config_dir.join(tls_material::TLS_DIR_NAME))
                        .map_err(|err| err.to_string())?;
                if report.created.is_empty() {
                    println!(
                        "TLS 材料已存在, 跳过 (不覆盖): {}",
                        report.skipped.join(", ")
                    );
                } else {
                    println!("TLS 材料已生成 (~/.rdog/tls/):");
                    for item in &report.created {
                        println!("  {item}");
                    }
                    println!(
                        "客户端分发: 拷贝 ca.pem (服务器验证模式); mTLS 再加 client/{{cert,key}}.pem"
                    );
                }
            }
        },
        Command::Agent {
            name,
            namespace,
            target_name,
            entry_point,
        } => {
            // 伴生 agent (issue #74): 连接现有 daemon, mailbox 注册 + inbox 消费。
            // daemon 缺失时报错 (内嵌带起是记录在案的实现偏差, 见 agent_runtime 模块注释)
            let namespace = match namespace {
                Some(explicit) => explicit,
                None => crate::zenoh_identity::infer_namespace_from_daemon_name(&name).ok_or_else(
                    || format!("无法从 agent name `{name}` 推断 namespace, 请显式传 --namespace"),
                )?,
            };
            let daemon_name = match &target_name {
                Some(explicit) => explicit.clone(),
                None => zenoh_runtime::find_local_daemon_name(Some(&namespace))
                    .map_err(|err| err.to_string())?,
            };
            let entry_points = if entry_point.is_empty() {
                zenoh_runtime::resolve_client_connect_endpoints(
                    &[],
                    std::time::Duration::from_secs(3),
                    zenoh_runtime::UnixpipeClientProbe::new(Some(&namespace), Some(&daemon_name)),
                )
                .map_err(|err| err.to_string())?
            } else {
                entry_point
            };
            let session =
                zenoh_runtime::open_client_session(&entry_points).map_err(|err| err.to_string())?;
            let config = agent_runtime::resolve_agent_config(
                &namespace,
                &name,
                &daemon_name,
                std::sync::Arc::new(agent_runtime::EchoDecision),
            );
            println!("agent starting: name={name}, namespace={namespace}, daemon={daemon_name}");
            agent_runtime::run_agent(&session, &config).map_err(|err| err.to_string())?;
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
            // CDP lane 开关随配置初始化 (默认 false; spec 安全模型: 默认关闭)
            control_cdp::configure(daemon_config.cdp.enabled);
            // 先记下 transport 由谁决定, 横幅才能如实写出来源。
            // 只报"走了 Zenoh"而不报来源, 用户会反过来怀疑 rdog_win.toml 写错了,
            // 而不是意识到自己没写 `--transport tcp`。
            let transport_source = match transport {
                Some(_) => "命令行 --transport",
                None if daemon_config.zenoh.enabled => "配置 [zenoh].enabled = true",
                None => "默认 TCP",
            };
            let transport = resolve_daemon_transport(transport, &daemon_config);

            match transport {
                Transport::Tcp => {
                    config::validate_tcp_daemon_profile(&daemon_config)
                        .map_err(|err| err.to_string())?;
                    // 横幅先于 spawn / run: Windows 上进程马上就转入隐藏常驻,
                    // 这个窗口是用户唯一能当场读到"实际生效配置"的机会。
                    let banner = tcp_daemon_banner(&daemon_config);
                    log::info!("{banner}");
                    // 校验在 spawn 之前跑完: 隐藏 child 起来就脱离控制台,
                    // 配置有问题时必须让本命令当场报错,不能等 child 静默退出。
                    if spawn_windows_resident_daemon(
                        config.as_deref(),
                        &daemon_config.hidden.log_file,
                    )? {
                        return Ok(());
                    }
                    daemon::run(daemon_config).map_err(|err| err.to_string())?;
                }
                Transport::Zenoh => {
                    let banner = zenoh_daemon_banner(
                        &daemon_config,
                        namespace.as_deref(),
                        daemon_name.as_deref(),
                    );
                    log::info!("{banner}");
                    // ------------------------------------------------------------
                    // 2026-10-05 远程作业的头号误判源头: 仓库自带的 rdog_win.toml 里
                    // `[zenoh].enabled = true`, transport 因此判成 Zenoh, 而
                    // run_zenoh_router 会把 TCP inbound/outbound 一起关掉 ——
                    // 配置文件里躺着的 `[inbound].port = 5555` 于是永远没人 bind。
                    // 用户连 5555 失败,看不到任何 daemon 侧日志,只能判成"进程崩了"。
                    // ------------------------------------------------------------
                    log::warn!(
                        "daemon 本次以 Zenoh transport 启动 (来源: {transport_source}): TCP inbound/outbound 已被强制关闭, 配置里的 [inbound]/[outbound] 端口不会 bind。要用 TCP 请显式 `rdog daemon --transport tcp`"
                    );
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
        Command::Overlay => {
            control_overlay::run_overlay_main().map_err(|err| err.to_string())?;
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

/// Windows 上让 `rdog daemon` 自动转成隐藏常驻进程。
///
/// 2026-10-05 实测: 用 `Start-Process` 直接起的 daemon 会随那个转瞬即逝的控制台窗口
/// 一起消失,当天最后靠 schtasks /SC ONLOGON 才稳住。项目里本来就有完整的隐藏 spawn
/// 路径(`hidden_mode::spawn_hidden_daemon_process`,CREATE_NO_WINDOW |
/// DETACHED_PROCESS),所以这里不新增参数,直接复用 —— 目标就是"敲 `rdog daemon` 就常驻"。
///
/// 只在 TCP transport 上启用: hidden child 走的是 `daemon::run` 且不接受 transport 覆盖,
/// 把 Zenoh 路由也塞进去会静默变成 TCP 语义,比不做还糟。
///
/// 逃生口: 环境变量 `RDOG_DAEMON_FOREGROUND` 置 1/true/yes 时保持前台。
/// 自动隐藏化之后,前台是唯一能实时看 stderr / 用 Ctrl-C 停掉的方式;没有这个开关,
/// 想边跑边看日志的人就只能去翻日志文件。用环境变量而不是新参数,是因为常驻形态的
/// 调用方(计划任务 / 脚本)本来就在拼环境,多一个必填 CLI 参数反而更容易漏。
///
/// 返回 `true` 表示"已交给隐藏 child",调用方必须立即返回,不要再前台 run。
/// 非 Windows 恒为 `false`,unix 侧行为保持不变。
fn spawn_windows_resident_daemon(
    config_path: Option<&Path>,
    log_file: &Path,
) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let foreground = matches!(
            std::env::var("RDOG_DAEMON_FOREGROUND")
                .ok()
                .as_deref()
                .map(str::trim),
            Some("1" | "true" | "yes")
        );
        if foreground {
            log::info!("RDOG_DAEMON_FOREGROUND 已置位, 保持前台运行, 不转隐藏常驻");
            return Ok(false);
        }

        hidden_mode::spawn_hidden_daemon_process(config_path, log_file)
            .map_err(|err| err.to_string())?;
        announce_hidden_resident(log_file);
        return Ok(true);
    }

    #[cfg(not(windows))]
    {
        let _ = (config_path, log_file);
        Ok(false)
    }
}

/// 隐藏常驻 spawn 成功后的终端告知。
///
/// 2026-10-05 的误判链条: stderr 绑在一个转瞬即逝的控制台窗口上,窗口一关日志就没了,
/// 于是"进程消失"被读成"静默崩溃"。这一行把日志落点钉在屏幕上,窗口关掉后仍照着它找。
#[cfg(windows)]
fn announce_hidden_resident(log_file: &Path) {
    println!("rdog daemon 已转入 Windows 隐藏常驻进程, 本命令立即返回。");
    println!("日志文件: {}", describe_log_path(log_file));
    println!("停止: taskkill /IM rdog.exe /F  (按进程名匹配, 会一并结束其它 rdog 进程)");
}

/// 日志落点文案: 相对路径按当前工作目录展开成绝对路径。
///
/// 隐藏常驻进程的 CWD 与用户敲命令的目录可能不同(计划任务尤其如此),
/// 只回显 `rdog_hidden.log` 这种相对路径等于没告诉用户日志在哪。
#[cfg(any(windows, test))]
fn describe_log_path(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }

    match std::env::current_dir() {
        Ok(dir) => dir.join(path).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// TCP daemon 的启动横幅文案: 一次把 transport / host:port / mode / shell 讲完。
///
/// 单独抽成纯函数是为了能直接断言文案 —— 这条横幅的价值全在"别再让人对着一个
/// 没人监听的端口猜",文案本身错了就等于没打。
fn tcp_daemon_banner(config: &config::DaemonConfig) -> String {
    let inbound = if config.inbound.enabled {
        format!(
            "inbound={}(mode={}, shell={}, transport={})",
            describe_endpoint_address(config.inbound.host.as_deref(), config.inbound.port),
            endpoint_mode_label(config.inbound.mode),
            config.inbound.shell.as_deref().unwrap_or("<未配置>"),
            control_transport_label(config.inbound.transport),
        )
    } else {
        "inbound=未启用".to_string()
    };

    let outbound = if config.outbound.enabled {
        format!(
            "outbound={}(mode={}, shell={})",
            describe_endpoint_address(config.outbound.host.as_deref(), config.outbound.port),
            endpoint_mode_label(config.outbound.mode),
            config.outbound.shell.as_deref().unwrap_or("<未配置>"),
        )
    } else {
        "outbound=未启用".to_string()
    };

    format!("daemon 启动: transport=tcp {inbound} {outbound}")
}

/// Zenoh router 的启动横幅文案。
///
/// namespace 与 listen_endpoints 都调用运行期真正使用的那两个解析函数,不复刻推断规则:
/// 横幅一旦自己算一遍,就成了第二个真相源,正好重演它要防的那个误判。
/// 解析失败时把结论写进横幅,错误本身仍由 `run_zenoh_router` 正式抛出。
fn zenoh_daemon_banner(
    config: &config::DaemonConfig,
    namespace_override: Option<&str>,
    daemon_name_override: Option<&str>,
) -> String {
    let daemon_name = daemon_name_override
        .or(config.zenoh.daemon_name.as_deref())
        .unwrap_or("<未配置>");

    let namespace =
        match crate::zenoh_identity::resolve_namespace(namespace_override, Some(daemon_name)) {
            Ok(namespace) => namespace,
            Err(err) => format!("<解析失败: {err}>"),
        };

    let listen = match crate::zenoh_runtime::compose_listen_endpoints(
        &config.zenoh,
        &namespace,
        daemon_name,
    ) {
        Ok(composed) => composed.listen_endpoints.join(", "),
        Err(err) => format!("<解析失败: {err}>"),
    };

    format!(
        "daemon 启动: transport=zenoh({}) name={daemon_name} namespace={namespace} listen=[{listen}] tcp=不启动",
        zenoh_mode_label(config.zenoh.mode)
    )
}

/// 端点地址文案: 缺 host/port 时明说缺哪一项,不用占位符糊过去。
fn describe_endpoint_address(host: Option<&str>, port: Option<u16>) -> String {
    match (host, port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (host, port) => format!(
            "{}:{}",
            host.unwrap_or("<host 未配置>"),
            port.map(|value| value.to_string())
                .unwrap_or_else(|| "<port 未配置>".to_string())
        ),
    }
}

fn endpoint_mode_label(mode: config::EndpointMode) -> &'static str {
    match mode {
        config::EndpointMode::Interactive => "interactive",
        config::EndpointMode::Control => "control",
    }
}

fn control_transport_label(kind: control_transport::ControlTransportKind) -> &'static str {
    match kind {
        control_transport::ControlTransportKind::Tcp => "tcp",
        control_transport::ControlTransportKind::WebSocket => "websocket",
    }
}

fn zenoh_mode_label(mode: config::ZenohMode) -> &'static str {
    match mode {
        config::ZenohMode::Router => "router",
        config::ZenohMode::PeerLegacy => "peer-legacy",
    }
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
            control_cdp::configure(daemon_config.cdp.enabled);
            // 横幅必须由 child 打: 它已经脱离控制台, 日志文件是唯一留下来的现场。
            // parent 打的那份会跟着 parent 的窗口一起消失, 正是 2026-10-05 踩的坑。
            let banner = tcp_daemon_banner(&daemon_config);
            log::info!("{banner}");

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
        announce_hidden_resident(&daemon_config.hidden.log_file);
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
    use super::{
        describe_endpoint_address, describe_log_path, resolve_daemon_transport, tcp_daemon_banner,
        zenoh_daemon_banner,
    };
    use crate::{
        config::{
            DaemonConfig, EndpointMode, InboundConfig, OutboundConfig, UnixpipeConfig, ZenohConfig,
            ZenohMode,
        },
        control_transport::ControlTransportKind,
        input::Transport,
    };
    use std::path::PathBuf;

    /// rdog_win.toml 的 inbound 段形态: 5555 + control lane + powershell。
    fn win_inbound() -> InboundConfig {
        InboundConfig {
            enabled: true,
            host: Some("0.0.0.0".to_string()),
            port: Some(5555),
            shell: Some("powershell.exe".to_string()),
            mode: EndpointMode::Control,
            transport: ControlTransportKind::Tcp,
        }
    }

    #[test]
    fn tcp_daemon_banner_should_print_effective_lane_fields() {
        // 横幅的价值全在这四个字段上: transport / host:port / mode / shell。
        let config = DaemonConfig {
            inbound: win_inbound(),
            ..DaemonConfig::default()
        };

        let banner = tcp_daemon_banner(&config);
        assert!(banner.contains("transport=tcp"), "{banner}");
        assert!(
            banner.contains(
                "inbound=0.0.0.0:5555(mode=control, shell=powershell.exe, transport=tcp)"
            ),
            "{banner}"
        );
        assert!(banner.contains("outbound=未启用"), "{banner}");
    }

    #[test]
    fn tcp_daemon_banner_should_name_disabled_lanes() {
        // 端点没开时不能只字不提: 配置里明明写着 5555, 横幅却对它闭口不谈,
        // 用户只会读成"没注意", 而事实是根本没人 bind —— 2026-10-05 误判的形状。
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            inbound: InboundConfig {
                enabled: false,
                host: Some("0.0.0.0".to_string()),
                port: Some(5555),
                ..InboundConfig::default()
            },
            ..DaemonConfig::default()
        };

        let banner = tcp_daemon_banner(&config);
        assert!(banner.contains("inbound=未启用"), "{banner}");
        assert!(banner.contains("outbound=未启用"), "{banner}");
    }

    #[test]
    fn tcp_daemon_banner_should_mark_missing_host_and_port() {
        let config = DaemonConfig {
            inbound: InboundConfig {
                enabled: true,
                ..InboundConfig::default()
            },
            outbound: OutboundConfig {
                enabled: true,
                host: Some("10.0.0.5".to_string()),
                port: Some(4444),
                shell: Some("cmd.exe".to_string()),
                mode: EndpointMode::Interactive,
            },
            ..DaemonConfig::default()
        };

        let banner = tcp_daemon_banner(&config);
        assert!(
            banner.contains("inbound=<host 未配置>:<port 未配置>"),
            "{banner}"
        );
        assert!(banner.contains("outbound=10.0.0.5:4444"), "{banner}");
        assert!(banner.contains("mode=interactive"), "{banner}");
    }

    #[test]
    fn zenoh_daemon_banner_should_state_that_tcp_will_not_start() {
        // rdog_win.toml 的真实形态: zenoh.enabled = true 而 inbound 段里还躺着 5555。
        // 横幅必须把 "TCP 不启动" 写出来, 否则配置里的端口号就是纯误导。
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                mode: ZenohMode::Router,
                namespace: "lab".to_string(),
                daemon_name: Some("dongfeng-unity.lab".to_string()),
                listen_endpoints: vec!["udp/0.0.0.0:7447".to_string()],
                // unixpipe 在 unix 上默认开着, compose 会往列表最前注入一条,
                // 让断言依赖注入结果就等于依赖 $TMPDIR 长度。夹具里显式关掉, 只测显式部分。
                unixpipe: UnixpipeConfig {
                    enabled: false,
                    ..UnixpipeConfig::default()
                },
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        let banner = zenoh_daemon_banner(&config, None, None);
        assert!(banner.contains("transport=zenoh(router)"), "{banner}");
        assert!(banner.contains("name=dongfeng-unity.lab"), "{banner}");
        assert!(banner.contains("namespace=lab"), "{banner}");
        assert!(banner.contains("listen=[udp/0.0.0.0:7447]"), "{banner}");
        assert!(banner.contains("tcp=不启动"), "{banner}");
    }

    #[test]
    fn zenoh_daemon_banner_should_report_unresolvable_identity_instead_of_inventing_one() {
        // 解析不出来就如实说解析失败: 横幅自己另算一套 namespace 推断,
        // 就等于立了第二个真相源, 正好重演它要防的误判。
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        let banner = zenoh_daemon_banner(&config, None, None);
        assert!(banner.contains("name=<未配置>"), "{banner}");
        assert!(banner.contains("namespace=<解析失败"), "{banner}");
    }

    #[test]
    fn zenoh_daemon_banner_should_follow_explicit_overrides() {
        // --namespace / --name 是运行期真正的覆盖来源, 横幅必须跟着它们走,
        // 否则用户传了参数却看到配置里的旧名字, 又是一次"配置说了不算"。
        let config = DaemonConfig {
            zenoh: ZenohConfig {
                enabled: true,
                namespace: "lab".to_string(),
                daemon_name: Some("configured.lab".to_string()),
                ..ZenohConfig::default()
            },
            ..DaemonConfig::default()
        };

        let banner = zenoh_daemon_banner(&config, Some("other"), Some("override.other"));
        assert!(banner.contains("name=override.other"), "{banner}");
        assert!(banner.contains("namespace=other"), "{banner}");
    }

    #[test]
    fn describe_log_path_should_keep_absolute_path_as_is() {
        // 用 current_dir 拼绝对路径: 写死 "/var/log/..." 在 Windows 上不算绝对路径
        // (缺盘符), 会掉进相对分支, 让这条用例只在 unix 上成立。
        let path = std::env::current_dir()
            .expect("cwd")
            .join("rdog_daemon.log");
        assert_eq!(describe_log_path(&path), path.display().to_string());
    }

    #[test]
    fn describe_log_path_should_expand_relative_path_against_cwd() {
        // 隐藏常驻进程的 CWD 未必是用户敲命令的目录, 只回显相对路径等于没告诉用户日志在哪。
        let described = describe_log_path(&PathBuf::from("rdog_hidden.log"));
        assert!(described.ends_with("rdog_hidden.log"), "{described}");
    }

    #[test]
    fn describe_endpoint_address_should_name_the_missing_half() {
        assert_eq!(
            describe_endpoint_address(Some("127.0.0.1"), Some(5555)),
            "127.0.0.1:5555"
        );
        assert_eq!(
            describe_endpoint_address(None, Some(5555)),
            "<host 未配置>:5555"
        );
        assert_eq!(
            describe_endpoint_address(Some("0.0.0.0"), None),
            "0.0.0.0:<port 未配置>"
        );
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
