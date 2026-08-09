# local-default registry 关键洞察

## [2026-07-18 10:22:47] [Session ID: omx-1784304547353-h5409r] 主题: destructive cleanup的ownership必须与资源路径同构

### 发现来源

- 同名重复 daemon与不同名称共享显式 unixpipe path的两组动态实验.

### 核心问题

- service-name只标识逻辑 daemon,不能唯一标识它会删除或重建的 FIFO path.
- 只要配置允许不同 identity解析到同一路径,基于 identity的 guard就无法证明 cleanup权限.

### 为什么重要

- 对共享文件、socket、FIFO或缓存目录执行 destructive cleanup前,guard key必须从被操作资源的 canonical identity生成.
- endpoint、cleanup和registry如果各自推导路径,即使每段代码局部正确,组合后仍会形成 split-brain.

### 未来风险

- 当前 PID sidecar仍只验证进程存活,没有验证进程启动时间或二进制身份;PID复用可能误判 owner.
- local-default继续使用 JSON + PID双文件,写入中断仍可能再次制造状态分裂.

### 当前结论

- 当前修复已阻止已复现的活跃 FIFO误删,并保留 dead PID后的 stale cleanup.
- 原子状态源与强进程身份校验尚未实现,已进入延期计划.

### 后续讨论入口

- 先审阅 `src/zenoh_runtime.rs` 的 local-default/path guard实现,再设计统一 lease记录与原子替换协议.
