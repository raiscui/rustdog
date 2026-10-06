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
    /// 日志改道到文件。两种落到文件的形态受众不同,用 `console_observer` 区分。
    File {
        path: PathBuf,
        /// 文件背后是否还坐着一个盯着控制台的操作者。
        ///
        /// 2026-10-06 真机实测 (Windows 11 / ssh 前台会话): 同一份 rdog.exe、同一条
        /// `rdog daemon -c bad.toml`, 唯一差别是设不设 `RDOG_LOG_FILE`:
        ///   - 不设 → stderr 519 字节 (info 横幅 + warn + error 全在)
        ///   - 设了 → stderr **0 字节**, 同样 519 字节进了文件
        /// 环境变量一旦被 `setx` 写进用户环境, 后面每一条命令都走"改道"这条路,
        /// 而 stderr 一个字节都没有、也没有任何提示说明日志去了哪 ——
        /// 现场据此把"日志去文件了"误判成"daemon 静默崩溃"。
        ///
        /// 两种形态的差别就在"还有没有人看控制台":
        ///   - hidden child 由 `CREATE_NO_WINDOW | DETACHED_PROCESS` 拉起, stderr 接的是
        ///     `Stdio::null()`, 操作者根本不在 → 通知没有收件人, 只会变成无意义的写入失败。
        ///   - `rdog daemon` + `RDOG_LOG_FILE` 是**前台**改道, 操作者就站在控制台前,
        ///     他需要知道失败原因 —— 哪怕日志主体被改走了。
        console_observer: bool,
    },
}

/// 长驻命令把日志改道到文件的环境变量。
///
/// 2026-10-05 在 Windows 上用 `Start-Process` 起 daemon,stderr 绑在一个转瞬即逝的
/// 新控制台窗口上,窗口一关日志就没了 —— 于是"进程消失"被当成静默崩溃,排查为此多绕了
/// 好几轮。常驻命令的调用方常常是计划任务 / 自动化,stderr 根本没人接,必须能落盘。
///
/// 这里用环境变量而不是给 `rdog daemon` 加 `--log-file`:子命令的 clap 定义不在本模块,
/// 而常驻形态的日志落点本来就有配置真相源(`[hidden].log_file`,hidden child 走它),
/// 环境变量只作为显式逃生口,不另立一条默认路径。
pub const LOG_FILE_ENV: &str = "RDOG_LOG_FILE";

#[cfg(any(windows, test))]
pub fn enable_hidden_session() {
    HIDDEN_SESSION_ENABLED.store(true, Ordering::Relaxed);
}

#[cfg(any(windows, test))]
pub fn hidden_session_enabled() -> bool {
    HIDDEN_SESSION_ENABLED.load(Ordering::Relaxed)
}

pub fn log_target_for_command(command: &Command) -> LogTarget {
    resolve_log_target(command, std::env::var(LOG_FILE_ENV).ok().as_deref())
}

/// 日志目标的判定本体,环境变量值由调用方传入。
///
/// 拆出纯函数是为了让单测不碰进程级环境变量 —— Rust 测试默认多线程并行,
/// 真在测试里 `set_var` 会和"daemon 默认走 stderr"的用例互相污染。
fn resolve_log_target(command: &Command, env_log_file: Option<&str>) -> LogTarget {
    match command {
        // hidden child 的日志路径由 parent 通过 --log-file 钉死,优先级最高:
        // parent/child 两侧的 config 校验已经比对过这条路径,环境变量不许在子进程里二次改道。
        Command::HiddenDaemon {
            child: true,
            log_file: Some(path),
            ..
        } => LogTarget::File {
            path: path.clone(),
            // child 已经脱离控制台, 没有任何操作者等着看 stderr。
            console_observer: false,
        },
        // `rdog daemon` 默认仍走 stderr(前台语义不变),但允许用 RDOG_LOG_FILE 显式改道。
        // 只对 daemon 生效:hidden-daemon 的 parent 是瞬时进程,它的落点由下面的 spawn
        // 告知行负责,让它也读环境变量只会把 parent 和 child 的日志拆到两个文件。
        Command::Daemon { .. } => match env_log_file.map(str::trim) {
            Some(path) if !path.is_empty() => LogTarget::File {
                path: PathBuf::from(path),
                // 这是前台改道, 操作者还在控制台前 —— 失败必须让他看得见。
                console_observer: true,
            },
            _ => LogTarget::Stderr,
        },
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

        // 显式传 None 而不是走 log_target_for_command: 后者读进程环境变量,
        // 谁在挂了 RDOG_LOG_FILE 的 shell 里跑测试, 这条用例就会假红。
        match resolve_log_target(&command, None) {
            LogTarget::Stderr => {}
            LogTarget::File { path, .. } => panic!("unexpected file target: {}", path.display()),
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
            LogTarget::File { path, .. } => assert_eq!(path, PathBuf::from("rdog_hidden.log")),
            // 这个测试只是验证 hidden child 不会落到 stderr 这条分支。
            // (注:LogTarget 早期叫 Stdout 且真的写 stdout,2026-06-19 切到 stderr,
            // 2026-06-20 改名为 Stderr 以反映实际语义。)
            LogTarget::Stderr => panic!("hidden child should not use stderr"),
        }
    }

    fn daemon_command() -> Command {
        Command::Daemon {
            config: None,
            transport: Some(Transport::Tcp),
            namespace: None,
            daemon_name: None,
            entry_point: Vec::new(),
        }
    }

    #[test]
    fn log_target_should_use_env_file_for_daemon() {
        // 2026-10-05 的直接教训: 常驻命令的 stderr 常常绑在一个没人接的控制台上,
        // 窗口一关日志就没了。RDOG_LOG_FILE 是这条逃生口。
        match resolve_log_target(&daemon_command(), Some("rdog_daemon.log")) {
            LogTarget::File { path, .. } => assert_eq!(path, PathBuf::from("rdog_daemon.log")),
            LogTarget::Stderr => panic!("daemon with RDOG_LOG_FILE should write to file"),
        }
    }

    #[test]
    fn log_target_should_fall_back_to_stderr_for_blank_env_file() {
        // 空白/未设置都视为"没给路径": Windows 计划任务里空环境变量很常见,
        // 把它当成路径会在 init_logger 阶段直接失败,daemon 根本起不来。
        for value in [None, Some(""), Some("   ")] {
            match resolve_log_target(&daemon_command(), value) {
                LogTarget::Stderr => {}
                LogTarget::File { path, .. } => {
                    panic!("blank RDOG_LOG_FILE {value:?} should fall back to stderr, got {path:?}")
                }
            }
        }
    }

    #[test]
    fn log_target_should_not_leak_env_file_to_other_commands() {
        // 环境变量只对 daemon 生效: control / listen 这类前台命令的 stdout 是解析面,
        // 让它们改道会把 agent 的管道输出搅乱。
        let command = Command::Connect {
            shell: "powershell.exe".to_string(),
            mode: crate::shell::ShellMode::Interactive,
            host: vec!["127.0.0.1".to_string(), "5555".to_string()],
        };

        match resolve_log_target(&command, Some("rdog_daemon.log")) {
            LogTarget::Stderr => {}
            LogTarget::File { path, .. } => {
                panic!("connect should stay on stderr, got {}", path.display())
            }
        }
    }

    #[test]
    fn hidden_child_log_file_should_win_over_env_file() {
        // parent/child 两侧会比对 --log-file 与 config.hidden.log_file 是否一致,
        // 子进程里再被环境变量改道会让这条校验形同虚设。
        let command = Command::HiddenDaemon {
            config: None,
            child: true,
            log_file: Some(PathBuf::from("rdog_hidden.log")),
        };

        match resolve_log_target(&command, Some("rdog_daemon.log")) {
            LogTarget::File { path, .. } => assert_eq!(path, PathBuf::from("rdog_hidden.log")),
            LogTarget::Stderr => panic!("hidden child should not use stderr"),
        }
    }

    /// 两条落到文件的路径,受众不同 —— 这是 2026-10-06 修复的判据本身。
    ///
    /// 判错任一方向都会坏: 把 hidden child 也当成"有人在看", 每次启动都往
    /// `Stdio::null()` 上白写一次错误, 纯噪音; 反过来把前台改道当成"没人看",
    /// 就退回成今天这样"操作者敲一条命令、静默退出、stderr 0 字节"。
    #[test]
    fn file_target_should_mark_who_the_console_observer_is() {
        match resolve_log_target(&daemon_command(), Some("rdog_daemon.log")) {
            LogTarget::File {
                path,
                console_observer,
            } => {
                assert_eq!(path, PathBuf::from("rdog_daemon.log"));
                assert!(
                    console_observer,
                    "前台 daemon 被 RDOG_LOG_FILE 改道时操作者还在控制台前"
                );
            }
            LogTarget::Stderr => panic!("daemon with RDOG_LOG_FILE should write to file"),
        }

        let child = Command::HiddenDaemon {
            config: None,
            child: true,
            log_file: Some(PathBuf::from("rdog_hidden.log")),
        };
        match resolve_log_target(&child, None) {
            LogTarget::File {
                path,
                console_observer,
            } => {
                assert_eq!(path, PathBuf::from("rdog_hidden.log"));
                assert!(
                    !console_observer,
                    "hidden child 已脱离控制台, 不该再往 stderr 通知"
                );
            }
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
