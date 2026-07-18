# local-default registry 错误修复记录

## [2026-07-18 10:22:47] [Session ID: omx-1784304547353-h5409r] 问题: 重复 daemon破坏活跃 unixpipe

### 现象

- `rdog daemon` 报本机默认 daemon已存在.
- 同时裸 `rdog control` 报没有可用 local-default registry,并回退到多个无关 FIFO候选.
- 显式 `rdog control mac.lab @ping` 仍能通过 UDP返回 pong.

### 已验证原因

- 第二实例先执行 `cleanup_stale_unixpipe_socket`,之后才获取 service-name/local-default ownership.
- 它虽然最终启动失败,却已删除第一实例的 canonical FIFO;client随后把缺少 uplink的 registry判为 stale并删除.
- 不同 daemon name共享显式 `socket_path` 时也能复现 FIFO inode被替换,说明只移动 service-name guard仍不完整.

### 修复

- endpoint composition产出唯一 resolved base,删除 daemon层的二次路径推导.
- router先获取 service-name guard,再获取 canonical base-path guard,两者成功后才允许 stale cleanup.
- local-default注册与 path guard都覆盖 router主循环生命周期.
- 测试使用动态 namespace、私有 state home和 RAII子进程清理.

### 验证

- 两条原始 RED场景均转绿:同名重复启动、不同名称共享显式 path都不会替换第一实例 FIFO.
- stale owner接管、多显式 endpoint拒绝、完整 runtime/e2e/router-client回归均通过.
- 真实机重复启动 exit 1,但前后两次裸 ping均返回 pong,uplink inode和 PID状态保持不变.

### 过程错误

- 首次 router-client隔离目录过长,触发95字节路径保护;改用短 `/tmp/rr.*` 后通过.
- 一次 zsh包装使用只读变量 `status`;改为 `exitCode` 并重新验证整体 exit 0.
- 前序上下文标题误用 UTC和估算时间;已追加校正,不回改 append-only记录.
