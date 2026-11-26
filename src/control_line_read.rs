//! 有上限的行读取 —— line-based 控制协议的边界
//!
//! # 为什么需要它
//!
//! 协议是**按行文本**的（`@verb` / `{json}` 每条一行）。原先四处读取
//! （入站 socket、控制端 stdin、shell 监听器）都用 `BufRead::read_line`，
//! 它会一直读到 `\n` 为止，**中间没有任何上限**。
//!
//! 后果：一条畸形或超大消息（未来 `@file-chunk` 传 base64 载荷、
//! 一次误粘贴的巨型截图、恶意构造的长行）会让 daemon **无界缓冲**，
//! 直到内存耗尽或连接被对端撑爆 —— 而且不产生任何错误，只是变慢/变卡。
//!
//! # 契约
//!
//! - 超过 [`MAX_CONTROL_MESSAGE_BYTES`] 的行**不是错误**（坏行已被消费），
//!   调用方拿到 [`LineOutcome::TooLong`] 后应**向用户/日志披露并继续服务**，
//!   而不是终止会话 —— 可恢复的坏行不等于连接死亡。
//! - 坏行的剩余部分会被**读到 `\n` 为止并丢弃**，保证流不错位：
//!   下一行仍是完整消息，不会收到半截。
//! - 用 `read_until` 而非 `read_line`：后者遇无效 UTF-8 直接失败，
//!   我们需要"先按字节计长、超限就跳过、否则再解 UTF-8"。

use std::io::{self, BufRead};

/// 单条控制消息的字节上限（含行尾，不含 `\n`）。
///
/// 取 8 MiB 的理由：既能容纳 4K 截图的 base64 载荷（数 MB 量级），
/// 又让未来分块传输的 chunk 有明确天花板（建议取本值的 80% 以下）。
/// 超过即判定为坏行 —— line-based 协议下超长行没有合理语义。
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// 一次有上限读行的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineOutcome {
    /// 读到一条完整行（已去除行尾 `\r` / `\n`）。
    Line(String),
    /// 对端关闭，无更多数据。
    Eof,
    /// 行超过上限；坏行剩余部分已被消费丢弃，**流仍然对齐**。
    TooLong {
        /// 实际读到的字节数（含被丢弃部分），用于向用户披露规模。
        consumed_bytes: usize,
    },
}

/// 从 `reader` 读一条以 `\n` 结尾的消息，行**内容**（不含行尾）超过
/// `limit` 字节时返回 [`LineOutcome::TooLong`]，并把该行剩余部分消费掉。
///
/// `buf` 与 `sink` 由调用方持有并在多次调用间复用，避免每行重新分配。
/// `sink` 只在超限丢弃阶段使用。
pub fn read_line_limited<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    sink: &mut Vec<u8>,
    limit: usize,
) -> io::Result<LineOutcome> {
    // 不清 buf: 上一次调用可能因读超时/中断返回错误, 而半个行已经攒在里面。
    // 清掉它就是把对端「先发正文、后发换行」的分片消息丢掉 —— 25ms 读超时的
    // PTY 桥接最容易踩到 (RAI-415: @pty-stdin 帧被吞, 远端收不到 ^C)。
    let mut total = buf.len();
    let mut over_limit = false;

    loop {
        // read_until 读到行尾或 EOF 为止 —— 这是"消费到换行"的唯一动作。
        // sink 是丢弃目标, 每轮清空即可; buf 是累积目标, **绝不能每轮清**
        // —— 否则对端在没有换行的情况下关闭时, 末行内容会被清掉。
        if over_limit {
            sink.clear();
        }
        let target: &mut Vec<u8> = if over_limit { sink } else { buf };

        let n = match reader.read_until(b'\n', target) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };

        if n == 0 {
            // 对端关闭且本行未收到任何字节
            if total == 0 {
                buf.clear();
                return Ok(LineOutcome::Eof);
            }
            break;
        }
        total += n;

        if over_limit {
            // 丢弃阶段：只等行尾，不累积
            if target.last() == Some(&b'\n') {
                break;
            }
            continue;
        }

        if buf.last() == Some(&b'\n') {
            // 行已完整：按"内容长度"判超限（不把行尾算进去）
            let content_len = buf.len() - 1;
            let content_len = if buf.ends_with(b"\r\n") {
                content_len - 1
            } else {
                content_len
            };
            if content_len > limit {
                over_limit = true;
                buf.clear();
                // 本次已消费到行尾，直接收手
                break;
            }
            break;
        }

        // 行尚未读完：累积已超限就切换到丢弃模式
        if buf.len() > limit {
            over_limit = true;
            buf.clear();
        }
    }

    if over_limit {
        buf.clear();
        return Ok(LineOutcome::TooLong {
            consumed_bytes: total,
        });
    }

    // 去掉行尾 \r / \n（兼容 CRLF）
    let line = buf.strip_suffix(b"\n").unwrap_or(&buf[..]);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let outcome = match String::from_utf8(line.to_vec()) {
        Ok(text) => LineOutcome::Line(text),
        // 无效 UTF-8：当作坏行处理（流已对齐到行尾, 不影响后续消息）
        Err(_) => LineOutcome::TooLong {
            consumed_bytes: total,
        },
    };
    // 行已消费完, 这里才清 —— 交给下一次调用一个干净的起点。
    buf.clear();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read_all(input: &[u8], limit: usize) -> Vec<LineOutcome> {
        let mut reader = Cursor::new(input.to_vec());
        let mut buf = Vec::new();
        let mut sink = Vec::new();
        let mut out = Vec::new();
        loop {
            match read_line_limited(&mut reader, &mut buf, &mut sink, limit).unwrap() {
                LineOutcome::Eof => return out,
                other => out.push(other),
            }
        }
    }

    #[test]
    fn reads_plain_lines() {
        let got = read_all(b"@ping\n@capabilities\n", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(
            got,
            vec![
                LineOutcome::Line("@ping".into()),
                LineOutcome::Line("@capabilities".into()),
            ]
        );
    }

    #[test]
    fn strips_crlf() {
        let got = read_all(b"@ping\r\n", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(got, vec![LineOutcome::Line("@ping".into())]);
    }

    /// 按脚本吐出数据块 / 读超时的假 reader: 复现「正文与换行分两次到达,
    /// 中间被读超时打断」的分片消息 (RAI-415)。
    struct ChunkedReader {
        steps: std::collections::VecDeque<Result<Vec<u8>, io::ErrorKind>>,
    }

    impl std::io::Read for ChunkedReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            match self.steps.pop_front() {
                Some(Ok(chunk)) => {
                    out[..chunk.len()].copy_from_slice(&chunk);
                    Ok(chunk.len())
                }
                Some(Err(kind)) => Err(io::Error::new(kind, "scripted read error")),
                None => Ok(0),
            }
        }
    }

    #[test]
    fn partial_line_survives_read_timeout_between_chunks() {
        let mut reader = std::io::BufReader::new(ChunkedReader {
            steps: std::collections::VecDeque::from(vec![
                Ok(b"@pty-stdin {\"session_id\":\"a\"".to_vec()),
                Err(io::ErrorKind::WouldBlock),
                Ok(b",\"data\":\"Aw==\"}\n".to_vec()),
            ]),
        });
        let mut buf = Vec::new();
        let mut sink = Vec::new();

        let err = read_line_limited(&mut reader, &mut buf, &mut sink, MAX_CONTROL_MESSAGE_BYTES)
            .expect_err("首块没有换行, 应把读超时原样上抛");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            String::from_utf8_lossy(&buf),
            "@pty-stdin {\"session_id\":\"a\"",
            "已收到的半行必须留在 buf 里"
        );

        let outcome =
            read_line_limited(&mut reader, &mut buf, &mut sink, MAX_CONTROL_MESSAGE_BYTES)
                .expect("超时后的续读应成功");
        assert_eq!(
            outcome,
            LineOutcome::Line("@pty-stdin {\"session_id\":\"a\",\"data\":\"Aw==\"}".to_owned()),
            "续读必须拼上后半截, 而不是只剩后半截"
        );
        assert!(buf.is_empty(), "整行交出后 buf 应被清空");
    }

    #[test]
    fn empty_input_is_eof() {
        assert!(read_all(b"", 1024).is_empty());
    }

    #[test]
    fn oversized_line_is_reported_and_stream_stays_aligned() {
        // 首行超限, 后面跟一条正常行 —— 关键断言: 坏行被消费干净
        let input = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaa\n@ping\n".to_vec();
        let got = read_all(&input, 10);
        assert_eq!(
            got,
            vec![
                LineOutcome::TooLong { consumed_bytes: 29 },
                LineOutcome::Line("@ping".into()),
            ]
        );
    }

    #[test]
    fn oversized_line_spanning_many_chunks_is_reported() {
        // 远大于 limit, 迫使 read_until 多次循环
        let mut input = vec![b'x'; 100_000];
        input.push(b'\n');
        input.extend_from_slice(b"@ping\n");
        let got = read_all(&input, 1_024);
        assert_eq!(
            got[0],
            LineOutcome::TooLong {
                consumed_bytes: 100_001
            }
        );
        assert_eq!(got[1], LineOutcome::Line("@ping".into()));
    }

    #[test]
    fn line_at_exact_limit_is_accepted() {
        // 恰好等于上限不算超限 (边界: > limit 才算)
        let line = "x".repeat(10);
        let mut input = line.clone().into_bytes();
        input.push(b'\n');
        let got = read_all(&input, 10);
        assert_eq!(got, vec![LineOutcome::Line(line)]);
    }

    #[test]
    fn line_one_over_limit_is_rejected() {
        let line = "x".repeat(11);
        let mut input = line.into_bytes();
        input.push(b'\n');
        let got = read_all(&input, 10);
        assert_eq!(got, vec![LineOutcome::TooLong { consumed_bytes: 12 }]);
    }

    #[test]
    fn multibyte_utf8_line_is_preserved() {
        let got = read_all("点击 @click:{x:1}\n".as_bytes(), MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(got, vec![LineOutcome::Line("点击 @click:{x:1}".into())]);
    }

    #[test]
    fn invalid_utf8_is_reported_as_bad_line_without_desync() {
        // 无效 UTF-8 字节后跟正常行 —— 流必须仍然对齐
        let mut input = vec![0xff, 0xfe, b'\n'];
        input.extend_from_slice(b"@ping\n");
        let got = read_all(&input, MAX_CONTROL_MESSAGE_BYTES);
        assert!(matches!(got[0], LineOutcome::TooLong { .. }));
        assert_eq!(got[1], LineOutcome::Line("@ping".into()));
    }

    #[test]
    fn final_line_without_newline_is_returned() {
        let got = read_all(b"@ping", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(got, vec![LineOutcome::Line("@ping".into())]);
    }

    #[test]
    fn empty_lines_are_returned_as_empty_strings() {
        // 调用方负责跳过空行, 这里只保证不吞行
        let got = read_all(b"@a\n\n@b\n", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(
            got,
            vec![
                LineOutcome::Line("@a".into()),
                LineOutcome::Line("".into()),
                LineOutcome::Line("@b".into()),
            ]
        );
    }
}
