use super::*;
use crate::control_protocol::{KeyResponseMode, DEFAULT_KEY_HOLD_MS};
use std::{
    collections::BTreeMap,
    ffi::CString,
    io::{self, Write},
    os::raw::{c_char, c_int, c_void},
    process::{Command, Stdio},
    ptr, thread,
    time::Duration,
};

type Boolean = u8;
type CFIndex = isize;
type CFTypeID = usize;
type CFTypeRef = *const c_void;
type CFAllocatorRef = CFTypeRef;
type CFStringRef = CFTypeRef;
type CFArrayRef = CFTypeRef;
type CFDictionaryRef = CFTypeRef;
type CFNumberRef = CFTypeRef;
type CFBooleanRef = CFTypeRef;
type AXUIElementRef = CFTypeRef;
type AXValueRef = CFTypeRef;
type AXError = i32;
type CGEventSourceRef = CFTypeRef;
type CGEventRef = CFTypeRef;
type CGKeyCode = u16;
type UniChar = u16;

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT32: i32 = 3;
const CF_NUMBER_DOUBLE: i32 = 13;
const AX_SCROLL_PAGE_FRACTION: f64 = 0.25;
const AX_SUCCESS: AXError = 0;
const AX_ERROR_FAILURE: AXError = -25200;
const AX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;
const AX_ERROR_CANNOT_COMPLETE: AXError = -25204;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: AXError = -25205;
const AX_ERROR_ACTION_UNSUPPORTED: AXError = -25206;
const AX_ERROR_NOT_IMPLEMENTED: AXError = -25208;
const AX_ERROR_API_DISABLED: AXError = -25211;
const AX_ERROR_NO_VALUE: AXError = -25212;
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;
const CG_WINDOW_ON_SCREEN_ONLY: u32 = 1 << 0;
const CG_WINDOW_EXCLUDE_DESKTOP: u32 = 1 << 4;
const CG_EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;
const KEYCODE_BACKSPACE: u16 = 0x33;
const KEYCODE_TAB: u16 = 0x30;
const KEYCODE_RETURN: u16 = 0x24;
const KEYCODE_ESCAPE: u16 = 0x35;
const KEYCODE_SPACE: u16 = 0x31;
const KEYCODE_FORWARD_DELETE: u16 = 0x75;
const KEYCODE_HOME: u16 = 0x73;
const KEYCODE_END: u16 = 0x77;
const KEYCODE_PAGE_UP: u16 = 0x74;
const KEYCODE_PAGE_DOWN: u16 = 0x79;
const KEYCODE_LEFT_ARROW: u16 = 0x7B;
const KEYCODE_RIGHT_ARROW: u16 = 0x7C;
const KEYCODE_DOWN_ARROW: u16 = 0x7D;
const KEYCODE_UP_ARROW: u16 = 0x7E;
const KEYCODE_LEFT_COMMAND: u16 = 0x37;
const KEYCODE_RIGHT_COMMAND: u16 = 0x36;
const KEYCODE_LEFT_SHIFT: u16 = 0x38;
const KEYCODE_RIGHT_SHIFT: u16 = 0x3C;
const KEYCODE_LEFT_OPTION: u16 = 0x3A;
const KEYCODE_RIGHT_OPTION: u16 = 0x3D;
const KEYCODE_LEFT_CONTROL: u16 = 0x3B;
const KEYCODE_RIGHT_CONTROL: u16 = 0x3E;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFBooleanRef;
    static kCGWindowBounds: CFStringRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowOwnerName: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;

    fn AXIsProcessTrusted() -> Boolean;
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut Boolean,
    ) -> AXError;
    fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXValueGetTypeID() -> CFTypeID;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> Boolean;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFBooleanGetValue(boolean: CFBooleanRef) -> Boolean;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(value: CFTypeRef) -> CFTypeID;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFNumberGetValue(number: CFNumberRef, number_type: i32, value_ptr: *mut c_void) -> Boolean;
    fn CFNumberCreate(
        allocator: CFAllocatorRef,
        number_type: i32,
        value_ptr: *const c_void,
    ) -> CFNumberRef;
    fn CFRelease(value: CFTypeRef);
    fn CFRetain(value: CFTypeRef) -> CFTypeRef;
    fn CFStringCreateWithCString(
        alloc: CFTypeRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> Boolean;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: CGKeyCode,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventKeyboardSetUnicodeString(
        event: CGEventRef,
        string_length: usize,
        unicode_string: *const UniChar,
    );
    fn CGEventPostToPid(pid: c_int, event: CGEventRef);
    fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

#[derive(Debug)]
struct CfOwned(CFTypeRef);

impl CfOwned {
    fn new(value: CFTypeRef) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }

    unsafe fn retain(value: CFTypeRef) -> Option<Self> {
        if value.is_null() {
            return None;
        }
        Some(Self(CFRetain(value)))
    }

    fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CFRelease(self.0);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct VisibleWindow {
    process_name: String,
    title: Option<String>,
    rect: Option<AxRect>,
}

#[derive(Debug, Default)]
struct BuildState {
    element_count: usize,
    truncated: bool,
}

pub(super) fn snapshot(request: &AxTreeRequest) -> io::Result<AxSnapshot> {
    ensure_trusted()?;

    let visible_by_pid = visible_windows_by_pid();
    let mut windows = Vec::new();
    let mut state = BuildState::default();

    for pid in visible_by_pid.keys().copied() {
        let app = unsafe { CfOwned::new(AXUIElementCreateApplication(pid)) };
        let Some(app) = app else {
            continue;
        };

        let process_name = visible_by_pid
            .get(&pid)
            .and_then(|items| items.first())
            .map(|window| window.process_name.clone())
            .unwrap_or_else(|| pid.to_string());

        let Some(ax_windows) = copy_attribute(app.as_ptr(), "AXWindows")? else {
            windows.extend(fallback_visible_windows(
                pid,
                &process_name,
                visible_by_pid.get(&pid),
            ));
            continue;
        };

        let count = unsafe { CFArrayGetCount(ax_windows.as_ptr()) };
        for window_index in 0..count {
            let window_ref = unsafe { CFArrayGetValueAtIndex(ax_windows.as_ptr(), window_index) };
            if window_ref.is_null() {
                continue;
            }
            let fallback = visible_by_pid
                .get(&pid)
                .and_then(|items| items.get(window_index as usize));
            windows.push(build_window(
                pid,
                &process_name,
                window_index as usize,
                window_ref,
                fallback,
                request,
                &mut state,
            )?);
            if state.element_count >= usize::from(request.max_elements) {
                state.truncated = true;
                break;
            }
        }
    }

    Ok(AxSnapshot::complete("macos", windows, state.truncated))
}

pub(super) fn perform_action(request: &AxActionRequest) -> io::Result<AxPerformedActionReport> {
    ensure_trusted()?;

    let target_id = resolve_live_target_id(&request.target)?;
    perform_action_on_target_id(&target_id, request.action)?;
    Ok(AxPerformedActionReport::success(
        "macos-accessibility",
        Some(target_id),
        request.action,
    ))
}

pub(super) fn set_value(request: &AxSetValueRequest) -> io::Result<AxSetValueReport> {
    ensure_trusted()?;

    let target_id = resolve_live_target_id(&request.target)?;
    let element = retain_target_element(&target_id)?;
    ensure_ax_value_settable(element.as_ptr())?;

    let value_redacted = target_value_is_redacted(element.as_ptr())?;
    let current_value = copy_string_attr(element.as_ptr(), "AXValue")?;
    let final_value = build_final_ax_value(current_value, &request.value, request.mode)?;

    set_ax_value_string(element.as_ptr(), &final_value)?;
    Ok(AxSetValueReport::success(
        "macos-accessibility",
        Some(target_id),
        request.mode,
        value_redacted,
        value_redacted,
    ))
}

pub(super) fn deliver_key(request: &KeyRequest) -> io::Result<KeyDeliveryReport> {
    ensure_trusted()?;

    let (target_pid, window_id) = match request.delivery {
        KeyDelivery::Global => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "global @key 不应走 macOS targeted delivery 后端",
            ))
        }
        KeyDelivery::PidTargeted => (
            request
                .pid
                .ok_or_else(|| invalid_input("@key pid-targeted 缺少 pid"))?,
            None,
        ),
        KeyDelivery::WindowTargeted => {
            let window_id = request
                .window_id
                .clone()
                .ok_or_else(|| invalid_input("@key window-targeted 缺少 window_id"))?;
            let window_ref = retain_target_element(&window_id)?;
            focus_window_element(window_ref.as_ptr())?;
            let parsed = parse_target_id(&window_id)?;
            (parsed.pid, Some(window_id))
        }
    };

    post_key_request_to_pid(target_pid, request)?;
    Ok(KeyDeliveryReport::success(
        "macos-cg-event-post-to-pid",
        request,
        Some(target_pid),
        window_id,
    ))
}

pub(super) fn focus(request: &AxFocusRequest) -> io::Result<AxFocusReport> {
    ensure_trusted()?;

    if let Some(window_id) = &request.window_id {
        let window_ref = retain_target_element(window_id)?;
        focus_window_element(window_ref.as_ptr())?;
        return Ok(AxFocusReport::success(
            "macos-accessibility",
            None,
            Some(window_id.clone()),
            request.activate,
        ));
    }

    let target = request
        .target
        .as_ref()
        .ok_or_else(|| invalid_input("@ax-focus 缺少 target"))?;
    let target_id = resolve_live_target_id(target)?;
    let target_ref = retain_target_element(&target_id)?;
    let parsed = parse_target_id(&target_id)?;
    if parsed.path.is_empty() {
        focus_window_element(target_ref.as_ptr())?;
        return Ok(AxFocusReport::success(
            "macos-accessibility",
            None,
            Some(target_id),
            request.activate,
        ));
    }

    focus_element(target_ref.as_ptr())?;
    Ok(AxFocusReport::success(
        "macos-accessibility",
        Some(target_id),
        None,
        request.activate,
    ))
}

pub(super) fn scroll(request: &AxScrollRequest) -> io::Result<AxScrollReport> {
    ensure_trusted()?;

    let target_id = resolve_live_target_id(&request.target)?;
    prepare_text_target(&target_id)?;
    set_scrollbar_value_for_target(&target_id, request.direction, request.pages)?;

    Ok(AxScrollReport::success(
        "macos-accessibility",
        Some(target_id),
        request.direction,
        request.pages,
        0,
        "ax-scrollbar-value",
    ))
}

pub(super) fn type_text(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    ensure_trusted()?;

    match request.mode {
        TypeTextMode::AxValue => type_text_via_ax_value(request),
        TypeTextMode::TargetedKeyboard => type_text_via_targeted_keyboard(request),
        TypeTextMode::Clipboard => type_text_via_clipboard(request),
        TypeTextMode::Auto => match type_text_via_ax_value(request) {
            Ok(report) => Ok(report),
            Err(ax_err)
                if matches!(
                    ax_err.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
                ) =>
            {
                match type_text_via_targeted_keyboard(request) {
                    Ok(report) => Ok(report),
                    Err(keyboard_err)
                        if request.allow_clipboard
                            && matches!(
                                keyboard_err.kind(),
                                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
                            ) =>
                    {
                        type_text_via_clipboard(request)
                    }
                    Err(keyboard_err) => Err(remap_type_text_targeted_keyboard_error(keyboard_err)),
                }
            }
            Err(ax_err) => Err(remap_type_text_ax_value_error(ax_err)),
        },
    }
}

fn type_text_via_ax_value(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    let set_request = AxSetValueRequest {
        target: request.target.clone(),
        value: request.text.clone(),
        mode: AxValueSetMode::Replace,
    };
    let report = set_value(&set_request).map_err(remap_type_text_ax_value_error)?;
    Ok(TypeTextReport::ax_value_success(
        report.backend,
        report.target_id,
        request.mode,
    ))
}

fn type_text_via_targeted_keyboard(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    let target_id = resolve_live_target_id(&request.target)?;
    prepare_text_target(&target_id)?;
    let parsed = parse_target_id(&target_id)?;
    post_unicode_text_to_pid(parsed.pid, &request.text)?;
    Ok(TypeTextReport::targeted_keyboard_success(
        "macos-cg-event-post-to-pid",
        Some(target_id),
    ))
}

fn type_text_via_clipboard(request: &TypeTextRequest) -> io::Result<TypeTextReport> {
    if !request.allow_clipboard {
        return Err(invalid_input(
            "type-text clipboard 路径需要显式 `allow_clipboard:true`",
        ));
    }

    let target_id = resolve_live_target_id(&request.target)?;
    prepare_text_target(&target_id)?;
    let parsed = parse_target_id(&target_id)?;
    with_temporary_clipboard_text(&request.text, || {
        let paste_request = KeyRequest {
            key: "cmd+v".to_owned(),
            hold_ms: DEFAULT_KEY_HOLD_MS,
            mode: KeyMode::PressRelease,
            delivery: KeyDelivery::PidTargeted,
            pid: Some(parsed.pid),
            window_id: None,
            response_mode: KeyResponseMode::Structured,
        };
        post_key_request_to_pid(parsed.pid, &paste_request)
    })?;
    Ok(TypeTextReport::clipboard_success(
        "macos-clipboard+cg-event-post-to-pid",
        Some(target_id),
    ))
}

fn visible_windows_by_pid() -> BTreeMap<i32, Vec<VisibleWindow>> {
    let mut result = BTreeMap::<i32, Vec<VisibleWindow>>::new();
    let options = CG_WINDOW_ON_SCREEN_ONLY | CG_WINDOW_EXCLUDE_DESKTOP;
    let Some(windows) = (unsafe { CfOwned::new(CGWindowListCopyWindowInfo(options, 0)) }) else {
        return result;
    };

    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    for index in 0..count {
        let dict = unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), index) };
        if dict.is_null() {
            continue;
        }

        let Some(pid) = dictionary_i32(dict, unsafe { kCGWindowOwnerPID }) else {
            continue;
        };
        let layer = dictionary_i32(dict, unsafe { kCGWindowLayer }).unwrap_or(0);
        if layer != 0 {
            continue;
        }

        let process_name = dictionary_string(dict, unsafe { kCGWindowOwnerName })
            .unwrap_or_else(|| pid.to_string());
        let title = dictionary_string(dict, unsafe { kCGWindowName });
        let rect = dictionary_rect(dict, unsafe { kCGWindowBounds });

        result.entry(pid).or_default().push(VisibleWindow {
            process_name,
            title,
            rect,
        });
    }

    result
}

fn fallback_visible_windows(
    pid: i32,
    process_name: &str,
    visible: Option<&Vec<VisibleWindow>>,
) -> Vec<AxWindow> {
    visible
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, window)| AxWindow {
            id: format!("pid:{pid}/window:{index}"),
            pid,
            process_name: process_name.to_owned(),
            title: window.title.clone(),
            role: "AXWindow".to_owned(),
            subrole: None,
            rect: window.rect,
            focused: None,
            elements: Vec::new(),
        })
        .collect()
}

fn build_window(
    pid: i32,
    process_name: &str,
    window_index: usize,
    window_ref: AXUIElementRef,
    fallback: Option<&VisibleWindow>,
    request: &AxTreeRequest,
    state: &mut BuildState,
) -> io::Result<AxWindow> {
    let title =
        copy_string_attr(window_ref, "AXTitle")?.or_else(|| fallback.and_then(|w| w.title.clone()));
    let role = copy_string_attr(window_ref, "AXRole")?.unwrap_or_else(|| "AXWindow".to_owned());
    let subrole = copy_string_attr(window_ref, "AXSubrole")?;
    let rect = copy_ax_rect(window_ref)?.or_else(|| fallback.and_then(|w| w.rect));
    let focused = copy_bool_attr(window_ref, "AXFocused")?;
    let mut elements = Vec::new();

    if request.depth > 0 {
        if let Some(children) = copy_attribute(window_ref, "AXChildren")? {
            let count = unsafe { CFArrayGetCount(children.as_ptr()) };
            for index in 0..count {
                if state.element_count >= usize::from(request.max_elements) {
                    state.truncated = true;
                    break;
                }
                let child = unsafe { CFArrayGetValueAtIndex(children.as_ptr(), index) };
                if child.is_null() {
                    continue;
                }
                state.element_count += 1;
                let path = vec![index as usize];
                elements.push(build_element(
                    pid,
                    window_index,
                    child,
                    path,
                    request.depth.saturating_sub(1),
                    request,
                    state,
                )?);
            }
        }
    }

    Ok(AxWindow {
        id: format!("pid:{pid}/window:{window_index}"),
        pid,
        process_name: process_name.to_owned(),
        title,
        role,
        subrole,
        rect,
        focused,
        elements,
    })
}

fn build_element(
    pid: i32,
    window_index: usize,
    element_ref: AXUIElementRef,
    path: Vec<usize>,
    remaining_depth: u8,
    request: &AxTreeRequest,
    state: &mut BuildState,
) -> io::Result<AxElement> {
    let role = copy_string_attr(element_ref, "AXRole")?.unwrap_or_else(|| "AXUnknown".to_owned());
    let subrole = copy_string_attr(element_ref, "AXSubrole")?;
    let name = copy_string_attr(element_ref, "AXTitle")?;
    let description = copy_string_attr(element_ref, "AXDescription")?;
    let rect = copy_ax_rect(element_ref)?;
    let enabled = copy_bool_attr(element_ref, "AXEnabled")?;
    let actions = copy_action_names(element_ref)?;
    let value_redacted = looks_like_secure_element(&role, subrole.as_deref());
    let value = if request.include_values && !value_redacted {
        copy_string_attr(element_ref, "AXValue")?
    } else {
        None
    };
    let mut children = Vec::new();

    if remaining_depth > 0 {
        if let Some(child_array) = copy_attribute(element_ref, "AXChildren")? {
            let count = unsafe { CFArrayGetCount(child_array.as_ptr()) };
            for index in 0..count {
                if state.element_count >= usize::from(request.max_elements) {
                    state.truncated = true;
                    break;
                }
                let child = unsafe { CFArrayGetValueAtIndex(child_array.as_ptr(), index) };
                if child.is_null() {
                    continue;
                }
                state.element_count += 1;
                let mut child_path = path.clone();
                child_path.push(index as usize);
                children.push(build_element(
                    pid,
                    window_index,
                    child,
                    child_path,
                    remaining_depth - 1,
                    request,
                    state,
                )?);
            }
        }
    }

    Ok(AxElement {
        id: format!(
            "pid:{pid}/window:{window_index}/path:{}",
            path.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(".")
        ),
        role,
        subrole,
        name,
        value,
        value_redacted,
        description,
        rect,
        enabled,
        actions,
        ax_path: path,
        children,
    })
}

fn resolve_live_target_id(target: &AxTarget) -> io::Result<String> {
    match &target.id {
        Some(id) => Ok(id.clone()),
        None => {
            let lookup_request = AxTreeRequest {
                depth: 8,
                max_elements: 5000,
                include_values: false,
                ..AxTreeRequest::default()
            };
            let snapshot = snapshot(&lookup_request)?;
            resolve_target_id_in_snapshot(&snapshot, target)
        }
    }
}

fn retain_target_element(target_id: &str) -> io::Result<CfOwned> {
    let parsed = parse_target_id(target_id)?;
    let app = unsafe { CfOwned::new(AXUIElementCreateApplication(parsed.pid)) }
        .ok_or_else(|| invalid_input("AX target 无法创建目标应用 AX element"))?;
    let windows = copy_attribute(app.as_ptr(), "AXWindows")?
        .ok_or_else(|| invalid_input("AX target 应用没有 AXWindows"))?;
    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    if parsed.window_index >= count as usize {
        return Err(invalid_input(format!(
            "AX target window index 已失效: {}",
            parsed.window_index
        )));
    }

    let window_ref =
        unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), parsed.window_index as CFIndex) };
    let mut current = unsafe { CfOwned::retain(window_ref) }
        .ok_or_else(|| invalid_input("AX target window 已失效"))?;

    for step in parsed.path {
        let children = copy_attribute(current.as_ptr(), "AXChildren")?
            .ok_or_else(|| invalid_input("AX target 路径已失效"))?;
        let count = unsafe { CFArrayGetCount(children.as_ptr()) };
        if step >= count as usize {
            return Err(invalid_input(format!("AX target 路径 step 已失效: {step}")));
        }
        let child = unsafe { CFArrayGetValueAtIndex(children.as_ptr(), step as CFIndex) };
        current = unsafe { CfOwned::retain(child) }
            .ok_or_else(|| invalid_input("AX target 元素已失效"))?;
    }

    Ok(current)
}

fn perform_action_on_target_id(target_id: &str, action: AxActionName) -> io::Result<()> {
    let current = retain_target_element(target_id)?;
    let available_actions = copy_action_names(current.as_ptr())?;
    let action_name = action.protocol_str();
    if !available_actions.iter().any(|item| item == action_name) {
        return Err(invalid_input(format!(
            "目标 AX 元素不支持动作 {action_name}"
        )));
    }
    with_cf_string(action_name, |action_ref| unsafe {
        map_ax_action_error(
            AXUIElementPerformAction(current.as_ptr(), action_ref),
            action_name,
        )
    })
}

fn ensure_ax_value_settable(element: AXUIElementRef) -> io::Result<()> {
    with_cf_string("AXValue", |attribute| {
        let mut settable = 0;
        let error = unsafe { AXUIElementIsAttributeSettable(element, attribute, &mut settable) };
        match error {
            AX_SUCCESS => {
                if settable != 0 {
                    Ok(())
                } else {
                    Err(invalid_input("目标 AX 元素的 AXValue 不可写"))
                }
            }
            AX_ERROR_ATTRIBUTE_UNSUPPORTED => Err(invalid_input("目标 AX 元素不支持 AXValue")),
            AX_ERROR_API_DISABLED => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "macOS Accessibility API 当前不可用或未授权",
            )),
            code => Err(io::Error::other(format!(
                "检查 AXValue 是否可写失败: AXError {code}"
            ))),
        }
    })
}

fn target_value_is_redacted(element: AXUIElementRef) -> io::Result<bool> {
    let role = copy_string_attr(element, "AXRole")?.unwrap_or_else(|| "AXUnknown".to_owned());
    let subrole = copy_string_attr(element, "AXSubrole")?;
    Ok(looks_like_secure_element(&role, subrole.as_deref()))
}

fn build_final_ax_value(
    current_value: Option<String>,
    request_value: &str,
    mode: AxValueSetMode,
) -> io::Result<String> {
    match mode {
        AxValueSetMode::Replace => Ok(request_value.to_owned()),
        AxValueSetMode::Append => {
            let current_value = current_value
                .ok_or_else(|| invalid_input("目标 AX 元素当前 AXValue 不可读,无法执行 append"))?;
            Ok(format!("{current_value}{request_value}"))
        }
    }
}

fn set_ax_value_string(element: AXUIElementRef, value: &str) -> io::Result<()> {
    with_cf_string("AXValue", |attribute| {
        with_cf_string(value, |cf_value| unsafe {
            map_ax_set_value_error(AXUIElementSetAttributeValue(element, attribute, cf_value))
        })
    })
}

fn prepare_text_target(target_id: &str) -> io::Result<()> {
    let target_ref = retain_target_element(target_id)?;
    let parsed = parse_target_id(target_id)?;
    if parsed.path.is_empty() {
        focus_window_element(target_ref.as_ptr())
    } else {
        focus_element(target_ref.as_ptr())
    }
}

fn focus_element(element: AXUIElementRef) -> io::Result<()> {
    set_bool_attribute(element, "AXFocused", true).map_err(|err| {
        if err.kind() == io::ErrorKind::InvalidInput {
            invalid_input(format!("目标 AX 元素无法获得焦点: {err}"))
        } else {
            err
        }
    })
}

fn focus_window_element(window_ref: AXUIElementRef) -> io::Result<()> {
    let mut focused = false;
    match set_bool_attribute(window_ref, "AXMain", true) {
        Ok(()) => focused = true,
        Err(err) if err.kind() == io::ErrorKind::InvalidInput => {}
        Err(err) => return Err(err),
    }
    match set_bool_attribute(window_ref, "AXFocused", true) {
        Ok(()) => focused = true,
        Err(err) if err.kind() == io::ErrorKind::InvalidInput => {}
        Err(err) => return Err(err),
    }

    if focused {
        Ok(())
    } else {
        Err(invalid_input(
            "目标窗口不支持 AXMain/AXFocused,无法在不激活桌面的前提下聚焦",
        ))
    }
}

fn set_scrollbar_value_for_target(
    target_id: &str,
    direction: AxScrollDirection,
    pages: u16,
) -> io::Result<()> {
    let parsed = parse_target_id(target_id)?;
    let window_id = format!("pid:{}/window:{}", parsed.pid, parsed.window_index);
    let window = retain_target_element(&window_id)?;
    let scrollbar = find_scrollbar_element(window.as_ptr(), direction)?.ok_or_else(|| {
        invalid_input(format!("目标窗口没有可用于 {:?} 的 AXScrollBar", direction,))
    })?;

    ensure_ax_value_settable(scrollbar.as_ptr())?;
    let current_value = copy_number_attr(scrollbar.as_ptr(), "AXValue")?
        .ok_or_else(|| invalid_input("目标 AXScrollBar 当前 AXValue 不可读"))?;
    let next_value = next_scrollbar_value(current_value, direction, pages);
    set_ax_number_value(scrollbar.as_ptr(), next_value)
}

fn find_scrollbar_element(
    element: AXUIElementRef,
    direction: AxScrollDirection,
) -> io::Result<Option<CfOwned>> {
    let role = copy_string_attr(element, "AXRole")?.unwrap_or_default();
    if role == "AXScrollBar" && scrollbar_matches_direction(element, direction)? {
        return unsafe { CfOwned::retain(element) }
            .map(Some)
            .ok_or_else(|| invalid_input("AXScrollBar 已失效,无法执行非鼠标滚动"));
    }

    let Some(children) = copy_attribute(element, "AXChildren")? else {
        return Ok(None);
    };
    let count = unsafe { CFArrayGetCount(children.as_ptr()) };
    for index in 0..count {
        let child = unsafe { CFArrayGetValueAtIndex(children.as_ptr(), index) };
        if child.is_null() {
            continue;
        }
        if let Some(scrollbar) = find_scrollbar_element(child, direction)? {
            return Ok(Some(scrollbar));
        }
    }
    Ok(None)
}

fn scrollbar_matches_direction(
    element: AXUIElementRef,
    direction: AxScrollDirection,
) -> io::Result<bool> {
    let Some(rect) = copy_ax_rect(element)? else {
        return Ok(true);
    };
    Ok(match direction {
        AxScrollDirection::Up | AxScrollDirection::Down => rect.height >= rect.width,
        AxScrollDirection::Left | AxScrollDirection::Right => rect.width > rect.height,
    })
}

fn next_scrollbar_value(current_value: f64, direction: AxScrollDirection, pages: u16) -> f64 {
    // macOS AXScrollBar 的 AXValue 通常是 0..1 的比例值。
    // `pages` 是协议抽象,这里用温和的比例步进映射,避免一次滚到尽头。
    let delta = f64::from(pages) * AX_SCROLL_PAGE_FRACTION;
    match direction {
        AxScrollDirection::Up | AxScrollDirection::Left => (current_value - delta).max(0.0),
        AxScrollDirection::Down | AxScrollDirection::Right => (current_value + delta).min(1.0),
    }
}

fn set_ax_number_value(element: AXUIElementRef, value: f64) -> io::Result<()> {
    with_cf_string("AXValue", |attribute| {
        let number_ref = unsafe {
            CfOwned::new(CFNumberCreate(
                ptr::null(),
                CF_NUMBER_DOUBLE,
                (&value as *const f64).cast(),
            ))
        }
        .ok_or_else(|| io::Error::other("创建 AXScrollBar 数值失败"))?;
        unsafe {
            map_ax_set_value_error(AXUIElementSetAttributeValue(
                element,
                attribute,
                number_ref.as_ptr(),
            ))
        }
    })
}

fn set_bool_attribute(element: AXUIElementRef, attr: &str, value: bool) -> io::Result<()> {
    with_cf_string(attr, |attribute| unsafe {
        let bool_ref = if value { kCFBooleanTrue } else { ptr::null() };
        map_ax_bool_set_error(
            attr,
            AXUIElementSetAttributeValue(element, attribute, bool_ref),
        )
    })
}

fn post_key_request_to_pid(pid: i32, request: &KeyRequest) -> io::Result<()> {
    let chord = parse_mac_key_chord(&request.key, request.delivery)?;
    match request.mode {
        KeyMode::PressRelease => {
            for modifier in &chord.modifiers {
                post_key_event_to_pid(pid, *modifier, true, None)?;
            }
            post_main_key_event_to_pid(pid, &chord.main, true)?;
            if request.hold_ms > 0 {
                thread::sleep(Duration::from_millis(request.hold_ms));
            }
            post_main_key_event_to_pid(pid, &chord.main, false)?;
            for modifier in chord.modifiers.iter().rev() {
                post_key_event_to_pid(pid, *modifier, false, None)?;
            }
        }
        KeyMode::Press => {
            for modifier in &chord.modifiers {
                post_key_event_to_pid(pid, *modifier, true, None)?;
            }
            post_main_key_event_to_pid(pid, &chord.main, true)?;
        }
        KeyMode::Release => {
            post_main_key_event_to_pid(pid, &chord.main, false)?;
            for modifier in chord.modifiers.iter().rev() {
                post_key_event_to_pid(pid, *modifier, false, None)?;
            }
        }
    }
    Ok(())
}

fn post_unicode_text_to_pid(pid: i32, text: &str) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let unicode = text.encode_utf16().collect::<Vec<_>>();
    post_key_event_to_pid(pid, 0, true, Some(&unicode))?;
    post_key_event_to_pid(pid, 0, false, Some(&unicode))?;
    Ok(())
}

fn with_temporary_clipboard_text(
    text: &str,
    run: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let previous = Command::new("pbpaste")
        .output()
        .map_err(|err| io::Error::other(format!("读取系统剪贴板失败: {err}")))?;
    write_clipboard_text(text)?;
    let run_result = run();
    let restore_result = write_clipboard_bytes(&previous.stdout);
    run_result?;
    restore_result?;
    Ok(())
}

fn write_clipboard_text(text: &str) -> io::Result<()> {
    write_clipboard_bytes(text.as_bytes())
}

fn write_clipboard_bytes(bytes: &[u8]) -> io::Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| io::Error::other(format!("写入系统剪贴板失败: {err}")))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(bytes)
            .map_err(|err| io::Error::other(format!("写入系统剪贴板失败: {err}")))?;
    }
    let status = child
        .wait()
        .map_err(|err| io::Error::other(format!("等待 pbcopy 结束失败: {err}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("pbcopy 退出失败: {status}")))
    }
}

#[derive(Debug, Clone)]
struct MacKeyChord {
    modifiers: Vec<CGKeyCode>,
    main: MacKeyMain,
}

#[derive(Debug, Clone)]
enum MacKeyMain {
    KeyCode(CGKeyCode),
    Unicode(Vec<UniChar>),
}

fn parse_mac_key_chord(key: &str, delivery: KeyDelivery) -> io::Result<MacKeyChord> {
    let mut tokens = key
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some(main) = tokens.pop() else {
        return Err(invalid_input("@key payload 不能为空"));
    };

    let mut modifiers = Vec::new();
    for token in tokens {
        modifiers.push(mac_modifier_keycode(token)?);
    }

    let main = mac_main_key(main, !modifiers.is_empty(), delivery)?;
    Ok(MacKeyChord { modifiers, main })
}

fn mac_modifier_keycode(token: &str) -> io::Result<CGKeyCode> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Ok(KEYCODE_LEFT_CONTROL),
        "left-ctrl" | "left-control" | "lctrl" | "lcontrol" => Ok(KEYCODE_LEFT_CONTROL),
        "right-ctrl" | "right-control" | "rctrl" | "rcontrol" => Ok(KEYCODE_RIGHT_CONTROL),
        "shift" | "left-shift" | "lshift" => Ok(KEYCODE_LEFT_SHIFT),
        "right-shift" | "rshift" => Ok(KEYCODE_RIGHT_SHIFT),
        "cmd" | "command" | "meta" | "super" => Ok(KEYCODE_LEFT_COMMAND),
        "left-cmd" | "left-command" | "left-meta" | "left-super" => Ok(KEYCODE_LEFT_COMMAND),
        "right-cmd" | "right-command" | "right-meta" | "right-super" => Ok(KEYCODE_RIGHT_COMMAND),
        "alt" | "option" | "left-alt" | "left-option" => Ok(KEYCODE_LEFT_OPTION),
        "right-alt" | "right-option" => Ok(KEYCODE_RIGHT_OPTION),
        _ => Err(invalid_input(format!(
            "macOS targeted @key 不支持的修饰键: {token}"
        ))),
    }
}

fn mac_main_key(token: &str, has_modifiers: bool, delivery: KeyDelivery) -> io::Result<MacKeyMain> {
    if let Some(keycode) = mac_named_keycode(token) {
        return Ok(MacKeyMain::KeyCode(keycode));
    }
    if token.chars().count() == 1 {
        let ch = token.chars().next().expect("single char should exist");
        if let Some(keycode) = mac_ascii_keycode(ch) {
            return Ok(MacKeyMain::KeyCode(keycode));
        }
        if !has_modifiers {
            return Ok(MacKeyMain::Unicode(ch.encode_utf16(&mut [0; 2]).to_vec()));
        }
    }
    Err(invalid_input(format!(
        "macOS {:?} 当前只支持 ASCII 字母/数字/空格、常见 named keys 和常见修饰键组合: {token}",
        delivery
    )))
}

fn mac_named_keycode(token: &str) -> Option<CGKeyCode> {
    match token.to_ascii_lowercase().as_str() {
        "f1" => Some(0x7A),
        "f2" => Some(0x78),
        "f3" => Some(0x63),
        "f4" => Some(0x76),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f10" => Some(0x6D),
        "f11" => Some(0x67),
        "f12" => Some(0x6F),
        "enter" | "return" => Some(KEYCODE_RETURN),
        "tab" => Some(KEYCODE_TAB),
        "space" => Some(KEYCODE_SPACE),
        "esc" | "escape" => Some(KEYCODE_ESCAPE),
        "backspace" => Some(KEYCODE_BACKSPACE),
        "delete" => Some(KEYCODE_FORWARD_DELETE),
        "home" => Some(KEYCODE_HOME),
        "end" => Some(KEYCODE_END),
        "pageup" => Some(KEYCODE_PAGE_UP),
        "pagedown" => Some(KEYCODE_PAGE_DOWN),
        "up" => Some(KEYCODE_UP_ARROW),
        "down" => Some(KEYCODE_DOWN_ARROW),
        "left" => Some(KEYCODE_LEFT_ARROW),
        "right" => Some(KEYCODE_RIGHT_ARROW),
        _ => None,
    }
}

fn mac_ascii_keycode(ch: char) -> Option<CGKeyCode> {
    match ch.to_ascii_lowercase() {
        'a' => Some(0x00),
        'b' => Some(0x0B),
        'c' => Some(0x08),
        'd' => Some(0x02),
        'e' => Some(0x0E),
        'f' => Some(0x03),
        'g' => Some(0x05),
        'h' => Some(0x04),
        'i' => Some(0x22),
        'j' => Some(0x26),
        'k' => Some(0x28),
        'l' => Some(0x25),
        'm' => Some(0x2E),
        'n' => Some(0x2D),
        'o' => Some(0x1F),
        'p' => Some(0x23),
        'q' => Some(0x0C),
        'r' => Some(0x0F),
        's' => Some(0x01),
        't' => Some(0x11),
        'u' => Some(0x20),
        'v' => Some(0x09),
        'w' => Some(0x0D),
        'x' => Some(0x07),
        'y' => Some(0x10),
        'z' => Some(0x06),
        '0' => Some(0x1D),
        '1' => Some(0x12),
        '2' => Some(0x13),
        '3' => Some(0x14),
        '4' => Some(0x15),
        '5' => Some(0x17),
        '6' => Some(0x16),
        '7' => Some(0x1A),
        '8' => Some(0x1C),
        '9' => Some(0x19),
        ' ' => Some(KEYCODE_SPACE),
        _ => None,
    }
}

fn post_main_key_event_to_pid(pid: i32, main: &MacKeyMain, key_down: bool) -> io::Result<()> {
    match main {
        MacKeyMain::KeyCode(keycode) => post_key_event_to_pid(pid, *keycode, key_down, None),
        MacKeyMain::Unicode(text) => post_key_event_to_pid(pid, 0, key_down, Some(text)),
    }
}

fn post_key_event_to_pid(
    pid: i32,
    keycode: CGKeyCode,
    key_down: bool,
    unicode: Option<&[UniChar]>,
) -> io::Result<()> {
    let source = create_event_source()?;
    let event = unsafe {
        CfOwned::new(CGEventCreateKeyboardEvent(
            source.as_ptr(),
            keycode,
            key_down,
        ))
    }
    .ok_or_else(|| io::Error::other("创建 macOS keyboard CGEvent 失败"))?;
    if let Some(unicode) = unicode {
        unsafe {
            CGEventKeyboardSetUnicodeString(event.as_ptr(), unicode.len(), unicode.as_ptr());
        }
    }
    unsafe {
        CGEventPostToPid(pid, event.as_ptr());
    }
    Ok(())
}

fn create_event_source() -> io::Result<CfOwned> {
    unsafe { CfOwned::new(CGEventSourceCreate(CG_EVENT_SOURCE_STATE_COMBINED_SESSION)) }
        .ok_or_else(|| io::Error::other("创建 macOS CGEventSource 失败"))
}

#[derive(Debug)]
struct ParsedTargetId {
    pid: i32,
    window_index: usize,
    path: Vec<usize>,
}

fn parse_target_id(target_id: &str) -> io::Result<ParsedTargetId> {
    let parts = target_id.split('/').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err(invalid_input(format!("AX target id 格式非法: {target_id}")));
    }

    let pid = parts[0]
        .strip_prefix("pid:")
        .ok_or_else(|| invalid_input(format!("AX target id 缺少 pid: {target_id}")))?
        .parse::<i32>()
        .map_err(|_| invalid_input(format!("AX target id pid 非法: {target_id}")))?;
    let window_index = parts[1]
        .strip_prefix("window:")
        .ok_or_else(|| invalid_input(format!("AX target id 缺少 window: {target_id}")))?
        .parse::<usize>()
        .map_err(|_| invalid_input(format!("AX target id window 非法: {target_id}")))?;

    let path = match parts.get(2) {
        Some(path) => {
            let path = path
                .strip_prefix("path:")
                .ok_or_else(|| invalid_input(format!("AX target id path 非法: {target_id}")))?;
            if path.is_empty() || path.split('.').any(str::is_empty) {
                return Err(invalid_input(format!(
                    "AX target id path step 非法: {target_id}"
                )));
            }
            path.split('.')
                .map(|item| {
                    item.parse::<usize>().map_err(|_| {
                        invalid_input(format!("AX target id path step 非法: {target_id}"))
                    })
                })
                .collect::<io::Result<Vec<_>>>()?
        }
        None => Vec::new(),
    };

    Ok(ParsedTargetId {
        pid,
        window_index,
        path,
    })
}

fn ensure_trusted() -> io::Result<()> {
    if unsafe { AXIsProcessTrusted() } != 0 {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS Accessibility 权限不足: 请给实际执行 rdog daemon/control 的进程授予辅助功能权限,授权后重启该进程",
    ))
}

fn copy_attribute(element: AXUIElementRef, attr: &str) -> io::Result<Option<CfOwned>> {
    with_cf_string(attr, |attr_ref| {
        let mut value = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attr_ref, &mut value) };
        match error {
            AX_SUCCESS => Ok(CfOwned::new(value)),
            code if snapshot_optional_ax_error(code) => Ok(None),
            AX_ERROR_API_DISABLED => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "macOS Accessibility API 当前不可用或未授权",
            )),
            code => Err(io::Error::other(format!(
                "读取 AX attribute `{attr}` 失败: AXError {code}"
            ))),
        }
    })
}

fn snapshot_optional_ax_error(error: AXError) -> bool {
    matches!(
        error,
        AX_ERROR_FAILURE
            | AX_ERROR_NOT_IMPLEMENTED
            | AX_ERROR_ATTRIBUTE_UNSUPPORTED
            | AX_ERROR_NO_VALUE
            | AX_ERROR_CANNOT_COMPLETE
            | AX_ERROR_INVALID_UI_ELEMENT
    )
}

fn copy_string_attr(element: AXUIElementRef, attr: &str) -> io::Result<Option<String>> {
    let Some(value) = copy_attribute(element, attr)? else {
        return Ok(None);
    };
    Ok(cf_to_string(value.as_ptr()))
}

fn copy_bool_attr(element: AXUIElementRef, attr: &str) -> io::Result<Option<bool>> {
    let Some(value) = copy_attribute(element, attr)? else {
        return Ok(None);
    };
    Ok(cf_to_bool(value.as_ptr()))
}

fn copy_number_attr(element: AXUIElementRef, attr: &str) -> io::Result<Option<f64>> {
    let Some(value) = copy_attribute(element, attr)? else {
        return Ok(None);
    };
    Ok(cf_to_f64(value.as_ptr()))
}

fn copy_action_names(element: AXUIElementRef) -> io::Result<Vec<String>> {
    let mut actions = ptr::null();
    let error = unsafe { AXUIElementCopyActionNames(element, &mut actions) };
    match error {
        AX_SUCCESS => {}
        // 有些真实 macOS UI 元素能读取 role/title/rect,但对其它查询只返回
        // kAXErrorFailure / not implemented。snapshot 的职责是尽量描述当前桌面,
        // 不能因为一个元素不愿意列 actions 就让整棵 AX tree 失败。
        code if snapshot_optional_ax_error(code) || code == AX_ERROR_ACTION_UNSUPPORTED => {
            return Ok(Vec::new())
        }
        AX_ERROR_API_DISABLED => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "macOS Accessibility API 当前不可用或未授权",
            ))
        }
        code => {
            return Err(io::Error::other(format!(
                "读取 AX actions 失败: AXError {code}"
            )))
        }
    }

    let Some(actions) = CfOwned::new(actions) else {
        return Ok(Vec::new());
    };
    let count = unsafe { CFArrayGetCount(actions.as_ptr()) };
    let mut names = Vec::new();
    for index in 0..count {
        let value = unsafe { CFArrayGetValueAtIndex(actions.as_ptr(), index) };
        if let Some(name) = cf_to_string(value) {
            names.push(name);
        }
    }
    Ok(names)
}

fn copy_ax_rect(element: AXUIElementRef) -> io::Result<Option<AxRect>> {
    let Some(position) = copy_attribute(element, "AXPosition")? else {
        return Ok(None);
    };
    let Some(size) = copy_attribute(element, "AXSize")? else {
        return Ok(None);
    };
    let Some(point) = cf_to_point(position.as_ptr()) else {
        return Ok(None);
    };
    let Some(size) = cf_to_size(size.as_ptr()) else {
        return Ok(None);
    };
    Ok(ax_rect_from_parts(point, size))
}

fn dictionary_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    cf_to_string(value)
}

fn dictionary_i32(dict: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    cf_to_i32(value)
}

fn dictionary_rect(dict: CFDictionaryRef, key: CFStringRef) -> Option<AxRect> {
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    if value.is_null() {
        return None;
    }

    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = unsafe { CGRectMakeWithDictionaryRepresentation(value, &mut rect) };
    ok.then(|| ax_rect_from_parts(rect.origin, rect.size))
        .flatten()
}

fn cf_to_string(value: CFTypeRef) -> Option<String> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return cf_to_i32(value)
            .map(|number| number.to_string())
            .or_else(|| cf_to_bool(value).map(|flag| flag.to_string()));
    }

    let mut buffer = vec![0i8; 4096];
    let ok =
        unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as CFIndex, UTF8) };
    if ok == 0 {
        return None;
    }

    let bytes = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    String::from_utf8(bytes).ok()
}

fn cf_to_bool(value: CFTypeRef) -> Option<bool> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFBooleanGetTypeID() } {
        return None;
    }
    Some(unsafe { CFBooleanGetValue(value as CFBooleanRef) != 0 })
}

fn cf_to_i32(value: CFTypeRef) -> Option<i32> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut number = 0i32;
    let ok = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            CF_NUMBER_SINT32,
            (&mut number as *mut i32).cast(),
        )
    };
    (ok != 0).then_some(number)
}

fn cf_to_f64(value: CFTypeRef) -> Option<f64> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut number = 0.0f64;
    let ok = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            CF_NUMBER_DOUBLE,
            (&mut number as *mut f64).cast(),
        )
    };
    (ok != 0).then_some(number)
}

fn cf_to_point(value: CFTypeRef) -> Option<CGPoint> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let ok = unsafe {
        AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CG_POINT,
            (&mut point as *mut CGPoint).cast(),
        )
    };
    (ok != 0).then_some(point)
}

fn cf_to_size(value: CFTypeRef) -> Option<CGSize> {
    if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { AXValueGetTypeID() } {
        return None;
    }
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = unsafe {
        AXValueGetValue(
            value as AXValueRef,
            AX_VALUE_CG_SIZE,
            (&mut size as *mut CGSize).cast(),
        )
    };
    (ok != 0).then_some(size)
}

fn ax_rect_from_parts(point: CGPoint, size: CGSize) -> Option<AxRect> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !size.width.is_finite()
        || !size.height.is_finite()
        || size.width < 0.0
        || size.height < 0.0
    {
        return None;
    }

    Some(AxRect {
        x: point.x.round() as i32,
        y: point.y.round() as i32,
        width: size.width.round() as u32,
        height: size.height.round() as u32,
    })
}

fn looks_like_secure_element(role: &str, subrole: Option<&str>) -> bool {
    role.to_ascii_lowercase().contains("secure")
        || subrole
            .map(|value| value.to_ascii_lowercase().contains("secure"))
            .unwrap_or(false)
}

fn with_cf_string<T>(value: &str, f: impl FnOnce(CFStringRef) -> T) -> T {
    let c_string = CString::new(value).expect("AX constant should not contain NUL");
    let cf_string = unsafe {
        CfOwned::new(CFStringCreateWithCString(
            ptr::null(),
            c_string.as_ptr(),
            UTF8,
        ))
    }
    .expect("CFStringCreateWithCString should create string");
    f(cf_string.as_ptr())
}

unsafe fn map_ax_action_error(error: AXError, action_name: &str) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ACTION_UNSUPPORTED => Err(invalid_input(format!(
            "目标 AX 元素不支持动作 {action_name}"
        ))),
        AX_ERROR_INVALID_UI_ELEMENT | AX_ERROR_NO_VALUE => {
            Err(invalid_input("AX target 元素已失效"))
        }
        code => Err(io::Error::other(format!(
            "执行动作 {action_name} 失败: AXError {code}"
        ))),
    }
}

unsafe fn map_ax_set_value_error(error: AXError) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED => Err(invalid_input("目标 AX 元素不支持 AXValue")),
        AX_ERROR_INVALID_UI_ELEMENT | AX_ERROR_NO_VALUE => {
            Err(invalid_input("AX target 元素已失效"))
        }
        code => Err(io::Error::other(format!(
            "写入 AXValue 失败: AXError {code}"
        ))),
    }
}

unsafe fn map_ax_bool_set_error(attr: &str, error: AXError) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED | AX_ERROR_NOT_IMPLEMENTED | AX_ERROR_NO_VALUE => {
            Err(invalid_input(format!("目标 AX 元素不支持 `{attr}`")))
        }
        AX_ERROR_INVALID_UI_ELEMENT => Err(invalid_input("AX target 元素已失效")),
        code => Err(io::Error::other(format!(
            "设置 `{attr}` 失败: AXError {code}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_id_should_reject_malformed_paths() {
        let parsed = parse_target_id("pid:123/window:2/path:3.4").unwrap();
        assert_eq!(parsed.pid, 123);
        assert_eq!(parsed.window_index, 2);
        assert_eq!(parsed.path, vec![3, 4]);

        let window = parse_target_id("pid:123/window:2").unwrap();
        assert!(window.path.is_empty());

        assert!(parse_target_id("pid:123/window:2/path:").is_err());
        assert!(parse_target_id("pid:123/window:2/path:3.").is_err());
        assert!(parse_target_id("pid:123/window:2/path:3/extra").is_err());
        assert!(parse_target_id("pid:123/window:2/path:bad").is_err());
        let error = parse_target_id("bad-target-id").unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("AX target id"),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("@ax-press target"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn build_final_ax_value_should_reject_append_when_current_value_is_unreadable() {
        assert_eq!(
            build_final_ax_value(Some("hello".to_owned()), " world", AxValueSetMode::Append)
                .unwrap(),
            "hello world"
        );
        assert_eq!(
            build_final_ax_value(None, "fresh", AxValueSetMode::Replace).unwrap(),
            "fresh"
        );
        let error = build_final_ax_value(None, " world", AxValueSetMode::Append).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("目标 AX 元素当前 AXValue 不可读,无法执行 append"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn snapshot_should_treat_partial_ax_failures_as_missing_optional_fields() {
        assert!(snapshot_optional_ax_error(AX_ERROR_FAILURE));
        assert!(snapshot_optional_ax_error(AX_ERROR_NOT_IMPLEMENTED));
        assert!(snapshot_optional_ax_error(AX_ERROR_ATTRIBUTE_UNSUPPORTED));
        assert!(snapshot_optional_ax_error(AX_ERROR_NO_VALUE));
        assert!(snapshot_optional_ax_error(AX_ERROR_CANNOT_COMPLETE));
        assert!(snapshot_optional_ax_error(AX_ERROR_INVALID_UI_ELEMENT));
        assert!(!snapshot_optional_ax_error(AX_ERROR_API_DISABLED));
        assert!(!snapshot_optional_ax_error(AX_SUCCESS));
    }
}
