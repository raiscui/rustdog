//! Windows 原生鼠标后端 — 绕开 enigo 的归一化与"只取主屏"限制 (坐标第 3 层)
//!
//! # 为什么不用 enigo (源码实证, 2026-10-06)
//!
//! `enigo-0.6.1/src/win/win_impl.rs:162-176` 的 Abs 移动:
//! 1. 取 `main_display()` (只取主屏) 尺寸做 0-65535 归一化
//! 2. 自己留着未做的 TODO: `MOUSEEVENTF_VIRTUALDESK` 未加 —
//!    坐标只在主屏范围有效, 副屏的点会落回主屏
//! 3. DPI-unaware 进程里尺寸是虚拟化逻辑值 (第 1 层已修, 但多屏/混合
//!    DPI 仍无法用单一分母表达)
//!
//! 单屏统一缩放时 rdog 传逻辑坐标、enigo 按逻辑尺寸归一化、Windows 按
//! 逻辑解释 — 误差恰好为零, 这正是 macOS 与早期 Windows 验证没暴露的原因。
//! 多屏 / 混合 DPI 下无解, 属 enigo 层面缺陷, rdog 无法通过配置绕过。
//!
//! # 方案
//!
//! `SetCursorPos` 收**物理绝对坐标** (per-monitor aware 进程内), 无归一化、
//! 无主屏假设; 按键/滚轮走 `SendInput` (不受鼠标加速曲线影响,
//! 旧 `mouse_event` 已被微软标记废弃)。
//!
//! 坐标换算: 协议坐标 (`os-logical`) → 物理由**命中屏自己的 scale** 完成:
//! `native = os_rect.origin + (logical - os_logical.origin) × scale`。
//! 纯函数 [`logical_point_to_native`] 可脱离 Win32 单测。

// 这三个 import 只被下面那个 `#[cfg(windows)] impl MouseBackend`
// (move_mouse / button / scroll) 用到, 跨平台单测一律不碰 (单测只调换算纯函数),
// 所以 gate 必须是纯 `windows` —— 加 `test` 会让它们在测试构建里白导入,
// 变成另一条 unused warning。
#[cfg(windows)]
use super::backend::MouseBackend;
#[cfg(any(windows, test))]
use crate::control_display_scope::DisplaySummary;
#[cfg(windows)]
use crate::screenshot::current_display_summaries;
#[cfg(windows)]
use enigo::{Axis, Button, Coordinate, Direction, InputError};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

/// Windows 原生鼠标后端。无状态 — 每次移动现查 display catalog 拿 scale,
/// 换取多屏/混合 DPI 的正确性 (catalog 枚举 ~毫秒级, 远小于点击链路的 40ms 收敛等待)。
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WinNativeMouseBackend;

#[cfg(windows)]
impl WinNativeMouseBackend {
    pub fn new() -> Self {
        Self
    }
}

/// 单屏的换算要素: 物理矩形 + 该屏 scale。
/// 逻辑矩形 = 物理 / scale (Windows DPI-aware 下 os_rect 恒为物理, 本模块
/// 只编译进 windows 目标, 因此这里直接用除法语义 — 与
/// screenshot::LogicalRect::to_os_logical 的非 mac 分支一致)。
///
/// 下面三个纯函数带 `any(windows, test)`: 线上唯一的调用点是下面那个
/// `#[cfg(windows)] impl` 的 `move_mouse`, 而它们的单测是跨平台跑的 (mac 上验
/// Windows 换算语义)。不 gate 的话, 非 Windows 的非测试构建会把这三样报成
/// "never used" —— 那是构建目标的差异, 不是换算没接上。
#[cfg(any(windows, test))]
struct ScreenMapping {
    os_x: i32,
    os_y: i32,
    logical_x: i32,
    logical_y: i32,
    logical_width: i32,
    logical_height: i32,
    scale: f64,
}

#[cfg(any(windows, test))]
fn screen_mapping_of(display: &DisplaySummary) -> ScreenMapping {
    let scale = if display.scale_factor.is_finite() && display.scale_factor > 0.0 {
        f64::from(display.scale_factor)
    } else {
        1.0
    };
    ScreenMapping {
        os_x: display.os_rect.x,
        os_y: display.os_rect.y,
        logical_x: (f64::from(display.os_rect.x) / scale).round() as i32,
        logical_y: (f64::from(display.os_rect.y) / scale).round() as i32,
        logical_width: (f64::from(display.os_rect.width) / scale).round() as i32,
        logical_height: (f64::from(display.os_rect.height) / scale).round() as i32,
        scale,
    }
}

/// 逻辑坐标点 → OS 原生坐标点 (物理)。
///
/// 找到逻辑点所在的屏 (按该屏逻辑矩形包含判断), 用**该屏自己的** scale 做
/// 屏内换算并加上屏的物理偏移 — 混合 DPI 多屏下每屏 scale 不同, 不能用
/// 统一系统值。点落在所有屏之外时按主屏 scale 线性外推 (与旧行为兼容:
/// 至少有确定性的落点而不是报错中断整个点击链)。
///
/// 接线核对 (2026-10-06): 唯一调用点是本文件 `#[cfg(windows)] impl
/// MouseBackend::move_mouse` 的 `Coordinate::Abs` 分支, 而该 backend 由
/// `control_actions::execute_mouse_plan` 的 `#[cfg(windows)]` 分支实例化 ——
/// Windows 上 `@click` 的 os-logical → 物理换算是接上的, 别被非 Windows 构建
/// 上的 "never used" 警告误导成缺实现。
#[cfg(any(windows, test))]
fn logical_point_to_native(x: i32, y: i32, displays: &[DisplaySummary]) -> (i32, i32) {
    let mut fallback_scale = 1.0_f64;
    for display in displays {
        let mapping = screen_mapping_of(display);
        let inside = x >= mapping.logical_x
            && y >= mapping.logical_y
            && x < mapping.logical_x + mapping.logical_width
            && y < mapping.logical_y + mapping.logical_height;
        if inside {
            let native_x =
                mapping.os_x + ((f64::from(x - mapping.logical_x)) * mapping.scale).round() as i32;
            let native_y =
                mapping.os_y + ((f64::from(y - mapping.logical_y)) * mapping.scale).round() as i32;
            return (native_x, native_y);
        }
        if display.primary {
            fallback_scale = mapping.scale;
        }
    }
    // 屏外: 主屏 scale 外推 (确定性落点, 不中断点击链)
    (
        (f64::from(x) * fallback_scale).round() as i32,
        (f64::from(y) * fallback_scale).round() as i32,
    )
}

#[cfg(windows)]
impl MouseBackend for WinNativeMouseBackend {
    fn move_mouse(&mut self, x: i32, y: i32, coordinate: Coordinate) -> Result<(), InputError> {
        match coordinate {
            Coordinate::Abs => {
                let displays = current_display_summaries()
                    .map_err(|_| InputError::Simulate("display catalog unavailable"))?;
                let (native_x, native_y) = logical_point_to_native(x, y, &displays);
                log::debug!("win mouse: logical ({x},{y}) → native ({native_x},{native_y})");
                // SAFETY: SetCursorPos 只移动本会话的光标, 无指针参数。
                if unsafe { SetCursorPos(native_x, native_y) } == 0 {
                    log::warn!("SetCursorPos({native_x},{native_y}) 失败");
                    return Err(InputError::Simulate("SetCursorPos failed"));
                }
                Ok(())
            }
            // 协议当前只产生 Abs (coordinate_space 只支持 os-logical 绝对坐标)。
            // 相对移动仍走 SendInput 相对量保底, 不走 enigo (其相对路径受
            // 鼠标加速曲线影响)。
            Coordinate::Rel => send_mouse_move_relative(x, y),
        }
    }

    fn button(&mut self, button: Button, direction: Direction) -> Result<(), InputError> {
        let down_flag = match button {
            Button::Left => MOUSEEVENTF_LEFTDOWN,
            Button::Middle => MOUSEEVENTF_MIDDLEDOWN,
            Button::Right => MOUSEEVENTF_RIGHTDOWN,
            _ => return Err(InputError::InvalidInput("unsupported mouse button")),
        };
        let up_flag = match button {
            Button::Left => MOUSEEVENTF_LEFTUP,
            Button::Middle => MOUSEEVENTF_MIDDLEUP,
            Button::Right => MOUSEEVENTF_RIGHTUP,
            _ => return Err(InputError::InvalidInput("unsupported mouse button")),
        };
        let flag = match direction {
            Direction::Click | Direction::Press => down_flag,
            Direction::Release => up_flag,
        };
        send_mouse_input(flag, 0, 0, 0)
    }

    fn scroll(&mut self, length: i32, axis: Axis) -> Result<(), InputError> {
        match axis {
            Axis::Vertical => send_mouse_input(MOUSEEVENTF_WHEEL, length * 120, 0, 0),
            Axis::Horizontal => send_mouse_input(MOUSEEVENTF_HWHEEL, length * 120, 0, 0),
        }
    }
}

#[cfg(windows)]
/// 相对移动: SendInput 的相对量不受鼠标速度/加速设置影响 (微软文档明示),
/// 这是换成 SendInput 的额外正确性红利。
fn send_mouse_move_relative(dx: i32, dy: i32) -> Result<(), InputError> {
    send_mouse_input(MOUSEEVENTF_MOVE, 0, dx, dy)
}

#[cfg(windows)]
fn send_mouse_input(flags: u32, wheel_delta: i32, dx: i32, dy: i32) -> Result<(), InputError> {
    let mouse_input = MOUSEINPUT {
        dx,
        dy,
        mouseData: wheel_delta as u32,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    let mut input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            mi: mouse_input,
        },
    };
    // SAFETY: input 是合法的单一 MOUSEEVENT, SendInput 只注入输入事件。
    let sent = unsafe { SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        log::warn!("SendInput(flags={flags:#x}) 送入 {sent} 个事件");
        return Err(InputError::Simulate("SendInput failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 注: 用例按 Windows 物理坐标语义构造 (本模块只编译进 win 二进制的
    // Win32 调用层); 换算纯函数本身全平台编译, mac 上同样可跑可验。
    use super::*;
    use crate::control_display_scope::DisplayRect;

    fn display(id: &str, primary: bool, os: (i32, i32, u32, u32), scale: f32) -> DisplaySummary {
        DisplaySummary {
            display_id: id.to_string(),
            stable_key: None,
            primary,
            name: id.to_string(),
            os_rect: DisplayRect {
                x: os.0,
                y: os.1,
                width: os.2,
                height: os.3,
            },
            image_rect: DisplayRect {
                x: os.0,
                y: os.1,
                width: os.2,
                height: os.3,
            },
            scale_factor: scale,
            rotation: 0.0,
            display_id_stability: "session",
        }
    }

    /// 4K@200% 单屏: 逻辑 (1920,1080) 中点 → 物理 (3840,2160) 中点。
    #[test]
    fn logical_midpoint_maps_to_native_midpoint_on_200pct() {
        let displays = vec![display("1", true, (0, 0, 3840, 2160), 2.0)];
        let (x, y) = logical_point_to_native(1920, 1080, &displays);
        assert_eq!((x, y), (3840, 2160));
    }

    /// 混合 DPI 的"逻辑空间空洞": 每屏独立 96-DPI 基准意味着主屏逻辑右缘
    /// (3840/2=1920) ≠ 副屏逻辑左缘 (3840/1=3840) — 中间 [1920,3840) 是洞。
    /// 落洞里的点既不在任何屏内 → 主屏 scale 外推 (4000,200)。这是契约的
    /// 确定性行为, 不是缺陷; 真实点击点来自截图换算, 不会落在洞里。
    #[test]
    fn mixed_dpi_gap_point_falls_back_to_primary_scale() {
        let displays = vec![
            display("1", true, (0, 0, 3840, 2160), 2.0),
            display("2", false, (3840, 0, 1920, 1080), 1.0),
        ];
        let (x, y) = logical_point_to_native(2000, 100, &displays);
        assert_eq!((x, y), (4000, 200));
    }

    /// 副屏 1.0x 内的点: scale=1 时逻辑=物理, 恒等映射。
    #[test]
    fn secondary_screen_scale_one_is_identity() {
        let displays = vec![
            display("1", true, (0, 0, 3840, 2160), 2.0),
            display("2", false, (3840, 0, 1920, 1080), 1.0),
        ];
        // 副屏逻辑 x ∈ [3840, 5760): 逻辑 (4500,100) → 物理 (4500,100)
        let (x, y) = logical_point_to_native(4500, 100, &displays);
        assert_eq!((x, y), (4500, 100));
    }

    /// 副屏也是 HiDPI (1.5x) 时用副屏自己的 scale, 不用主屏的 2.0。
    /// 副屏物理 (3840,0,2880,1620) @1.5 → 逻辑起点 3840/1.5=2560, 逻辑宽 1920。
    /// 逻辑 (3520, 0) = 副屏逻辑横向中点 → 物理 3840 + 960×1.5 = 5280。
    #[test]
    fn secondary_screen_with_its_own_hidpi_scale() {
        let displays = vec![
            display("1", true, (0, 0, 3840, 2160), 2.0),
            display("2", false, (3840, 0, 2880, 1620), 1.5),
        ];
        let (x, _) = logical_point_to_native(3520, 0, &displays);
        assert_eq!(x, 5280);
    }

    /// 屏外点: 按主屏 scale 外推, 有确定性落点而不是报错。
    #[test]
    fn offscreen_point_falls_back_to_primary_scale() {
        let displays = vec![display("1", true, (0, 0, 3840, 2160), 2.0)];
        let (x, y) = logical_point_to_native(-100, 50, &displays);
        assert_eq!((x, y), (-200, 100));
    }

    /// 空屏列表: 不 panic, 按恒等返回 (catalog 异常时退化为旧行为)。
    #[test]
    fn empty_display_list_degrades_to_identity() {
        let (x, y) = logical_point_to_native(123, 456, &[]);
        assert_eq!((x, y), (123, 456));
    }

    /// 非法 scale (xcap 失败回填 0/负) 按 1.0 兜底, 防除零。
    #[test]
    fn invalid_scale_falls_back_to_identity() {
        let displays = vec![display("1", true, (0, 0, 1920, 1080), 0.0)];
        let (x, y) = logical_point_to_native(123, 456, &displays);
        assert_eq!((x, y), (123, 456));
    }
}
