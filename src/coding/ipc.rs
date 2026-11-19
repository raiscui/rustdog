use std::sync::mpsc::{Receiver, SyncSender};

use serde::{Deserialize, Serialize};

use super::model::{CodingReply, CodingRequest, ProcessIdentity, Result, SessionMetadata};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostEnvelope {
    pub session_id: String,
    pub host_instance_id: String,
    pub request: CodingRequest,
}

pub(crate) struct HostCall {
    pub envelope: HostEnvelope,
    pub reply: SyncSender<CodingReply>,
    pub written: Receiver<Result<()>>,
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ,
        GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    use super::super::model::{CodingError, ErrorCode};
    use super::super::platform;
    use super::{
        CodingReply, CodingRequest, HostCall, HostEnvelope, ProcessIdentity, Result,
        SessionMetadata,
    };

    const MAX_FRAME: usize = 256 * 1024;
    const IO_TIMEOUT: Duration = Duration::from_secs(10);
    const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
    const CLIENT_TIMEOUT: Duration = Duration::from_secs(45);
    const POLL_MS: u32 = 100;

    fn error(code: ErrorCode, message: impl Into<String>) -> CodingError {
        CodingError::new(code, message)
    }

    fn io_error(context: &str) -> CodingError {
        error(
            ErrorCode::Io,
            format!("{context}: {}", io::Error::last_os_error()),
        )
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 此对象独占有效句柄，关闭后不会再使用。
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn event() -> Result<Handle> {
        // SAFETY: 空安全属性和名称创建进程内使用的匿名事件。
        let handle = unsafe { CreateEventW(null(), 1, 0, null()) };
        if handle.is_null() {
            Err(io_error("creating pipe event"))
        } else {
            Ok(Handle(handle))
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // 每个操作持有独立事件和 OVERLAPPED，取消后等待内核完成再释放缓冲区。
    fn operation<F>(
        pipe: HANDLE,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
        start: F,
    ) -> Result<u32>
    where
        F: FnOnce(*mut OVERLAPPED) -> i32,
    {
        let event = event()?;
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event.0;
        let started = start(&mut overlapped);
        if started == 0 {
            // SAFETY: 紧接 Win32 调用读取当前线程的错误码。
            let code = unsafe { GetLastError() };
            if code == ERROR_PIPE_CONNECTED {
                return Ok(0);
            }
            if code != ERROR_IO_PENDING {
                return Err(error(
                    ErrorCode::Io,
                    format!(
                        "pipe operation: {}",
                        io::Error::from_raw_os_error(code as i32)
                    ),
                ));
            }
        }
        loop {
            let stopped = shutdown.is_some_and(|flag| flag.load(Ordering::Acquire));
            let remaining = deadline.saturating_duration_since(Instant::now());
            if stopped || remaining.is_zero() {
                // SAFETY: 操作及其 OVERLAPPED 和缓冲区仍存活，取消后同步等待完成。
                unsafe {
                    CancelIoEx(pipe, &overlapped);
                    let mut ignored = 0;
                    GetOverlappedResult(pipe, &overlapped, &mut ignored, 1);
                }
                return Err(error(
                    ErrorCode::Io,
                    if stopped {
                        "pipe server shutting down"
                    } else {
                        "pipe operation timed out"
                    },
                ));
            }
            let millis = remaining.as_millis().min(u128::from(POLL_MS)).max(1) as u32;
            // SAFETY: 事件句柄在本函数内有效。
            let wait = unsafe { WaitForSingleObject(event.0, millis) };
            if wait == WAIT_TIMEOUT {
                continue;
            }
            if wait != WAIT_OBJECT_0 {
                let failure = io_error("waiting for pipe operation");
                // SAFETY: 取消后等待完成，确保内核不再访问栈上的 OVERLAPPED。
                unsafe {
                    CancelIoEx(pipe, &overlapped);
                    let mut ignored = 0;
                    GetOverlappedResult(pipe, &overlapped, &mut ignored, 1);
                }
                return Err(failure);
            }
            let mut transferred = 0;
            // SAFETY: 事件已触发，句柄及 OVERLAPPED 均有效。
            if unsafe { GetOverlappedResult(pipe, &overlapped, &mut transferred, 0) } == 0 {
                return Err(io_error("completing pipe operation"));
            }
            return Ok(transferred);
        }
    }

    fn read_exact(
        pipe: HANDLE,
        mut buffer: &mut [u8],
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<()> {
        while !buffer.is_empty() {
            let len = buffer.len() as u32;
            let ptr = buffer.as_mut_ptr();
            let count = operation(pipe, shutdown, deadline, |overlapped| {
                // SAFETY: 缓冲区和 OVERLAPPED 在操作完成或取消前保持有效。
                unsafe { ReadFile(pipe, ptr, len, null_mut(), overlapped) }
            })? as usize;
            if count == 0 {
                return Err(error(ErrorCode::Io, "incomplete pipe frame"));
            }
            buffer = &mut buffer[count..];
        }
        Ok(())
    }

    fn write_all(
        pipe: HANDLE,
        mut buffer: &[u8],
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<()> {
        while !buffer.is_empty() {
            let len = buffer.len() as u32;
            let ptr = buffer.as_ptr();
            let count = operation(pipe, shutdown, deadline, |overlapped| {
                // SAFETY: 缓冲区和 OVERLAPPED 在操作完成或取消前保持有效。
                unsafe { WriteFile(pipe, ptr, len, null_mut(), overlapped) }
            })? as usize;
            if count == 0 {
                return Err(error(ErrorCode::Io, "incomplete pipe write"));
            }
            buffer = &buffer[count..];
        }
        Ok(())
    }

    fn read_frame<T: DeserializeOwned>(
        pipe: HANDLE,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<T> {
        let mut prefix = [0; 4];
        read_exact(pipe, &mut prefix, shutdown, deadline)?;
        let len = u32::from_be_bytes(prefix) as usize;
        if len == 0 || len > MAX_FRAME {
            return Err(error(
                ErrorCode::InvalidRequest,
                "invalid pipe frame length",
            ));
        }
        let mut bytes = vec![0; len];
        read_exact(pipe, &mut bytes, shutdown, deadline)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            error(
                ErrorCode::InvalidRequest,
                format!("invalid pipe frame JSON: {e}"),
            )
        })
    }

    fn write_frame<T: Serialize>(
        pipe: HANDLE,
        value: &T,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(error(
                ErrorCode::InvalidRequest,
                "pipe frame exceeds size limit",
            ));
        }
        write_all(
            pipe,
            &(bytes.len() as u32).to_be_bytes(),
            shutdown,
            deadline,
        )?;
        write_all(pipe, &bytes, shutdown, deadline)
    }

    fn send_reply(pipe: HANDLE, reply: &CodingReply, shutdown: &AtomicBool) -> Result<()> {
        let deadline = Instant::now() + IO_TIMEOUT;
        write_frame(pipe, reply, Some(shutdown), deadline)?;
        let mut ack = [0];
        read_exact(pipe, &mut ack, Some(shutdown), deadline)?;
        if ack != [1] {
            return Err(error(ErrorCode::Io, "invalid pipe reply acknowledgment"));
        }
        Ok(())
    }

    fn peer_pid(pipe: HANDLE, server: bool) -> Result<u32> {
        let mut pid = 0;
        // SAFETY: 句柄有效，pid 指向可写的 u32。
        let ok = unsafe {
            if server {
                GetNamedPipeServerProcessId(pipe, &mut pid)
            } else {
                GetNamedPipeClientProcessId(pipe, &mut pid)
            }
        };
        if ok == 0 {
            return Err(io_error("getting pipe peer process"));
        }
        platform::verify_process_user(pid)?;
        Ok(pid)
    }

    fn serve_one(
        pipe: HANDLE,
        meta: &SessionMetadata,
        tx: &SyncSender<HostCall>,
        shutdown: &AtomicBool,
    ) -> Result<()> {
        peer_pid(pipe, false)?;
        let envelope: HostEnvelope = read_frame(pipe, Some(shutdown), Instant::now() + IO_TIMEOUT)?;
        if envelope.session_id != meta.session_id
            || envelope.host_instance_id != meta.host_instance_id
            || envelope
                .request
                .session_id()
                .is_some_and(|id| id != meta.session_id)
        {
            return Err(error(
                ErrorCode::HostIdentityMismatch,
                "pipe envelope identity mismatch",
            ));
        }
        envelope.request.validate()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let (written_tx, written_rx) = mpsc::sync_channel(1);
        match tx.try_send(HostCall {
            envelope,
            reply: reply_tx,
            written: written_rx,
        }) {
            Ok(()) => (),
            Err(TrySendError::Full(_)) => {
                return Err(error(ErrorCode::Busy, "host request queue is full"))
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(error(
                    ErrorCode::HostUnavailable,
                    "host request queue is closed",
                ))
            }
        }
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            if shutdown.load(Ordering::Acquire) {
                return Err(error(ErrorCode::HostUnavailable, "host is shutting down"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(error(ErrorCode::UnknownOutcome, "host reply timed out"));
            }
            match reply_rx.recv_timeout(remaining.min(Duration::from_millis(u64::from(POLL_MS)))) {
                Ok(reply) => {
                    let result = send_reply(pipe, &reply, shutdown);
                    let _ = written_tx.send(result.clone());
                    return result;
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(error(
                        ErrorCode::UnknownOutcome,
                        "host reply channel closed",
                    ))
                }
            }
        }
    }

    fn run(
        meta: SessionMetadata,
        tx: SyncSender<HostCall>,
        shutdown: Arc<AtomicBool>,
        started: SyncSender<Result<()>>,
    ) {
        let setup = || -> Result<Handle> {
            let name = wide(&platform::endpoint(&meta)?);
            let security = platform::user_security()?;
            let attributes = security.attributes();
            // SAFETY: 名称以零结尾，安全描述符在调用期间存活，句柄不继承。
            let handle = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    MAX_FRAME as u32,
                    MAX_FRAME as u32,
                    0,
                    &attributes,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                Err(io_error("creating named pipe"))
            } else {
                Ok(Handle(handle))
            }
        };
        let pipe = match setup() {
            Ok(pipe) => {
                if started.send(Ok(())).is_err() {
                    return;
                }
                pipe
            }
            Err(err) => {
                let _ = started.send(Err(err));
                return;
            }
        };
        while !shutdown.load(Ordering::Acquire) {
            let connect = operation(
                pipe.0,
                Some(&shutdown),
                Instant::now() + IO_TIMEOUT,
                |overlapped| {
                    // SAFETY: 管道句柄独占，OVERLAPPED 持续到操作结束。
                    unsafe { ConnectNamedPipe(pipe.0, overlapped) }
                },
            );
            if let Err(err) = connect {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                if err.message == "pipe operation timed out" {
                    continue;
                }
                tracing::error!(error = %err, "coding pipe accept failed");
                break;
            }
            if let Err(err) = serve_one(pipe.0, &meta, &tx, &shutdown) {
                if !shutdown.load(Ordering::Acquire) {
                    let reply = CodingReply::failure(err);
                    if let Err(write_err) = send_reply(pipe.0, &reply, &shutdown) {
                        tracing::warn!(error = %write_err, "coding pipe error response failed");
                    }
                }
            }
            // SAFETY: 本线程独占服务端句柄，先断开本次连接再复用同一实例。
            unsafe {
                DisconnectNamedPipe(pipe.0);
            }
        }
    }

    pub(crate) struct Server {
        shutdown: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Server {
        pub(crate) fn start(meta: &SessionMetadata, tx: SyncSender<HostCall>) -> Result<Self> {
            let shutdown = Arc::new(AtomicBool::new(false));
            let worker_shutdown = Arc::clone(&shutdown);
            let meta = meta.clone();
            let (started_tx, started_rx) = mpsc::sync_channel(1);
            let thread = thread::Builder::new()
                .name("coding-pipe".into())
                .spawn(move || run(meta, tx, worker_shutdown, started_tx))
                .map_err(CodingError::from)?;
            match started_rx.recv() {
                Ok(Ok(())) => Ok(Self {
                    shutdown,
                    thread: Some(thread),
                }),
                Ok(Err(err)) => {
                    let _ = thread.join();
                    Err(err)
                }
                Err(_) => {
                    let _ = thread.join();
                    Err(error(
                        ErrorCode::HostUnavailable,
                        "pipe server failed to start",
                    ))
                }
            }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub(crate) fn call(
        meta: &SessionMetadata,
        expected_host: &ProcessIdentity,
        request: &CodingRequest,
    ) -> Result<CodingReply> {
        request.validate()?;
        let name = wide(&platform::endpoint(meta)?);
        // SAFETY: 名称以零结尾，使用非继承句柄及 overlapped 模式。
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io_error("opening host pipe"));
        }
        let pipe = Handle(handle);
        let pid = peer_pid(pipe.0, true)?;
        if pid != expected_host.pid || !platform::identity_alive(expected_host)? {
            return Err(error(
                ErrorCode::HostIdentityMismatch,
                "pipe server process identity mismatch",
            ));
        }
        let envelope = HostEnvelope {
            session_id: meta.session_id.clone(),
            host_instance_id: meta.host_instance_id.clone(),
            request: request.clone(),
        };
        let bytes = serde_json::to_vec(&envelope)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(error(
                ErrorCode::InvalidRequest,
                "pipe frame exceeds size limit",
            ));
        }
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        write_all(pipe.0, &(bytes.len() as u32).to_be_bytes(), None, deadline)
            .and_then(|()| write_all(pipe.0, &bytes, None, deadline))
            .map_err(|e| {
                error(
                    ErrorCode::UnknownOutcome,
                    format!("sending host request: {e}"),
                )
            })?;
        let reply = read_frame(pipe.0, None, deadline).map_err(|e| {
            error(
                ErrorCode::UnknownOutcome,
                format!("receiving host reply: {e}"),
            )
        })?;
        write_all(pipe.0, &[1], None, deadline).map_err(|e| {
            error(
                ErrorCode::UnknownOutcome,
                format!("acknowledging host reply: {e}"),
            )
        })?;
        Ok(reply)
    }
}

#[cfg(windows)]
pub(crate) use windows::{call, Server};
