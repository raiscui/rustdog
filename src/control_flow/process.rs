use std::{
    collections::BTreeMap,
    io,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::control_actions::{
    build_shell_command, isolate_process_group, terminate_process_tree, TerminationSignal,
};

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
    isolate_process_group(&mut command);

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
                break terminate_timed_out_child(&mut child, child_id)?;
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

fn terminate_timed_out_child(
    child: &mut Child,
    child_id: u32,
) -> Result<Option<std::process::ExitStatus>, String> {
    terminate_process_tree(child, child_id, TerminationSignal::Terminate)
        .map_err(|err| format!("终止 flow 命令进程树失败: {err}"))?;
    let grace_deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < grace_deadline {
        if let Some(status) = child
            .try_wait()
            .map_err(|err| format!("等待 flow 命令状态失败: {err}"))?
        {
            return Ok(Some(status));
        }
        thread::sleep(Duration::from_millis(10));
    }

    terminate_process_tree(child, child_id, TerminationSignal::Kill)
        .map_err(|err| format!("强制终止 flow 命令进程树失败: {err}"))?;
    child
        .wait()
        .map(Some)
        .map_err(|err| format!("收割 flow 命令失败: {err}"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn flow_shell_should_not_allocate_a_console() {
        let step = FlowCmdStep {
            run: concat!(
                "Add-Type -Namespace Rdog -Name ConsoleProbe -MemberDefinition ",
                "'[DllImport(\"kernel32.dll\")] public static extern ",
                "System.IntPtr GetConsoleWindow();'; ",
                "[Rdog.ConsoleProbe]::GetConsoleWindow().ToInt64()"
            )
            .to_owned(),
            shell: Some("powershell.exe".to_owned()),
            cwd: None,
            env: BTreeMap::new(),
            timeout_ms: Some(10_000),
            capture: None,
        };
        let result = execute_cmd_step(
            &step,
            "powershell.exe",
            &FlowPolicy::default(),
            Duration::from_secs(10),
        )
        .expect("flow shell 应成功运行 console probe");

        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert_eq!(result.stdout.trim(), "0");
    }
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
