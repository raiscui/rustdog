use crate::input::Command;
use std::path::PathBuf;

#[cfg(windows)]
use std::path::Path;

#[cfg(any(windows, test))]
use std::sync::atomic::{AtomicBool, Ordering};

// ============================================================
// 这份模块只负责“隐藏常驻模式”的少量全局状态和入口辅助。
// 普通模式不应依赖这里的状态,避免把旧语义一起拖宽。
// ============================================================
#[cfg(any(windows, test))]
static HIDDEN_SESSION_ENABLED: AtomicBool = AtomicBool::new(false);

pub enum LogTarget {
    Stdout,
    File(PathBuf),
}

#[cfg(any(windows, test))]
pub fn enable_hidden_session() {
    HIDDEN_SESSION_ENABLED.store(true, Ordering::Relaxed);
}

#[cfg(any(windows, test))]
pub fn hidden_session_enabled() -> bool {
    HIDDEN_SESSION_ENABLED.load(Ordering::Relaxed)
}

pub fn log_target_for_command(command: &Command) -> LogTarget {
    match command {
        Command::HiddenDaemon {
            child: true,
            log_file: Some(path),
            ..
        } => LogTarget::File(path.clone()),
        _ => LogTarget::Stdout,
    }
}

#[cfg(windows)]
pub fn spawn_hidden_daemon_process(
    config_path: Option<&Path>,
    log_file: &Path,
) -> std::io::Result<()> {
    use std::env;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    // ------------------------------------------------------------
    // 这里采用“父进程负责拉起隐藏 child, 然后立即退出”的模式。
    // 这样既不需要把整个二进制改成 windows_subsystem,也不会污染
    // 现有命令的控制台行为。
    // ------------------------------------------------------------
    let current_exe = env::current_exe()?;
    let mut command = Command::new(current_exe);
    command.arg("hidden-daemon").arg("--child");
    command.arg("--log-file").arg(log_file);

    if let Some(config_path) = config_path {
        command.arg("--config").arg(config_path);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Transport;
    use std::path::PathBuf;

    #[test]
    fn log_target_should_use_stdout_for_normal_daemon() {
        let command = Command::Daemon {
            config: None,
            transport: Some(Transport::Tcp),
            namespace: None,
            daemon_name: None,
            entry_point: Vec::new(),
        };

        match log_target_for_command(&command) {
            LogTarget::Stdout => {}
            LogTarget::File(path) => panic!("unexpected file target: {}", path.display()),
        }
    }

    #[test]
    fn log_target_should_use_file_for_hidden_child() {
        let command = Command::HiddenDaemon {
            config: None,
            child: true,
            log_file: Some(PathBuf::from("rdog_hidden.log")),
        };

        match log_target_for_command(&command) {
            LogTarget::File(path) => assert_eq!(path, PathBuf::from("rdog_hidden.log")),
            LogTarget::Stdout => panic!("hidden child should not use stdout"),
        }
    }

    #[test]
    fn hidden_session_flag_should_enable_once_set() {
        HIDDEN_SESSION_ENABLED.store(false, Ordering::Relaxed);
        assert!(!hidden_session_enabled());

        enable_hidden_session();
        assert!(hidden_session_enabled());
    }
}
