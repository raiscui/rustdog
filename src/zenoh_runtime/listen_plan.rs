//! daemon 启动期的 listen endpoint 分类与可用性预检 (RAI-383)。
//!
//! Zenoh 对 serial 端点是**惰性**的: `zenoh-link-serial` 的 `new_listener`
//! 不打开串口, 只把 endpoint 记进 locator 集合, 真正打开设备发生在后台
//! accept 循环 (`accept_read_task`)。后果是一个根本不存在的串口仍会出现在
//! `Zenoh can be reached at:` 的对外通告里, 对端按它拨号必然失败, 而启动
//! 日志没有任何提示。
//!
//! 这里在把 endpoint 交给 Zenoh 之前分两类:
//! - **必选** (`tcp` / `udp` / `tls` / `unixpipe` 等控制面入口): 一律保留,
//!   绑定失败仍由 Zenoh 致命上报; endpoint 形状非法时直接返回错误。
//! - **可选** (`serial`): 只有设备实际可用才进入对外通告的 locator 集合,
//!   否则记一条带原因的被剔除项, 由调用方降级为 warn 并继续启动。

use std::io;

use zenoh::config::EndPoint;

const SERIAL_PROTOCOL: &str = "serial";

/// 因设备不可用而未进入 locator 集合的可选 listener。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedListenEndpoint {
    pub endpoint: String,
    pub reason: String,
}

/// daemon 实际交给 Zenoh 的 listener 集合, 以及被剔除的可选 listener。
#[derive(Debug, Clone)]
pub struct ListenEndpointPlan {
    pub endpoints: Vec<String>,
    pub dropped: Vec<DroppedListenEndpoint>,
}

/// 分类 `[zenoh].listen_endpoints`: 必选入口原样保留, 可选 serial 入口先探测设备。
pub fn plan_listen_endpoints(endpoints: &[String]) -> io::Result<ListenEndpointPlan> {
    let mut planned = Vec::with_capacity(endpoints.len());
    let mut dropped = Vec::new();

    for endpoint in endpoints {
        let parsed = endpoint.parse::<EndPoint>().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("listen endpoint `{endpoint}` 无法解析: {err}"),
            )
        })?;
        if parsed.protocol().as_str() != SERIAL_PROTOCOL {
            planned.push(endpoint.clone());
            continue;
        }
        match probe_serial_device(parsed.address().as_str()) {
            Ok(()) => planned.push(endpoint.clone()),
            Err(reason) => dropped.push(DroppedListenEndpoint {
                endpoint: endpoint.clone(),
                reason,
            }),
        }
    }

    Ok(ListenEndpointPlan {
        endpoints: planned,
        dropped,
    })
}

/// 探测 serial endpoint 指向的设备当前是否可用; `Err` 里是给人看的原因。
fn probe_serial_device(device_path: &str) -> Result<(), String> {
    if device_path.trim().is_empty() {
        return Err("endpoint 缺少串口设备路径".to_owned());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;

        let metadata = std::fs::metadata(device_path)
            .map_err(|err| format!("设备 {device_path} 无法访问: {err}"))?;
        if !metadata.file_type().is_char_device() {
            return Err(format!("{device_path} 存在但不是字符设备, 不是串口"));
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        let path = if device_path.starts_with(r"\\.\") {
            device_path.to_owned()
        } else {
            format!(r"\\.\{device_path}")
        };
        // 与 zenoh 底下的 serialport 保持同一打开语义: 读写 + share_mode 0。
        // 通信设备不接受默认的 FILE_SHARE_READ|WRITE|DELETE, 用默认共享模式会把
        // 一个本来可用的串口判成不可用 (serialport-4.9.0/src/windows/com.rs:62)。
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .map(|_| ())
            .map_err(|err| format!("无法打开串口 {device_path}: {err}"))
    }

    #[cfg(not(any(unix, windows)))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable_serial_endpoint() -> String {
        #[cfg(windows)]
        {
            "serial/COM255#baudrate=115200".to_owned()
        }

        #[cfg(not(windows))]
        {
            "serial//dev/rdog-rai383-no-such-serial#baudrate=115200".to_owned()
        }
    }

    #[test]
    fn optional_serial_listener_should_degrade_when_device_is_unavailable() {
        let serial = unreachable_serial_endpoint();
        let endpoints = vec!["tcp/127.0.0.1:17447".to_owned(), serial.clone()];

        let plan = plan_listen_endpoints(&endpoints).expect("必选入口可用时不应报错");

        assert_eq!(plan.endpoints, vec!["tcp/127.0.0.1:17447".to_owned()]);
        assert_eq!(plan.dropped.len(), 1);
        assert_eq!(plan.dropped[0].endpoint, serial);
        assert!(
            !plan.dropped[0].reason.trim().is_empty(),
            "剔除原因必须非空, 启动日志靠它解释为什么该 endpoint 不可用"
        );
    }

    #[cfg(unix)]
    #[test]
    fn optional_serial_listener_should_stay_when_device_exists() {
        let endpoint = "serial//dev/null#baudrate=115200".to_owned();
        let endpoints = vec!["tcp/127.0.0.1:17447".to_owned(), endpoint.clone()];

        let plan = plan_listen_endpoints(&endpoints).expect("存在的字符设备应通过探测");

        assert_eq!(plan.endpoints, endpoints);
        assert!(plan.dropped.is_empty());
    }

    #[test]
    fn required_listener_should_stay_even_when_address_is_unreachable() {
        let endpoints = vec![
            "tcp/192.0.2.1:17447".to_owned(),
            unreachable_serial_endpoint(),
        ];

        let plan = plan_listen_endpoints(&endpoints).expect("必选入口不做可达性探测");

        assert_eq!(plan.endpoints, vec!["tcp/192.0.2.1:17447".to_owned()]);
        assert_eq!(plan.dropped.len(), 1);
    }

    #[test]
    fn required_listener_parse_failure_should_stay_fatal() {
        let endpoints = vec![
            "tcp/127.0.0.1:17447".to_owned(),
            "bogus-endpoint".to_owned(),
        ];

        let err = plan_listen_endpoints(&endpoints).expect_err("形状非法的必选入口必须致命");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("bogus-endpoint"));
    }
}
