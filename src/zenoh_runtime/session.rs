//! Zenoh session打开、router发现与client endpoint解析。

use std::{
    io,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    time::{Duration, Instant},
};

use zenoh::{config::WhatAmI, scouting::Hello, Config, Session, Wait};

#[cfg(unix)]
use super::unixpipe::{unixpipe_base_path_alive, unixpipe_locator, unixpipe_socket_path};

pub fn open_router_session(listen_endpoints: &[String]) -> io::Result<Session> {
    open_session("router", &[], listen_endpoints)
}

pub fn open_client_session(connect_endpoints: &[String]) -> io::Result<Session> {
    open_session("client", connect_endpoints, &[])
}

pub fn resolve_client_connect_endpoints(
    connect_endpoints: &[String],
    discovery_timeout: Duration,
    unixpipe_probe: UnixpipeClientProbe<'_>,
) -> io::Result<Vec<String>> {
    if !connect_endpoints.is_empty() {
        // 用户显式给了 entry-point,尊重用户选择,不再尝试 unixpipe fast path。
        // 防止"显式给 udp/远端-host"却被本机 unixpipe 误抢先"的混淆。
        return Ok(connect_endpoints.to_vec());
    }

    // 本机 fast path:同机 daemon 通常会开 unixpipe endpoint。
    //
    // 实现要点:不能主动 open FIFO 探活(那会让 daemon 的 request channel 看到 EOF
    // 并破坏后续 client),只能用 `Path::exists` 做"FIFO 文件在不在"的轻量检查。
    // 如果 FIFO 在,直接把 unixpipe locator 作为唯一 connect endpoint 交给 zenoh::open。
    // zenoh::open 内部如果 unixpipe 不可达,会返回 Err,调用方在 `open_client_session`
    // 那一步会拿到错误并决定如何 fallback。
    #[cfg(unix)]
    {
        if let Some((namespace, target_name)) =
            unixpipe_probe.namespace.zip(unixpipe_probe.target_name)
        {
            if let Ok(base_path) = unixpipe_socket_path(namespace, target_name) {
                if unixpipe_base_path_alive(&base_path) {
                    log::info!(
                        "unixpipe endpoint detected, taking fast path (path: {})",
                        base_path.display()
                    );
                    return Ok(vec![unixpipe_locator(&base_path)]);
                }
            }
        }
    }

    autodiscover_router_endpoints(discovery_timeout)
}

/// 客户端 unixpipe fast path 提示。
///
/// 当 `namespace` 和 `target_name` 都是 `Some` 时,会触发 unixpipe 存在性检查并
/// 把对应的 locator 作为唯一 connect endpoint 返回,跳过 UDP scout。
/// `None` 任意一个都走老 autodiscover 路径(用于 `rdog control` 没指定 target 的场景)。
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixpipeClientProbe<'a> {
    pub namespace: Option<&'a str>,
    pub target_name: Option<&'a str>,
}

impl<'a> UnixpipeClientProbe<'a> {
    pub fn new(namespace: Option<&'a str>, target_name: Option<&'a str>) -> Self {
        Self {
            namespace,
            target_name,
        }
    }
}

fn open_session(
    mode: &str,
    connect_endpoints: &[String],
    listen_endpoints: &[String],
) -> io::Result<Session> {
    let mut config = Config::default();
    config
        .insert_json5("mode", &format!("\"{mode}\""))
        .map_err(to_io_error)?;

    if !connect_endpoints.is_empty() {
        let value = json_string_list(connect_endpoints);
        config
            .insert_json5("connect/endpoints", &value)
            .map_err(to_io_error)?;
    }

    if !listen_endpoints.is_empty() {
        let value = json_string_list(listen_endpoints);
        config
            .insert_json5("listen/endpoints", &value)
            .map_err(to_io_error)?;
    }

    zenoh::open(config)
        .wait()
        .map_err(|err| to_open_session_error(err, listen_endpoints))
}

fn autodiscover_router_endpoints(discovery_timeout: Duration) -> io::Result<Vec<String>> {
    // ------------------------------------------------------------
    // 这里不再把“发现 router”与“按 Hello 原始顺序逐个连接 locator”
    // 完全交给 zenoh::open() 内部处理。
    //
    // 原因是 Windows 多网卡现场里,Hello 可能先列出多个 169.254.* 死地址,
    // 3 秒 scouting 窗口会先被这些慢连接耗尽,还没轮到真正可达的 LAN IP。
    // 因此我们先自己 scout 一次,把 locator 排序后再显式 open。
    // ------------------------------------------------------------
    let mut config = Config::default();
    config
        .insert_json5("mode", r#""client""#)
        .map_err(to_io_error)?;

    let scout = zenoh::scout(WhatAmI::Router, config)
        .wait()
        .map_err(to_io_error)?;
    let deadline = Instant::now() + discovery_timeout;

    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "Zenoh autodiscovery 在 {}ms 内未找到可连接的 router locator",
                        discovery_timeout.as_millis()
                    ),
                )
            })?;

        match scout.recv_timeout(remaining) {
            Ok(Some(hello)) => {
                let endpoints = prioritize_hello_locators(&hello);
                if !endpoints.is_empty() {
                    return Ok(endpoints);
                }
            }
            Ok(None) => continue,
            Err(err) => {
                let kind = if Instant::now() >= deadline {
                    io::ErrorKind::TimedOut
                } else {
                    io::ErrorKind::Other
                };
                let message = if kind == io::ErrorKind::TimedOut {
                    format!(
                        "Zenoh autodiscovery 在 {}ms 内未找到可连接的 router locator",
                        discovery_timeout.as_millis()
                    )
                } else {
                    format!("Zenoh autodiscovery scout 提前结束: {err}")
                };
                return Err(io::Error::new(kind, message));
            }
        }
    }
}

fn prioritize_hello_locators(hello: &Hello) -> Vec<String> {
    let mut locators = hello
        .locators()
        .iter()
        .map(ToString::to_string)
        .filter(|locator| !is_serial_locator(locator))
        .collect::<Vec<_>>();

    locators.sort_by(|left, right| {
        locator_sort_key(left)
            .cmp(&locator_sort_key(right))
            .then_with(|| left.cmp(right))
    });
    locators.dedup();
    locators
}

fn locator_sort_key(locator: &str) -> (u8, &str) {
    match parse_locator_socket_addr(locator) {
        Some(addr) if addr.ip().is_loopback() => (0, locator),
        Some(addr) if is_link_local_ip(addr.ip()) => (2, locator),
        Some(_) => (1, locator),
        None => (3, locator),
    }
}

fn parse_locator_socket_addr(locator: &str) -> Option<SocketAddr> {
    let (_, address) = locator.split_once('/')?;
    let address = address.split(['#', '?']).next()?;
    SocketAddr::from_str(address).ok()
}

fn is_serial_locator(locator: &str) -> bool {
    locator
        .split_once('/')
        .map(|(scheme, _)| scheme.eq_ignore_ascii_case("serial"))
        .unwrap_or(false)
}

fn is_link_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 169 && octets[1] == 254
        }
        IpAddr::V6(v6) => v6.is_unicast_link_local(),
    }
}

fn json_string_list(values: &[String]) -> String {
    let joined = values
        .iter()
        .map(|value| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{joined}]")
}

fn to_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

fn to_open_session_error(err: impl std::fmt::Display, listen_endpoints: &[String]) -> io::Error {
    let message = err.to_string();

    if looks_like_windows_listen_access_denied(&message) && !listen_endpoints.is_empty() {
        let endpoints = listen_endpoints.join(", ");
        return io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{message}. 当前 Windows 现场对 listen_endpoints={endpoints} 的绑定被拒绝。请优先改用具体网卡 IP + 高位端口,例如 tcp/192.168.50.57:17447,不要先用 tcp/0.0.0.0:7447。"
            ),
        );
    }

    io::Error::other(message)
}

fn looks_like_windows_listen_access_denied(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("access is denied") || lower.contains("拒绝访问") || lower.contains("os error 5")
}

#[cfg(test)]
mod tests;
