//! 独立的固定数据回显协议。

pub const MAX_CONTROL_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const ALPN: &[u8] = b"iroh-fixed-echo/1";

pub fn big_line(target_len: usize) -> String {
    "x".repeat(target_len - 1)
}
