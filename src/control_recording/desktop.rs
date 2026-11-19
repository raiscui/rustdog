#[cfg(any(windows, test))]
fn permitted(thread: Option<&[u16]>, input: Option<&[u16]>, own_session: Option<u32>, target_session: Option<u32>) -> bool {
    fn is_default(name: &[u16]) -> bool {
        name.strip_suffix(&[0]).unwrap_or(name) == [68, 101, 102, 97, 117, 108, 116]
    }
    match (thread, input, own_session, target_session) {
        (Some(thread), Some(input), Some(own), Some(target)) => {
            own != 0 && own == target && is_default(thread) && is_default(input)
        }
        _ => false,
    }
}

#[cfg(windows)]
fn desktop_name(desktop: *mut core::ffi::c_void) -> Option<Vec<u16>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::StationsAndDesktops::{GetUserObjectInformationW, UOI_NAME};
    if desktop.is_null() { return None; }
    let mut length = 0;
    // SAFETY: 空缓冲区只查询所需长度.
    unsafe { GetUserObjectInformationW(desktop, UOI_NAME, null_mut(), 0, &mut length); }
    if length == 0 || length > 4096 || length % 2 != 0 { return None; }
    let mut name = vec![0u16; (length / 2) as usize];
    // SAFETY: 缓冲区容量与系统返回的字节数一致.
    if unsafe { GetUserObjectInformationW(desktop, UOI_NAME, name.as_mut_ptr().cast(), length, &mut length) } == 0 { return None; }
    Some(name)
}

#[cfg(windows)]
fn desktop_names() -> Option<(Vec<u16>, Vec<u16>)> {
    use windows_sys::Win32::System::{StationsAndDesktops::{CloseDesktop, GetThreadDesktop, OpenInputDesktop, DESKTOP_READOBJECTS}, Threading::GetCurrentThreadId};
    // SAFETY: 线程 desktop 由系统拥有, input desktop 在本函数关闭.
    let (thread, input) = unsafe { (GetThreadDesktop(GetCurrentThreadId()), OpenInputDesktop(0, 0, DESKTOP_READOBJECTS)) };
    if input.is_null() { return None; }
    let names = desktop_name(thread).zip(desktop_name(input));
    // SAFETY: input 是上方打开的句柄.
    unsafe { CloseDesktop(input); }
    names
}

#[cfg(windows)]
pub(super) fn check_recording_desktop() -> Result<(), String> {
    match desktop_names() {
        Some((thread, input)) if thread == input => Ok(()),
        _ => Err("hook thread is not on the input desktop".into()),
    }
}

#[cfg(windows)]
pub(crate) fn check_interactive_desktop(target_pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32; }
    let names = desktop_names();
    let mut own = 0;
    let mut target = 0;
    // SAFETY: session 输出指针有效, 进程 ID 由系统查询.
    let sessions_ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut own) != 0 && ProcessIdToSessionId(target_pid, &mut target) != 0 };
    if sessions_ok && names.as_ref().is_some_and(|(thread, input)| permitted(Some(thread), Some(input), Some(own), Some(target))) {
        Ok(())
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "交互桌面不可用"))
    }
}

#[cfg(test)]
mod tests {
    use super::permitted;
    #[test]
    fn requires_matching_default_desktop_and_nonzero_session() {
        let default: Vec<u16> = "Default\0".encode_utf16().collect();
        let other: Vec<u16> = "Winlogon\0".encode_utf16().collect();
        assert!(permitted(Some(&default), Some(&default), Some(2), Some(2)));
        assert!(!permitted(Some(&default), Some(&other), Some(2), Some(2)));
        assert!(!permitted(Some(&default), Some(&default), Some(2), Some(3)));
        assert!(!permitted(Some(&default), Some(&default), Some(0), Some(0)));
        assert!(!permitted(None, Some(&default), Some(2), Some(2)));
    }
}
