//! control_ax 模块的 AX 树/捕获相关函数。
//!
//! 当前装的是最小子集:独立、不依赖内部 helper 的 capture / platform 函数。
//! 后续 commit 会逐步把 capture_ax_find_snapshot、resolve_target_id_in_snapshot、
//! 以及 AX snapshot selector helpers 搬过来。

use crate::control_ax::types::{AxCapturedSubtree, AxSnapshot, AxTreeRequest};
use std::io;

use super::types::*;
use super::AxBackend;

/// 当前 daemon 进程对应的 AX 后端平台标识。
///
/// 只用于协议层 reporting,不参与 dispatch 决策。
pub fn current_ax_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unsupported"
    }
}

/// 默认全屏 AX 树捕获入口。
///
/// 调用 `SystemAxBackend::snapshot`,后者由 `macos.rs` / 其他 platform
/// 适配器实现。capture 后的 snapshot 由 caller 通过 `with_observation`
/// (在 control_ax.rs 的 `impl AxSnapshot` 块里) 关联 observation。
pub fn capture_default_ax_snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    SystemAxBackend.snapshot(request)
}

/// 捕获当前 target_id 对应的子树。
///
/// `platform_capture_current_subtree` 留在 control_ax.rs 作为 platform
/// 抽象层(由 `macos.rs` 的 `#[cfg]` 分支具体实现)。
pub fn capture_current_ax_subtree(
    target_id: &str,
    request: &AxTreeRequest,
) -> io::Result<AxCapturedSubtree> {
    super::platform_capture_current_subtree(target_id, request)
}
