//! **async ↔ sync 桥** —— 这就是 rdog 引入 iroh 的"侵入面"的实体。
//!
//! rdog 现有的 `ControlTransport`(`src/control_transport.rs:42`)是**同步 trait**:
//! `read_message()` / `write_message()` / `set_read_timeout()`,daemon 是线程式的,
//! `src/` 里没有 async runtime。
//!
//! iroh 是 tokio-based 的 async QUIC。所以要接上,必须有这样一层胶水。
//! 本文件刻意**写成生产可用的样子**(而不是 toy),因为它的行数就是选型成本。
//!
//! 设计要点:
//! - 读侧:一个 async 任务把 QUIC 字节流切成行,经 `mpsc` 交给同步侧;
//! - 写侧:同步侧把整行塞进 `mpsc`,一个 async 任务负责 `write_all` + `flush`;
//! - 同步侧用 `blocking_recv()` —— 它在 async 上下文里会 panic,所以同步角色
//!   必须跑在**独立线程**上(rdog 的 daemon 本来就是线程式的,天然满足)。

use std::io;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

use crate::protocol::MAX_CONTROL_MESSAGE_BYTES;

/// 行桥。同步侧使用。
pub struct LineBridge {
    /// 写侧:同步 → async。
    tx: mpsc::UnboundedSender<Vec<u8>>,
    /// 读侧:async → 同步。`blocking_recv` 只能在非 async 上下文用。
    rx: mpsc::UnboundedReceiver<Option<String>>,
}

impl LineBridge {
    /// 从一条已建立的 QUIC 双向流建桥。
    ///
    /// 读侧任务与写侧任务都在当前 tokio 运行时上跑;调用方在**另一个线程**
    /// 上使用返回的 `LineBridge`。
    pub fn new<R, W>(recv: R, send: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (line_tx, line_rx) = mpsc::unbounded_channel::<Option<String>>();
        tokio::spawn(reader_task(recv, line_tx));

        let (byte_tx, byte_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(writer_task(send, byte_rx));

        Self {
            tx: byte_tx,
            rx: line_rx,
        }
    }

    /// 写一行。语义对齐 `ControlTransport::write_message`。
    pub fn write_message(&mut self, line: &str) -> io::Result<()> {
        if line.len() + 1 > MAX_CONTROL_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "控制消息超过 {} 字节上限",
                    MAX_CONTROL_MESSAGE_BYTES
                ),
            ));
        }
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        self.tx
            .send(buf)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "对端已关闭"))
    }

    /// 读一行。语义对齐 `ControlTransport::read_message`。
    ///
    /// 返回 `Ok(None)` 表示对端关闭(EOF)。
    pub fn read_message(&mut self) -> io::Result<Option<String>> {
        match self.rx.blocking_recv() {
            Some(Some(line)) => Ok(Some(line)),
            Some(None) | None => Ok(None),
        }
    }
}

/// 读侧任务:字节流 → 行。
async fn reader_task<R>(mut recv: R, tx: mpsc::UnboundedSender<Option<String>>)
where
    R: AsyncRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = match recv.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(%err, "读侧出错");
                break;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
        // 按 \n 切行
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = buf.drain(..=pos).collect();
            line.pop(); // 去掉结尾 \n
            if tx.send(Some(String::from_utf8_lossy(&line).into_owned())).is_err() {
                return;
            }
        }
        // 超限行:与 rdog `read_line_limited` 同样的处置 —— 丢弃但保持流对齐。
        if buf.len() > MAX_CONTROL_MESSAGE_BYTES {
            tracing::warn!(
                bytes = buf.len(),
                "控制消息超过上限,已丢弃(连接保持)"
            );
            buf.clear();
        }
    }
    let _ = tx.send(None);
}

/// 写侧任务:行字节 → 字节流。
async fn writer_task<W>(mut send: W, mut rx: mpsc::UnboundedReceiver<Vec<u8>>)
where
    W: AsyncWrite + Unpin,
{
    while let Some(bytes) = rx.recv().await {
        if send.write_all(&bytes).await.is_err() {
            break;
        }
        if send.flush().await.is_err() {
            break;
        }
    }
}
