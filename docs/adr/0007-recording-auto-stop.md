# 0007. Recording auto-stop via `--duration`

> Status: **Accepted** (in grilling 2026-07-29)

## Context

`rdog record start` 当前只支持手动 `@record-stop` 触发 Bundle 提交 + 单帧 base64 `@savefile` 投递。人类录制场景经常有"录 30 秒后自动结束"的需求 (e.g. 录一段交互 demo 等)。

本次要做的最小扩展:在 `rdog record start` 加一个 `--duration` 可选参数,daemon 端到时间后自动 `@record-stop`,Bundle 走完正常 stop 路径。

## Decisions

### 1. Duration syntax

humantime 三件套 (`--duration 30s` / `5m` / `1h`),支持小数 (`1.5m` = 90s),内部统一到 `duration_ms` 整数毫秒。

### 2. Timer owner

daemon 端后台任务 (跨线程 + atomic Bundle 收口)。`RecordingHandler::start` 内部起线程 timer,`@record-start` 把 `duration_ms` 传到 `RecordRequest::Start`,timer 线程到点直接调 `lifecycle.begin_finalize` + Bundle 写 + delivery。

### 3. Auto-stop 的最终响应接收方

Bundle 始终落盘 + 响应只在 owner 当前连接。owner 已 disconnect 时, 走 completed retry (跟 ticket #19 路径一致)。

### 3a. Disconnect 后的回执

不 push, 只在 `lifecycle.last_session` metadata + Bundle 路径 (跟 ticket #19 completed retry 复用)。owner 重连后调 `@record-stop <recording_id>` 重新拿 Bundle。

### 4. Owner 提前 cancel 的语义

timer 自动取消, 走 cancel 路径, 不走 stop。`RecordingHandler` 在 `start` 时建一个 `Arc<AtomicBool> cancelled_flag`,cancel/stop 路径都 set true,timer 线程每次 wake-up check flag 然后 return。

### 5. 跟手动 stop 冲突

timer 自动取消, 跟 Q4 同一份代码, 手动 stop 优先。`@record-stop` 也 set `cancelled_flag = true`,timer 线程退出。

### 6. 上限保护 + 错误码

**硬上限 1h** (`3_600_000 ms`), 超出返 `DURATION_TOO_LARGE` 错误码 `4120`, 风格跟 #19 `DELIVERY_RATE_LIMITED` (4200) 一致。

**边界**:
- `--duration 0s` 合法, 立即触发 stop (实质等同手动 stop)
- `--duration < 100ms` 视为非法, 返 `DURATION_TOO_SMALL` 错误码 `4121`
- 负数 / 单位缺失 / 数字溢出 / 未知单位: CLI 层 clap + 验证, panic 拒绝
- 1h 上限只针对 `duration_ms` 字段, 跟 #19 的 256 MiB Bundle size 上限互相独立

## Consequences

- `RecordingHandler` 跨线程持有 session 句柄, 需要 `Arc<Mutex<LifecycleManager>>` 或把 `RecordingHandler` 整个 `Arc<Mutex<>>` 包起来。
- `cancelled_flag: Arc<AtomicBool>` 是 timer 唯一信号, timer 线程 join 必须在 `RecordingHandler` 销毁前完成, 否则 timer 悬空引用 session。
- `RecordRequest::Start` 加 `duration_ms: Option<u64>` 字段, `RecordingHandler::start` 在 `Some` 时起 timer。
- `LifecycleManager` 不动, 复用现有 `begin_finalize` + `complete_current` 路径。
- Bundle commit 失败 + timer 取消同时发生时, 走 `lifecycle.fail` 而不是 leak, 跟 #19 一致。

## Open implementation tickets (下一步 /to-spec → /to-tickets)

1. `RecordRequest::Start` 加 `duration_ms: Option<u64>` 字段, `RecordSubcommand::Start` 加 `--duration <humantime>` flag,parser 统一到 `duration_ms`。
2. `RecordingHandler` 加 timer 字段: `Option<(JoinHandle, Arc<AtomicBool>)>`,在 start 时创建, cancel / stop / 销毁时 cancel + join。
3. Timer 线程实现: `thread::sleep` (首次实现不用 tokio,跟现有 `LifecycleManager` 同步模型一致),到点 check flag, 调 handler 内部 stop 路径。
4. CLI 层: `humantime` parser (自己写 ~30 行, 不引入新 dep),`rdog record start --duration 30s self` smoke。
5. 集成测试: 4 条 (success / cancel-preempt / stop-preempt / duration-too-large / too-small)。
