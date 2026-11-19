use super::displays::DisplayTopology;
use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetAncestor, GetForegroundWindow, GetWindow, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, PeekMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WindowFromPoint, EVENT_OBJECT_DESTROY, GA_ROOT, GW_OWNER, HHOOK, KBDLLHOOKSTRUCT,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, OBJID_WINDOW, PM_REMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WINEVENT_OUTOFCONTEXT, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::super::{CaptureSpec, CapturedInput, NativeRect, WindowIdentity, SELF_EVENT_MARKER};
use crate::control_recording::desktop::check_recording_desktop;

fn same_instance(a: WindowIdentity, b: WindowIdentity) -> bool {
    a.hwnd == b.hwnd && a.pid == b.pid && a.process_start == b.process_start
}

#[derive(Clone)]
pub(super) struct IdentityEvidence {
    pub window: WindowIdentity,
    roots: Arc<[WindowIdentity]>,
    live: Arc<AtomicBool>,
    fault: Arc<AtomicBool>,
}

impl IdentityEvidence {
    fn live(&self) -> bool {
        self.live.load(Ordering::Acquire) && !self.fault.load(Ordering::Acquire)
    }
}

struct RegisteredIdentity {
    window: WindowIdentity,
    live: Arc<AtomicBool>,
}

pub(super) struct HookEvent {
    pub at_ns: u64,
    pub input: CapturedInput,
    pub evidence: Option<IdentityEvidence>,
}

pub(super) enum HookCommand {
    Freeze,
    ParameterBoundary,
}

pub(super) struct HookRuntime {
    pub spec: CaptureSpec,
    pub events: SyncSender<HookEvent>,
    pub commands: Receiver<HookCommand>,
    pub ready: SyncSender<Result<(), String>>,
    pub fault: Arc<AtomicBool>,
    pub origin: Instant,
}

struct State {
    roots: Arc<[WindowIdentity]>,
    identities: Vec<RegisteredIdentity>,
    next_generation: u64,
    events: SyncSender<HookEvent>,
    fault: Arc<AtomicBool>,
    origin: Instant,
    redaction: Option<(WindowIdentity, u64)>,
    segment: u64,
    contexts: Vec<(WindowIdentity, NativeRect, u32, bool)>,
    next_contexts: Vec<(WindowIdentity, NativeRect, u32, bool)>,
    displays: DisplayTopology,
}

thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

fn elapsed(origin: Instant) -> u64 {
    origin.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

impl State {
    fn emit(&self, input: CapturedInput) {
        if self
            .events
            .try_send(HookEvent {
                at_ns: elapsed(self.origin),
                input,
                evidence: None,
            })
            .is_err()
        {
            self.fault.store(true, Ordering::Release);
        }
    }

    fn register_identity(&mut self, mut window: WindowIdentity) -> WindowIdentity {
        if let Some(existing) = self
            .identities
            .iter()
            .find(|entry| same_instance(entry.window, window))
        {
            return existing.window;
        }
        self.invalidate_identity(window.hwnd);
        window.generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        self.identities.push(RegisteredIdentity {
            window,
            live: Arc::new(AtomicBool::new(true)),
        });
        window
    }

    fn invalidate_identity(&mut self, hwnd: u64) {
        self.identities.retain(|entry| {
            if entry.window.hwnd == hwnd {
                entry.live.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
    }

    fn evidence(&self, window: WindowIdentity) -> Option<IdentityEvidence> {
        let entry = self
            .identities
            .iter()
            .find(|entry| entry.window == window)?;
        Some(IdentityEvidence {
            window,
            roots: self.roots.clone(),
            live: entry.live.clone(),
            fault: self.fault.clone(),
        })
    }

    fn mouse_down(&mut self, window: WindowIdentity, x: i32, y: i32, button: u8, injected: bool) {
        self.close_redaction();
        let Some(evidence) = self.evidence(window) else {
            self.fault.store(true, Ordering::Release);
            return;
        };
        let input = CapturedInput::MouseDown {
            window,
            x,
            y,
            button,
            injected,
        };
        if self
            .events
            .try_send(HookEvent {
                at_ns: elapsed(self.origin),
                input,
                evidence: Some(evidence),
            })
            .is_err()
        {
            self.fault.store(true, Ordering::Release);
        }
    }

    fn close_redaction(&mut self) {
        if let Some((window, segment)) = self.redaction.take() {
            self.emit(CapturedInput::Redaction {
                window: Some(window),
                segment,
                active: false,
            });
        }
    }

    fn keyboard(&mut self, window: Option<WindowIdentity>) {
        if self.redaction.as_ref().map(|(w, _)| *w) == window {
            return;
        }
        self.close_redaction();
        if let Some(window) = window {
            self.segment = self.segment.saturating_add(1);
            self.redaction = Some((window, self.segment));
            self.emit(CapturedInput::Redaction {
                window: Some(window),
                segment: self.segment,
                active: true,
            });
        }
    }

    fn resolve(&mut self, hwnd: HWND) -> Option<WindowIdentity> {
        if self.fault.load(Ordering::Acquire) {
            return None;
        }
        let root = root_window(hwnd)?;
        if !authorized(root, &self.roots) {
            return None;
        }
        let raw = identity(root)?;
        if self
            .roots
            .iter()
            .any(|expected| expected.hwnd == raw.hwnd && !same_instance(*expected, raw))
        {
            return None;
        }
        Some(self.register_identity(raw))
    }

    fn observe(&mut self) -> Result<(), String> {
        check_recording_desktop()?;
        if let Some((generation, displays)) = self.displays.observe()? {
            self.emit(CapturedInput::DisplayContext {
                generation,
                displays,
            });
        }
        for root in self.roots.iter() {
            if !identity(root.hwnd as usize as HWND)
                .is_some_and(|current| same_instance(current, *root))
            {
                return Err("recording target no longer valid".into());
            }
        }
        let foreground = self.resolve(foreground());
        if self
            .redaction
            .as_ref()
            .map(|(w, _)| *w)
            .is_some_and(|w| Some(w) != foreground)
        {
            self.close_redaction();
        }
        self.next_contexts.clear();
        for index in 0..self.roots.len()
            + usize::from(foreground.is_some_and(|window| !self.roots.contains(&window)))
        {
            let window = if index < self.roots.len() {
                self.roots[index]
            } else {
                foreground.expect("foreground exists")
            };
            let hwnd = window.hwnd as usize as HWND;
            let mut rect = RECT::default();
            // SAFETY: hwnd 已校验, rect 是有效的可写输出位置.
            if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                return Err("window geometry unavailable".into());
            }
            // SAFETY: hwnd 已通过当前窗口身份校验.
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            if dpi == 0 {
                return Err("window DPI unavailable".into());
            }
            let bounds = NativeRect {
                x: rect.left,
                y: rect.top,
                width: rect.right.saturating_sub(rect.left) as u32,
                height: rect.bottom.saturating_sub(rect.top) as u32,
            };
            let context = (window, bounds, dpi, foreground == Some(window));
            if !self.contexts.contains(&context) {
                self.emit(CapturedInput::WindowContext {
                    window,
                    rect: bounds,
                    dpi,
                    focused: context.3,
                });
            }
            self.next_contexts.push(context);
        }
        std::mem::swap(&mut self.contexts, &mut self.next_contexts);
        Ok(())
    }
}

fn foreground() -> HWND {
    // SAFETY: 只读查询不接收指针, 不改变窗口状态.
    unsafe { GetForegroundWindow() }
}

fn root_window(hwnd: HWND) -> Option<HWND> {
    if hwnd.is_null() {
        return None;
    }
    // SAFETY: API 允许过期 HWND, 查询失败时返回空句柄.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    (!root.is_null()).then_some(root)
}

fn process_start(pid: u32) -> Option<u64> {
    // SAFETY: 只请求 limited-query 权限, 不改变目标进程.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: process 是已打开的句柄, 四个输出指针均有效.
    let ok = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    // SAFETY: 此句柄由本函数独占, 只在这里关闭.
    unsafe {
        CloseHandle(process);
    }
    (ok != 0).then_some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
}

fn identity(hwnd: HWND) -> Option<WindowIdentity> {
    if hwnd.is_null() {
        return None;
    }
    // SAFETY: 这两个查询 API 允许过期 HWND, 不会解引用该数字句柄.
    if unsafe { IsWindow(hwnd) } == 0 {
        return None;
    }
    let mut pid = 0;
    // SAFETY: pid 是有效的可写输出位置.
    if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0 || pid == 0 {
        return None;
    }
    let hwnd = hwnd as usize as u64;
    Some(WindowIdentity {
        pid,
        hwnd,
        process_start: process_start(pid)?,
        generation: 0,
    })
}

fn owner_chain_authorized<F>(start: u64, roots: &[WindowIdentity], mut owner: F) -> bool
where
    F: FnMut(u64) -> Option<u64>,
{
    let mut current = start;
    for _ in 0..64 {
        if roots.iter().any(|root| root.hwnd == current) {
            return true;
        }
        match owner(current) {
            Some(next) if next != current => current = next,
            _ => return false,
        }
    }
    false
}

fn authorized(hwnd: HWND, roots: &[WindowIdentity]) -> bool {
    owner_chain_authorized(hwnd as usize as u64, roots, |current| {
        // SAFETY: GetWindow 允许过期 HWND, 空句柄终止 owner 链.
        let owner = unsafe { GetWindow(current as usize as HWND, GW_OWNER) };
        root_window(owner).map(|window| window as usize as u64)
    }) && roots.iter().all(|root| {
        identity(root.hwnd as usize as HWND).is_some_and(|current| same_instance(current, *root))
    })
}

pub(super) fn valid_identity(evidence: &IdentityEvidence) -> bool {
    if !evidence.live() {
        return false;
    }
    let window = evidence.window;
    let hwnd = window.hwnd as usize as HWND;
    let valid = identity(hwnd).is_some_and(|current| same_instance(current, window))
        && root_window(hwnd) == Some(hwnd)
        && authorized(hwnd, &evidence.roots);
    valid && evidence.live()
}

fn self_event(extra: usize) -> bool {
    extra == SELF_EVENT_MARKER as usize
}

unsafe extern "system" fn mouse_hook(code: i32, message: WPARAM, param: LPARAM) -> LRESULT {
    if code >= 0 && param != 0 {
        // SAFETY: 非负低级鼠标 callback 由系统传入有效 MSLLHOOKSTRUCT.
        let mouse = unsafe { &*(param as *const MSLLHOOKSTRUCT) };
        if !self_event(mouse.dwExtraInfo) {
            STATE.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    if let Some(state) = slot.as_mut() {
                        if matches!(
                            message as u32,
                            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                        ) {
                            state.close_redaction();
                        }
                        // SAFETY: WindowFromPoint 按值接收 POINT.
                        let target = state.resolve(unsafe { WindowFromPoint(mouse.pt) });
                        if let Some(window) = target {
                            let (x, y, injected) =
                                (mouse.pt.x, mouse.pt.y, mouse.flags & LLMHF_INJECTED != 0);
                            let button = match message as u32 {
                                WM_LBUTTONDOWN | WM_LBUTTONUP => 1,
                                WM_RBUTTONDOWN | WM_RBUTTONUP => 2,
                                WM_MBUTTONDOWN | WM_MBUTTONUP => 3,
                                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                                    if mouse.mouseData >> 16 == 1 {
                                        4
                                    } else {
                                        5
                                    }
                                }
                                _ => 0,
                            };
                            let input = match message as u32 {
                                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
                                | WM_XBUTTONDOWN => {
                                    state.mouse_down(window, x, y, button, injected);
                                    None
                                }
                                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                                    Some(CapturedInput::MouseUp {
                                        window,
                                        x,
                                        y,
                                        button,
                                        injected,
                                    })
                                }
                                WM_MOUSEMOVE => Some(CapturedInput::MouseMove {
                                    window,
                                    x,
                                    y,
                                    injected,
                                }),
                                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                                    let delta = (mouse.mouseData >> 16) as u16 as i16 as i32;
                                    let (delta_x, delta_y) = if message as u32 == WM_MOUSEHWHEEL {
                                        (delta, 0)
                                    } else {
                                        (0, delta)
                                    };
                                    Some(CapturedInput::Scroll {
                                        window,
                                        x,
                                        y,
                                        delta_x,
                                        delta_y,
                                        injected,
                                    })
                                }
                                _ => None,
                            };
                            if let Some(input) = input {
                                state.emit(input);
                            }
                        }
                    }
                }
            });
        }
    }
    // SAFETY: 原样转交参数, 保持系统 hook 链.
    unsafe { CallNextHookEx(null_mut(), code, message, param) }
}

unsafe extern "system" fn keyboard_hook(code: i32, message: WPARAM, param: LPARAM) -> LRESULT {
    if code >= 0 && param != 0 && matches!(message as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: 非负低级键盘 callback 由系统传入有效 KBDLLHOOKSTRUCT.
        let keyboard = unsafe { &*(param as *const KBDLLHOOKSTRUCT) };
        if !self_event(keyboard.dwExtraInfo) {
            STATE.with(|slot| {
                if let Ok(mut slot) = slot.try_borrow_mut() {
                    if let Some(state) = slot.as_mut() {
                        let window = state.resolve(foreground());
                        state.keyboard(window);
                    }
                }
            });
        }
    }
    // SAFETY: 原样转交参数, 保持系统 hook 链.
    unsafe { CallNextHookEx(null_mut(), code, message, param) }
}

unsafe extern "system" fn window_destroyed(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.is_null() || object != OBJID_WINDOW || child != 0 {
        return;
    }
    let id = hwnd as usize as u64;
    STATE.with(|slot| {
        if let Ok(mut state) = slot.try_borrow_mut() {
            if let Some(state) = state.as_mut() {
                if state.roots.iter().any(|root| root.hwnd == id) {
                    state.fault.store(true, Ordering::Release);
                }
                state.invalidate_identity(id);
            }
        }
    });
}

struct Hooks {
    mouse: HHOOK,
    keyboard: HHOOK,
    windows: HWINEVENTHOOK,
}

impl Hooks {
    fn install() -> Result<Self, String> {
        // SAFETY: 空名称表示当前可执行模块, callback 遵循 Win32 ABI.
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return Err("hook module unavailable".into());
        }
        // SAFETY: 低级全局 hook 在安装线程的消息循环中执行.
        let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0) };
        if mouse.is_null() {
            return Err("mouse hook installation failed".into());
        }
        // SAFETY: keyboard 与 mouse 使用同一安装线程和 ABI 生命周期.
        let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0) };
        if keyboard.is_null() {
            // SAFETY: mouse 已安装且仍由本函数独占.
            unsafe {
                UnhookWindowsHookEx(mouse);
            }
            return Err("keyboard hook installation failed".into());
        }
        // SAFETY: out-of-context 通知在此线程的消息循环中执行.
        let windows = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_DESTROY,
                null_mut(),
                Some(window_destroyed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if windows.is_null() {
            // SAFETY: 两个 hook 均由本函数安装, 此前尚未卸载.
            unsafe {
                UnhookWindowsHookEx(mouse);
                UnhookWindowsHookEx(keyboard);
            }
            return Err("window lifecycle hook installation failed".into());
        }
        Ok(Self {
            mouse,
            keyboard,
            windows,
        })
    }
    fn uninstall(self) -> Result<(), String> {
        // SAFETY: 三个 hook 均在此线程安装, 本次消费并卸载一次.
        let (mouse, keyboard, windows) = unsafe {
            (
                UnhookWindowsHookEx(self.mouse),
                UnhookWindowsHookEx(self.keyboard),
                UnhookWinEvent(self.windows),
            )
        };
        if mouse == 0 || keyboard == 0 || windows == 0 {
            Err("hook removal failed".into())
        } else {
            Ok(())
        }
    }
}

pub(super) fn run(runtime: HookRuntime) -> Result<(), String> {
    let setup = || -> Result<State, String> {
        check_recording_desktop()?;
        if runtime.spec.targets.is_empty() {
            return Err("recording requires a target".into());
        }
        let mut state = State {
            roots: Arc::from([]),
            identities: Vec::new(),
            next_generation: 1,
            events: runtime.events.clone(),
            fault: runtime.fault.clone(),
            origin: runtime.origin,
            redaction: None,
            segment: 0,
            contexts: Vec::new(),
            next_contexts: Vec::new(),
            displays: DisplayTopology::new(),
        };
        let mut roots = Vec::new();
        for target in &runtime.spec.targets {
            let hwnd = target.hwnd as usize as HWND;
            let window = identity(hwnd).ok_or("invalid recording target")?;
            if window.pid != target.pid
                || root_window(hwnd) != Some(hwnd)
                || roots.iter().any(|root| same_instance(*root, window))
            {
                return Err("invalid recording target scope".into());
            }
            roots.push(state.register_identity(window));
        }
        state.roots = roots.into();
        Ok(state)
    };
    let state = match setup() {
        Ok(state) => state,
        Err(err) => {
            let _ = runtime.ready.send(Err(err.clone()));
            return Err(err);
        }
    };
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    let hooks = match Hooks::install() {
        Ok(hooks) => hooks,
        Err(err) => {
            STATE.with(|slot| {
                slot.borrow_mut().take();
            });
            let _ = runtime.ready.send(Err(err.clone()));
            return Err(err);
        }
    };
    let initial = STATE.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .expect("hook state installed")
            .observe()
    });
    if let Err(err) = initial {
        let _ = hooks.uninstall();
        STATE.with(|slot| {
            slot.borrow_mut().take();
        });
        let _ = runtime.ready.send(Err(err.clone()));
        return Err(err);
    }
    if runtime.ready.send(Ok(())).is_err() {
        let _ = hooks.uninstall();
        STATE.with(|slot| {
            slot.borrow_mut().take();
        });
        return Err("hook readiness receiver closed".into());
    }
    let mut result = Ok(());
    loop {
        let freezing = match runtime.commands.try_recv() {
            Ok(HookCommand::Freeze) => true,
            Ok(HookCommand::ParameterBoundary) => {
                STATE.with(|slot| {
                    slot.borrow_mut()
                        .as_mut()
                        .expect("hook state installed")
                        .close_redaction()
                });
                false
            }
            Err(TryRecvError::Disconnected) => {
                result = Err("hook command channel closed".into());
                true
            }
            Err(TryRecvError::Empty) => false,
        };
        let mut message = MSG::default();
        // SAFETY: message 可写, 空 HWND 选择本线程的消息队列.
        let mut pumped = 0usize;
        loop {
            // 尚未进入 callback 的 OS 消息不是已接纳事件, freeze 不能等全局输入安静.
            if pumped == 128 {
                break;
            }
            if unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } == 0 {
                break;
            }
            pumped += 1;
            // SAFETY: message 刚由本线程的 PeekMessageW 填充.
            unsafe {
                DispatchMessageW(&message);
            }
        }
        if freezing {
            if let Err(err) = STATE.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .expect("hook state installed")
                    .observe()
            }) {
                result = Err(err);
            }
            break;
        }
        if let Err(err) = STATE.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("hook state installed")
                .observe()
        }) {
            result = Err(err);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Err(err) = hooks.uninstall() {
        result = Err(err);
    }
    STATE.with(|slot| {
        if let Some(mut state) = slot.borrow_mut().take() {
            state.close_redaction();
        }
    });
    if runtime.fault.load(Ordering::Acquire) {
        return Err("hook capture fault".into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    fn window(hwnd: u64) -> WindowIdentity {
        WindowIdentity {
            pid: 1,
            hwnd,
            process_start: 9,
            generation: hwnd,
        }
    }
    fn state() -> (State, Receiver<HookEvent>) {
        let (events, received) = sync_channel(32);
        (
            State {
                roots: Arc::from([]),
                identities: vec![],
                next_generation: 1,
                events,
                fault: Arc::new(AtomicBool::new(false)),
                origin: Instant::now(),
                redaction: None,
                segment: 0,
                contexts: vec![],
                next_contexts: vec![],
                displays: DisplayTopology::new(),
            },
            received,
        )
    }
    #[test]
    fn owner_chain_excludes_siblings_and_stops_on_cycles() {
        let roots = [window(10)];
        assert!(owner_chain_authorized(12, &roots, |w| match w {
            12 => Some(11),
            11 => Some(10),
            _ => None,
        }));
        assert!(!owner_chain_authorized(13, &roots, |_| None));
        assert!(!owner_chain_authorized(13, &roots, |w| Some(if w == 13 {
            14
        } else {
            13
        })));
    }
    #[test]
    fn repeated_keys_only_emit_redaction_boundaries() {
        let (mut state, events) = state();
        state.keyboard(Some(window(1)));
        for _ in 0..100 {
            state.keyboard(Some(window(1)));
        }
        state.keyboard(Some(window(2)));
        state.close_redaction();
        let inputs: Vec<_> = events.try_iter().map(|event| event.input).collect();
        assert_eq!(
            inputs,
            vec![
                CapturedInput::Redaction {
                    window: Some(window(1)),
                    segment: 1,
                    active: true
                },
                CapturedInput::Redaction {
                    window: Some(window(1)),
                    segment: 1,
                    active: false
                },
                CapturedInput::Redaction {
                    window: Some(window(2)),
                    segment: 2,
                    active: true
                },
                CapturedInput::Redaction {
                    window: Some(window(2)),
                    segment: 2,
                    active: false
                }
            ]
        );
    }
    #[test]
    fn focus_loss_closes_segment_and_self_marker_is_filtered() {
        let (mut state, events) = state();
        state.keyboard(Some(window(1)));
        state.keyboard(None);
        state.keyboard(None);
        assert_eq!(events.try_iter().count(), 2);
        assert!(self_event(SELF_EVENT_MARKER as usize));
        assert!(!self_event((SELF_EVENT_MARKER as usize).wrapping_add(1)));
    }
    #[test]
    fn destroyed_dialog_gets_new_generation_but_destroyed_root_fails() {
        let (mut state, _) = state();
        let root = state.register_identity(window(101));
        let dialog = state.register_identity(window(102));
        state.roots = vec![root].into();
        let evidence = state.evidence(dialog).expect("dialog evidence");
        assert!(evidence.live());
        let fault = state.fault.clone();
        STATE.with(|slot| *slot.borrow_mut() = Some(state));
        // SAFETY: 此 callback 只比较数值型 HWND, 不解引用它.
        unsafe {
            window_destroyed(
                null_mut(),
                EVENT_OBJECT_DESTROY,
                102usize as HWND,
                OBJID_WINDOW,
                0,
                0,
                0,
            );
        }
        assert!(!fault.load(Ordering::Acquire));
        assert!(!evidence.live());
        let replacement = STATE.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("hook state installed")
                .register_identity(window(102))
        });
        assert_ne!(dialog.generation, replacement.generation);
        let replacement_evidence = STATE.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("hook state installed")
                .evidence(replacement)
                .expect("replacement evidence")
        });
        assert!(replacement_evidence.live());
        // SAFETY: 此 callback 只比较数值型 HWND, 不解引用它.
        unsafe {
            window_destroyed(
                null_mut(),
                EVENT_OBJECT_DESTROY,
                101usize as HWND,
                OBJID_WINDOW,
                0,
                0,
                0,
            );
        }
        assert!(fault.load(Ordering::Acquire));
        assert!(!replacement_evidence.live());
        STATE.with(|slot| {
            slot.borrow_mut().take();
        });
    }
    #[test]
    fn pointer_and_explicit_boundary_close_segments() {
        let (mut state, events) = state();
        let target = state.register_identity(window(1));
        state.keyboard(Some(target));
        state.mouse_down(target, 3, 4, 1, false);
        state.keyboard(Some(target));
        state.close_redaction();
        state.keyboard(Some(target));
        state.close_redaction();
        let inputs: Vec<_> = events.try_iter().map(|event| event.input).collect();
        assert_eq!(
            inputs,
            vec![
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 1,
                    active: true
                },
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 1,
                    active: false
                },
                CapturedInput::MouseDown {
                    window: target,
                    x: 3,
                    y: 4,
                    button: 1,
                    injected: false
                },
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 2,
                    active: true
                },
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 2,
                    active: false
                },
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 3,
                    active: true
                },
                CapturedInput::Redaction {
                    window: Some(target),
                    segment: 3,
                    active: false
                },
            ]
        );
    }
    #[test]
    fn overflow_is_sticky_even_when_queue_recovers() {
        let (sender, receiver) = sync_channel(1);
        let (mut state, _) = state();
        state.events = sender;
        state.keyboard(Some(window(1)));
        state.close_redaction();
        assert!(state.fault.load(Ordering::Acquire));
        assert!(receiver.try_recv().is_ok());
        state.keyboard(Some(window(2)));
        assert!(state.fault.load(Ordering::Acquire));
    }
}
