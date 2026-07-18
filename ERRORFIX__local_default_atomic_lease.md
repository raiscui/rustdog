# local-default 原子lease错误修复记录

## [2026-07-18 12:40:06] [Session ID: omx-1784340333160-6bwnss] 错误修复: PID guard状态分裂与lease identity误判

### 现象

- PID guard只能证明某个PID存在,不能证明该PID仍属于原daemon实例.
- Drop或stale cleanup删除active lock path时,新进程可以在新inode上建立第二个owner.
- metadata发布失败后,同一进程重试会把自己写入的PID误判为legacy active owner.
- local-default旧registry在相同PID、不同lease ID的active lock下仍被判active.
- 部分managed字段会错误降级到legacy PID路径.

### 原因

- 旧实现把PID文件存在性、`kill -0`和多个JSON/FIFO文件组合成liveness判断,没有OS生命周期绑定的原子owner事实.
- stable lock内容先改写PID,但metadata发布错误路径没有回滚未提交状态.
- registry的lease字段没有与独立sidecar关联,managed与legacy缺少严格三态判别.

### 修复

- 使用stable inode上的non-blocking exclusive lock声明owner,shared probe判断active.
- Drop永不unlink stable lock path;正常退出和SIGKILL都由OS关闭descriptor释放lock.
- 保存获取前的lock原始内容,metadata未发布时在exclusive lock内回滚.
- local-default拆分lease sidecar与业务registry,比较schema、ID、resource、创建时间和PID.
- 只有五个lease扩展字段全空时才按legacy v1处理;部分或非法managed记录判inactive.

### RED/GREEN证据

- 发布失败重试RED:`0 passed,1 failed`,报self legacy PID;修复后`1 passed`.
- 不同lease ID RED:`0 passed,1 failed`,旧registry误报active;修复后`1 passed`.
- 部分managed字段RED:`0 passed,1 failed`,错误降级legacy;修复后`1 passed`.
- 最终runtime`38 passed`,unixpipe e2e`12 passed`,router-client`26 passed,2 ignored`.

### 以后避免

- 不要删除仍可能被持锁的路径.文件锁审查必须同时检查inode生命周期和unlink行为.
- 不要把`Option::None`同时用于legacy与invalid两种状态.
- 多文件发布失败路径必须验证重试语义,不能只测成功写入.
