use std::{ffi::c_void, marker::PhantomData, ptr, rc::Rc};

use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{HWND, POINT, RECT},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::Accessibility::CUIAutomation,
    },
};

use super::super::model::NativeRect;

type HResult = i32;
type Object = *mut c_void;
const IID_AUTOMATION: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);
const MAX_ELEMENTS: usize = 512;

// 对照 windows 0.61.3 的 SDK 生成接口, 保留未使用的槽位以维持 ABI。
#[repr(C)]
struct UnknownVtable {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(Object) -> u32,
}
#[repr(C)]
struct AutomationVtable {
    unknown: UnknownVtable,
    compare_elements: usize,
    compare_runtime_ids: usize,
    root_element: usize,
    element_from_handle: unsafe extern "system" fn(Object, HWND, *mut Object) -> HResult,
    before_raw_walker: [usize; 9],
    raw_walker: unsafe extern "system" fn(Object, *mut Object) -> HResult,
}
#[repr(C)]
struct ElementVtable {
    unknown: UnknownVtable,
    before_control_type: [usize; 18],
    control_type: unsafe extern "system" fn(Object, *mut i32) -> HResult,
    before_bounds: [usize; 21],
    bounds: unsafe extern "system" fn(Object, *mut RECT) -> HResult,
}
#[repr(C)]
struct WalkerVtable {
    unknown: UnknownVtable,
    parent: usize,
    first_child: unsafe extern "system" fn(Object, Object, *mut Object) -> HResult,
    last_child: usize,
    next_sibling: unsafe extern "system" fn(Object, Object, *mut Object) -> HResult,
}

struct ComObject {
    raw: Object,
    _apartment: PhantomData<Rc<()>>,
}
impl ComObject {
    fn from_result(hr: HResult, raw: Object) -> Result<Self, &'static str> {
        if hr < 0 || raw.is_null() {
            return Err("UIA object unavailable");
        }
        Ok(Self {
            raw,
            _apartment: PhantomData,
        })
    }
    fn optional(hr: HResult, raw: Object) -> Result<Option<Self>, &'static str> {
        if hr < 0 {
            return Err("UIA traversal failed");
        }
        if raw.is_null() {
            Ok(None)
        } else {
            Ok(Some(Self {
                raw,
                _apartment: PhantomData,
            }))
        }
    }
    unsafe fn vtable<T>(&self) -> &T {
        // SAFETY: 调用者选择与创建该 COM 接口时相同的 vtable 类型。
        unsafe { &**(self.raw as *const *const T) }
    }
}
impl Drop for ComObject {
    fn drop(&mut self) {
        // SAFETY: raw 是本 apartment 中拥有一个引用的有效 COM 接口。
        unsafe {
            (self.vtable::<UnknownVtable>().release)(self.raw);
        }
    }
}

struct Apartment;
impl Apartment {
    fn new() -> Result<Self, &'static str> {
        // SAFETY: 线程专用于 UIA, 参数符合 CoInitializeEx 契约。
        let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr < 0 {
            return Err("UIA MTA initialization failed");
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: 本线程的成功初始化与此调用一一配对。
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) struct Candidate {
    pub control_type: u32,
    pub bounds: NativeRect,
}

// 所有 COM 对象只在创建它们的 MTA 线程中使用和释放。
pub(super) struct Automation {
    automation: ComObject,
    walker: ComObject,
    _apartment: Apartment,
}
impl Automation {
    pub fn new() -> Result<Self, &'static str> {
        let apartment = Apartment::new()?;
        let mut raw = ptr::null_mut();
        // SAFETY: 输出指针有效, CLSID 和 IID 来自 Windows SDK。
        let hr = unsafe {
            CoCreateInstance(
                &CUIAutomation,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_AUTOMATION,
                &mut raw,
            )
        };
        let automation = ComObject::from_result(hr, raw)?;
        let mut walker = ptr::null_mut();
        // SAFETY: automation 持有 IUIAutomation 接口, 输出指针有效。
        let hr = unsafe {
            (automation.vtable::<AutomationVtable>().raw_walker)(automation.raw, &mut walker)
        };
        let walker = ComObject::from_result(hr, walker)?;
        Ok(Self {
            automation,
            walker,
            _apartment: apartment,
        })
    }

    pub fn candidate(&self, hwnd: HWND, point: POINT) -> Result<Option<Candidate>, &'static str> {
        let mut raw = ptr::null_mut();
        // SAFETY: automation 是 IUIAutomation, hwnd 由调用者先验证授权, 输出指针有效。
        let hr = unsafe {
            (self
                .automation
                .vtable::<AutomationVtable>()
                .element_from_handle)(self.automation.raw, hwnd, &mut raw)
        };
        let root = ComObject::from_result(hr, raw)?;
        let mut stack = vec![(root, 0usize)];
        let mut best = None;
        let mut visited = 0usize;
        while let Some((element, depth)) = stack.pop() {
            visited += 1;
            if visited > MAX_ELEMENTS {
                return Err("UIA subtree budget exceeded");
            }
            let mut rect = RECT::default();
            // SAFETY: element 是 IUIAutomationElement, rect 为有效输出。
            let hr = unsafe { (element.vtable::<ElementVtable>().bounds)(element.raw, &mut rect) };
            if hr < 0 {
                return Err("UIA bounds unavailable");
            }
            if point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom
            {
                let mut kind = 0i32;
                // SAFETY: element 是 IUIAutomationElement, kind 为有效输出。
                let hr = unsafe {
                    (element.vtable::<ElementVtable>().control_type)(element.raw, &mut kind)
                };
                if hr < 0 {
                    return Err("UIA control type unavailable");
                }
                if kind > 0
                    && best
                        .as_ref()
                        .map_or(true, |(best_depth, _)| depth > *best_depth)
                {
                    let width = u32::try_from(i64::from(rect.right) - i64::from(rect.left))
                        .map_err(|_| "invalid UIA bounds")?;
                    let height = u32::try_from(i64::from(rect.bottom) - i64::from(rect.top))
                        .map_err(|_| "invalid UIA bounds")?;
                    best = Some((
                        depth,
                        Candidate {
                            control_type: kind as u32,
                            bounds: NativeRect {
                                x: rect.left,
                                y: rect.top,
                                width,
                                height,
                            },
                        },
                    ));
                }
            }
            let mut child = ptr::null_mut();
            // SAFETY: walker 和 element 分别为有效的树遍历器和节点, 输出指针有效。
            let hr = unsafe {
                (self.walker.vtable::<WalkerVtable>().first_child)(
                    self.walker.raw,
                    element.raw,
                    &mut child,
                )
            };
            let mut child = ComObject::optional(hr, child)?;
            while let Some(current) = child {
                if stack.len() + visited >= MAX_ELEMENTS {
                    return Err("UIA subtree budget exceeded");
                }
                let mut next = ptr::null_mut();
                // SAFETY: current 是同一树中的有效节点, 输出指针有效。
                let hr = unsafe {
                    (self.walker.vtable::<WalkerVtable>().next_sibling)(
                        self.walker.raw,
                        current.raw,
                        &mut next,
                    )
                };
                child = ComObject::optional(hr, next)?;
                stack.push((current, depth + 1));
            }
        }
        Ok(best.map(|(_, candidate)| candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};

    #[test]
    #[ignore = "requires an explicitly authorized Windows interactive desktop"]
    fn authorized_foreground_root_returns_a_bounded_numeric_candidate() {
        // SAFETY: 查询当前前台窗口与鼠标位置, 不改变桌面状态。
        let hwnd = unsafe { GetForegroundWindow() };
        assert!(
            !hwnd.is_null(),
            "an authorized foreground window is required"
        );
        let mut point = POINT::default();
        // SAFETY: point 是有效输出指针。
        assert_ne!(
            unsafe { GetCursorPos(&mut point) },
            0,
            "cursor position unavailable"
        );
        let automation = Automation::new().expect("UIA MTA unavailable");
        let candidate = automation
            .candidate(hwnd, point)
            .expect("authorized root query failed")
            .expect("place the cursor over the authorized window before running");
        assert!(candidate.control_type > 0);
        assert!(i64::from(point.x) >= i64::from(candidate.bounds.x));
        assert!(i64::from(point.y) >= i64::from(candidate.bounds.y));
        assert!(
            i64::from(point.x) < i64::from(candidate.bounds.x) + i64::from(candidate.bounds.width)
        );
        assert!(
            i64::from(point.y) < i64::from(candidate.bounds.y) + i64::from(candidate.bounds.height)
        );
    }
}
