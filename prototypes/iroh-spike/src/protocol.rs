//! rdog line 协议的最小复刻 —— **协议字节级不改**,只取 rdog 已有的约束。
//!
//! 引用:`specs/control-line-protocol.md`;行上限与 rdog 保持一致
//! (`src/control_line_read.rs:30` `MAX_CONTROL_MESSAGE_BYTES = 8 * 1024 * 1024`)。

/// 单条控制消息上限,与 rdog `MAX_CONTROL_MESSAGE_BYTES` 一致(8 MiB)。
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// iroh 1.x 的 `Endpoint::connect` 以 ALPN 做协议协商,两端必须一致。
pub const ALPN: &[u8] = b"rdog/1";

/// 请求行编码:一行文本 + `\n`。
pub fn encode_line(line: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    buf
}

/// 构造一条 N 字节的请求行,用于压测行上限。
///
/// 形如 `@cmd#1:<padding>`,总长精确等于 `target_len`(含结尾 `\n`)。
pub fn big_line(target_len: usize) -> String {
    assert!(target_len > 16);
    let head = "@cmd#1:";
    let pad = target_len - head.len() - 1; // 留 1 给结尾 \n
    let mut s = String::with_capacity(target_len);
    s.push_str(head);
    s.extend(std::iter::repeat_n('x', pad));
    s
}
