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

/// 跨调用保留的读行状态。
///
/// 一条消息可能被对端拆成多次写, 而中间只要撞上一次读超时(`WouldBlock`),
/// `read_until` 就会带着已读到的字节返回 `Err`。半分好消息必须留在 `buf` 里交给
/// 下一次调用—— 入口清空就等于把消息丢了(RAI-415: `@pty-stdin` 帧被吞, 远端收不到 ^C)。
///
/// 超限行的丢弃也必须跨调用进行: `discarding` 标记与已丢弃字节数一起留在状态里,
/// 否则读超时会把「正在丢弃」忘掉, 把坏行的剩余部分当成一条新行收下。
#[derive(Debug, Default)]
pub struct LineReadState {
    buf: Vec<u8>,
    discarding: bool,
    discarded_bytes: usize,
}

impl LineReadState {
    pub fn new() -> Self {
        Self::default()
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.discarding = false;
        self.discarded_bytes = 0;
    }

    fn consumed(&self) -> usize {
        self.discarded_bytes + self.buf.len()
    }
}

/// 完整行的内容长度: 不含行尾 `\n`, 也不含 CRLF 里的 `\r`。
fn complete_content_len(buf: &[u8]) -> usize {
    let without_lf = buf.strip_suffix(b"\n").unwrap_or(buf);
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf).len()
}

/// 尚未读到行尾时的有效长度: 结尾那个 `\r` 可能正好是 CRLF 的前半截,
/// 不能算进内容长度 —— 否则「同一串字节一次性到达」与「被读超时切在 `\r` 之后」
/// 会给出不同判定。
fn pending_content_len(buf: &[u8]) -> usize {
    buf.strip_suffix(b"\r").unwrap_or(buf).len()
}

/// 从 `reader` 读一条以 `\n` 结尾的消息，行**内容**（不含行尾）超过
/// `limit` 字节时返回 [`LineOutcome::TooLong`]，并把该行剩余部分消费掉。
///
/// `state` 由调用方持有并在多次调用间复用: 半行、丢弃标记与已丢弃字节数都在里面,
/// 所以读超时不会丢数据, 也不会让超限行绕过上限。
pub fn read_line_limited<R: BufRead>(
    reader: &mut R,
    state: &mut LineReadState,
    limit: usize,
) -> io::Result<LineOutcome> {
    let mut scratch: Vec<u8> = Vec::new();

    loop {
        if state.discarding {
            scratch.clear();
            match reader.read_until(b'\n', &mut scratch) {
                Ok(0) => {
                    let consumed = state.consumed();
                    state.reset();
                    return Ok(LineOutcome::TooLong {
                        consumed_bytes: consumed,
                    });
                }
                Ok(n) => {
                    state.discarded_bytes += n;
                    if scratch.last() == Some(&b'\n') {
                        let consumed = state.consumed();
                        state.reset();
                        return Ok(LineOutcome::TooLong {
                            consumed_bytes: consumed,
                        });
                    }
                    continue;
                }
                // read_until 会带着已消费的字节返回错误, 这段也要计入丢弃量,
                // 否则跨读超时丢弃的字节数会从 consumed_bytes 里凭空消失。
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    state.discarded_bytes += scratch.len();
                    continue;
                }
                // 丢弃状态留在 state 里, 下一次调用接着丢
                Err(e) => {
                    state.discarded_bytes += scratch.len();
                    return Err(e);
                }
            }
        }

        match reader.read_until(b'\n', &mut state.buf) {
            Ok(0) => {
                if state.buf.is_empty() {
                    state.reset();
                    return Ok(LineOutcome::Eof);
                }
                // 对端关闭且末行没有换行: 仍然按内容长度判超限
                let consumed = state.consumed();
                let outcome = if complete_content_len(&state.buf) > limit
                    || pending_content_len(&state.buf) > limit
                {
                    LineOutcome::TooLong {
                        consumed_bytes: consumed,
                    }
                } else {
                    decode_line(&state.buf, consumed)
                };
                state.reset();
                return Ok(outcome);
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if pending_content_len(&state.buf) > limit {
                    state.discarded_bytes += state.buf.len();
                    state.buf.clear();
                    state.discarding = true;
                }
                return Err(e);
            }
        }

        if state.buf.last() == Some(&b'\n') {
            let consumed = state.consumed();
            let outcome = if complete_content_len(&state.buf) > limit {
                LineOutcome::TooLong {
                    consumed_bytes: consumed,
                }
            } else {
                decode_line(&state.buf, consumed)
            };
            state.reset();
            return Ok(outcome);
        }

        if pending_content_len(&state.buf) > limit {
            state.discarded_bytes += state.buf.len();
            state.buf.clear();
            state.discarding = true;
        }
    }
}

/// 去掉行尾 `\r` / `\n`（兼容 CRLF）后解码; 无效 UTF-8 当作坏行。
fn decode_line(buf: &[u8], consumed: usize) -> LineOutcome {
    let line = buf.strip_suffix(b"\n").unwrap_or(buf);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    match String::from_utf8(line.to_vec()) {
        Ok(text) => LineOutcome::Line(text),
        // 无效 UTF-8：当作坏行处理（流已对齐到行尾, 不影响后续消息）
        Err(_) => LineOutcome::TooLong {
            consumed_bytes: consumed,
        },
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Cursor;

    fn read_all(input: &[u8], limit: usize) -> Vec<LineOutcome> {
        let mut reader = Cursor::new(input.to_vec());
        let mut state = LineReadState::new();
        let mut out = Vec::new();
        loop {
            match read_line_limited(&mut reader, &mut state, limit).unwrap() {
                LineOutcome::Eof => return out,
                other => out.push(other),
            }
        }
    }

    /// 按脚本吐出数据块 / 读超时的假 reader: 复现「正文与换行分两次到达,
    /// 中间被读超时打断」的分片消息 (RAI-415)。
    struct ChunkedReader {
        steps: VecDeque<Result<Vec<u8>, io::ErrorKind>>,
    }

    impl ChunkedReader {
        fn new(steps: Vec<Result<Vec<u8>, io::ErrorKind>>) -> Self {
            Self {
                steps: VecDeque::from(steps),
            }
        }
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

    #[test]
    fn partial_line_survives_read_timeout_between_chunks() {
        let mut reader = std::io::BufReader::new(ChunkedReader::new(vec![
            Ok(b"@pty-stdin {\"session_id\":\"a\"".to_vec()),
            Err(io::ErrorKind::WouldBlock),
            Ok(b",\"data\":\"Aw==\"}\n".to_vec()),
        ]));
        let mut state = LineReadState::new();

        let err = read_line_limited(&mut reader, &mut state, MAX_CONTROL_MESSAGE_BYTES)
            .expect_err("首块没有换行, 应把读超时原样上抛");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);

        assert_eq!(
            read_line_limited(&mut reader, &mut state, MAX_CONTROL_MESSAGE_BYTES)
                .expect("超时后的续读应成功"),
            LineOutcome::Line("@pty-stdin {\"session_id\":\"a\",\"data\":\"Aw==\"}".to_owned()),
            "续读必须拼上后半截, 而不是只剩后半截"
        );
    }

    #[test]
    fn crlf_split_after_cr_is_not_rejected_at_the_limit() {
        // limit=10 时 "xxxxxxxxxx\r" 的内容长度正好是 10; 读超时把它切成两半,
        // 不能因为原始长度 11 就判成超限 (同一串字节一次到达是合法的 Line)。
        let mut reader = std::io::BufReader::new(ChunkedReader::new(vec![
            Ok(b"xxxxxxxxxx\r".to_vec()),
            Err(io::ErrorKind::WouldBlock),
            Ok(b"\n".to_vec()),
        ]));
        let mut state = LineReadState::new();
        let err = read_line_limited(&mut reader, &mut state, 10)
            .expect_err("首块没有换行, 应把读超时原样上抛");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);

        assert_eq!(
            read_line_limited(&mut reader, &mut state, 10).expect("续读应成功"),
            LineOutcome::Line("xxxxxxxxxx".to_owned()),
            "切在 CRLF 的 \\r 之后不能改变判定"
        );
    }

    #[test]
    fn retained_partial_line_cannot_grow_past_the_limit_across_timeouts() {
        // 每块都不带换行, 且中间都被读超时打断: 半行跨调用保留后必须仍然受 limit 约束,
        // 丢掉的字节数还要如实报出来 (consumed_bytes 是给用户/日志看的真实规模)。
        let limit = 1_024;
        let mut steps = Vec::new();
        for _ in 0..8 {
            steps.push(Ok(vec![b'a'; 512]));
            steps.push(Err(io::ErrorKind::WouldBlock));
        }
        steps.push(Ok(b"\n".to_vec()));
        let mut reader = std::io::BufReader::new(ChunkedReader::new(steps));
        let mut state = LineReadState::new();

        let mut consumed = None;
        for _ in 0..24 {
            match read_line_limited(&mut reader, &mut state, limit) {
                Ok(LineOutcome::TooLong { consumed_bytes }) => {
                    consumed = Some(consumed_bytes);
                    break;
                }
                Ok(other) => panic!("超限半行不应正常交出: {other:?}"),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => panic!("非预期错误: {err}"),
            }
        }
        assert_eq!(
            consumed.expect("累计超过 limit 的半行必须被判成 TooLong"),
            8 * 512 + 1,
            "consumed_bytes 必须覆盖整条坏行(含跨读超时丢弃的部分)"
        );
    }

    #[test]
    fn empty_input_is_eof() {
        assert!(read_all(b"", 1024).is_empty());
    }

    #[test]
    fn empty_lines_are_returned_as_empty_strings() {
        let got = read_all(b"\n\n", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(
            got,
            vec![
                LineOutcome::Line(String::new()),
                LineOutcome::Line(String::new()),
            ]
        );
    }

    #[test]
    fn final_line_without_newline_is_returned() {
        let got = read_all(b"@ping\n@exit", MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(
            got,
            vec![
                LineOutcome::Line("@ping".into()),
                LineOutcome::Line("@exit".into()),
            ]
        );
    }

    #[test]
    fn line_at_exact_limit_is_accepted() {
        let content = "a".repeat(1024);
        let got = read_all(format!("{content}\n").as_bytes(), 1024);
        assert_eq!(got, vec![LineOutcome::Line(content)]);
    }

    #[test]
    fn line_one_over_limit_is_rejected() {
        let content = "a".repeat(1025);
        let got = read_all(format!("{content}\n").as_bytes(), 1024);
        assert_eq!(
            got,
            vec![LineOutcome::TooLong {
                consumed_bytes: 1026
            }]
        );
    }

    #[test]
    fn oversized_line_is_reported_and_stream_stays_aligned() {
        let mut input = "a".repeat(100_000).into_bytes();
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
    fn oversized_line_spanning_many_chunks_is_reported() {
        let mut steps = Vec::new();
        for _ in 0..40 {
            steps.push(Ok(vec![b'z'; 512]));
        }
        steps.push(Ok(b"\n".to_vec()));
        let mut reader = std::io::BufReader::new(ChunkedReader::new(steps));
        let mut state = LineReadState::new();
        let outcome = read_line_limited(&mut reader, &mut state, 1_024).expect("应读到行尾");
        assert_eq!(
            outcome,
            LineOutcome::TooLong {
                consumed_bytes: 40 * 512 + 1
            }
        );
    }

    #[test]
    fn multibyte_utf8_line_is_preserved() {
        let got = read_all("@cmd:中文\r\n".as_bytes(), MAX_CONTROL_MESSAGE_BYTES);
        assert_eq!(got, vec![LineOutcome::Line("@cmd:中文".into())]);
    }

    #[test]
    fn invalid_utf8_is_reported_as_bad_line_without_desync() {
        let mut input = vec![0xff, 0xfe, 0xfd];
        input.push(b'\n');
        input.extend_from_slice(b"@ping\n");
        let got = read_all(&input, MAX_CONTROL_MESSAGE_BYTES);
        assert!(matches!(got[0], LineOutcome::TooLong { .. }));
        assert_eq!(got[1], LineOutcome::Line("@ping".into()));
    }
}
