//! `@spawn` / `@task-status` / `@task-output` / `@task-cancel` 的后台任务注册表。
//!
//! 设计口径 (specs/rdog-task-spawn-control-plan.md Phase 1):
//! - `@spawn` 提交即返回 task id, 子进程在独立线程里跑, 不阻塞任何控制 lane
//! - task id (`t-` 前缀) 是 registry 主键, 与 request id (`@cmd#42`) 分层: request id
//!   只关联单次协议请求, task id 跨请求存活直到终态回收
//! - 输出捕获: stdout/stderr 合流进内存 ring buffer, 硬上限 1MB, 超限保尾部并标记
//!   truncated; 不做落盘 (大输出场景留给 Phase 2+ 的 savefile 集成)
//! - registry 不持久化: daemon 重启后旧 task id 查询诚实返回 not_found
//! - 取消: 与 PTY close 同款的"共享 child + 轮询收割"模式。waiter 线程 50ms
//!   try_wait 轮询 (PTY 输出转发已是 25ms 轮询风格, 开销可忽略); `@task-cancel`
//!   同步 kill + 收割, 与 waiter 的竞争由 `Arc<Mutex<Option<Child>>>` 单收割方闭环
//! - 终态保留最近 MAX_FINISHED_TASKS 条后按 finished_at 回收最老条目, running
//!   超过 MAX_RUNNING_TASKS 拒绝新 spawn (资源保护, fail-fast)

use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read},
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::control_actions::{
    build_shell_command, isolate_process_group, terminate_process_tree, TerminationSignal,
};
use crate::control_frames::{ControlFrame, TaskCompletedFrame, TaskFailedFrame, TaskStartedFrame};

/// 输出 ring buffer 硬上限 (1MB): 超出部分丢头部, 保留最新尾部。
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// `@task-output` 默认返回的尾部行数 (对齐 herdr `recent` 的 80 行默认)。
pub const DEFAULT_OUTPUT_TAIL_LINES: usize = 80;
/// 并发 running 任务上限: 防止失控 client 打爆 daemon 进程表。
const MAX_RUNNING_TASKS: usize = 64;
/// 终态任务保留条数: 超过后按 finished_at 回收最老条目。
const MAX_FINISHED_TASKS: usize = 64;
/// 每任务待 drain 帧上限: legacy 路径 (未绑定 lane) 的帧滞留到条目回收, 防 pending 无界。
const MAX_PENDING_FRAMES_PER_TASK: usize = 8;
/// waiter 线程收割轮询间隔。
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Phase 1 确定性任务状态机 (spec Phase 1 简化版):
/// `spawn_failed` (进程起不来) 不进 registry, 直接以 error response 返回。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Completed,
    Failed,
    Canceled,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// stdout/stderr 合流的环形输出缓冲。
///
/// `total_written` 记录进程累计输出字节数 (含被丢弃的头部),
/// 配合 `truncated` 让消费方诚实感知"这不是全部输出"。
#[derive(Debug, Default)]
struct TaskOutputBuffer {
    data: Vec<u8>,
    total_written: u64,
    truncated: bool,
}

impl TaskOutputBuffer {
    fn append(&mut self, bytes: &[u8]) {
        self.total_written += bytes.len() as u64;
        self.data.extend_from_slice(bytes);
        if self.data.len() > MAX_OUTPUT_BYTES {
            let excess = self.data.len() - MAX_OUTPUT_BYTES;
            self.data.drain(..excess);
            self.truncated = true;
        }
    }

    /// 返回尾部 `lines` 行 (按 `\\n` 切分, 不满 `lines` 行则全量返回)。
    fn tail_lines(&self, lines: usize) -> String {
        if lines == 0 {
            return String::new();
        }
        let text = String::from_utf8_lossy(&self.data);
        let all_lines: Vec<&str> = text.split('\n').collect();
        if all_lines.len() <= lines {
            return text.into_owned();
        }
        let tail: Vec<&str> = all_lines[all_lines.len() - lines..].to_vec();
        // split 尾部若为空行 (data 以 \n 结尾) 会被 trim 掉, 与"尾部 N 行"直觉一致
        format!(
            "{}{}",
            tail.join("\n"),
            if text.ends_with('\n') { "\n" } else { "" }
        )
    }
}

/// registry 内的单个任务条目。
struct TaskEntry {
    command: String,
    seq: u64,
    state: TaskState,
    exit_code: Option<i32>,
    /// 终态时间戳: reap_finished_tasks 按 it 排序回收最老条目。
    /// (spawned_at 等时间戳 Phase 2 进度帧需要时再加, Phase 1 不存无消费字段)
    finished_at_ms: Option<u128>,
    output: Arc<Mutex<TaskOutputBuffer>>,
    /// waiter 与 cancel 共享的 child 句柄; `None` 表示已被收割方 take 走。
    child: Arc<Mutex<Option<Child>>>,
    /// spawn 请求来路的 session lane (spec §6.4): None = legacy/query 路径,
    /// 帧 static 滞留 pending 无人 drain (上限防泄漏), 语义即"静默不推"。
    session_lane: Option<String>,
    /// 待 session bridge drain 的进度帧。无条件入队 (bind 前产生的 started
    /// 帧等 bind 后一并 drain, 无丢失); drain 只取走绑定本 session 的任务。
    pending_frames: VecDeque<ControlFrame>,
}

/// `@task-status` / `@task-cancel` 的查询快照。
#[derive(Debug)]
pub struct TaskSnapshot {
    pub task_id: String,
    pub seq: u64,
    pub command: String,
    pub state: TaskState,
    pub exit_code: Option<i32>,
}

static TASKS: OnceLock<Mutex<HashMap<String, TaskEntry>>> = OnceLock::new();

/// 全局单调 task seq (spec §6.2): 每次 spawn 递增, 帧与响应携带,
/// 消费方用它检测帧丢失。daemon 重启归零, 与 registry 不持久化一致。
static TASK_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_task_seq() -> u64 {
    TASK_SEQ.fetch_add(1, Ordering::Relaxed) + 1
}

fn registry() -> &'static Mutex<HashMap<String, TaskEntry>> {
    TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

/// 生成 `t-` 前缀短 id: uuid v4 取前 8 位 hex, 足够 registry 生命周期内唯一。
fn new_task_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("t-{}", &hex[..8])
}

/// 后台 spawn 一个 shell 任务, 立即返回 task id。
///
/// 进程启动失败 (shell 二进制缺失等) 直接返回 Err —— 不进 registry,
/// 这是 spec 固定的 `spawn_failed` 边界。命令本身不存在不属于启动失败:
/// `sh -c nosuchcmd` 仍会成功起 shell, 命令以非零退出码走 `failed` 终态。
pub fn spawn_task(shell: &str, command: &str, cwd: Option<&str>) -> io::Result<String> {
    if command.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "@spawn 命令不能为空",
        ));
    }

    let mut registry = registry().lock().expect("task registry lock should work");
    let running_count = registry
        .values()
        .filter(|entry| entry.state == TaskState::Running)
        .count();
    if running_count >= MAX_RUNNING_TASKS {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            format!("@spawn 拒绝启动: running 任务已达上限 {MAX_RUNNING_TASKS}"),
        ));
    }

    let mut command_builder = build_shell_command(shell, command);
    command_builder
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_process_group(&mut command_builder);
    if let Some(cwd) = cwd.filter(|cwd| !cwd.trim().is_empty()) {
        command_builder.current_dir(cwd);
    }

    let child = command_builder
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("@spawn 进程启动失败: {err}")))?;

    let task_id = new_task_id();
    let seq = next_task_seq();
    let output = Arc::new(Mutex::new(TaskOutputBuffer::default()));
    let child_handle = Arc::new(Mutex::new(Some(child)));

    let started_frame = ControlFrame::TaskStarted(TaskStartedFrame {
        task_id: task_id.clone(),
        seq,
        command: command.to_owned(),
    });
    let mut pending_frames = VecDeque::new();
    pending_frames.push_back(started_frame);
    registry.insert(
        task_id.clone(),
        TaskEntry {
            command: command.to_owned(),
            seq,
            state: TaskState::Running,
            exit_code: None,
            finished_at_ms: None,
            output: Arc::clone(&output),
            child: Arc::clone(&child_handle),
            session_lane: None,
            pending_frames,
        },
    );

    let waiter_task_id = task_id.clone();
    thread::spawn(move || run_task_waiter(waiter_task_id, child_handle, output));
    Ok(task_id)
}

/// waiter 线程: 读流 + 轮询收割。
///
/// 收割协议: `Arc<Mutex<Option<Child>>>` 只有一方能 `take` 走 child 收割终态
/// (waiter 的 try_wait 成功路径, 或 cancel 的 kill+wait 路径);
/// 另一方看到 `None` 时说明终态已被写定, 直接退出, 不重复更新状态。
fn run_task_waiter(
    task_id: String,
    child_handle: Arc<Mutex<Option<Child>>>,
    output: Arc<Mutex<TaskOutputBuffer>>,
) {
    let (stdout, stderr) = {
        let mut guard = child_handle.lock().expect("task child lock should work");
        let child = guard.as_mut().expect("waiter 启动时 child 必然存在");
        (child.stdout.take(), child.stderr.take())
    };

    // stdout/stderr 各一个 reader 线程: 阻塞读到 EOF (进程退出或被 kill 后管道关闭),
    // 把字节合流写进共享 buffer。顺序可能交错, 与终端合流语义一致。
    // ChildStdout / ChildStderr 是不同类型, 不能塞同一个 Vec, 分别 spawn。
    let mut reader_handles = Vec::new();
    if let Some(stdout) = stdout {
        let output = Arc::clone(&output);
        reader_handles.push(thread::spawn(move || pump_stream(stdout, &output)));
    }
    if let Some(stderr) = stderr {
        let output = Arc::clone(&output);
        reader_handles.push(thread::spawn(move || pump_stream(stderr, &output)));
    }

    loop {
        let exit_code = {
            let mut guard = child_handle.lock().expect("task child lock should work");
            let Some(child) = guard.as_mut() else {
                // cancel 方已收割 (take) — 终态已写定, waiter 退出
                break;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    guard.take();
                    Some(status.code().unwrap_or(-1))
                }
                Ok(None) => None,
                Err(err) => {
                    log::warn!("task waiter try_wait failed: task_id={task_id}, error={err}");
                    guard.take();
                    Some(-1)
                }
            }
        };

        if let Some(exit_code) = exit_code {
            finalize_task(&task_id, exit_code, |entry| {
                entry.state = if exit_code == 0 {
                    TaskState::Completed
                } else {
                    TaskState::Failed
                };
            });
            break;
        }

        thread::sleep(WAIT_POLL_INTERVAL);
    }

    for handle in reader_handles {
        let _ = handle.join();
    }
}

fn pump_stream<R: Read>(mut stream: R, output: &Mutex<TaskOutputBuffer>) {
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut output) = output.lock() {
                    output.append(&chunk[..n]);
                }
            }
        }
    }
}

/// 写终态并做回收检查。`decide` 由收割方决定状态 (completed/failed/canceled)。
fn finalize_task(task_id: &str, exit_code: i32, decide: impl FnOnce(&mut TaskEntry)) {
    let mut registry = registry().lock().expect("task registry lock should work");
    let Some(entry) = registry.get_mut(task_id) else {
        return;
    };
    // 只从 Running 迁出: 若 cancel 已先写终态, 这里不再覆盖
    if entry.state != TaskState::Running {
        return;
    }
    decide(entry);
    entry.exit_code = Some(exit_code);
    entry.finished_at_ms = Some(now_ms());
    // 终态帧入 pending (spec §6.3/6.4): canceled 复用 @task-failed + canceled:true
    let terminal_frame = match entry.state {
        TaskState::Completed => ControlFrame::TaskCompleted(TaskCompletedFrame {
            task_id: task_id.to_owned(),
            seq: entry.seq,
            exit_code,
        }),
        TaskState::Failed => ControlFrame::TaskFailed(TaskFailedFrame {
            task_id: task_id.to_owned(),
            seq: entry.seq,
            exit_code,
            canceled: false,
        }),
        TaskState::Canceled => ControlFrame::TaskFailed(TaskFailedFrame {
            task_id: task_id.to_owned(),
            seq: entry.seq,
            exit_code,
            canceled: true,
        }),
        TaskState::Running => unreachable!("finalize 只处理终态"),
    };
    entry.pending_frames.push_back(terminal_frame);
    if entry.pending_frames.len() > MAX_PENDING_FRAMES_PER_TASK {
        entry.pending_frames.pop_front();
    }
    reap_finished_tasks(&mut registry);
}

/// 终态条目超过 MAX_FINISHED_TASKS 时移除最老 (按 finished_at) 的条目。
fn reap_finished_tasks(registry: &mut HashMap<String, TaskEntry>) {
    let finished: Vec<(String, u128)> = registry
        .iter()
        .filter_map(|(task_id, entry)| {
            entry
                .finished_at_ms
                .map(|finished_at| (task_id.clone(), finished_at))
        })
        .collect();
    if finished.len() <= MAX_FINISHED_TASKS {
        return;
    }
    let mut finished = finished;
    finished.sort_by_key(|(_, finished_at)| *finished_at);
    let excess = finished.len() - MAX_FINISHED_TASKS;
    for (task_id, _) in finished.into_iter().take(excess) {
        registry.remove(&task_id);
    }
}

/// 查询任务状态快照。
pub fn task_snapshot(task_id: &str) -> io::Result<TaskSnapshot> {
    let registry = registry().lock().expect("task registry lock should work");
    let entry = registry
        .get(task_id)
        .ok_or_else(|| task_not_found(task_id))?;
    Ok(TaskSnapshot {
        task_id: task_id.to_owned(),
        seq: entry.seq,
        command: entry.command.clone(),
        state: entry.state,
        exit_code: entry.exit_code,
    })
}

/// 读取任务尾部输出。
pub fn task_output(task_id: &str, tail_lines: usize) -> io::Result<TaskOutputReport> {
    let registry = registry().lock().expect("task registry lock should work");
    let entry = registry
        .get(task_id)
        .ok_or_else(|| task_not_found(task_id))?;
    let (output, truncated, total_written) = {
        let buffer = entry.output.lock().expect("task output lock should work");
        (
            buffer.tail_lines(tail_lines),
            buffer.truncated,
            buffer.total_written,
        )
    };
    Ok(TaskOutputReport {
        task_id: task_id.to_owned(),
        seq: entry.seq,
        state: entry.state,
        output,
        truncated,
        total_written,
    })
}

/// 请求取消一个任务。
///
/// - running: 同步 kill + 收割, 状态写为 canceled; kill 失败时返回 Err
///   (registry 状态保持, client 可重试)
/// - 终态: 幂等返回当前状态 (spec 固定)
pub fn cancel_task(task_id: &str) -> io::Result<TaskSnapshot> {
    cancel_task_with(task_id, |child, child_id| {
        terminate_process_tree(child, child_id, TerminationSignal::Kill)
    })
}

fn cancel_task_with(
    task_id: &str,
    terminate: impl FnOnce(&mut Child, u32) -> io::Result<()>,
) -> io::Result<TaskSnapshot> {
    let child_handle = {
        let registry = registry().lock().expect("task registry lock should work");
        let Some(entry) = registry.get(task_id) else {
            return Err(task_not_found(task_id));
        };
        if entry.state.is_terminal() {
            return Ok(TaskSnapshot {
                task_id: task_id.to_owned(),
                seq: entry.seq,
                command: entry.command.clone(),
                state: entry.state,
                exit_code: entry.exit_code,
            });
        }
        Arc::clone(&entry.child)
    };

    // 保持 child 单收割方协议, 但不让 taskkill / wait 阻塞全局 registry。
    let cancel_outcome = {
        let mut child_guard = child_handle.lock().expect("task child lock should work");
        let outcome = match child_guard.as_mut() {
            Some(child) => {
                let child_id = child.id();
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let exit_code = status.code().unwrap_or(-1);
                        Some((exit_code, state_for_exit_code(exit_code)))
                    }
                    Ok(None) => match terminate(child, child_id) {
                        Ok(()) => {
                            let status = child.wait().map_err(|err| {
                                io::Error::other(format!("@task-cancel 等待进程退出失败: {err}"))
                            })?;
                            Some((status.code().unwrap_or(-1), TaskState::Canceled))
                        }
                        Err(kill_err) => match child.try_wait() {
                            Ok(Some(status)) => {
                                let exit_code = status.code().unwrap_or(-1);
                                Some((exit_code, state_for_exit_code(exit_code)))
                            }
                            Ok(None) => {
                                return Err(io::Error::other(format!(
                                    "@task-cancel 进程树终止失败: {kill_err}"
                                )));
                            }
                            Err(wait_err) => {
                                return Err(io::Error::other(format!(
                                    "@task-cancel 进程树终止失败: {kill_err}; 检查进程状态失败: {wait_err}"
                                )));
                            }
                        },
                    },
                    Err(err) => {
                        return Err(io::Error::other(format!(
                            "@task-cancel 检查进程状态失败: {err}"
                        )));
                    }
                }
            }
            None => None,
        };
        if outcome.is_some() {
            drop(child_guard.take());
        }
        outcome
    };

    if let Some((exit_code, state)) = cancel_outcome {
        finalize_task(task_id, exit_code, |entry| entry.state = state);
    }

    if let Some(snapshot) = wait_terminal_briefly(task_id) {
        return Ok(snapshot);
    }
    task_snapshot(task_id)
}

fn state_for_exit_code(exit_code: i32) -> TaskState {
    if exit_code == 0 {
        TaskState::Completed
    } else {
        TaskState::Failed
    }
}

/// 最多 2s 轮询等待任务到达终态, 用于 cancel 竞态窗口的响应收口。
fn wait_terminal_briefly(task_id: &str) -> Option<TaskSnapshot> {
    for _ in 0..40 {
        let snapshot = task_snapshot(task_id).ok()?;
        if snapshot.state.is_terminal() {
            return Some(snapshot);
        }
        thread::sleep(WAIT_POLL_INTERVAL);
    }
    None
}

/// session bridge 在 spawn 响应返回后绑定 lane (spec §6.4: 推送 lane 跟随来路)。
///
/// bind 前 pending 里已有 started 帧, bind 后首轮 drain 一并取走, 无丢失窗口。
pub fn bind_task_lane(task_id: &str, session_id: &str) {
    let mut registry = registry().lock().expect("task registry lock should work");
    if let Some(entry) = registry.get_mut(task_id) {
        entry.session_lane = Some(session_id.to_owned());
    }
}

/// 从 spawn 的 `@response` 行提取 task id 并绑定 lane。
///
/// 由 daemon_bridge 在 dispatch spawn 响应后调用; 非 spawn 响应 (缺 task 字段)
/// 是常态, 静默返回 false。响应形状与 control_core 的 task_snapshot_json 同源:
/// 带 request id 时在 value 信封内, 无 id 时裸对象。
pub fn bind_task_lane_from_spawn_response(response_line: &str, session_id: &str) -> bool {
    let Some(json_text) = response_line.trim().strip_prefix("@response ") else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return false;
    };
    let task_id = value["task"]
        .as_str()
        .or_else(|| value["value"]["task"].as_str());
    let Some(task_id) = task_id else {
        return false;
    };
    bind_task_lane(task_id, session_id);
    true
}

/// 取走绑定到指定 session 的全部待发进度帧 (按任务 seq 顺序)。
///
/// drain 即移除: 帧的消费是 at-most-once, 丢帧检测靠 seq (spec §6.2)。
pub fn drain_session_frames(session_id: &str) -> Vec<ControlFrame> {
    let mut registry = registry().lock().expect("task registry lock should work");
    let mut frames = Vec::new();
    for entry in registry.values_mut() {
        if entry.session_lane.as_deref() == Some(session_id) && !entry.pending_frames.is_empty() {
            frames.extend(entry.pending_frames.drain(..));
        }
    }
    frames
}

/// 指定 session 是否还有绑定且 running 的任务 (或有待 drain 帧)。
///
/// daemon bridge 用它决定轮询节奏: 有活跃绑定 -> 25ms 快轮询 (对齐 PTY active),
/// 否则维持 60s idle, 不给空闲 session 增加 CPU。
pub fn session_has_live_tasks(session_id: &str) -> bool {
    let registry = registry().lock().expect("task registry lock should work");
    registry.values().any(|entry| {
        entry.session_lane.as_deref() == Some(session_id)
            && (entry.state == TaskState::Running || !entry.pending_frames.is_empty())
    })
}

fn task_not_found(task_id: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("task not found: {task_id}"),
    )
}

/// `@task-output` 的响应数据。
#[derive(Debug)]
pub struct TaskOutputReport {
    pub task_id: String,
    pub seq: u64,
    pub state: TaskState,
    pub output: String,
    pub truncated: bool,
    pub total_written: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用 shell: Windows 走 PowerShell, unix 走 /bin/sh。
    ///
    /// 选 PowerShell 而非 cmd.exe, 与仓库既有测试 (`shell/tests.rs` 的
    /// `control_test_shell`) 同一口径: 产品侧 `build_shell_command` 对 pwsh
    /// 走 `-NoLogo -NoProfile -NonInteractive -Command`, 这条参数链本来就是
    /// Windows 的在役路径; 且 `Start-Sleep` 是进程内 cmdlet, `cancel_task`
    /// 直接 kill 掉 powershell.exe 就能收回, 不会像 `cmd /C` 套外部程序那样
    /// 留下收不掉的孤儿子进程。
    const TEST_SHELL: &str = if cfg!(windows) {
        "powershell.exe"
    } else {
        "/bin/sh"
    };

    /// 语义命令词汇表 —— 每条测试只说"我要长跑 / 要非零退出 / 要 200 行输出",
    /// 平台方言在这一处收敛 (单一真相源), 测试正文保持平台中立。
    ///
    /// 退出码 / stdout / stderr / kill 收割这些才是 @spawn 真正要守的契约,
    /// 它们与平台无关; 真正 unix 特有的是这些命令的**写法**, 故逐条翻译而不是
    /// 整条加 `#[cfg(unix)]` 剔除 (剔除 = Windows 上恰好最需要覆盖的面全裸奔)。
    mod cmd {
        /// 长时占用约 5s (验 @spawn 即答, 不阻塞 lane)。
        pub(super) const IDLE_5S: &str = if cfg!(windows) {
            "Start-Sleep 5"
        } else {
            "sleep 5"
        };
        /// 长时占用约 30s (验 cancel 收割 / started 帧之后再被 kill)。
        pub(super) const IDLE_30S: &str = if cfg!(windows) {
            "Start-Sleep 30"
        } else {
            "sleep 30"
        };
        /// 成功退出、不产生输出。
        pub(super) const NOOP: &str = if cfg!(windows) { "exit 0" } else { "true" };
        /// 以退出码 3 结束。
        pub(super) const EXIT_3: &str = "exit 3";
        /// 打印一行到 stdout。
        pub(super) const ECHO_HELLO: &str = "echo hello";
        /// 打印一行到 stderr。
        ///
        /// Windows 侧走 `[Console]::Error.WriteLine` 直写进程 stderr 句柄,
        /// 而不是 `Write-Error` 的 Error 流 —— 后者的宿主序列化形态在
        /// Windows PowerShell 5.1 与 pwsh 7 之间不一致, 断言会变成在测
        /// PowerShell 版本而不是在测 @spawn 的 stdout/stderr 合流。
        pub(super) const ECHO_ERR: &str = if cfg!(windows) {
            "[Console]::Error.WriteLine('err')"
        } else {
            "echo err 1>&2"
        };
        /// stdout 打 out, stderr 打 err, stdout 再打 done (验合流顺序)。
        pub(super) const OUT_ERR_DONE: &str = if cfg!(windows) {
            "Write-Output out; [Console]::Error.WriteLine('err'); Write-Output done"
        } else {
            "echo out; echo err 1>&2; echo done"
        };
        /// 1..200 逐行输出 (验尾部行数截断)。
        pub(super) const LINES_1_200: &str = if cfg!(windows) { "1..200" } else { "seq 1 200" };
        /// 打印当前工作目录 (验 cwd 生效)。
        pub(super) const PWD: &str = if cfg!(windows) {
            "Write-Output $PWD"
        } else {
            "pwd"
        };
        /// 先打一行标记再长睡 1s (验 running 期间可查状态)。
        pub(super) const ECHO_THEN_IDLE_1S: &str = if cfg!(windows) {
            "Write-Output task-e2e; Start-Sleep 1"
        } else {
            "echo task-e2e; sleep 1"
        };
    }

    fn spawn_quick(command: &str) -> String {
        spawn_task(TEST_SHELL, command, None).expect("quick spawn should succeed")
    }

    /// 等待任务到达终态 (测试辅助, 上限 10s)。
    fn wait_terminal(task_id: &str) -> TaskSnapshot {
        for _ in 0..200 {
            if let Ok(snapshot) = task_snapshot(task_id) {
                if snapshot.state.is_terminal() {
                    return snapshot;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("task 未在 10s 内到达终态: {task_id}");
    }

    #[test]
    fn spawn_should_return_immediately_for_long_running_command() {
        let started = std::time::Instant::now();
        let task_id = spawn_quick(cmd::IDLE_5S);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "@spawn 必须立即返回, 实际耗时 {elapsed:?}"
        );
        assert_eq!(task_snapshot(&task_id).unwrap().state, TaskState::Running);
        cancel_task(&task_id).unwrap();
    }

    #[test]
    fn completed_task_should_carry_exit_code_zero() {
        let task_id = spawn_quick(cmd::ECHO_HELLO);
        let snapshot = wait_terminal(&task_id);
        assert_eq!(snapshot.state, TaskState::Completed);
        assert_eq!(snapshot.exit_code, Some(0));
    }

    #[test]
    fn failed_task_should_carry_nonzero_exit_code() {
        let task_id = spawn_quick(cmd::EXIT_3);
        let snapshot = wait_terminal(&task_id);
        assert_eq!(snapshot.state, TaskState::Failed);
        assert_eq!(snapshot.exit_code, Some(3));
    }

    #[test]
    fn output_should_capture_stdout_and_stderr_merged() {
        let task_id = spawn_quick(cmd::OUT_ERR_DONE);
        wait_terminal(&task_id);
        let report = task_output(&task_id, DEFAULT_OUTPUT_TAIL_LINES).unwrap();
        assert!(report.output.contains("out"));
        assert!(report.output.contains("err"));
        assert!(report.output.contains("done"));
        assert!(!report.truncated);
    }

    #[test]
    fn output_tail_lines_should_limit_result() {
        let task_id = spawn_quick(cmd::LINES_1_200);
        wait_terminal(&task_id);
        let report = task_output(&task_id, 5).unwrap();
        // 逐行 trim 后再比: Windows 行尾是 CRLF, 拿 `1\n2\n3\n4\n5\n6\n`
        // 这种 LF 字面量去 contains 会恒真 —— 断言看着在, 实际什么都没守。
        let lines: Vec<&str> = report.output.trim().lines().map(str::trim).collect();
        assert!(lines.len() <= 5, "尾部行数应不超过 5, 实际 {}", lines.len());
        assert_eq!(lines.last(), Some(&"200"), "尾部应停在末行 200");
        assert!(!lines.contains(&"1"), "尾部 5 行不该含首行 1: {lines:?}");
    }

    #[test]
    fn cancel_running_task_should_reap_canceled_state() {
        let task_id = spawn_quick(cmd::IDLE_30S);
        let snapshot = cancel_task(&task_id).unwrap();
        assert_eq!(snapshot.state, TaskState::Canceled);
        // 进程已死: 不再占用 running 名额
        let registry = registry().lock().unwrap();
        assert!(registry[&task_id].state.is_terminal());
    }

    #[test]
    fn cancel_failure_should_leave_task_running_and_retryable() {
        let task_id = spawn_quick(cmd::IDLE_30S);
        let error = cancel_task_with(&task_id, |_child, _child_id| {
            Err(io::Error::other("injected tree cleanup failure"))
        })
        .unwrap_err();
        let state = task_snapshot(&task_id).unwrap().state;
        let child_retained = {
            let registry = registry().lock().unwrap();
            let retained = registry[&task_id].child.lock().unwrap().is_some();
            retained
        };
        let retry = cancel_task(&task_id).unwrap();

        assert!(error.to_string().contains("injected tree cleanup failure"));
        assert_eq!(state, TaskState::Running);
        assert!(child_retained, "失败后应保留 child 句柄供重试");
        assert_eq!(retry.state, TaskState::Canceled);
    }

    #[test]
    fn cancel_wait_should_not_hold_registry_lock() {
        let task_id = spawn_quick(cmd::IDLE_30S);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let cancel_task_id = task_id.clone();
        let cancel_thread = thread::spawn(move || {
            cancel_task_with(&cancel_task_id, move |child, child_id| {
                entered_tx
                    .send(())
                    .map_err(|err| io::Error::other(format!("通知进入取消闭包失败: {err}")))?;
                release_rx
                    .recv()
                    .map_err(|err| io::Error::other(format!("等待取消测试放行失败: {err}")))?;
                terminate_process_tree(child, child_id, TerminationSignal::Kill)
            })
        });
        let cancel_entered = entered_rx.recv_timeout(Duration::from_secs(3)).is_ok();

        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::channel();
        let snapshot_task_id = task_id.clone();
        let snapshot_thread = thread::spawn(move || {
            let _ = snapshot_tx.send(task_snapshot(&snapshot_task_id));
        });
        let snapshot_before_release = snapshot_rx.recv_timeout(Duration::from_secs(2));
        let snapshot_returned_while_cancel_waited = matches!(&snapshot_before_release, Ok(Ok(_)));

        let _ = release_tx.send(());
        let cancel_result = cancel_thread.join().ok().and_then(|result| result.ok());
        let snapshot_result = match snapshot_before_release {
            Ok(result) => Some(result),
            Err(_) => snapshot_rx.recv_timeout(Duration::from_secs(3)).ok(),
        };
        let snapshot_thread_joined = snapshot_thread.join().is_ok();
        let cleanup_state = cancel_task(&task_id).map(|snapshot| snapshot.state).ok();

        assert!(cancel_entered, "取消闭包应进入阻塞区");
        assert!(
            snapshot_returned_while_cancel_waited,
            "取消等待期间 task_snapshot 不应被 registry 锁阻塞"
        );
        assert!(snapshot_result.is_some_and(|result| result.is_ok()));
        assert!(snapshot_thread_joined, "snapshot 线程应能收敛");
        assert_eq!(
            cancel_result.map(|snapshot| snapshot.state),
            Some(TaskState::Canceled)
        );
        assert_eq!(cleanup_state, Some(TaskState::Canceled));
    }

    #[cfg(windows)]
    #[test]
    fn spawned_task_should_not_allocate_a_console() {
        let script = concat!(
            "Add-Type -Namespace Rdog -Name ConsoleProbe -MemberDefinition ",
            "'[DllImport(\"kernel32.dll\")] public static extern ",
            "System.IntPtr GetConsoleWindow();'; ",
            "[Rdog.ConsoleProbe]::GetConsoleWindow().ToInt64()"
        );
        let task_id = spawn_quick(script);
        let snapshot = wait_terminal(&task_id);
        let output = task_output(&task_id, DEFAULT_OUTPUT_TAIL_LINES).unwrap();

        assert_eq!(snapshot.state, TaskState::Completed);
        assert_eq!(output.output.trim(), "0");
    }

    #[cfg(windows)]
    struct WindowsTaskCleanup {
        task_id: String,
        ready_files: Vec<std::path::PathBuf>,
        unrelated: Option<Child>,
    }

    #[cfg(windows)]
    impl Drop for WindowsTaskCleanup {
        fn drop(&mut self) {
            let _ = cancel_task(&self.task_id);
            if let Some(mut unrelated) = self.unrelated.take() {
                let _ = unrelated.kill();
                let _ = unrelated.wait();
            }
            for path in &self.ready_files {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    #[cfg(windows)]
    fn quote_powershell(text: &str) -> String {
        text.replace('\'', "''")
    }

    #[cfg(windows)]
    fn listener_script(ready_file: &std::path::Path) -> String {
        let ready_file = quote_powershell(&ready_file.to_string_lossy());
        format!(
            "$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0); $listener.Start(); [System.IO.File]::WriteAllText('{ready_file}', [string]$listener.LocalEndpoint.Port); $deadline = [DateTime]::UtcNow.AddSeconds(30); while ([DateTime]::UtcNow -lt $deadline) {{ Start-Sleep -Milliseconds 50 }}; $listener.Stop()"
        )
    }

    #[cfg(windows)]
    fn descendant_listener_script(ready_file: &std::path::Path) -> String {
        let child_script = quote_powershell(&listener_script(ready_file));
        format!(
            "$childScript = '{child_script}'; $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childScript)); $child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded) -WindowStyle Hidden -PassThru; $deadline = [DateTime]::UtcNow.AddSeconds(30); while ([DateTime]::UtcNow -lt $deadline) {{ Start-Sleep -Milliseconds 50 }}"
        )
    }

    #[cfg(windows)]
    fn wait_listener_port(path: &std::path::Path) -> Option<u16> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let port = std::fs::read_to_string(path)
                .ok()
                .and_then(|value| value.trim().parse::<u16>().ok());
            if let Some(port) = port.filter(|port| listener_is_open(*port)) {
                return Some(port);
            }
            thread::sleep(Duration::from_millis(25));
        }
        None
    }

    #[cfg(windows)]
    fn listener_is_open(port: u16) -> bool {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(150)).is_ok()
    }

    #[cfg(windows)]
    fn wait_listener_closed(port: u16) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if !listener_is_open(port) {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[cfg(windows)]
    #[test]
    fn cancel_should_stop_windows_descendant_listener_and_preserve_unrelated_listener() {
        let nonce = uuid::Uuid::new_v4();
        let target_ready = std::env::temp_dir().join(format!("rdog-target-{nonce}.port"));
        let unrelated_ready = std::env::temp_dir().join(format!("rdog-unrelated-{nonce}.port"));
        let task_id = spawn_quick(&descendant_listener_script(&target_ready));
        let mut cleanup = WindowsTaskCleanup {
            task_id: task_id.clone(),
            ready_files: vec![target_ready.clone(), unrelated_ready.clone()],
            unrelated: None,
        };
        cleanup.unrelated = Some(
            build_shell_command(TEST_SHELL, &listener_script(&unrelated_ready))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("应启动独立 listener"),
        );
        let target_port = wait_listener_port(&target_ready);
        let unrelated_port = wait_listener_port(&unrelated_ready);
        let both_started = target_port.is_some() && unrelated_port.is_some();
        let cancel = cancel_task(&task_id);
        let target_closed = target_port.is_some_and(wait_listener_closed);
        let unrelated_alive = unrelated_port.is_some_and(listener_is_open);
        let canceled = cancel.is_ok_and(|snapshot| snapshot.state == TaskState::Canceled);

        drop(cleanup);

        assert!(both_started, "两个 listener 都应在独立端口就绪");
        assert!(canceled, "任务树清理成功后才应回 canceled");
        assert!(target_closed, "取消后任务子树的 listener 端口仍可连接");
        assert!(unrelated_alive, "取消任务树误杀了无关 listener");
    }

    #[cfg(unix)]
    #[test]
    fn cancel_should_stop_descendants_and_preserve_unrelated_process() {
        let path = std::env::temp_dir().join(format!("rdog-task-child-{}", uuid::Uuid::new_v4()));
        let task = spawn_quick(&format!("sleep 30 & echo $! > '{}'; wait", path.display()));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let pid = loop {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(pid) = text.trim().parse::<i32>() {
                    break pid;
                }
            }
            assert!(std::time::Instant::now() < deadline, "子进程未写 PID");
            thread::sleep(Duration::from_millis(20));
        };
        let mut unrelated = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        cancel_task(&task).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        // 只探测本测试创建并取回 PID 的子进程; 失败时也收回现场.
        while unsafe { libc::kill(pid, 0) } == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let descendant_alive = unsafe { libc::kill(pid, 0) } == 0;
        let unrelated_alive = unrelated.try_wait().unwrap().is_none();
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = unrelated.kill();
        let _ = unrelated.wait();
        let _ = std::fs::remove_file(path);
        assert!(!descendant_alive, "取消后子进程 {pid} 仍存活");
        assert!(unrelated_alive, "取消误杀了其他任务的进程");
    }

    #[test]
    fn cancel_terminal_task_should_be_idempotent() {
        let task_id = spawn_quick(cmd::NOOP);
        wait_terminal(&task_id);
        let snapshot = cancel_task(&task_id).unwrap();
        assert_eq!(snapshot.state, TaskState::Completed);
    }

    #[test]
    fn unknown_task_should_return_not_found() {
        let err = task_snapshot("t-deadbeef").unwrap_err();
        assert!(err.to_string().contains("task not found"));
        let err = cancel_task("t-deadbeef").unwrap_err();
        assert!(err.to_string().contains("task not found"));
    }

    #[test]
    fn spawn_empty_command_should_be_rejected() {
        let err = spawn_task(TEST_SHELL, "   ", None).unwrap_err();
        assert!(err.to_string().contains("不能为空"));
    }

    #[test]
    fn spawn_with_cwd_should_execute_in_that_directory() {
        let temp = std::env::temp_dir().join("rdog-task-cwd-test");
        std::fs::create_dir_all(&temp).unwrap();
        let task_id = spawn_task(TEST_SHELL, cmd::PWD, Some(temp.to_str().unwrap())).unwrap();
        wait_terminal(&task_id);
        let report = task_output(&task_id, DEFAULT_OUTPUT_TAIL_LINES).unwrap();
        assert!(
            report.output.contains("rdog-task-cwd-test"),
            "pwd 应输出 cwd, 实际: {}",
            report.output
        );
    }

    // ===== 端到端协议链路 (parse_and_execute_control_line 全链) =====
    // 验收矩阵核心: @spawn 即答后, 同一执行 lane 上 @ping 等命令不再排队。

    fn execute_line(line: &str) -> String {
        let executor = crate::control_actions::SystemControlActionExecutor::default();
        let outcome = crate::control_core::parse_and_execute_control_line(
            line,
            TEST_SHELL,
            &executor,
            &executor.cancel_registry(),
        );
        match outcome.outbound_frames.first() {
            Some(crate::control_frames::ControlFrame::ResponseLine(line)) => line.clone(),
            other => panic!("预期 ResponseLine, 实际 {other:?}"),
        }
    }

    fn extract_task_id(response: &str) -> String {
        let json = response.trim_start_matches("@response ");
        let value: serde_json::Value = serde_json::from_str(json)
            .unwrap_or_else(|err| panic!("spawn 响应应是 JSON: {response}, err={err}"));
        // 带 request id 时 render_structured_success_response 会包进
        // {"id":..,"value":{...}} 信封; 无 id 时原样输出。两种形状都兼容。
        let task = value["task"]
            .as_str()
            .or_else(|| value["value"]["task"].as_str())
            .unwrap_or_else(|| panic!("spawn 响应缺 task 字段: {response}"));
        task.to_owned()
    }

    /// 断言 @ping 回的是结构化诊断体, 而不是裸 JSON 串 `"pong"`。
    ///
    /// 判据刻意落在 `value.is_object()` 上: 裸串形态下 `value` 是 JSON string
    /// (带 request id 时在 `{"id":..,"value":...}` 信封里, 无 id 时就是
    /// 裸的 `"pong"`), 两种形态都会被 `is_object()` 当场拒掉, 而
    /// `contains("pong")` 对二者一视同仁 —— 这正是旧断言的盲区。
    fn assert_ping_is_structured(response: &str) {
        let json = response.trim_start_matches("@response ");
        let value: serde_json::Value = serde_json::from_str(json)
            .unwrap_or_else(|err| panic!("@ping 响应应是 JSON: {response}, err={err}"));
        // 带 request id 时会包进 {"id":..,"value":{...}} 信封; 无 id 时原样输出。
        let ping = if value.get("value").is_some() {
            &value["value"]
        } else {
            &value
        };
        assert!(ping.is_object(), "@ping 的响应体应是对象, 实际: {ping}");
        assert_eq!(ping["pong"], serde_json::json!(true), "{ping}");
        for field in ["ok", "uptime_ms", "bind_state", "shell"] {
            assert!(
                !ping[field].is_null(),
                "@ping 结构化响应缺 {field} 字段: {ping}"
            );
        }
    }

    #[test]
    fn e2e_spawn_should_answer_immediately_and_not_block_lane() {
        let started = std::time::Instant::now();
        let response = execute_line(&format!("@spawn#101:{}", cmd::IDLE_30S));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "@spawn 必须即答"
        );
        assert!(
            response.contains("\"id\":101"),
            "响应应带 request id: {response}"
        );
        assert!(response.contains("\"state\":\"running\""), "{response}");

        // 同 lane 后续命令立即执行 — 这就是 Phase 1 要解决的阻塞体验
        let ping_started = std::time::Instant::now();
        let ping = execute_line("@ping");
        assert!(ping_started.elapsed() < Duration::from_secs(1));
        // 排障用的 @ping 必须回**结构化**体: 裸 "pong" 与 {"pong":true,...}
        // 都能满足 contains("pong"), 旧断言因此盖不住 "结构化响应被短路成裸串"
        // 这个洞 —— 而裸串恰恰没有 bind_state, 也就分不出"正在监听"与
        // "bind 失败在退避重试"。这里按解析后的字段断言, 两侧形态都断得掉。
        assert_ping_is_structured(&ping);

        let task_id = extract_task_id(&response);
        let cancel = execute_line(&format!("@task-cancel:{task_id}"));
        assert!(cancel.contains("\"state\":\"canceled\""), "{cancel}");
    }

    #[test]
    fn e2e_task_lifecycle_through_protocol() {
        let response = execute_line(&format!(
            "@spawn {{command:\"{}\"}}",
            cmd::ECHO_THEN_IDLE_1S
        ));
        let task_id = extract_task_id(&response);

        // running 期间可查状态
        let status = execute_line(&format!("@task-status:{task_id}"));
        assert!(status.contains("\"state\":\"running\""), "{status}");

        // 等终态后 status 带 exit_code, output 有内容
        wait_terminal(&task_id);
        let status = execute_line(&format!("@task-status:{task_id}"));
        assert!(status.contains("\"state\":\"completed\""), "{status}");
        assert!(status.contains("\"exit_code\":0"), "{status}");

        let output = execute_line(&format!("@task-output:{task_id}"));
        assert!(output.contains("task-e2e"), "{output}");

        // 终态 cancel 幂等返回现态
        let cancel = execute_line(&format!("@task-cancel:{task_id}"));
        assert!(cancel.contains("\"state\":\"completed\""), "{cancel}");
    }

    #[test]
    fn e2e_unknown_task_should_return_error_response() {
        let response = execute_line("@task-status:t-0000ffff");
        assert!(response.contains("\"code\""), "{response}");
        assert!(response.contains("task not found"), "{response}");
    }

    #[test]
    fn consecutive_spawns_should_have_strictly_increasing_seq() {
        let first = spawn_quick(cmd::NOOP);
        let second = spawn_quick(cmd::NOOP);
        let third = spawn_quick(cmd::NOOP);
        let first_seq = task_snapshot(&first).unwrap().seq;
        let second_seq = task_snapshot(&second).unwrap().seq;
        let third_seq = task_snapshot(&third).unwrap().seq;
        assert!(first_seq < second_seq, "{first_seq} 应小于 {second_seq}");
        assert!(second_seq < third_seq, "{second_seq} 应小于 {third_seq}");
        // 输出报告也携带同一 seq (真相源一致)
        let report_seq = task_output(&first, 10).unwrap().seq;
        assert_eq!(report_seq, first_seq);
    }

    #[test]
    fn session_lane_frames_should_drain_only_for_bound_session() {
        let task_id = spawn_quick(cmd::NOOP);
        // bind 前其他 session drain 不到
        assert!(drain_session_frames("sess-other").is_empty());
        // 未绑定 lane 的主 session 也 drain 不到 (帧在 pending 滞留)
        assert!(drain_session_frames("sess-main").is_empty());

        bind_task_lane(&task_id, "sess-main");
        // bind 后 started 帧可被 drain
        let frames = drain_session_frames("sess-main");
        assert_eq!(frames.len(), 1, "应 drain 出 started 帧");
        assert!(matches!(frames[0], ControlFrame::TaskStarted(_)));

        // 终态帧: 等任务完成后 drain
        wait_terminal(&task_id);
        let frames = drain_session_frames("sess-main");
        assert_eq!(frames.len(), 1, "应 drain 出终态帧");
        assert!(matches!(frames[0], ControlFrame::TaskCompleted(_)));

        // drain 即移除: 第二次为空
        assert!(drain_session_frames("sess-main").is_empty());
    }

    #[test]
    fn canceled_task_terminal_frame_should_reuse_task_failed_with_flag() {
        let task_id = spawn_quick(cmd::IDLE_30S);
        bind_task_lane(&task_id, "sess-main");
        let _ = drain_session_frames("sess-main"); // 取走 started
        cancel_task(&task_id).unwrap();
        let frames = drain_session_frames("sess-main");
        match frames.first() {
            Some(ControlFrame::TaskFailed(frame)) => {
                assert!(
                    frame.canceled,
                    "取消终态应复用 @task-failed + canceled:true"
                );
            }
            other => panic!("预期 TaskFailed 帧, 实际 {other:?}"),
        }
    }

    #[test]
    fn legacy_unbound_task_frames_should_not_leak_beyond_cap() {
        let task_id = spawn_quick(cmd::ECHO_ERR);
        let registry = registry().lock().unwrap();
        let entry = &registry[&task_id];
        assert!(entry.pending_frames.len() <= MAX_PENDING_FRAMES_PER_TASK);
        assert!(entry.session_lane.is_none());
    }

    #[test]
    fn bind_task_lane_from_spawn_response_should_parse_both_envelope_shapes() {
        let task_id = spawn_quick(cmd::NOOP);
        // 有 request id 的 value 信封形状
        let with_id = format!(
            "@response {{\"id\":101,\"value\":{{\"task\":\"{task_id}\",\"state\":\"running\"}}}}"
        );
        assert!(bind_task_lane_from_spawn_response(&with_id, "sess-a"));
        // registry guard 必须在下一个 spawn 前释放:
        // 同线程 Mutex 不可重入, guard 跨 spawn 存活会自死锁
        {
            let registry = registry().lock().unwrap();
            assert_eq!(registry[&task_id].session_lane.as_deref(), Some("sess-a"));
        }

        // 无信封裸对象形状
        let task2 = spawn_quick(cmd::NOOP);
        let bare = format!("@response {{\"task\":\"{task2}\",\"state\":\"running\"}}");
        assert!(bind_task_lane_from_spawn_response(&bare, "sess-b"));
        // 非 spawn 响应静默 false
        assert!(!bind_task_lane_from_spawn_response("@response 0", "sess-b"));
    }

    #[test]
    fn output_buffer_truncation_should_keep_tail() {
        let mut buffer = TaskOutputBuffer::default();
        let big = vec![b'x'; MAX_OUTPUT_BYTES + 4096];
        buffer.append(&big);
        assert!(buffer.truncated);
        assert_eq!(buffer.data.len(), MAX_OUTPUT_BYTES);
        assert_eq!(buffer.total_written, big.len() as u64);
    }
}
