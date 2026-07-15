use std::path::Path;

use crate::input::Transport;
use crate::{control_frames, control_transport, zenoh_control, zenoh_runtime};

pub(crate) const LEGACY_ZENOH_PEER_TRANSPORT_ERROR: &str =
    "旧 transport `zenoh-peer` 已废弃; 请改用 `--transport zenoh`。`rdog control` 默认会自动发现 router，必要时再补 `--entry-point tcp/<router-host>:<port>`";

pub(crate) fn host_from_opts(host: Vec<String>) -> Result<(String, String), String> {
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

#[derive(Debug, Eq, PartialEq)]
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
        [] => Ok(ControlInvocation::ZenohLocal { namespace: None }),
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

pub(crate) fn parse_port(port: &str) -> Result<u16, String> {
    port.parse::<u16>()
        .map_err(|err| format!("Invalid port `{port}`: {err}"))
}

pub(crate) fn send_single_control_line_tcp(
    host: &str,
    port: u16,
    line: &str,
) -> Result<(), String> {
    let mut transport = control_transport::ControlTransport::connect_tcp(host, port)
        .map_err(|err| err.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::{
        extract_one_shot_lines, parse_port, resolve_control_invocation, ControlInvocation,
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
