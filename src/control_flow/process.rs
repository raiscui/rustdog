use std::{
    collections::BTreeMap,
    io,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::control_actions::build_shell_command;

use super::{FlowCmdStep, FlowCommandResult, FlowPolicy, FlowScriptStep};

pub(super) fn execute_cmd_step(
    step: &FlowCmdStep,
    default_shell: &str,
    policy: &FlowPolicy,
    flow_remaining: Duration,
) -> Result<FlowCommandResult, String> {
    let shell = step.shell.as_deref().unwrap_or(default_shell);
    let mut command = build_shell_command(shell, &step.run);
    apply_command_context(&mut command, step.cwd.as_deref(), &step.env);
    let timeout = step_timeout(policy, step.timeout_ms, flow_remaining);
    execute_command_with_timeout(command, timeout, policy.max_output_bytes)
}

pub(super) fn execute_script_step(
    step: &FlowScriptStep,
    default_shell: &str,
    policy: &FlowPolicy,
    flow_remaining: Duration,
) -> Result<FlowCommandResult, String> {
    let shell = step.shell.as_deref().unwrap_or(default_shell);
    let mut command = build_shell_command(shell, &step.text);
    apply_command_context(&mut command, step.cwd.as_deref(), &step.env);
    let timeout = step_timeout(policy, step.timeout_ms, flow_remaining);
    execute_command_with_timeout(command, timeout, policy.max_output_bytes)
}

fn apply_command_context(command: &mut Command, cwd: Option<&str>, env: &BTreeMap<String, String>) {
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    for (key, value) in env {
        command.env(key, value);
    }
}

fn execute_command_with_timeout(
    mut command: Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<FlowCommandResult, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    set_command_process_group(&mut command);

    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|err| format!("启动命令失败: {err}"))?;
    let child_id = child.id();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法接管命令 stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法接管命令 stderr".to_owned())?;
    let stdout_handle = thread::spawn(move || read_stream_limited(stdout, max_output_bytes));
    let stderr_handle = thread::spawn(move || read_stream_limited(stderr, max_output_bytes));

    let deadline = start + timeout;
    let mut timed_out = false;
    let status = loop {
        match child
            .try_wait()
            .map_err(|err| format!("等待命令状态失败: {err}"))?
        {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                timed_out = true;
                break terminate_timed_out_child(&mut child, child_id);
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };

    let stdout = join_stream_reader(stdout_handle)?;
    let stderr = join_stream_reader(stderr_handle)?;
    let duration_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    Ok(FlowCommandResult {
        exit_code: status.and_then(|status| status.code()),
        stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
        duration_ms,
        timed_out,
        truncated: stdout.truncated || stderr.truncated,
    })
}

fn terminate_timed_out_child(child: &mut Child, child_id: u32) -> Option<std::process::ExitStatus> {
    terminate_process_tree(child, child_id, TerminationSignal::Terminate);
    let grace_deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < grace_deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        thread::sleep(Duration::from_millis(10));
    }

    terminate_process_tree(child, child_id, TerminationSignal::Kill);
    child.wait().ok()
}

enum TerminationSignal {
    Terminate,
    Kill,
}

#[cfg(unix)]
fn set_command_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
fn set_command_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child, child_id: u32, signal: TerminationSignal) {
    // 2026-08-28 CI 实测 (strace): 外部 /usr/bin/kill 在 GNU coreutils 上会把
    // ["-TERM", "-<pid>"] 解析成 kill(-2, SIGTERM) — 信号发往进程组 2 而非
    // 目标组, ESRCH 静默失败。孤儿子孙进程因此存活并持有 stdout/stderr 管道
    // 写端, join_stream_reader 阻塞到其自然退出, shell_lane 超时上限被突破
    // (ubuntu CI duration_ms=2001)。macOS 的 BSD kill 对同参数无此歧义,
    // 所以仅 linux 暴露。改用进程内 libc::kill(2) 直发进程组信号, 消灭
    // 外部命令的参数解析歧义 + PATH 依赖 + spawn 开销三重脆弱点。
    let signum = match signal {
        TerminationSignal::Terminate => libc::SIGTERM,
        TerminationSignal::Kill => libc::SIGKILL,
    };
    // 负 pid = 进程组信号: 一次性覆盖组长 (shell) 与全部子孙 (如 sleep)
    let process_group_id = -(child_id as libc::pid_t);
    // ESRCH (组已消亡) 与其余发送失败都只能尽力而为, 与原实现的吞错语义一致
    unsafe {
        libc::kill(process_group_id, signum);
    }
    // 兜底: 直杀 child 本体 (SIGKILL 单进程), 确保组长必定终结
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate_process_tree(child: &mut Child, _child_id: u32, _signal: TerminationSignal) {
    let _ = child.kill();
}

struct LimitedStreamCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_stream_limited<R: io::Read>(
    mut reader: R,
    max_output_bytes: usize,
) -> io::Result<LimitedStreamCapture> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }

        let remaining = max_output_bytes.saturating_sub(bytes.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }

        let take = read.min(remaining);
        bytes.extend_from_slice(&buffer[..take]);
        if take < read {
            truncated = true;
        }
    }

    Ok(LimitedStreamCapture { bytes, truncated })
}

fn join_stream_reader(
    handle: thread::JoinHandle<io::Result<LimitedStreamCapture>>,
) -> Result<LimitedStreamCapture, String> {
    match handle.join() {
        Ok(result) => result.map_err(|err| format!("读取命令输出失败: {err}")),
        Err(_) => Err("读取命令输出线程 panic".to_owned()),
    }
}

fn step_timeout(
    policy: &FlowPolicy,
    step_timeout_ms: Option<u64>,
    flow_remaining: Duration,
) -> Duration {
    let step_timeout = Duration::from_millis(step_timeout_ms.unwrap_or(policy.timeout_ms));
    step_timeout.min(flow_remaining)
}
