# rdog Iroh 会话准入契约

状态：RAI-179 定案的契约，RAI-305 完成生产接线。生产 iroh 入口的 PTY 只在 daemon profile 显式给出 `policy_file` 时开启；未给出时仍是 P1 的 ping-only + 回环门禁。跨网地址放开由 `routable` 控制，缺省不启用。正式 P3、公网与 NAT、真实远端设备与 bulk 仍未验收。

## 默认入口与生产入口

正常构建的 Iroh CLI 和 daemon 只处理 ping。默认请求门禁的唯一定义是 [`ping_only`](../src/control_iroh.rs)，调用方为同文件的 `run_control` 和 `serve_ping`。

准入 PTY 通道的开关是 [`DaemonConfig::policy_file`](../src/control_iroh.rs)：给出策略文件时 daemon 走 [`serve_admitted`](../src/control_iroh.rs)，否则走 `serve_ping`。两条路径都不改变旧 TCP、WebSocket 或 Zenoh 准入。

准入通道的服务面只有 ping 和 PTY：

- `@ping` 与 `@pty` / `@pty-attach` / `@pty-close` / `@pty-detach`，以及桥接内的 `@pty-stdin` / `@pty-resize`。
- 其余 control 行一律拒绝，错误码与文案取自 [`LANE_REFUSED`](../src/control_iroh.rs)。
- bulk 传输不在这条通道上，P4 保持关闭。

跨网地址放开用 profile 的 `routable` 字段（缺省 `false`）。地址的策略是 [`network`](../src/control_iroh.rs) 与 [`relay_allowed`](../src/control_iroh.rs)：目录、endpoint、relay/relay-QAD 的**绑定**地址与注册校验都走它们。缺省时仍是 P1 的 IPv4 回环门禁与原错误文案；`routable` 下拒绝未指定、组播和广播地址。

relay 侧另有一个 `advertise` 字段（缺省空），只改写给对端的 relay URL，不改绑定位：1:1 NAT 机型的公网地址（EIP）只存在于 NAT 设备上，本机网卡根本绑不住（`EADDRNOTAVAIL`），而 registry 原本用绑定地址拼 relay URL，于是对端拿到一个它到不了的地址、注册被 `iroh_registration_denied` 拒。所以 `bind`/`relay_*`/`qad` 绑网卡上的私有地址，`advertise` 填公网地址。**`advertise` 有意不过 [`network`](../src/control_iroh.rs)**：它不是本机地址，按绑定地址的规则校验它只会把唯一可用的配置方式判死；它进 URL 后仍要过 `RelayUrl` 解析与对端的 `relay_allowed`（https + 显式端口）。

运维事实（真机跨网两轮后补，别当 bug 查）：

- **`bind` 可以整条不写。** [`bind_addr`](../src/control_iroh.rs) 在 `routable` 下把"省略"和 `0.0.0.0:0` 都解析成通配绑定 `0.0.0.0:0`（[`bindable`](../src/control_iroh.rs) 让绑定面接受未指定地址），daemon 与 client 都适用：换网（家宽 → 手机热点）、换 IP 都不用改配置。只有这两种写法被放宽——通配地址 + 固定端口仍按 `iroh routable profile requires an explicit address` 拒，不静默丢掉端口；loopback profile 仍要回环 socket。**被通告/被拨的地址永远具体**：daemon 注册时用 [`register`](../src/control_iroh.rs) 枚举端点实际拥有的接口地址（通配绑定下是逐接口展开，不会出现 `0.0.0.0` 字面值），client 侧 [`connect_route`](../src/control_iroh.rs) 把**所有**候选 IP 一起拨，不再只用第一个。若主动钉了具体地址，它仍须跟本机当前地址走：旧地址不在网卡上时新建连接 `EADDRNOTAVAIL`，CLI 表面是 `Failed to bind sockets`（实测 `192.168.50.143` 切 `172.20.10.2` 沿用旧值即报该错），这时先重填 `bind`。
- **relay 主机的运行库**：本地 glibc 2.39 构建在 glibc 2.38 的宿主上 `GLIBC_2.39 not found`，用 `rust:1-slim-bookworm` 容器构建可解。用 `LD_LIBRARY_PATH` 外带依赖库时只能带 GUI 那几个（`libgbm`/`wayland`/`xcb`/`drm`/`expat`/`Xau`/`Xdmcp`/`bsd`/`md`/`ffi`/`stdc++`），把 `ld-linux`/`libc`/`libm`/`libpthread`/`libgcc_s` 混进去会让进程直接 `Floating point exception (core dumped)`。

### 目录的 relay 承载（`directory.via_relay`）

数据路径可以走 relay，目录原本不行：目录端点只能按 `id + SocketAddr` 拨，两端都没有入站地址时连"去哪儿查这个 daemon"都发不出去。拨号目标由 [`Directory::dial`](../src/control_iroh.rs) 决定，`directory` 写 `via_relay: true` 配 `relay: {url, root_cert}` 时改走 relay（两个字段都必填：见下面的信任面）：

- **拨号目标只有 `TransportAddr::Relay(url)`**，不带任何 IP 候选——地址在配置里本来就不存在，不是查出来的。这一跳用独立预算 `DIRECTORY_CONNECT`，并**按整次调用限时** `timeout(LEASE - RENEW, ...)`：connect、open、写帧、读帧四步任一慢，都不会让这次续期越过它要刷新的租约；同时**关掉 QAD**（`RelayConfig::new(url, None)`），目录请求因此不会漂到直连路径。**查询成功拿到回复之后**才落 `iroh_directory_path=<relay|direct>`（取不到选中路径时 `debug` 级 `unknown (<err>)`），所以「目录跳走的是 relay」是实测行而不是配置推断，也不会出现「先打路径、后失败」的假读数；这一行在稳态下按**状态变化**才进 `info`（daemon 每 5s 续期一次，不该每次加一行），其余走 `debug`。
- **目录侧**（rendezvous profile 的 `directory_relay: {url, root_cert}`，同样两项都必填）把自己的目录端点挂到该 relay 上（同一个 key，id 不变），并把自身 id 加进自建 relay 的准入表，否则它自己会被自己的 ACL 拒掉；ready 行随之多出 `directory_relay=<url>`。
- **daemon 侧不能有第二个端点**：一条对端身份在一个 relay 上只有一条连接，目录端点与服务端点同时挂同一个 relay 会互相顶掉（实测原文 `Relay server reports problem: Another endpoint connected with the same endpoint id`，随后注册超时、daemon 非零退出）。所以打开开关时 daemon 复用服务端点本身（ALPN 同时带 `CONTROL` 与 `DIRECTORY`，relay map 是 lane relay ∪ directory relay）；关掉开关时仍是"目录端点无 relay + 服务端点挂 lane relay"两个端点。服务端点上的 `DIRECTORY_ALPN` **不对内服务**：它只是为了从该端点拨出目录请求；对端若用该 ALPN 拨进来，daemon 记 `iroh directory alpn not served peer=<id>` 并按 `UNSERVED_CODE`（44）关闭，不留一个"看起来支持"的空洞。
- **宽化只挂在显式的信任锚上**：一个 endpoint 只有一份 TLS 配置，根集合由 [`relay_roots`](../src/control_iroh.rs) 一处决定：
  - 只拨目录的端点（rendezvous 的目录端点、client 的查找端点）用 `custom_roots(directory.relay.root_cert)`，**不引入公共根**；
  - daemon 的共享端点（lane + 目录同一个 endpoint）用 `custom_roots(lane 链).with_extra_roots(directory 链)`，也就是两份**显式链**，**不**打底任何公共根：私有目录 relay 的场景根本不需要 WebPKI，无条件 `embedded()` 会让整条私有链路信任全量公共 CA。公共目录 relay 需要操作者把那份根包作为 `root_cert` 交进来。

  `root_cert` 因此是**必填字段**：打开 `via_relay` 却不给信任锚时 profile 直接解析失败，不存在"开关一开顺带放宽 TLS"的路径 —— 功能维度（开不开目录的 relay 承载）与安全维度（信任面）不共用一个布尔量；用公共 relay 就必须显式给出那份 CA 链，这是"操作者知道自己信什么"的价格。关掉开关时仍是 `custom_roots(lane 链)`。
- **地址策略不放宽**：目录 relay 的 URL 必须 https，非 `routable` profile 还必须是回环主机。四种半配置一律 fail closed：有 `relay` 没 `via_relay`、有 `via_relay` 没 `relay`、两者都没有又不给 `addr`、以及**同时给了 `addr` 与 `via_relay`**（两个拨号答案不静默取其一）。

实测（`tests/manual/iroh_directory_relay.sh` → `iroh_directory_relay.py`，三网络栈，目录端点绑在中转机回环上、任何对端都拨不到它；回执 `handoffs/rai426/receipts/directory-relay.log`）：

| 用例 | 结果 |
|---|---|
| 不带开关、`directory.addr` 填一个没人听的地址（今天的行为） | daemon exit=1 且从未注册，client exit=1 |
| 打开开关，`path:"auto"` | `@ping` 应答，`iroh_path=direct` |
| 打开开关，`path:"relay"` | 应答，`iroh_path=relay` |
| 打开开关，`path:"auto"` + 黑洞掉 daemon 地址 | 先 `iroh_path=direct unavailable`，再 `iroh_path=relay` 应答 |

`data.online()` 的语义要读准：它只表示「relay map 里**至少一条** home 连接已建立」，当 endpoint 同时挂着 lane relay 与目录 relay 时，先通的可能是后者。**注册成功证明的是「目录这一跳可达」**；lane relay 是否承载这个 daemon 要另看该 relay 自己的连接状态 —— Registry 的 `Register` 只做形状校验（id、IP 候选、恰好 1 个等于配置值的 relay），不做任何可达性探测，而注册本身走的是目录 relay。

注册契约的形状（[`Registry::request`](../src/control_iroh.rs) 的 `Register` 分支）：
- **至少 1 个 IP 候选 + 恰好 1 个 relay，且该 relay 等于配置值**；每个 IP 候选单独过 [`candidate_ok`](../src/control_iroh.rs)（`network` + 端口非 0）且端口非 0。通配绑定朝多网卡展开，所以候选数不设上限——这是"多候选"的唯一合法来源。
- **`routable` 下回环地址不是候选**：对端拨 `127.0.0.1` 只会打到它自己，既浪费一次拨号又会让日志出现误导性的前半段失败。daemon 通告前用同一个 `candidate_ok` 过滤，registry 收到回环候选同样拒（与未指定地址同类，仍是 `iroh_registration_denied`）；非 `routable`（回环 profile）不变。
- **去重不需要单独代码**：`EndpointAddr.addrs` 是 `BTreeSet<TransportAddr>`，重复地址在容器层就不存在。
- **一个名字只有一个 owner**：`owners[name] == Some(peer)` 与 `addr.id == peer` 两条保留不动。名字是别人拨的地址索引，允许改绑或抢占等于允许冒名注册；这条与候选数量无关，放宽候选数不动它。
- 拒绝仍只报一个 `iroh_registration_denied`：0 个 IP、未指定地址（`0.0.0.0` 字面值或端口 0）、relay 不匹配、非 owner 注册，四种都走这一个码。

实测（模拟 1:1 NAT 的三个网络栈，`tests/manual/iroh_pty_advertise.sh` → `iroh_pty_lane.py --advertise`，回执 `receipts/u10/advertise.log`）：绑私网通告公网时 daemon 的 relay home 是通告值、注册被接受、ping 通过；不写 `advertise` 时注册被拒；通告一个没人持有的地址时 daemon 非零退出且从未注册。公网真机由 RAI-302 侧另判。

通配绑定的实测（`receipts/u12/`）：daemon 通配绑定（netns 双接口）注册成功、client 省略 `bind` 后从非回环接口 ping 通过（`routable-wildcard.log` 18/18，`routable.log` 18/18）；两轮的 `iroh directory registered … candidates=` 都只列非回环候选（通配那轮双接口也只出现 `10.42.0.2:<port>`）；`bind` 填本机没有的地址仍非零退出。注册契约的多候选正例与五个负例（0 个 IP、未指定地址、回环候选、relay 不匹配、非 owner）由 `src/control_iroh/registry_tests.rs` 覆盖。

写帧失败即作废该流：[`FrameGuard`](../src/control_iroh.rs) 在帧被完整写入之前不放行下一帧，任何一次写失败或写超时都把这条流标记成不可用，之后的所有写直接返回 [`FRAME_BROKEN`](../src/control_iroh.rs)，lane 随之结束并用同一 reason 关闭会话（[`abort_broken_frame`](../src/control_iroh.rs)）。理由是残留半帧会让后续帧错位解析，宁可断开也不继续发。

单帧写预算由 profile 的 `frame_timeout_ms` 给出，覆盖 lane 上的帧写（目录那一跳用固定预算：走 IP 时 `CONNECT`，走 relay 时 `RELAY_CONNECT`），两侧都校验：低于 [`MIN_FRAME_TIMEOUT`](../src/control_iroh.rs) 或高于 [`MAX_FRAME_TIMEOUT`](../src/control_iroh.rs) 直接按字段名报错，缺省 3s。

实测边界（三主机拓扑，`tests/manual/iroh_pty_routable.sh`，`RAI305_STALL` / `RAI305_BURST` / `RAI305_FRAME_MS` 驱动）：

- 链路整段消失 2s、6s、以及 6s 配 12s 预算：会话都以连接层的 `connection lost`（无可用路径）结束，写预算不改变结果 —— 路径消失归连接活性管。
- 冻结读端 20s + 16MB 积压，配 **1s** 预算（短于连接活性）：写挂起在 1s 到点，daemon 记录 `iroh lane aborted reason=iroh_frame_broken`，会话以该 reason 关闭。回执 `receipts/u7/u7-stall-budget1s.log`。

所以写预算的实际作用范围是"路径仍在但对端停止读取、窗口写满"的写挂起，且只有当它短于连接的 idle 判定时才会成为约束条件；预算长于连接活性时，连接层先收掉会话。

交互路径回退是 [`Route::Auto`](../src/control_iroh.rs)：先试直连地址，直连不可得再走显式 relay。每个候选由 [`candidate_endpoint`](../src/control_iroh.rs) 建立与它自己那份固定 profile 相同的 endpoint（`direct` = 绑定 IP、无 relay；`relay` = 不绑定、仅 relay），因为一条已经卡死的远端路径会被复用重试，同 endpoint 换候选拿不到新的 QAD 应答。候选预算按性质分开：`direct` 用 `CONNECT`，`relay` 用 `RELAY_CONNECT`（relay 候选要先建 home）。最终实际路径由 [`connect_with_path`](../src/control_iroh.rs) 记录并打印 `iroh_path=`，且实际路径与预期候选不一致时按串路拒绝。`direct` / `relay` 两个固定取值仍按 profile 强制，不允许串路。

## 身份、签发和租约

- **H1 = B：** 每 daemon 登记一枚长期公钥。会话 token 由本地签发，接收端验证签名以及签发身份、握手身份、目标和会话绑定。地址、目录结果和另一个 daemon 的身份都不能代用。密钥私有部分不进仓库或回执。
- 会话 token 有期限，可显式续租，不能发行无期限授权。单份 token 的运行时上限是 [`MAX_SESSION_TTL`](../src/control_iroh/admission.rs)，下限是 [`MIN_SESSION_TTL`](../src/control_iroh/admission.rs)（低于下限的 `lease_ttl_ms` 与签发都拒绝，避免续租线程空转）。续租由客户端按 TTL/3 发起（[`spawn_lease_renewer`](../src/control_iroh.rs)），服务端重新校验许可（[`Admission::renew`](../src/control_iroh/admission.rs)）；会话进行中续租帧由 lane 侧的 [`LaneIo`](../src/control_iroh.rs) 在桥接层截获并处理，绝不写进远端 PTY 的 stdin，否则续租会被当成输入、租约到点自动杀掉会话。失败不延长旧期限，已到期会话不能续活。全会话总时长和续租次数不设上限（H4 = B）。
- 跨主机签发的 grant 允许接收侧时钟落后，容忍窗口是 [`CLOCK_SKEW`](../src/control_iroh/admission.rs)；超出窗口才拒绝。否则时钟快几毫秒的对端会被判成 `iroh_target_denied`，与策略拒绝无法区分。
- 一条租约只持有一个 PTY 会话：`@pty` 打开前检查租约已持有的会话，仍存活就拒绝（[`Admission::session_of`](../src/control_iroh/admission.rs)）。detach 后进程按设计继续活着，若允许同租约再开一条，第一条就再无回收路径。
- [`Admission`](../src/control_iroh/admission.rs) 以解析后的 UUID 保证实例内防重，确认终止后也保留已消费标识。跨重启的单次使用保证仍未验证。
- 未准入不创建子进程：准入在 `@pty` 分派之前完成，`serve_lane` 先校验活动租约（[`require_active`](../src/control_iroh.rs)）再调用 PTY 打开。
- 截止回收由生产定时器 [`lease_sweeper`](../src/control_iroh.rs) 驱动，无需后续请求，覆盖活动及已 detach 的会话。拒绝码与 PTY 终帧分别证明授权和生命周期，socket EOF 不等于终帧。

## 策略权威、角色和 ownership

- **H2 = A：** 唯一策略权威是本地 allowlist 文件，读取入口为 [`Admission::policy`](../src/control_iroh/admission.rs)，没有中心或环境回退。准入通道上的 `@ping` 也由同一文件决定（[`Admission::admits_ping`](../src/control_iroh/admission.rs)）。策略不可读或损坏时拒绝新请求。生产 `rdog` 的本地策略管理子命令属未来范围：当前由操作者在 daemon 侧直接维护该文件。
- 本地管理员管理 allowlist 与撤销；申请者只能用已登记身份请求自己的授权；目标 daemon 校验授权并跟踪租约。目录、传输和本地管理员角色不能被普通会话继承。
- PTY 会话 ID 仅标识子进程，不等于控制权。准入 lane 每次 close / detach / attach 都按主体判定（[`authorize_pty_subject`](../src/pty_control.rs)），同主体跨连接可以操作自己的会话，异主体拒绝。旧通道仍按 id 关闭（[`close_active_pty_session`](../src/pty_control.rs)），其语义是"可达即全权"，在本契约里不作为 owner 授权使用。Detach 不终结进程，也不延长租约。

## 操作者路径

本地管理员用 `rdog policy --config <daemon profile> <list|admit|revoke|emergency>` 管理准入方（profile 的 `policy_file` 指到哪个文件就改哪个文件），不再需要手改 JSON：

- `list` 打印每个准入方的 endpoint id、SHA-256 指纹前缀、登记时间与备注，以及是否已 `revoked` / `terminate`。
- `admit` 写入 `allow` 并记录备注与登记时间（幂等）。对仍处于 `revoked` 的主体拒绝并明说"仍被撤销，先解封"，不会在撤销状态下谎报成功。
- 读-改-写全程持有策略文件的排他锁（`<policy>.lock` 上的 `flock`），两个操作者并发不会用旧快照覆盖掉别人刚落盘的撤销标记。
- `revoke`（graceful）先把主体写进 `revoked` 并落盘，返回即代表标记已持久化；此后新会话与续租被拒，现存会话跑到自身租约截止。revoke 同时把主体留在 `allow` 里作为"已登记但已撤销"的记录，准入判定仍以 `revoked` 一票否决，不靠从 allow 里删条目表达撤销。
- `emergency` 在 `revoked` 之外再写 `terminate`，同样先落盘再返回；服务侧在下一个 sweep 见到 `terminate` 后终止现存会话（按进程组，见下）。

落盘用 [`save_policy`](../src/control_iroh/admission.rs)：写临时文件、替换、同步父目录。写失败报"未写入"，替换后持久性无法确认报"已替换但持久性未确认"，两种情况命令都非零退出，不会假装成功。撤销标记只在文件里，所以 daemon 重启后仍然拒新（验收用例覆盖）。命令的实现入口是 [`run_policy`](../src/control_iroh.rs)。

## 撤销排序和错误语义

**H3 = C：** 默认 graceful，持久写入撤销标记后拒绝新会话及续租，旧会话只运行到当前租约截止。仅显式 emergency 在完成同样两步后通知现存活会话终止。策略文件用 `revoked` 表达第一步、`terminate` 表达第二步，两步都落盘后才生效。存储与撤销状态区分写入前失败、替换后持久性未确认及标记持久但终止待确认；未确认持久性时拒绝新的准入，不声称写入成功。待终止记录在同一实例的重试中保留，只有观察到终帧并回收进程后才确认结束。

回收选择与终帧收尾都在 [`Admission::sweep`](../src/control_iroh/admission.rs) 里，同一把锁、同一次时钟读取；重启后的待终止恢复与中心分发仍未保证。

终止按进程组执行（[`signal_pty_group`](../src/pty_control.rs)）：portable-pty 把命令启动成会话首进程，因此负 pid 的组信号覆盖该命令派生的一切，而不是只杀直接子进程；[`escalate_pty_group_kill`](../src/pty_control.rs) 走满 [`PTY_KILL_GRACE`](../src/pty_control.rs) 之后才探测组并升级为 SIGKILL。宽限期按组是否还在判断、不依赖会话注册表，因此 sweep、`@pty-close` 和断链三条关闭路径是同一套升级语义，忽略挂断的子进程不会让 sweep 永久停在待终止。

| 情形 | 拒绝阶段与真相源 |
|---|---|
| 默认入口收到非 ping | 本地或服务端只读门禁拒绝；错误来自 [`ping_only`](../src/control_iroh.rs)，不能声称已建立可写会话。 |
| 准入通道收到非 ping、非 PTY 的 control 行 | 该 lane 拒绝，文案取自 [`LANE_REFUSED`](../src/control_iroh.rs)。 |
| 身份、授权或绑定校验失败 | 在处理器前拒绝，不执行操作。拒绝码与原因取自 [`DENIED_CODE` / `DENIED`](../src/control_iroh.rs)；准入通道上的拒绝以握手回复或连接关闭码两种形式送达。 |
| 活动会话租约到期 | 关闭授权并回收该会话；拒绝取自 [`Rejection::Expired` 的 `code_reason`](../src/control_iroh/admission.rs)，终帧 reason 取自 [`EXPIRED`](../src/control_iroh/admission.rs)。 |
| Graceful / emergency 撤销 | 新会话和续租归目标拒绝；emergency 的已建会话终止须显式可观测，终帧 reason 取自 [`REVOKED_REASON`](../src/control_iroh/admission.rs)。 |
| 同一租约再开一个 PTY | 拒绝，reason 取自 [`LEASE_BUSY`](../src/control_iroh.rs)，与 lane 能力拒绝区分。 |
| 过期/异主的操作 | 证据须显示原会话未受影响；不能把"没有收到响应"当作命令执行或回收成功。 |

## 验收证据

RAI-305 交付的回执分两层：

- 隔离回环矩阵（[`tests/manual/iroh_pty_lane.py`](../tests/manual/iroh_pty_lane.py)）：准入前零进程、attach/stdin/退出码、会话活过 TTL/3 的续租且续租帧不进终端、撤销时孙进程随进程组消失、同主体跨连接关闭、异主体拒绝且目标仍在、detach 后无后续请求的到期回收、graceful 拒绝新会话而保留现存、emergency 终止现存，以及真 TTY 恢复。
- 非回环矩阵（`tests/manual/iroh_pty_routable.sh` 搭三主机拓扑：客户 / 中转（目录 + relay，开转发）/ daemon，再以 `--routable` 跑同一脚本）：真实握手、长会话、relay-only 可达、直连被黑洞时 `direct` 必失败而 `auto` 落到 `iroh_path=relay`，以及缺省路径仍 ping-only + 回环。

RAI-179 的隔离夹具矩阵（[`fixed_fixture_lifecycle_and_revocation_over_isolated_iroh`](../src/control_iroh/lifecycle_tests.rs)）在搬迁后重跑仍通过，用于证明同一份准入语义没有被搬移改变。

仍未验证：公网与多 NAT 拓扑、真实远端设备、bulk 并发、跨重启单次使用保证、Windows 同 SHA 产物。只看局部单测、只读 ping 或旧 TCP 进程回执均不能签成正式 Iroh PTY 验收。
