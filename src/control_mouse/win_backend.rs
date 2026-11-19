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
//! 绝对移动复用现有物理落点转换, 再按整个虚拟桌面编码为 `SendInput` 绝对量.
//! 所有鼠标注入都携带同一个 recorder source marker, 不再让 `SetCursorPos`
//! 绕开自注入过滤. 多屏落点和实际 hook 来源仍需 Windows 真机验证.
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
// `resolve_abs_move` (跨平台可测) 要构造 AppliedPoint, 所以这两个类型跟它
// 一起 gate 到 any(windows, test)。
#[cfg(any(windows, test))]
use super::backend::{AppliedPoint, AppliedPointSpace};
#[cfg(any(windows, test))]
use crate::control_display_scope::DisplaySummary;
#[cfg(any(windows, test))]
use crate::control_mouse::request::MousePoint;
#[cfg(windows)]
use crate::screenshot::current_display_summaries;
#[cfg(windows)]
use enigo::{Axis, Button, Coordinate, Direction, InputError};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT,
};

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

/// 统一计算后端的目标物理落点与 `resolved_os_point`, 不将解析结果当作已观察位置.
///
/// 该转换不负责 Win32 的归一化编码. 编码只在实际注入边界执行.
///
/// 带 `any(windows, test)`: 与 [`logical_point_to_native`] 同理, 线上调用点
/// 只有下面的 `#[cfg(windows)] impl`, 但这条语义断言要能在 mac 上验 Windows。
#[cfg(any(windows, test))]
fn resolve_abs_move(x: i32, y: i32, displays: &[DisplaySummary]) -> AppliedPoint {
    let (native_x, native_y) = logical_point_to_native(x, y, displays);
    AppliedPoint {
        point: MousePoint {
            x: native_x,
            y: native_y,
        },
        space: AppliedPointSpace::Physical,
    }
}

#[cfg(any(windows, test))]
struct AbsoluteInputPoint {
    dx: i32,
    dy: i32,
}

#[cfg(any(windows, test))]
fn absolute_input_point(
    point: MousePoint,
    displays: &[DisplaySummary],
) -> Option<AbsoluteInputPoint> {
    let mut bounds = None;
    for display in displays {
        let rect = &display.os_rect;
        let (left, top) = (i64::from(rect.x), i64::from(rect.y));
        let (right, bottom) = (left + i64::from(rect.width), top + i64::from(rect.height));
        if right <= left || bottom <= top {
            return None;
        }
        bounds = Some(match bounds {
            None => (left, top, right, bottom),
            Some((x0, y0, x1, y1)) => (left.min(x0), top.min(y0), right.max(x1), bottom.max(y1)),
        });
    }
    let (left, top, right, bottom) = bounds?;
    let (width, height) = (right - left, bottom - top);
    let (x, y) = (i64::from(point.x), i64::from(point.y));
    if !(left..right).contains(&x) || !(top..bottom).contains(&y) || width > 65536 || height > 65536
    {
        return None;
    }
    // 选取目标像素归一化区间内的整数, 超宽桌面也不能向下落到前一个像素.
    Some(AbsoluteInputPoint {
        dx: (((x - left) * 65536 + width - 1) / width) as i32,
        dy: (((y - top) * 65536 + height - 1) / height) as i32,
    })
}

#[cfg(windows)]
impl MouseBackend for WinNativeMouseBackend {
    fn move_mouse(
        &mut self,
        x: i32,
        y: i32,
        coordinate: Coordinate,
    ) -> Result<Option<AppliedPoint>, InputError> {
        match coordinate {
            Coordinate::Abs => {
                let displays = current_display_summaries()
                    .map_err(|_| InputError::Simulate("display catalog unavailable"))?;
                let applied = resolve_abs_move(x, y, &displays);
                let native_x = applied.point.x;
                let native_y = applied.point.y;
                log::debug!("win mouse: logical ({x},{y}) → native ({native_x},{native_y})");
                let input = absolute_input_point(applied.point, &displays).ok_or(
                    InputError::InvalidInput("absolute mouse point cannot be encoded"),
                )?;
                send_mouse_input(
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    0,
                    input.dx,
                    input.dy,
                )?;
                Ok(Some(applied))
            }
            // 协议当前只产生 Abs (coordinate_space 只支持 os-logical 绝对坐标)。
            // 相对移动仍走 SendInput 相对量保底, 不走 enigo (其相对路径受
            // 鼠标加速曲线影响)。相对量没有绝对落点, 如实回 None 而不猜。
            Coordinate::Rel => send_mouse_move_relative(x, y).map(|()| None::<AppliedPoint>),
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
        dwExtraInfo: crate::control_recording::SELF_EVENT_MARKER as usize,
    };
    let mut input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 { mi: mouse_input },
    };
    // SAFETY: input 是合法的单一 MOUSEEVENT, SendInput 只注入输入事件。
    let sent = unsafe { SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        log::warn!("SendInput(flags={flags:#x}) 送入 {sent} 个事件");
        return Err(InputError::Simulate("SendInput failed"));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn execute_guarded(
    step: &crate::control_flow::FlowGuardedMouseStep,
    timeout: std::time::Duration,
) -> std::io::Result<()> {
    use crate::control_flow::{FlowMouseAction, FlowMousePoint, FlowMouseVerification};
    use std::{io, time::{Duration, Instant}};
    use windows_sys::Win32::{
        Foundation::{POINT, HWND},
        UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GetPhysicalCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetScrollInfo, GetWindowThreadProcessId, IsChild, IsWindow, SendMessageTimeoutW, WindowFromPoint, GA_ROOT, GUITHREADINFO, SCROLLINFO, SB_HORZ, SB_VERT, SIF_POS, SIF_RANGE, SMTO_ABORTIFHUNG},
    };

    fn rejected() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "鼠标动作未派发") }
    fn unknown() -> io::Error { io::Error::other("鼠标动作结果未知") }

    let root = usize::try_from(step.root_hwnd).map_err(|_| rejected())? as HWND;
    let control_id = match &step.verify {
        FlowMouseVerification::Position { control_hwnd }
        | FlowMouseVerification::Focus { control_hwnd }
        | FlowMouseVerification::Scroll { control_hwnd, .. }
        | FlowMouseVerification::Selection { control_hwnd, .. } => *control_hwnd,
    };
    let control = usize::try_from(control_id).map_err(|_| rejected())? as HWND;
    let deadline = Instant::now() + timeout;
    let guard = |point: &FlowMousePoint, require_cursor: bool| -> io::Result<()> {
        crate::control_recording::desktop::check_interactive_desktop(step.pid).map_err(|_| rejected())?;
        if Instant::now() >= deadline { return Err(rejected()); }
        let displays = current_display_summaries().map_err(|_| rejected())?;
        let hits = displays.iter().filter(|display| {
            let m = screen_mapping_of(display);
            i64::from(point.x) >= i64::from(m.logical_x)
                && i64::from(point.y) >= i64::from(m.logical_y)
                && i64::from(point.x) < i64::from(m.logical_x) + i64::from(m.logical_width)
                && i64::from(point.y) < i64::from(m.logical_y) + i64::from(m.logical_height)
        }).count();
        if hits != 1 { return Err(rejected()); }
        let (x, y) = logical_point_to_native(point.x, point.y, &displays);
        let rect = crate::control_window::current_logical_window_rect(root).ok_or_else(rejected)?;
        let mut pid = 0;
        let mut cursor = POINT::default();
        let at = POINT { x, y };
        // SAFETY: 句柄仅用作系统查询, 所有输出指针均有效.
        let valid = unsafe {
            let hit = WindowFromPoint(at);
            IsWindow(root) != 0 && IsWindow(control) != 0 && GetForegroundWindow() == root
                && GetWindowThreadProcessId(root, &mut pid) != 0
                && GetAncestor(hit, GA_ROOT) == root && (hit == control || IsChild(control, hit) != 0)
                && (!require_cursor || (GetPhysicalCursorPos(&mut cursor) != 0 && cursor.x == x && cursor.y == y))
        };
        if !valid || pid != step.pid || rect.x != step.rect.x || rect.y != step.rect.y
            || rect.width != step.rect.width || rect.height != step.rect.height
        { return Err(rejected()); }
        Ok(())
    };
    let wait = |duration: Duration| -> io::Result<()> {
        if Instant::now().checked_add(duration).is_none_or(|end| end >= deadline) { return Err(unknown()); }
        std::thread::sleep(duration);
        Ok(())
    };
    if timeout <= Duration::from_millis(40) { return Err(rejected()); }
    let mut backend = WinNativeMouseBackend::new();
    let mut pressed = None;
    let first = match &step.action {
        FlowMouseAction::Move { point } | FlowMouseAction::Click { point, .. }
        | FlowMouseAction::Wheel { point, .. } => point,
        FlowMouseAction::Drag { points, .. } => &points[0],
    };
    guard(first, false)?;
    let observe = || -> io::Result<i64> {
        crate::control_recording::desktop::check_interactive_desktop(step.pid).map_err(|_| rejected())?;
        let mut pid = 0;
        // SAFETY: 系统查询使用有效输出指针, 目标归属在每次读取前核对.
        let thread_id = unsafe { GetWindowThreadProcessId(control, &mut pid) };
        if thread_id == 0 || pid != step.pid || unsafe { IsWindow(control) == 0 || (control != root && IsChild(root, control) == 0) || GetForegroundWindow() != root } {
            return Err(rejected());
        }
        match &step.verify {
            FlowMouseVerification::Position { .. } => Ok(1),
            FlowMouseVerification::Focus { .. } => {
                let mut info = GUITHREADINFO::default();
                info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
                // SAFETY: info 是初始化后的 GUI 线程状态输出.
                if unsafe { GetGUIThreadInfo(thread_id, &mut info) } == 0 { return Err(rejected()); }
                Ok(i64::from(info.hwndFocus == control))
            }
            FlowMouseVerification::Scroll { vertical, .. } => {
                let mut info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as u32, fMask: SIF_POS | SIF_RANGE, ..Default::default() };
                // SAFETY: info 的大小和掩码符合 GetScrollInfo 契约.
                if unsafe { GetScrollInfo(control, if *vertical { SB_VERT } else { SB_HORZ }, &mut info) } == 0
                    || info.nMax <= info.nMin { return Err(rejected()); }
                Ok(i64::from(info.nPos))
            }
            FlowMouseVerification::Selection { .. } => {
                let mut class = [0u16; 128];
                // SAFETY: class 是固定大小的可写 UTF-16 缓冲区.
                let count = unsafe { GetClassNameW(control, class.as_mut_ptr(), class.len() as i32) };
                if count <= 0 { return Err(rejected()); }
                let name = String::from_utf16_lossy(&class[..count as usize]).to_ascii_lowercase();
                if name != "edit" && !name.starts_with("richedit") { return Err(rejected()); }
                let remaining = deadline.saturating_duration_since(Instant::now()).as_millis().min(250) as u32;
                if remaining == 0 { return Err(rejected()); }
                let mut result = 0usize;
                // SAFETY: EM_GETSEL 无指针参数, SendMessageTimeoutW 的结果指针有效.
                if unsafe { SendMessageTimeoutW(control, 0x00b0, 0, 0, SMTO_ABORTIFHUNG, remaining, &mut result) } == 0
                    || result as u32 == u32::MAX { return Err(rejected()); }
                Ok(i64::from(result as u32))
            }
        }
    };
    let before = observe()?;
    if !super::guarded::precondition(&step.verify, before) { return Err(rejected()); }
    let duration_ns = match &step.action {
        FlowMouseAction::Click { hold_ns, .. } => *hold_ns,
        FlowMouseAction::Drag { points, .. } => points.last().ok_or_else(rejected)?.offset_ns,
        _ => 0,
    };
    if Instant::now().checked_add(Duration::from_nanos(duration_ns) + Duration::from_millis(40))
        .is_none_or(|end| end >= deadline) { return Err(rejected()); }
    if let FlowMouseAction::Drag { points, .. } = &step.action {
        for point in points { guard(point, false)?; }
    }
    let run = (|| -> io::Result<()> {
        backend.move_mouse(first.x, first.y, Coordinate::Abs).map_err(|_| unknown())?;
        wait(Duration::from_millis(40))?;
        match &step.action {
            FlowMouseAction::Move { .. } => guard(first, true)?,
            FlowMouseAction::Click { point, button, hold_ns } => {
                let button = match button { 1 => Button::Left, 2 => Button::Right, 3 => Button::Middle, _ => return Err(unknown()) };
                guard(point, true)?;
                backend.button(button, Direction::Press).map_err(|_| unknown())?;
                pressed = Some(button);
                wait(Duration::from_nanos(*hold_ns))?;
                guard(point, true)?;
                backend.button(button, Direction::Release).map_err(|_| unknown())?;
                pressed = None;
            }
            FlowMouseAction::Drag { points, button } => {
                let button = match button { 1 => Button::Left, 2 => Button::Right, 3 => Button::Middle, _ => return Err(unknown()) };
                guard(first, true)?;
                backend.button(button, Direction::Press).map_err(|_| unknown())?;
                pressed = Some(button);
                let started = Instant::now();
                for point in &points[1..] {
                    let due = started + Duration::from_nanos(point.offset_ns);
                    if let Some(remaining) = due.checked_duration_since(Instant::now()) { wait(remaining)?; }
                    guard(point, false)?;
                    backend.move_mouse(point.x, point.y, Coordinate::Abs).map_err(|_| unknown())?;
                }
                guard(points.last().ok_or_else(unknown)?, true)?;
                backend.button(button, Direction::Release).map_err(|_| unknown())?;
                pressed = None;
            }
            FlowMouseAction::Wheel { point, delta_x, delta_y } => {
                for (axis, delta) in super::guarded::raw_wheel_events(*delta_x, *delta_y) {
                    if delta == 0 { continue; }
                    guard(point, true)?;
                    let flag = match axis { Axis::Horizontal => MOUSEEVENTF_HWHEEL, Axis::Vertical => MOUSEEVENTF_WHEEL };
                    send_mouse_input(flag, delta, 0, 0).map_err(|_| unknown())?;
                }
            }
        }
        Ok(())
    })();
    if let Some(button) = pressed { let _ = backend.button(button, Direction::Release); }
    run.map_err(|_| unknown())?;
    loop {
        let after = observe().map_err(|_| unknown())?;
        if matches!(step.verify, FlowMouseVerification::Position { .. }) {
            guard(first, true).map_err(|_| unknown())?;
        }
        let passed = super::guarded::postcondition(&step.verify, before, after);
        if passed { return Ok(()); }
        if Instant::now().checked_add(Duration::from_millis(20)).is_none_or(|end| end >= deadline) { return Err(unknown()); }
        std::thread::sleep(Duration::from_millis(20));
    }
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

    /// 回执语义锁 (2026-10-06): Windows 后端下回执报的必须是**换算后的物理坐标**,
    /// 且空间标签为 physical。
    ///
    /// 这条不是重复上面的纯换算用例 —— 它断言的是**接线**: `resolve_abs_move`
    /// 同时产出 `SetCursorPos` 收到的数字与回执 `resolved_os_point` 的数字。
    /// 换成直接调 `logical_point_to_native` 的旧写法, 本用例就测不到"回执有没有
    /// 跟着换算走"这件事 (它只会证明算术对)。取样点是 2026-10-06 那台真机
    /// (4K@200%) 的实际点击坐标。
    #[test]
    fn abs_move_reports_the_converted_physical_point() {
        let displays = vec![display("1", true, (0, 0, 3840, 2160), 2.0)];
        let applied = resolve_abs_move(605, 1056, &displays);
        assert_eq!(applied.point, MousePoint { x: 1210, y: 2112 });
        assert_eq!(applied.space, AppliedPointSpace::Physical);
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

    #[test]
    fn absolute_input_preserves_native_pixels_across_negative_and_mixed_dpi_displays() {
        let displays = vec![
            display("left", false, (-1920, 0, 1920, 1080), 1.0),
            display("main", true, (0, 0, 3840, 2160), 2.0),
        ];
        for point in [
            MousePoint { x: -1920, y: 0 },
            MousePoint { x: -1, y: 1079 },
            resolve_abs_move(480, 270, &displays).point,
            MousePoint { x: 3839, y: 2159 },
        ] {
            let input = absolute_input_point(point, &displays).expect("有效虚拟桌面像素必须可编码");
            let decoded_x = i64::from(input.dx) * 5760 / 65536 - 1920;
            let decoded_y = i64::from(input.dy) * 2160 / 65536;
            assert_eq!(
                (decoded_x, decoded_y),
                (i64::from(point.x), i64::from(point.y))
            );
        }
    }

    #[test]
    fn absolute_input_rejects_missing_or_unrepresentable_desktop() {
        let point = MousePoint { x: 0, y: 0 };
        assert!(absolute_input_point(point, &[]).is_none());
        assert!(
            absolute_input_point(point, &[display("invalid", true, (0, 0, 0, 1080), 1.0)])
                .is_none()
        );
        assert!(absolute_input_point(
            point,
            &[display("too-wide", true, (0, 0, 65537, 1080), 1.0)]
        )
        .is_none());
        assert!(absolute_input_point(
            MousePoint { x: -1, y: 0 },
            &[display("main", true, (0, 0, 1920, 1080), 1.0)]
        )
        .is_none());
    }

    #[test]
    fn absolute_input_preserves_every_pixel_at_large_supported_axis_lengths() {
        for width in [1_u32, 1920, 38400, 65536] {
            let displays = [display("wide", true, (-15360, 0, width, 1), 1.0)];
            for offset in 0..width {
                let point = MousePoint {
                    x: -15360 + offset as i32,
                    y: 0,
                };
                let encoded = absolute_input_point(point, &displays).unwrap();
                let decoded = i64::from(encoded.dx) * i64::from(width) / 65536 - 15360;
                assert_eq!(
                    decoded,
                    i64::from(point.x),
                    "width={width}, offset={offset}"
                );
            }
        }
    }
}
