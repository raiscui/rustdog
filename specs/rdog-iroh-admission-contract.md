# rdog Iroh 会话准入契约

状态：RAI-179 定案的契约，RAI-305 完成生产接线。生产 iroh 入口的 PTY 只在 daemon profile 显式给出 `policy_file` 时开启；未给出时仍是 P1 的 ping-only + 回环门禁。跨网地址放开由 `routable` 控制，缺省不启用。正式 P3、公网与 NAT、真实远端设备与 bulk 仍未验收。

## 默认入口与生产入口

正常构建的 Iroh CLI 和 daemon 只处理 ping。默认请求门禁的唯一定义是 [`ping_only`](../src/control_iroh.rs)，调用方为同文件的 `run_control` 和 `serve_ping`。

准入 PTY 通道的开关是 [`DaemonConfig::policy_file`](../src/control_iroh.rs)：给出策略文件时 daemon 走 [`serve_admitted`](../src/control_iroh.rs)，否则走 `serve_ping`。两条路径都不改变旧 TCP、WebSocket 或 Zenoh 准入。

准入通道的服务面只有 ping 和 PTY：

- `@ping` 与 `@pty` / `@pty-attach` / `@pty-close` / `@pty-detach`，以及桥接内的 `@pty-stdin` / `@pty-resize`。
- 其余 control 行一律拒绝，错误码与文案取自 [`LANE_REFUSED`](../src/control_iroh.rs)。
- bulk 传输不在这条通道上，P4 保持关闭。

跨网地址放开用 profile 的 `routable` 字段（缺省 `false`）。地址的唯一策略是 [`network`](../src/control_iroh.rs) 与 [`relay_allowed`](../src/control_iroh.rs)：目录、endpoint、relay-QAD、advertise 与注册校验都走它们。缺省时仍是 P1 的 IPv4 回环门禁与原错误文案；`routable` 下拒绝未指定、组播和广播地址。

交互路径回退是 [`Route::Auto`](../src/control_iroh.rs)：先试直连地址，直连不可得再走显式 relay，最终实际路径由 [`connect_with_path`](../src/control_iroh.rs) 记录并打印 `iroh_path=`。`direct` / `relay` 两个固定取值仍按 profile 强制，不允许串路。

## 身份、签发和租约

- **H1 = B：** 每 daemon 登记一枚长期公钥。会话 token 由本地签发，接收端验证签名以及签发身份、握手身份、目标和会话绑定。地址、目录结果和另一个 daemon 的身份都不能代用。密钥私有部分不进仓库或回执。
- 会话 token 有期限，可显式续租，不能发行无期限授权。单份 token 的运行时上限是 [`MAX_SESSION_TTL`](../src/control_iroh/admission.rs)。续租由客户端按 TTL/3 发起（[`spawn_lease_renewer`](../src/control_iroh.rs)），服务端重新校验许可（[`Admission::renew`](../src/control_iroh/admission.rs)）；失败不延长旧期限，已到期会话不能续活。全会话总时长和续租次数不设上限（H4 = B）。
- [`Admission`](../src/control_iroh/admission.rs) 以解析后的 UUID 保证实例内防重，确认终止后也保留已消费标识。跨重启的单次使用保证仍未验证。
- 未准入不创建子进程：准入在 `@pty` 分派之前完成，`serve_lane` 先校验活动租约（[`require_active`](../src/control_iroh.rs)）再调用 PTY 打开。
- 截止回收由生产定时器 [`lease_sweeper`](../src/control_iroh.rs) 驱动，无需后续请求，覆盖活动及已 detach 的会话。拒绝码与 PTY 终帧分别证明授权和生命周期，socket EOF 不等于终帧。

## 策略权威、角色和 ownership

- **H2 = A：** 唯一策略权威是本地 allowlist 文件，读取入口为 [`Admission::policy`](../src/control_iroh/admission.rs)，没有中心或环境回退。准入通道上的 `@ping` 也由同一文件决定（[`Admission::admits_ping`](../src/control_iroh/admission.rs)）。策略不可读或损坏时拒绝新请求。生产 `rdog` 的本地策略管理子命令属未来范围：当前由操作者在 daemon 侧直接维护该文件。
- 本地管理员管理 allowlist 与撤销；申请者只能用已登记身份请求自己的授权；目标 daemon 校验授权并跟踪租约。目录、传输和本地管理员角色不能被普通会话继承。
- PTY 会话 ID 仅标识子进程，不等于控制权。准入 lane 每次 close / detach / attach 都按主体判定（[`authorize_pty_subject`](../src/pty_control.rs)），同主体跨连接可以操作自己的会话，异主体拒绝。旧通道仍按 id 关闭（[`close_active_pty_session`](../src/pty_control.rs)），其语义是"可达即全权"，在本契约里不作为 owner 授权使用。Detach 不终结进程，也不延长租约。

## 撤销排序和错误语义

**H3 = C：** 默认 graceful，持久写入撤销标记后拒绝新会话及续租，旧会话只运行到当前租约截止。仅显式 emergency 在完成同样两步后通知现存活会话终止。策略文件用 `revoked` 表达第一步、`terminate` 表达第二步，两步都落盘后才生效。存储与撤销状态区分写入前失败、替换后持久性未确认及标记持久但终止待确认；未确认持久性时拒绝新的准入，不声称写入成功。待终止记录在同一实例的重试中保留，只有观察到终帧并回收进程后才确认结束。

回收选择与终帧收尾都在 [`Admission::sweep`](../src/control_iroh/admission.rs) 里，同一把锁、同一次时钟读取；重启后的待终止恢复与中心分发仍未保证。

| 情形 | 拒绝阶段与真相源 |
|---|---|
| 默认入口收到非 ping | 本地或服务端只读门禁拒绝；错误来自 [`ping_only`](../src/control_iroh.rs)，不能声称已建立可写会话。 |
| 准入通道收到非 ping、非 PTY 的 control 行 | 该 lane 拒绝，文案取自 [`LANE_REFUSED`](../src/control_iroh.rs)。 |
| 身份、授权或绑定校验失败 | 在处理器前拒绝，不执行操作。拒绝码与原因取自 [`DENIED_CODE` / `DENIED`](../src/control_iroh.rs)；准入通道上的拒绝以握手回复或连接关闭码两种形式送达。 |
| 活动会话租约到期 | 关闭授权并回收该会话；拒绝取自 [`Rejection::Expired` 的 `code_reason`](../src/control_iroh/admission.rs)，终帧 reason 取自 [`EXPIRED`](../src/control_iroh/admission.rs)。 |
| Graceful / emergency 撤销 | 新会话和续租归目标拒绝；emergency 的已建会话终止须显式可观测，终帧 reason 取自 [`REVOKED_REASON`](../src/control_iroh/admission.rs)。 |
| 过期/异主的操作 | 证据须显示原会话未受影响；不能把"没有收到响应"当作命令执行或回收成功。 |

## 验收证据

RAI-305 交付的回执分两层：

- 隔离回环矩阵（[`tests/manual/iroh_pty_lane.py`](../tests/manual/iroh_pty_lane.py)）：准入前零进程、attach/stdin/退出码、同主体跨连接关闭、异主体拒绝且目标仍在、detach 后无后续请求的到期回收、graceful 拒绝新会话而保留现存、emergency 终止现存，以及真 TTY 恢复。
- 非回环矩阵（同一脚本的 `--routable` 模式，两个可路由地址）：真实握手、长会话、自动回退到 relay 并记录实际路径，以及缺省路径仍 ping-only + 回环。

RAI-179 的隔离夹具矩阵（[`fixed_fixture_lifecycle_and_revocation_over_isolated_iroh`](../src/control_iroh/lifecycle_tests.rs)）在搬迁后重跑仍通过，用于证明同一份准入语义没有被搬移改变。

仍未验证：公网与多 NAT 拓扑、真实远端设备、bulk 并发、跨重启单次使用保证、Windows 同 SHA 产物。只看局部单测、只读 ping 或旧 TCP 进程回执均不能签成正式 Iroh PTY 验收。
