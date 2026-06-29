use clap::Parser;
use fern::colors::{Color, ColoredLevelConfig};
use fern::Dispatch;
use std::{
    fs::{self, OpenOptions},
    io::{stderr, Write as _},
    path::{Path, PathBuf},
    process::exit,
    time::{SystemTime, UNIX_EPOCH},
};

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
mod control_display_scope;
mod control_flow;
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
// UI script runner 复用现有 line-control transport。
// 这里保持 CLI-side orchestration,不新增 daemon-side UI 协议。
mod ui_script;
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
    /// 优先读 local-default registry,没有 registry 时再扫描唯一 unixpipe FIFO。
    ZenohLocal {
        namespace: Option<String>,
    },
}

struct UiScriptRunOptions {
    dry_run: bool,
    url: Option<String>,
    transport: Option<Transport>,
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    trace_dir: Option<PathBuf>,
    positional: Vec<String>,
}

#[derive(Debug, Clone)]
struct UiScriptArtifactRecord {
    filename: String,
    mime: String,
    path: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Clone)]
struct ControlLineExchange {
    line: String,
    frames: Vec<control_frames::ControlFrame>,
    response_line: Option<String>,
    artifacts: Vec<UiScriptArtifactRecord>,
}

#[derive(Debug, Clone)]
struct PendingUiScriptControlLine {
    step_index: usize,
    step_kind: &'static str,
    line: String,
}

struct UiScriptRunState {
    run_id: String,
    run_dir: PathBuf,
    artifacts_dir: PathBuf,
    trace_path: PathBuf,
    trace_file: fs::File,
    completed_step_count: usize,
    failed_step_index: Option<usize>,
    last_response_line: Option<String>,
    last_response_value: Option<serde_json::Value>,
    last_artifacts: Vec<UiScriptArtifactRecord>,
}

impl UiScriptRunState {
    fn create(
        script_path: &Path,
        dry_run: &ui_script::UiScriptDryRun,
        trace_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let run_id = build_ui_script_run_id();
        let run_dir = trace_dir.unwrap_or_else(|| PathBuf::from("rdog_script_runs").join(&run_id));
        let artifacts_dir = run_dir.join("artifacts");
        fs::create_dir_all(&artifacts_dir).map_err(|err| {
            format!(
                "创建 UI script artifacts 目录失败: {}: {err}",
                artifacts_dir.display()
            )
        })?;
        let trace_path = run_dir.join("trace.jsonl");
        let trace_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&trace_path)
            .map_err(|err| format!("创建 UI script trace 失败: {}: {err}", trace_path.display()))?;

        write_ui_script_normalized_plan(&run_dir, script_path, dry_run, &run_id)?;
        write_ui_script_summary(&run_dir, dry_run, &run_id, "running", 0, None, None)?;

        Ok(Self {
            run_id,
            run_dir,
            artifacts_dir,
            trace_path,
            trace_file,
            completed_step_count: 0,
            failed_step_index: None,
            last_response_line: None,
            last_response_value: None,
            last_artifacts: Vec::new(),
        })
    }

    fn record_step(&mut self, record: serde_json::Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.trace_file, &record)
            .map_err(|err| format!("写入 UI script trace JSON 失败: {err}"))?;
        writeln!(self.trace_file).map_err(|err| format!("写入 UI script trace 换行失败: {err}"))?;
        self.trace_file
            .flush()
            .map_err(|err| format!("刷新 UI script trace 失败: {err}"))
    }
}

fn split_ui_script_run_positionals(
    mut positional: Vec<String>,
) -> Result<(Vec<String>, PathBuf), String> {
    let Some(script_path) = positional.pop() else {
        return Err("`rdog ui-script run` 需要脚本文件路径".to_string());
    };
    if positional.iter().any(|item| item.starts_with('@')) {
        return Err(
            "`rdog ui-script run` 的 target 位置参数不能是 `@<line>`;脚本内容应写在 JSON 文件里"
                .to_string(),
        );
    }
    Ok((positional, PathBuf::from(script_path)))
}

fn apply_ui_script_target(
    program: &ui_script::UiScriptProgram,
    control_positionals: &mut Vec<String>,
    namespace: &mut Option<String>,
    target_name: &mut Option<String>,
) -> Result<(), String> {
    let mut targets = program.steps.iter().filter_map(|step| match step {
        ui_script::UiScriptStep::Target(payload) => Some(payload),
        _ => None,
    });
    let Some(target) = targets.next() else {
        return Ok(());
    };
    if targets.next().is_some() {
        return Err("UI script 只能声明一个 Target step".to_string());
    }

    if let Some(script_namespace) = target.get("namespace").and_then(serde_json::Value::as_str) {
        match namespace {
            Some(cli_namespace) if cli_namespace != script_namespace => {
                return Err(format!(
                    "CLI --namespace={cli_namespace} 与脚本 Target.namespace={script_namespace} 不一致"
                ));
            }
            Some(_) => {}
            None => *namespace = Some(script_namespace.to_owned()),
        }
    }

    let Some(script_target_name) = target.get("name").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    if let Some(cli_target_name) = target_name.as_deref() {
        if cli_target_name != script_target_name {
            return Err(format!(
                "CLI --target-name={cli_target_name} 与脚本 Target.name={script_target_name} 不一致"
            ));
        }
        return Ok(());
    }
    if control_positionals.is_empty() {
        control_positionals.push(script_target_name.to_owned());
        return Ok(());
    }
    if control_positionals.len() == 1 && control_positionals[0] == script_target_name {
        return Ok(());
    }
    Err(format!(
        "CLI target {:?} 与脚本 Target.name={script_target_name} 不一致",
        control_positionals
    ))
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

            if !one_shot_lines.is_empty() {
                send_control_lines_for_invocation(
                    &invocation,
                    &one_shot_lines,
                    Path::new("rdog_downloads"),
                )?;
                return Ok(());
            }

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
                ControlInvocation::ZenohLocal { namespace } => {
                    // `rdog control self @<line>` / 空 target 的本机 fast path。
                    // PTY 不支持(one-shot 支持,直接走 send_control_lines_zenoh 复用同 session)。
                    if pty || pty_close.is_some() || pty_detach.is_some() || pty_attach.is_some() {
                        return Err(
                            "`rdog control self` / 空 target 不支持 PTY 操作,请显式指定 target name"
                                .to_string(),
                        );
                    }

                    // 本机默认选择由 runtime 层统一处理:
                    // 先读 local-default registry,再 fallback 到唯一 FIFO 扫描。
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
            run_ui_script(UiScriptRunOptions {
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

fn run_ui_script(options: UiScriptRunOptions) -> Result<(), String> {
    let mut options = options;
    let (mut control_positionals, script_path) =
        split_ui_script_run_positionals(options.positional)?;
    let program = ui_script::parse_script_file(&script_path).map_err(|err| err.to_string())?;
    apply_ui_script_target(
        &program,
        &mut control_positionals,
        &mut options.namespace,
        &mut options.target_name,
    )?;
    let dry_run = ui_script::compile_dry_run(&program).map_err(|err| err.to_string())?;

    if options.dry_run {
        emit_ui_script_dry_run(&script_path, &dry_run);
        return Ok(());
    }

    let invocation = resolve_control_invocation(
        options.transport,
        options.url,
        options.namespace,
        options.target_name,
        options.entry_point,
        control_positionals,
    )?;
    let mut state = UiScriptRunState::create(&script_path, &dry_run, options.trace_dir)?;
    let result = execute_ui_script_plan(&invocation, &dry_run, &mut state);
    let (status, error) = match &result {
        Ok(()) => ("complete", None),
        Err(err) => ("failed", Some(err.as_str())),
    };
    write_ui_script_summary(
        &state.run_dir,
        &dry_run,
        &state.run_id,
        status,
        state.completed_step_count,
        state.failed_step_index,
        error,
    )?;
    println!("ui-script trace: {}", state.trace_path.display());
    result
}

fn emit_ui_script_dry_run(script_path: &PathBuf, dry_run: &ui_script::UiScriptDryRun) {
    println!("ui-script dry-run: {}", script_path.display());
    println!(
        "summary: steps={}, backend_requests={}, semantic_actions={}, mouse_fallbacks={}",
        dry_run.summary.step_count,
        dry_run.summary.backend_request_count,
        dry_run.summary.semantic_action_count,
        dry_run.summary.mouse_fallback_count
    );
    for step in &dry_run.steps {
        match &step.effect {
            ui_script::UiScriptDryRunEffect::Context(label) => {
                println!("step {} {} local context:{label}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Local(label) => {
                println!("step {} {} local {label}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Expect(payload) => {
                let kind = payload
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                println!("step {} {} expect {kind}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::ControlLine(line) => {
                println!("step {} {} control {line}", step.index, step.kind);
            }
            ui_script::UiScriptDryRunEffect::Exit => {
                println!("step {} {} exit", step.index, step.kind);
            }
        }
    }
}

fn execute_ui_script_plan(
    invocation: &ControlInvocation,
    dry_run: &ui_script::UiScriptDryRun,
    state: &mut UiScriptRunState,
) -> Result<(), String> {
    let mut pending_lines = Vec::new();
    for step in &dry_run.steps {
        match &step.effect {
            ui_script::UiScriptDryRunEffect::ControlLine(line) => {
                pending_lines.push(PendingUiScriptControlLine {
                    step_index: step.index,
                    step_kind: step.kind,
                    line: line.clone(),
                });
            }
            ui_script::UiScriptDryRunEffect::Context(label) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                record_ui_script_local_step(state, step.index, step.kind, "context", label, None)?;
            }
            ui_script::UiScriptDryRunEffect::Local(label) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                match execute_ui_script_local_effect(label) {
                    Ok(()) => {
                        record_ui_script_local_step(
                            state, step.index, step.kind, "local", label, None,
                        )?;
                    }
                    Err(err) => {
                        record_ui_script_local_step(
                            state,
                            step.index,
                            step.kind,
                            "local",
                            label,
                            Some(err.as_str()),
                        )?;
                        state.failed_step_index = Some(step.index);
                        return Err(err);
                    }
                }
            }
            ui_script::UiScriptDryRunEffect::Expect(payload) => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                match evaluate_ui_script_expect(payload, state) {
                    Ok(()) => {
                        record_ui_script_expect_step(state, step.index, step.kind, payload, None)?;
                    }
                    Err(err) => {
                        record_ui_script_expect_step(
                            state,
                            step.index,
                            step.kind,
                            payload,
                            Some(err.as_str()),
                        )?;
                        state.failed_step_index = Some(step.index);
                        return Err(err);
                    }
                }
            }
            ui_script::UiScriptDryRunEffect::Exit => {
                flush_ui_script_pending_lines(invocation, &mut pending_lines, state)?;
                record_ui_script_exit_step(state, step.index, step.kind)?;
                return Ok(());
            }
        }
    }
    flush_ui_script_pending_lines(invocation, &mut pending_lines, state)
}

fn flush_ui_script_pending_lines(
    invocation: &ControlInvocation,
    pending_lines: &mut Vec<PendingUiScriptControlLine>,
    state: &mut UiScriptRunState,
) -> Result<(), String> {
    if pending_lines.is_empty() {
        return Ok(());
    }
    let lines = pending_lines
        .iter()
        .map(|pending| pending.line.clone())
        .collect::<Vec<_>>();
    let exchanges = send_control_lines_for_invocation(invocation, &lines, &state.artifacts_dir)?;
    for (pending, exchange) in pending_lines.iter().zip(exchanges.iter()) {
        apply_control_line_exchange_to_state(state, exchange);
        record_ui_script_control_step(state, pending, exchange)?;
    }
    pending_lines.clear();
    Ok(())
}

fn execute_ui_script_local_effect(label: &str) -> Result<(), String> {
    if let Some(ms) = label.strip_prefix("sleep_ms:") {
        let ms = ms
            .parse::<u64>()
            .map_err(|err| format!("UI script SleepMs 编译结果非法: {ms}, error={err}"))?;
        std::thread::sleep(std::time::Duration::from_millis(ms));
        return Ok(());
    }
    if label.starts_with("expect:") {
        return Err("UI script real runner 暂不支持 Expect 验证;请先用显式 control step 验证,或使用 --dry-run 检查编译结果".to_string());
    }
    if label == "barrier:observe" {
        return Err(
            "UI script real runner 暂不支持 Barrier observe;请先显式插入 Observe step".to_string(),
        );
    }
    Ok(())
}

fn record_ui_script_local_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
    effect: &str,
    label: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let status = if error.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error.is_none() {
        state.completed_step_count += 1;
    }
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": effect,
        "label": label,
        "status": status,
        "error": error,
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_expect_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
    error: Option<&str>,
) -> Result<(), String> {
    let status = if error.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error.is_none() {
        state.completed_step_count += 1;
    }
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": "expect",
        "expect": payload,
        "status": status,
        "error": error,
        "last_response": state.last_response_line,
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_exit_step(
    state: &mut UiScriptRunState,
    step_index: usize,
    step_kind: &str,
) -> Result<(), String> {
    state.completed_step_count += 1;
    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": step_index,
        "step_kind": step_kind,
        "effect": "exit",
        "status": "complete",
        "finished_at_unix_ms": unix_time_ms(),
    }))
}

fn record_ui_script_control_step(
    state: &mut UiScriptRunState,
    pending: &PendingUiScriptControlLine,
    exchange: &ControlLineExchange,
) -> Result<(), String> {
    let error_message = if last_response_is_error(state) {
        Some(ui_script_control_response_error_message(state, exchange))
    } else {
        None
    };
    let status = if error_message.is_some() {
        "failed"
    } else {
        "complete"
    };
    if error_message.is_none() {
        state.completed_step_count += 1;
    } else {
        state.failed_step_index = Some(pending.step_index);
    }

    state.record_step(serde_json::json!({
        "schema": "rdog.ui-script.trace-step.v1",
        "run_id": state.run_id,
        "step_index": pending.step_index,
        "step_kind": pending.step_kind,
        "effect": "control",
        "control_lines": [exchange.line],
        "status": status,
        "error": error_message.as_deref(),
        "response": exchange.response_line,
        "frames": summarize_control_frames(&exchange.frames),
        "artifacts": summarize_artifacts(&exchange.artifacts),
        "finished_at_unix_ms": unix_time_ms(),
    }))?;

    match error_message {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

fn apply_control_line_exchange_to_state(
    state: &mut UiScriptRunState,
    exchange: &ControlLineExchange,
) {
    state.last_response_line = exchange.response_line.clone();
    state.last_response_value = exchange
        .response_line
        .as_deref()
        .and_then(parse_response_payload_value);
    state.last_artifacts = exchange.artifacts.clone();
}

fn evaluate_ui_script_expect(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let kind = payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Expect 缺少 kind 字段".to_string())?;
    match kind {
        "response_status" => expect_response_status(payload, state),
        "response_contains" => expect_response_contains(payload, state),
        "control_status" => expect_control_status(payload, state),
        "window_rect" => expect_window_rect(payload, state),
        "screenshot_exists" => expect_screenshot_exists(payload, state),
        other => Err(format!(
            "UI script real runner 暂不支持 Expect kind: {other}"
        )),
    }
}

fn expect_response_status(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let expected = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ok");
    let is_error = last_response_is_error(state);
    match expected {
        "ok" if !is_error => Ok(()),
        "error" if is_error => Ok(()),
        "ok" | "error" => Err(format!(
            "Expect response_status={expected} 不满足, last_response={:?}",
            state.last_response_line
        )),
        other => {
            let actual = find_json_string_field(state.last_response_value.as_ref(), "status");
            if actual.as_deref() == Some(other) {
                Ok(())
            } else {
                Err(format!(
                    "Expect response_status={other} 不满足, actual={actual:?}"
                ))
            }
        }
    }
}

fn expect_response_contains(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let needle = payload
        .get("contains")
        .or_else(|| payload.get("text"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Expect response_contains 需要 contains 或 text 字段".to_string())?;
    let response = state
        .last_response_line
        .as_deref()
        .ok_or_else(|| "Expect response_contains 没有上一条 @response".to_string())?;
    if response.contains(needle) {
        Ok(())
    } else {
        Err(format!("Expect response_contains 未命中: needle={needle}"))
    }
}

fn expect_control_status(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    if let Some(expected_ok) = payload.get("ok").and_then(serde_json::Value::as_bool) {
        let actual_ok = !last_response_is_error(state);
        return if actual_ok == expected_ok {
            Ok(())
        } else {
            Err(format!(
                "Expect control_status ok={expected_ok} 不满足, actual={actual_ok}"
            ))
        };
    }

    let expected_code = payload
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let actual_code = response_error_code(state.last_response_value.as_ref()).unwrap_or(0);
    if actual_code == expected_code {
        Ok(())
    } else {
        Err(format!(
            "Expect control_status code={expected_code} 不满足, actual={actual_code}"
        ))
    }
}

fn expect_window_rect(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let rect = find_rect_value(state.last_response_value.as_ref())
        .ok_or_else(|| "Expect window_rect 没有在上一条响应里找到 rect/after_rect".to_string())?;
    let tolerance = payload
        .get("tolerance_px")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);

    for field in ["x", "y", "width", "height"] {
        let Some(expected) = payload.get(field).and_then(serde_json::Value::as_i64) else {
            continue;
        };
        let actual = rect
            .get(field)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("Expect window_rect 响应 rect 缺少 {field}"))?;
        if (actual - expected).abs() > tolerance {
            return Err(format!(
                "Expect window_rect {field}={expected} 不满足, actual={actual}, tolerance={tolerance}"
            ));
        }
    }
    Ok(())
}

fn expect_screenshot_exists(
    payload: &serde_json::Map<String, serde_json::Value>,
    state: &UiScriptRunState,
) -> Result<(), String> {
    let label = payload.get("label").and_then(serde_json::Value::as_str);
    let matched = state.last_artifacts.iter().any(|artifact| {
        artifact.path.exists()
            && label
                .map(|label| artifact.filename.contains(label))
                .unwrap_or(true)
    });
    if matched {
        Ok(())
    } else {
        Err(format!(
            "Expect screenshot_exists 不满足, label={label:?}, artifacts={}",
            state.last_artifacts.len()
        ))
    }
}

fn parse_response_payload_value(line: &str) -> Option<serde_json::Value> {
    let payload = line.trim_start().strip_prefix("@response ")?;
    serde_json::from_str(payload.trim()).ok()
}

fn last_response_is_error(state: &UiScriptRunState) -> bool {
    response_error_code(state.last_response_value.as_ref())
        .map(|code| code != 0)
        .unwrap_or(false)
        || matches!(
            response_status_value(state.last_response_value.as_ref()),
            Some("error" | "failed")
        )
}

fn response_error_code(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(code) = value.get("code").and_then(serde_json::Value::as_i64) {
        return Some(code);
    }
    value
        .get("value")
        .and_then(|inner| inner.get("code"))
        .and_then(serde_json::Value::as_i64)
}

fn response_status_value(value: Option<&serde_json::Value>) -> Option<&str> {
    let value = value?;
    value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("value")
                .and_then(|inner| inner.get("status"))
                .and_then(serde_json::Value::as_str)
        })
}

fn ui_script_control_response_error_message(
    state: &UiScriptRunState,
    exchange: &ControlLineExchange,
) -> String {
    let detail = find_json_string_field(state.last_response_value.as_ref(), "error")
        .or_else(|| response_status_value(state.last_response_value.as_ref()).map(str::to_owned))
        .or_else(|| state.last_response_line.clone())
        .unwrap_or_else(|| "unknown control error".to_owned());
    format!(
        "UI script control step `{}` failed: {detail}",
        exchange.line
    )
}

fn find_json_string_field(value: Option<&serde_json::Value>, field: &str) -> Option<String> {
    let value = value?;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(field).and_then(serde_json::Value::as_str) {
                return Some(found.to_owned());
            }
            map.values()
                .find_map(|child| find_json_string_field(Some(child), field))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|child| find_json_string_field(Some(child), field)),
        _ => None,
    }
}

fn find_rect_value(
    value: Option<&serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let value = value?;
    match value {
        serde_json::Value::Object(map) => {
            if let Some(rect) = map
                .get("after_rect")
                .or_else(|| map.get("rect"))
                .and_then(serde_json::Value::as_object)
            {
                return Some(rect);
            }
            map.values().find_map(|child| find_rect_value(Some(child)))
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|child| find_rect_value(Some(child)))
        }
        _ => None,
    }
}

fn collect_control_lines_from_transport(
    transport: &mut control_transport::ControlTransport,
    lines: &[String],
    artifacts_dir: &Path,
) -> std::io::Result<Vec<ControlLineExchange>> {
    let mut exchanges = Vec::with_capacity(lines.len());
    for line in lines {
        transport.write_message(line)?;
        let mut frames = Vec::new();
        loop {
            let Some(message) = transport.read_message()? else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "control connection 在收到 UI script 结果前就关闭了",
                ));
            };
            let frame = control_frames::ControlFrame::parse_inbound_result_message(&message)?;
            let is_response = matches!(frame, control_frames::ControlFrame::ResponseLine(_));
            frames.push(frame);
            if is_response {
                break;
            }
        }
        let exchange = collect_control_exchange_from_frames(line, frames, artifacts_dir)?;
        print_control_line_exchange(&exchange)?;
        exchanges.push(exchange);
    }
    Ok(exchanges)
}

fn collect_control_exchanges_from_frames(
    lines: &[String],
    line_frames: Vec<Vec<control_frames::ControlFrame>>,
    artifacts_dir: &Path,
) -> std::io::Result<Vec<ControlLineExchange>> {
    if lines.len() != line_frames.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "UI script control line 数量和返回 frame 组数量不一致: lines={}, frames={}",
                lines.len(),
                line_frames.len()
            ),
        ));
    }

    let mut exchanges = Vec::with_capacity(lines.len());
    for (line, frames) in lines.iter().zip(line_frames.into_iter()) {
        let exchange = collect_control_exchange_from_frames(line, frames, artifacts_dir)?;
        print_control_line_exchange(&exchange)?;
        exchanges.push(exchange);
    }
    Ok(exchanges)
}

fn collect_control_exchange_from_frames(
    line: &str,
    frames: Vec<control_frames::ControlFrame>,
    artifacts_dir: &Path,
) -> std::io::Result<ControlLineExchange> {
    let mut response_line = None::<String>;
    let mut artifacts = Vec::new();

    for frame in &frames {
        match frame {
            control_frames::ControlFrame::ResponseLine(line) => {
                response_line = Some(line.clone());
            }
            control_frames::ControlFrame::SaveFile(savefile) => {
                let path = savefile.save_to_directory(artifacts_dir)?;
                artifacts.push(UiScriptArtifactRecord {
                    filename: savefile.filename.clone(),
                    mime: savefile.mime.clone(),
                    path,
                    width: savefile.width,
                    height: savefile.height,
                });
            }
            control_frames::ControlFrame::PtyReady(_)
            | control_frames::ControlFrame::PtyOutput(_)
            | control_frames::ControlFrame::PtyExit(_)
            | control_frames::ControlFrame::PtyClosed(_)
            | control_frames::ControlFrame::PtyDetached(_)
            | control_frames::ControlFrame::PtyAttached(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "UI script line-control response 收到了意外 PTY frame",
                ));
            }
        }
    }

    if response_line.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("UI script control line 没有收到 @response: {line}"),
        ));
    }

    Ok(ControlLineExchange {
        line: line.to_owned(),
        frames,
        response_line,
        artifacts,
    })
}

fn print_control_line_exchange(exchange: &ControlLineExchange) -> std::io::Result<()> {
    for artifact in &exchange.artifacts {
        println!("saved file: {}", artifact.path.display());
    }
    if let Some(response) = &exchange.response_line {
        println!("{response}");
    }
    Ok(())
}

fn summarize_control_frames(frames: &[control_frames::ControlFrame]) -> Vec<serde_json::Value> {
    frames
        .iter()
        .map(|frame| match frame {
            control_frames::ControlFrame::ResponseLine(line) => serde_json::json!({
                "kind": "response",
                "line": line,
            }),
            control_frames::ControlFrame::SaveFile(frame) => serde_json::json!({
                "kind": "savefile",
                "filename": frame.filename,
                "mime": frame.mime,
                "width": frame.width,
                "height": frame.height,
            }),
            control_frames::ControlFrame::PtyReady(_) => serde_json::json!({"kind": "pty-ready"}),
            control_frames::ControlFrame::PtyOutput(_) => serde_json::json!({"kind": "pty-output"}),
            control_frames::ControlFrame::PtyExit(_) => serde_json::json!({"kind": "pty-exit"}),
            control_frames::ControlFrame::PtyClosed(_) => serde_json::json!({"kind": "pty-closed"}),
            control_frames::ControlFrame::PtyDetached(_) => {
                serde_json::json!({"kind": "pty-detached"})
            }
            control_frames::ControlFrame::PtyAttached(_) => {
                serde_json::json!({"kind": "pty-attached"})
            }
        })
        .collect()
}

fn summarize_artifacts(artifacts: &[UiScriptArtifactRecord]) -> Vec<serde_json::Value> {
    artifacts
        .iter()
        .map(|artifact| {
            serde_json::json!({
                "filename": artifact.filename,
                "mime": artifact.mime,
                "path": artifact.path,
                "width": artifact.width,
                "height": artifact.height,
            })
        })
        .collect()
}

fn write_ui_script_normalized_plan(
    run_dir: &Path,
    script_path: &Path,
    dry_run: &ui_script::UiScriptDryRun,
    run_id: &str,
) -> Result<(), String> {
    let steps = dry_run
        .steps
        .iter()
        .map(|step| {
            serde_json::json!({
                "index": step.index,
                "kind": step.kind,
                "effect": ui_script_effect_summary(&step.effect),
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "schema": "rdog.ui-script.normalized.v1",
        "run_id": run_id,
        "source_path": script_path,
        "steps": steps,
        "control_lines": dry_run.control_lines,
    });
    write_json_file(&run_dir.join("script.normalized.json"), &value)
}

fn write_ui_script_summary(
    run_dir: &Path,
    dry_run: &ui_script::UiScriptDryRun,
    run_id: &str,
    status: &str,
    completed_step_count: usize,
    failed_step_index: Option<usize>,
    error: Option<&str>,
) -> Result<(), String> {
    let value = serde_json::json!({
        "schema": "rdog.ui-script.run.v1",
        "run_id": run_id,
        "status": status,
        "step_count": dry_run.summary.step_count,
        "completed_step_count": completed_step_count,
        "failed_step_index": failed_step_index,
        "backend_request_count": dry_run.summary.backend_request_count,
        "semantic_action_count": dry_run.summary.semantic_action_count,
        "mouse_fallback_count": dry_run.summary.mouse_fallback_count,
        "verification_passed": status == "complete",
        "error": error,
        "updated_at_unix_ms": unix_time_ms(),
    });
    write_json_file(&run_dir.join("summary.json"), &value)
}

fn ui_script_effect_summary(effect: &ui_script::UiScriptDryRunEffect) -> serde_json::Value {
    match effect {
        ui_script::UiScriptDryRunEffect::Context(label) => {
            serde_json::json!({"kind": "context", "label": label})
        }
        ui_script::UiScriptDryRunEffect::Local(label) => {
            serde_json::json!({"kind": "local", "label": label})
        }
        ui_script::UiScriptDryRunEffect::Expect(payload) => {
            serde_json::json!({"kind": "expect", "payload": payload})
        }
        ui_script::UiScriptDryRunEffect::ControlLine(line) => {
            serde_json::json!({"kind": "control", "line": line})
        }
        ui_script::UiScriptDryRunEffect::Exit => serde_json::json!({"kind": "exit"}),
    }
}

fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|err| format!("序列化 JSON 文件失败: {}: {err}", path.display()))?;
    fs::write(path, format!("{content}\n"))
        .map_err(|err| format!("写入 JSON 文件失败: {}: {err}", path.display()))
}

fn build_ui_script_run_id() -> String {
    format!("uiscript-{}", unix_time_ms())
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn send_control_lines_for_invocation(
    invocation: &ControlInvocation,
    lines: &[String],
    artifacts_dir: &Path,
) -> Result<Vec<ControlLineExchange>, String> {
    match invocation {
        ControlInvocation::Tcp { host, port } => {
            send_control_lines_tcp(host, parse_port(port)?, lines, artifacts_dir)
        }
        ControlInvocation::WebSocket { url } => {
            send_control_lines_websocket(url, lines, artifacts_dir)
        }
        ControlInvocation::Zenoh {
            namespace,
            target_name,
            entry_point,
        } => send_control_lines_zenoh(
            namespace.clone(),
            target_name.clone(),
            entry_point.clone(),
            lines,
            artifacts_dir,
        ),
        ControlInvocation::ZenohLocal { namespace } => {
            let target_name = zenoh_runtime::find_local_daemon_name(namespace.as_deref())
                .map_err(|err| err.to_string())?;
            let resolved_namespace = namespace
                .clone()
                .or_else(|| crate::zenoh_identity::infer_namespace_from_daemon_name(&target_name));
            let Some(resolved_namespace) = resolved_namespace else {
                return Err(format!(
                    "`rdog control self` 找不到 namespace;请传 `--namespace`(例如 `--namespace lab`)。daemon_name={target_name:?} 没有可推断的 namespace 后缀"
                ));
            };
            send_control_lines_zenoh(
                Some(resolved_namespace),
                Some(target_name),
                Vec::new(),
                lines,
                artifacts_dir,
            )
        }
    }
}

/// TCP 多 line one-shot 入口:一次性发一组 `@<line>`,共享同一条 TCP 连接。
///
/// 与 `send_single_control_line_tcp` 的区别:
/// - 走完整 frame 收口循环,能正确处理 `@screenshot` 这种 `@savefile` 多 frame 场景
/// - 一次 connect,不再每条重连
/// - 任一行失败整组退出
fn send_control_lines_tcp(
    host: &str,
    port: u16,
    lines: &[String],
    artifacts_dir: &Path,
) -> Result<Vec<ControlLineExchange>, String> {
    let mut transport = control_transport::ControlTransport::connect_tcp(host, port)
        .map_err(|err| err.to_string())?;
    collect_control_lines_from_transport(&mut transport, lines, artifacts_dir)
        .map_err(|err| err.to_string())
}

/// WebSocket 多 line one-shot 入口,语义同 `send_control_lines_tcp`。
fn send_control_lines_websocket(
    url: &str,
    lines: &[String],
    artifacts_dir: &Path,
) -> Result<Vec<ControlLineExchange>, String> {
    let mut transport = control_transport::ControlTransport::connect_websocket(url)
        .map_err(|err| err.to_string())?;
    collect_control_lines_from_transport(&mut transport, lines, artifacts_dir)
        .map_err(|err| err.to_string())
}

/// Zenoh 多 line one-shot 入口:复用一条 session bridge 串行执行一组 `@<line>`。
///
/// 任一行失败整组退出,不做行级重试(避免半成功半失败状态)。
fn send_control_lines_zenoh(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    lines: &[String],
    artifacts_dir: &Path,
) -> Result<Vec<ControlLineExchange>, String> {
    let line_frames = zenoh_control::send_control_lines_collect_frames(
        namespace,
        target_name,
        entry_point,
        3_000,
        lines,
    )
    .map_err(|err| err.to_string())?;
    collect_control_exchanges_from_frames(lines, line_frames, artifacts_dir)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_control_line_exchange_to_state, apply_ui_script_target, evaluate_ui_script_expect,
        extract_one_shot_lines, parse_port, parse_response_payload_value,
        record_ui_script_control_step, resolve_control_invocation, resolve_daemon_transport,
        split_ui_script_run_positionals, ControlInvocation, ControlLineExchange,
        PendingUiScriptControlLine, UiScriptArtifactRecord, UiScriptRunState,
    };
    use crate::{
        config::{DaemonConfig, ZenohConfig},
        control_frames::ControlFrame,
        input::{Command, Opts as InputOpts, Transport, UiScriptCommand},
    };
    use clap::Parser as _;
    use std::{
        fs::{self, OpenOptions},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_ui_script_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_millis();
        std::env::temp_dir().join(format!(
            "rdog-ui-script-{name}-{millis}-{}",
            std::process::id()
        ))
    }

    fn fake_ui_script_state(name: &str) -> UiScriptRunState {
        let run_dir = temp_ui_script_dir(name);
        let artifacts_dir = run_dir.join("artifacts");
        fs::create_dir_all(&artifacts_dir).expect("test artifacts dir should create");
        let trace_path = run_dir.join("trace.jsonl");
        let trace_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&trace_path)
            .expect("test trace file should create");

        UiScriptRunState {
            run_id: format!("test-{name}"),
            run_dir,
            artifacts_dir,
            trace_path,
            trace_file,
            completed_step_count: 0,
            failed_step_index: None,
            last_response_line: None,
            last_response_value: None,
            last_artifacts: Vec::new(),
        }
    }

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

    #[test]
    fn ui_script_run_positionals_should_use_single_arg_as_script_path() {
        let (target, script_path) = split_ui_script_run_positionals(vec![
            "tests/fixtures/ui_script/ping_control_line.json".to_string(),
        ])
        .unwrap();

        assert!(target.is_empty());
        assert_eq!(
            script_path,
            PathBuf::from("tests/fixtures/ui_script/ping_control_line.json")
        );
    }

    #[test]
    fn ui_script_run_positionals_should_keep_target_before_script_path() {
        let (target, script_path) = split_ui_script_run_positionals(vec![
            "mac.lab".to_string(),
            "tests/fixtures/ui_script/ping_control_line.json".to_string(),
        ])
        .unwrap();

        assert_eq!(target, vec!["mac.lab".to_string()]);
        assert_eq!(
            script_path,
            PathBuf::from("tests/fixtures/ui_script/ping_control_line.json")
        );
    }

    #[test]
    fn ui_script_run_positionals_should_reject_control_line_as_target() {
        let err = split_ui_script_run_positionals(vec![
            "@ping".to_string(),
            "tests/fixtures/ui_script/ping_control_line.json".to_string(),
        ])
        .unwrap_err();

        assert!(err.contains("target 位置参数不能是 `@<line>`"));
    }

    #[test]
    fn ui_script_run_cli_should_parse_target_and_script_path() {
        let opts = InputOpts::try_parse_from([
            "rdog",
            "ui-script",
            "run",
            "self",
            "tests/fixtures/ui_script/ping_control_line.json",
        ])
        .unwrap();

        let Command::UiScript {
            command:
                UiScriptCommand::Run {
                    dry_run,
                    positional,
                    trace_dir,
                    ..
                },
        } = opts.command
        else {
            panic!("expected ui-script run command");
        };
        assert!(!dry_run);
        assert!(trace_dir.is_none());
        assert_eq!(
            positional,
            vec![
                "self".to_string(),
                "tests/fixtures/ui_script/ping_control_line.json".to_string()
            ]
        );
    }

    #[test]
    fn ui_script_run_cli_should_parse_trace_dir() {
        let opts = InputOpts::try_parse_from([
            "rdog",
            "ui-script",
            "run",
            "--trace-dir",
            "rdog_script_runs/demo",
            "tests/fixtures/ui_script/ping_control_line.json",
        ])
        .unwrap();

        let Command::UiScript {
            command:
                UiScriptCommand::Run {
                    trace_dir,
                    positional,
                    ..
                },
        } = opts.command
        else {
            panic!("expected ui-script run command");
        };
        assert_eq!(trace_dir, Some(PathBuf::from("rdog_script_runs/demo")));
        assert_eq!(
            positional,
            vec!["tests/fixtures/ui_script/ping_control_line.json".to_string()]
        );
    }

    #[test]
    fn ui_script_expect_should_validate_response_and_window_rect() {
        let mut state = fake_ui_script_state("expect-response");
        state.last_response_line =
            Some(r#"@response {"value":{"status":"ok","after_rect":{"x":10,"y":20,"width":300,"height":200}}}"#.to_string());
        state.last_response_value = state
            .last_response_line
            .as_deref()
            .and_then(parse_response_payload_value);

        let response_contains = serde_json::json!({
            "kind": "response_contains",
            "contains": "after_rect"
        });
        evaluate_ui_script_expect(response_contains.as_object().unwrap(), &state).unwrap();

        let response_status = serde_json::json!({
            "kind": "response_status",
            "status": "ok"
        });
        evaluate_ui_script_expect(response_status.as_object().unwrap(), &state).unwrap();

        let window_rect = serde_json::json!({
            "kind": "window_rect",
            "x": 10,
            "y": 21,
            "width": 300,
            "height": 200,
            "tolerance_px": 1
        });
        evaluate_ui_script_expect(window_rect.as_object().unwrap(), &state).unwrap();
    }

    #[test]
    fn ui_script_expect_should_validate_control_status_and_screenshot_artifact() {
        let mut state = fake_ui_script_state("expect-artifact");
        state.last_response_line =
            Some(r#"@response {"code":64,"error":"bad request"}"#.to_string());
        state.last_response_value = state
            .last_response_line
            .as_deref()
            .and_then(parse_response_payload_value);
        let control_status = serde_json::json!({
            "kind": "control_status",
            "code": 64
        });
        evaluate_ui_script_expect(control_status.as_object().unwrap(), &state).unwrap();

        let artifact_path = state.artifacts_dir.join("before.jpg");
        fs::write(&artifact_path, b"fake image").expect("fake artifact should write");
        state.last_artifacts.push(UiScriptArtifactRecord {
            filename: "before.jpg".to_string(),
            mime: "image/jpeg".to_string(),
            path: artifact_path,
            width: Some(1),
            height: Some(1),
        });
        let screenshot_exists = serde_json::json!({
            "kind": "screenshot_exists",
            "label": "before"
        });
        evaluate_ui_script_expect(screenshot_exists.as_object().unwrap(), &state).unwrap();
    }

    #[test]
    fn ui_script_trace_should_write_control_step_record() {
        let mut state = fake_ui_script_state("trace-control");
        let pending = PendingUiScriptControlLine {
            step_index: 0,
            step_kind: "ControlLine",
            line: "@ping".to_string(),
        };
        let exchange = ControlLineExchange {
            line: "@ping".to_string(),
            frames: vec![ControlFrame::ResponseLine(
                r#"@response "pong""#.to_string(),
            )],
            response_line: Some(r#"@response "pong""#.to_string()),
            artifacts: Vec::new(),
        };

        record_ui_script_control_step(&mut state, &pending, &exchange).unwrap();
        let trace = fs::read_to_string(&state.trace_path).unwrap();
        assert!(trace.contains(r#""step_kind":"ControlLine""#));
        assert!(trace.contains(r#"@response \"pong\""#));
    }

    #[test]
    fn ui_script_control_step_should_fail_on_error_response() {
        let mut state = fake_ui_script_state("trace-control-error");
        let pending = PendingUiScriptControlLine {
            step_index: 7,
            step_kind: "ControlLine",
            line: "@window-resize#1:{}".to_string(),
        };
        let exchange = ControlLineExchange {
            line: pending.line.clone(),
            frames: vec![ControlFrame::ResponseLine(
                r#"@response {"code":64,"error":"permission denied"}"#.to_string(),
            )],
            response_line: Some(r#"@response {"code":64,"error":"permission denied"}"#.to_string()),
            artifacts: Vec::new(),
        };

        apply_control_line_exchange_to_state(&mut state, &exchange);
        let err = record_ui_script_control_step(&mut state, &pending, &exchange)
            .expect_err("error response should fail the script step");
        let trace = fs::read_to_string(&state.trace_path).unwrap();

        assert_eq!(state.completed_step_count, 0);
        assert_eq!(state.failed_step_index, Some(7));
        assert!(err.contains("permission denied"));
        assert!(trace.contains(r#""status":"failed""#));
    }

    #[test]
    fn ui_script_target_should_fill_empty_cli_target_and_namespace() {
        let program = crate::ui_script::parse_script_json(
            r#"[{"Target":{"name":"self","namespace":"lab"}},{"ControlLine":"@ping"}]"#,
        )
        .unwrap();
        let mut control_positionals = Vec::new();
        let mut namespace = None;
        let mut target_name = None;

        apply_ui_script_target(
            &program,
            &mut control_positionals,
            &mut namespace,
            &mut target_name,
        )
        .unwrap();

        assert_eq!(control_positionals, vec!["self".to_string()]);
        assert_eq!(namespace.as_deref(), Some("lab"));
        assert!(target_name.is_none());
    }

    #[test]
    fn ui_script_target_should_reject_cli_target_mismatch() {
        let program = crate::ui_script::parse_script_json(
            r#"[{"Target":{"name":"self"}},{"ControlLine":"@ping"}]"#,
        )
        .unwrap();
        let mut control_positionals = vec!["mac.lab".to_string()];
        let mut namespace = None;
        let mut target_name = None;
        let err = apply_ui_script_target(
            &program,
            &mut control_positionals,
            &mut namespace,
            &mut target_name,
        )
        .unwrap_err();

        assert!(err.contains("不一致"));
    }
}
