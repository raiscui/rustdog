//! CDP 远程通道 (Chrome DevTools Protocol) — 最小闭环
//!
//! spec: `specs/cdp-remote-lane-spec.md`。定位: 与 AX web lane 互补 —
//! CDP 走 DOM/JS 路线, Windows 上 AX lane 整体 unsupported, CDP 是那边
//! 唯一能拿到 DOM 的路; Electron 应用原生支持 (spec 面 A)。
//!
//! # 安全模型 (spec 决策 3)
//!
//! CDP = 被控端任意代码执行。本 lane **默认关闭**, 需 daemon 配置
//! `[cdp] enabled = true` 显式开启; 开启后也只连**本机回环**的 debug 端口
//! (Chromium 的 remote-debugging-port 本就只绑回环), 不监听不转发任何端口。
//! `@capabilities` 报告当前开关状态。
//!
//! # 三个 verb
//!
//! - `@cdp-targets:{port}` — 列出 debug 端口上的 targets (页面/worker)
//! - `@cdp-eval:{port, expression, target_id?}` — 在指定页执行 JS
//!   (省 target_id 时用第一个 `type=="page"` 的 target)
//! - `@cdp:{port, method, params?, target_id?}` — 任意 CDP 方法透传
//!   (spec 决策 2: 透传保证能力不封死, 薄封装保证易用)
//!
//! # HTTP/WS 客户端
//!
//! `/json/list` 用 std TcpStream 手写 GET (一个端点, 不引 HTTP 库);
//! CDP 方法调用走 tungstenite (spec 决策已定: 同步纯 Rust client,
//! 与项目 `std::process` 风格一致; 项目无异步 runtime)。

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

/// CDP lane 的运行时开关。daemon 启动时由配置设置 ([`configure`]),
/// 默认 false — "默认关闭"是 spec 的安全要求, 不做反向默认。
static CDP_ENABLED: AtomicBool = AtomicBool::new(false);

/// daemon 启动接线: 把配置里的 `[cdp].enabled` 写进运行时开关。
/// 只该在 daemon 启动序列调用一次 (配置启动期 fail-fast 后不再变)。
pub fn configure(enabled: bool) {
    CDP_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn cdp_enabled() -> bool {
    CDP_ENABLED.load(Ordering::SeqCst)
}

/// CDP lane 关闭时的统一拒绝错误 (调用方转成 permission_denied envelope)。
/// 错误文本含 "[cdp] enabled" 关键字 — control_core 按它分流 permission 语义。
fn ensure_enabled() -> io::Result<()> {
    if cdp_enabled() {
        Ok(())
    } else {
        Err(io::Error::other(
            "CDP lane 已关闭: 在 daemon 配置 [cdp] enabled = true 后重启 daemon 以启用 \
             (安全模型: CDP 等同被控端任意代码执行, 默认不开放)",
        ))
    }
}

// ============================================================
// 请求模型与解析
// ============================================================

const DEFAULT_CDP_PORT: u16 = 9222;

#[derive(Debug, Clone, PartialEq)]
pub struct CdpRequest {
    pub port: u16,
    pub target_id: Option<String>,
    /// CDP 方法名 (透传), 如 "Runtime.evaluate" / "Page.captureScreenshot"
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CdpEvalRequest {
    pub port: u16,
    pub target_id: Option<String>,
    pub expression: String,
}

fn port_from_value(value: &Value) -> u16 {
    value
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(DEFAULT_CDP_PORT)
}

fn target_id_from_value(value: &Value) -> Option<String> {
    value
        .get("target_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn parse_cdp_request(payload: &str) -> io::Result<CdpRequest> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|e| io::Error::other(format!("@cdp payload 必须是 JSON 对象: {e}")))?;
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("@cdp 缺 method (如 \"Runtime.evaluate\")"))?
        .to_owned();
    Ok(CdpRequest {
        port: port_from_value(&value),
        target_id: target_id_from_value(&value),
        method,
        params: value.get("params").cloned(),
    })
}

pub fn parse_cdp_eval_request(payload: &str) -> io::Result<CdpEvalRequest> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|e| io::Error::other(format!("@cdp-eval payload 必须是 JSON 对象: {e}")))?;
    let expression = value
        .get("expression")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("@cdp-eval 缺 expression"))?
        .to_owned();
    Ok(CdpEvalRequest {
        port: port_from_value(&value),
        target_id: target_id_from_value(&value),
        expression,
    })
}

pub fn parse_cdp_targets_request(payload: &str) -> io::Result<u16> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|e| io::Error::other(format!("@cdp-targets payload 必须是 JSON 对象: {e}")))?;
    Ok(port_from_value(&value))
}

// ============================================================
// 执行
// ============================================================

/// `@cdp-targets`: GET /json/list, 原样回 targets 数组 (含 id/type/title/url)。
pub fn execute_cdp_targets(port: u16) -> io::Result<Value> {
    ensure_enabled()?;
    let body = http_get_json(port, "/json/list")?;
    let targets: Value =
        serde_json::from_str(&body).map_err(|e| io::Error::other(format!("targets 响应不是 JSON: {e}")))?;
    Ok(json!({
        "kind": "cdp-targets",
        "port": port,
        "targets": targets,
    }))
}

/// `@cdp`: 任意 CDP 方法透传。
pub fn execute_cdp(request: &CdpRequest) -> io::Result<Value> {
    ensure_enabled()?;
    let target_id = resolve_target_id(request.port, request.target_id.as_deref())?;
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/page/{target_id}", port = request.port);
    let result = cdp_call(&ws_url, &request.method, request.params.as_ref())?;
    Ok(json!({
        "kind": "cdp-result",
        "port": request.port,
        "target_id": target_id,
        "method": request.method,
        "result": result,
    }))
}

/// `@cdp-eval`: Runtime.evaluate + returnByValue, 返回值与异常文本。
pub fn execute_cdp_eval(request: &CdpEvalRequest) -> io::Result<Value> {
    ensure_enabled()?;
    let target_id = resolve_target_id(request.port, request.target_id.as_deref())?;
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/page/{target_id}", port = request.port);
    let params = json!({
        "expression": request.expression,
        "returnByValue": true,
        "awaitPromise": false,
    });
    let result = cdp_call(&ws_url, "Runtime.evaluate", Some(&params))?;
    // 拍平 Runtime.evaluate 的双壳 (result.result.value / result.exceptionDetails)
    let value = result
        .get("result")
        .and_then(|r| r.get("value"))
        .cloned();
    let exception = result
        .get("exceptionDetails")
        .map(|e| {
            e.get("exception")
                .and_then(|x| x.get("description"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(e.to_string()))
                .unwrap_or_default()
        });
    Ok(json!({
        "kind": "cdp-eval",
        "port": request.port,
        "target_id": target_id,
        "value": value,
        "exception": exception,
    }))
}

// ============================================================
// CDP 会话 (WS) 与 target 发现 (HTTP)
// ============================================================

/// 把 target_id 解析成具体 target: 未指定时选第一个 type=="page" 的。
/// 省略 target_id 的动机: 最常见场景就是"在主窗口里执行", 让 agent 少一步。
fn resolve_target_id(port: u16, target_id: Option<&str>) -> io::Result<String> {
    if let Some(id) = target_id {
        return Ok(id.to_owned());
    }
    let body = http_get_json(port, "/json/list")?;
    let targets: Value =
        serde_json::from_str(&body).map_err(|e| io::Error::other(format!("targets 响应不是 JSON: {e}")))?;
    targets
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|t| t.get("type").and_then(Value::as_str) == Some("page"))
        })
        .and_then(|t| t.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::other(format!(
                "debug 端口 {port} 上没有 type=page 的 target (应用未启动或未带 --remote-debugging-port?)"
            ))
        })
}

/// 手写 HTTP GET (只此一个端点, 不引 HTTP 客户端库)。
/// Connection: close 强制服务端关流, 读到 EOF 即完整响应。
fn http_get_json(port: u16, path: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| io::Error::other(format!("连不上 127.0.0.1:{port} (debug 端口未开?): {e}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| io::Error::other(e.to_string()))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| io::Error::other(format!("发送 HTTP 请求失败: {e}")))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| io::Error::other(format!("读取 HTTP 响应失败: {e}")))?;
    split_http_body(&raw)
}

/// 从原始 HTTP 响应字节里取 body (按 \r\n\r\n 分隔; 大小写不敏感头边界)。
fn split_http_body(raw: &[u8]) -> io::Result<String> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("HTTP 响应缺少头体分隔"))?;
    let status_line = String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == b'\r').unwrap_or(0)]);
    if !status_line.contains(" 200 ") {
        return Err(io::Error::other(format!("HTTP 状态异常: {}", status_line.trim())));
    }
    Ok(String::from_utf8_lossy(&raw[header_end + 4..]).into_owned())
}

/// 单次 CDP 调用: 连 WS → 发 method → 读到对应 id 的响应帧 → 关闭。
/// 每次调用新建连接 — 最小闭环不为低频调试动作维护长连会话状态
/// (console 流式订阅同理, spec 建议先做一次性拉取)。
fn cdp_call(ws_url: &str, method: &str, params: Option<&Value>) -> io::Result<Value> {
    let (mut ws, _resp) = tungstenite::client::connect(ws_url)
        .map_err(|e| io::Error::other(format!("WS 连接失败 ({ws_url}): {e}")))?;
    let request = json!({
        "id": 1,
        "method": method,
        "params": params.unwrap_or(&json!({})),
    });
    ws.send(tungstenite::Message::Text(request.to_string().into()))
        .map_err(|e| io::Error::other(format!("发送 CDP 请求失败: {e}")))?;

    // 读帧直到拿到 id=1 的响应 (事件帧无 id, 跳过) — 有超时上限防挂死
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(io::Error::other("CDP 响应超时 (30s)"));
        }
        let msg = ws
            .read()
            .map_err(|e| io::Error::other(format!("读取 CDP 响应失败: {e}")))?;
        let text = match msg {
            tungstenite::Message::Text(text) => text,
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) => continue,
            tungstenite::Message::Close(frame) => {
                return Err(io::Error::other(format!("CDP 连接被对端关闭: {frame:?}")));
            }
            _ => continue,
        };
        let value: Value = serde_json::from_str(&text).map_err(|e| io::Error::other(format!("CDP 响应不是 JSON: {e}")))?;
        if value.get("id").and_then(Value::as_u64) == Some(1) {
            if let Some(err) = value.get("error") {
                return Err(io::Error::other(format!("CDP 错误 ({method}): {err}")));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| io::Error::other(format!("CDP 响应缺少 result 字段: {text}")));
        }
        // 事件帧 (无 id 或 id != 1): 继续读
    }
}

// 小工具: 把 T 通过管道包成 Result (Rust 没有 |> ; 命名避免与 trait 冲突)
trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 权限门 ──
    #[test]
    fn disabled_lane_rejects_all_verbs() {
        // 默认 (未 configure(true)) 必须拒绝 — "默认关闭"是安全要求
        assert!(ensure_enabled().is_err());
        let err = ensure_enabled().unwrap_err();
        assert!(
            err.to_string().contains("[cdp] enabled"),
            "错误信息要给可照做的指引: {err}"
        );
    }

    // ── 解析 ──
    #[test]
    fn parse_cdp_request_full() {
        let r = parse_cdp_request(
            r#"{"port":9333,"method":"Page.captureScreenshot","target_id":"ABC"}"#,
        )
        .unwrap();
        assert_eq!(r.port, 9333);
        assert_eq!(r.method, "Page.captureScreenshot");
        assert_eq!(r.target_id.as_deref(), Some("ABC"));
        assert!(r.params.is_none());
    }

    #[test]
    fn parse_cdp_request_defaults_port_and_accepts_params() {
        let r = parse_cdp_request(
            r#"{"method":"Runtime.evaluate","params":{"expression":"1+1"}}"#,
        )
        .unwrap();
        assert_eq!(r.port, DEFAULT_CDP_PORT);
        assert_eq!(r.params.unwrap().get("expression").unwrap(), "1+1");
    }

    #[test]
    fn parse_cdp_request_requires_method() {
        assert!(parse_cdp_request(r#"{"port":9222}"#).is_err());
    }

    #[test]
    fn parse_cdp_eval_requires_expression() {
        assert!(parse_cdp_eval_request(r#"{"port":9222}"#).is_err());
        let r = parse_cdp_eval_request(r#"{"expression":"document.title"}"#).unwrap();
        assert_eq!(r.expression, "document.title");
        assert_eq!(r.port, DEFAULT_CDP_PORT);
    }

    #[test]
    fn parse_cdp_targets_defaults_port() {
        assert_eq!(parse_cdp_targets_request("{}").unwrap(), DEFAULT_CDP_PORT);
        assert_eq!(parse_cdp_targets_request(r#"{"port":9333}"#).unwrap(), 9333);
    }

    // ── HTTP body 切分 ──
    #[test]
    fn split_http_body_extracts_after_crlfcrlf() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n[{\"id\":\"x\"}]";
        assert_eq!(split_http_body(raw).unwrap(), r#"[{"id":"x"}]"#);
    }

    #[test]
    fn split_http_body_rejects_non_200() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\nnope";
        assert!(split_http_body(raw).is_err());
    }

    #[test]
    fn split_http_body_rejects_missing_separator() {
        assert!(split_http_body(b"HTTP/1.1 200 OK\r\n").is_err());
    }
}
