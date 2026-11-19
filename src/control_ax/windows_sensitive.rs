use std::{ffi::c_void, io, marker::PhantomData, ptr, rc::Rc};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
        UI::{Accessibility::CUIAutomation, WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsChild, IsWindow}},
    },
};

use crate::control_flow::{FlowTextParameterStep, FlowTextTarget};

type Object = *mut c_void;
type HResult = i32;
const IID_AUTOMATION: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);
const IID_VALUE: GUID = GUID::from_u128(0xa94cd8b1_0844_4cd6_9d2d_640537ab39e9);
const UIA_VALUE_PATTERN: i32 = 10002;

#[link(name = "oleaut32")]
unsafe extern "system" {
    fn SysStringLen(value: *const u16) -> u32;
    fn SysFreeString(value: *mut u16);
}

#[repr(C)]
struct UnknownVtable {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(Object) -> u32,
}

#[repr(C)]
struct AutomationVtable {
    unknown: UnknownVtable,
    before_element: [usize; 3],
    element_from_handle: unsafe extern "system" fn(Object, HWND, *mut Object) -> HResult,
    element_from_point: usize,
    focused_element: unsafe extern "system" fn(Object, *mut Object) -> HResult,
}

#[repr(C)]
struct ElementVtable {
    unknown: UnknownVtable,
    before_pattern: [usize; 11],
    pattern_as: unsafe extern "system" fn(Object, i32, *const GUID, *mut Object) -> HResult,
    before_process: [usize; 5],
    process_id: unsafe extern "system" fn(Object, *mut i32) -> HResult,
    control_type: unsafe extern "system" fn(Object, *mut i32) -> HResult,
    before_focus: [usize; 4],
    has_focus: unsafe extern "system" fn(Object, *mut i32) -> HResult,
    before_native_window: [usize; 9],
    native_window: unsafe extern "system" fn(Object, *mut HWND) -> HResult,
}

#[repr(C)]
struct ValueVtable {
    unknown: UnknownVtable,
    set_value: unsafe extern "system" fn(Object, *const u16) -> HResult,
    current_value: unsafe extern "system" fn(Object, *mut *mut u16) -> HResult,
    is_read_only: unsafe extern "system" fn(Object, *mut i32) -> HResult,
}

struct ComObject {
    raw: Object,
    apartment: PhantomData<Rc<()>>,
}
impl ComObject {
    fn new(result: HResult, raw: Object) -> io::Result<Self> {
        if result < 0 || raw.is_null() { return Err(failed()); }
        Ok(Self { raw, apartment: PhantomData })
    }
    unsafe fn vtable<T>(&self) -> &T {
        // SAFETY: 接口由对应 IID 创建, 指针在当前 apartment 保持有效.
        unsafe { &**(self.raw as *const *const T) }
    }
}
impl Drop for ComObject {
    fn drop(&mut self) {
        // SAFETY: 本对象独占一个当前 apartment 内的 COM 引用.
        unsafe { (self.vtable::<UnknownVtable>().release)(self.raw); }
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: 成功初始化的当前线程调用一次反初始化.
        unsafe { CoUninitialize() }
    }
}
struct Bstr(*mut u16);
impl Drop for Bstr {
    fn drop(&mut self) {
        // SAFETY: 指针由 SysAllocStringLen 或 UIA 返回, 可由 SysFreeString 释放.
        unsafe { SysFreeString(self.0) }
    }
}
fn failed() -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, "文本动作未派发") }
fn unknown() -> io::Error { io::Error::other("文本动作结果未知") }

pub(super) fn set_and_verify(step: &FlowTextParameterStep, value: &str) -> io::Result<()> {
    crate::control_recording::desktop::check_interactive_desktop(step.pid)?;
    if value.contains('\0') { return Err(failed()); }
    let root = usize::try_from(step.root_hwnd).map_err(|_| failed())? as HWND;
    let mut root_pid = 0;
    // SAFETY: 窗口句柄仅用于系统查询, PID 输出指针有效.
    let valid = unsafe {
        IsWindow(root) != 0 && GetForegroundWindow() == root
            && GetWindowThreadProcessId(root, &mut root_pid) != 0
    };
    if !valid || root_pid != step.pid { return Err(failed()); }
    // SAFETY: 本调用线程初始化 MTA, 成功后由 Apartment 负责清理.
    if unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) } < 0 { return Err(failed()); }
    let _apartment = Apartment;
    let mut raw = ptr::null_mut();
    // SAFETY: CLSID、IID 和输出指针符合 COM 契约.
    let result = unsafe { CoCreateInstance(&CUIAutomation, ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_AUTOMATION, &mut raw) };
    let automation = ComObject::new(result, raw)?;
    raw = ptr::null_mut();
    let (result, expected_handle, expected_type) = match step.target {
        FlowTextTarget::FocusedControl { control_type } => {
            // SAFETY: automation 是 IUIAutomation, 输出指针有效.
            (unsafe { (automation.vtable::<AutomationVtable>().focused_element)(automation.raw, &mut raw) }, None, Some(control_type))
        }
        FlowTextTarget::NativeControl { hwnd } => {
            let control = usize::try_from(hwnd).map_err(|_| failed())? as HWND;
            // SAFETY: automation 是 IUIAutomation, 句柄和输出指针有效.
            (unsafe { (automation.vtable::<AutomationVtable>().element_from_handle)(automation.raw, control, &mut raw) }, Some(hwnd), None)
        }
    };
    let element = ComObject::new(result, raw)?;
    let mut pid = 0;
    let mut focused = 0;
    let mut control_type = 0;
    let mut control = ptr::null_mut();
    // SAFETY: element 是 IUIAutomationElement, 所有输出指针有效.
    let valid = unsafe {
        (element.vtable::<ElementVtable>().process_id)(element.raw, &mut pid) >= 0
            && (element.vtable::<ElementVtable>().control_type)(element.raw, &mut control_type) >= 0
            && (element.vtable::<ElementVtable>().native_window)(element.raw, &mut control) >= 0
            && (element.vtable::<ElementVtable>().has_focus)(element.raw, &mut focused) >= 0
            && IsWindow(control) != 0 && IsChild(root, control) != 0
            && GetForegroundWindow() == root
    };
    if !valid || pid != step.pid as i32 || focused == 0
        || expected_type.is_some_and(|expected| expected != control_type)
        || expected_handle.is_some_and(|expected| expected != control as usize as u64)
    { return Err(failed()); }
    raw = ptr::null_mut();
    // SAFETY: 通过目标元素请求指定 ValuePattern 接口, 输出指针有效.
    let result = unsafe { (element.vtable::<ElementVtable>().pattern_as)(element.raw, UIA_VALUE_PATTERN, &IID_VALUE, &mut raw) };
    let pattern = ComObject::new(result, raw)?;
    let mut read_only = 1;
    // SAFETY: pattern 是 IUIAutomationValuePattern, 输出指针有效.
    if unsafe { (pattern.vtable::<ValueVtable>().is_read_only)(pattern.raw, &mut read_only) } < 0 || read_only != 0 { return Err(failed()); }
    let mut utf16: Vec<u16> = value.encode_utf16().collect();
    utf16.push(0);
    let mut control_pid = 0;
    root_pid = 0;
    focused = 0;
    // SAFETY: 句柄和输出指针均有效, 每个条件都在投递前重新核对.
    let valid = unsafe {
        IsWindow(root) != 0 && IsWindow(control) != 0 && IsChild(root, control) != 0
            && GetForegroundWindow() == root
            && GetWindowThreadProcessId(root, &mut root_pid) != 0
            && GetWindowThreadProcessId(control, &mut control_pid) != 0
            && (element.vtable::<ElementVtable>().has_focus)(element.raw, &mut focused) >= 0
    };
    if !valid || root_pid != step.pid || control_pid != step.pid || focused == 0 { return Err(failed()); }
    crate::control_recording::desktop::check_interactive_desktop(step.pid)?;
    // SAFETY: pattern 有效, UTF-16 缓冲以 NUL 结尾且内部不含 NUL.
    if unsafe { (pattern.vtable::<ValueVtable>().set_value)(pattern.raw, utf16.as_ptr()) } < 0 { return Err(unknown()); }
    let mut actual = ptr::null_mut();
    // SAFETY: pattern 有效, UIA 将 BSTR 所有权写入输出指针.
    if unsafe { (pattern.vtable::<ValueVtable>().current_value)(pattern.raw, &mut actual) } < 0 || actual.is_null() { return Err(unknown()); }
    let actual = Bstr(actual);
    // SAFETY: actual 是有效 BSTR, 所有读取均受其长度约束.
    let same = unsafe { std::slice::from_raw_parts(actual.0, SysStringLen(actual.0) as usize) == &utf16[..utf16.len() - 1] };
    if !same { return Err(unknown()); }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperInput {
    step: FlowTextParameterStep,
    value: String,
}

#[derive(serde::Serialize)]
struct HelperInputRef<'a> {
    step: &'a FlowTextParameterStep,
    value: &'a str,
}

pub(super) fn run_helper() -> io::Result<()> {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    std::io::stdin().take(1_048_577).read_to_end(&mut input).map_err(|_| failed())?;
    if input.len() > 1_048_576 { return Err(failed()); }
    let request: HelperInput = serde_json::from_slice(&input).map_err(|_| failed())?;
    let result = set_and_verify(&request.step, &request.value);
    let status: &[u8] = match result {
        Ok(()) => b"verified",
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => b"rejected",
        Err(_) => b"unknown",
    };
    std::io::stdout().write_all(status).map_err(|_| unknown())
}

pub(super) fn execute(step: &FlowTextParameterStep, value: &str, timeout: std::time::Duration) -> io::Result<()> {
    use std::{
        io::{Read, Write},
        os::windows::{io::{AsRawHandle, FromRawHandle, OwnedHandle}, process::CommandExt},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use windows_sys::Win32::System::{
        JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE},
        Threading::CREATE_NO_WINDOW,
    };
    if value.len() > 262_144 || value.contains('\0') || timeout.is_zero() { return Err(failed()); }
    let deadline = Instant::now() + timeout.min(Duration::from_secs(5));
    // SAFETY: 空参数创建私有 Job, 返回句柄由 OwnedHandle 独占.
    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw.is_null() { return Err(failed()); }
    // SAFETY: raw 是尚未转移所有权的有效 Job 句柄.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: 参数类型和大小符合 Win32 Job 契约.
    if unsafe { SetInformationJobObject(job.as_raw_handle(), JobObjectExtendedLimitInformation, &limits as *const _ as _, std::mem::size_of_val(&limits) as u32) } == 0 { return Err(failed()); }
    let mut child = Command::new(std::env::current_exe().map_err(|_| failed())?)
        .arg("__flow-text-helper").creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|_| failed())?;
    // SAFETY: 两个句柄分别指向本任务 Job 和子进程; 分配成功前不发送输入.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
        let _ = child.kill();
        if Instant::now() < deadline { let _ = child.try_wait(); }
        return Err(failed());
    }
    let input = serde_json::to_vec(&HelperInputRef { step, value }).map_err(|_| failed())?;
    let mut stdin = child.stdin.take().ok_or_else(failed)?;
    let writer = std::thread::spawn(move || {
        stdin.write_all(&input).map_err(|_| ())?;
        stdin.flush().map_err(|_| ())
    });
    let mut stdout = child.stdout.take().ok_or_else(unknown)?;
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.by_ref().take(9).read_to_end(&mut output).map(|_| output).map_err(|_| ())
    });
    loop {
        match child.try_wait() {
            Ok(Some(status)) if writer.is_finished() && reader.is_finished() => {
                if writer.join().ok().and_then(Result::ok).is_none() || !status.success() { return Err(unknown()); }
                return match reader.join().ok().and_then(Result::ok).as_deref() {
                    Some(b"verified") => Ok(()),
                    Some(b"rejected") => Err(failed()),
                    _ => Err(unknown()),
                };
            }
            Ok(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                // SAFETY: job 是本任务创建的有效 Job.
                unsafe { TerminateJobObject(job.as_raw_handle(), 1); }
                if writer.is_finished() { let _ = writer.join(); }
                if reader.is_finished() { let _ = reader.join(); }
                return Err(unknown());
            }
        }
    }
}
