# local-default 原子lease关键洞察

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 主题: 文件锁保护的是inode而不是路径

### 发现来源

- local-default/unixpipe guard演进的跨进程`try_lock`、SIGKILL和unlink最小实验.

### 核心问题

- active owner持锁期间unlink路径后,旧owner继续锁旧inode,新owner却能创建新inode并成功拿锁.
- 任何Drop、client cleanup或stale recovery只要删除stable lock path,都会让互斥失效.

### 为什么重要

- 这不是普通临时文件清理问题,而是所有权协议的安全不变量.
- PID、JSON和路径存在性都不能替代OS lock的liveness事实.

### 当前结论

- stable lock path一旦创建就永久保留;退出只释放descriptor.
- metadata必须使用独立sidecar原子替换,不能rename被锁文件本身.
- local-default registry必须引用sidecar identity,否则PID复用会产生ABA误判.

### 后续讨论入口

- 阅读`specs/zenoh-unixpipe-fast-path-plan.md` 3.5节和`LATER_PLANS__local_default_atomic_lease.md`的legacy退役计划.
