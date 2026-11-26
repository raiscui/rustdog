//! 跨模块单测共享的进程级状态锁。
//!
//! `cargo test` 默认 harness 在同一进程内多线程并行跑用例, 进程级状态
//! (cwd / 环境变量 / 全局 subscriber) 因此被隐式共享。纪律与已踩坑案例见
//! `docs/solutions/best-practices/parallel-test-global-state-single-lock.md`。

use std::sync::{Mutex, MutexGuard};

/// 串行化"改进程级 cwd 的用例"与"子进程依赖 cwd 的用例"。
///
/// `figment::Jail` 会把进程 cwd 切到自己的临时目录, 退出后再把目录删掉;
/// 它内部的锁只串行化 Jail 之间 (私有 static, 外部拿不到), 挡不住并行启动
/// `/bin/sh` 的用例 —— 子进程继承到即将被删的 cwd 后 `getcwd` 失败, 输出
/// `shell-init: error retrieving current directory`, 于是本该"成功无 stderr"
/// 的脚本变成 `shell_stderr` 结构化失败。
///
/// 锁必须全局唯一: 各用例各自 `static Mutex` 等于各自为政。
static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

/// 取 cwd 串行锁。两个方向的用例都取: 切/删 cwd 的一方 (Jail),
/// 以及 spawn 子进程且对 stderr/stdout 做严格断言的一方。
pub(crate) fn cwd_test_guard() -> MutexGuard<'static, ()> {
    CWD_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 串行化"真的启动 shell 子进程"的用例。
///
/// 2026-11-26 实测 (RAI-429): windows runner 上 `build_shell_command("powershell.exe", …)`
/// 偶发宿主级失败 —— PowerShell 起来了但托管运行时装载不成功, stderr 为
/// `Internal Windows PowerShell error ... failed with error 8009001d`。被测对象是 builder
/// 的建台标志, 不是 PowerShell 自己能不能起来, 所以两件事一起做: 让"启动 shell 的用例"
/// 互斥 (降低同时涌入的 console-less 子进程数), 再对**启动失败**做有界重试。
///
/// 与 `cwd_test_guard` 分开是刻意的: 那条锁保护的是 cwd 语义, 持锁时间短得多,
/// 混用会把无关用例也拖成串行。
static SHELL_SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// 取 shell 子进程串行锁。
pub(crate) fn shell_spawn_guard() -> MutexGuard<'static, ()> {
    SHELL_SPAWN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Windows「子进程有没有分配到控制台」探针: 脚本 + 读数口径 (RAI-563)。
///
/// 仓里三条 spawn 路径各验一次同一件事 —— builder 的建台标志 (`control_actions`)、
/// flow step (`control_flow::process`)、后台 task (`task_control`)。三条**必须各自保留**
/// (被测的是各自的 spawn 实现), 但探针脚本与读数只能有一份, 否则同一个仪器缺陷有三个副本。
///
/// 探针靠 `Add-Type -MemberDefinition` 在运行时编译 C#, 所以**它自己会失败**。仪器坏了与
/// 被测性质破了曾经共用同一个退出码, 日志里只剩 `left: Some(1)` / `right: Some(0)`;
/// 现在编译失败打 `PROBE_COMPILE_FAILED` 并以专用退出码退出, 读数先判仪器再判句柄。
///
/// 本模块不按平台门控: 脚本与读数都是纯数据与纯判断, 只有「谁来 spawn 它」是 windows 的事。
/// 这样分类逻辑在三个平台的 Linux job 上都能被 `cargo test` 跑到, 不必等 windows runner。
pub(crate) mod console_probe {
    /// 探针自身编译失败时打到 stdout 的标记。
    pub(crate) const COMPILE_FAILED_MARKER: &str = "PROBE_COMPILE_FAILED";

    /// 探针自身编译失败时的专用退出码, 与被测性质的任何读数都不重叠。
    pub(crate) const COMPILE_FAILED_EXIT: i32 = 73;

    /// 探针编出来了但调用 `GetConsoleWindow()` 失败 (P/Invoke 层) 时的标记。
    pub(crate) const CALL_FAILED_MARKER: &str = "PROBE_CALL_FAILED";

    /// 调用阶段的专用退出码, 与编译失败、与被测性质都不重叠。
    pub(crate) const CALL_FAILED_EXIT: i32 = 74;

    /// 探针脚本。仪器可用时 stdout 只有 `GetConsoleWindow()` 的十进制值。
    ///
    /// 两个阶段各自留标记与专用退出码, 因为它们在日志里必须长得不一样:
    ///   1. 编译 —— `try/catch` 把编译器的原话留给日志, 但真正判「仪器可用」的是
    ///      `'Rdog.ConsoleProbe' -as [type]`: 只要类型没建起来 (编译失败, 或失败没抛成
    ///      终止错误), 都走同一个标记, 不再掉进「类型找不到」的 exit 1;
    ///   2. 调用 —— 类型有了但 `GetConsoleWindow()` 本身失败 (P/Invoke 层, 例如在非 windows
    ///      上跑同一条脚本), 与「句柄为 0」是两件事。
    pub(crate) fn script() -> String {
        format!(
            "$compileError = $null; \
             try {{ Add-Type -Namespace Rdog -Name ConsoleProbe -MemberDefinition \
             '[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetConsoleWindow();' \
             -ErrorAction Stop }} \
             catch {{ $compileError = $_.Exception.Message }}; \
             if (-not ('Rdog.ConsoleProbe' -as [type])) {{ \
             Write-Output '{compile_marker}'; \
             if ($compileError) {{ Write-Error $compileError }}; \
             exit {compile_exit} }}; \
             try {{ $handle = [Rdog.ConsoleProbe]::GetConsoleWindow().ToInt64() }} \
             catch {{ Write-Output '{call_marker}'; \
             Write-Error $_.Exception.Message; exit {call_exit} }}; \
             $handle",
            compile_marker = COMPILE_FAILED_MARKER,
            compile_exit = COMPILE_FAILED_EXIT,
            call_marker = CALL_FAILED_MARKER,
            call_exit = CALL_FAILED_EXIT,
        )
    }

    /// 一次探针运行的读数。
    pub(crate) enum Reading {
        /// 仪器可用, 子进程没有控制台 —— 期望值。
        NoConsole,
        /// 仪器可用, 但子进程拿到了控制台: 建台标志回退了。
        Allocated(i64),
        /// 读数不成立: 与「子进程是否分到控制台」无关, 不许被读成任一方向。
        Unavailable(String),
    }

    /// `stdout` 传探针的 stdout (`task_control` 那条路径的 stdout 与 stderr 合流)。
    ///
    /// `Unavailable` 的文案必须点明**哪个阶段**失败: 下一个读日志的人要能分开
    /// 「编译失败」「调用失败」「根本没跑起来」, 而不是看到一个笼统的「探针不可用」。
    pub(crate) fn read(exit_code: Option<i32>, stdout: &str, stderr: &str) -> Reading {
        let stdout = stdout.trim();
        let stderr = stderr.trim();
        if stdout.contains(COMPILE_FAILED_MARKER) || exit_code == Some(COMPILE_FAILED_EXIT) {
            return Reading::Unavailable(format!(
                "阶段一 编译失败: Add-Type 没把探针类型编出来\n  stdout: {stdout}\n  stderr: {stderr}"
            ));
        }
        if stdout.contains(CALL_FAILED_MARKER) || exit_code == Some(CALL_FAILED_EXIT) {
            return Reading::Unavailable(format!(
                "阶段二 调用失败: 探针编出来了, 但 GetConsoleWindow() 本身抛错 (P/Invoke 层)\n  \
                 stdout: {stdout}\n  stderr: {stderr}"
            ));
        }
        if exit_code.is_none() {
            return Reading::Unavailable(format!(
                "脚本没有正常退出 (超时或被杀), 读数不成立\n  stdout: {stdout}\n  stderr: {stderr}"
            ));
        }
        if exit_code != Some(0) {
            return Reading::Unavailable(format!(
                "脚本以意外退出码结束 (exit {exit_code:?}), 没有走到读数\n  \
                 stdout: {stdout}\n  stderr: {stderr}"
            ));
        }
        match stdout.parse::<i64>() {
            Ok(0) => Reading::NoConsole,
            Ok(handle) => Reading::Allocated(handle),
            Err(err) => Reading::Unavailable(format!(
                "退出码 0 但 stdout 不是窗口句柄数值 ({err})\n  stdout: {stdout}\n  stderr: {stderr}"
            )),
        }
    }

    /// 断言子进程没有控制台。仪器坏时说「探针不可用」, 性质破时说拿到了句柄多少。
    pub(crate) fn assert_no_console(exit_code: Option<i32>, stdout: &str, stderr: &str) {
        match read(exit_code, stdout, stderr) {
            Reading::NoConsole => {}
            Reading::Allocated(handle) => {
                panic!("子进程拿到了控制台 (GetConsoleWindow = {handle}), 建台标志回退了")
            }
            Reading::Unavailable(reason) => panic!("console 探针不可用: {reason}"),
        }
    }

    /// Windows PowerShell 的重定向错误流可能是 UTF-16LE, 先认编码再解码。
    pub(crate) fn decode_stderr(bytes: &[u8]) -> String {
        let is_utf16 = bytes.len() > 1
            && bytes.len().is_multiple_of(2)
            && bytes.chunks(2).take(8).all(|pair| pair[1] == 0);
        if is_utf16 {
            String::from_utf16_lossy(
                &bytes
                    .chunks(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>(),
            )
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

/// 探针读数口径的用例 —— 不 spawn 任何进程, 所以三个平台都跑。
///
/// 真正的 PowerShell 采样只在 windows 上做 (三条 spawn 路径各自的用例); 这里钉住的是
/// 「一次采样的结果怎么读」: 仪器坏了必须与性质破了分开, 且不许互相冒充。
#[cfg(test)]
mod console_probe_tests {
    use super::console_probe::{self, Reading};

    fn unavailable(reading: Reading) -> String {
        match reading {
            Reading::Unavailable(reason) => reason,
            Reading::NoConsole => panic!("应读到「仪器不可用」, 实际读到无控制台"),
            Reading::Allocated(handle) => panic!("应读到「仪器不可用」, 实际读到句柄 {handle}"),
        }
    }

    #[test]
    fn compile_failure_marker_is_read_as_instrument_failure() {
        let reason = unavailable(console_probe::read(
            Some(1),
            "PROBE_COMPILE_FAILED\nAdd-Type : 类型已存在",
            "",
        ));
        assert!(reason.contains("阶段一 编译失败"), "{reason}");

        // 标记丢了、只有专用退出码时, 也必须读成编译失败。
        assert!(
            unavailable(console_probe::read(Some(73), "", "")).contains("阶段一 编译失败"),
            "专用退出码单独出现时也要认出编译失败"
        );
    }

    /// 编译失败与调用失败必须是两句不同的话 —— 它们在真机上对应完全不同的处置。
    #[test]
    fn call_failure_is_read_separately_from_compile_failure() {
        let compile = unavailable(console_probe::read(Some(73), "PROBE_COMPILE_FAILED", ""));
        let call = unavailable(console_probe::read(
            Some(74),
            "PROBE_CALL_FAILED",
            "Exception calling GetConsoleWindow",
        ));
        assert!(call.contains("阶段二 调用失败"), "{call}");
        assert!(call.contains("P/Invoke"), "{call}");
        assert!(!call.contains("阶段一"), "{call}");
        assert_ne!(compile, call, "两个阶段的文案不许合并成一句");
    }

    #[test]
    fn console_allocation_is_read_as_property_break() {
        assert!(matches!(
            console_probe::read(Some(0), "1310742\n", ""),
            Reading::Allocated(1_310_742)
        ));
        assert!(matches!(
            console_probe::read(Some(0), "0", ""),
            Reading::NoConsole
        ));
    }

    #[test]
    fn anything_else_is_instrument_failure_not_a_verdict() {
        // 没有退出码 = 超时或被杀: 读数根本不成立, 不许被读成「分了控制台」。
        let timed_out = unavailable(console_probe::read(None, "", ""));
        assert!(timed_out.contains("超时或被杀"), "{timed_out}");
        // 意外退出码又没有标记: 也不构成任何一方的证据。
        let unexpected = unavailable(console_probe::read(Some(1), "", "boom"));
        assert!(unexpected.contains("意外退出码"), "{unexpected}");
        assert_ne!(timed_out, unexpected, "超时与意外退出码要能分辨");
        // 退出码 0 但 stdout 不是句柄数值: 这条曾是最容易假绿的一个形状。
        assert!(unavailable(console_probe::read(Some(0), "hello", "")).contains("不是窗口句柄数值"));
    }

    /// 断言文本是这张票的交付物之一: 仪器坏时读的人必须一眼看到「探针不可用」。
    #[test]
    fn assertion_names_the_instrument_when_the_probe_did_not_build() {
        let outcome = std::panic::catch_unwind(|| {
            console_probe::assert_no_console(Some(73), "PROBE_COMPILE_FAILED", "Add-Type : 编译失败")
        })
        .expect_err("仪器失败不应通过断言");
        let message = outcome
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(message.starts_with("console 探针不可用"), "{message}");
        assert!(message.contains(console_probe::COMPILE_FAILED_MARKER), "{message}");
        assert!(!message.contains("拿到了控制台"), "{message}");
    }

    /// 脚本与读数常量必须同值 —— 否则改了一处就会把仪器失败读成别的。
    #[test]
    fn script_and_reading_agree_on_marker_and_exit_code() {
        let script = console_probe::script();
        assert!(
            script.contains(&format!("'{}'", console_probe::COMPILE_FAILED_MARKER)),
            "脚本必须打出标记: {script}"
        );
        assert!(
            script.contains(&format!("exit {}", console_probe::COMPILE_FAILED_EXIT)),
            "脚本必须用专用退出码: {script}"
        );
        assert!(
            script.contains(&format!("'{}'", console_probe::CALL_FAILED_MARKER)),
            "脚本必须把调用阶段的失败单独打出来: {script}"
        );
        assert!(
            script.contains(&format!("exit {}", console_probe::CALL_FAILED_EXIT)),
            "调用阶段必须有独立退出码: {script}"
        );
        assert!(
            script.contains("-as [type]"),
            "仪器可用性必须由类型是否存在来判: {script}"
        );
    }

    #[test]
    fn stderr_is_decoded_from_utf16_when_that_is_what_came_back() {
        let utf16: Vec<u8> = "Internal Windows PowerShell error".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            console_probe::decode_stderr(&utf16),
            "Internal Windows PowerShell error"
        );
        assert_eq!(console_probe::decode_stderr(b"plain utf8"), "plain utf8");
    }
}
