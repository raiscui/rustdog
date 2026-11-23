//! 独立固定 echo 原型使用的 async 和 sync 行桥。
//!
//! 本桥尚未接入 rdog 的控制通道, 不能据此推断正式集成成本。
//!
//! 设计要点:
//! - 读侧:一个 async 任务把 QUIC 字节流切成行,经 `mpsc` 交给同步侧;
//! - 写侧:同步侧发送整行并等待 async 任务的 `write_all` + `flush` 结果;
//! - 同步侧用 `blocking_recv()` —— 它在 async 上下文里会 panic,所以同步角色
//!   必须跑在**独立线程**上(rdog 的 daemon 本来就是线程式的,天然满足)。

use std::io;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot},
};

use crate::protocol::MAX_CONTROL_MESSAGE_BYTES;

/// 行桥。同步侧使用。
pub struct LineBridge {
    /// 写侧:同步 → async。
    tx: mpsc::UnboundedSender<(Vec<u8>, oneshot::Sender<io::Result<()>>)>,
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

        let (byte_tx, byte_rx) = mpsc::unbounded_channel();
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
        let (done_tx, done_rx) = oneshot::channel();
        self.tx
            .send((buf, done_tx))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "写入任务已退出"))?;
        done_rx
            .blocking_recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "写入结果未返回"))?
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
async fn writer_task<W>(
    mut send: W,
    mut rx: mpsc::UnboundedReceiver<(Vec<u8>, oneshot::Sender<io::Result<()>>)>,
)
where
    W: AsyncWrite + Unpin,
{
    while let Some((bytes, done)) = rx.recv().await {
        let result = match send.write_all(&bytes).await {
            Ok(()) => send.flush().await,
            Err(err) => Err(err),
        };
        let failed = result.is_err();
        let _ = done.send(result);
        if failed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    struct FailingWriter {
        fail_flush: bool,
    }

    impl AsyncWrite for FailingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.fail_flush {
                Poll::Ready(Ok(data.len()))
            } else {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "注入写入失败")))
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "注入刷新失败")))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_write_failure_on_the_same_call() {
        let mut bridge = LineBridge::new(tokio::io::empty(), FailingWriter { fail_flush: false });
        let result = tokio::task::spawn_blocking(move || bridge.write_message("echo"))
            .await
            .unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "注入写入失败");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_flush_failure_on_the_same_call() {
        let mut bridge = LineBridge::new(tokio::io::empty(), FailingWriter { fail_flush: true });
        let result = tokio::task::spawn_blocking(move || bridge.write_message("echo"))
            .await
            .unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
        assert_eq!(error.to_string(), "注入刷新失败");
    }
}
