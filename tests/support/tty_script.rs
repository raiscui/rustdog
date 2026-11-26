//! `script(1)` 伪终端夹具 (RAI-410)。
//!
//! 两个互不兼容的调用形式:
//!
//! - util-linux (`script from util-linux 2.39.3` 实测): 只接受
//!   `script -q -c "CMD ARGS..." /dev/null`; 把命令当尾随参数传给它是
//!   `script: unexpected number of arguments` 并立刻以非零码退出 ——
//!   子进程 stdin 随后写什么都只能是 BrokenPipe。
//! - BSD / macOS (`script [-q] file command...`): 只接受尾随命令形式,
//!   没有 `-c`。
//!
//! 所以这里不按 OS 名猜, 而是探一次 `script -c` 能力并缓存结果: 探测成功走
//! `-c`, 否则走尾随命令。探测失败 (含 `script` 根本不存在) 走尾随形式, 让
//! 调用点照常拿到 spawn 错误, 不静默跳过任何用例。
#![allow(dead_code)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// 在伪终端里跑 `program args...`, 返回 `script` 进程的 [`Command`], 由调用点
/// 继续补 `.env(...)`, `.stdin(...)` 等并 spawn。
pub fn scripted(program: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("script");
    if script_supports_command_flag() {
        command
            .arg("-q")
            .arg("-c")
            .arg(shell_joined(program, args))
            .arg("/dev/null");
    } else {
        command.arg("-q").arg("/dev/null").arg(program).args(args);
    }
    command
}

/// 把程序与参数拼成 `-c` 需要一个 shell 命令行。
fn shell_joined(program: &Path, args: &[&str]) -> String {
    let mut joined = shell_quote(&program.to_string_lossy());
    for arg in args {
        joined.push(' ');
        joined.push_str(&shell_quote(arg));
    }
    joined
}

/// 单引号引用: 引号内的 `'` 用 `'\''` 收口再重开。`-c` 的串会交给 `sh -c`,
/// 不引用就会在含空格的路径上被拆错。
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `script` 是否支持 `-c <command>` (util-linux 形态)。
fn script_supports_command_flag() -> bool {
    static SUPPORTS_COMMAND_FLAG: OnceLock<bool> = OnceLock::new();
    *SUPPORTS_COMMAND_FLAG.get_or_init(|| {
        Command::new("script")
            .args(["-q", "-c", "exit 0", "/dev/null"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::shell_joined;
    use std::path::Path;

    #[test]
    fn command_line_should_survive_spaces_and_quotes() {
        assert_eq!(
            shell_joined(Path::new("/tmp/a b/rdog"), &["control", "it's", "--x=y z"]),
            r"'/tmp/a b/rdog' 'control' 'it'\''s' '--x=y z'"
        );
    }
}
