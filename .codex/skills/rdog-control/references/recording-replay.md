# Record and replay authorized operations

## Record an authorized Windows window

1. Confirm the user authorizes recording the chosen window and any owned dialogs. Recording can expose coordinates, timing, application identity, and intent even when text is redacted. Select a private artifact directory.
2. Check the target daemon with `@capabilities` and use its actual response, not the local client version, to assess support. The current Windows capability reports `unknown`; a successful start establishes that the required lanes are ready for that session. Other platforms have no capture backend. If an older or shared daemon lacks support, report the limit; do not automatically upgrade, replace, or restart it. Use a fresh window observation and independently resolve the Windows PID and HWND of the authorized root. A `pid:PID/window:INDEX` locator is not an HWND. Do not guess either value or include unrelated windows.
3. Start one persistent CLI connection. Replace the placeholders with the authorized daemon's actual configuration and window identity. Zenoh recording requires an authenticated, explicitly configured TLS-only bridge. Socket recording mutations require an actual loopback peer.

```text
rdog record --transport zenoh --target-name TARGET --namespace NAMESPACE --entry-point tls/HOST:PORT --artifacts-dir PRIVATE_DIR
record> status
record> start --target PID:HWND --duration 5m
record> status
record> mark checkpoint
record> stop
```

The prompt lines are commands inside the same `rdog record` process, not separate shell commands. The `record> ` prompt appears only when both stdin and stdout are terminals. With piped or wrapped streams, keep the process and stdin open without waiting for a banner. Send `status` in that process and read the `rdog.record-control.v2` response with `kind:"record-status"`, `phase:"idle"` and `cleanup_status:"complete"` before `start`. This proves RPC communication and the reported session state, not capture readiness. If another owner is recording, or status or cleanup is uncertain, stop; do not claim that session. After a previous terminal session ends, start a new process and check idle and cleanup again. A successful idle status does not override a later `start` rejection such as `RESULT_PENDING`.

Do not infer readiness from a PID, log line, echo or exit code. A wrapper PID may not be the native client PID. After `start`, require its `record-start` response with `phase:"recording"` before demonstration. A successful start establishes the required capture lanes; the response does not have a separate lane acknowledgement field. Windows capability `unknown` alone is not a failure.

The implementation is `src/control_recording/cli.rs` (`run` and `run_session`);
status, start and cleanup states are defined in `src/control_recording/control_handler.rs`.

`--target` may be repeated; `--window` is its alias. HWND accepts decimal or `0x` hexadecimal. `--profile semantic` is the default; `physical` is also accepted. Follow the returned policy and errors rather than assuming the requested duration is accepted. `mark parameter_boundary` is the other supported mark; arbitrary labels are not accepted. To abandon the session, use `cancel` instead of `stop`. EOF or Ctrl-C requests cancellation.

Keep the connection alive through stop or cancel. The CLI keeps the recording ID and renews the owner lease with status requests. If auto-stop seals the Journal, the CLI observes `recorded` and requests delivery. Connection loss or lease expiry fails the active session; a new connection cannot claim it. Do not blindly retry an uncertain stop because delivery is attempted at most once.

For an agent using the protocol directly, keep one trusted owner connection and correlate responses by request ID. The payloads use `@record-start:{"profile":"semantic","targets":[{"pid":PID,"hwnd":HWND}]}`, then `@record-status:{"recording_id":"ID"}`, `@record-mark:{"recording_id":"ID","mark":"checkpoint"}`, and `@record-stop:{"recording_id":"ID"}` or `@record-cancel:{"recording_id":"ID"}`. Replace placeholders with actual JSON values. Request IDs correlate replies, not ownership. Owner status with the recording ID renews the lease; read the returned policy for its interval. A standalone status without an owner can expose only a summary.

`stop` seals an `rdog.recording.v2` JSONL Journal named `<recording_id>.rdogrec.jsonl`. Server `recorded` and `delivery_status:sent_unconfirmed` do not prove local receipt. The CLI checks the received file's ID, size, and SHA-256 and saves without overwriting an existing file; only its `local_verified` output confirms that local delivery. A digest proves matching bytes, not the trustworthiness of the application or completeness of capture.

If the authorized source action is intended to replace a whole value, verify the
application's before and after state independently. Sending Ctrl+A and text is
not a universal whole-value guarantee; input sent or `local_verified` delivery
alone does not establish the intended application result. If the result is
unknown or differs from the authorized intent, stop rather than blindly retry.

The Windows backend records best-effort pointer and window facts, and limited read-only UIA candidates. Keyboard activity is an unknown redacted input interval with `text_mode=parameter_intent_only`; it does not reconstruct typed text, shortcuts, or replay parameters. Mouse-menu paste is not recognized as a text commit. The recorder does not capture screenshots or clipboard contents. Treat the Journal as sensitive and preserve its access restrictions. The start response and Journal report `replay_status=not_compiled`. Native physical coordinates and UIA candidates are not verified replay targets.

## 回放来源关联计划

`rdog replay` 支持读取本地 `rdog.recording.v2` Journal, 并执行 agent 根据一个或多个来源事实编排的计划. 它不是任意 GUI 录制的全自动 compiler. `--inspect` 输出完整文件 SHA-256、事实和规范化 segment; `source_seqs` 保留动作及穿插的相关 metadata. 每次运行必须显式选择完整 segment 范围, 包含无法配对或无法验证的动作时停止, 不省略后声称完整回放.

```text
rdog replay --inspect JOURNAL_A JOURNAL_B
rdog replay --plan PLAN --namespace NAMESPACE --target-name TARGET --entry-point tls/HOST:PORT JOURNAL_A JOURNAL_B < /dev/fd/FD
```

运行端仅使用已认证 TLS 的指定目标. `FD` 是受保护的非 TTY 管道, 不是秘密文件. 发送任何 binding 前, runner 查询远端 `@capabilities`, 要求 Windows 的 `capabilities.replay` 声明 `rdog.protected-replay-flow.v1` 及 `protected_parameter`, `identity_projection`, `guarded_pointer`; 缺失或不支持即停止, 不升级 daemon. 声明只表示契约存在, 不证明当前桌面或应用可用. 根据 inspect 的实际 digest、首尾 seq 和 segment 首 seq 创建不含输入值的计划. 同一来源的步骤必须保持源序, 不同来源由当前 agent 根据用户目标安排. 每段只选择一次, 任何范围缺口、来源变更或不明确目标都必须拒绝.

```json
{"schema":"rdog.replay-plan.v1","sources":[{"sha256":"来源A完整SHA-256","first_seq":1,"last_seq":2},{"sha256":"来源B完整SHA-256","first_seq":1,"last_seq":1}],"steps":[{"sha256":"来源A完整SHA-256","journal_seq":1,"intent":"position_pointer","parameter":"move"},{"sha256":"来源A完整SHA-256","journal_seq":2,"intent":"confirmed_text","parameter":"textA"},{"sha256":"来源B完整SHA-256","journal_seq":1,"intent":"confirmed_text","parameter":"textB"}]}
```

此例只说明两源结构, 必须按各自实际 Journal 替换完整 digest、范围、seq 和意图, 不能照抄假设. `position_pointer` 对应 move, `focus_control` 对应只要求控件获得焦点的 click, 其他指针意图为 `confirmed_click`, `confirmed_drag`, `confirmed_wheel`. `confirmed_text` 只能用于用户明确确认的 unknown 输入段, 不得从 redaction 或 UIA 候选推测文本、快捷键或粘贴.

计划只有一个权威磁盘文件, 例如 `replay-plan.json`. 写入或修改该文件后, 使用
skill 自带的 `scripts/check-replay-plan.py` 重新校验该文件和实际 Journal, 而非
以聊天中的 JSON 或口头修正代替. 最终答复只引用校验输出中的唯一 `plan_path`,
`plan_sha256` 和 stage, 不再粘贴另一份完整计划或口头修正. 后续修改使该次
结果失效, 必须重新校验.

从独立准备的 binding 模板提取名称, 写入不含值的 manifest, 例如
`{"bindings":["textA","textB"],"pointer_bindings":["move"]}`.
不要从 plan 复制名称列表作为自证. 模板本身必须是有效 JSON; manifest 通过
不证明模板有效. 纯指针时 `bindings` 必须为空数组.
元数据模板、程序化名称提取和受控内存 builder 的步骤见
`replay-binding-preparation.md`. 不从 plan 倒推模板或在工具调用代码中构造完整 runtime.

```text
python3 SKILL_DIR/scripts/check-replay-plan.py --rdog RDOG --plan replay-plan.json --parameter-names names.json JOURNAL_A JOURNAL_B
```

也可改用 `--runtime-stdin`, 从受保护非 TTY 管道读取实际 runtime JSON;
不要把 secret 写入文件或 argv. 两种模式互斥, 均只比对参数名称, 不校验
binding 结构、PID、授权或 fresh 状态. runtime 必须含对象 `bindings`,
可选对象 `pointer_bindings`; 纯指针使用 `"bindings":{}`.
检查器实时调用实际 binary 的本地 `--inspect`, 不接受缓存, 并检查计划和
来源字节、范围、顺序与意图. 只有退出码 0 且 stage 为
`plan_and_parameter_names_validated` 才表示该阶段通过; `runtime_validated`
和 `live_validated` 始终为 false. 失败时不得报告通过或派发. 检查器不会
自动修复或写第二份计划; agent 可根据实际 Journal 修正同一文件后重新校验.
来源、范围或输入语义授权不明时停止.
校验回执不是执行授权, 也不能消除检查与执行之间的竞争; 实际 runner 仍需
重新校验和 fresh 观察. 本检查复用 inspect 的 typed v2 reader 和规范化结果,
独立复制计划规则, 维护时须与 Rust runner 同步; 它不证明录制完整或可信.

通过受保护的非 TTY 管道提供运行参数. 完整 runtime 不得出现在工具调用代码、
argv、计划、日志、trace、错误或持久文件, 公开 marker 也不例外. 字段定义以
`src/control_recording/replay.rs` 的 `Runtime`, `Binding` 和 `PointerBinding` 为准;
受控 builder 的操作与授权边界见 `replay-binding-preparation.md`.

顶层 `bindings` 必填, 纯指针计划使用空对象; 纯文本计划可省略
`pointer_bindings`. 两类 binding 名称必须与计划中对应的参数一一匹配.
若两个文本 binding 指向同一控件, 后一次整体替换前一次, 不会拼接文本.

`target` 也可使用 `{"kind":"native_control","hwnd":CONTROL}`. `focused_control` 要求当前唯一 UIA 焦点的明确 control type、同 PID/root、前台、native HWND 和可写 ValuePattern. 例如 Document 的 type 为 50030, 不匹配时停止. 文本动作使用 ValuePattern.SetValue 替换控件的整个值, 不是在光标处输入或重放按键; 只有用户目标允许整体替换时才使用. 不要把根窗口 HWND 当作控件 HWND.

文本回放前, 用 fresh 目标证据确认它不是密码控件. 当前 native 文本执行路径
`src/control_ax/windows_sensitive.rs::set_and_verify` 没有内置 `IsPassword`
检查; 这是额外的使用前安全门槛. 在与目标关联的 native CUIAutomation 通道
读取 `IsPassword`, 必须 HRESULT 成功且为 false; 缺失或未知就停止. 若任务
明确提供经审的只读 native 诊断工具, 可用它观察同一目标的控件句柄、PID、
ValuePattern、IsReadOnly 和 IsPassword 的结果及 HRESULT. 对应调用必须成功,
ValuePattern 必须可用且 IsReadOnly 为 false; HRESULT 失败或结果未知就停止.
还要单独确认 root 归属、前台和焦点; 单个只读 probe 不证明焦点、SetValue 或业务结果.
没有足够的 fresh 证据就停止, 不要假设存在未提供的原生命令.

默认托管 .NET `FromHandle` 的 Pane 或无 pattern 不能单独证明 native
CUIAutomation 不支持该控件; 以本次目标及实际通道的 fresh 证据为准.
不把通道差异、只读 ValuePattern 观察或
`status` 回执当作成功 setter 或回放证据. 保持值和完整 runtime JSON 不落盘.

用相同安全连接先查询 `@window-find:{pid:PID,limit:256,include_state:true,include_recipes:false,identity_only:true}`. 只使用实际返回的 Windows `window_id`、os-logical `rect` 和可选 `focused_hwnd`, 缺失或歧义时停止. 该投影在采集时按 PID 过滤, 不读取标题或类名, 不生成 durable selector. PID/HWND 改变后重新绑定. 坐标按当前目标重新选择, 不直接复用源文件的 native 坐标.

每个 pointer binding 的 `points` 是当前 os-logical 点. move、click、wheel 各给一个 `offset_ns:0` 点; drag 点数及每个 `offset_ns` 必须与 inspect 的 segment 相同. 按明确意图提供对应验证: 定位使用 `{"kind":"position","control_hwnd":ROOT}`; `focus_control` 使用 `{"kind":"focus","control_hwnd":CONTROL}`; 标准 Edit/RichEdit 选区变化使用 `{"kind":"selection","control_hwnd":CONTROL,"start":START,"end":END}`; 可观察 Win32 scrollbar 位移使用 `{"kind":"scroll","control_hwnd":CONTROL,"vertical":true,"increase":true}`. 已聚焦时 `focus_control` 报告 `already_satisfied` 和 `not_dispatched`, 不冒充执行了源 click; 否则要求动作后目标获得焦点. 此判据不证明按钮或其他业务结果. 不支持的 verifier 和未知结果不能报告为成功.

意图和验证必须匹配: `position_pointer` 只配 `position`, `focus_control` 只配 `focus`, `confirmed_click` 和 `confirmed_drag` 只配 `selection`, `confirmed_wheel` 只配对应非零轴的 `scroll`. 不用焦点验证冒充 click 的选区或其他业务结果.

wheel 使用来源 segment 的原始 delta, 不自行乘除 120. click 或 drag 的 selection 验证只证明选区达到预期, 不保证动作后焦点持续; 后续动作依赖焦点时先 fresh 观察并确认目标, 不自动补发焦点操作.

selection 已等于目标时 click 或 drag 会在派发前拒绝. click 按住时长至多 5 秒; drag 要求 2 至 128 点, offset 非递减且末点不超过 5 秒. wheel 至少一轴非零, 各轴 delta 须可用 i16 表示.

首次 mutation 前对每个唯一 PID/root 做一次 fresh identity-only 查询, 检查全部已绑定 pointer 点和路径均在当前窗口矩形内. 每一步仍重新核对源 digest、短期 observation、目标 PID/HWND、当前几何、交互 Session/Default desktop、前台和命中归属. 后续目标变化可能导致部分动作已执行, 不自动回滚. 全局 SendInput 不能锁住其他进程或操作系统的焦点变化, 检查与派发之间仍有竞争; 要求绝对不误投时不能使用该路径. `position` 只证明物理指针位置和窗口归属, 不证明 hover 业务. 最终业务结果还需针对目标应用 fresh 验证. 超时、断线或派发后无法核验是 unknown, 不自动重发.

Windows 候选仅支持上述明确动作和 verifier, 不是全平台或所有应用支持. selection 验证仅支持标准 Edit/RichEdit 的 16-bit 索引, 超出可表达范围或无 native 控件句柄时拒绝. 不支持按键、快捷键或粘贴重建; wheel verifier 要求控件暴露 Win32 scrollbar 状态. 混合 DPI、跨屏、所有 UIA 控件和完整 Windows 矩阵不可从编译或单次局部验收推断. 普通 `@flow` 和 `@gui-bench` 仍是独立工作流和基准, 不等于 Journal 回放.
