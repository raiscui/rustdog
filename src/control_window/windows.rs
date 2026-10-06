//! Windows 窗口身份面 backend —— EnumWindows 家族, **不碰** UI Automation。
//!
//! # 为什么窗口身份 ≠ 可访问性树 (两者不要互相暗示等价)
//!
//! "这个窗口是谁、在哪、能不能点到" 只要 user32 的窗口表就能回答:
//! `EnumWindows` / `GetWindowTextW` / `GetClassNameW` / `GetWindowRect` /
//! `IsWindowVisible`。而 AX tree (控件、role、name、value) 在 Windows 上是
//! UI Automation, 每层遍历都是跨进程 COM 调用, 成本与失败面高一个量级,
//! 且和窗口身份不是同一份真相。所以本模块只做窗口身份 verb:
//! `find` / `activate` / `close` / `resolve_target_rect`;
//! `@ax-*` 在 Windows 上继续 unsupported, 本模块不构成对它的任何承诺。
//!
//! # 两个坐标系必须分开 (Windows 上它们不相等)
//!
//! daemon 进程是 per-monitor v2 DPI aware (见 `win_dpi::enable_dpi_awareness`),
//! 因此:
//! - `GetWindowRect` 给的是**物理像素**;
//! - 但 `WINDOW_COORDINATE_SPACE` 声明的 `os-logical` 在 Windows 上是
//!   **物理 / 该窗所在屏 scale** (96-DPI 基准, 与 `screenshot.rs` 里
//!   `LogicalRect::to_os_logical` 的 Windows 分支同一规则);
//! - 而 `DisplaySummary.os_rect` 在 Windows 上是**物理像素** (macOS 上恰好
//!   与逻辑坐标同值, 掩盖了这个差异)。
//!
//! 所以每个窗口都留两份矩形: `native_rect` 喂 display scope / guard 这类与
//! display catalog 的比较, `rect` 喂对外的 `coordinate_space: "os-logical"`
//! 契约。只回一份就必然说谎一份 —— 要么 agent 按窗口 rect 换算点击坐标翻倍
//! (4K@200% 正好 2 倍), 要么 display guard 在非 100% 缩放的屏上误判。
//!
//! # `app` 字段在 Windows 上是**窗口类名**, 不是进程镜像名
//!
//! macOS 的 app 来自 Launch Services (pid → 应用)。Windows 侧进程镜像名要
//! `OpenProcess` + `QueryFullProcessImageNameW` (`Win32_System_Threading`
//! feature, 当前未启用), 因此 `app.name` 取 `GetClassNameW` 的类名
//! (如 `Chrome_WidgetWin_1` / `CabinetWClass` / `ApplicationFrameWindow`)。
//! **pid 才是 Windows 上的权威身份**, 按应用找窗口请用 `pid` / `title_contains`。
//! `bundle_id` 在 Windows 上恒为 `None` —— 带 `bundle_id` 的查询在 Windows 上
//! 必然 0 匹配, 这是 fail closed, 不是静默放宽。

use super::*;
use crate::{
    control_ax::AxRect,
    control_display_scope::{
        display_intersects_rect, display_scope_report, resolve_display_scope,
        resolve_observation_window_ref, DisplayRect, DisplayScope, DisplaySelector,
    },
    control_observation::{resolve_observation_ref, stale_observation_ref_error},
    control_resource_lane::capture_resource_epochs,
    screenshot::current_display_summaries,
};
use std::{
    io,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use windows_sys::core::BOOL;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT, TRUE},
    UI::HiDpi::GetDpiForWindow,
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
        PostMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW, WM_CLOSE,
    },
};

/// 类名缓冲上限。Win32 注册窗口类时长上限 255 字符, 留一个 NUL 槽刚好装下。
const CLASS_NAME_CHARS: usize = 256;

/// 标题读取上限。安装器/下载器这类标题可以很长, 但一个响应字段没有理由
/// 为它分配无界缓冲 —— 截断让枚举成本与标题长度解耦。
const TITLE_CHARS: usize = 512;

/// os-logical 换算的 96-DPI 基准, 与 `screenshot.rs` 同源。
const BASELINE_DPI: f64 = 96.0;

/// Windows 上的 to_interact 步骤词表。
///
/// 与 macOS 的 `unhide_app` / `activate_app` / `raise_window` **不是同一套词**:
/// Windows 没有"激活应用"与"raise 窗口"之分, 只有恢复 / 显示 / 置前三个原子动作。
/// 跨平台 step 词不做别名兼容 —— 不认识的 step fail closed, 并在 report 里
/// 说清 Windows 支持哪几个, 而不是静默忽略。
const STEP_RESTORE: &str = "restore_window";
const STEP_SHOW: &str = "show_window";
const STEP_SET_FOREGROUND: &str = "set_foreground_window";

const WINDOWS_RESIZE_UNSUPPORTED_REASON: &str =
    "windows_resize_not_implemented: SetWindowPos 的外框语义与 per-monitor DPI 下的尺寸换算 \
     尚未在本仓验证; 给出与 macOS 同名不同义的 rect 契约, 比诚实地不支持更糟";

/// 一个顶层窗口的完整身份快照。
#[derive(Debug, Clone)]
struct WinWindow {
    hwnd: HWND,
    pid: i32,
    class_name: String,
    title: Option<String>,
    /// Win32 原生 (物理像素) 矩形, 见模块头"两个坐标系"一节。
    native_rect: Option<AxRect>,
    /// os-logical 契约矩形, 对外暴露用。
    rect: Option<AxRect>,
    visible: bool,
    minimized: bool,
    focused: bool,
}

impl WinWindow {
    /// macOS 的 `current_space` 在 Windows 上的对应物: 可见且没最小化。
    fn on_screen(&self) -> bool {
        self.visible && !self.minimized
    }

    /// Windows 没有公开的 per-window 遮挡查询 (DWM 不给), 这里用与 macOS
    /// 同一种"在屏但不是前台"的代理判断。真正的遮挡证据只能来自截图。
    fn occluded(&self) -> bool {
        self.on_screen() && !self.focused
    }

    /// 可交互 = 在屏 + 有非零面积。面积为零的窗口接不到指针输入,
    /// 放进 interactable 会让唯一窗口选择器 (`select_unique_interactable_window_id`)
    /// 选到一个点不动的目标。
    fn interactable(&self) -> bool {
        self.on_screen()
            && self
                .rect
                .as_ref()
                .map(|rect| rect.width > 0 && rect.height > 0)
                .unwrap_or(false)
    }
}

#[derive(Debug, Copy, Clone)]
struct ParsedWindowId {
    pid: i32,
    hwnd: HWND,
}

/// 枚举回调的落袋容器。EnumWindows 只给 `LPARAM`, 没有用户态上下文指针,
/// 这是 Win32 枚举的标准做法。
struct WindowSink {
    handles: Vec<HWND>,
}

unsafe extern "system" fn collect_window_handle(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam 由 `enumerate_top_level_handles` 传入, 指向同一次调用栈上的
    // `WindowSink`, 在 EnumWindows 返回前一直有效; 回调不保存该指针。
    let sink = unsafe { &mut *(lparam as *mut WindowSink) };
    sink.handles.push(hwnd);
    TRUE
}

fn enumerate_top_level_handles() -> io::Result<Vec<HWND>> {
    let mut sink = WindowSink {
        handles: Vec::new(),
    };
    // SAFETY: 见 `collect_window_handle`。回调只 push 句柄, 不触碰窗口状态。
    let ok = unsafe {
        EnumWindows(
            Some(collect_window_handle),
            &mut sink as *mut WindowSink as LPARAM,
        )
    };
    if ok == 0 {
        return Err(io::Error::other("EnumWindows 失败, 无法枚举顶层窗口"));
    }
    Ok(sink.handles)
}

fn enumerate_windows() -> io::Result<Vec<WinWindow>> {
    let handles = enumerate_top_level_handles()?;
    if handles.is_empty() {
        // 交互式桌面至少存在一个窗口 (shell / Progman / WorkerW)。枚举全空
        // 只有一种现实解释: daemon 不在目标用户的 window station 上
        // (典型是 Session 0 服务化启动)。此时回 unsupported 会让 agent
        // 误以为"Windows 没实现", 所以显式报成可诊断的一等错误。
        return Err(io::Error::other(
            "EnumWindows 返回 0 个顶层窗口: daemon 很可能不在目标用户的交互式 window station \
             (Session 0 服务化启动的典型症状); 请把 daemon 跑在登录用户会话里再试",
        ));
    }

    let foreground = unsafe { GetForegroundWindow() };
    Ok(handles
        .into_iter()
        .map(|hwnd| unsafe { read_window(hwnd, foreground) })
        .collect())
}

unsafe fn read_window(hwnd: HWND, foreground: HWND) -> WinWindow {
    let mut process_id: u32 = 0;
    // SAFETY: 枚举出来的 HWND 在本次调用内存活, 全部是只读属性查询。
    unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    let native_rect = read_window_rect(hwnd);
    let scale = window_scale_factor(hwnd);
    WinWindow {
        hwnd,
        pid: process_id as i32,
        class_name: read_class_name(hwnd),
        title: read_window_title(hwnd),
        // AxRect 不是 Copy, 所以走引用而不是把 native_rect 移走。
        rect: native_rect.as_ref().map(|rect| to_os_logical(rect, scale)),
        native_rect,
        visible: unsafe { IsWindowVisible(hwnd) != 0 },
        minimized: unsafe { IsIconic(hwnd) != 0 },
        focused: hwnd == foreground,
    }
}

/// 该窗口所在屏的缩放系数。
///
/// per-monitor v2 aware 进程里 `GetDpiForWindow` 给的是窗口自己所在屏的 DPI,
/// 混合 DPI 多屏下每屏不同, 不能用系统统一值 (同 `win_backend::screen_mapping_of`)。
/// 取不到时退回 1.0 —— 与 `screenshot.rs::normalized_scale` 的兜底一致。
fn window_scale_factor(hwnd: HWND) -> f64 {
    // SAFETY: 只读窗口 DPI, 无副作用。
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        1.0
    } else {
        f64::from(dpi) / BASELINE_DPI
    }
}

/// Win32 原生 (物理像素) 矩形 → os-logical 契约矩形。
fn to_os_logical(rect: &AxRect, scale: f64) -> AxRect {
    if (scale - 1.0).abs() <= f64::EPSILON {
        return rect.clone();
    }
    AxRect {
        x: (f64::from(rect.x) / scale).round() as i32,
        y: (f64::from(rect.y) / scale).round() as i32,
        width: (f64::from(rect.width) / scale).round() as u32,
        height: (f64::from(rect.height) / scale).round() as u32,
    }
}

fn read_window_rect(hwnd: HWND) -> Option<AxRect> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: rect 是可写的有效 RECT。
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some(AxRect {
        x: rect.left,
        y: rect.top,
        width: u32::try_from(rect.right - rect.left).ok()?,
        height: u32::try_from(rect.bottom - rect.top).ok()?,
    })
}

fn read_class_name(hwnd: HWND) -> String {
    let mut buffer = [0_u16; CLASS_NAME_CHARS];
    // SAFETY: 缓冲区大小与 nmaxcount 一致, 不会写越界。
    let copied = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    read_wide_buffer(&buffer, usize::try_from(copied).unwrap_or(0))
}

fn read_window_title(hwnd: HWND) -> Option<String> {
    // GetWindowTextLengthW 返回 caption 字符数 (不含 NUL); 它对**其他进程**的
    // 顶层窗口同样有效, 跨进程限制只针对控件 (Edit/Button) 的正文。
    // SAFETY: 只读窗口 caption。
    let declared = usize::try_from(unsafe { GetWindowTextLengthW(hwnd) }).unwrap_or(0);
    if declared == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; declared.min(TITLE_CHARS) + 1];
    // SAFETY: 缓冲区比声明长度多一个 NUL 槽, nmaxcount 与实际容量一致。
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    let title = read_wide_buffer(&buffer, usize::try_from(copied).unwrap_or(0));
    (!title.is_empty()).then_some(title)
}

/// 按 Win32 返回的字符数裁掉尾部 NUL 后解码。返回值少于声明长度时
/// (窗口在这两次调用之间被改标题) 按实际拷到的长度收, 不越界读未初始化槽。
fn read_wide_buffer(buffer: &[u16], copied: usize) -> String {
    let len = copied.min(buffer.len().saturating_sub(1));
    String::from_utf16_lossy(&buffer[..len])
}

/// Windows 窗口 ID = `pid:<pid>/hwnd:0x<handle>`。
///
/// `pid:` 前缀是**必须的**而不是顺手写的格式: `control_resource_lane::
/// resource_key_from_backend_id` 靠它把 backend id 归一成 `pid:<pid>` 资源键,
/// 否则 window observation 拿不到 write epoch, refind 的 stale 门禁会静默失效。
/// 后半段用 hwnd 而不是 window index, 因为 Windows 没有"某进程的窗口数组"
/// 这个稳定索引 —— HWND 是唯一真实身份 (窗口一关即失效, 所以 locator
/// lifetime 是 short_lived)。
fn window_id(window: &WinWindow) -> String {
    format!("pid:{}/hwnd:0x{:X}", window.pid, window.hwnd as usize)
}

fn parse_window_id(input: &str) -> io::Result<ParsedWindowId> {
    let (pid_part, hwnd_part) = input.split_once('/').ok_or_else(|| stale_error(input))?;
    let pid = pid_part
        .strip_prefix("pid:")
        .ok_or_else(|| stale_error(input))?
        .parse::<i32>()
        .map_err(|_| stale_error(input))?;
    let hwnd_text = hwnd_part
        .strip_prefix("hwnd:")
        .ok_or_else(|| stale_error(input))?
        .trim_start_matches("0x");
    // 0 是空 HWND 哨兵, 不是一个可操作的句柄。
    let hwnd = usize::from_str_radix(hwnd_text, 16).map_err(|_| stale_error(input))?;
    if hwnd == 0 {
        return Err(stale_error(input));
    }
    Ok(ParsedWindowId {
        pid,
        hwnd: hwnd as HWND,
    })
}

fn resolve_window_id_direct(id: &str) -> io::Result<WinWindow> {
    let parsed = parse_window_id(id)?;
    // SAFETY: 句柄来自调用方给的 id, 先验存活再读属性。
    if unsafe { IsWindow(parsed.hwnd) } == 0 {
        return Err(stale_error(id));
    }
    // SAFETY: 已确认存活; 两处都只读属性。
    let foreground = unsafe { GetForegroundWindow() };
    let window = unsafe { read_window(parsed.hwnd, foreground) };
    // HWND 会被 Windows 回收复用: 同一个句柄可能已经属于别的进程。
    // pid 与 hwnd 必须自洽, 否则 id 已失效 —— 不能拿别的窗口顶替执行动作。
    if window.pid != parsed.pid {
        return Err(stale_error(id));
    }
    Ok(window)
}

pub(super) fn find(request: &WindowFindRequest) -> io::Result<WindowFindResponse> {
    let resource_capture = capture_resource_epochs();
    let meta = WindowSnapshotMeta::now();
    let all_windows = enumerate_windows()?;
    let mut matched = all_windows
        .iter()
        .filter(|window| {
            request.query.matches_candidate(&to_candidate(
                window,
                request.include_state,
                request.include_recipes,
            ))
        })
        .collect::<Vec<_>>();
    let display_scope = match request.display_scope.as_ref() {
        Some(scope) => apply_display_scope_filter(&mut matched, &all_windows, scope)?,
        None => None,
    };
    let match_count = matched.len();
    matched.truncate(usize::from(request.limit));
    let mut candidates = matched
        .iter()
        .map(|window| to_candidate(window, request.include_state, request.include_recipes))
        .collect::<Vec<_>>();
    let observation = Some(attach_window_observation(
        &mut candidates,
        "@window-find",
        "windows",
        &resource_capture,
    )?);

    Ok(WindowFindResponse {
        kind: "window-find",
        schema: WINDOW_SCHEMA,
        platform: "windows".to_owned(),
        status: "complete".to_owned(),
        capabilities: WindowCapabilities::windows(),
        observation,
        display_scope,
        match_count,
        returned_count: candidates.len(),
        snapshot_id: meta.snapshot_id,
        observed_at_unix_ms: meta.observed_at_unix_ms,
        matches: candidates,
    })
}

fn to_candidate(window: &WinWindow, include_state: bool, include_recipes: bool) -> WindowCandidate {
    WindowCandidate {
        window_id: window_id(window),
        ref_id: None,
        locator_lifetime: "short_lived",
        app: WindowAppDescriptor {
            // Windows 上是窗口类名而非进程镜像名, 见模块头说明。
            name: window.class_name.clone(),
            pid: window.pid,
            bundle_id: None,
            hidden: !window.visible,
            frontmost: window.focused,
        },
        title: window.title.clone(),
        rect: window.rect.clone(),
        coordinate_space: WINDOW_COORDINATE_SPACE,
        state: include_state.then(|| WindowState {
            occluded: window.occluded(),
            minimized: window.minimized,
            app_hidden: !window.visible,
            current_space: window.on_screen(),
            // Windows 没有 Mission Control Space 这层概念, 该字段无对应物。
            fullscreen_space: false,
            interactable: window.interactable(),
            confidence: if window.on_screen() {
                "best_effort".to_owned()
            } else {
                "limited".to_owned()
            },
        }),
        recipes: include_recipes.then(|| WindowRecipes {
            to_interact: interaction_steps(window),
            to_close_gracefully: vec!["post_wm_close"],
            to_force_close: vec!["terminate_app", "kill_process"],
        }),
    }
}

fn apply_display_scope_filter(
    matched: &mut Vec<&WinWindow>,
    all_windows: &[WinWindow],
    scope: &DisplayScope,
) -> io::Result<Option<serde_json::Value>> {
    let before = matched.len();
    let displays = current_display_summaries()?;
    let resolution = resolve_display_scope(scope, &displays, |selector| {
        window_native_rect_for_selector(all_windows, selector)
    })?;
    // 与 display catalog 的 os_rect 比 → 必须用 native (物理) 矩形,
    // 不能用候选里对外暴露的那份 os-logical。
    matched.retain(|window| {
        window
            .native_rect
            .clone()
            .map(DisplayRect::from)
            .map(|rect| display_intersects_rect(&resolution.resolved, rect))
            .unwrap_or(false)
    });
    let after = matched.len();
    let mut report = display_scope_report(&resolution);
    report["matched_before_filter"] = serde_json::json!(before);
    report["matched_after_filter"] = serde_json::json!(after);
    Ok(Some(report))
}

fn window_native_rect_for_selector(
    windows: &[WinWindow],
    selector: &DisplaySelector,
) -> io::Result<Option<DisplayRect>> {
    let id = match selector {
        DisplaySelector::WindowId(window_id) => window_id.clone(),
        DisplaySelector::WindowRef {
            observation_id,
            ref_id,
        } => resolve_observation_window_ref(observation_id, ref_id)?.window_id,
        _ => return Ok(None),
    };
    Ok(windows
        .iter()
        .find(|window| window_id(window) == id)
        .and_then(|window| window.native_rect.clone())
        .map(DisplayRect::from))
}

fn action_report(
    action: &'static str,
    window: &WinWindow,
    snapshot_id: String,
    observed_at_unix_ms: u64,
    status: &str,
) -> WindowActionReport {
    WindowActionReport {
        kind: "window-action",
        schema: WINDOW_SCHEMA,
        platform: "windows".to_owned(),
        action,
        status: status.to_owned(),
        window_id: Some(window_id(window)),
        snapshot_id: Some(snapshot_id),
        observed_at_unix_ms: Some(observed_at_unix_ms),
        strategy: None,
        target_pid: None,
        process_scope: None,
        termination_attempted: None,
        failed_step: None,
        error_code: None,
        before_rect: None,
        requested_size: None,
        requested_rect: None,
        after_rect: None,
        delta: None,
        verify: None,
        guard: None,
        clamp_reason: None,
        steps: Vec::new(),
    }
}

pub(super) fn activate(request: &WindowActivateRequest) -> io::Result<WindowActionReport> {
    let (window, snapshot_id, observed_at_unix_ms, guard_candidates) =
        resolve_window_for_activate(request)?;
    let target_window_id = window_id(&window);
    let mut report = action_report(
        "activate",
        &window,
        snapshot_id,
        observed_at_unix_ms,
        "pending",
    );

    // ------------------------------------------------------------
    // display guard 是动作前置条件。guard 不匹配时不能先激活再报失败。
    // guard 对象整体活在 display catalog 的坐标空间 (Windows 上是物理
    // 像素), 所以内部的 before_rect 用 native_rect; 顶层 report 的
    // before_rect 是 os-logical 契约坐标。两者在 macOS 上恰好同值,
    // 在 Windows 上差一个该屏 scale —— 别把它们混用。
    // ------------------------------------------------------------
    if let Some(scope) = request.guard.as_ref() {
        let Some(guard_rect) = window.native_rect.clone() else {
            report.status = "failed".to_owned();
            report.error_code = Some("WINDOW_ACTIVATE_STATE_UNREADABLE");
            report.failed_step = Some("guard_display".to_owned());
            report.steps.push(WindowActionStepReport {
                step: "guard_display".to_owned(),
                status: "failed".to_owned(),
                reason: Some("target_window_rect_unreadable".to_owned()),
                error: None,
            });
            return Ok(report);
        };
        let (guard, intersects) = window_display_guard_report(
            scope,
            &target_window_id,
            guard_rect,
            &guard_candidates,
            "before_rect",
            "before_rect_intersects",
        )?;
        report.before_rect = window.rect.clone();
        report.guard = Some(guard);
        report.steps.push(WindowActionStepReport {
            step: "guard_display".to_owned(),
            status: if intersects { "ok" } else { "failed" }.to_owned(),
            reason: (!intersects).then(|| "target_window_outside_display_guard".to_owned()),
            error: None,
        });
        if !intersects {
            report.status = "failed".to_owned();
            report.error_code = Some("WINDOW_ACTIVATE_GUARD_FAILED");
            report.failed_step = Some("guard_display".to_owned());
            return Ok(report);
        }
    }

    for step in resolve_activation_steps(request, &window) {
        let step_report = perform_activation_step(&step, &window);
        if step_report.status == "failed" {
            let failed_step = step_report.step.clone();
            report.steps.push(step_report);
            report.status = "failed".to_owned();
            report.failed_step = Some(failed_step);
            return Ok(report);
        }
        report.steps.push(step_report);
    }

    // 轮询 fresh focus state。backend accepted 不是完整成功 ——
    // SetForegroundWindow 撞上 Windows 前台锁时同样会返回失败, 而返回成功
    // 之后系统也可能把前台抢回给别的窗口, 所以必须看轮询结果而不是返回值。
    let verify_result = poll_window_activation_state(window.hwnd, request.verify);
    apply_window_activation_verification(&mut report, verify_result);
    Ok(report)
}

/// 恢复到前台所需的最小步骤集。
fn interaction_steps(window: &WinWindow) -> Vec<&'static str> {
    let mut steps = Vec::new();
    if window.minimized {
        steps.push(STEP_RESTORE);
    }
    if !window.visible {
        steps.push(STEP_SHOW);
    }
    steps.push(STEP_SET_FOREGROUND);
    steps
}

fn resolve_activation_steps(request: &WindowActivateRequest, window: &WinWindow) -> Vec<String> {
    if !request.steps.is_empty() {
        return request.steps.clone();
    }
    let steps = match request.recipe.as_deref() {
        // to_interact 与不指定 recipe 都表达"把它叫到前面并可交互"。
        None | Some("to_interact") => interaction_steps(window),
        // 其余 recipe 只表达"叫到前面", 不做恢复/显示。
        Some(_) => vec![STEP_SET_FOREGROUND],
    };
    steps.into_iter().map(str::to_owned).collect()
}

fn perform_activation_step(step: &str, window: &WinWindow) -> WindowActionStepReport {
    match step {
        STEP_RESTORE => {
            if !window.minimized {
                return skipped_step(step, "already_restored");
            }
            // SAFETY: ShowWindow 只影响目标窗口的显示状态。
            // 注意返回值是**之前的可见状态**而不是成功与否, 0 不等于失败,
            // 所以这里不能用返回值判成败。
            unsafe { ShowWindow(window.hwnd, SW_RESTORE) };
            ok_step(step)
        }
        STEP_SHOW => {
            if window.visible {
                return skipped_step(step, "already_visible");
            }
            // SAFETY: 同上; 同样是"之前可见状态"语义, 不作成败判据。
            unsafe { ShowWindow(window.hwnd, SW_SHOW) };
            ok_step(step)
        }
        STEP_SET_FOREGROUND => {
            // SAFETY: 置前台只改本会话的前台窗口, 无指针参数。
            let ok = unsafe { SetForegroundWindow(window.hwnd) };
            if ok == 0 {
                return WindowActionStepReport {
                    step: step.to_owned(),
                    status: "failed".to_owned(),
                    reason: Some("set_foreground_window_rejected".to_owned()),
                    error: Some(
                        "SetForegroundWindow 返回 0: Windows 前台锁拒绝了本次置前 \
                         (调用进程当前不在前台, 或目标窗口由更高权限进程拥有)"
                            .to_owned(),
                    ),
                };
            }
            ok_step(step)
        }
        unknown => WindowActionStepReport {
            step: unknown.to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(format!(
                "未知 activate step: {unknown}; Windows 支持 {STEP_RESTORE} / {STEP_SHOW} / \
                 {STEP_SET_FOREGROUND}"
            )),
        },
    }
}

fn ok_step(step: &str) -> WindowActionStepReport {
    WindowActionStepReport {
        step: step.to_owned(),
        status: "ok".to_owned(),
        reason: None,
        error: None,
    }
}

fn skipped_step(step: &str, reason: &str) -> WindowActionStepReport {
    WindowActionStepReport {
        step: step.to_owned(),
        status: "skipped".to_owned(),
        reason: Some(reason.to_owned()),
        error: None,
    }
}

fn apply_window_activation_verification(
    report: &mut WindowActionReport,
    verify_result: io::Result<WindowActivateVerifyReport>,
) {
    match verify_result {
        Ok(verify) => {
            let passed = verify.status == "passed";
            report.verify = Some(WindowActionVerifyReport::Activate(verify));
            report.steps.push(WindowActionStepReport {
                step: "verify_focus".to_owned(),
                status: if passed { "ok" } else { "failed" }.to_owned(),
                reason: (!passed).then(|| "focus_not_acquired_before_timeout".to_owned()),
                error: None,
            });
            if passed {
                report.status = "ok".to_owned();
            } else {
                report.status = "failed".to_owned();
                report.error_code = Some("WINDOW_FOCUS_NOT_ACQUIRED");
                report.failed_step = Some("verify_focus".to_owned());
            }
        }
        Err(err) => {
            report.status = "failed".to_owned();
            report.error_code = Some("WINDOW_ACTIVATE_STATE_UNREADABLE");
            report.failed_step = Some("verify_focus".to_owned());
            report.steps.push(WindowActionStepReport {
                step: "verify_focus".to_owned(),
                status: "failed".to_owned(),
                reason: Some("activation_state_unreadable".to_owned()),
                error: Some(err.to_string()),
            });
        }
    }
}

/// 轮询焦点状态直到 verify 判定通过或超时。
///
/// 返回的是**未包 enum 的** `WindowActivateVerifyReport`: 轮询面只处理
/// activate 一种语义, 提升成 `WindowActionVerifyReport` 属于"写回 report"
/// 那一层的责任, 由 `apply_window_activation_verification` 收口。
fn poll_window_activation_state(
    hwnd: HWND,
    verify: WindowActivateVerify,
) -> io::Result<WindowActivateVerifyReport> {
    let started = Instant::now();
    let timeout = Duration::from_millis(u64::from(verify.timeout_ms));
    let poll_interval = Duration::from_millis(u64::from(verify.poll_interval_ms));
    let mut last_report = None;
    let mut last_error = None;

    loop {
        match read_window_activation_state(hwnd) {
            Ok(state) => {
                let report = evaluate_window_activate_verification(
                    state,
                    verify,
                    started.elapsed().as_millis() as u64,
                );
                if report.status == "passed" {
                    return Ok(report);
                }
                last_report = Some(report);
            }
            Err(err) => last_error = Some(err),
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            if let Some(mut report) = last_report {
                report.elapsed_ms = elapsed.as_millis() as u64;
                return Ok(report);
            }
            return Err(last_error
                .unwrap_or_else(|| io::Error::other("activation state 在 timeout 内始终不可读")));
        }
        thread::sleep(poll_interval.min(timeout.saturating_sub(elapsed)));
    }
}

fn read_window_activation_state(hwnd: HWND) -> io::Result<WindowActivationState> {
    // SAFETY: 句柄在 resolve 阶段验过存活; 这里再验一次是為了捕捉动作执行
    // 期间窗口被关掉的形态 —— 那种情况下继续读属性只会拿到陈旧默认值。
    if unsafe { IsWindow(hwnd) } == 0 {
        return Err(io::Error::other("目标窗口在 activate 验证期间已消失"));
    }
    // SAFETY: 只读前台窗口与窗口状态。
    let foreground = unsafe { GetForegroundWindow() };
    Ok(WindowActivationState {
        // Windows 没有"前台应用"与"聚焦窗口"之分, 两者都是前台窗口判定。
        frontmost: hwnd == foreground,
        focused: hwnd == foreground,
        hidden: unsafe { IsWindowVisible(hwnd) } == 0,
        minimized: unsafe { IsIconic(hwnd) } != 0,
    })
}

pub(super) fn close(request: &WindowCloseRequest) -> io::Result<WindowActionReport> {
    let find_response = find(&WindowFindRequest {
        query: request.target.query.clone(),
        display_scope: None,
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: true,
        include_recipes: true,
    })?;
    let strategy = request.strategy;
    let (window, snapshot_id, observed_at_unix_ms) = resolve_single_window(
        &find_response,
        &request.target,
        request.allow_ambiguous,
        request.select,
        Some(strategy),
        "close",
    )?;

    let step = match strategy {
        WindowCloseStrategy::Graceful => post_window_close(&window),
        WindowCloseStrategy::Terminate => terminate_process_step(&window, false),
        WindowCloseStrategy::Kill => terminate_process_step(&window, true),
    };
    let failed_step = (step.status == "failed").then(|| step.step.clone());
    let killed_process = strategy != WindowCloseStrategy::Graceful;
    let mut report = action_report(
        "close",
        &window,
        snapshot_id,
        observed_at_unix_ms,
        if failed_step.is_some() {
            "failed"
        } else {
            "ok"
        },
    );
    report.strategy = Some(strategy.as_str());
    report.failed_step = failed_step;
    report.target_pid = killed_process.then_some(window.pid);
    report.process_scope = killed_process.then_some("single_resolved_process");
    report.termination_attempted = killed_process.then_some(true);
    report.steps = vec![step];
    Ok(report)
}

/// 优雅关闭 = 投递 WM_CLOSE。
///
/// 用 `PostMessageW` 而不是 `DestroyWindow`: WM_CLOSE 是窗口自己的关闭协议
/// (应用可以弹"是否保存"对话框), 而 DestroyWindow 只能销毁本进程创建的窗口。
fn post_window_close(window: &WinWindow) -> WindowActionStepReport {
    // SAFETY: 只向目标窗口投递消息, 不跨进程读写内存。
    let ok = unsafe { PostMessageW(window.hwnd, WM_CLOSE, 0, 0) };
    if ok == 0 {
        return WindowActionStepReport {
            step: "post_wm_close".to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(format!(
                "PostMessageW(WM_CLOSE) 失败 ({}): 窗口可能已关闭, 或目标进程处于更高完整性级别而被 UIPI 拒绝",
                io::Error::last_os_error()
            )),
        };
    }
    WindowActionStepReport {
        step: "post_wm_close".to_owned(),
        status: "ok".to_owned(),
        // WM_CLOSE 是异步投递: 报 ok 只代表消息已进目标线程队列。
        reason: Some("message_posted_window_may_prompt_before_exiting".to_owned()),
        error: None,
    }
}

/// 强制结束进程。
///
/// 进程级动作只能靠系统自带的 `taskkill` —— `TerminateProcess` 在
/// `Win32_System_Threading` feature 下, 当前未启用。语义对齐 macOS:
/// `terminate` 是非强杀 (让应用走自己的退出流程), `kill` 才加 `/F`,
/// 两者都只针对解析出的那一个进程 (不加 `/T`), 不牵连子进程。
fn terminate_process_step(window: &WinWindow, force: bool) -> WindowActionStepReport {
    let step = if force {
        "kill_process"
    } else {
        "terminate_app"
    };
    let mut command = Command::new("taskkill");
    command.arg("/PID").arg(window.pid.to_string());
    if force {
        command.arg("/F");
    }
    match command.status() {
        Ok(status) if status.success() => ok_step(step),
        // taskkill 的输出随系统语言变 (简体中文机器上是 GBK), 把它塞进 report
        // 只会复现 "@task-output 乱码" 那条老问题。只回退出码, 语义稳定。
        Ok(status) => WindowActionStepReport {
            step: step.to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(format!("taskkill exited with status {status}")),
        },
        Err(err) => WindowActionStepReport {
            step: step.to_owned(),
            status: "failed".to_owned(),
            reason: None,
            error: Some(err.to_string()),
        },
    }
}

pub(super) fn resize(request: &WindowResizeRequest) -> io::Result<WindowActionReport> {
    // 先解析目标再回答不支持: 这样"这个窗口 id 是有效的"与"resize 不可用"
    // 两件事能在同一个响应里各自自证, 不会用一个笼统的 unsupported
    // 掩盖真正的定位错误。
    let find_response = find(&WindowFindRequest {
        query: request.target.query.clone(),
        display_scope: None,
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: true,
        include_recipes: true,
    })?;
    // `WindowResizeRequest` 契约里没有 `allow_ambiguous` / `select`
    // (歧义消解只属于 activate/close 两个字), 所以 resize 的目标解析
    // 恒为 fail closed 的唯一匹配 —— 与 macOS 侧同一条约束。
    let (window, snapshot_id, observed_at_unix_ms) = resolve_single_window(
        &find_response,
        &request.target,
        false,
        None,
        None,
        "resize",
    )?;

    let mut report = WindowActionReport::unsupported("resize", "windows");
    report.window_id = Some(window_id(&window));
    report.snapshot_id = Some(snapshot_id);
    report.observed_at_unix_ms = Some(observed_at_unix_ms);
    report.before_rect = window.rect.clone();
    report.steps.push(WindowActionStepReport {
        step: "resize".to_owned(),
        status: "unsupported".to_owned(),
        reason: Some(WINDOWS_RESIZE_UNSUPPORTED_REASON.to_owned()),
        error: None,
    });
    Ok(report)
}

pub(super) fn resolve_target_rect(
    target: &WindowCommandTarget,
) -> io::Result<WindowResolvedTargetRect> {
    let find_response = find(&WindowFindRequest {
        query: target.query.clone(),
        display_scope: None,
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: false,
        include_recipes: false,
    })?;
    let (resolved, _, _) =
        resolve_single_window(&find_response, target, false, None, None, "window-target")?;
    // 返回 native_rect: 本函数的两个消费方 (鼠标 display guard 与 observe 的
    // display selector) 都拿它去比 DisplaySummary.os_rect, 那个空间在 Windows
    // 上是物理像素。对外的 os-logical 版本走 @window-find 的 candidates.rect。
    Ok(WindowResolvedTargetRect {
        window_id: window_id(&resolved),
        rect: resolved.native_rect,
    })
}

fn resolve_window_for_activate(
    request: &WindowActivateRequest,
) -> io::Result<(WinWindow, String, u64, Vec<WinWindow>)> {
    let meta = WindowSnapshotMeta::now();
    if let Some(id) = request.target.window_id.as_deref() {
        return Ok((
            resolve_window_id_direct(id)?,
            meta.snapshot_id,
            meta.observed_at_unix_ms,
            Vec::new(),
        ));
    }

    if let (Some(observation_id), Some(ref_id)) = (
        request.target.observation_id.as_deref(),
        request.target.ref_id.as_deref(),
    ) {
        let id = resolve_observation_window_id(observation_id, ref_id)?;
        return Ok((
            resolve_window_id_direct(&id)?,
            meta.snapshot_id,
            meta.observed_at_unix_ms,
            Vec::new(),
        ));
    }

    let find_response = find(&WindowFindRequest {
        query: request.target.query.clone(),
        display_scope: None,
        limit: DEFAULT_WINDOW_FIND_LIMIT,
        include_state: true,
        include_recipes: true,
    })?;
    let guard_candidates = resolve_guard_candidates(&find_response);
    let (window, snapshot_id, observed_at_unix_ms) = resolve_single_window(
        &find_response,
        &request.target,
        request.allow_ambiguous,
        request.select,
        None,
        "activate",
    )?;
    Ok((window, snapshot_id, observed_at_unix_ms, guard_candidates))
}

/// guard 的 display selector 可能指向**目标之外**的另一个窗口, 所以要带上
/// 同一批候选做 id → 矩形回查, 不能只带选中那一个。
fn resolve_guard_candidates(find_response: &WindowFindResponse) -> Vec<WinWindow> {
    find_response
        .matches
        .iter()
        .filter_map(|candidate| resolve_window_id_direct(&candidate.window_id).ok())
        .collect()
}

fn resolve_observation_window_id(observation_id: &str, ref_id: &str) -> io::Result<String> {
    let entry = resolve_observation_ref(observation_id, ref_id)?;
    if entry.kind != "window" {
        return Err(stale_observation_ref_error(
            observation_id,
            ref_id,
            format!("ref kind 不是 window: {}", entry.kind),
        ));
    }
    Ok(entry.backend_id)
}

fn resolve_single_window(
    find_response: &WindowFindResponse,
    target: &WindowCommandTarget,
    allow_ambiguous: bool,
    select: Option<WindowSelectPolicy>,
    strategy: Option<WindowCloseStrategy>,
    ambiguous_action: &'static str,
) -> io::Result<(WinWindow, String, u64)> {
    if let (Some(observation_id), Some(ref_id)) =
        (target.observation_id.as_deref(), target.ref_id.as_deref())
    {
        let id = resolve_observation_window_id(observation_id, ref_id)?;
        return Ok((
            resolve_window_id_direct(&id)?,
            find_response.snapshot_id.clone(),
            find_response.observed_at_unix_ms,
        ));
    }

    if let Some(id) = target.window_id.as_deref() {
        return Ok((
            resolve_window_id_direct(id)?,
            find_response.snapshot_id.clone(),
            find_response.observed_at_unix_ms,
        ));
    }

    let candidates = find_response.matches.as_slice();
    let candidate = match candidates {
        [candidate] => candidate.clone(),
        [] => {
            return Err(invalid_json_error(
                "window-not-found",
                64,
                "window target did not match any window",
            ));
        }
        many => {
            // 强杀动作不给多目标兜底: 误杀一个进程的代价远高于让调用方补条件。
            if let Some(strategy) = strategy {
                if strategy != WindowCloseStrategy::Graceful {
                    return Err(ambiguous_error("close", many, Some(strategy)));
                }
            }
            if !allow_ambiguous {
                return Err(ambiguous_error(ambiguous_action, many, strategy));
            }
            select_candidate(many, select)?
        }
    };

    Ok((
        resolve_window_id_direct(&candidate.window_id)?,
        find_response.snapshot_id.clone(),
        find_response.observed_at_unix_ms,
    ))
}

fn select_candidate(
    candidates: &[WindowCandidate],
    select: Option<WindowSelectPolicy>,
) -> io::Result<WindowCandidate> {
    // 解析失败的候选直接剔除而不是让整个选择失败: 一个无关的失效窗口不该
    // 否决一个本来能唯一定位的目标 (真要一个都不剩, 后面 resolve 会回 stale)。
    let resolvable = candidates
        .iter()
        .filter_map(|candidate| {
            resolve_window_id_direct(&candidate.window_id)
                .ok()
                .map(|window| (candidate.clone(), window))
        })
        .collect::<Vec<_>>();

    let chosen = match select.unwrap_or(WindowSelectPolicy::Frontmost) {
        // Windows 上 frontmost 与 focused 同源 (都是 GetForegroundWindow 判定),
        // 所以这里只有"是不是前台"一档。
        WindowSelectPolicy::Frontmost => resolvable
            .iter()
            .find(|(_, window)| window.focused)
            .map(|(candidate, _)| candidate.clone()),
        WindowSelectPolicy::First => resolvable.first().map(|(candidate, _)| candidate.clone()),
    };

    chosen.ok_or_else(|| {
        invalid_json_error(
            "window-not-found",
            64,
            "window target did not match any window",
        )
    })
}

fn window_display_guard_report(
    scope: &DisplayScope,
    target_window_id: &str,
    rect: AxRect,
    candidates: &[WinWindow],
    rect_field: &str,
    intersects_field: &str,
) -> io::Result<(serde_json::Value, bool)> {
    let displays = current_display_summaries()?;
    let resolution = resolve_display_scope(scope, &displays, |selector| {
        let id = match selector {
            DisplaySelector::WindowId(window_id) => window_id.clone(),
            DisplaySelector::WindowRef {
                observation_id,
                ref_id,
            } => resolve_observation_window_ref(observation_id, ref_id)?.window_id,
            _ => return Ok(None),
        };
        if id == target_window_id {
            return Ok(Some(DisplayRect::from(rect)));
        }
        // guard 的 display 挂在别的窗口上时, 用同批候选的矩形回查;
        // 查不到就交回 None, 由 resolve_display_scope 按"无法定位"处理,
        // 不猜。
        Ok(candidates
            .iter()
            .find(|window| window_id(window) == id)
            .and_then(|window| window.native_rect.clone())
            .map(DisplayRect::from))
    })?;
    let intersects = display_intersects_rect(&resolution.resolved, DisplayRect::from(rect));
    let mut report = display_scope_report(&resolution);
    report[rect_field] = serde_json::json!(rect);
    report[intersects_field] = serde_json::json!(intersects);
    Ok((report, intersects))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假 HWND 只用于纯函数 (id 编解码 / 状态映射), 这些用例一次 Win32
    /// 调用都不发, 所以句柄不需要真实存在。
    fn window(
        pid: i32,
        hwnd: usize,
        visible: bool,
        minimized: bool,
        focused: bool,
        rect: Option<AxRect>,
    ) -> WinWindow {
        WinWindow {
            hwnd: hwnd as HWND,
            pid,
            class_name: "Chrome_WidgetWin_1".to_owned(),
            title: Some("Yino 安装".to_owned()),
            native_rect: rect.clone(),
            rect,
            visible,
            minimized,
            focused,
        }
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Option<AxRect> {
        Some(AxRect {
            x,
            y,
            width,
            height,
        })
    }

    /// id 必须保持 `pid:` 前缀 —— 它是 window observation 拿 write epoch 的
    /// 唯一通道 (control_resource_lane::resource_key_from_backend_id)。
    #[test]
    fn window_id_round_trips_through_pid_and_hwnd() {
        let id = window_id(&window(4321, 0x40A0_F2C, true, false, true, None));
        assert_eq!(id, "pid:4321/hwnd:0x40A0F2C");
        let parsed = parse_window_id(&id).expect("id 应该能解析");
        assert_eq!(parsed.pid, 4321);
        assert_eq!(parsed.hwnd as usize, 0x40A0_F2C);
    }

    /// macOS 的 id 形态与空句柄都必须 fail closed —— 静默解析成别的窗口
    /// 比直接报 stale 危险得多。
    #[test]
    fn parse_window_id_rejects_foreign_and_null_forms() {
        for input in [
            "pid:1/window:0",
            "pid:1/hwnd:0x0",
            "pid:1/hwnd:",
            "pid:1/hwnd:0xZZ",
            "1/hwnd:0x10",
        ] {
            let error = parse_window_id(input).expect_err("非 Windows id 必须被拒");
            assert!(
                error.to_string().contains("\"kind\":\"window-stale\""),
                "{input} 应当回 window-stale, 实际: {error}"
            );
        }
    }

    /// 4K@200% 的实测机: 物理 3840×2160 的窗口在 os-logical 里是 1920×1080。
    /// 少这一步换算, agent 按窗口 rect 算出的点击坐标会整整翻倍。
    #[test]
    fn os_logical_rect_halves_physical_rect_at_200pct() {
        let native = AxRect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
        };
        assert_eq!(
            to_os_logical(&native, 2.0),
            AxRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }
        );
        assert_eq!(to_os_logical(&native, 1.0), native, "100% 屏必须恒等");
    }

    /// 最小化 / 隐藏 / 零面积 都不是可交互目标 —— 唯一窗口选择器据此拒绝,
    /// 否则会把"叫到前台之前"的窗口当成唯一目标。
    #[test]
    fn interactable_requires_visible_non_minimized_and_non_empty() {
        let visible = window(1, 0x10, true, false, true, rect(0, 0, 800, 600));
        assert!(visible.interactable());
        assert!(!window(1, 0x10, true, true, false, rect(0, 0, 800, 600)).interactable());
        assert!(!window(1, 0x10, false, false, false, rect(0, 0, 800, 600)).interactable());
        assert!(!window(1, 0x10, true, false, true, rect(0, 0, 0, 600)).interactable());
        assert!(!window(1, 0x10, true, false, true, None).interactable());
    }

    /// 最小化窗口的 confidence 必须落到 limited —— 与 macOS 的
    /// `current_space == false` 分支同义, 否则唯一窗口门禁会误信它。
    #[test]
    fn minimized_candidate_is_limited_and_not_interactable() {
        let minimized = window(1, 0x10, true, true, false, rect(0, 0, 800, 600));
        let candidate = to_candidate(&minimized, true, true);
        let state = candidate.state.expect("include_state 应产出 state");
        assert!(state.minimized);
        assert!(!state.current_space);
        assert!(!state.interactable);
        assert_eq!(state.confidence, "limited");
    }

    /// Windows 没有 bundle id 概念, 带 bundle_id 的查询必然 0 匹配 ——
    /// 候选侧必须恒为 None, 不能塞类名冒充。
    #[test]
    fn candidate_exposes_class_name_as_app_and_never_a_bundle_id() {
        let candidate = to_candidate(
            &window(42, 0x10, true, false, true, rect(0, 0, 800, 600)),
            true,
            true,
        );
        assert_eq!(candidate.app.name, "Chrome_WidgetWin_1");
        assert_eq!(candidate.app.pid, 42);
        assert_eq!(candidate.app.bundle_id, None);
        assert_eq!(candidate.locator_lifetime, "short_lived");

        let by_bundle = WindowQuery {
            bundle_id: Some("com.apple.calculator".to_owned()),
            ..WindowQuery::default()
        };
        assert!(!by_bundle.matches_candidate(&candidate));

        let by_class = WindowQuery {
            app: Some("chrome_widgetwin_1".to_owned()),
            ..WindowQuery::default()
        };
        assert!(by_class.matches_candidate(&candidate));
    }

    /// 置前步骤必须按状态补齐恢复/显示, 且顺序是恢复 → 显示 → 置前。
    #[test]
    fn interaction_steps_restore_then_show_then_foreground() {
        assert_eq!(
            interaction_steps(&window(1, 0x10, true, false, false, None)),
            vec![STEP_SET_FOREGROUND]
        );
        assert_eq!(
            interaction_steps(&window(1, 0x10, true, true, false, None)),
            vec![STEP_RESTORE, STEP_SET_FOREGROUND]
        );
        assert_eq!(
            interaction_steps(&window(1, 0x10, false, true, false, None)),
            vec![STEP_RESTORE, STEP_SHOW, STEP_SET_FOREGROUND]
        );
    }

    /// 显式 steps 优先于 recipe 推导; 不认识的 step 由 perform_activation_step
    /// fail closed, 这里只锁"推导不被显式 steps 覆盖"这一条。
    #[test]
    fn explicit_steps_override_the_derived_recipe() {
        let minimized = window(1, 0x10, true, true, false, None);
        let request = WindowActivateRequest {
            target: WindowCommandTarget {
                window_id: Some("pid:1/hwnd:0x10".to_owned()),
                ..WindowCommandTarget::default()
            },
            recipe: None,
            steps: vec![STEP_SET_FOREGROUND.to_owned()],
            allow_ambiguous: false,
            select: None,
            guard: None,
            verify: WindowActivateVerify::default(),
        };
        assert_eq!(
            resolve_activation_steps(&request, &minimized),
            vec![STEP_SET_FOREGROUND.to_owned()]
        );
    }
}
