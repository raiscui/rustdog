//! control_ax 模块的文本/按键输入 verb。
//!
//! 收纳 `@type-text` 与 `@key delivery` 的 daemon-side 默认执行路径。
//! 这两个 verb 共享 AX targeting 但走完全不同的执行栈:
//! - type-text: AXValue / targeted-keyboard / clipboard 三种模式
//! - key delivery: Global / WindowTargeted / PidTargeted 三种定向方式
//!
//! 状态变更但更接近 press 的 verb (press / focus / value / scroll / action)
//! 留在 control_ax.rs 或后续 commit 搬到 press.rs。

use crate::control_protocol::{KeyDelivery, KeyRequest};
use std::io;

use super::types::*;
use super::{
    platform_key_delivery, AxBackend,
};

// ---- perform_default_key_delivery (was lines 1076-1083) ----
pub fn perform_default_key_delivery(request: &KeyRequest) -> io::Result<Option<KeyDeliveryReport>> {
    match request.delivery {
        KeyDelivery::Global => Ok(None),
        KeyDelivery::PidTargeted | KeyDelivery::WindowTargeted => {
            platform_key_delivery(request).map(Some)
        }
    }
}

// ---- perform_default_type_text (was lines 1093-1095) ----
pub fn perform_default_type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    SystemAxBackend.type_text(request)
}

// ---- remap_type_text_ax_value_error (was lines 1681-1698) ----
pub(crate) fn remap_type_text_ax_value_error(err: io::Error) -> io::Error {
    let message = err.to_string();
    match err.kind() {
        io::ErrorKind::Unsupported => io::Error::new(
            io::ErrorKind::Unsupported,
            "type-text 当前只支持 macOS AXValue 路径",
        ),
        io::ErrorKind::InvalidInput => io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("type-text AXValue 路径失败: {message}"),
        ),
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("type-text AXValue 路径失败: {message}"),
        ),
        _ => io::Error::other(format!("type-text AXValue 路径失败: {message}")),
    }
}

// ---- remap_type_text_targeted_keyboard_error (was lines 1700-1717) ----
pub(crate) fn remap_type_text_targeted_keyboard_error(err: io::Error) -> io::Error {
    let message = err.to_string();
    match err.kind() {
        io::ErrorKind::Unsupported => io::Error::new(
            io::ErrorKind::Unsupported,
            "type-text 当前只支持 macOS targeted keyboard 路径",
        ),
        io::ErrorKind::InvalidInput => io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("type-text targeted keyboard 路径失败: {message}"),
        ),
        io::ErrorKind::PermissionDenied => io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("type-text targeted keyboard 路径失败: {message}"),
        ),
        _ => io::Error::other(format!("type-text targeted keyboard 路径失败: {message}")),
    }
}
