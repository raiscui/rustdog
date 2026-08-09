# Recording redaction 研究笔记

## [2026-07-22 12:44:00] [Session ID: omx-1784512435044-92wxat] 笔记: 既有安全与回放契约

## 已验证事实

### Capture 与 Secure Input

- `specs/rdog-macos-operation-capture-research.md` 已固定: Recorder 可以查询 Secure Input 是否活跃,但只能可靠识别 secure period,不能假设观察到期间每个键盘事件或精确键数。
- Secure Input 活跃时不得保存 keycode、Unicode、modifier sequence 或剪贴板内容,也不得通过前后 UI 文本推回输入。
- `secure_input` 是运行时状态 lane,不是权限。mouse、window 和 app lifecycle 在 redaction period 内仍可继续记录。
- AX secure field 或安全属性无法可靠判断时,既有研究要求按 redaction 处理。

### Recording Journal

- `specs/rdog-recording-journal-model.md` 已固定独立 `redaction` event family,使用 enter/exit interval。
- cause 至少覆盖 `secure_input`、`secure_field` 和 `security_unknown`。
- redaction interval 不得保存 keycode、Unicode、modifier sequence、clipboard content、逐键 marker 或 suppressed count。
- redaction 活跃时 keyboard/text physical entry 不写 Journal;这不是 physical entry 写入后再覆写。
- Replay compiler 对未知 event kind 必须 fail closed,不能静默跳过。

### 现有输入控制语义

- `@type-text` 只负责普通文本输入,支持 `ax-value`、`targeted-keyboard`、显式 opt-in 的 `clipboard` 和 `auto`。
- `@paste` 只表示对当前前台焦点发送系统粘贴快捷键。它不携带 clipboard content,也不是稳定的普通文本输入接口。
- `@key` 负责快捷键、功能键和导航键,不应作为普通文本注入接口。
- `rdog.flow.v1` 当前只有 policy、options 和 steps,仓库源码与规格未发现通用 parameter、variable 或 placeholder 机制。

### 当前代码边界

- `src/control_ax.rs` 有 `AXSecureTextField` 序列化脱敏测试,证明 AX 输出模型能表达 `value_redacted:true` 且省略 value。
- 当前搜索没有发现 Recorder secure-field classifier 的生产实现。该测试不能作为"所有非 secure role 都是 ordinary"的动态证据。

## 待确认的主张

### 当前推荐

- 输入安全分类使用 `ordinary`、`sensitive`、`unknown` 三态。
- `ordinary` 必须同时满足 Secure Input 在完整输入区间内为 inactive、目标元素已明确解析、AX 分类明确为非 secure、capture provenance 完整。
- `sensitive` 包括 Secure Input active、明确 secure field、显式 secret 参数来源。
- 任一所需信号缺失、权限不足、目标/focus 歧义、区间存在相关 gap 时归入 `unknown`。
- `unknown` 与 `sensitive` 使用相同的落盘规则:只写 redaction marker 和参数描述,不写真实值。

### 最强备选解释

- 可以只把已确认的 Secure Input/secure field 归为 `sensitive`,其余一律按 ordinary 记录。这会提升文本保真,但会把 AX 不可用、Web 自绘控件、焦点竞争和 classifier 缺失误当成安全输入,不符合 map 已固定的"安全性未知只保留 redacted marker"。

### 推翻当前推荐所需的证据

- 若平台能提供一个覆盖所有文本目标、不会漏报的 authoritative secure-field API,则 `unknown` 可以缩小。
- 当前仓库、Apple API 研究和既有 Journal 契约均未提供这种证据。

## 下一决定点

- 先确认三态分类与 fail-closed 规则。
- paste、快捷键、输入法组合、parameter descriptor 和 runtime injection 在后续逐项确认,不与第一个问题混问。

## [2026-07-22 13:18:05] [Session ID: omx-1784512435044-92wxat] 笔记: 三态分类已确认

- 用户确认采用 `ordinary`、`sensitive`、`unknown` 三态分类。
- `ordinary` 需要完整输入区间内 Secure Input inactive、目标和焦点明确、平台分类器明确判定非 secure,且相关 provenance 完整。
- `sensitive` 覆盖 Secure Input active、明确 secure field 和显式 secret 声明。
- 任何安全证据缺失、权限不足、焦点歧义、目标无法分类或相关 gap 都归入 `unknown`。
- `sensitive` 和 `unknown` 采用相同持久化边界:不保存真实值、键值、modifier sequence、逐键 marker、键数或 clipboard content。
- 已把 `Ordinary Input`、`Sensitive Input`、`Unknown-Safety Input` 写入 `CONTEXT.md`。

## 下一决定点

- 确认录制到 paste 时,是依赖回放环境当前 clipboard,还是强制生成运行时参数。

## [2026-07-22 13:37:55] [Session ID: omx-1784512435044-92wxat] 笔记: paste 规则已确认

- Recorder 只记录 paste 意图和目标上下文,不读取或保存当前 clipboard content。
- recorded paste 不编译为依赖环境 clipboard 的裸 `@paste`。
- Replay Script 为 paste 生成必填文本参数占位符,运行时通过 `@type-text mode:"clipboard"` 和显式 `allow_clipboard:true` 注入。
- 参数真实值只存在于回放进程内存和实际投递路径,不得进入 Journal、manifest、Replay Script、trace、日志或 artifact。
- 缺少 paste 参数时必须在任何 Replay step 执行前失败。
- v1 只支持文本 paste;非文本 clipboard payload 不建模、不录制内容、不得猜测转换。

## 下一决定点

- 确认普通快捷键、功能键和导航键何时可以记录为动作,以及 redaction interval 内是否允许保留快捷键结构。

## [2026-07-22 20:09:38] [Session ID: omx-1784512435044-92wxat] 笔记: 快捷键规则已确认

- 用户要求继续,按上一轮推荐固定快捷键方案。
- redaction interval 外,完整且目标焦点明确的 `Cmd` / `Ctrl` 快捷键、功能键和导航键可以形成 `@key` semantic action。
- `Return`、`Escape`、`Tab`、方向键等按非文本动作处理。
- `Shift+字符`、`Option+字符` 和输入法相关按键不得直接当作快捷键;它们进入文本/组合输入判定。
- `Cmd+V` / `Ctrl+V` 使用已确认的 paste 参数规则。
- `sensitive` 或 `unknown` redaction interval 内不保存快捷键、keycode 或 modifier sequence。独立观察到的 mouse、window、app 和 AX 事件不受此限制。
- 无法区分快捷键与文本输入时 fail closed,不猜测生成 `@key`。

## 下一决定点

- 确认 IME、dead key、emoji 和 composed text 是否只保存最终提交文本,并要求什么证据才能视为 ordinary。

## [2026-07-22 20:10:42] [Session ID: omx-1784512435044-92wxat] 笔记: IME 最终提交文本规则已确认

- IME、dead key、emoji 选择和 composed text 的组合过程不是 Replay action。
- Replay Script 只使用目标绑定的语义 lane 已确认的最终 committed text。
- raw keycode、event Unicode、AXValue 单一信号、候选词选择和当前输入法状态都不能独立证明最终文本。
- ordinary 输入可以保留符合 Journal 规则的 physical evidence,但 intermediate marked text 和候选内容不生成 Replay step。
- sensitive/unknown 输入继续只使用 redaction 和参数占位符。
- Replay 不得退回 raw key replay 来模拟组合过程。

## 下一决定点

- 确认 ordinary 输入无法获得可靠 committed text 时,是否改为 required runtime parameter,而不是让整个 Replay Script 无法生成。

## [2026-07-22 20:15:13] [Session ID: omx-1784512435044-92wxat] 笔记: ordinary unverified text 参数化已确认

- ordinary 输入缺少可靠 committed text 时,Replay compiler 自动生成 required text parameter。
- Journal 仍可保留 ordinary physical evidence;语义不确定不会把安全分类改写成 `unknown`。
- Replay 不根据 keycode、输入法状态或单一 AXValue 猜测 literal text。
- parameter reason 使用 `semantic_commit_unverified`。
- 缺参时在任何 Replay step 执行前失败;提供参数后走普通 `@type-text` 投递。
- 所有 Replay Parameter 统一禁止进入 trace、错误正文和 artifact,不因参数是 ordinary 而分叉处理路径。
- 已将 `Replay Parameter` 写入 `CONTEXT.md`。

## 下一决定点

- 确认 v1 parameter descriptor 的最小字段集和单一引用身份。

## [2026-07-22 20:26:27] [Session ID: omx-1784512435044-92wxat] 笔记: parameter descriptor 已确认

- v1 descriptor 固定为 `parameter_id`、`value_type`、`classification`、`reason`、`origin_journal_seq` 五个字段。
- `parameter_id` 是 Recording Bundle 内唯一且稳定的引用身份。
- `value_type` 在 v1 只允许 `text`。
- `classification` 使用 `ordinary`、`sensitive` 或 `unknown`。
- `reason` 使用 `secure_input`、`secure_field`、`security_unknown`、`paste` 或 `semantic_commit_unverified`。
- `origin_journal_seq` 指回产生参数的 canonical Journal entry。
- v1 所有 Replay Parameter 默认 required,不重复保存 `required:true`。
- descriptor 禁止 `value`、`default`、regex、length constraint、vault reference、prompt template 和重复 target locator。

## 下一决定点

- 确认 descriptor 的单一真相源以及 Journal、manifest、Replay Script 三处的派生和引用关系。

## [2026-07-22 20:30:24] [Session ID: omx-1784512435044-92wxat] 笔记: descriptor ownership 已确认

- Recording Journal 是 canonical parameter descriptor 的唯一创建者。
- sensitive、unknown 和 paste descriptor 写入对应 `redaction` enter entry。
- ordinary committed text 无法确认时,对应 `semantic_candidate` 必须在 terminal 前记录 `parameter_required` 和完整 descriptor。
- Bundle manifest 和 Replay Script 根级 `parameters[]` 只复制 Journal descriptor。
- Replay step 只引用 `parameter_id`,不重复 classification、reason 或 origin。
- Compiler 需要参数但找不到 Journal descriptor 时返回 `PARAMETER_ORIGIN_MISSING`,不得临时生成参数。
- 该规则保证在线和离线重新编译得到相同参数集合。

## 已验证结构约束

- CodeGraph 显示当前 `FlowRequest` 只有 `schema`、`policy`、`steps`、`options`,并启用 `deny_unknown_fields`。
- 当前 `FlowStep::ControlLine` 直接持有 `String`;`Cmd.run` 和 `Script.text` 也直接持有字符串。
- 因此把 parameter placeholder 插入这些字符串会引入通用模板和额外注入面,不符合本 ticket 的简化边界。

## 下一决定点

- 确认参数化文本是否使用专用 typed flow step,而不是给任意字符串添加插值。

## [2026-07-22 20:36:32] [Session ID: omx-1784512435044-92wxat] 笔记: typed `TypeText` step 已确认

- `rdog.flow.v1` 增加专用 `TypeText` step,Recorder 生成的 literal 和 parameterized text 都使用该 step。
- `text` 是严格二选一 union: `{literal:"..."}` 或 `{parameter:"param-1"}`。
- parameter reference 必须解析到根级 `parameters[]` 中的 descriptor。
- paste 使用 `mode:"clipboard"` 与 `allow_clipboard:true`;普通文本默认使用 `mode:"auto"`。
- Runtime 将 typed step 构造成结构化 `@type-text` request,继续复用现有 control core 和 backend。
- `ControlLine`、`Cmd`、`Script`、`env`、`Expect` 和 artifact path 不支持参数引用或字符串插值。
- 现有 `ControlLine(String)` 保持兼容,但不作为 Recorder text output。

## 下一决定点

- 确认 Replay Parameter value 的运行时注入通道,避免 argv、environment 和持久化参数文件泄漏。

## [2026-07-22 20:37:28] [Session ID: omx-1784512435044-92wxat] 笔记: transport confidentiality 边界

- `README.md` 只要求暴露到不可信网络前审查 `0.0.0.0` bind endpoint。
- `specs/code-agent-rdog-control-usage.md` 明确写明控制能力"只应该在可信主机、可信网络和可信 daemon 上启用"。
- 当前规格和配置搜索没有发现 TCP、WebSocket 或 Zenoh control transport 的 TLS/authentication/confidentiality 保证。
- 因此"参数值不落盘"不能被表述为"参数值端到端保密"。
- 本轮先确认 controller-side 注入来源;transport gate 作为紧随其后的独立决定点。

## [2026-07-22 20:50:35] [Session ID: omx-1784512435044-92wxat] 笔记: runtime binding 输入源已确认

- CLI 只支持 `rdog replay BUNDLE --params-stdin`,从 stdin 读取严格 JSON object `parameter_id -> string`。
- SDK 只接受调用方传入的 in-memory map。
- v1 禁止 argv `--param id=value`、environment、`.env` / `.envrc`、持久化 params file,以及把 bindings 写回 Replay Script/manifest。
- Controller 在连接和执行前完成参数 ID、text type、缺参、重复 JSON key 和未声明参数校验。
- bindings 只附加到单次内存 execution request,所有输出只显示 parameter ID 和状态。
- 执行结束或失败后不缓存、不自动复用 bindings。

## 下一决定点

- 确认 sensitive/unknown bindings 是否只允许通过 local unixpipe 或明确声明 confidential 的 transport 发送。

## [2026-07-22 20:52:15] [Session ID: omx-1784512435044-92wxat] 笔记: transport confidentiality gate 已确认

- ordinary parameter 可以沿用当前 trusted-network control transport。
- sensitive/unknown parameter 只允许通过动态验证为 confidential 的实际 transport。
- local unixpipe 只有在实际 endpoint 为 unixpipe、FIFO/lease identity 匹配目标 daemon、FIFO owner 为当前 UID、mode 为 `0600` 时才算 confidential。
- sensitive/unknown binding 存在时,unixpipe 失败不得透明 fallback 到 UDP/TCP;每个实际 transport 都必须重新过 gate。
- 其他 transport 只有未来明确提供认证与加密并报告 `confidential:true` 后才能承载 sensitive/unknown binding。
- gate 必须在发送任何 binding 和执行任何 step 前完成;失败返回 `PARAMETER_TRANSPORT_UNSAFE`。
- 当前门禁实现前,远程 sensitive/unknown Replay 默认不可执行。

## 下一决定点

- 确认 descriptor、step reference 和 runtime bindings 的全量集合校验规则。

## [2026-07-22 21:02:35] [Session ID: omx-1784512435044-92wxat] 笔记: parameter 集合校验已确认

- 每个 `parameter_id` 在 `parameters[]` 中只能声明一次。
- 每个 descriptor 必须被且只能被一个 `TypeText` step 引用;同一个值需要输入两次时使用两个 parameter ID。
- 未声明 step reference 返回 `PARAMETER_REFERENCE_UNDECLARED`。
- 未使用 descriptor 返回 `PARAMETER_UNUSED`。
- 缺少 binding 返回 `PARAMETER_MISSING`;额外 binding 返回 `PARAMETER_UNDECLARED`。
- stdin JSON duplicate key 返回 `PARAMETER_DUPLICATE`;非 string value 返回 `PARAMETER_TYPE_MISMATCH`。
- empty string 是合法 text value,v1 不增加长度约束。
- 校验顺序为 script descriptor/reference、binding exact set、transport gate;全部通过后才允许发送 binding 和执行 step。
- 所有错误只包含 code 与 parameter ID,不包含 value。

## 下一决定点

- 确认日志、trace、response、error、metrics、artifact 和 Debug 输出的 value/metadata 审计边界。

## [2026-07-22 22:35:24] [Session ID: omx-1784512435044-92wxat] 笔记: audit 与 runtime exposure 已确认

- 允许记录 parameter ID、classification、reason、validation status、delivery mode、clipboard restored 状态、error code 和聚合 count。
- 禁止记录 value、length、hash、prefix/suffix、字符统计、serialized binding request 和 clipboard content。
- `TypeText` trace 对 literal/parameter 都只记录 source kind、可用的 parameter ID 和 `value_redacted:true`;ordinary literal 也不回显。
- parameter value 使用 redacted `Debug` 包装并在释放时 best-effort zeroize;parse error 不附带原始 stdin/request payload。
- metrics 只保存聚合数量,parameter ID 不作为 label;artifact 和 `SaveArtifact` 不得访问 binding map。
- paste 必须在 manifest/preflight 标记 `runtime_clipboard_exposure:true`,继续执行 `restore-if-unchanged`,但不承诺删除第三方 clipboard history。
- controller/daemon memory、经过门禁的 transport、目标应用和 paste clipboard 是诚实声明的运行时暴露面;系统 crash dump 和目标应用日志不属于 rdog 的持久化保证。

## 下一决定点

- 确认 parameter value 的一次性消费、失败清理和断线/timeout 不自动重试规则。

## [2026-07-22 22:39:30] [Session ID: omx-1784512435044-92wxat] 笔记: parameter lifecycle 与 no-retry 已确认

- bindings 只属于单次 Replay runtime,不得缓存或跨连接恢复。
- 每个 `TypeText` 开始时取出对应 value,step 成功或失败后立即 best-effort zeroize。
- 任一步失败时终止 Replay并清理所有尚未使用的 bindings。
- paste 失败仍执行 `restore-if-unchanged` clipboard cleanup。
- parameterized `TypeText` 不做 step-level 自动重试。
- binding 发送后出现 timeout/disconnect 返回 `PARAMETER_DELIVERY_UNCERTAIN`,不得假设输入未发生或自动重发。
- 重试只能由调用方显式启动全新 Replay invocation并重新提供全部 bindings;不继承旧执行位置或 binding map。

## 下一决定点

- 确认一个持续 redaction period 如何按 target/cause 切分为一个或多个 Replay Parameter。

## [2026-07-22 22:40:59] [Session ID: omx-1784512435044-92wxat] 笔记: redaction segment cardinality 已确认

- 一个 `cause + target identity` segment 对应一个 Replay Parameter。
- Secure Input 持续 active 但 focused target 改变时关闭旧 segment并开启新 segment。
- security cause 在 `secure_input`、`secure_field`、`security_unknown` 间变化时切分。
- 每次 paste 单独形成一个 segment;ordinary unverified text 每个 target-bound semantic input span形成一个参数。
- 不按时间、推测键数、suppressed count 或值相等性切分/合并。
- target unresolved 时仍保存 descriptor,但 compiler 返回 `PARAMETER_TARGET_UNRESOLVED`,不生成无目标 `TypeText`。
- Secure Input segment 无法证明实际输入数量时仍生成参数;empty string binding 合法。

## 下一决定点

- 确认 `parameter_id` 的 recording-local 顺序分配和稳定性规则。

## [2026-07-23 00:19:47] [Session ID: omx-1784512435044-92wxat] 笔记: parameter ID 已确认

- `parameter_id` 使用 Recording Session 内从 1 开始的 `param-N` 顺序号。
- Journal writer 在 descriptor 首次追加时分配,顺序严格跟随 descriptor entry 的 `journal_seq`。
- 已分配 ID 不复用、不重排;manifest、Replay Script 和离线重编译只复制 Journal ID。
- ID 只在当前 Recording Bundle 内唯一,不承诺跨录制稳定。
- ID 不编码 value、target、classification、reason 或 hash;`origin_journal_seq` 继续作为独立来源指针。

## 下一决定点

- 确认 runtime bindings 是否作为 `rdog.flow.v1` execution request 的临时根字段发送,而不进入保存的 Replay Script。

## [2026-07-23 15:52:06] [Session ID: omx-1784512435044-92wxat] 笔记: bindings wire placement 已确认

- 保存的 `rdog.flow.v1` Replay Script 包含 root `parameters[]`,不包含 values。
- Controller 只在单次 execution request 内临时附加 root `bindings:{parameter_id:string}`。
- Bundle writer 和 Replay Script serializer 永远禁止输出 `bindings`。
- Controller 必须在集合校验和 actual transport gate 通过后才序列化/发送 bindings。
- daemon parse 后立即把 value 移入 redacted/zeroizing wrapper。
- 不新增 `@bind`、secret session 或多帧参数协议;不提供 CLI argv inline binding。
- raw `@flow` payload 禁止进入日志、trace 和错误正文。

## 下一决定点

- 确认旧 daemon compatibility/capability preflight,禁止把 parameterized Replay 降级为 literal/string interpolation。

## [2026-07-23 16:24:00] [Session ID: omx-1784512435044-92wxat] 笔记: flow parameter capability gate 已确认

- 保存格式继续使用 `rdog.flow.v1`,不通过 schema 版本分叉表达参数能力。
- daemon 的 `@capabilities` 新增 `flow_parameters`,参数化 Replay 要求其 `status` 为 `available`。
- Recording Bundle manifest 声明 `requires:["flow_parameters"]`。
- Replay 必须先在本地验证 script,再连接 target并获取 `@capabilities`,随后验证 capability 和实际 transport confidentiality gate。
- 只有 capability 与 transport gate 均通过后,CLI 才读取 stdin bindings并校验 exact binding set。
- 旧 daemon 或缺少该 capability 的 daemon 返回 `FLOW_PARAMETERS_UNSUPPORTED`。
- 禁止降级为 literal、字符串插值、旧 `ControlLine` 或任何可能持久化 parameter value 的形式。

## 阶段结论

- 访谈决策已经覆盖输入分类、参数 schema、Replay step、runtime bindings、传输、审计和失败生命周期。
- 阶段 3 完成。下一步进入阶段 4,撰写正式规格、验证示例与图表,并完成 Wayfinder ticket 收口。

## [2026-07-23 17:12:00] [Session ID: omx-1784512435044-92wxat] 笔记: resolution 与 frontier

- 正式规格已发布到 commit `28c21dc70338f2180a5a789ec7c10ec9eeb8b063`。
- GitHub resolution comment: `https://github.com/raiscui/rustdog/issues/3#issuecomment-5055992990`。
- `定义敏感输入脱敏与 Replay 参数模型` 已关闭。
- Wayfinder map 的 Decisions so far 已追加一行 pointer,没有复制完整决策正文。
- 关闭后两个 open、unblocked、unassigned frontier 是:
  - `验证语义提升与坐标 fallback 的可行性`。
  - `定义 Recording Bundle schema 与原子导出`。
- 地图顺序中前者排在前面,应作为下一次 Wayfinder session 的默认 ticket。
- 本轮没有新增 fog 或需要创建的新 ticket。Bundle projection 和 replay preflight 已分别由现有 ticket 覆盖。
