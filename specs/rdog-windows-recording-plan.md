# Windows 动作录制实施方案

## 目标与交付边界

用户需要 rdog 具备类似 Terminator 的 Windows 动作录制能力. 对 agent 来说, 结果应是一份能说明操作顺序, 所属窗口, 控件候选和缺失信息的记录, 而不是成功启动但没有事件的空文件. 维护者应继承一条 Session, 一套 Journal 和一个平台采集入口.

本文定义目标架构, 并记录首阶段实现状态. 当前已接通 Windows recording-only 候选代码, 尚未完成 Windows 真机发布验收. 本任务不部署或重启共享 daemon, 不操作共享桌面; 编译、fixture 和协议测试不能代替真实录制.

首次正式交付包含真实鼠标/窗口事件, 自动生成的脱敏输入区间与参数意图, UIA 只读候选, 会话终结和可校验导出. 普通文本与快捷键只有满足目标绑定和安全证据时才允许升级为语义事件. 不支持的输入必须在能力和记录中明确表达, 不能悄悄丢弃并宣称完整录制.

以下事项不属于本轮实现承诺.

- 不承诺完整 Replay compiler, Replay reader 或自动回放. 不生成空脚本冒充可回放产物.
- 不把录制当作 `SetForegroundWindow` 拒绝的修复. Recorder 不调用置前, SetFocus, Invoke, SetValue, SendInput 或剪贴板写入.
- 不录制视频/音频, 不读取剪贴板, 不抓取任意 UI 文本, 不安装提权服务或驱动.
- 不支持 secure desktop, UAC 输入, 跨 Windows 登录 session 或任意 integrity level 的无条件覆盖.

## 使用方式先于内部类型

下列持久 CLI 已接线. `--target-name` 选择 daemon, REPL 的 `--target PID:HWND` 授权录制根窗口. 不能用多个独立 one-shot 进程拼出 owner 会话.

人工入口采用持久连接交互模式. 进入模式后, 先从 fresh 窗口列表选择明确授权的根窗口. 新出现的 owned dialog 只有验证 owner 链属于已授权根窗口后才纳入, 同 PID 的兄弟窗口不自动纳入.

```text
rdog record --transport zenoh --target-name windows-host.lab --namespace lab --entry-point tls/<host>:<port> --artifacts-dir ./artifacts
record> start --target <PID>:<根窗口HWND> --duration 5m
record> status
record> mark checkpoint
record> stop
```

进入模式本身不启动监听. `start` 成功才表示所声明的 required lanes 已就绪. `stop` 返回本地已校验的 `<recording_id>.rdogrec.jsonl`. CLI 输出同时展示 `replay_status=not_compiled`, 输入支持范围和 `coverage=best_effort`. 输入 EOF 或 Ctrl-C 请求 cancel, 不把用户在控制台输入的字符记成目标应用操作.

agent 必须复用同一可信持久连接. start 之后的 mark/stop/cancel 必须包含 `recording_id`, owner 的 status 带该 id 才续租. request id 只关联响应. Zenoh 使用录制专用 logical bridge, proof 在 ingress 校验, 不接受 status JSON 的 `owner_nonce` 作为认证. 无 owner 的裸 query 只能读取无内容摘要.

三个领域调用点如下. 这是接口说明, 不是新增 SDK 承诺.

1. 连接入口创建服务端 `RecordingOwner` 并传入 dispatcher, 不从请求 JSON 构造它.
2. handler 向单一 Session worker 发送 `Start/Mark/Stop/Cancel/OwnerClosed`, 不自行写 Journal.
3. CLI 的 stop exchange 收到 artifact 和 metadata 后, 核对录制 id, 字节数和 SHA-256, 原子保存后才返回成功.

## 当前代码实际具备什么

下表把实施前缺口与当前代码对应. 状态列只描述源码接线, 真机通过与否另行记录.

| 实施前缺口 | 当前代码与改变 | 当前状态 | 最后复核 |
| --- | --- | --- | --- |
| Windows 空 Stub 返回成功 | `mod.rs::platform_capture`, `windows/runtime.rs`, `stub.rs` | Windows owned helper; 其他平台 capture unavailable | 2026-11-05 |
| macOS 只有权限检查和空 worker | 旧 `macos.rs` 和关联假成功测试已删除 | 不再宣称 macOS 录制可用 | 2026-11-05 |
| raw text/key 可直接入队 | `model.rs::CapturedInput`, `windows/hooks.rs`, `windows/uia.rs` | 有限安全字段, unknown 输入区间, 无 UIA 文本 | 2026-11-05 |
| 无持续 drain, stop 丢尾部 | `control_handler.rs::Worker`, `session.rs`, `windows/runtime.rs` | 单写者持续消费; stop 等待期间仍 drain | 2026-11-05 |
| request id 充当 owner | `owner.rs`, `src/shell.rs`, `src/control_core.rs`, `src/zenoh_control/daemon_bridge.rs` | 真实连接/受保护 bridge owner; 匿名 mutation 拒绝 | 2026-11-05 |
| 空 flow 和假 delivered | 旧 Bundle-only 生产路径已删除; `journal.rs`, `delivery.rs`, `cli.rs` | 只导出 v2 JSONL; sent_unconfirmed 与 local_verified 分开 | 2026-11-05 |
| terminal 晚于导出 | `journal.rs::prepare_seal` | terminal, flush/sync/close, 验证与 digest 均先于提交 | 2026-11-05 |
| HWND 查询冒充语义 | `windows/uia.rs`, `windows/hooks.rs::IdentityEvidence` | 只读 control type/bounds 候选; 不当作回放定位器 | 2026-11-05 |
| 路由与文件权限未保护 | `src/zenoh_runtime/session.rs`, `private_files.rs`, `delivery.rs` | TLS-only 显式链路, 创建时私有 DACL/Unix mode, 实际帧/摘要校验 | 2026-11-05 |

以上录制内部路径以 `src/control_recording/` 为根. `RecordingHandler` 只是共享命令入口, `Worker` 独占活动 Session、timer、writer 和终态, `RecorderCapture` 负责安全事件来源. 文件封存、服务端发送和客户端校验是三个不同事实. 没有真实 Replay compiler 生产接线.

## 方案比较与综合决定

两份独立候选均比较了进程内线程和 owned helper 两种整体结构, 最后都选择单 helper. 候选使用配置中的同一 `openai-codex/gpt-6.1-terra:medium` 模型, 不属于跨模型族共识. 当前批准模型及 cross-judge pool 均在 `openai-codex` 家族内, 因此未进行要求不同家族的 arena cross-judge. 最终取舍由主 agent 根据源码和约束作出.

| 结构 | 隐藏的复杂性 | 暴露的风险 | 决定 |
| --- | --- | --- | --- |
| daemon 内 hook thread 和 UIA MTA | 不需进程 IPC, API 较小 | 卡住的 COM 调用无法通过放弃 JoinHandle 实现真实回收 | 不作为包含 UIA 的发布方案 |
| daemon 管 Session/Journal, 单个 owned helper 管 hook/UIA | 隐藏 Job, 私有 pipe, COM 与线程退出 | 一个 helper 故障会终止本次录制 | 采用 |
| 分开 capture helper 与 UIA helper | UIA 故障可不结束物理采集 | 双子进程, 双 IPC 和额外恢复状态 | 当前不建, 除非之后明确需要语义故障不中断录制 |

采用候选 B 的职责分配和只读 UIA 白名单, 合入候选 A 对 frozen barrier, typed delivery 和编码帧检查的细化. 不采用两份候选中的纯鼠标自动采集加人工标记所有键盘操作的范围. 那不足以满足用户所要的类似 Terminator 的动作录制. 本方案保留自动 keyboard-to-redaction/parameter 路径, 并将安全语义升级作为明确实施步骤.

产物选择单一 recording-only JSONL. 不选择 recording-only Bundle 新 major, 因为当前唯一 canonical 文件是 Journal, 新 manifest/TAR 契约不能增加首轮用户能力. 不在现行 `rdog.recording.bundle.v1` 内悄悄删 `flow.json`. 该版本既有含义仍是含真实 Replay Script 的 Bundle, 见 `specs/rdog-recording-bundle-schema.md:97-103`.

## 数据模型与模块责任

先确定四类领域对象. 公共控制接口不暴露 COM 指针, pipe, Job handle 或原始 Windows hook struct.

- `RecordingOwner` 来自服务端连接上下文. `RecordingId` 来自 daemon. 两者共同授权一次 mutation.
- `CaptureEvent` 表示允许进入队列的事件事实, 含 source sequence、source 单调时间和安全 payload, 定义在 `model.rs`.
- 实际安全事件类型为 `CapturedInput`, 定义在 `src/control_recording/model.rs`. 它保存 pointer/window/display 事实、redaction 区间及只读 semantic candidate/missing, 不含 raw key/text. VerifiedShortcut、VerifiedCommittedText 和 Replay Parameter descriptor 尚未实现, 不声明为当前能力.
- `SealedRecording` 表示已关闭、校验并原子提交的 Journal. 交付事实由 metadata 的 sent_unconfirmed 与客户端成功写入后的 local_verified 区分, 不反写录制终态.

具体接口以 `control_handler.rs::RecordingHandler`、`mod.rs::RecorderCapture`、`journal.rs::JournalWriter` 与 `delivery.rs::save_recording` 为准. 不另建通用 RecordingManager 或 SDK facade.

安全模型中没有通用 raw key/text 数据位, IPC 反序列化拒绝未知字段. raw keyboard struct 只在 callback 内判断输入来源和输入区间, 不序列化、不进入普通队列或日志. 跨回调不保留 key history、文本缓存或 suppressed count. generic 路径不重建 Unicode. 未来 VerifiedCommittedText 只能由目标绑定的受支持 adapter 创建, 不能由普通 String 或 IsPassword=false 构造.

首阶段 identity 是 Windows PID、process creation time、HWND 与 recording-local generation. desktop 是运行时门禁, 不是持久化的跨登录 session 身份凭据. HWND/PID 是录制期线索, 不是未来重放的永久身份. root 销毁使录制失败, owned dialog 销毁使旧 generation token 失效.

一个 daemon 只有一个 Session worker. 它独占 writer, lifecycle state, timer 和终态缓存. command dispatcher 只做已解析命令投递和结果返回, 不在全局 handler mutex 内等待 UIA 或文件 I/O. helper 只采集与分类, 不写正式 Journal. 这样 timer, stop, 断线与物理事件不会分别修改同一份录制状态.

## Windows 捕获与语义关联

### helper 是短命子进程, 不是新服务

使用同一 rdog 二进制的内部 helper 入口, 每次录制一个 owned child. daemon 在启动子进程时绑定 Job Object, 只继承必要 pipe handles. 采用私有匿名 pipe, 限额且有版本的内部消息. stdin EOF/父进程退出触发清理, Job kill-on-close 防止 daemon 异常退出后遗留捕获进程. 创建, 绑定和恢复执行的顺序必须避免 child 在 Job 接管前开始监听.

helper 内分专用 hook/message thread 和无窗口的 UIA MTA thread. hook callback 只进行轻量来源/范围判断, 安全分类和非阻塞有界入队, 然后调用 CallNextHookEx. 不在 callback 查询 UIA, 不读取磁盘或等待 pipe.

只使用观察接口. UIA 只读查询卡住时, 终止 helper 使本任务客户端资源可回收. 这不等于证明远端 provider 调用已被取消. 禁止增加 UIA mutation 来改善录制命中或恢复焦点.

### mouse-down 保存事实, UIA 保存候选

首轮保存原始 down/up、move 和 wheel 事实, 不合成 click、double-click 或 drag 成功事件. 缺少任一端时保留未配对事实, 不补造另一端. 队列和总产物预算有界, overflow 明确失败, 不通过丢弃尾部假装完成.

mouse-down 同时发起异步 UIA 查询. 请求持有本次 helper 中的 IdentityEvidence, 从已授权 root 限定查找, 不做 desktop-wide 遍历. dispatch 与接纳都核对同一 liveness/generation、root owner 和 `for_sequence`; candidate 的 `observed_ns` 表示真实观察时刻, 不是点击前证明. 超过 cutoff 或 token 失效只记 missing, 同 PID 不替代子树验证.

实际 UIA 候选只包含 numeric control type 与物理 bounds. 不请求 Value、Name、title、document text、AutomationId 或 provider 字符串. pattern 集和结构路径尚不在首阶段实现中. selector draft 与 role 即使未来加入, 也不是已验证可回放定位器.

### Windows 坐标不照抄 macOS 语义

helper 使用 per-monitor DPI awareness. `DisplayContext` 保存数值型原生显示几何和拓扑 generation, `WindowContext` 保存原生窗口 rect 与 DPI; 真相源为 `model.rs`、`windows/displays.rs` 和 `windows/hooks.rs`. 显示目录是独立观察, 不是每个输入瞬间的原子拓扑证明. 本版不提供 normalized 可执行坐标. 原有逻辑到原生转换仍由 `src/control_mouse/win_backend.rs` 负责, 不在文档另抄公式.

多屏负坐标、混合缩放和窗口跨屏期间的 topology 变化仍需真机验证. `coordinate_space=native_physical_screen` 不代表提供了可直接执行的 os-logical 坐标, 不能以 scale=1 猜测. 鼠标后端本轮只为统一 source marker 调整绝对注入边界, 不改变既有 logical→native 换算.

### 输入来源和采集完整性

自注入过滤先于公开 capture sequence. keyboard/paste 的 enigo Settings 与所有 Windows SendInput 鼠标路径使用 `mod.rs::SELF_EVENT_MARKER`. 绝对移动保留既有 logical→native 转换, 仅在 Win32 注入边界编码到虚拟桌面, 不再经无 marker 的 SetCursorPos. marker 不是认证凭据; injected flag 也不是人类物理输入的证明. 真实来源对照仍属真机验收项.

queue overflow, IPC 断裂和 writer 错误进入明确失败路径. callback 需要独立的 sticky fault channel 或保留控制容量, 不能依靠已经满的事件队列通知溢出. 只有隐私允许的事件才分配可公开 capture sequence, 不让 sensitive/unknown 的逐键数通过 gap 或序号旁路泄漏.

`coverage=best_effort` 是 Windows low-level hook 的已知边界. helper heartbeat 只证明部分活性, 不证明 hook 未被静默移除或从未漏事件. 已知 required lane 丢失使 Session failed. 无已知缺口也不得显示 lossless 或 complete coverage. 自动重装 hook 不能抹去缺口.

## 隐私边界与真实能力分级

默认策略是参数化文本. 键盘观察用于识别输入区间, 不能先把 raw key/text 放进队列再等待 UIA 分类. 首轮支持范围必须在 start response 和 artifact 首行声明.

| 输入或数据 | 首轮处理 | 能力升级条件 |
| --- | --- | --- |
| 普通, 密码或未知字段的字符输入 | 自动合并为目标绑定的 unknown redaction/input segment, 不保存真实值, 不把 unresolved 区间编译成 TypeText | 仅受支持 adapter 证明 ordinary 和最终 committed text 时可产 VerifiedCommittedText |
| 安全证据不全的快捷键 | 仍属于 redaction segment, 不保存键码/修饰键或推测动作 | 只有 ordinary 区间及可靠目标/快捷意图证据齐全时产 VerifiedShortcut |
| paste/IME | 不读剪贴板, 不保存候选词或组合按键, 首阶段仍为 unresolved input segment | paste 只记录有证据的意图; IME adapter 只接受最终 commit, 不读旧字段猜增量 |
| UIA 和窗口文本 | 默认不抓取 Name, Value, title, document text, AutomationId 字符串或其 hash | 独立安全 adapter 的有限字段白名单, 不能仅以它不是 Value 为由放行 |
| screenshot/AX evidence | 首轮拒绝显式 evidence 请求并给出 unsupported reason, 默认不采集 | 之后具有敏感区间整份抑制与 artifact allowlist 才可开放 |

参数区间按已验证 target/cause 变化划分, 不按字符, 键数或任意静默时长切分. 未能确定文字还是快捷键时标 unknown, 不假造某种文本内容. 首个未分类输入可以打开一个 segment, 后续键不产生逐键 marker 或递增公开计数. 没有 target 时保留 unresolved descriptor, 不为未来回放猜找目标. mark 是显式 checkpoint/parameter boundary, 不授权读取屏幕文字或放宽脱敏.

unknown 输入不等于已确认的文本输入. 只有确认文本意图后才生成 text parameter descriptor. 无法区分文字与快捷键的区间保留 unresolved input, 不替它生成未来 TypeText. 内部 callback 活性序号不能复用为公开 capture_seq 或 freeze boundary, 也不能通过 gap/count 暴露被抑制的逐键数.

`IsPassword=false`, 旧焦点缓存, 按键 Unicode 或单次 AXValue 都不足以把输入升为 ordinary committed text. 安全 adapter 必须记录可验证的 target identity, provider/控件支持条件, 输入区间边界, commit 语义和 freshness. 晚到的 UIA 结果不能追认之前 raw 输入安全. 不支持的 provider 默认参数化, 能力明确 `text_mode=parameter_intent_only` 和 `shortcut_mode=unsupported_without_evidence`.

adapter 的唯一文本入口是受支持 provider/control 提供的目标绑定 commit 通知, 在任何普通队列或 IPC 之前完成安全判断. 先取文本再异步查 IsPassword, 从整个 Value 算差分或重解释旧 raw keys 均不属于该入口. 未声明 adapter 支持时没有该生产分支. 普通正例允许其已授权 committed text, forbidden token 集合按隐私测试 case 定义.

自动参数化, UIA 候选和真实鼠标/窗口记录共同构成最小动作录制. 通用逐字复现和所有应用快捷键识别不属于首轮保证. 这些限制必须直接展示给 agent, 不能只藏在文档. 支持普通文本的 adapter 是同一录制项目的后续可验收单元, 不需要先建设完整回放.

坐标, 时间, 应用身份和操作意图仍可能敏感. 只在明确授权范围内启动, 默认私有工作目录和限权文件. 不跨 owner 返回内容. 禁止 payload logging, Debug 输出文本, 记录值长度/hash/prefix 或把异常原文带进错误回执. 用户手工提交的 checkpoint 也采用有限枚举, 不接受任意 label 当作隐私绕过渠道.

## 生命周期, owner 和停止屏障

录制态采用 Starting, Recording, Finalizing, Recorded, Failed 和 Cancelled. `Recorded` 只表示 recording-only Journal 已封存原子提交, 与 replay 无关. 不用现有 completed/空 flow 表示该状态. Required lanes 的健康来自实际启动与事件, 不预填 Available.

owner 校验先于 mutation. 同时校验 recording id. 裸 TCP/WS 没有现成身份认证, 首阶段只给实际 loopback peer 录制 owner, 信任本机使用者, 不承诺本机多用户或浏览器 origin 隔离. 非 loopback mutation 明确拒绝. Zenoh owner 来自认证且 TLS-only 的受保护 logical bridge, 不由客户端 session_id/request_id 直接构造; 共享凭据不等于多租户隔离.

TCP/WS owner 随服务端连接对象创建和释放. Zenoh session-open 为本 bridge generation 生成随机 proof, 客户端仅内存保存并在 bridge envelope 中回传. ingress 验证 generation/proof 后传服务端 RecordingOwner, recording JSON 只含业务 id. close/lease 使授权失效, 新 bridge 不复用旧 proof. 普通 status、Debug、日志不暴露它. 录制专用 router/client 在 `src/zenoh_runtime/session.rs` 关闭 discovery、autoconnect 和隐式 listener, 只保留显式 TLS 链路. 若混入 unixpipe/明文或认证未启用, 不授予录制 bridge.

Zenoh 需要验证消息确实绑定该活 bridge 及认证上下文. 若已有 routing ID 无法提供绑定, 实施项必须补齐服务端 owner/session 关联, 不能仅换一个变量名. 无法安全完成前, 该入口必须显式 unavailable, 但不能将此状态算作最终 canonical Zenoh 验收通过. 同 owner 的 status 自动续租, lease 和检查以服务端单调时钟为准. socket close, bridge close 或 lease expiry 进入 failed, 不是用户主动 cancelled. 只承诺已观察断线或 lease 到期后终结, 不承诺瞬间检测物理断网.

首版资源策略不是性能测量. duration/lease/产物预算集中于 `session.rs::RecordingPolicy`, helper/stop/UIA 时限以 `windows/runtime.rs` 的常量为准. start 返回实际会话 policy, CLI 消费返回的续租周期, 不自行猜测.

| 策略 | 首版默认 | 行为 |
| --- | --- | --- |
| 录制时限 | 5 分钟 | 可请求更短 duration, 更长请求明确拒绝, 不静默截短 |
| owner 续租 | 每 15 秒发一次 status, 45 秒无有效续租过期 | CLI 在用户不输入 REPL 时也自动续租; active recording 的 bridge 受此 lease 驱动, 不另受旧 idle timer 提前关闭 |
| helper 启动 | 5 秒 | required lanes 未 ready 则 start 失败并回收 child |
| UIA 查询 | 250 毫秒软截止, 2 秒硬截止 | 软截止标 missing, 不重复发同一请求; 持续占用至硬截止则终结 helper 并失败 |
| stop/cancel 回收 | 2 秒协作退出, 再给 3 秒终止与 reap | 不把请求 kill 当退出证明; 未确认退出则 cleanup_pending 并拒绝新 start, 直到本任务 helper 已退出 |

短期查询超时不是取消远端 provider 的证明. 本版不自动重启 helper 或恢复录制. `cleanup_pending` 属于资源回收状态, 不是第二条录制成功路径.

正常停止由单写者执行以下顺序.

1. 验权后进入 Finalizing, 关闭 helper 的统一接纳点. keyboard/mouse hook 与 required window producer 都卸载/停止后确认各自 boundary, 由统一接纳序列形成 FrozenCaptureBoundary. 不能只用最后一个键鼠事件代表所有 producer 已停止, 也不能由 timer 线程猜 end sequence.
2. Finalizing 仍持续读取已接纳的队列和 pipe 到该 boundary. 单独关闭 enrichment 接纳并标记未决 UIA missing, 不改变既有 target. terminal 后的旧 epoch 消息一律不得追加. writer 错误立刻失败.
3. 请求 helper 退出并确认进程/Job 已结束. 超过 policy 截止则终止 Job 并等待退出. 本版 required UIA runtime 触及硬截止时整次录制失败, 不增加物理采集继续运行的恢复分支. 强杀不等于成功排空, 未拿到可靠 boundary 时必须失败.
4. 在 staging Journal 写入 terminal, flush/sync/close. `journal.rs::prepare_seal` 校验 envelope、终态和字节数, 计算 SHA-256 后, 在同目录用 hard link 原子无覆盖提交. 不支持硬链接的文件系统明确失败, 不回退到会覆盖目标的 rename. 提交后的 staging unlink 失败单独报告, 不撤销已 Recorded 的文件事实.
5. 释放采集 active slot 并保留有界终态/待交付 slot. worker 生成有界文件帧, transport exchange 才负责实际网络发送. 显式 stop 最多发起一次交付, 不把构造 SaveFile 叫客户端已收到.

start deadline, auto-stop, cancel, owner_closed 和 stop 竞争由同一 worker 串行裁决. auto-stop 到时主动冻结, 不等待下一条 status. 用户 cancel 与 pre-commit failure 回收 helper 并清理本次未提交 staging. 清理失败必须报告残留, 不删除其他会话或未知目录. 原子提交后不因断线删已提交文件. 不实现断线认领, 自动重试投递, 续录或 crash replay.

auto-stop 和 limit-stop 只封存并保留同一活 owner 的待交付产物, 不主动推送无 request id 的文件或 Response. CLI 的后台 status 循环观察到 Recorded 后发出一次显式 stop exchange, agent 也采用该流程. 每次录制最多一个交付尝试, 重复 stop 只返回相同摘要和交付状态, 不再次发文件. 尚未尝试交付的结果占用一个有界 result slot, 新 start 返回 RESULT_PENDING, 直到该 owner 发起交付或关闭会话. owner 关闭后释放待交付授权, 保留已提交本地文件, 不允许新连接认领.

同一活 owner 对已知 terminal 的重复请求返回相同摘要, 不再次采集或编译. request timeout/结果未知不触发客户端自动重发. Recorded artifact 在 daemon 重启后仍是文件事实, 但本轮不承诺跨重启远程重领接口. 下次启动只处理能证明属于 recorder 的 staging, 未知或历史目录不自动删除.

## recording-only 导出契约

正式产物是单个 `<recording_id>.rdogrec.jsonl`. 不新建另一套 raw event dump. 一次 clean cutover 到 `rdog.recording.v2` 和 `rdog.record-control.v2`, 保留原 envelope 结构与顶层 event family, 明确新的安全输入/坐标/recorded 语义. 首行仍为 `session_start`, 通过 payload 声明 recording-only, Windows backend, 支持字段, 隐私模式, native coordinate space 和 `replay_status=not_compiled`. 不插入另一种独立 header.

新增 payload subtype, `recorded` terminal 与控制协议在实施单元 1 一起冻结并迁移全部生产调用者, tests 和文档. 未识别 major 明确失败, 不添加 v1/v2 兼容出口. capture runtime 只有 Recorded 一种成功终态, 删除旧 Completed 假生产路径, 而不是长期并存两个同义成功态. 原 v1 Replay Bundle 的 flow 必需契约不被重定义, 其未实现 compiler 不成为本次依赖.

当前 stop 的 placeholder flow, 假 compiler provenance 和读文件失败返回空数据路径全部删除. 没有真实消费者的 Bundle-only 生产路径一并退出 recording 主流程, 不保留第二种成功出口. 是否删除 `bundle.rs` 模块以实施时引用结果为准, 不留为假设性未来使用的 dead code. 未来真实 compiler 直接消费 sealed Journal, 不反向修改录制原件.

首轮只复用单 SaveFile 和 response, 默认整个 Journal 上限为 4 MiB, 包括首行, 控制记录, 最终 lanes/redaction closures 和 terminal. 上限集中在 RecordingPolicy, 并从实际 transport/codec 限额校验可发送性. helper/IPC 对所有可入队事件设最大序列化长度和容量, 预留量必须覆盖 freeze 生效前全部在途事件以及终态开销. UIA 候选也受独立总量预算限制. 事件预算不足时主动 freeze, 不丢弃 boundary 内已接纳事件以凑限额. 若完整 drain/terminal 最终不能装入总预算则 failed, 不截断.

发送前仍用真实 serializer 检查完整编码帧能否通过实际通道上限, 不以 raw 文件大小估算替代. 上限来源为现有 transport/codec 的唯一定义, 不照搬 Bundle 上限. limit trigger 表示录制因预算而结束, 不谎称用户主动停止. 不增加分片协议.

服务端只报告 `sent_unconfirmed`, 不把构造 SaveFile 叫 delivered. 客户端要求 artifact 帧存在, recording id/文件名匹配, 字节数与 SHA-256 一致, 无多份冲突文件. 写本地同目录临时文件, flush/close, 再做不覆盖既有目标的原子提交. 完成后才返回 `local_verified`. digest 证明字节一致, 不是签名或目标应用可信性的证明. 缺帧, 超限, 损坏, 断线或磁盘错误都不能因随后收到成功 response 而被忽略.

## 代码与文档对应关系

下表是改动责任面. 录制内部 API 已 clean cutover, 未再保留旧 Bundle-only 成功出口. `src/control_frames.rs` 复用现有 SaveFile, `src/control_protocol.rs` 复用既有分流; 其内部 recording parser/调用测试随 v2 迁移, 不为改文件数而重复改造它们.

| 修改区域 | 文件 | 必须完成的改变 |
| --- | --- | --- |
| 采集模型和平台 factory | `src/control_recording/mod.rs`, `model.rs`, `stub.rs`; 删除旧 macOS 空 worker | 安全事件, Windows 真 backend, unsupported 诚实返回 |
| Windows helper | 新 `src/control_recording/windows/` 私有实现, `src/main.rs` | Job/pipe, hook, MTA, scope, source filtering, privacy 和 freeze boundary; 不建常驻服务 |
| Journal 与 Session | `journal.rs`, `session.rs`, `control_handler.rs` | 持续 drain, 单写者, capture_seq/gap, redaction/parameter, 错误传播, auto-stop, 终态与 slot 释放 |
| 控制入口和 owner | `src/daemon.rs`, `src/shell.rs`, `src/control_core.rs`, `src/control_session.rs`, `src/zenoh_control.rs`, `src/zenoh_control/daemon_bridge.rs` | 真实连接/bridge owner 上下文与关闭通知, 消除 request-id owner 和匿名 mutation |
| 协议与 CLI | `protocol.rs`, `src/control_protocol.rs`, `cli.rs`, `src/input.rs`, `src/main.rs` | recording id, scope 与有限 mark, 持久模式, 明确一条 session 内的 agent 用法, 去掉误导性的 one-shot 组合 |
| 导出与接收 | 删除旧 `bundle.rs`; 调整 `journal.rs`, `delivery.rs`, `src/control_invocation.rs`; 复用 `src/control_frames.rs` | 单 Journal 封存, 无空 flow, 实际帧限额, metadata/digest 对账, 原子接收和真实 delivery 状态 |
| Windows 身份/坐标/注入 | `src/control_window/windows.rs`, `src/win_dpi.rs`, `src/control_mouse/win_backend.rs` 及 Windows keyboard 注入调用者 | 复用/补齐 process-window-desktop identity, 统一转换与注入 marker, 不复制公式 |
| 能力, 测试与文档 | `src/control_capabilities.rs`, 相关 recording tests, `tests/recording_e2e.rs`, 新 Windows acceptance; 现有 lifecycle/journal/redaction/bundle/acceptance specs, `CONTEXT.md`, README | available 与 planned 分开, Windows 不再套用 macOS replay 验收 skip, 同波切换数据和用户契约 |

## 实施步骤与退出条件

这些单元是依赖顺序, 不是只交付其中一半就宣布支持 Windows. 新类型按第一单元确定, 不跨阶段保留 placeholder 生产路径.

| 单元 | 实施内容 | 可观察退出条件 |
| --- | --- | --- |
| 1. 固定安全数据与真实会话契约 | sanitized domain types, v2 Journal/control cutover, owner context, RecordingPolicy 默认值, recording-only artifact, capability unavailable 语义 | request id 相同的两个连接不能互改; 非 owner 不影响 timer; 不支持平台不会 start 假成功; 无 Completed/Recorded 双成功路径 |
| 2. 接通 Windows capture 与持久消费 | owned helper, mouse/window/keyboard 分类, source marker, 单写者 drain, seq 与 sticky fault | 隔离 Windows scratch app 的真实操作进入 Journal; known gap 和写失败不假成功; helper crash 后 daemon 仍响应 |
| 3. 接通自动输入意图与 UIA | 自动 redaction/input-intent segment, root ownership, UIA 时序候选; 按支持条件加入 shortcut/committed-text adapter | 可观察的键盘输入无需逐步人工 mark; unknown/sensitive 无内容或逐键痕迹; 未支持 adapter 如实 unknown, 不宣称已有 Replay Parameter |
| 4. 贯通停止与可信导出 | freeze/drain/terminal/无覆盖提交, 持久 CLI 自动续租和一次 stop exchange, TCP/WS/Zenoh owner close/lease, 在途预算与实际帧/digest 校验 | 无人工命令仍续租; 到时独立封存而不误推 Response; 尾部/terminal 在收到文件中一致; 缺帧/坏 hash/超限/磁盘错误失败 |
| 5. 完成真机发布验收 | 以下矩阵, 受影响 macOS 契约回归, 文档与载体守卫 | 所有声明 available 的入口与输入能力都有真机证据; 不以 fixture 模拟或测试数量替代实际录制 |

若单元 3 的普通文本 adapter 无法证明安全和 commit, 不升级 text capability, 不靠 raw key 重建文本. 最小发布仍可提供输入意图录制, 但它不等于可回放的参数化脚本. 该限制必须在交付说明中显式报告, 不声称已经等同 Terminator 的逐字录制. 不因为这个限制去建设 replay 服务或放宽隐私.

## 真机验收条件

### 环境和操作边界

使用专用测试账户/虚拟机/明确授权的交互桌面. 测试目录, 端口, daemon identity, PID, helper Job 和 scratch app 都有本任务归属. 新启动服务前核对端口, 不重启或替换共享 daemon. Recorder 自身不置前或输入. 正向录制操作由操作者或受控测试驱动完成, 测试合成输入与人类物理输入分开标记.

未切换的独立 desktop 可验证部分 hook/UIA/资源回收行为, 不能替代输入 desktop 上的真实操作验收. 建 fixture 前确认线程/process 的真实 desktop identity. 只比较测试前后两次 foreground HWND 不足以证明期间没有干扰. 共享桌面只能做已授权的只读核对, 不用于输入/锁屏/UAC故障注入.

### 验收矩阵

| 类别 | 必须观察到的行为 | 不能替代它的证据 |
| --- | --- | --- |
| 正向录制 | 自建双窗口 app 执行 click, double click, drag, wheel, move/resize, 自动输入区间, mark 和 stop; 收到的 Journal 顺序/target/终态与操作对应 | 仅有 start 成功, event_count 非零或 TAR 存在 |
| UIA 与窗口归属 | 同 PID 兄弟/跨应用遮挡窗口被排除; owned dialog 仅验证授权链后纳入; root 销毁重建, stop 后或旧 helper 迟到结果不追加; 同根控件替换结果不被冒充 pre-action target | 仅 PID 相同, API 返回成功或 tree 非空 |
| 隐私 | 普通/密码/unknown/IME/粘贴按 case 定义 forbidden token; 队列/IPC/Journal/日志/Debug/导出无 forbidden 值或逐键痕迹. 相同 target/cause 与公开操作下输入 1/10/100 次, 含 autorepeat/filtered injection/queue pressure, public seq/segment/gap/status/boundary 不编码 suppressed count; UIA 文本属性注入 token 不落盘 | 只搜索最终文件, 只测 Password=true, 或把普通 adapter 正例文本也误判为泄漏 |
| 真实输入能力 | 至少一种真实普通控件和一种真实应用验证自动 unknown 输入区间; 声明支持的 shortcut/committed adapter 必须分别验证输入与结果, 不支持则准确报告 | 人工 mark 一个 shortcut 后声称自动录到了它, 或只有输入区间就声称已有可回放 Parameter |
| 资源与失败 | UIA 卡住, helper crash, queue/pipe 满, writer 磁盘错, stop/cancel/timeout 竞争有可查询终态; stop 前在途 mouse-up/window event 出现一次, stop 后新 callback 不进入; 资源确认退出后 slot 才可重用 | 超时返回, 丢弃线程句柄或杀客户端后宣称远端 provider 已取消 |
| owner 和自动停止 | TCP, WS, canonical Zenoh 测 request-id 碰撞, 非 owner, close, 断网/lease; REPL 无人工命令仍自动续租; 到时独立封存, 后续一次 stop 交付不混淆其他请求响应 | 只通过 status poll 触发停止, 或只测 TCP 后宣称所有入口可用 |
| 坐标与来源 | 100/150/200% 以及混合 DPI/负坐标/跨屏, 核对 native point 与 UIA bounds; 物理输入/rdog注入/外部注入有真实对照 | 把 scale=1 fallback 或 injected flag 当完整来源认证 |
| 导出 | frozen terminal 已在文件内; 缺帧, 超帧, 坏 hash, 本地写失败和断线均无 LocalVerified; 原子提交不会覆盖既有文件 | 只有 server response 或服务端文件 digest |
| 平台与权限 | 无桌面, 锁屏/桌面变化, 拒绝访问和不支持平台准确 unavailable/failed, 无隐式提权/重试 | 将 skip, 非交互 desktop 成功或 macOS 空 worker 当 Windows 支持证明 |

Windows acceptance runner 必须显式启用并在缺环境时失败说明原因, 不能静默跳过仍报告全绿. 普通 CI 的无真机结果只证明类型/模块/契约回归. 本任务没有性能指标承诺, 不复制 Terminator 宣传的成功率或速度. 不把外部输入系统的 best-effort 边界改写为绝对无遗漏保证.

## 原则如何改变本方案

- Model the Domain 与 Boundary Discipline 使 owner, sanitized event, sealed artifact 和 delivery 成为不同类型, 而不是用多个同步布尔值或到处补过滤条件.
- Separate Before Serializing Shared State 使 helper 只采集, Session 单写 Journal. 候选各写独立文件, 主 agent 独占最终文档.
- Laziness Protocol 使首轮只导出 canonical JSONL, 不新增常驻服务, 分片传输或空 replay 层.
- Exhaust the Design Space 使进程内与 owned helper 两种整体结构被明确比较, 没有因为第一种写起来方便就定案.
- Prove It Works 使 release gate 要求真实 Windows 产物和故障行为, 不接受 start 成功, 测试数量或源码阅读充当录制验收.

## 依据与未完成验证

代码证据以本文当前事实表中的 `src/` 路径为准. 既有领域规则参见 `CONTEXT.md`, `specs/rdog-recording-session-lifecycle.md`, `specs/rdog-recording-journal-model.md`, `specs/rdog-recording-redaction-parameter-model.md` 和 `specs/rdog-recording-bundle-schema.md`. 这些文件有部分 planned 内容, 不能当实现清单.

Windows API 约束来自本轮读取的官方文档.

- [LowLevelMouseProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc) 与 [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) 规定 message loop, callback 时序与静默移除限制.
- [UI Automation threading](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading) 规定 MTA, handler 注册/移除和 apartment 生命周期约束.
- [UIA property identifiers](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids) 定义 IsPassword, RuntimeId 与 physical BoundingRectangle.
- [MSLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct) 定义 per-monitor-aware point, injected flags 和 dwExtraInfo.
- [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) 说明 out-of-context 通知的线程/消息循环和重入边界.

Terminator 参考来自用户提供的 `local://paste-1.md` 中固定版本 `73a381c` 的研究报告. 只借鉴机制, 不声称本轮运行或验证了 Terminator. 两份候选与当前架构取证在会话产物中保留, 供核对综合过程.

本任务已在授权的 Windows 交互桌面运行独立候选 daemon、受控 fixture 和真实录制客户端. 自注入 marker 的失败及修复后对照、外部合成输入和本地校验导出证据维护在 `task_plan__windows_recording_impl.md`. 现有证据只覆盖实际执行的场景, Windows 真机矩阵仍未全部通过; 不将交叉构建或局部真机结果改写成完整发布声明.

设计阶段的两位 reviewer 静态复核了会话/导出和隐私/采集边界. 实施阶段的独立复核还发现并修复了身份表锁竞争、停止时暂停消费、Windows 文件继承 ACL、canonical 时间倒退及超宽桌面编码问题. 具体复现和回归证据见实施记录. 这些均为同模型族独立审查, 不属于跨模型族共识或 Windows 功能验收.

## 首阶段产物与剩余门槛

本阶段交付安全采集到可信 JSONL 的实现候选. `rdog.recording.v2` 由 `journal.rs` 写入, canonical `monotonic_ns` 是 writer 的追加时间, `captured_monotonic_ns` 保留 source 时间. 迟到 enrichment 不会造成 canonical 时间倒退, 也不冒充较早时刻的 observation.

键盘 hook 可观察到的输入仅表示为 unknown 区间, 不区分普通文本、密码、IME commit、paste 或 shortcut 语义. 区间由显式 mark 或相关 pointer/focus 变化结束, 不按字符数或静默时间切分. 仅通过鼠标菜单触发的粘贴只保留物理操作, 不宣称识别了文本提交或输入参数. 尚无 committed-text/shortcut adapter 和通用 Replay compiler. 客户端的来源关联计划只能在用户明确确认 unknown 意图后绑定参数, 不从 raw key 重建内容.

发布前仍须在本任务独占的 Windows 交互环境完成上方矩阵. 特别包括真实 hook/UIA、同 PID sibling/owned dialog、root destroy/reuse、输入 forbidden-token 与差分计数、混合 DPI/负坐标/跨屏、来源 marker、stop 在途尾部、UIA 卡死/Job 回收及实际 DACL. 无真机结果的测试和编译只能证明其实际覆盖范围.

## 客户端来源关联回放契约

`rdog replay --inspect` 严格读取已封存的 Windows v2 Journal, 输出白名单类型事实、完整 SHA-256 和源序 segment. 单源与多源均使用 `rdog.replay-plan.v1`, 每源显式给出 digest 与完整 seq 范围, 每步绑定来源 segment 和明确意图. 不完整动作、重叠或逆序、未知字段、无法解析的来源与不匹配的运行参数必须拒绝. canonical 时间与 source capture 时间分别保留; drag 点位 offset 以纳秒传递, wheel 沿用 hook 的原始 delta.

客户端在发送任何 binding 前使用既有 `@capabilities` 校验 Windows `capabilities.replay` 的 `rdog.protected-replay-flow.v1`、受保护参数、identity 投影和 guarded pointer 声明. 缺字段、旧版本或 unsupported 一律 failclosed, 不靠旧 parser 对未知字段的偶然反应推断安全, 不自动升级. 声明只代表实现契约, 当前桌面与目标仍需动态验证.

执行时从受保护标准输入提供 per-run 目标和参数. 脱敏 unknown 只有用户明确确认文本意图后才能使用 typed `TextParameter`; 其目标是互斥的 native 控件句柄或指定 control type 的当前 UIA 焦点. 值不进入计划、argv、日志、trace、错误或持久产物. UIA helper 先核对交互 Session、Default input/thread desktop、PID/root/焦点、可写 ValuePattern 与当前 observation epoch, 再执行并在内存比较结果. 目标应用接收值是授权效果, 其他持久副本不允许.

每个指针动作绑定当前 os-logical 点, 不直接回放源 native 坐标. 派发前后核对 identity-only 窗口观察、几何、命中归属、交互桌面与前台; 短期观察使用 PID-only 投影, 不读取窗口标题或类名, 不创建 durable selector. 定位意图只验证实际物理光标位置和窗口归属. 显式 `focus_control` 若已满足则报告 `already_satisfied` 且不派发, 否则验证焦点由非目标变化到目标, 不将其冒充按钮业务成功. drag 仅在受支持控件的 selection 达到预期时通过, wheel 仅在可观察 scrollbar 向指定方向变化时通过. 不支持或无可用 postcondition 时拒绝, 派发后无法确认或连接中断为 unknown 且不自动重发.

这些 guard 紧邻派发但不与 Windows 其他进程或焦点变化原子化. 仅有全局 SendInput 时不能承诺绝不误投, 要求该保证的任务必须拒绝. 编译与本机定向测试不构成 Windows 真实回放验收; 实际候选及单/多源、重绑定、业务结果和隐私证据以实施记录为准.

独立 Windows 局部验收已对真实单源和双源各自的 seq 3..7 编排 move、focus goal 和明确确认的文本, 在新的目标 PID/root 执行后由目标应用独立比较确认. move 和文本被验证, 焦点目标已满足而源 click 未派发. 这不是整个 Journal、真实 click/drag/wheel 或完整平台矩阵通过的证据. 候选和原始证据见实施记录.

2026-11-06 补验使用任务自有 Windows WinForms 窗口中的原生 Edit, 外部 SendInput 驱动录制. 三份 `local_verified` Journal 分别选取 click 的 seq 4..6, drag 的 seq 4..8 和 wheel 的 seq 4, 经 inspect, 来源关联计划及真实 runner 执行. click 的独立选区从 10..15 变为 68..68; 同一 click 来源在另一项运行中使焦点从另一个控件转到目标控件. 两项回放都观察到 down/up, 焦点不是已满足跳过. drag 的独立选区从 10..15 变为 68..97, 并观察到 down, 移动和 up. wheel 原始 delta 为 -120, 控件收到同一 delta, scrollbar 位置从 0 变为 3. 证据见 `local://record-replay-gestures-review.md` 和 `local://record-replay-gestures-20261106/`.

该 fixture 是真实 GUI, 但不是 Notepad 或其他业务应用的普遍证明. 本次未覆盖完整 Journal, 非 120 倍数 wheel, 其他控件及应用, 混合 DPI 或全平台矩阵. 成功回执的 stderr 仍有 Zenoh `session closed` 日志, 原因未诊断.

drag 事后独立 witness 的 `focus_hwnd` 为 0. 选区结果仍达到计划, 但本次没有证明 drag 后焦点持续, 也未确定焦点状态变化的原因.

同日针对上述焦点现象在任务自有 Edit fixture 进行 baseline, drag-only, witness-only, drag+witness 及重置后的重复对照. 三次实际来源关联 drag 均 verified, 各组 QPC marker 范围内的采样均观察到目标 Edit 有焦点. 本轮未复现历史 focus 0, 不能据此排除间歇性问题或断定历史原因. foreground 与 GUI 状态并非原子读取, 最大采样间隙约 32.1 ms. 修改过的 witness 副本增加了进程身份元数据写入. 该结论仅限该单机 fixture, 证据见 `local://record-replay-drag-focus-review.md` 和 `local://record-replay-drag-focus-20261106/results.json`.

后续低扰动对照恢复原 fixture 和 witness 字节及原 `@run` 默认同步等待. 三次来源关联 drag 和两次无 drag witness 对照未观察到自然 focus 0. 上述前一轮使用 `wait:false` 和修改 witness 的阴性仅适用于该轮条件. 人工切窗校准观察到成功 GUIInfo 和 focus 0, 但不能证明历史自然 drag 的原因. 本轮最大采样间隙约 48.2ms, 读取非原子, witness 的精确进程启动及退出 QPC 未记录. 历史原因和是否产品缺陷仍未确定. 证据见 `local://record-replay-drag-focus-round2-20261106/results.json`.

