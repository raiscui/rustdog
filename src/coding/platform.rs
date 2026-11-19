use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::model::{CodingError, ErrorCode, ProcessIdentity, Result, SessionMetadata};

#[cfg(windows)]
mod windows {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::ffi::{c_void, OsStr};
    use std::io;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::Component;
    use std::process::{Command, Stdio};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_INVALID_PARAMETER, ERROR_SUCCESS, FILETIME, GENERIC_ALL,
        HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetAce, GetSecurityDescriptorControl, GetTokenInformation, IsWellKnownSid,
        TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACL,
        DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx, DELETE,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, WRITE_DAC, WRITE_OWNER,
    };
    use windows_sys::Win32::System::JobObjects::{
        IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, WaitForSingleObject,
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, DETACHED_PROCESS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    fn os_error() -> CodingError {
        io::Error::last_os_error().into()
    }
    fn invalid(message: impl Into<String>) -> CodingError {
        CodingError::new(ErrorCode::Storage, message.into())
    }
    fn wide(value: &OsStr) -> Result<Vec<u16>> {
        let mut result: Vec<u16> = value.encode_wide().collect();
        if result.contains(&0) {
            return Err(invalid("Windows string contains NUL"));
        }
        result.push(0);
        Ok(result)
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 此句柄由成功的 Win32 创建或打开调用返回，且仅由本对象关闭。
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Local(*mut c_void);
    impl Drop for Local {
        fn drop(&mut self) {
            // SAFETY: 指针由返回 LocalAlloc 内存的 Win32 API 提供，只释放一次。
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    struct User {
        buffer: Vec<usize>,
    }
    impl User {
        fn sid(&self) -> PSID {
            // SAFETY: buffer 由 GetTokenInformation 填充，包含完整 TOKEN_USER 和其 SID。
            unsafe { (*(self.buffer.as_ptr() as *const TOKEN_USER)).User.Sid }
        }
    }
    fn process_user(process: HANDLE) -> Result<User> {
        let mut token = null_mut();
        // SAFETY: process 是有效进程句柄，token 是可写输出指针。
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(os_error());
        }
        let token = Handle(token);
        let mut bytes = 0;
        // SAFETY: 首次调用仅查询所需大小，零长度允许空缓冲区。
        unsafe {
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(os_error());
        }
        let mut buffer =
            vec![0usize; (bytes as usize + size_of::<usize>() - 1) / size_of::<usize>()];
        // SAFETY: buffer 有足够的空间和对齐，返回长度由系统写入。
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        } == 0
        {
            return Err(os_error());
        }
        Ok(User { buffer })
    }
    fn current_user() -> Result<User> {
        // SAFETY: GetCurrentProcess 返回当前进程的有效伪句柄。
        process_user(unsafe { GetCurrentProcess() })
    }
    fn sid_string(sid: PSID) -> Result<String> {
        let mut text = null_mut();
        // SAFETY: 调用方持有包含 SID 的系统缓冲区, text 接收系统分配的字符串.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(os_error());
        }
        let allocation = Local(text.cast());
        let mut len = 0usize;
        // SAFETY: 系统返回以 NUL 结尾的 UTF-16 字符串。
        unsafe {
            while *text.add(len) != 0 {
                len += 1;
            }
        }
        // SAFETY: 上面的扫描确定了字符串的有效范围。
        let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) })
            .map_err(|_| invalid("invalid SID string"))?;
        drop(allocation);
        Ok(result)
    }

    pub(crate) struct UserSecurity {
        descriptor: Local,
    }
    impl UserSecurity {
        pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor.0,
                bInheritHandle: 0,
            }
        }
    }
    pub(crate) fn user_security() -> Result<UserSecurity> {
        let user = current_user()?;
        let sid = sid_string(user.sid())?;
        let sddl = wide(OsStr::new(&format!(
            "O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
        )))?;
        let mut descriptor = null_mut();
        // SAFETY: SDDL 是有效结尾字符串，descriptor 是可写输出指针。
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(os_error());
        }
        Ok(UserSecurity {
            descriptor: Local(descriptor),
        })
    }
    pub(crate) fn verify_process_user(pid: u32) -> Result<()> {
        // SAFETY: 按 PID 打开只读查询句柄，不继承句柄。
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(os_error());
        }
        let process = Handle(process);
        let current = current_user()?;
        let peer = process_user(process.0)?;
        // SAFETY: 两个 SID 均属于仍存活的令牌信息缓冲区。
        if unsafe { EqualSid(current.sid(), peer.sid()) } == 0 {
            return Err(CodingError::new(
                ErrorCode::HostIdentityMismatch,
                "peer user SID mismatch",
            ));
        }
        Ok(())
    }

    pub(crate) fn default_state_dir() -> Result<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("LOCALAPPDATA is not set"))?;
        let path = PathBuf::from(base);
        if !path.is_absolute() {
            return Err(invalid("LOCALAPPDATA must be absolute"));
        }
        Ok(path.join("rdog").join("coding"))
    }
    fn open_directory(path: &Path) -> Result<Handle> {
        let name = wide(path.as_os_str())?;
        // SAFETY: 名称以 NUL 结尾，打开目录本身而非重解析目标，并禁止重命名替换。
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x0002_0000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(os_error());
        }
        let handle = Handle(handle);
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: 句柄和输出结构有效，尺寸与信息类匹配。
        if unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileAttributeTagInfo,
                (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        } == 0
        {
            return Err(os_error());
        }
        if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(invalid(format!(
                "directory is not a real directory: {}",
                path.display()
            )));
        }
        Ok(handle)
    }
    fn verify_directory(handle: &Handle) -> Result<()> {
        let user = current_user()?;
        let mut owner: PSID = null_mut();
        let mut acl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: 句柄有效，输出指针指向本地变量，系统分配的描述符统一释放。
        let status = unsafe {
            GetSecurityInfo(
                handle.0,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32).into());
        }
        let _descriptor = Local(descriptor);
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: descriptor 是 GetSecurityInfo 返回的有效安全描述符。
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(os_error());
        }
        // SAFETY: owner 和 user SID 的存储均在本作用域内有效。
        if owner.is_null()
            || unsafe { EqualSid(owner, user.sid()) } == 0
            || acl.is_null()
            || control & SE_DACL_PROTECTED == 0
        {
            return Err(invalid("directory owner or DACL is not restricted"));
        }
        // SAFETY: ACL 指针由有效描述符提供，并在描述符释放前读取。
        let count = unsafe { (*acl).AceCount };
        if count == 0 {
            return Err(invalid("directory DACL is empty"));
        }
        let mut user_allowed = false;
        for index in 0..u32::from(count) {
            let mut ace = null_mut();
            // SAFETY: index 在 ACL 的 ACE 数量内，ace 是可写输出。
            if unsafe { GetAce(acl, index, &mut ace) } == 0 {
                return Err(os_error());
            }
            // SAFETY: GetAce 成功返回有效 ACE，检查类型和长度后读取 SID。
            let header = unsafe { &*(ace as *const windows_sys::Win32::Security::ACE_HEADER) };
            if header.AceType != 0 || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
            {
                return Err(invalid("unsupported directory ACE"));
            }
            // SAFETY: 已检查标准允许 ACE 的类型和长度；系统 ACL 自身保证 SID 边界。
            let sid =
                unsafe { &mut (*(ace as *mut ACCESS_ALLOWED_ACE)).SidStart as *mut u32 as PSID };
            // SAFETY: SID 位于有效的系统 ACL 中，user SID 仍然存活。
            let is_user = unsafe { EqualSid(sid, user.sid()) } != 0;
            // SAFETY: SID 位于有效的系统 ACL 中。
            let is_system = unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } != 0;
            if !is_user && !is_system {
                return Err(invalid("directory DACL grants another principal"));
            }
            user_allowed |= is_user;
        }
        if !user_allowed {
            return Err(invalid("directory DACL does not grant its owner"));
        }
        Ok(())
    }
    fn trusted_ancestor_principal(sid: PSID, user: &User) -> Result<bool> {
        // SAFETY: SID 均在调用方持有的系统描述符或令牌缓冲区内.
        if unsafe {
            EqualSid(sid, user.sid()) != 0
                || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
        } {
            return Ok(true);
        }
        // 系统卷根目录的 owner 可能是 TrustedInstaller, 不是当前登录用户.
        Ok(sid_string(sid)? == "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")
    }
    fn verify_ancestor(handle: &Handle) -> Result<()> {
        let user = current_user()?;
        let mut owner: PSID = null_mut();
        let mut acl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: 句柄和输出指针有效, 系统描述符由 RAII 释放.
        let status = unsafe {
            GetSecurityInfo(
                handle.0,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32).into());
        }
        let _descriptor = Local(descriptor);
        if owner.is_null() || acl.is_null() || !trusted_ancestor_principal(owner, &user)? {
            return Err(invalid("ancestor owner or DACL is not trusted"));
        }
        // SAFETY: ACL 来自有效系统描述符, 其生命周期覆盖本次遍历.
        for index in 0..u32::from(unsafe { (*acl).AceCount }) {
            let mut ace = null_mut();
            // SAFETY: index 未超过系统提供的 ACE 数量.
            if unsafe { GetAce(acl, index, &mut ace) } == 0 {
                return Err(os_error());
            }
            // SAFETY: GetAce 成功后返回至少包含 ACE_HEADER 的 ACE.
            let header = unsafe { &*(ace as *const windows_sys::Win32::Security::ACE_HEADER) };
            if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 || header.AceType == 1 {
                continue;
            }
            if header.AceType != 0 || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
            {
                return Err(invalid("unsupported ancestor ACE"));
            }
            // SAFETY: 已验证标准允许 ACE 的类型和长度, SID 边界由系统描述符保证.
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            if allowed.Mask & (GENERIC_ALL | DELETE | FILE_DELETE_CHILD | WRITE_DAC | WRITE_OWNER)
                != 0
                && !trusted_ancestor_principal(sid, &user)?
            {
                return Err(invalid(
                    "ancestor permits another principal to replace protected state",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn secure_directory(path: &Path) -> Result<()> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(invalid("directory path must be absolute and normalized"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| invalid("directory has no parent"))?;
        let mut held = Vec::new();
        let mut current = PathBuf::new();
        for component in parent.components() {
            current.push(component.as_os_str());
            if matches!(component, Component::Normal(_) | Component::RootDir) {
                let directory = open_directory(&current)?;
                verify_ancestor(&directory)?;
                held.push(directory);
            }
        }
        let security = user_security()?;
        let attrs = security.attributes();
        let name = wide(path.as_os_str())?;
        // SAFETY: 父目录句柄阻止路径分量被替换，创建调用原子应用受保护 DACL。
        if unsafe { CreateDirectoryW(name.as_ptr(), &attrs) } == 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.into());
            }
        }
        let directory = open_directory(path)?;
        verify_directory(&directory)
    }

    fn identity(process: HANDLE, pid: u32) -> Result<ProcessIdentity> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: process 是有效句柄，四个 FILETIME 输出各自有效。
        if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
            == 0
        {
            return Err(os_error());
        }
        Ok(ProcessIdentity {
            pid,
            created_at: (u64::from(creation.dwHighDateTime) << 32)
                | u64::from(creation.dwLowDateTime),
        })
    }
    pub(crate) fn current_identity() -> Result<ProcessIdentity> {
        // SAFETY: 当前进程伪句柄在调用期间有效。
        identity(unsafe { GetCurrentProcess() }, std::process::id())
    }
    pub(crate) fn identity_alive(expected: &ProcessIdentity) -> Result<bool> {
        // SAFETY: 按 PID 打开查询和同步句柄，不继承。
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                expected.pid,
            )
        };
        if process.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(false);
            }
            return Err(error.into());
        }
        let process = Handle(process);
        if identity(process.0, expected.pid)? != *expected {
            return Ok(false);
        }
        // SAFETY: 进程句柄具有同步权限，零超时不阻塞。
        match unsafe { WaitForSingleObject(process.0, 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            _ => Err(os_error()),
        }
    }
    pub(crate) fn endpoint(meta: &SessionMetadata) -> Result<String> {
        let user = current_user()?;
        let sid = sid_string(user.sid())?;
        let mut hash = Sha256::new();
        for part in [
            sid.as_bytes(),
            meta.session_id.as_bytes(),
            meta.host_instance_id.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        Ok(format!(r"\\.\pipe\rdog-coding-{:x}", hash.finalize()))
    }

    pub(crate) fn spawn_host(
        session_dir: &Path,
        meta: &SessionMetadata,
        env: &BTreeMap<String, String>,
    ) -> Result<ProcessIdentity> {
        let mut in_job = 0;
        // SAFETY: 当前进程伪句柄与输出指针有效, 空 Job 查询任意成员资格.
        if unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut in_job) } == 0 {
            return Err(os_error());
        }
        let mut flags = CREATE_SUSPENDED | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        if in_job != 0 {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // SAFETY: 空 Job 查询当前进程所属 Job, 输出缓冲区大小与信息类别匹配.
            if unsafe {
                QueryInformationJobObject(
                    null_mut(),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    null_mut(),
                )
            } == 0
            {
                return Err(os_error());
            }
            let limits = limits.BasicLimitInformation.LimitFlags;
            if limits & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK == 0 {
                if limits & JOB_OBJECT_LIMIT_BREAKAWAY_OK == 0 {
                    return Err(invalid("parent job does not allow breakaway"));
                }
                flags |= CREATE_BREAKAWAY_FROM_JOB;
            }
        }
        let stderr = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(session_dir.join("host.stderr.log"))?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("coding-host")
            .arg("--session-dir")
            .arg(session_dir)
            .arg("--instance-id")
            .arg(&meta.host_instance_id)
            .current_dir(session_dir)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .creation_flags(flags)
            .spawn()?;
        let setup = (|| {
            let mut child_in_job = 0;
            // SAFETY: 子进程仍挂起且句柄有效, 不能在检查前执行 Host 代码.
            if unsafe { IsProcessInJob(child.as_raw_handle(), null_mut(), &mut child_in_job) } == 0
            {
                return Err(os_error());
            }
            if child_in_job != 0 {
                return Err(invalid("host remains inside a job"));
            }
            let identity = identity(child.as_raw_handle(), child.id())?;
            super::super::process::resume_initial_thread(child.id())?;
            Ok(identity)
        })();
        match setup {
            Ok(identity) => Ok(identity),
            Err(error) => {
                let cleanup = child.kill().and_then(|()| child.wait().map(|_| ()));
                match cleanup {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(invalid(format!(
                        "host setup failed: {error}; child cleanup failed: {cleanup}"
                    ))),
                }
            }
        }
    }
}
#[cfg(windows)]
pub(super) use windows::{
    current_identity, default_state_dir, endpoint, identity_alive, secure_directory, spawn_host,
    user_security, verify_process_user,
};
