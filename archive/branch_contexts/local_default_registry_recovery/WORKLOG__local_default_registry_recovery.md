# local-default registry 恢复工作日志

## [2026-07-18 10:22:47] [Session ID: omx-1784304547353-h5409r] 任务名称: unixpipe ownership 与真实 daemon 恢复

### 任务内容

- 修复重复 daemon启动在 ownership确认前删除活跃 unixpipe FIFO的问题.
- 统一 listener endpoint、cleanup、registry 与 path guard使用的 canonical base路径.
- 补充隔离 e2e、stale owner恢复测试,同步 Zenoh规格并恢复本机 `mac.lab` daemon.

### 完成过程

- 用同名双启动 e2e复现第一实例 FIFO被删除,确认原 cleanup顺序参与失败路径.
- 用不同 daemon name + 同一显式 `socket_path` 反例证明 service-name guard不足以保护共享路径.
- 新增 base-path sidecar PID guard,并把 cleanup收敛到 `prepare_unixpipe_listener`.
- `compose_listen_endpoints` 返回最终 endpoints与 resolved base,拒绝多条显式 unixpipe或配置路径冲突.
- 重构 unixpipe e2e fixture为 RAII资源管理,隔离 `XDG_STATE_HOME`,避免测试删除真实 daemon状态.
- 安装当前工作树,终止旧 tmux daemon,启动安装版并执行真实重复启动 smoke.

### 验证

- runtime单测32个、unixpipe e2e 11个、router-client 26个通过,2个按既有条件跳过.
- scoped rustfmt、diff check、all-targets check、debug build与release install均成功.
- 真实重复启动前后 uplink inode保持512111047,daemon/guard PID保持69053.
- 重复启动正确 exit 1,前后两次裸 ping和最终显式 ping都返回 pong.

### 总结感悟

- destructive cleanup必须由它实际操作的资源路径所有权保护,不能从 service identity间接推断.
- 测试的"空环境"必须通过唯一 namespace和私有 state构造,不能清空全局 `$TMPDIR`.
- endpoint composition应同时返回运行配置与副作用所需 identity,否则调用层会再次推导并产生状态分裂.

## [2026-07-18 10:53:37] [Session ID: omx-1784304547353-h5409r] 任务名称: scoped commit 收口

### 任务内容

- 以显式文件清单提交 local-default ownership修复,不使用 `git add .`.

### 完成过程

- 核对 main分支、无子模块、15个 staged文件和 cached diff.
- 提交前重跑 runtime 32项、unixpipe e2e 11项、scoped rustfmt和 live ping.
- 创建 `fix(zenoh): protect active unixpipe listener ownership` commit.

### 总结感悟

- mixed-worktree提交必须先固定文件清单,再以 cached diff作为最终真相源.
