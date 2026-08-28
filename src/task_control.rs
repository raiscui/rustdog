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

use crate::control_actions::build_shell_command;
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
    // 阶段 1: registry 锁内做 kill + 收割 child; 锁内绝不调 finalize_task
    // (它内部要再拿 registry 锁, std Mutex 不可重入, 持锁调用必死锁)。
    // 锁顺序约定 (全库唯一处): registry -> child。waiter 只单拿 child/registry, 无环。
    let cancel_outcome = {
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
        let mut child_guard = entry.child.lock().expect("task child lock should work");
        if let Some(child) = child_guard.as_mut() {
            let kill_result = child.kill();
            let wait_status = child.wait();
            child_guard.take();
            match (kill_result, wait_status) {
                (Ok(()), Ok(status)) => Some(status.code().unwrap_or(-1)),
                (Err(err), _) => {
                    return Err(io::Error::other(format!("@task-cancel kill 失败: {err}")));
                }
                (Ok(()), Err(err)) => {
                    return Err(io::Error::other(format!(
                        "@task-cancel kill 后 wait 失败: {err}"
                    )));
                }
            }
        } else {
            None
        }
    };

    // 阶段 2: registry 锁外写终态
    if let Some(exit_code) = cancel_outcome {
        finalize_task(task_id, exit_code, |entry| {
            entry.state = TaskState::Canceled;
        });
    }

    // 阶段 3: 罕见竞态 — waiter 刚 take 走 child 还没写终态时, 短等它落笔,
    // 保证 cancel 响应绝大多数情况下就是 canceled 终态
    if let Some(snapshot) = wait_terminal_briefly(task_id) {
        return Ok(snapshot);
    }
    task_snapshot(task_id)
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

    fn spawn_quick(command: &str) -> String {
        spawn_task("/bin/sh", command, None).expect("quick spawn should succeed")
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
        let task_id = spawn_quick("sleep 5");
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
        let task_id = spawn_quick("echo hello");
        let snapshot = wait_terminal(&task_id);
        assert_eq!(snapshot.state, TaskState::Completed);
        assert_eq!(snapshot.exit_code, Some(0));
    }

    #[test]
    fn failed_task_should_carry_nonzero_exit_code() {
        let task_id = spawn_quick("exit 3");
        let snapshot = wait_terminal(&task_id);
        assert_eq!(snapshot.state, TaskState::Failed);
        assert_eq!(snapshot.exit_code, Some(3));
    }

    #[test]
    fn output_should_capture_stdout_and_stderr_merged() {
        let task_id = spawn_quick("echo out; echo err 1>&2; echo done");
        wait_terminal(&task_id);
        let report = task_output(&task_id, DEFAULT_OUTPUT_TAIL_LINES).unwrap();
        assert!(report.output.contains("out"));
        assert!(report.output.contains("err"));
        assert!(report.output.contains("done"));
        assert!(!report.truncated);
    }

    #[test]
    fn output_tail_lines_should_limit_result() {
        let task_id = spawn_quick("seq 1 200");
        wait_terminal(&task_id);
        let report = task_output(&task_id, 5).unwrap();
        let line_count = report.output.trim().lines().count();
        assert!(line_count <= 5, "尾部行数应不超过 5, 实际 {line_count}");
        assert!(report.output.contains("200"));
        assert!(!report.output.contains("1\n2\n3\n4\n5\n6\n"));
    }

    #[test]
    fn cancel_running_task_should_reap_canceled_state() {
        let task_id = spawn_quick("sleep 30");
        let snapshot = cancel_task(&task_id).unwrap();
        assert_eq!(snapshot.state, TaskState::Canceled);
        // 进程已死: 不再占用 running 名额
        let registry = registry().lock().unwrap();
        assert!(registry[&task_id].state.is_terminal());
    }

    #[test]
    fn cancel_terminal_task_should_be_idempotent() {
        let task_id = spawn_quick("true");
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
        let err = spawn_task("/bin/sh", "   ", None).unwrap_err();
        assert!(err.to_string().contains("不能为空"));
    }

    #[test]
    fn spawn_with_cwd_should_execute_in_that_directory() {
        let temp = std::env::temp_dir().join("rdog-task-cwd-test");
        std::fs::create_dir_all(&temp).unwrap();
        let task_id = spawn_task("/bin/sh", "pwd", Some(temp.to_str().unwrap())).unwrap();
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
            "/bin/sh",
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

    #[test]
    fn e2e_spawn_should_answer_immediately_and_not_block_lane() {
        let started = std::time::Instant::now();
        let response = execute_line("@spawn#101:sleep 30");
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
        assert!(ping.contains("pong"), "{ping}");

        let task_id = extract_task_id(&response);
        let cancel = execute_line(&format!("@task-cancel:{task_id}"));
        assert!(cancel.contains("\"state\":\"canceled\""), "{cancel}");
    }

    #[test]
    fn e2e_task_lifecycle_through_protocol() {
        let response = execute_line("@spawn {command:\"echo task-e2e; sleep 1\"}");
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
        let first = spawn_quick("true");
        let second = spawn_quick("true");
        let third = spawn_quick("true");
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
        let task_id = spawn_quick("true");
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
        let task_id = spawn_quick("sleep 30");
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
        let task_id = spawn_quick("echo x");
        let registry = registry().lock().unwrap();
        let entry = &registry[&task_id];
        assert!(entry.pending_frames.len() <= MAX_PENDING_FRAMES_PER_TASK);
        assert!(entry.session_lane.is_none());
    }

    #[test]
    fn bind_task_lane_from_spawn_response_should_parse_both_envelope_shapes() {
        let task_id = spawn_quick("true");
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
        let task2 = spawn_quick("true");
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
