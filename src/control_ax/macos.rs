use super::*;
use std::{
    collections::BTreeMap,
    ffi::CString,
    os::raw::{c_char, c_int, c_void},
    ptr,
};

type Boolean = u8;
type CFIndex = isize;
type CFTypeID = usize;
type CFTypeRef = *const c_void;
type CFStringRef = CFTypeRef;
type CFArrayRef = CFTypeRef;
type CFDictionaryRef = CFTypeRef;
type CFNumberRef = CFTypeRef;
type CFBooleanRef = CFTypeRef;
type AXUIElementRef = CFTypeRef;
type AXValueRef = CFTypeRef;
type AXError = i32;

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT32: i32 = 3;
const AX_SUCCESS: AXError = 0;
const AX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;
const AX_ERROR_CANNOT_COMPLETE: AXError = -25204;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: AXError = -25205;
const AX_ERROR_ACTION_UNSUPPORTED: AXError = -25206;
const AX_ERROR_API_DISABLED: AXError = -25211;
const AX_ERROR_NO_VALUE: AXError = -25212;
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;
const CG_WINDOW_ON_SCREEN_ONLY: u32 = 1 << 0;
const CG_WINDOW_EXCLUDE_DESKTOP: u32 = 1 << 4;

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
    static kCGWindowBounds: CFStringRef;
    static kCGWindowLayer: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowOwnerName: CFStringRef;
    static kCGWindowOwnerPID: CFStringRef;

    fn AXIsProcessTrusted() -> Boolean;
    fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
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

pub(super) fn press(request: &AxPressRequest) -> io::Result<AxActionReport> {
    ensure_trusted()?;

    let target_id = match &request.target.id {
        Some(id) => id.clone(),
        None => {
            let lookup_request = AxTreeRequest {
                depth: 8,
                max_elements: 5000,
                include_values: false,
                ..AxTreeRequest::default()
            };
            let snapshot = snapshot(&lookup_request)?;
            resolve_target_id_in_snapshot(&snapshot, &request.target)?
        }
    };

    press_target_id(&target_id)?;
    Ok(AxActionReport::press(
        "macos-accessibility",
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

fn press_target_id(target_id: &str) -> io::Result<()> {
    let parsed = parse_target_id(target_id)?;
    let app = unsafe { CfOwned::new(AXUIElementCreateApplication(parsed.pid)) }
        .ok_or_else(|| invalid_input("@ax-press 无法创建目标应用 AX element"))?;
    let windows = copy_attribute(app.as_ptr(), "AXWindows")?
        .ok_or_else(|| invalid_input("@ax-press 目标应用没有 AXWindows"))?;
    let count = unsafe { CFArrayGetCount(windows.as_ptr()) };
    if parsed.window_index >= count as usize {
        return Err(invalid_input(format!(
            "@ax-press 目标 window index 已失效: {}",
            parsed.window_index
        )));
    }

    let window_ref =
        unsafe { CFArrayGetValueAtIndex(windows.as_ptr(), parsed.window_index as CFIndex) };
    let mut current = unsafe { CfOwned::retain(window_ref) }
        .ok_or_else(|| invalid_input("@ax-press 目标 window 已失效"))?;

    for step in parsed.path {
        let children = copy_attribute(current.as_ptr(), "AXChildren")?
            .ok_or_else(|| invalid_input("@ax-press 目标路径已失效"))?;
        let count = unsafe { CFArrayGetCount(children.as_ptr()) };
        if step >= count as usize {
            return Err(invalid_input(format!(
                "@ax-press 目标路径 step 已失效: {step}"
            )));
        }
        let child = unsafe { CFArrayGetValueAtIndex(children.as_ptr(), step as CFIndex) };
        current = unsafe { CfOwned::retain(child) }
            .ok_or_else(|| invalid_input("@ax-press 目标元素已失效"))?;
    }

    with_cf_string("AXPress", |action| unsafe {
        map_ax_action_error(AXUIElementPerformAction(current.as_ptr(), action))
    })
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
        return Err(invalid_input(format!(
            "@ax-press target id 格式非法: {target_id}"
        )));
    }

    let pid = parts[0]
        .strip_prefix("pid:")
        .ok_or_else(|| invalid_input(format!("@ax-press target id 缺少 pid: {target_id}")))?
        .parse::<i32>()
        .map_err(|_| invalid_input(format!("@ax-press target id pid 非法: {target_id}")))?;
    let window_index = parts[1]
        .strip_prefix("window:")
        .ok_or_else(|| invalid_input(format!("@ax-press target id 缺少 window: {target_id}")))?
        .parse::<usize>()
        .map_err(|_| invalid_input(format!("@ax-press target id window 非法: {target_id}")))?;

    let path = match parts.get(2) {
        Some(path) => {
            let path = path.strip_prefix("path:").ok_or_else(|| {
                invalid_input(format!("@ax-press target id path 非法: {target_id}"))
            })?;
            if path.is_empty() || path.split('.').any(str::is_empty) {
                return Err(invalid_input(format!(
                    "@ax-press target id path step 非法: {target_id}"
                )));
            }
            path.split('.')
                .map(|item| {
                    item.parse::<usize>().map_err(|_| {
                        invalid_input(format!("@ax-press target id path step 非法: {target_id}"))
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
            AX_ERROR_ATTRIBUTE_UNSUPPORTED
            | AX_ERROR_NO_VALUE
            | AX_ERROR_CANNOT_COMPLETE
            | AX_ERROR_INVALID_UI_ELEMENT => Ok(None),
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

fn copy_action_names(element: AXUIElementRef) -> io::Result<Vec<String>> {
    let mut actions = ptr::null();
    let error = unsafe { AXUIElementCopyActionNames(element, &mut actions) };
    match error {
        AX_SUCCESS => {}
        AX_ERROR_ACTION_UNSUPPORTED
        | AX_ERROR_NO_VALUE
        | AX_ERROR_CANNOT_COMPLETE
        | AX_ERROR_INVALID_UI_ELEMENT => return Ok(Vec::new()),
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

unsafe fn map_ax_action_error(error: AXError) -> io::Result<()> {
    match error {
        AX_SUCCESS => Ok(()),
        AX_ERROR_API_DISABLED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "macOS Accessibility API 当前不可用或未授权",
        )),
        AX_ERROR_ACTION_UNSUPPORTED => Err(invalid_input("目标 AX 元素不支持 AXPress")),
        AX_ERROR_INVALID_UI_ELEMENT | AX_ERROR_NO_VALUE => {
            Err(invalid_input("@ax-press 目标元素已失效"))
        }
        code => Err(io::Error::other(format!(
            "执行 AXPress 失败: AXError {code}"
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
    }
}
