#[cfg(unix)]
use std::{fs, io, path::Path};

#[cfg(unix)]
pub(super) fn create_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(unix)]
pub(super) fn create_new(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsStr};
    use std::fs;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 句柄由成功的 OpenProcessToken 返回，且只在此关闭。
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct Local(*mut c_void);

    impl Drop for Local {
        fn drop(&mut self) {
            // SAFETY: 指针由返回 LocalAlloc 内存的系统调用提供，只在此释放。
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
        let mut result: Vec<u16> = value.encode_wide().collect();
        if result.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        result.push(0);
        Ok(result)
    }

    struct User(Vec<usize>);

    impl User {
        fn current() -> io::Result<Self> {
            let mut token = null_mut();
            // SAFETY: 当前进程伪句柄有效，token 是可写的输出位置。
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Handle(token);
            let mut bytes = 0;
            // SAFETY: 零长度调用只查询所需缓冲区大小。
            unsafe {
                GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut bytes);
            }
            if bytes == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
            // SAFETY: buffer 具有足够的空间和对齐，bytes 是可写的输出位置。
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
                return Err(io::Error::last_os_error());
            }
            Ok(Self(buffer))
        }

        fn sid(&self) -> PSID {
            // SAFETY: 缓冲区由 GetTokenInformation 填充，包含 TOKEN_USER 和其 SID。
            unsafe { (*(self.0.as_ptr().cast::<TOKEN_USER>())).User.Sid }
        }
    }

    struct Security(Local);

    impl Security {
        fn new() -> io::Result<Self> {
            let user = User::current()?;
            let mut text = null_mut();
            // SAFETY: 用户缓冲区在调用期间有效，text 是可写的输出位置。
            if unsafe { ConvertSidToStringSidW(user.sid(), &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let _text = Local(text.cast());
            let mut len = 0;
            // SAFETY: 系统返回以 NUL 结尾的字符串。
            unsafe {
                while *text.add(len) != 0 {
                    len += 1;
                }
            }
            // SAFETY: 上面的扫描确定了字符串范围，分配仍由 _text 持有。
            let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) })
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SID string"))?;
            let sddl = wide(OsStr::new(&format!(
                "D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
            )))?;
            let mut descriptor = null_mut();
            // SAFETY: SDDL 以 NUL 结尾，descriptor 是可写的输出位置。
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(Local(descriptor)))
        }

        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0 .0,
                bInheritHandle: 0,
            }
        }
    }

    pub(crate) fn create_directory(path: &Path) -> io::Result<()> {
        let name = wide(path.as_os_str())?;
        let security = Security::new()?;
        create_directory_inner(path, &name, &security)
    }

    fn create_directory_inner(path: &Path, name: &[u16], security: &Security) -> io::Result<()> {
        // SAFETY: 路径以 NUL 结尾，安全描述符在调用期间保持有效。
        if unsafe { CreateDirectoryW(name.as_ptr(), &security.attributes()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists
            && fs::metadata(path).is_ok_and(|m| m.is_dir())
        {
            return Ok(());
        }
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
        let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        else {
            return Err(error);
        };
        let parent_name = wide(parent.as_os_str())?;
        create_directory_inner(parent, &parent_name, security)?;
        // SAFETY: 路径和安全描述符在调用期间保持有效。
        if unsafe { CreateDirectoryW(name.as_ptr(), &security.attributes()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists
            && fs::metadata(path).is_ok_and(|m| m.is_dir())
        {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub(crate) fn create_new(path: &Path) -> io::Result<fs::File> {
        let name = wide(path.as_os_str())?;
        let security = Security::new()?;
        // SAFETY: 路径和安全描述符在调用期间保持有效，CREATE_NEW 不打开已有文件。
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                &security.attributes(),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: 成功的 CreateFileW 返回唯一拥有的句柄，所有权转移给 File。
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            EqualSid, GetAce, GetSecurityDescriptorControl, IsWellKnownSid, WinLocalSystemSid,
            ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

        fn assert_private(path: &Path) {
            let name = wide(path.as_os_str()).unwrap();
            let mut acl: *mut ACL = null_mut();
            let mut descriptor = null_mut();
            // SAFETY: 路径和输出指针有效，返回的描述符由 Local 释放。
            let status = unsafe {
                GetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    &mut acl,
                    null_mut(),
                    &mut descriptor,
                )
            };
            assert_eq!(status, ERROR_SUCCESS);
            let _descriptor = Local(descriptor);
            assert!(!acl.is_null());
            let mut control = 0;
            let mut revision = 0;
            // SAFETY: descriptor 和输出指针有效。
            assert_ne!(
                unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            // SAFETY: ACL 包含系统返回的有效 ACE 数量。
            assert_eq!(unsafe { (*acl).AceCount }, 2);
            let user = User::current().unwrap();
            let mut user_found = false;
            let mut system_found = false;
            for index in 0..2 {
                let mut ace = null_mut();
                // SAFETY: 索引在 ACL 的 ACE 数量内，ace 是可写输出位置。
                assert_ne!(unsafe { GetAce(acl, index, &mut ace) }, 0);
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                // SAFETY: 本测试要求 ACE 为允许访问的标准 ACE。
                unsafe {
                    assert_eq!((*ace).Header.AceType, 0);
                    assert_eq!((*ace).Mask & FILE_ALL_ACCESS, FILE_ALL_ACCESS);
                    let sid = std::ptr::addr_of_mut!((*ace).SidStart).cast();
                    user_found |= EqualSid(sid, user.sid()) != 0;
                    system_found |= IsWellKnownSid(sid, WinLocalSystemSid) != 0;
                }
            }
            assert!(user_found && system_found);
        }

        #[test]
        fn new_files_and_nested_directories_have_protected_dacls() {
            let root = std::env::temp_dir().join(format!("rdog-private-{}", uuid::Uuid::new_v4()));
            let nested = root.join("nested");
            create_directory(&nested).unwrap();
            let file = nested.join("data");
            drop(create_new(&file).unwrap());
            assert_private(&root);
            assert_private(&nested);
            assert_private(&file);
            assert_eq!(
                create_new(&file).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn rejects_nul_paths() {
            use std::os::windows::ffi::OsStringExt;
            let path = std::ffi::OsString::from_wide(&[b'a' as u16, 0, b'b' as u16]);
            assert_eq!(
                create_directory(Path::new(&path)).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(
                create_new(Path::new(&path)).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }
}

#[cfg(windows)]
pub(super) use windows::{create_directory, create_new};

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_creation_preserves_existing_directories() {
        let root = std::env::temp_dir().join(format!("rdog-private-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let nested = root.join("first").join("second");
        create_directory(&nested).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o755
        );
        for path in [root.join("first"), nested.clone()] {
            assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o077, 0);
        }
        let file = nested.join("data");
        drop(create_new(&file).unwrap());
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o177, 0);
        assert_eq!(
            create_new(&file).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        fs::remove_dir_all(root).unwrap();
    }
}
