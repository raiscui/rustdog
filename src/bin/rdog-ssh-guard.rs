#[cfg(not(windows))]
fn main() {
    eprintln!("rdog-ssh-guard 仅支持 Windows OpenSSH");
    std::process::exit(1);
}

#[cfg(any(windows, test))]
fn required_command(command: Option<std::ffi::OsString>) -> std::io::Result<std::ffi::OsString> {
    command.filter(|value| !value.is_empty()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "缺少 SSH_ORIGINAL_COMMAND",
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_is_required_and_is_not_rewritten() {
        use std::ffi::OsString;
        assert!(super::required_command(None).is_err());
        assert!(super::required_command(Some(OsString::new())).is_err());
        let command = OsString::from("echo 中文 & exit 7");
        assert_eq!(
            super::required_command(Some(command.clone())).unwrap(),
            command
        );
    }
}

#[cfg(windows)]
fn main() {
    match windows::run() {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => {
            eprintln!("SSH guard: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::{c_void, OsString},
        io,
        mem::{size_of, zeroed},
        os::windows::ffi::OsStringExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, FILETIME, HANDLE,
            INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{
            Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            SystemInformation::GetSystemDirectoryW,
            Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
                GetCurrentProcessId, GetExitCodeProcess, GetProcessTimes,
                InitializeProcThreadAttributeList, OpenProcess, QueryFullProcessImageNameW,
                ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForMultipleObjects,
                WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
                PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            },
        },
    };

    struct Handle(HANDLE);
    impl Handle {
        fn new(handle: HANDLE) -> io::Result<Self> {
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(handle))
            }
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn check(ok: i32) -> io::Result<()> {
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    fn invalid(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
    fn creation_time(handle: HANDLE) -> io::Result<u64> {
        let (mut a, mut b, mut c, mut d) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            check(GetProcessTimes(handle, &mut a, &mut b, &mut c, &mut d))?;
        }
        Ok((u64::from(a.dwHighDateTime) << 32) | u64::from(a.dwLowDateTime))
    }
    fn session_process() -> io::Result<Handle> {
        unsafe {
            let snapshot = Handle::new(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0))?;
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..zeroed()
            };
            check(Process32FirstW(snapshot.0, &mut entry))?;
            let mut entries = Vec::new();
            loop {
                entries.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(snapshot.0, &mut entry) == 0 {
                    break;
                }
            }
            let mut id = GetCurrentProcessId();
            let mut younger = creation_time(GetCurrentProcess())?;
            for _ in 0..32 {
                let parent = entries
                    .iter()
                    .find(|(pid, _)| *pid == id)
                    .map(|(_, parent)| *parent)
                    .ok_or_else(|| invalid("进程树缺少父进程"))?;
                if parent == 0 || parent == id {
                    break;
                }
                let handle = Handle::new(OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    parent,
                ))?;
                let born = creation_time(handle.0)?;
                if born > younger {
                    return Err(invalid("父进程身份已变化"));
                }
                let mut path = vec![0u16; 32768];
                let mut len = path.len() as u32;
                check(QueryFullProcessImageNameW(
                    handle.0,
                    0,
                    path.as_mut_ptr(),
                    &mut len,
                ))?;
                let path = OsString::from_wide(&path[..len as usize]);
                if std::path::Path::new(&path)
                    .file_name()
                    .is_some_and(|s| s.eq_ignore_ascii_case("sshd.exe"))
                {
                    if WaitForSingleObject(handle.0, 0) != WAIT_TIMEOUT {
                        return Err(invalid("SSH 会话已结束"));
                    }
                    return Ok(handle);
                }
                id = parent;
                younger = born;
            }
            Err(invalid("未找到 SSH 会话进程"))
        }
    }
    struct Attributes(*mut c_void);
    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.0);
            }
        }
    }
    fn duplicate_std(which: u32) -> io::Result<Handle> {
        unsafe {
            let source = GetStdHandle(which);
            if source.is_null() || source == INVALID_HANDLE_VALUE {
                return Err(invalid("SSH 标准句柄不可用"));
            }
            let mut duplicate = null_mut();
            check(DuplicateHandle(
                GetCurrentProcess(),
                source,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            ))?;
            Handle::new(duplicate)
        }
    }
    pub fn run() -> io::Result<u32> {
        let original = super::required_command(std::env::var_os("SSH_ORIGINAL_COMMAND"))?;
        let session = session_process()?;
        unsafe {
            let job = Handle::new(CreateJobObjectW(null(), null()))?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            check(SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ))?;
            let stdin = duplicate_std(STD_INPUT_HANDLE)?;
            let stdout = duplicate_std(STD_OUTPUT_HANDLE)?;
            let stderr = duplicate_std(STD_ERROR_HANDLE)?;
            let handles = [stdin.0, stdout.0, stderr.0];
            let mut bytes = 0;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
            if bytes == 0 {
                return Err(io::Error::last_os_error());
            }
            let words = bytes.div_ceil(size_of::<usize>());
            let mut storage = vec![0usize; words];
            let attrs = storage.as_mut_ptr().cast::<c_void>();
            check(InitializeProcThreadAttributeList(attrs, 1, 0, &mut bytes))?;
            let attrs = Attributes(attrs);
            check(UpdateProcThreadAttribute(
                attrs.0,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(&handles),
                null_mut(),
                null(),
            ))?;
            let mut startup: STARTUPINFOEXW = zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = stdin.0;
            startup.StartupInfo.hStdOutput = stdout.0;
            startup.StartupInfo.hStdError = stderr.0;
            startup.lpAttributeList = attrs.0;
            let mut system_dir = vec![0u16; 32768];
            let dir_len = GetSystemDirectoryW(system_dir.as_mut_ptr(), system_dir.len() as u32);
            if dir_len == 0 || dir_len as usize >= system_dir.len() {
                return Err(io::Error::last_os_error());
            }
            let mut app = OsString::from_wide(&system_dir[..dir_len as usize]);
            app.push("\\cmd.exe");
            let mut cmd = OsString::from("\"");
            cmd.push(&app);
            cmd.push("\" /c ");
            cmd.push(original);
            use std::os::windows::ffi::OsStrExt;
            let mut cmd: Vec<u16> = cmd.encode_wide().chain(std::iter::once(0)).collect();
            let app: Vec<u16> = app.encode_wide().chain(std::iter::once(0)).collect();
            let mut process: PROCESS_INFORMATION = zeroed();
            if WaitForSingleObject(session.0, 0) != WAIT_TIMEOUT {
                return Err(invalid("SSH 会话已结束"));
            }
            check(CreateProcessW(
                app.as_ptr(),
                cmd.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                null(),
                null(),
                &startup.StartupInfo,
                &mut process,
            ))?;
            let child = Handle::new(process.hProcess)?;
            let thread = Handle::new(process.hThread)?;
            if let Err(error) = check(AssignProcessToJobObject(job.0, child.0)) {
                check(TerminateProcess(child.0, 1)).map_err(|termination| {
                    io::Error::other(format!(
                        "无法绑定 Job: {error}; 终止挂起进程失败: {termination}"
                    ))
                })?;
                if WaitForSingleObject(child.0, INFINITE) != WAIT_OBJECT_0 {
                    return Err(io::Error::other(format!(
                        "无法绑定 Job: {error}; 等待挂起进程退出失败: {}",
                        io::Error::last_os_error()
                    )));
                }
                return Err(error);
            }
            if WaitForSingleObject(session.0, 0) != WAIT_TIMEOUT {
                return Err(invalid("SSH 会话已结束"));
            }
            if ResumeThread(thread.0) == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            match WaitForMultipleObjects(2, [session.0, child.0].as_ptr(), 0, INFINITE) {
                WAIT_OBJECT_0 => Err(invalid("SSH 会话中断")),
                n if n == WAIT_OBJECT_0 + 1 => {
                    let mut exit = 0;
                    check(GetExitCodeProcess(child.0, &mut exit))?;
                    Ok(exit)
                }
                _ => Err(io::Error::last_os_error()),
            }
        }
    }
}
