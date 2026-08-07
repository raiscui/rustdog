# local-default legacy退役关键洞察

## [2026-07-18 13:26:23] [Session ID: omx-1784304547353-h5409r] 主题:新版lease不能约束仍可执行的旧二进制

### 发现来源

- commit `893398c`旧版与当前新版的隔离启动、SIGKILL和反向降级矩阵.

### 核心问题

- 新版可以保证自己不unlink stable lease,但旧版仍会把已释放PID视为stale并删除同一路径.
- 因此跨版本原子互锁无法只靠新版代码补齐.

### 为什么重要

- 如果部署只升级一个正在运行的进程,却保留旧二进制或旧自启动项,后续回滚/重启可能重新破坏stable inode契约.
- client继续接受纯v1/FIFO时,这种退化还会表现为"仍然能用",掩盖owner模型已经倒退.

### 当前结论

- active legacy PID检查必须保留为fail-closed升级门.
- 正常client必须managed-only,让降级状态明确失败而不是静默兼容.
- 部署验收必须审计PATH、cargo安装、副本和LaunchAgent/LaunchDaemon等启动入口.

### 后续讨论入口

- Windows仍未迁移到同等lifecycle-bound ownership,见`LATER_PLANS__local_default_legacy_retirement.md`.
- `src/zenoh_runtime.rs`已超过模块行数建议,拆分计划仍在`LATER_PLANS__local_default_atomic_lease.md`.
