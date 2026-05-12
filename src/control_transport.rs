use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::random;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    io::{self, BufRead, BufReader, ErrorKind, Read, Write},
    net::{Shutdown, TcpStream},
    time::Duration,
};
use url::Url;

const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_HTTP_HANDSHAKE_BYTES: usize = 16 * 1024;
// phase 2 起,控制面已经允许 `@savefile` 承载单张 JPEG/base64 screenshot。
// 64 KiB 对这类 payload 明显不够,这里先把 phase 1 的单消息上限抬到 8 MiB。
// 这仍然是“单条消息有边界”的保守口径,只是从命令级文本扩到了单文件结果。
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Copy, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ControlTransportKind {
    #[default]
    Tcp,
    #[serde(rename = "websocket")]
    WebSocket,
}

pub(crate) enum ControlTransport {
    Tcp(TcpLineTransport),
    WebSocket(WebSocketTextTransport),
}

pub(crate) enum ControlTransportWriter {
    Tcp(TcpLineWriter),
    WebSocket(WebSocketTextWriter),
}

impl ControlTransport {
    pub fn connect_tcp(host: &str, port: u16) -> io::Result<Self> {
        let stream = TcpStream::connect((host, port))?;
        Self::from_tcp_stream(stream)
    }

    pub fn connect_websocket(raw_url: &str) -> io::Result<Self> {
        let request = WebSocketClientRequest::parse(raw_url)?;
        let mut stream = TcpStream::connect((request.host.as_str(), request.port))?;
        write_client_handshake(&mut stream, &request)?;
        read_server_handshake(&mut stream, &request.expected_accept)?;
        Ok(Self::WebSocket(WebSocketTextTransport::new(stream, true)))
    }

    pub fn from_tcp_stream(stream: TcpStream) -> io::Result<Self> {
        Ok(Self::Tcp(TcpLineTransport::new(stream)?))
    }

    pub fn read_message(&mut self) -> io::Result<Option<String>> {
        match self {
            Self::Tcp(transport) => transport.read_message(),
            Self::WebSocket(transport) => transport.read_message(),
        }
    }

    pub fn write_message(&mut self, message: &str) -> io::Result<()> {
        match self {
            Self::Tcp(transport) => transport.write_message(message),
            Self::WebSocket(transport) => transport.write_message(message),
        }
    }

    pub fn try_clone_writer(&self) -> io::Result<ControlTransportWriter> {
        match self {
            Self::Tcp(transport) => transport
                .try_clone_writer()
                .map(ControlTransportWriter::Tcp),
            Self::WebSocket(transport) => transport
                .try_clone_writer()
                .map(ControlTransportWriter::WebSocket),
        }
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(transport) => transport.set_read_timeout(timeout),
            Self::WebSocket(transport) => transport.set_read_timeout(timeout),
        }
    }

    pub fn close(self) -> io::Result<()> {
        match self {
            Self::Tcp(transport) => transport.close(),
            Self::WebSocket(transport) => transport.close(),
        }
    }
}

impl ControlTransportWriter {
    pub fn write_message(&mut self, message: &str) -> io::Result<()> {
        match self {
            Self::Tcp(writer) => writer.write_message(message),
            Self::WebSocket(writer) => writer.write_message(message),
        }
    }
}

pub fn accept_websocket_stream(mut stream: TcpStream) -> io::Result<ControlTransport> {
    let request = read_http_head(&mut stream)?;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut headers);
    parsed.parse(&request).map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("websocket 请求握手无法解析: {err}"),
        )
    })?;

    let method = parsed
        .method
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "websocket 握手缺少 HTTP 方法"))?;
    if method != "GET" {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("websocket 握手必须使用 GET,实际是 {method}"),
        ));
    }

    let upgrade = header_value(parsed.headers, "Upgrade")?;
    if !upgrade.eq_ignore_ascii_case("websocket") {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "websocket 握手缺少 Upgrade: websocket",
        ));
    }

    let connection = header_value(parsed.headers, "Connection")?;
    if !connection
        .split(',')
        .map(str::trim)
        .any(|value| value.eq_ignore_ascii_case("upgrade"))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "websocket 握手缺少 Connection: Upgrade",
        ));
    }

    let version = header_value(parsed.headers, "Sec-WebSocket-Version")?;
    if version != "13" {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("phase 1 只支持 Sec-WebSocket-Version: 13,实际是 {version}"),
        ));
    }

    let key = header_value(parsed.headers, "Sec-WebSocket-Key")?;
    let accept = websocket_accept_value(key);

    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.flush()?;

    Ok(ControlTransport::WebSocket(WebSocketTextTransport::new(
        stream, false,
    )))
}

pub(crate) struct TcpLineTransport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

pub(crate) struct TcpLineWriter {
    writer: TcpStream,
}

impl TcpLineTransport {
    fn new(stream: TcpStream) -> io::Result<Self> {
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
        })
    }

    fn read_message(&mut self) -> io::Result<Option<String>> {
        loop {
            let mut line = String::new();
            let bytes_read = self.reader.read_line(&mut line)?;

            if bytes_read == 0 {
                return Ok(None);
            }

            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }

            return Ok(Some(line.to_owned()));
        }
    }

    fn write_message(&mut self, message: &str) -> io::Result<()> {
        self.writer.write_all(message.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    fn try_clone_writer(&self) -> io::Result<TcpLineWriter> {
        Ok(TcpLineWriter {
            writer: self.writer.try_clone()?,
        })
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }

    fn close(mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.shutdown(Shutdown::Both)
    }
}

impl TcpLineWriter {
    fn write_message(&mut self, message: &str) -> io::Result<()> {
        self.writer.write_all(message.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

pub(crate) struct WebSocketTextTransport {
    stream: TcpStream,
    write_masked: bool,
}

pub(crate) struct WebSocketTextWriter {
    stream: TcpStream,
    write_masked: bool,
}

impl WebSocketTextTransport {
    fn new(stream: TcpStream, write_masked: bool) -> Self {
        Self {
            stream,
            write_masked,
        }
    }

    fn read_message(&mut self) -> io::Result<Option<String>> {
        loop {
            let frame = WebSocketFrame::read_from(&mut self.stream)?;

            match frame.opcode {
                WebSocketOpcode::Text => {
                    let text = String::from_utf8(frame.payload).map_err(|err| {
                        io::Error::new(
                            ErrorKind::InvalidData,
                            format!("websocket text frame 不是合法 UTF-8: {err}"),
                        )
                    })?;

                    if text.trim().is_empty() {
                        continue;
                    }

                    return Ok(Some(text));
                }
                WebSocketOpcode::Binary => {
                    let _ = WebSocketFrame::close(Vec::new())
                        .write_to(&mut self.stream, self.write_masked);
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "phase 1 websocket control 不支持 binary frame",
                    ));
                }
                WebSocketOpcode::Ping => {
                    WebSocketFrame::pong(frame.payload)
                        .write_to(&mut self.stream, self.write_masked)?;
                }
                WebSocketOpcode::Pong => {}
                WebSocketOpcode::Close => {
                    let _ = WebSocketFrame::close(frame.payload)
                        .write_to(&mut self.stream, self.write_masked);
                    return Ok(None);
                }
            }
        }
    }

    fn write_message(&mut self, message: &str) -> io::Result<()> {
        WebSocketFrame::text(message.as_bytes().to_vec())
            .write_to(&mut self.stream, self.write_masked)
    }

    fn try_clone_writer(&self) -> io::Result<WebSocketTextWriter> {
        Ok(WebSocketTextWriter {
            stream: self.stream.try_clone()?,
            write_masked: self.write_masked,
        })
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    fn close(mut self) -> io::Result<()> {
        let _ = WebSocketFrame::close(Vec::new()).write_to(&mut self.stream, self.write_masked);
        self.stream.shutdown(Shutdown::Both)
    }
}

impl WebSocketTextWriter {
    fn write_message(&mut self, message: &str) -> io::Result<()> {
        WebSocketFrame::text(message.as_bytes().to_vec())
            .write_to(&mut self.stream, self.write_masked)
    }
}

#[derive(Debug)]
struct WebSocketClientRequest {
    host: String,
    port: u16,
    host_header: String,
    request_target: String,
    nonce: String,
    expected_accept: String,
}

impl WebSocketClientRequest {
    fn parse(raw_url: &str) -> io::Result<Self> {
        let url = Url::parse(raw_url).map_err(|err| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!("无效 websocket URL `{raw_url}`: {err}"),
            )
        })?;

        match url.scheme() {
            "ws" => {}
            "wss" => {
                return Err(io::Error::new(
                    ErrorKind::Unsupported,
                    "phase 1 websocket control 暂不支持 wss://",
                ))
            }
            scheme => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("不支持的 websocket URL scheme `{scheme}`"),
                ))
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "websocket URL 缺少 host"))?
            .to_owned();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "websocket URL 缺少可用端口"))?;
        let path = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };
        let request_target = match url.query() {
            Some(query) => format!("{path}?{query}"),
            None => path.to_owned(),
        };
        let host_header = if port == 80 {
            host.clone()
        } else {
            format!("{host}:{port}")
        };
        let nonce = BASE64_STANDARD.encode(random::<[u8; 16]>());
        let expected_accept = websocket_accept_value(&nonce);

        Ok(Self {
            host,
            port,
            host_header,
            request_target,
            nonce,
            expected_accept,
        })
    }
}

fn write_client_handshake(
    stream: &mut TcpStream,
    request: &WebSocketClientRequest,
) -> io::Result<()> {
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\n\r\n",
        request.request_target,
        request.host_header,
        request.nonce
    )?;
    stream.flush()
}

fn websocket_accept_value(key: &str) -> String {
    let mut digest = Sha1::new();
    digest.update(key.as_bytes());
    digest.update(WEBSOCKET_GUID.as_bytes());
    BASE64_STANDARD.encode(digest.finalize())
}

fn read_server_handshake(stream: &mut TcpStream, expected_accept: &str) -> io::Result<()> {
    let response = read_http_head(stream)?;
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut parsed = httparse::Response::new(&mut headers);
    parsed.parse(&response).map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("websocket 响应握手无法解析: {err}"),
        )
    })?;

    let status = parsed
        .code
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "握手响应缺少状态码"))?;
    if status != 101 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("websocket 握手失败,响应状态不是 101: {status}"),
        ));
    }

    let accept_header = header_value(parsed.headers, "Sec-WebSocket-Accept")?;
    if accept_header != expected_accept {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "websocket 握手返回的 Sec-WebSocket-Accept 不匹配",
        ));
    }

    Ok(())
}

fn read_http_head(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let bytes_read = stream.read(&mut chunk)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "websocket 握手在头部结束前就断开了",
            ));
        }

        buffer.extend_from_slice(&chunk[..bytes_read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(buffer);
        }

        if buffer.len() > MAX_HTTP_HANDSHAKE_BYTES {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "websocket 握手头过大",
            ));
        }
    }
}

fn header_value<'a>(headers: &'a [httparse::Header<'a>], name: &str) -> io::Result<&'a str> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!("websocket 握手缺少头: {name}"),
            )
        })?;

    std::str::from_utf8(header.value).map_err(|err| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("websocket 头 `{name}` 不是合法 UTF-8: {err}"),
        )
    })
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum WebSocketOpcode {
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

struct WebSocketFrame {
    opcode: WebSocketOpcode,
    payload: Vec<u8>,
}

impl WebSocketFrame {
    fn text(payload: Vec<u8>) -> Self {
        Self {
            opcode: WebSocketOpcode::Text,
            payload,
        }
    }

    fn pong(payload: Vec<u8>) -> Self {
        Self {
            opcode: WebSocketOpcode::Pong,
            payload,
        }
    }

    fn close(payload: Vec<u8>) -> Self {
        Self {
            opcode: WebSocketOpcode::Close,
            payload,
        }
    }

    fn read_from(stream: &mut TcpStream) -> io::Result<Self> {
        let mut header = [0_u8; 2];
        stream.read_exact(&mut header)?;

        let fin = header[0] & 0b1000_0000 != 0;
        if !fin {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "phase 1 websocket control 不支持分片 frame",
            ));
        }

        let opcode = match header[0] & 0b0000_1111 {
            0x1 => WebSocketOpcode::Text,
            0x2 => WebSocketOpcode::Binary,
            0x8 => WebSocketOpcode::Close,
            0x9 => WebSocketOpcode::Ping,
            0xA => WebSocketOpcode::Pong,
            other => {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("phase 1 websocket control 不支持 opcode: {other:#x}"),
                ))
            }
        };

        let masked = header[1] & 0b1000_0000 != 0;
        let payload_len = read_payload_len(stream, header[1] & 0b0111_1111)?;
        if payload_len > MAX_MESSAGE_BYTES {
            return Err(io::Error::new(ErrorKind::InvalidData, "websocket 消息过大"));
        }

        let masking_key = if masked {
            let mut key = [0_u8; 4];
            stream.read_exact(&mut key)?;
            Some(key)
        } else {
            None
        };

        let mut payload = vec![0_u8; payload_len];
        stream.read_exact(&mut payload)?;

        if let Some(masking_key) = masking_key {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= masking_key[index % 4];
            }
        }

        Ok(Self { opcode, payload })
    }

    fn write_to(self, stream: &mut TcpStream, mask: bool) -> io::Result<()> {
        let opcode = match self.opcode {
            WebSocketOpcode::Text => 0x1,
            WebSocketOpcode::Binary => 0x2,
            WebSocketOpcode::Close => 0x8,
            WebSocketOpcode::Ping => 0x9,
            WebSocketOpcode::Pong => 0xA,
        };

        let mut header = vec![0b1000_0000 | opcode];
        write_payload_len(&mut header, self.payload.len(), mask)?;
        stream.write_all(&header)?;

        if mask {
            let masking_key = random::<[u8; 4]>();
            stream.write_all(&masking_key)?;

            let mut masked_payload = self.payload;
            for (index, byte) in masked_payload.iter_mut().enumerate() {
                *byte ^= masking_key[index % 4];
            }
            stream.write_all(&masked_payload)?;
        } else {
            stream.write_all(&self.payload)?;
        }

        stream.flush()
    }
}

fn read_payload_len(stream: &mut TcpStream, marker: u8) -> io::Result<usize> {
    match marker {
        value @ 0..=125 => Ok(value as usize),
        126 => {
            let mut extended = [0_u8; 2];
            stream.read_exact(&mut extended)?;
            Ok(u16::from_be_bytes(extended) as usize)
        }
        127 => {
            let mut extended = [0_u8; 8];
            stream.read_exact(&mut extended)?;
            let value = u64::from_be_bytes(extended);
            usize::try_from(value).map_err(|_| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    "websocket 消息长度超出当前平台 usize 上限",
                )
            })
        }
        _ => Err(io::Error::new(
            ErrorKind::InvalidData,
            "websocket payload length marker 非法",
        )),
    }
}

fn write_payload_len(header: &mut Vec<u8>, payload_len: usize, mask: bool) -> io::Result<()> {
    let mask_bit = if mask { 0b1000_0000 } else { 0 };

    match payload_len {
        0..=125 => header.push(mask_bit | payload_len as u8),
        126..=65535 => {
            header.push(mask_bit | 126);
            header.extend_from_slice(&(payload_len as u16).to_be_bytes());
        }
        _ => {
            let extended = u64::try_from(payload_len).map_err(|_| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "websocket payload 长度无法转换为 u64",
                )
            })?;
            header.push(mask_bit | 127);
            header.extend_from_slice(&extended.to_be_bytes());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    fn bind_loopback_listener() -> std::net::TcpListener {
        let mut last_error = None;

        for _ in 0..20 {
            match std::net::TcpListener::bind(("127.0.0.1", 0)) {
                Ok(listener) => return listener,
                Err(err) => {
                    last_error = Some(err);
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }

        panic!(
            "listener should bind after retries: {:?}",
            last_error.expect("bind retry should record last error")
        );
    }

    #[test]
    fn websocket_client_request_should_reject_non_ws_scheme() {
        let err = WebSocketClientRequest::parse("http://127.0.0.1:8080").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn websocket_client_request_should_reject_wss_in_phase1() {
        let err = WebSocketClientRequest::parse("wss://127.0.0.1:8080").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn websocket_client_request_should_preserve_query_string_in_request_target() {
        let request = WebSocketClientRequest::parse("ws://127.0.0.1:8080/control?mode=json&v=1")
            .expect("ws request should parse");

        assert_eq!(request.host, "127.0.0.1");
        assert_eq!(request.port, 8080);
        assert_eq!(request.host_header, "127.0.0.1:8080");
        assert_eq!(request.request_target, "/control?mode=json&v=1");
        assert!(!request.nonce.is_empty(), "nonce should not be empty");
        assert!(
            !request.expected_accept.is_empty(),
            "expected accept should not be empty"
        );
    }

    #[cfg_attr(
        windows,
        ignore = "当前 Windows 环境下 TcpListener provider 初始化不稳定,业务链路由更高层 websocket 测试覆盖"
    )]
    #[test]
    fn websocket_client_transport_should_mask_outbound_frames() {
        let listener = bind_loopback_listener();
        let port = listener
            .local_addr()
            .expect("listener should expose local addr")
            .port();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept client");
            let request = read_http_head(&mut stream).expect("server should read handshake");
            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut parsed = httparse::Request::new(&mut headers);
            parsed
                .parse(&request)
                .expect("server should parse websocket request");
            let key = header_value(parsed.headers, "Sec-WebSocket-Key")
                .expect("handshake should carry key");
            let accept = websocket_accept_value(key);
            write!(
                stream,
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            )
            .expect("server should write handshake response");
            stream
                .flush()
                .expect("server should flush handshake response");

            let mut header = [0_u8; 2];
            stream
                .read_exact(&mut header)
                .expect("server should read first frame header");
            assert_ne!(
                header[1] & 0b1000_0000,
                0,
                "client -> server websocket frame must carry mask bit"
            );
        });

        let mut client =
            ControlTransport::connect_websocket(format!("ws://127.0.0.1:{port}").as_str())
                .expect("client should connect websocket");
        client
            .write_message("@ping")
            .expect("client should send websocket message");
        let _ = client.close();

        server.join().expect("server thread should finish");
    }
}
