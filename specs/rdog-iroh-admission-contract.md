# rdog Iroh 会话准入契约

状态：RAI-179 U1 已裁定的契约；不是生产 PTY 准入开关，也不表示 P3 已验收。隔离候选的实现和证据分别由 U2、U3 补齐。

## 默认入口与隔离入口

正常构建的 Iroh CLI 和 daemon 只处理 ping。默认请求门禁的唯一定义是 [`ping_only`](../src/control_iroh.rs)，调用方为同文件的 `run_control` 和 `serve_ping`。本契约不改变其默认行为，也不改变旧 TCP、WebSocket 或 Zenoh 准入。目录解析和传输握手只验证各自职责，不能授予 PTY 权限。

PTY 生命周期验证只走独立的测试编译入口。生产构建没有启用该入口的环境变量、CLI 参数或配置；测试入口只处理固定无害子进程和有限测试输入，不接收命令行、可执行文件路径或脚本。U2 实现时由测试模块持有该入口，U3 仅在回环、私有网络及进程命名空间、隔离 HOME/TMPDIR 下验。公网、真实设备、部署及 bulk 均不在本票范围。

## 身份、签发和租约

- **H1 = B：** 每 daemon 登记一枚长期公钥。会话 token 由本地签发，接收端验证签名以及签发身份、握手身份、目标和会话绑定。地址、目录结果和另一个 daemon 的身份都不能代用。密钥私有部分不进仓库或回执。
- 会话 token 有期限，可显式续租，不能发行无期限授权。H1 的运行时上限见隔离实现的 [`MAX_SESSION_TTL`](../src/control_iroh/admission_tests.rs)，由边界用例校验，不是生产默认 TTL。续租须重新校验许可；失败不延长旧期限，已到期会话不能靠续租复活。未通过准入的请求不得创建子进程。
- 到期停止新操作，对该租约所属的活动或已 detach 夹具执行一次回收。拒绝码与 PTY 终帧分别证明授权和生命周期，不能互相替代。失去连接不是收到终帧的证据。

## 策略权威、角色和 ownership

- **H2 = A：** 目标 daemon 的本地 allowlist 是唯一授权来源。生产读写边界归 `rdog` 本地管理子命令，U2 不发布新子命令或配置，仅用隔离的本地策略存储验语义。不存在中心回退或第二权威；策略不可读或损坏时不得放行新请求。
- 本地管理员管理 allowlist 与撤销；申请者只能用已登记身份请求自己的授权；目标 daemon 校验授权并跟踪租约。目录、传输和本地管理员角色不能被普通会话继承。
- PTY 会话 ID 仅标识子进程，不等于控制权。每次 attach 更新 `control_session_id`；异主或失效 attachment 的 `Disconnect`、`Close` 不得回收当前 owner。代码锚点为 [`AttachedPtySession`、`attach_active_pty_session` 与 runtime 的 `Disconnect` 分支](../src/pty_control.rs)。测试准入入口在派发前校验 owner，不能把旧有的本机显式 force close 当成带权限的远端 close。Detach 不终结进程，也不延长租约。

## 撤销排序和错误语义

**H3 = C：** 默认 graceful，持久写入撤销标记后拒绝新会话及续租，旧会话只运行到当前租约截止。仅显式 emergency 在完成同样两步之后才通知现存会话终止。撤销状态必须与新准入串行化；落盘失败不报成功，重启必须先载入该状态，拒绝已撤销身份的新会话。本规格不增加中心分发。

| 情形 | 拒绝阶段与真相源 |
|---|---|
| 默认入口收到非 ping | 本地或服务端只读门禁拒绝；错误来自 [`ping_only`](../src/control_iroh.rs)，不能声称已建立可写会话。 |
| 身份、授权或绑定校验失败 | 在处理器前拒绝，不执行操作。现有拒绝码与原因取自 [`DENIED_CODE` / `DENIED`](../src/control_iroh.rs)。 |
| 活动会话租约到期 | 关闭授权与回收该会话的夹具；隔离入口的拒绝取自 [`Rejection::Expired` 的 `code_reason`](../src/control_iroh/admission_tests.rs)，沿用 P2-A 的 `lease_expired` 语义，不是生产入口定义。 |
| Graceful / emergency 撤销 | 新会话和续租归目标拒绝；emergency 的已建会话终止须显式可观测。终态以 [`PtyTerminalOutcome`](../src/pty_control.rs) 的闭合帧为准，不以 socket EOF 或日志行代替。 |
| 过期/异主的操作 | 证据须显示原会话未受影响；不能把 "没有收到响应" 当作命令执行或回收成功。 |

## 验收证据

U2 需要新回执证明默认入口仍拒绝非 ping，并验证本地签发、目标/身份校验、续租上限、落盘撤销与重启拒绝。U3 记录授权前零进程、同一固定夹具的开/附着/输入/重附、到期回收和终帧、本地 TTY 恢复、异主不回收，以及 graceful 与 emergency 的不同终止时机。每项保留退出码和原始回执；只看局部单测、只读 ping 或旧 TCP 进程回执均不能签成正式 Iroh PTY 验收。
