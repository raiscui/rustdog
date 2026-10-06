//! Windows DPI awareness — 坐标链路正确的地基
//!
//! # 为什么需要它
//!
//! Windows 对 **DPI-unaware 进程**有一整套虚拟化行为：`GetSystemMetrics`
//! 返回虚拟化后的逻辑值而非物理值，`GetDpiForMonitor` / xcap 的
//! `scale_factor()` 拿不到真实缩放（真机现场：daemon 日志反复出现
//! `get_hi_dpi_scale_factor failed: Process not DPI aware`）。
//!
//! 这直接导致三层连锁错误：
//! 1. rdog 对"这块屏多大"的认知是错的（4K@200% 被当成 1920×1080）
//! 2. enigo 按错误的 `main_display()` 尺寸做 0-65535 归一化，落点偏移
//! 3. 多屏 / 混合 DPI 下坐标完全不可表达
//!
//! 详见 `specs/windows-hidpi-coordinate-fix-plan.md`（根因 1 / 修复第 1 层）。
//!
//! # 为什么用 API 而不是 exe manifest
//!
//! API 调用与打包方式（electron-builder / cargo / 手工 zip）解耦，且
//! 容易在单测里验证调用时机；manifest 声明要求改构建链并为每种打包方式
//! 分别处理。2026-10-06 拍板选 API。
//!
//! # 调用时机契约
//!
//! 必须在**任何窗口创建之前**调用（Windows 规定进程 DPI awareness 只能
//! 设置一次，首次窗口创建后即锁定）。`main()` 最开头调用本函数，
//! 早于 `Opts::parse` 与日志初始化，覆盖所有子命令（含 `hidden-daemon --child`）。

/// 进程级启用 per-monitor v2 DPI awareness（仅 Windows 有效，其他平台 no-op）。
///
/// 幂等：重复调用无害（Windows 只接受第一次，后续调用失败被忽略）。
/// 失败不 panic —— awareness 设置失败只意味着坐标链路退回旧缺陷，
/// daemon 的其余功能不应因此拒绝启动；失败会被记录供真机排查。
#[cfg(windows)]
pub fn enable_dpi_awareness() {
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    // SAFETY: 该 API 只影响本进程的 DPI 虚拟化行为, 无指针参数, 无跨进程副作用。
    let ok = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    if ok == 0 {
        // 两种已知失败: (a) 已经设置过 (幂等重入); (b) API 不存在于过老系统。
        // (b) 时退回 legacy API — 它没有 per-monitor 粒度, 但仍能把
        // "system DPI unaware" 抬到 "system DPI aware", 多数场景已够。
        // SAFETY: 同上, legacy API 同样只影响本进程。
        let legacy_ok =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware() };
        if legacy_ok == 0 {
            log::warn!(
                "SetProcessDpiAwarenessContext 与 SetProcessDPIAware 均失败 — \
                 坐标链路将退回 DPI-unaware 缺陷 (截图缩放/点击落点可能偏移)"
            );
        } else {
            log::info!("DPI awareness: per-monitor v2 不可用, 已退回 system DPI aware");
        }
    }
}

/// 非 Windows 平台 no-op — macOS AppKit 的 backingScaleFactor 与 Linux X11/Wayland
/// 的坐标语义本就正确, 无需进程级声明 (今天 macOS 没出问题不是运气)。
#[cfg(not(windows))]
pub fn enable_dpi_awareness() {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::HiDpi::{
        GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
        DPI_AWARENESS_PER_MONITOR_AWARE,
    };

    /// 调用后进程 SHALL 处于 per-monitor v2 awareness。
    /// 判据: awareness context 经 `GetAwarenessFromDpiAwarenessContext`
    /// 解析成 `DPI_AWARENESS` 枚举后等于 `PER_MONITOR_AWARE`。
    ///
    /// 为什么不直接比句柄: `DPI_AWARENESS_CONTEXT` 是伪句柄, SDK 常量 (-4) 只是
    /// 文档值 —— 2026-10-06 真机探针实证 Windows 11 返回的运行时句柄是 0x12/0x22
    /// 一类内部值, 与 -4 的 `assert_eq!` 永远不成立 (微软文档同样明言伪句柄
    /// 不可直接比较)。解析成枚举后 unaware/system/per-monitor 三级判据稳定;
    /// V2 与 V1 在枚举层同为 per-monitor, 但本进程走的是
    /// `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` 调用面, 级别归调用面管。
    #[test]
    fn process_becomes_per_monitor_aware_v2() {
        enable_dpi_awareness();
        let context = unsafe { GetThreadDpiAwarenessContext() };
        assert!(!context.is_null(), "GetThreadDpiAwarenessContext 调用失败");
        let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(context) };
        assert_eq!(
            awareness, DPI_AWARENESS_PER_MONITOR_AWARE,
            "进程未处于 per-monitor awareness"
        );
    }
}
