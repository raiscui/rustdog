use std::path::Path;

use crate::input::Transport;
use crate::{control_frames, control_transport, zenoh_control, zenoh_runtime};

pub(crate) const LEGACY_ZENOH_PEER_TRANSPORT_ERROR: &str =
    "旧 transport `zenoh-peer` 已废弃; 请改用 `--transport zenoh`。`rdog control` 默认会自动发现 router，必要时再补 `--entry-point tcp/<router-host>:<port>`";

/// 解析 `HOST [PORT]` 位置参数,保持**监听语义**:只给端口时 host 留 `0.0.0.0`,
/// 因为 `rdog listen` 用它决定 bind 地址。
///
/// 需要 connect 语义(未指定地址 -> 回环)的调用方走 `connect_hosts_from_opts`。
pub(crate) fn host_from_opts(host: Vec<String>) -> Result<(String, String), String> {
    let fixed_host = if host.len() == 1 {
        (BIND_WILDCARD_HOST.to_string(), host.get(0).unwrap().to_string()) // Safe to unwrap here
    } else if let [host, port] = &host[..] {
        (host.to_string(), port.to_string())
    } else {
        return Err("Missing host".to_string());
    };

    Ok(fixed_host)
}

/// 同 `host_from_opts`,但按 **connect 语义**把未指定地址翻成回环。
pub(crate) fn connect_hosts_from_opts(host: Vec<String>) -> Result<(String, String), String> {
    let (host, port) = host_from_opts(host)?;
    Ok((connect_host(&host).to_string(), port))
}

/// 只写端口时的默认 connect 目标。
pub(crate) const LOOPBACK_HOST: &str = "127.0.0.1";

/// 只写端口时 `rdog listen` 的默认 bind 地址。
pub(crate) const BIND_WILDCARD_HOST: &str = "0.0.0.0";

/// 把"绑定用的未指定地址"翻译成"本机能连的回环地址"。
///
/// `0.0.0.0` / `::` 是 listen 地址: 拿它当 connect 目标在 Linux 上碰巧落到本机,
/// 在 Windows 上直接 `os error 10049`(WSAEADDRNOTAVAIL)。用户真正想说的是
/// "这台机器上的 daemon",所以这里统一映射回环。
pub(crate) fn connect_host(host: &str) -> &str {
    match host {
        "0.0.0.0" => LOOPBACK_HOST,
        "::" | "[::]" | "::0" | "[::0]" => "::1",
        other => other,
    }
}

/// 该地址是否指向本机(回环或未指定地址)。
fn is_local_host(host: &str) -> bool {
    let host = connect_host(host);
    host == "::1" || host.eq_ignore_ascii_case("localhost") || host.starts_with("127.")
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
pub(crate) fn extract_one_shot_lines(host: Vec<String>) -> (Vec<String>, Vec<String>) {
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ControlInvocation {
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

#[derive(Debug, Clone)]
pub(crate) struct ControlArtifactRecord {
    pub(crate) filename: String,
    pub(crate) mime: String,
    pub(crate) path: std::path::PathBuf,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ControlLineExchange {
    pub(crate) line: String,
    pub(crate) frames: Vec<control_frames::ControlFrame>,
    pub(crate) response_line: Option<String>,
    pub(crate) artifacts: Vec<ControlArtifactRecord>,
    /// 本行途中到达的 task 进度帧 wire 文本 (旁路通知, 非本行响应产物)。
    /// Task/Spawn Phase 2: spawn 后续行会带回 daemon 推送的任务帧,
    /// 多行 one-shot / UI script 把它们作为通知行打印, 不计入响应。
    pub(crate) task_events: Vec<String>,
}

pub(crate) fn resolve_control_invocation(
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
        Some(Transport::Iroh) => Err("iroh control is dispatched before URL resolution".into()),
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

    let (host, port) = connect_hosts_from_opts(positional)?;
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
        [] => Ok(ControlInvocation::ZenohLocal { namespace: None }),
        [single] if single.parse::<u16>().is_ok() => Ok(ControlInvocation::Tcp {
            host: LOOPBACK_HOST.to_string(),
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
            let (host, port) = connect_hosts_from_opts(positional)?;
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

pub(crate) fn parse_port(port: &str) -> Result<u16, String> {
    port.parse::<u16>()
        .map_err(|err| format!("Invalid port `{port}`: {err}"))
}

/// `self` / 空 target 解析出的本机控制落点。
///
/// 本机 daemon 可能是 zenoh router(控制面在 zenoh 上),也可能是 line-control TCP
/// 接收端。客户端不该猜,所以解析结果直接带上"用哪种协议、连哪里"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalControlTarget {
    Zenoh {
        namespace: Option<String>,
        daemon_name: String,
        entry_points: Vec<String>,
    },
    Tcp {
        host: String,
        port: u16,
    },
}

/// 解析本机 fast path 目标。
///
/// 平台分工: unix 继续用 unixpipe local-default registry(行为不变); 其他平台读
/// `local_daemon_registry`(daemon 启动时登记的控制 lane)。两条都不通时,报出当前平台
/// 那条链路的原始错误 —— 升级排障要看得到 candidates / namespace 提示。
pub(crate) fn resolve_local_control_target(
    namespace: Option<String>,
) -> Result<LocalControlTarget, String> {
    let unixpipe_outcome = unixpipe_local_target(namespace.as_deref());
    if let Ok(Some(target)) = &unixpipe_outcome {
        return Ok(target.clone());
    }

    match registry_local_target(namespace.as_deref()) {
        Ok(target) => Ok(target),
        Err(registry_err) => match unixpipe_outcome {
            // unix: 老链路的诊断信息更具体(unmanaged FIFO 候选 / namespace 提示),优先保留。
            Err(unixpipe_err) => Err(unixpipe_err.to_string()),
            Ok(None) => Err(registry_err.to_string()),
            Ok(Some(target)) => Ok(target),
        },
    }
}

/// unix 的 unixpipe lane 解析;非 unix 平台恒为 None(那条链路不存在)。
fn unixpipe_local_target(
    namespace: Option<&str>,
) -> Result<Option<LocalControlTarget>, std::io::Error> {
    if !cfg!(unix) {
        return Ok(None);
    }

    let daemon_name = zenoh_runtime::find_local_daemon_name(namespace)?;
    let resolved_namespace = namespace
        .map(str::to_string)
        .or_else(|| crate::zenoh_identity::infer_namespace_from_daemon_name(&daemon_name));
    Ok(Some(LocalControlTarget::Zenoh {
        namespace: resolved_namespace,
        daemon_name,
        // unix 由 zenoh 层的 unixpipe 探测决定 endpoint,这里不预置。
        entry_points: Vec::new(),
    }))
}

/// 读本机 daemon 登记表,把登记的控制 lane 翻成客户端可直接执行的落点。
fn registry_local_target(
    namespace: Option<&str>,
) -> Result<LocalControlTarget, std::io::Error> {
    let record = crate::local_daemon_registry::find_local_daemon(namespace)?;
    Ok(record_into_target(record))
}

fn record_into_target(
    record: crate::local_daemon_registry::LocalDaemonRecord,
) -> LocalControlTarget {
    match record.lane {
        crate::local_daemon_registry::LocalControlLane::Zenoh {
            connect_endpoints,
        } => LocalControlTarget::Zenoh {
            namespace: Some(record.namespace),
            daemon_name: record.daemon_name,
            entry_points: connect_endpoints,
        },
        crate::local_daemon_registry::LocalControlLane::LineTcp { host, port } => {
            LocalControlTarget::Tcp { host, port }
        }
    }
}

/// 把"回环 host + 端口"的 TCP 目标升级成本机登记表里那条真实控制 lane。
///
/// 用户看到的是一个端口(`rdog control 17480`),但 zenoh 模式的 daemon 在那端口上
/// 只说 zenoh 协议,原始 line-control 打过去只会被断开。这里按端口认领: 命中已登记
/// daemon 就改用它的协议,没命中就保持 line-control TCP 老行为。
///
/// 只对推断出来的目标生效: 显式 `--transport tcp` 是用户的协议选择,不在这里改写。
pub(crate) fn promote_loopback_tcp_with<F>(invocation: ControlInvocation, lookup: F) -> ControlInvocation
where
    F: Fn(u16) -> Option<crate::local_daemon_registry::LocalDaemonRecord>,
{
    let ControlInvocation::Tcp { host, port } = &invocation else {
        return invocation;
    };
    if !is_local_host(host) {
        return invocation;
    }
    let Ok(port) = parse_port(port) else {
        return invocation;
    };
    match lookup(port) {
        Some(record) => match record_into_target(record) {
            LocalControlTarget::Zenoh {
                namespace,
                daemon_name,
                entry_points,
            } => {
                log::debug!(
                    "control link: local-daemon-registry -> zenoh (port={port}, namespace={namespace:?}, daemon={daemon_name}, connect_endpoints={entry_points:?})"
                );
                ControlInvocation::Zenoh {
                    namespace,
                    target_name: Some(daemon_name),
                    entry_point: entry_points,
                }
            }
            LocalControlTarget::Tcp { host, port } => {
                log::debug!("control link: local-daemon-registry -> line-control tcp {host}:{port}");
                ControlInvocation::Tcp {
                    host,
                    port: port.to_string(),
                }
            }
        },
        None => invocation,
    }
}

/// 生产路径: 端口归属查不到(或查不动)就保持原目标,不因为登记表读失败而拒绝连接。
pub(crate) fn promote_loopback_tcp(invocation: ControlInvocation) -> ControlInvocation {
    promote_loopback_tcp_with(invocation, |port| {
        match crate::local_daemon_registry::find_local_daemon_by_port(port) {
            Ok(record) => record,
            Err(err) => {
                log::debug!("control link: local-daemon-registry lookup for port {port} failed: {err}");
                None
            }
        }
    })
}

pub(crate) fn send_single_control_line_tcp(
    host: &str,
    port: u16,
    line: &str,
) -> Result<(), String> {
    let mut transport = connect_control_tcp(host, port)?;
    send_single_control_line_transport(&mut transport, line)
}

pub(crate) fn send_single_control_line_websocket(url: &str, line: &str) -> Result<(), String> {
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

pub(crate) fn send_single_control_line_zenoh(
    namespace: Option<String>,
    target_name: Option<String>,
    entry_point: Vec<String>,
    line: &str,
) -> Result<(), String> {
    zenoh_control::send_single_control_line(namespace, target_name, entry_point, 3_000, line)
        .map_err(|err| err.to_string())
}

pub(crate) fn send_control_lines_for_invocation(
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
            match resolve_local_control_target(namespace.clone())? {
                LocalControlTarget::Zenoh {
                    namespace,
                    daemon_name,
                    entry_points,
                } => {
                    log::debug!(
                        "control link: local-daemon -> zenoh (namespace={namespace:?}, daemon={daemon_name}, connect_endpoints={entry_points:?})"
                    );
                    send_control_lines_zenoh(
                        namespace,
                        Some(daemon_name),
                        entry_points,
                        lines,
                        artifacts_dir,
                    )
                }
                LocalControlTarget::Tcp { host, port } => {
                    log::debug!("control link: local-daemon -> line-control tcp {host}:{port}");
                    send_control_lines_tcp(&host, port, lines, artifacts_dir)
                }
            }
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
    log::debug!("control link: line-control tcp {host}:{port}");
    let mut transport = connect_control_tcp(host, port)?;
    collect_control_lines_from_transport(
        &mut transport,
        lines,
        artifacts_dir,
        Some(ControlPeer { host, port }),
    )
    .map_err(|err| err.to_string())
}

/// 建立 control TCP 连接, 并把建连失败翻译成能指导排查的提示。
///
/// 裸的 `Connection refused (os error 61)` 不携带任何可行动信息, 而远程场景下
/// "进程在跑但连不上" 有四种成因完全不同的可能, 只有第 (2) 种需要用户改配置。
fn connect_control_tcp(
    host: &str,
    port: u16,
) -> Result<control_transport::ControlTransport, String> {
    control_transport::ControlTransport::connect_tcp_with_retry(host, port)
        .map_err(|err| describe_tcp_connect_failure(host, port, &err))
}

fn describe_tcp_connect_failure(host: &str, port: u16, err: &std::io::Error) -> String {
    // 2026-10-07: 四行排查文案每次连接失败全量刷屏, agent 高频调用下纯噪音。
    // 默认一行给方向; RDOG_CONNECT_VERBOSE=1 保留完整排查清单 (老用户兜底)。
    if std::env::var("RDOG_CONNECT_VERBOSE")
        .ok()
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
    {
        return format!(
            "无法连接 daemon {host}:{port} ({err})。可能原因: \
             (1) daemon 未启动; \
             (2) daemon 走了 zenoh 路径 —— zenoh.enabled 为 true (仓库自带的 rdog_win.toml \
             的 `[zenoh]` 段就是这个默认值) 时, run_zenoh_router 会显式关掉 inbound/outbound, \
             TCP 端口根本不 bind, 进程在跑但连不上; 控制端显式用 \
             `--transport tcp {host} {port}` 时最容易撞上。解法是被控端把 \
             inbound.enabled 打开, 或把 zenoh.enabled 关掉; \
             (3) Windows 防火墙拦截; \
             (4) daemon 正在 bind 重试的退避窗口内"
        );
    }
    format!(
        "无法连接 daemon {host}:{port}: {err} — 检查被控端 daemon 是否在跑 / 端口与防火墙 \
         (详细排查: RDOG_CONNECT_VERBOSE=1)"
    )
}

/// WebSocket 多 line one-shot 入口,语义同 `send_control_lines_tcp`。
fn send_control_lines_websocket(
    url: &str,
    lines: &[String],
    artifacts_dir: &Path,
) -> Result<Vec<ControlLineExchange>, String> {
    let mut transport = control_transport::ControlTransport::connect_websocket(url)
        .map_err(|err| err.to_string())?;
    collect_control_lines_from_transport(&mut transport, lines, artifacts_dir, None)
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
    let outcome = zenoh_control::send_control_lines_collect_frames(
        namespace,
        target_name,
        entry_point,
        3_000,
        lines,
    );
    // 失败也要把**已收到的那几帧**打出来: 空 stdout 让读的人分不清
    // 「一条都没跑」与「跑了几条才卡住」(RAI-579)。先打印, 再报错。
    let completed = outcome.line_frames.len();
    let exchanges = collect_control_exchanges_from_frames(
        &lines[..completed],
        outcome.line_frames,
        artifacts_dir,
    )
    .map_err(|err| err.to_string())?;
    match outcome.failure {
        None => Ok(exchanges),
        Some(failure) => Err(multi_one_shot_failure_message(&failure, completed)),
    }
}

/// 多行 one-shot 失败的对外文案。
///
/// 必须包含三件读日志的人靠猜拿不到的事实: **第几条失败 / 共几条 / 已收到几帧**。
/// 否则「非零退出 + 空 stdout」既可能是「一条都没发出去」, 也可能是「跑了一半才断」——
/// 两者的处置完全不同 (前者可以重跑, 后者要先确认副作用)。RAI-579。
fn multi_one_shot_failure_message(
    failure: &zenoh_control::ControlLinesFailure,
    completed_lines: usize,
) -> String {
    // 单位是**行**, 不是帧: 一条命令可能回多个帧 (例如 `@screenshot` + `@savefile`),
    // 报「已收到 N 帧」会与「已完成 N 行」混为一谈 (复核指出)。
    let position = if failure.dispatched_any {
        format!("失败于第 {} 条", failure.line_index + 1)
    } else {
        "还没发出第一条就失败".to_owned()
    };
    format!(
        "多行 one-shot {position} (共 {} 条, 已完成 {completed_lines} 行): {}. 已完成的响应已打到 stdout; 未执行的行没有副作用, 但已执行的行可能有 —— 不要盲目重跑整段脚本, 先确认前面的副作用要不要回滚。",
        failure.total_lines, failure.cause
    )
}

/// TCP line-control 对端身份,只用于把"会话被关"翻译成可行动的错误。
#[derive(Debug, Clone, Copy)]
struct ControlPeer<'a> {
    host: &'a str,
    port: u16,
}

impl ControlPeer<'_> {
    /// 一帧都没收到就被关: 这个端口上说的不是 line-control。
    ///
    /// 实测成因 (RAI-385): daemon 以 zenoh 模式启动时, `run_zenoh_router` 会强制关掉
    /// inbound,配置里的 `tcp/...` 端口只跑 zenoh 协议。原始 TCP 握手照样成功,一发
    /// line-control 字节就被 zenoh 丢掉链路,客户端只看到 EOF。
    fn closed_before_any_frame_message(&self) -> String {
        format!(
            "对端 {host}:{port} 接受了 TCP 连接, 但一帧 control 都没回就关闭了会话 —— 该端口不是 \
             line-control 接收端 (daemon 以 zenoh 模式启动时, 配置里的 tcp 端口只跑 zenoh 协议, \
             daemon 横幅会写 `tcp=不启动`)。改用本机 fast path `rdog control self @<line>`, \
             或 zenoh 目标 `rdog control --transport zenoh --entry-point tcp/{host}:{port} <daemon-name> @<line>`, \
             或让 daemon 以 `rdog daemon --transport tcp` 启动",
            host = self.host,
            port = self.port
        )
    }
}

fn collect_control_lines_from_transport(
    transport: &mut control_transport::ControlTransport,
    lines: &[String],
    artifacts_dir: &Path,
    peer: Option<ControlPeer<'_>>,
) -> std::io::Result<Vec<ControlLineExchange>> {
    let mut exchanges = Vec::with_capacity(lines.len());
    for line in lines {
        transport.write_message(line)?;
        let mut frames = Vec::new();
        loop {
            let Some(message) = transport.read_message()? else {
                let message = match (peer, frames.is_empty()) {
                    (Some(peer), true) => peer.closed_before_any_frame_message(),
                    (_, true) => {
                        "control connection 在收到任何 frame 之前就关闭了;对端可能不是 line-control 接收端"
                            .to_string()
                    }
                    (_, false) => format!(
                        "control connection 在收到 UI script 结果前就关闭了 (已收到 {} 个 frame)",
                        frames.len()
                    ),
                };
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    message,
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
    let mut task_events = Vec::new();

    let mut response_line = None::<String>;
    let mut artifacts = Vec::new();

    for frame in &frames {
        match frame {
            control_frames::ControlFrame::ResponseLine(line) => {
                response_line = Some(line.clone());
            }
            control_frames::ControlFrame::SaveFile(savefile) => {
                let path = savefile.save_to_directory(artifacts_dir)?;
                artifacts.push(ControlArtifactRecord {
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
            // Task/Spawn Phase 2: task 进度帧是"旁边的通知", 不是本 line 的
            // 响应产物 — 记录 wire 文本进 task_events, 由打印层输出。
            control_frames::ControlFrame::TaskStarted(frame) => {
                task_events.push(frame.to_wire_message());
                continue;
            }
            control_frames::ControlFrame::TaskProgress(frame) => {
                task_events.push(frame.to_wire_message());
                continue;
            }
            control_frames::ControlFrame::TaskCompleted(frame) => {
                task_events.push(frame.to_wire_message());
                continue;
            }
            control_frames::ControlFrame::TaskFailed(frame) => {
                task_events.push(frame.to_wire_message());
                continue;
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
        task_events,
        frames,
        response_line,
        artifacts,
    })
}

fn print_control_line_exchange(exchange: &ControlLineExchange) -> std::io::Result<()> {
    // task 进度通知先于本行 response 打印 (时序上它们也先于 response 到达)
    for event in &exchange.task_events {
        println!("{event}");
    }
    for artifact in &exchange.artifacts {
        println!("saved file: {}", artifact.path.display());
    }
    if let Some(response) = &exchange.response_line {
        println!("{response}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        describe_tcp_connect_failure, extract_one_shot_lines, multi_one_shot_failure_message,
        parse_port, promote_loopback_tcp_with, resolve_control_invocation, ControlInvocation,
    };
    use crate::input::Transport;

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

        // 只写端口 = "这台机器上的 daemon": connect 目标必须是回环,
        // 不能拿 listen 用的 0.0.0.0 去连 (Windows 上 os error 10049)。
        assert_eq!(
            invocation,
            ControlInvocation::Tcp {
                host: "127.0.0.1".to_string(),
                port: "5555".to_string()
            }
        );
    }

    #[test]
    fn control_invocation_should_map_unspecified_bind_host_to_loopback() {
        let invocation = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["0.0.0.0".to_string(), "17480".to_string()],
        )
        .expect("wildcard host should still resolve as a tcp target");

        assert_eq!(
            invocation,
            ControlInvocation::Tcp {
                host: "127.0.0.1".to_string(),
                port: "17480".to_string()
            }
        );

        let ipv6 = resolve_control_invocation(
            None,
            None,
            None,
            None,
            Vec::new(),
            vec!["::".to_string(), "17480".to_string()],
        )
        .expect("ipv6 wildcard host should still resolve as a tcp target");
        assert_eq!(
            ipv6,
            ControlInvocation::Tcp {
                host: "::1".to_string(),
                port: "17480".to_string()
            }
        );
    }

    #[test]
    fn loopback_port_should_be_promoted_to_the_registered_local_daemon_lane() {
        let invocation = ControlInvocation::Tcp {
            host: "127.0.0.1".to_string(),
            port: "17480".to_string(),
        };

        let promoted = promote_loopback_tcp_with(invocation, |port| {
            (port == 17480).then(|| crate::local_daemon_registry::LocalDaemonRecord {
                schema: crate::local_daemon_registry::LOCAL_DAEMON_SCHEMA.to_string(),
                namespace: "lab".to_string(),
                daemon_name: "win-test.lab".to_string(),
                pid: 7,
                lane: crate::local_daemon_registry::LocalControlLane::Zenoh {
                    connect_endpoints: vec!["tcp/127.0.0.1:17480".to_string()],
                },
                created_at_unix_ms: 0,
            })
        });

        assert_eq!(
            promoted,
            ControlInvocation::Zenoh {
                namespace: Some("lab".to_string()),
                target_name: Some("win-test.lab".to_string()),
                entry_point: vec!["tcp/127.0.0.1:17480".to_string()],
            }
        );
    }

    #[test]
    fn remote_host_or_unregistered_port_should_keep_line_control_tcp() {
        let promoted = promote_loopback_tcp_with(
            ControlInvocation::Tcp {
                host: "192.168.50.57".to_string(),
                port: "17480".to_string(),
            },
            |_| panic!("remote host must not consult the local registry"),
        );
        assert_eq!(
            promoted,
            ControlInvocation::Tcp {
                host: "192.168.50.57".to_string(),
                port: "17480".to_string(),
            }
        );

        let unregistered = promote_loopback_tcp_with(
            ControlInvocation::Tcp {
                host: "127.0.0.1".to_string(),
                port: "5555".to_string(),
            },
            |_| None,
        );
        assert_eq!(
            unregistered,
            ControlInvocation::Tcp {
                host: "127.0.0.1".to_string(),
                port: "5555".to_string(),
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
    fn describe_tcp_connect_failure_should_name_target_and_all_four_causes() {
        // 2026-10-07: 默认精简为一行 + 方向指引; 四成因全量清单走 RDOG_CONNECT_VERBOSE。
        std::env::set_var("RDOG_CONNECT_VERBOSE", "1");
        let verbose = describe_tcp_connect_failure(
            "192.168.50.57",
            5555,
            &std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        );
        assert!(verbose.contains("192.168.50.57:5555"));
        // 进程在跑但连不上的真实成因: zenoh.enabled 时 TCP 端口压根没 bind。
        assert!(verbose.contains("zenoh.enabled"));
        assert!(verbose.contains("inbound"));
        for marker in ["(1)", "(2)", "(3)", "(4)"] {
            assert!(verbose.contains(marker), "缺少成因编号 {marker}");
        }

        std::env::remove_var("RDOG_CONNECT_VERBOSE");
        let brief = describe_tcp_connect_failure(
            "192.168.50.57",
            5555,
            &std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        );
        assert!(brief.contains("192.168.50.57:5555"));
        assert!(brief.contains("daemon 是否在跑"));
        assert!(brief.contains("RDOG_CONNECT_VERBOSE"));
        assert!(!brief.contains("(4)"), "精简态不应刷四成因清单");
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

    /// RAI-579: 失败文案必须自带「第几条 / 共几条 / 已收到几帧」——
    /// 缺了它们, 「非零退出 + 空 stdout」既可能是没发出去, 也可能是跑了一半才断。
    #[test]
    fn multi_one_shot_failure_message_must_name_the_line_and_frame_count() {
        let failure = crate::zenoh_control::ControlLinesFailure {
            line_index: 1,
            total_lines: 3,
            dispatched_any: true,
            cause: std::io::Error::new(std::io::ErrorKind::TimedOut, "请求超时"),
        };
        let message = multi_one_shot_failure_message(&failure, 1);
        assert!(message.contains("第 2 条"), "{message}");
        assert!(message.contains("共 3 条"), "{message}");
        assert!(message.contains("已完成 1 行"), "{message}");
        assert!(
            !message.contains("帧"),
            "单位必须是行: 一条命令可能回多个帧, 报帧数会与行数混淆; {message}"
        );
        assert!(message.contains("请求超时"), "{message}");
        assert!(
            message.contains("不要盲目重跑整段脚本"),
            "必须提醒副作用可能已发生: {message}"
        );
    }

    /// 连不上/解析不到目标时还没发出任何一条 —— 文案不许谎称「已收到 N 帧」。
    #[test]
    fn multi_one_shot_failure_message_must_not_claim_frames_when_nothing_was_sent() {
        let failure = crate::zenoh_control::ControlLinesFailure {
            line_index: 0,
            total_lines: 2,
            dispatched_any: false,
            cause: std::io::Error::new(std::io::ErrorKind::NotFound, "未找到目标 service"),
        };
        let message = multi_one_shot_failure_message(&failure, 0);
        assert!(
            message.contains("还没发出第一条就失败"),
            "一条都没发出时不许说「第 1 条失败」; {message}"
        );
        assert!(message.contains("已完成 0 行"), "{message}");
    }
}
