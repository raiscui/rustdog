use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use super::model::{ProcessIdentity, Result};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenThread, ResumeThread, WaitForSingleObject, CREATE_NO_WINDOW,
    CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

#[derive(Debug)]
pub(crate) struct OwnedOmp {
    child: Child,
    job: Option<OwnedHandle>,
}

impl OwnedOmp {
    pub(crate) fn spawn(command: Command) -> Result<(Self, ChildStdout, ChildStdin)> {
        Ok(Self::spawn_inner(command)?)
    }

    fn spawn_inner(mut command: Command) -> io::Result<(Self, ChildStdout, ChildStdin)> {
        let job = create_job()?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
        let mut assigned = false;

        let setup = (|| {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("spawned OMP has no piped stdout"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("spawned OMP has no piped stdin"))?;
            // SAFETY: 两个句柄在调用期间均由本函数持有且有效.
            if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0
            {
                return Err(io::Error::last_os_error());
            }
            assigned = true;
            resume_initial_thread(child.id())?;
            Ok((stdout, stdin))
        })();

        match setup {
            Ok((stdout, stdin)) => Ok((
                Self {
                    child,
                    job: Some(job),
                },
                stdout,
                stdin,
            )),
            Err(error) => {
                // SAFETY: job 是本函数独占的有效句柄.
                let job_terminated =
                    assigned && unsafe { TerminateJobObject(job.as_raw_handle(), 1) } != 0;
                let cleanup = match child.kill() {
                    Ok(()) => child.wait().map(|_| ()),
                    Err(_) if job_terminated => child.wait().map(|_| ()),
                    Err(kill_error) => match child.try_wait() {
                        Ok(Some(_)) => Ok(()),
                        Ok(None) => Err(kill_error),
                        Err(wait_error) => Err(wait_error),
                    },
                };
                drop(job);
                match cleanup {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => Err(io::Error::other(format!(
                        "OMP setup failed: {error}; child cleanup failed: {cleanup_error}"
                    ))),
                }
            }
        }
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn identity(&self) -> Result<ProcessIdentity> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: 子进程句柄有效, 所有输出结构均已初始化.
        if unsafe {
            GetProcessTimes(
                self.child.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        Ok(ProcessIdentity {
            pid: self.pid(),
            created_at: (u64::from(created.dwHighDateTime) << 32)
                | u64::from(created.dwLowDateTime),
        })
    }

    pub(crate) fn is_running(&self) -> io::Result<bool> {
        // SAFETY: 子进程句柄有效, 零超时不会阻塞.
        match unsafe { WaitForSingleObject(self.child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            status => Err(io::Error::other(format!(
                "unexpected process wait status: {status}"
            ))),
        }
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        let Some(job) = self.job.as_ref() else {
            return Ok(());
        };
        // SAFETY: job 是本对象拥有的有效 Job 句柄.
        if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: 输出缓冲区类型和长度与请求的信息类别一致, 句柄在调用期间有效.
            if unsafe {
                QueryInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let exited = self.child.try_wait()?.is_some();
            if accounting.ActiveProcesses == 0 && exited {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for OMP job and child to exit",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }

        // SAFETY: 该句柄由 self 独占; 成功关闭后立即放弃 Rust 所有权.
        if unsafe { CloseHandle(job.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(job) = self.job.take() {
            let _ = job.into_raw_handle();
        }
        Ok(())
    }
}

fn create_job() -> io::Result<OwnedHandle> {
    // SAFETY: 空安全属性和名称分别表示默认安全属性和匿名 Job.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW 成功返回新的独占句柄.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: 输入缓冲区类型和长度与请求的信息类别一致, Job 句柄有效.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

pub(super) fn resume_initial_thread(pid: u32) -> io::Result<()> {
    // SAFETY: 此调用不解引用指针, 返回的快照由本函数独占.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 成功返回的快照句柄需要由调用方关闭.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: 快照有效, entry 已初始化并设置了正确的结构大小.
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    let mut thread_id = None;
    while found != 0 {
        if entry.th32OwnerProcessID == pid && thread_id.replace(entry.th32ThreadID).is_some() {
            return Err(io::Error::other("suspended child has multiple threads"));
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: 快照有效, entry 是可写且大小正确的缓冲区.
        found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    let thread_id = thread_id.ok_or_else(|| io::Error::other("child initial thread not found"))?;
    // SAFETY: 使用快照中属于新子进程的线程 ID, 只申请恢复权限.
    let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenThread 成功返回新的独占句柄.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    // SAFETY: 线程句柄有效, 且具有 THREAD_SUSPEND_RESUME 权限.
    let previous = unsafe { ResumeThread(handle.as_raw_handle()) };
    if previous == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    if previous != 1 {
        return Err(io::Error::other(format!(
            "unexpected child initial thread suspend count: {previous}"
        )));
    }
    Ok(())
}
