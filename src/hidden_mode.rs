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
    /// 非 hidden 命令的日志目标:走 stderr(Unix 习惯)。
    ///
    /// 历史背景: 早期(2026-06-19 之前)这个 variant 叫 `Stdout` 且真的写 stdout,
    /// 改 init_logger 走 stderr 之后保留 `Stdout` 名字做向后兼容。
    /// 2026-06-20 改名为 `Stderr`,把"日志走 stderr"事实写进 enum 名字。
    Stderr,
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
        _ => LogTarget::Stderr,
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
            LogTarget::Stderr => {}
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
            // 这个测试只是验证 hidden child 不会落到 stderr 这条分支。
            // (注:LogTarget 早期叫 Stdout 且真的写 stdout,2026-06-19 切到 stderr,
            // 2026-06-20 改名为 Stderr 以反映实际语义。)
            LogTarget::Stderr => panic!("hidden child should not use stderr"),
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
