# rdog Iroh 会话准入契约

状态：RAI-179 U1 已裁定的契约；不是生产 PTY 准入开关，也不表示 P3 已验收。隔离候选的实现和证据分别由 U2、U3 补齐。

## 默认入口与隔离入口

正常构建的 Iroh CLI 和 daemon 只处理 ping。默认请求门禁的唯一定义是 [`ping_only`](../src/control_iroh.rs)，调用方为同文件的 `run_control` 和 `serve_ping`。本契约不改变其默认行为，也不改变旧 TCP、WebSocket 或 Zenoh 准入。目录解析和传输握手只验证各自职责，不能授予 PTY 权限。

PTY 生命周期验证只走独立的测试编译入口。生产构建没有启用该入口的环境变量、CLI 参数或配置；测试入口只处理固定无害子进程和有限测试输入，不接收命令行、可执行文件路径或脚本。U2 实现时由测试模块持有该入口，U3 仅在回环、私有网络及进程命名空间、隔离 HOME/TMPDIR 下验。公网、真实设备、部署及 bulk 均不在本票范围。

## 身份、签发和租约

- **H1 = B：** 每 daemon 登记一枚长期公钥。会话 token 由本地签发，接收端验证签名以及签发身份、握手身份、目标和会话绑定。地址、目录结果和另一个 daemon 的身份都不能代用。密钥私有部分不进仓库或回执。
- 会话 token 有期限，可显式续租，不能发行无期限授权。H1 的单份 token 运行时上限见隔离实现的 [`MAX_SESSION_TTL`](../src/control_iroh/admission_tests.rs)，不是生产默认 TTL。续租须重新校验许可；失败不延长旧期限，已到期会话不能续活。全会话总时长和续租次数未裁定。
- [`Admission`](../src/control_iroh/admission_tests.rs) 以解析后的 UUID 保证实例内防重，确认终止后也保留已消费标识。隔离入口不承诺跨重启单次使用；生产接线前必须另验跨重启的单次使用保证。未准入不创建子进程。
- 截止回收是 [`LaneDriver` / `Lane::finish`](../src/control_iroh/lifecycle_tests.rs) 中的测试专用定时器和终态处理，不需后续请求，覆盖活动及已 detach 固定夹具；生产没有该定时器。拒绝码与 PTY 终帧分别证明授权和生命周期，socket EOF 不等于终帧。

## 策略权威、角色和 ownership

- **H2 = A：** 隔离准入唯一使用 [`Admission::policy`](../src/control_iroh/admission_tests.rs) 读取本地 allowlist，没有中心或环境回退。生产 `rdog` 本地策略管理子命令属未来范围，本票不提供或改动该入口。隔离策略不可读或损坏时拒绝新请求。
- 本地管理员管理 allowlist 与撤销；申请者只能用已登记身份请求自己的授权；目标 daemon 校验授权并跟踪租约。目录、传输和本地管理员角色不能被普通会话继承。
- PTY 会话 ID 仅标识子进程，不等于控制权。每次 attach 更新 `control_session_id`；[`Lane::run`](../src/control_iroh/lifecycle_tests.rs) 为隔离入口校验 Close 的当前 owner，[`PtyRuntimeCommand::Disconnect`](../src/pty_control.rs) 在运行时检查当前 attachment。两者不证明旧生产 Close 或 force-close 带同样的 owner 校验，后者归 RAI-298；本票不改。Detach 不终结进程，也不延长租约。

## 撤销排序和错误语义

**H3 = C：** 默认 graceful，持久写入撤销标记后拒绝新会话及续租，旧会话只运行到当前租约截止。仅显式 emergency 在完成同样两步后通知现存活会话终止。[测试存储和撤销状态](../src/control_iroh/admission_tests.rs) 区分写入前失败、替换后持久性未确认及标记持久但终止待确认；未确认持久性时拒绝新的准入，不声称写入成功。待终止记录在同实例重试中保留，只有 [`Lane::finish`](../src/control_iroh/lifecycle_tests.rs) 观察到终帧及进程回收后才确认结束。重建策略对象遵从已落盘标记；还没有生产级通知与重启后的待终止恢复保证，也不包括中心分发。

| 情形 | 拒绝阶段与真相源 |
|---|---|
| 默认入口收到非 ping | 本地或服务端只读门禁拒绝；错误来自 [`ping_only`](../src/control_iroh.rs)，不能声称已建立可写会话。 |
| 身份、授权或绑定校验失败 | 在处理器前拒绝，不执行操作。现有拒绝码与原因取自 [`DENIED_CODE` / `DENIED`](../src/control_iroh.rs)。 |
| 活动会话租约到期 | 关闭授权与回收该会话的夹具；隔离入口拒绝取自 [`Rejection::Expired` 的 `code_reason`](../src/control_iroh/admission_tests.rs)，不是生产入口定义。[P2-A 夹具](../tests/manual/iroh_p2_probe.rs) 保留自己独立冻结的实验常量，不是第二个本入口真相源。 |
| Graceful / emergency 撤销 | 新会话和续租归目标拒绝；emergency 的已建会话终止须显式可观测。终态以 [`PtyTerminalOutcome`](../src/pty_control.rs) 的闭合帧为准，不以 socket EOF 或日志行代替。 |
| 过期/异主的操作 | 证据须显示原会话未受影响；不能把 "没有收到响应" 当作命令执行或回收成功。 |

## 验收证据

U2 需要新回执证明默认入口仍拒绝非 ping，并验证本地签发、目标/身份校验、续租上限、落盘撤销与重启拒绝。U3 记录授权前零进程、同一固定夹具的开/附着/输入/重附、到期回收和终帧、本地 TTY 恢复、异主不回收，以及 graceful 与 emergency 的不同终止时机。每项保留退出码和原始回执；只看局部单测、只读 ping 或旧 TCP 进程回执均不能签成正式 Iroh PTY 验收。
