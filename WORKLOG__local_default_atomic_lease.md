# local-default 原子lease工作记录

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 任务名称: local-default原子process lease

### 任务内容

- 将Unix service-name、canonical unixpipe path和local-default namespace三类PID guard迁移到共享OS文件锁lease.
- 保留三类独立resource key,避免错误合并冲突域.
- 增加稳定lock file、原子metadata sidecar、local-default registry lease引用与legacy v1兼容.
- 同步规格、TDD单测、SIGKILL e2e、真实旧版迁移和安装版live验证.

### 完成过程

- 用跨进程probe验证exclusive/shared互斥、SIGKILL自动释放和active lock path禁止unlink.
- 新增`ProcessLease`,以`File::try_lock`声明owner,Drop只关闭descriptor,不删除stable path.
- JSON通过同目录temp、file sync、rename和parent directory sync发布.
- metadata发布失败时在仍持exclusive lock期间恢复原lock内容,避免留下self-blocking PID.
- local-default registry引用独立sidecar的完整lease identity;部分managed字段不再降级为legacy PID.
- Unix迁移到新lease;Windows保留原guard实现,没有扩大未验证平台行为.

### 验证结果

- runtime:38 passed.
- unixpipe e2e:12 passed.
- router-client:26 passed,2 ignored.
- all-targets check与release build:exit 0,0 errors.
- Mermaid flowchart与sequenceDiagram均经`beautiful-mermaid-rs --ascii`验证.
- live旧daemon拒绝、正常迁移、duplicate保护、SIGKILL接管、`jd` identity比较和ping全部通过.

### 总结感悟

- advisory lock的互斥绑定inode,不是路径名.只要active lock path被unlink,第二owner就能在新inode上同时获取lock.
- liveness必须来自OS lock一个真相源;PID只承担旧版兼容,JSON只承担identity和诊断.
- managed/legacy判别必须显式区分"扩展字段全空"与"扩展字段不完整",不能用同一个`None`表示两种语义.
